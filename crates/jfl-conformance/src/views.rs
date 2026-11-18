//! The driver of `formal/views.qnt` (IDX-09; plan §7.1, `doc/formal-models.md`): the model's
//! stimuli applied to `jfl-application`'s [`ViewRegistry`] over a scripted world (the tree with
//! each file's version, stat and racy tick, each worktree's `HEAD` and index; one in-order watcher
//! queue numbered as the model numbers it; the summary store as a set of `{path, ver}`), and the
//! registry observed after every step.
//!
//! The world is scripted here, beside the driver, because every value of it is part of the
//! observation the replay compares: the model's stat is a number a racy write keeps, its versions
//! are what the store is keyed by, and its watcher's sequence numbers are the read stamps. The
//! registry sees only what a stat, a read or a delivered event would tell it: [`Disk`]'s
//! [`Observed`] compares the stat and presence and never the version.
//!
//! What is the driver's record and not the registry's (`doc/formal-models.md`, "Observation"):
//! each request's `history`, `sync*`, `pin*` and `reads`, and each `sync`'s `need` and
//! `needHead`. Everything else is read from the registry: each view's dirty set, rescan, watch,
//! certainty, build and published generation (its view identity included), and each request's
//! phase, binding, needed generation, cookie, pin and completeness.
//!
//! Every type of the observation, nested ones included, has `deny_unknown_fields` and a derived
//! `PartialEq` (IDX-09's obligation 9), so the comparison covers the whole model state.
//!
//! Two negative controls (plan §7.1: publishing before the last summary is stored), each
//! diverging where its fault first shows:
//!
//! - [`control`], the registered one: the registry is shown a store that lags the observed one
//!   by its last write (a write acknowledged before it can be read back), so the atomic-publish
//!   guard refuses (`NotStored`) the model's next `publish` step that names that summary. Pinned
//!   by tests/replay.rs::should_diverge_at_the_publish_when_the_views_control_store_lags.
//! - [`control_publishing_at_start`]: each build is published as soon as it starts, before its
//!   summaries are stored; it diverges at `reconcileStart` (with a path pending the guard
//!   refuses, with none the early generation differs from the model's). Pinned by
//!   tests/replay.rs::should_diverge_at_the_reconcile_when_the_views_control_publishes_early.
//!
//! Cookie events carry their cookie's sequence number as its stamp, which is where the model
//! writes it (`cookieSeq`). A rescan is given the scripted tree's listing of the view's present
//! files.

use std::collections::{BTreeMap, BTreeSet};

use jfl_application::views::{
    Cookie, Event, FileObserved, GitSeen, Observed, RequestId, RequestPhase, Stamp, Stored,
    SyncPhase, ViewRegistry, ViewState,
};
use jfl_domain::{FileUri, RelPath};
use serde::{Deserialize, Serialize};

use crate::driver::{Driver, Step, StepError};
use crate::replay::Replayer;

/// The model's worktrees: `n` is a linked worktree nested in `m` at `m/.claude/worktrees/n`.
const VIEWS: [(&str, &str); 2] = [("m", "/w/m"), ("n", "/w/m/.claude/worktrees/n")];
/// The model's files, each with its innermost worktree.
const PATHS: [(&str, &str); 3] = [("m/A", "m"), ("m/B", "m"), ("n/A", "n")];
/// The model's requests.
const REQS: [(&str, u64); 2] = [("r1", 1), ("r2", 2)];
/// The event `m`'s watcher raises for a change of its `HEAD` or index; `n`'s live outside it.
const M_GIT: &str = "m/.git";

/// A file, on the scripted disk and in an index: its version (the content, which no stat sees),
/// presence, stat (a racy write keeps it) and whether it was written within the current tick.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Disk {
    /// The content's version.
    pub ver: i64,
    /// Whether it exists.
    pub present: bool,
    /// What a stat sees.
    pub stat: i64,
    /// Written within the current mtime tick (in an index: the summary is racy).
    pub recent: bool,
}

impl Observed for Disk {
    fn proves_unchanged(&self, now: &Self) -> bool {
        !self.recent && self.stat == now.stat && self.present == now.present
    }
}

impl FileObserved for Disk {
    fn present(&self) -> bool {
        self.present
    }
}

/// `HEAD` or the index, as the model numbers its moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitVer(i64);

impl Observed for GitVer {
    fn proves_unchanged(&self, now: &Self) -> bool {
        self == now
    }
}

/// A stored summary: the model's store holds `{path, ver}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    /// The file.
    pub path: String,
    /// Its version.
    pub ver: i64,
}

/// A view's files, by the model's path.
pub type Index = BTreeMap<String, Disk>;

/// The scripted disk.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Tree {
    /// Every file.
    pub files: Index,
    /// Each worktree's `HEAD`.
    pub heads: BTreeMap<String, i64>,
    /// Each worktree's index.
    pub git_indexes: BTreeMap<String, i64>,
}

/// One queued watcher event: a file's or `m/.git`'s (`path`), or a request's cookie (`req`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WatchEvent {
    /// Its place in the delivery order.
    pub seq: i64,
    /// The view whose watch raised it.
    pub view: String,
    /// The file or `m/.git`; `""` for a cookie.
    pub path: String,
    /// The cookie's request; `""` otherwise.
    pub req: String,
}

/// The scripted watcher's queue.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Watcher {
    /// The queued events.
    pub events: BTreeSet<WatchEvent>,
    /// The next sequence number.
    pub next_seq: i64,
}

/// A published generation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GenerationObs {
    /// Its number.
    #[serde(rename = "gen")]
    pub generation: i64,
    /// `HEAD` it was built at.
    pub head: i64,
    /// The index it was built at.
    pub git_index: i64,
    /// Its files.
    pub index: Index,
    /// Each summary's read stamp; -1 for the opening scan.
    pub read_at: BTreeMap<String, i64>,
}

/// The build in progress, or the model's idle build.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BuildObs {
    /// Whether one is in progress.
    pub active: bool,
    /// `HEAD` it records.
    pub head: i64,
    /// The index it records.
    pub git_index: i64,
    /// Its files.
    pub index: Index,
    /// Each summary's read stamp.
    pub read_at: BTreeMap<String, i64>,
    /// The paths it has yet to summarise.
    pub pending: BTreeSet<String>,
    /// Whether its publish certifies the view.
    pub certifies: bool,
    /// The `sync` it serves, or -1.
    pub sync_epoch: i64,
}

/// One view.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewObs {
    /// Delivered file events no build has taken.
    pub dirty: BTreeSet<String>,
    /// A full reconcile is due.
    pub rescan: bool,
    /// The watch is up.
    pub watched: bool,
    /// No loss since a full scan that started with the watch up.
    pub certain: bool,
    /// The build.
    pub build: BuildObs,
    /// The published generation.
    pub published: GenerationObs,
    /// The generation the last publish replaced.
    pub prev_gen: i64,
}

/// One value a request read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadObs {
    /// The view it read from.
    pub view: String,
    /// The generation it read from.
    #[serde(rename = "gen")]
    pub generation: i64,
    /// The file.
    pub path: String,
    /// The version it got.
    pub ver: i64,
}

/// One request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RequestObs {
    /// `idle`, `waiting`, `pinned` or `done`.
    pub phase: String,
    /// Its file.
    pub path: String,
    /// The view it was bound to.
    pub view: String,
    /// The first generation it may pin.
    pub need_gen: i64,
    /// `none`, `queued`, `delivered` or `lost`.
    pub cookie: String,
    /// Its latest cookie's place on the clock.
    pub cookie_seq: i64,
    /// Cookies written.
    pub cookies: i64,
    /// The driver's record: every state of the view's files from the sync point to the pin.
    pub history: BTreeSet<Index>,
    /// The driver's record: the disk at the sync point.
    pub sync_disk: Index,
    /// The driver's record: `HEAD` at the sync point.
    pub sync_head: i64,
    /// The driver's record: the index at the sync point.
    pub sync_git_index: i64,
    /// The pinned generation.
    #[serde(rename = "gen")]
    pub generation: i64,
    /// Its files.
    pub snap: Index,
    /// Its `HEAD`.
    pub snap_head: i64,
    /// Its index.
    pub snap_git_index: i64,
    /// The driver's record: the disk at the pin.
    pub pin_disk: Index,
    /// The driver's record: `HEAD` at the pin.
    pub pin_head: i64,
    /// The driver's record: the index at the pin.
    pub pin_git_index: i64,
    /// `complete`.
    pub complete: bool,
    /// The view completeness was computed for.
    pub complete_view: String,
    /// The generation completeness was computed at.
    pub complete_gen: i64,
    /// The driver's record: what it read.
    pub reads: BTreeSet<ReadObs>,
}

/// One view's `sync`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SyncObs {
    /// `idle`, `waiting` or `done`.
    pub phase: String,
    /// Syncs started.
    pub epoch: i64,
    /// A rescan started after it has published.
    pub scanned: bool,
    /// The driver's record: the disk at `syncStart`.
    pub need: Index,
    /// The driver's record: `HEAD` at `syncStart`.
    pub need_head: i64,
}

/// The views model's six variables, as the driver observes them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewsState {
    /// The scripted disk.
    pub tree: Tree,
    /// The scripted watcher.
    pub watcher: Watcher,
    /// The registry's views.
    pub views: BTreeMap<String, ViewObs>,
    /// The summaries stored.
    pub store: BTreeSet<Summary>,
    /// The requests.
    pub requests: BTreeMap<String, RequestObs>,
    /// The syncs.
    pub syncs: BTreeMap<String, SyncObs>,
}

/// The driver's records of one request.
#[derive(Debug, Clone, Default)]
struct Records {
    history: BTreeSet<Index>,
    sync_disk: Index,
    sync_head: i64,
    sync_git_index: i64,
    pin_disk: Index,
    pin_head: i64,
    pin_git_index: i64,
    reads: BTreeSet<ReadObs>,
}

/// The scripted store.
#[derive(Debug, Clone, Default)]
struct Store(BTreeSet<Summary>);

impl Stored<Summary> for Store {
    fn holds(&self, key: &Summary) -> bool {
        self.0.contains(key)
    }
}

/// What the registry is shown of the store: the store itself, or, in the lagging-store control,
/// the store without its last write (an acknowledged write not yet readable).
struct StoreView<'a> {
    store: &'a Store,
    hidden: Option<&'a Summary>,
}

impl Stored<Summary> for StoreView<'_> {
    fn holds(&self, key: &Summary) -> bool {
        self.store.holds(key) && self.hidden != Some(key)
    }
}

/// The driver's faults: none for the driver, one per negative control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    /// The registry sees the store lag the observed one by its last write.
    LaggingStore,
    /// Each build is published as soon as it starts.
    PublishAtStart,
}

type Registry = ViewRegistry<Disk, GitVer, Summary>;

/// Drives a [`ViewRegistry`] by `views.qnt`'s actions.
#[derive(Debug, Clone)]
pub struct ViewsDriver {
    registry: Registry,
    tree: Tree,
    watcher: Watcher,
    watching: BTreeSet<String>,
    store: Store,
    records: BTreeMap<String, Records>,
    needs: BTreeMap<String, (Index, i64)>,
    last_put: Option<Summary>,
    fault: Fault,
}

fn refused(e: impl ToString) -> StepError {
    StepError::Refused(e.to_string())
}

fn harness(step: &Step, why: &str) -> StepError {
    StepError::Harness(format!("step {}: `{}`: {why}", step.index(), step.action()))
}

fn int(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn root_of(view: &str) -> Result<FileUri, String> {
    let path = VIEWS
        .iter()
        .find(|(v, _)| *v == view)
        .map(|(_, p)| *p)
        .ok_or_else(|| format!("no view `{view}`"))?;
    FileUri::from_path(path).map_err(|e| e.to_string())
}

fn view_name(root: &FileUri) -> String {
    VIEWS
        .iter()
        .find(|(_, p)| *p == root.path())
        .map(|(v, _)| (*v).to_owned())
        .unwrap_or_default()
}

fn innermost(path: &str) -> Option<&'static str> {
    PATHS.iter().find(|(p, _)| *p == path).map(|(_, v)| *v)
}

/// The model's path `view/x` as an absolute file.
fn file_of(path: &str) -> Result<FileUri, String> {
    let (view, rest) = path
        .split_once('/')
        .ok_or_else(|| format!("no view in `{path}`"))?;
    let root = root_of(view)?;
    let rel = RelPath::new(rest).map_err(|e| e.to_string())?;
    root.join(&rel).map_err(|e| e.to_string())
}

/// The model's path of `rel` in the view of `root`.
fn model_path(root: &FileUri, rel: &RelPath) -> String {
    format!("{}/{}", view_name(root), rel.as_str())
}

fn rel_of(path: &str) -> Result<RelPath, String> {
    let (_, rest) = path
        .split_once('/')
        .ok_or_else(|| format!("no view in `{path}`"))?;
    RelPath::new(rest).map_err(|e| e.to_string())
}

fn stamp(seq: i64) -> Stamp {
    Stamp(u64::try_from(seq).unwrap_or(0))
}

fn read_at(at: Option<Stamp>) -> i64 {
    at.map_or(-1, |s| int(s.0))
}

impl ViewsDriver {
    /// The driver over the registry, with `fault` (a negative control) or none.
    fn new(fault: Fault) -> Self {
        Self {
            registry: Registry::new(),
            tree: Tree {
                files: Index::new(),
                heads: BTreeMap::new(),
                git_indexes: BTreeMap::new(),
            },
            watcher: Watcher {
                events: BTreeSet::new(),
                next_seq: 0,
            },
            watching: BTreeSet::new(),
            store: Store::default(),
            records: BTreeMap::new(),
            needs: BTreeMap::new(),
            last_put: None,
            fault,
        }
    }

    fn file(&self, path: &str) -> Result<Disk, String> {
        self.tree
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("no file `{path}`"))
    }

    fn git(&self, view: &str) -> GitSeen<GitVer> {
        GitSeen {
            head: GitVer(self.tree.heads.get(view).copied().unwrap_or_default()),
            index: GitVer(self.tree.git_indexes.get(view).copied().unwrap_or_default()),
        }
    }

    fn restrict(&self, view: &str) -> Index {
        self.tree
            .files
            .iter()
            .filter(|(p, _)| innermost(p) == Some(view))
            .map(|(p, d)| (p.clone(), d.clone()))
            .collect()
    }

    /// Queues the events of one stimulus in `view`'s watcher, after every queued one, while its
    /// watch is up: each at `nextSeq` plus its rank, and `nextSeq` moves on by 4.
    fn raise(&mut self, view: &str, paths: &[&str]) {
        if !self.watching.contains(view) {
            return;
        }
        let rank = |p: &str| match p {
            "m/A" => 0,
            "m/B" => 1,
            "n/A" => 2,
            _ => 3,
        };
        for path in paths {
            self.watcher.events.insert(WatchEvent {
                seq: self.watcher.next_seq.saturating_add(rank(path)),
                view: view.to_owned(),
                path: (*path).to_owned(),
                req: String::new(),
            });
        }
        self.watcher.next_seq = self.watcher.next_seq.saturating_add(4);
    }

    fn queue_cookie(&mut self, view: &str, req: &str) {
        self.watcher.events.insert(WatchEvent {
            seq: self.watcher.next_seq,
            view: view.to_owned(),
            path: String::new(),
            req: req.to_owned(),
        });
        self.watcher.next_seq = self.watcher.next_seq.saturating_add(1);
    }

    /// Each waiting request records its view's files after a disk change.
    fn record_history(&mut self) {
        for (name, id) in REQS {
            let waiting = self
                .registry
                .request(RequestId(id))
                .is_some_and(|r| r.phase == RequestPhase::Waiting);
            if !waiting {
                continue;
            }
            let view = self
                .registry
                .request(RequestId(id))
                .map(|r| view_name(&r.view))
                .unwrap_or_default();
            let files = self.restrict(&view);
            self.records
                .entry(name.to_owned())
                .or_default()
                .history
                .insert(files);
        }
    }

    fn init(&mut self) -> Result<(), String> {
        *self = Self::new(self.fault);
        let one = Disk {
            ver: 1,
            present: true,
            stat: 1,
            recent: false,
        };
        for (path, _) in PATHS {
            self.tree.files.insert(path.to_owned(), one.clone());
            self.store.0.insert(Summary {
                path: path.to_owned(),
                ver: 1,
            });
        }
        for (view, _) in VIEWS {
            self.tree.heads.insert(view.to_owned(), 1);
            self.tree.git_indexes.insert(view.to_owned(), 1);
            self.watching.insert(view.to_owned());
            self.needs.insert(view.to_owned(), (Index::new(), 0));
            let mut files = Vec::new();
            for (path, owner) in PATHS {
                if owner == view {
                    let key = Summary {
                        path: path.to_owned(),
                        ver: 1,
                    };
                    files.push((rel_of(path)?, one.clone(), key));
                }
            }
            self.registry
                .open(root_of(view)?, true, self.git(view), files)
                .map_err(|e| e.to_string())?;
        }
        for (name, _) in REQS {
            self.records.insert(name.to_owned(), Records::default());
        }
        Ok(())
    }

    fn write(&mut self, path: &str, racy: bool, present: bool) -> Result<(), String> {
        let f = self.file(path)?;
        let ver = f.ver.saturating_add(1);
        let stat = if racy { f.stat } else { ver };
        self.tree.files.insert(
            path.to_owned(),
            Disk {
                ver,
                present,
                stat,
                recent: true,
            },
        );
        let view = innermost(path).ok_or_else(|| format!("no file `{path}`"))?;
        self.raise(view, &[path]);
        self.record_history();
        Ok(())
    }

    fn checkout(&mut self, view: &str) -> Result<(), String> {
        let mut raised = Vec::new();
        for (path, owner) in PATHS {
            if owner == view {
                let f = self.file(path)?;
                let ver = f.ver.saturating_add(1);
                self.tree.files.insert(
                    path.to_owned(),
                    Disk {
                        ver,
                        present: true,
                        stat: ver,
                        recent: true,
                    },
                );
                raised.push(path);
            }
        }
        if view == "m" {
            raised.push(M_GIT);
        }
        self.bump(view, true, true);
        self.raise(view, &raised);
        self.record_history();
        Ok(())
    }

    fn bump(&mut self, view: &str, head: bool, index: bool) {
        if head && let Some(h) = self.tree.heads.get_mut(view) {
            *h = h.saturating_add(1);
        }
        if index && let Some(i) = self.tree.git_indexes.get_mut(view) {
            *i = i.saturating_add(1);
        }
    }

    fn git_moved(&mut self, view: &str, head: bool) {
        self.bump(view, head, !head);
        if view == "m" {
            self.raise(view, &[M_GIT]);
        }
    }

    fn deliver(&mut self, keep: bool) -> Result<(), StepError> {
        let Some(first) = self.watcher.events.iter().min_by_key(|e| e.seq).cloned() else {
            return Err(refused("no event to deliver"));
        };
        self.watcher.events.remove(&first);
        if keep {
            self.watcher.events.insert(WatchEvent {
                seq: self.watcher.next_seq,
                ..first.clone()
            });
            self.watcher.next_seq = self.watcher.next_seq.saturating_add(1);
        }
        let root = root_of(&first.view).map_err(StepError::Harness)?;
        let event = if let Some((_, id)) = REQS.iter().find(|(r, _)| *r == first.req) {
            Event::Cookie {
                id: RequestId(*id),
                at: stamp(first.seq),
            }
        } else if first.path == M_GIT {
            Event::Git { view: root }
        } else {
            Event::File {
                view: root,
                path: rel_of(&first.path).map_err(StepError::Harness)?,
            }
        };
        self.registry.deliver(event).map_err(refused)
    }

    fn drop_event(&mut self, seq: i64) -> Result<(), StepError> {
        let lost: Vec<WatchEvent> = self
            .watcher
            .events
            .iter()
            .filter(|e| e.seq == seq)
            .cloned()
            .collect();
        if lost.is_empty() {
            return Err(refused("no event with that sequence number"));
        }
        for event in &lost {
            self.watcher.events.remove(event);
        }
        self.registry.overflow();
        for event in &lost {
            if let Some((_, id)) = REQS.iter().find(|(r, _)| *r == event.req) {
                self.registry.cookie_lost(RequestId(*id)).map_err(refused)?;
            }
        }
        Ok(())
    }

    fn reconcile_start(&mut self, view: &str) -> Result<(), StepError> {
        let root = root_of(view).map_err(StepError::Harness)?;
        let files = self.tree.files.clone();
        let git = self.git(view);
        let listed: BTreeSet<RelPath> = files
            .iter()
            .filter(|(p, d)| innermost(p) == Some(view) && d.present)
            .filter_map(|(p, _)| rel_of(p).ok())
            .collect();
        self.registry
            .reconcile_start(
                &root,
                Some(&listed),
                |rel| {
                    files.get(&model_path(&root, rel)).cloned().unwrap_or(Disk {
                        ver: 0,
                        present: false,
                        stat: 0,
                        recent: false,
                    })
                },
                || git,
            )
            .map_err(refused)?;
        if self.fault == Fault::PublishAtStart {
            self.registry.publish(&root, &self.store).map_err(refused)?;
        }
        Ok(())
    }

    fn summarise(&mut self, view: &str, path: &str) -> Result<(), StepError> {
        let root = root_of(view).map_err(StepError::Harness)?;
        let disk = self.file(path).map_err(StepError::Harness)?;
        let rel = file_of(path)
            .ok()
            .and_then(|f| f.relative_to(&root))
            .ok_or_else(|| refused("the path is not under the view"))?;
        let key = Summary {
            path: path.to_owned(),
            ver: disk.ver,
        };
        let read_at = stamp(self.watcher.next_seq);
        self.store.0.insert(key.clone());
        self.last_put = Some(key.clone());
        self.registry
            .summarised(&root, &rel, disk, key, read_at)
            .map_err(refused)
    }

    fn request_start(&mut self, req: &str, id: RequestId, path: &str) -> Result<(), StepError> {
        let file = file_of(path).map_err(StepError::Harness)?;
        let view = self
            .registry
            .bind(&file)
            .map(|(root, _)| view_name(root))
            .ok_or_else(|| refused("the path is under no view"))?;
        let disk = self.file(path).map_err(StepError::Harness)?;
        let git = self.git(&view);
        let started = self
            .registry
            .request_start(id, &file, &disk, &git, stamp(self.watcher.next_seq))
            .map_err(refused)?;
        let view = view_name(&started.view);
        if started.write_cookie {
            self.queue_cookie(&view, req);
        }
        let files = self.restrict(&view);
        self.records.insert(
            req.to_owned(),
            Records {
                history: BTreeSet::from([files.clone()]),
                sync_disk: files,
                sync_head: self.tree.heads.get(&view).copied().unwrap_or_default(),
                sync_git_index: self
                    .tree
                    .git_indexes
                    .get(&view)
                    .copied()
                    .unwrap_or_default(),
                ..Records::default()
            },
        );
        Ok(())
    }

    fn request_view(&self, id: RequestId) -> Result<String, StepError> {
        self.registry
            .request(id)
            .map(|r| view_name(&r.view))
            .ok_or_else(|| refused("the request has not started"))
    }

    fn request_pin(&mut self, req: &str, id: RequestId) -> Result<(), StepError> {
        let view = self.request_view(id)?;
        let git = self.git(&view);
        self.registry.request_pin(id, &git).map_err(refused)?;
        let pin_disk = self.restrict(&view);
        let (pin_head, pin_git_index) = (git.head.0, git.index.0);
        let records = self.records.entry(req.to_owned()).or_default();
        records.pin_disk = pin_disk;
        records.pin_head = pin_head;
        records.pin_git_index = pin_git_index;
        Ok(())
    }

    fn read(&mut self, req: &str, id: RequestId, path: &str) -> Result<(), StepError> {
        let request = self
            .registry
            .request(id)
            .ok_or_else(|| refused("the request has not started"))?;
        let file = file_of(path).map_err(StepError::Harness)?;
        let rel = file
            .relative_to(&request.view)
            .ok_or_else(|| refused("the path is not under the request's view"))?;
        let entry = self.registry.read(id, &rel).map_err(refused)?;
        let generation = request
            .pin
            .as_ref()
            .map_or(0, |p| int(p.generation.generation.0));
        let read = ReadObs {
            view: view_name(&request.view),
            generation,
            path: model_path(&request.view, &rel),
            ver: entry.seen.ver,
        };
        self.records
            .entry(req.to_owned())
            .or_default()
            .reads
            .insert(read);
        Ok(())
    }

    fn apply(&mut self, step: &Step) -> Result<(), StepError> {
        let path = || -> Result<String, StepError> {
            step.pick::<String>("path")?
                .ok_or_else(|| harness(step, "no `path` pick"))
        };
        let view = || -> Result<String, StepError> {
            step.pick::<String>("view")?
                .ok_or_else(|| harness(step, "no `view` pick"))
        };
        let req = || -> Result<(String, RequestId), StepError> {
            let name = step
                .pick::<String>("req")?
                .ok_or_else(|| harness(step, "no `req` pick"))?;
            let id = REQS
                .iter()
                .find(|(r, _)| *r == name)
                .map(|(_, id)| RequestId(*id))
                .ok_or_else(|| harness(step, "an unknown request"))?;
            Ok((name, id))
        };
        let flag = |name: &str| -> Result<bool, StepError> {
            Ok(step.pick::<bool>(name)?.unwrap_or(false))
        };
        match step.action() {
            "init" => self.init().map_err(StepError::Harness),
            "write" => self
                .write(&path()?, flag("racy")?, true)
                .map_err(StepError::Harness),
            "delete" => self
                .write(&path()?, false, false)
                .map_err(StepError::Harness),
            "tick" => {
                for file in self.tree.files.values_mut() {
                    file.recent = false;
                }
                Ok(())
            }
            "checkout" => self.checkout(&view()?).map_err(StepError::Harness),
            "moveHead" => {
                self.git_moved(&view()?, true);
                Ok(())
            }
            "stage" => {
                self.git_moved(&view()?, false);
                Ok(())
            }
            "deliver" => self.deliver(flag("keep")?),
            "dropEvent" => {
                let seq = step
                    .pick::<i64>("seq")?
                    .ok_or_else(|| harness(step, "no `seq` pick"))?;
                self.drop_event(seq)
            }
            "watchLost" => {
                let view = view()?;
                self.watching.remove(&view);
                let root = root_of(&view).map_err(StepError::Harness)?;
                self.registry.watch_lost(&root).map_err(refused)
            }
            "watchRestored" => {
                let view = view()?;
                self.watching.insert(view.clone());
                let root = root_of(&view).map_err(StepError::Harness)?;
                self.registry.watch_restored(&root).map_err(refused)
            }
            "reconcileStart" => self.reconcile_start(&view()?),
            "summarise" => self.summarise(&view()?, &path()?),
            "publish" => {
                let root = root_of(&view()?).map_err(StepError::Harness)?;
                let shown = StoreView {
                    store: &self.store,
                    hidden: match self.fault {
                        Fault::LaggingStore => self.last_put.as_ref(),
                        Fault::None | Fault::PublishAtStart => None,
                    },
                };
                self.registry
                    .publish(&root, &shown)
                    .map(|_| ())
                    .map_err(refused)
            }
            "requestStart" => {
                let (name, id) = req()?;
                self.request_start(&name, id, &path()?)
            }
            "requestRecookie" => {
                let (name, id) = req()?;
                let root = self
                    .registry
                    .request_recookie(id, stamp(self.watcher.next_seq))
                    .map_err(refused)?;
                self.queue_cookie(&view_name(&root), &name);
                Ok(())
            }
            "requestPin" => {
                let (name, id) = req()?;
                self.request_pin(&name, id)
            }
            "read" => {
                let (name, id) = req()?;
                self.read(&name, id, &path()?)
            }
            "finish" => {
                let (_, id) = req()?;
                self.registry.finish(id).map_err(refused)
            }
            "syncStart" => {
                let view = view()?;
                let root = root_of(&view).map_err(StepError::Harness)?;
                self.registry.sync_start(&root).map_err(refused)?;
                let need = (
                    self.restrict(&view),
                    self.tree.heads.get(&view).copied().unwrap_or_default(),
                );
                self.needs.insert(view, need);
                Ok(())
            }
            "syncDone" => {
                let root = root_of(&view()?).map_err(StepError::Harness)?;
                self.registry.sync_done(&root).map_err(refused)
            }
            _ => Err(StepError::unknown(step)),
        }
    }

    fn index_obs(
        root: &FileUri,
        index: &BTreeMap<RelPath, jfl_application::Entry<Disk, Summary>>,
    ) -> (Index, BTreeMap<String, i64>) {
        let files = index
            .iter()
            .map(|(rel, e)| (model_path(root, rel), e.seen.clone()))
            .collect();
        let read = index
            .iter()
            .map(|(rel, e)| (model_path(root, rel), read_at(e.read_at)))
            .collect();
        (files, read)
    }

    fn view_obs(root: &FileUri, view: &ViewState<Disk, GitVer, Summary>) -> ViewObs {
        let (index, read) = Self::index_obs(root, &view.published.index);
        let build = match &view.build {
            Some(b) => {
                let (index, read_at) = Self::index_obs(root, &b.index);
                BuildObs {
                    active: true,
                    head: b.git.head.0,
                    git_index: b.git.index.0,
                    index,
                    read_at,
                    pending: b.pending.iter().map(|p| model_path(root, p)).collect(),
                    certifies: b.certifies,
                    sync_epoch: b.serves_sync.map_or(-1, int),
                }
            }
            None => BuildObs {
                active: false,
                head: 0,
                git_index: 0,
                index: Index::new(),
                read_at: BTreeMap::new(),
                pending: BTreeSet::new(),
                certifies: false,
                sync_epoch: -1,
            },
        };
        ViewObs {
            dirty: view.dirty.iter().map(|p| model_path(root, p)).collect(),
            rescan: view.rescan,
            watched: view.watched,
            certain: view.certain,
            build,
            published: GenerationObs {
                generation: int(view.published.generation.0),
                head: view.published.git.head.0,
                git_index: view.published.git.index.0,
                index,
                read_at: read,
            },
            prev_gen: int(view.replaced.0),
        }
    }

    fn request_obs(&self, name: &str, id: RequestId) -> RequestObs {
        let records = self.records.get(name).cloned().unwrap_or_default();
        let Some(r) = self.registry.request(id) else {
            return RequestObs {
                phase: "idle".to_owned(),
                path: String::new(),
                view: String::new(),
                need_gen: 0,
                cookie: "none".to_owned(),
                cookie_seq: -1,
                cookies: 0,
                history: records.history,
                sync_disk: records.sync_disk,
                sync_head: records.sync_head,
                sync_git_index: records.sync_git_index,
                generation: 0,
                snap: Index::new(),
                snap_head: 0,
                snap_git_index: 0,
                pin_disk: records.pin_disk,
                pin_head: records.pin_head,
                pin_git_index: records.pin_git_index,
                complete: false,
                complete_view: String::new(),
                complete_gen: 0,
                reads: records.reads,
            };
        };
        let phase = match r.phase {
            RequestPhase::Waiting => "waiting",
            RequestPhase::Pinned => "pinned",
            RequestPhase::Done => "done",
        };
        let cookie = match r.cookie {
            Cookie::None => "none",
            Cookie::Queued => "queued",
            Cookie::Delivered => "delivered",
            Cookie::Lost => "lost",
        };
        let (generation, snap, snap_head, snap_git_index, complete, complete_view, complete_gen) =
            match &r.pin {
                Some(pin) => (
                    int(pin.generation.generation.0),
                    Self::index_obs(&r.view, &pin.generation.index).0,
                    pin.generation.git.head.0,
                    pin.generation.git.index.0,
                    pin.completeness.is_complete(),
                    view_name(&pin.complete_view),
                    pin.completeness.generation().map_or(0, |g| int(g.0)),
                ),
                None => (0, Index::new(), 0, 0, false, String::new(), 0),
            };
        RequestObs {
            phase: phase.to_owned(),
            path: model_path(&r.view, &r.path),
            view: view_name(&r.view),
            need_gen: int(r.need.0),
            cookie: cookie.to_owned(),
            cookie_seq: int(r.cookie_at.0),
            cookies: i64::from(r.cookies),
            history: records.history,
            sync_disk: records.sync_disk,
            sync_head: records.sync_head,
            sync_git_index: records.sync_git_index,
            generation,
            snap,
            snap_head,
            snap_git_index,
            pin_disk: records.pin_disk,
            pin_head: records.pin_head,
            pin_git_index: records.pin_git_index,
            complete,
            complete_view,
            complete_gen,
            reads: records.reads,
        }
    }
}

impl Driver for ViewsDriver {
    type State = ViewsState;

    fn step(&mut self, step: &Step) -> Result<(), StepError> {
        self.apply(step)
    }

    fn observe(&self) -> ViewsState {
        let mut views = BTreeMap::new();
        let mut syncs = BTreeMap::new();
        for (name, _) in VIEWS {
            let Ok(root) = root_of(name) else { continue };
            let Some(view) = self.registry.view(&root) else {
                continue;
            };
            views.insert(name.to_owned(), Self::view_obs(&root, view));
            let (need, need_head) = self.needs.get(name).cloned().unwrap_or_default();
            let phase = match view.sync.phase {
                SyncPhase::Idle => "idle",
                SyncPhase::Waiting => "waiting",
                SyncPhase::Done => "done",
            };
            syncs.insert(
                name.to_owned(),
                SyncObs {
                    phase: phase.to_owned(),
                    epoch: int(view.sync.epoch),
                    scanned: view.sync.scanned,
                    need,
                    need_head,
                },
            );
        }
        let requests = if views.is_empty() {
            BTreeMap::new()
        } else {
            REQS.iter()
                .map(|(name, id)| ((*name).to_owned(), self.request_obs(name, RequestId(*id))))
                .collect()
        };
        ViewsState {
            tree: self.tree.clone(),
            watcher: self.watcher.clone(),
            views,
            store: self.store.0.clone(),
            requests,
            syncs,
        }
    }
}

/// The driver over the registry.
pub fn driver() -> Replayer {
    Replayer::new(ViewsDriver::new(Fault::None))
}

/// The registered negative control (plan §7.1: publishing before the last summary is stored):
/// the registry is shown a store that lags the observed one by its last write, so a publish
/// naming that summary is refused (`NotStored`) at the model's `publish` step.
pub fn control() -> Replayer {
    Replayer::new(ViewsDriver::new(Fault::LaggingStore))
}

/// The second control: each build is published as soon as it starts, before its summaries are
/// stored; it diverges at `reconcileStart`.
pub fn control_publishing_at_start() -> Replayer {
    Replayer::new(ViewsDriver::new(Fault::PublishAtStart))
}
