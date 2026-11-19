//! The navigation queries over a pinned generation (IDX-10; plan §2.1, §2.2, §2.7; ADR 0014):
//! definition, hover, document symbols and workspace symbols.
//!
//! **One generation per answer.** Every query reads only the generation its request pinned
//! ([`crate::Answer`]): the summaries in it, and the text of a file only while its bytes are the
//! ones the generation's key names ([`Sources::text`]), since positions in the carrier's encoding
//! and Javadoc come from the text, never from a newer file. A file whose bytes moved on since is
//! left out of the answer, which says so (`ModuleStale`).
//!
//! **Completeness starts from the pin** (the request's own view: `Pending` files, gaps, an
//! unwatched view, pending changes) and each query only adds to it:
//! - `TargetUnresolved` when syntax could not prove the binding (resolution rules in
//!   [`resolve`]), `TargetNotIndexed` when every place the name could be declared in the view was
//!   searched and none declares it, `ResultsCapped` when more matched than the answer lists;
//! - `Deadline` (plan §2.7) when the internal deadline passed: the answer holds what was found by
//!   then (pinned by `tests/queries.rs::should_answer_incomplete_with_deadline_when_the_clock_passes_it`);
//! - an answer with no hit is never complete: a definition always exists somewhere, so an empty
//!   one says why (pinned by
//!   `tests/queries.rs::should_say_why_when_a_definition_finds_nothing`).
//!
//! **`exact` only on the allowlist** (`admit`): today that is the cursor on a declaration's own
//! name, which needs no name resolution. A name the scope rules of [`resolve`] bind is answered
//! `possible` (its candidate kept, with the reason the binding is not proved) and incomplete:
//! two adversarial reviews with javac as the oracle found the rules, read from summary ranges and
//! a text scanner, binding wrong declarations, so they rank candidates and prove nothing until
//! the syntax-tree resolver (the card's next item) earns a place on the allowlist. Bounded
//! ambiguity (plan §2.2) lists the overloads of the owner the rules found, never a same-name
//! member of another type. A target is named only with its location.

mod resolve;
mod scan;
mod symbols;

use core::future::Future;
use core::time::Duration;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use jfl_domain::{
    ByteRange, Completeness, DeclKind, Envelope, EnvelopeParts, FileUri, Freshness,
    IncompleteReason, LineIndex, Location, ModuleName, ObjectId, Position, PositionEncoding,
    PositionError, PossibleHit, Range, Reason, RelPath, SymbolId, SyntaxKey, Target, TargetStatus,
    View, Why,
};

use crate::clock::{WallClock, elapsed};
use crate::source_tree::MTime;
use crate::view_service::{self, Answer, GitObserved, Summary};
use crate::views::{FileSeen, Published};

use resolve::{Cursor, FileEntry, Hit, Outcome, Resolver, Snapshot, Unproved};

pub use symbols::{
    DocumentSymbol, DocumentSymbols, MAX_QUERY_BYTES, SymbolFilter, SymbolQuery, Symbols,
    WorkspaceSymbol, document_symbols, workspace_symbols,
};

/// The queries' settings; composition chooses them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryConfig {
    /// Definition's and hover's internal deadline (plan §2.7: 300 ms).
    pub definition_budget: Duration,
    /// Workspace symbols' internal deadline (plan §2.7: 2 s).
    pub symbols_budget: Duration,
    /// The most candidates a receiver of unknown type lists.
    pub candidates: usize,
    /// The most workspace symbols listed when the query names no limit.
    pub symbols_limit: usize,
}

impl Default for QueryConfig {
    /// Plan §2.7's deadlines, 64 candidates and 100 symbols.
    fn default() -> Self {
        Self {
            definition_budget: Duration::from_millis(300),
            symbols_budget: Duration::from_secs(2),
            candidates: 64,
            symbols_limit: 100,
        }
    }
}

/// An internal deadline (plan §2.7): read between units of work, never waited on.
pub trait Deadline: Sync {
    /// Whether it has passed.
    fn passed(&self) -> bool;
}

/// A deadline `budget` after it was made, on a [`WallClock`].
#[derive(Debug)]
pub struct ClockDeadline<'c, C> {
    clock: &'c C,
    start: MTime,
    budget: Duration,
}

impl<'c, C: WallClock> ClockDeadline<'c, C> {
    /// Passes `budget` from now.
    pub fn new(clock: &'c C, budget: Duration) -> Self {
        Self {
            clock,
            start: clock.now(),
            budget,
        }
    }
}

impl<C: WallClock> Deadline for ClockDeadline<'_, C> {
    fn passed(&self) -> bool {
        elapsed(self.start, self.clock.now()) >= self.budget
    }
}

/// A file's text and its line index, read for one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileText {
    /// The text, after a dropped byte-order mark (the offsets of a summary count from there).
    pub text: String,
    /// Its lines.
    pub lines: LineIndex,
}

impl FileText {
    /// `text` and its line index; `None` for a text over 4 GiB.
    #[must_use]
    pub fn new(text: String) -> Option<Self> {
        let lines = LineIndex::new(&text).ok()?;
        Some(Self { text, lines })
    }
}

/// Where the queries read a file's text from.
pub trait Sources: Sync {
    /// The text of `root/path`, only if its bytes are the ones `key` names; `None` otherwise (the
    /// file changed since, or could not be read).
    fn text(
        &self,
        root: &FileUri,
        path: &RelPath,
        key: &SyntaxKey,
    ) -> impl Future<Output = Option<Arc<FileText>>> + Send;
}

/// A pinned request as the queries read it: the answer, and what the caller knows beside it.
#[derive(Debug, Clone, Copy)]
pub struct Pinned<'a> {
    /// The pinned request.
    pub answer: &'a Answer,
    /// The view's `HEAD` commit, as the caller resolved it (`None` on an unborn branch).
    pub head: Option<ObjectId>,
    /// How current the view was.
    pub freshness: Freshness,
}

impl Pinned<'_> {
    fn view(&self) -> View {
        View {
            worktree: self.answer.view.clone(),
            head: self.head,
            generation: self.answer.generation.generation,
        }
    }
}

/// Why a query gave no answer at all. Anything it could still answer around is an incomplete
/// answer instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueryError {
    /// No name is at the position (`jfl` exits 1, plan §2.4).
    #[error("no symbol at the position")]
    NoSymbol,
    /// The position is not in the file (plan §2.3: refused, never clamped).
    #[error("position refused: {0}")]
    Position(#[from] PositionError),
    /// The request's file is not in the pinned generation (deleted).
    #[error("the file is not in the pinned generation")]
    NotInGeneration,
    /// The request's file changed between the pin and the read: ask again with a new request.
    #[error("the file changed after the request pinned it")]
    Changed,
}

/// What a definition found, before it is an envelope: shared with hover.
struct Found {
    envelope: Envelope,
    /// The hovered name's range in the request's file.
    range: Option<Range>,
    /// The declarations hover describes: the exact one, or a proved owner's overloads.
    described: Vec<HoverDeclaration>,
}

/// The generation a request pinned.
type Pinnable = Published<FileSeen, GitObserved, Summary>;

/// A view's last queried generation (held weakly: the cache keeps no generation alive) and its
/// snapshot.
type Cached = (Weak<Pinnable>, Arc<Snapshot>);

/// Each view's last queried generation and its [`Snapshot`]: the name tables are built once per
/// generation (the first query after a publish pays for them), not once per query. Held by the
/// daemon beside the view service; a generation no request holds any more is let go with the
/// next one's snapshot.
///
/// Pinned by: `tests/queries.rs::should_build_the_name_tables_once_when_a_generation_is_queried_again`.
#[derive(Debug, Default)]
pub struct QueryCache {
    views: Mutex<BTreeMap<FileUri, Cached>>,
    builds: AtomicUsize,
}

impl QueryCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many snapshots it built.
    #[must_use]
    pub fn builds(&self) -> usize {
        self.builds.load(Ordering::Relaxed)
    }

    /// The snapshot of `answer`'s pinned generation; `None` when `deadline` passed while it was
    /// built (nothing is kept then).
    fn snapshot<D: Deadline>(&self, answer: &Answer, deadline: &D) -> Option<Arc<Snapshot>> {
        let generation = Arc::downgrade(&answer.generation);
        {
            let views = self.views.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((held, snapshot)) = views.get(&answer.view)
                && held.ptr_eq(&generation)
            {
                return Some(Arc::clone(snapshot));
            }
        }
        let mut files = Vec::with_capacity(answer.generation.index.len());
        for (i, (path, entry)) in answer.generation.index.iter().enumerate() {
            if i % 1024 == 1023 && deadline.passed() {
                return None;
            }
            if let Some(Summary::Indexed { key, summary }) = &entry.summary {
                files.push(FileEntry {
                    path: path.clone(),
                    key: *key,
                    summary: Arc::clone(summary),
                });
            }
        }
        let snapshot = Arc::new(Snapshot::new(files));
        self.builds.fetch_add(1, Ordering::Relaxed);
        self.views
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(answer.view.clone(), (generation, Arc::clone(&snapshot)));
        Some(snapshot)
    }
}

/// The location of `range` in a file's text, in `encoding`.
fn convert(text: &FileText, range: ByteRange, encoding: PositionEncoding) -> Option<Range> {
    text.lines.range(range, encoding).ok()
}

/// How many files' text a query reads at once: each read is a hand-off to the ports' pool.
const READS_IN_FLIGHT: usize = 16;

/// Reads the text of each of `wanted` (an index, its path, its key), [`READS_IN_FLIGHT`] at a
/// time; once `deadline` passes, the rest are skipped. The texts read, how many files had moved
/// on since their key, and whether the deadline cut the reads.
async fn read_texts<'w, S: Sources, D: Deadline>(
    root: &FileUri,
    wanted: Vec<(usize, &'w RelPath, &'w SyntaxKey)>,
    sources: &S,
    deadline: &D,
) -> (BTreeMap<usize, Arc<FileText>>, u32, bool) {
    let read = view_service::bounded(READS_IN_FLIGHT, wanted, |(at, path, key)| async move {
        if deadline.passed() {
            return (at, None);
        }
        (at, Some(sources.text(root, path, key).await))
    })
    .await;
    let mut out = BTreeMap::new();
    let mut stale = 0_u32;
    let mut cut = false;
    for (at, text) in read {
        match text {
            None => cut = true,
            Some(None) => stale = stale.saturating_add(1),
            Some(Some(text)) => {
                out.insert(at, text);
            }
        }
    }
    (out, stale, cut)
}

/// Reads the text of every file among `hits` but the request's own (`known`).
async fn texts<S: Sources, D: Deadline>(
    root: &FileUri,
    snapshot: &Snapshot,
    hits: &[Hit],
    known: (u32, &Arc<FileText>),
    sources: &S,
    deadline: &D,
) -> (BTreeMap<u32, Arc<FileText>>, u32, bool) {
    let mut files: Vec<u32> = hits
        .iter()
        .map(|h| h.file)
        .filter(|&f| f != known.0)
        .collect();
    files.sort_unstable();
    files.dedup();
    let wanted = files
        .iter()
        .filter_map(|&file| {
            let f = snapshot.file(file)?;
            Some((usize::try_from(file).ok()?, &f.path, &f.key))
        })
        .collect();
    let (read, stale, cut) = read_texts(root, wanted, sources, deadline).await;
    let mut out: BTreeMap<u32, Arc<FileText>> = read
        .into_iter()
        .filter_map(|(at, text)| Some((u32::try_from(at).ok()?, text)))
        .collect();
    out.insert(known.0, Arc::clone(known.1));
    (out, stale, cut)
}

/// `jfl def` (plan §2.2): the declaration the name at `position` of the request's file binds to.
///
/// Pinned by `tests/queries.rs`.
pub async fn definition<S: Sources, D: Deadline>(
    pinned: &Pinned<'_>,
    sources: &S,
    position: Position,
    encoding: PositionEncoding,
    config: &QueryConfig,
    deadline: &D,
    cache: &QueryCache,
) -> Result<Envelope, QueryError> {
    Ok(
        define(pinned, sources, position, encoding, config, deadline, cache)
            .await?
            .envelope,
    )
}

/// The shared core of definition and hover.
async fn define<S: Sources, D: Deadline>(
    pinned: &Pinned<'_>,
    sources: &S,
    position: Position,
    encoding: PositionEncoding,
    config: &QueryConfig,
    deadline: &D,
    cache: &QueryCache,
) -> Result<Found, QueryError> {
    let answer = pinned.answer;
    let mut completeness = answer.completeness.clone();
    let parts = |completeness: Completeness| EnvelopeParts {
        view: pinned.view(),
        target: None,
        exact: Vec::new(),
        possible: Vec::new(),
        completeness,
        freshness: pinned.freshness,
        position_encoding: encoding,
    };
    let entry = answer
        .generation
        .index
        .get(&answer.path)
        .ok_or(QueryError::NotInGeneration)?;
    let (key, summary) = match &entry.summary {
        Some(Summary::Indexed { key, summary }) => (key, summary),
        Some(Summary::Absent) => return Err(QueryError::NotInGeneration),
        // Not read yet, or not indexable: nothing proves what is at the position.
        Some(Summary::Gap | Summary::Pending) | None => {
            return Ok(Found {
                envelope: Envelope::new(parts(
                    completeness.and(IncompleteReason::TargetUnresolved),
                )),
                range: None,
                described: Vec::new(),
            });
        }
    };
    let own = sources
        .text(&answer.view, &answer.path, key)
        .await
        .ok_or(QueryError::Changed)?;
    let offset = own.lines.offset(position, encoding)?;
    let cursor = resolve::cursor(summary, offset.get()).ok_or(QueryError::NoSymbol)?;
    let hovered = match cursor {
        Cursor::Declaration(id) => summary.declaration(id).map(|d| d.name_range),
        Cursor::Site(site) => Some(site.range),
    }
    .and_then(|r| convert(&own, r, encoding));
    let Some(snapshot) = cache.snapshot(answer, deadline) else {
        return Ok(Found {
            envelope: Envelope::new(parts(completeness.deadline())),
            range: hovered,
            described: Vec::new(),
        });
    };
    let request_file = snapshot
        .file_at(&answer.path)
        .ok_or(QueryError::NotInGeneration)?;
    let resolver = Resolver::with_deadline(&snapshot, Some(deadline));
    let outcome = match cursor {
        Cursor::Declaration(decl) => Outcome::Exact(Hit {
            file: request_file,
            decl,
        }),
        Cursor::Site(site) => {
            resolver.resolve_site(request_file, &own.text, &site, config.candidates)
        }
    };
    if resolver.cut() {
        // Every lookup after the deadline answered "outside": the outcome is no answer.
        return Ok(Found {
            envelope: Envelope::new(parts(completeness.deadline())),
            range: hovered,
            described: Vec::new(),
        });
    }
    let outcome = admit(cursor, outcome);
    // What the outcome lists, and how it is read.
    let mut target: Option<(Hit, TargetStatus)> = None;
    let mut exact: Vec<Hit> = Vec::new();
    let mut possible: Vec<(Hit, Why)> = Vec::new();
    let mut described: Vec<Hit> = Vec::new();
    match outcome {
        Outcome::Exact(hit) => {
            target = Some((hit, TargetStatus::Exact));
            exact.push(hit);
            described.push(hit);
        }
        Outcome::Overloads {
            all,
            applicable,
            outside,
        } => {
            let overloads = u32::try_from(all.len()).unwrap_or(u32::MAX);
            if let Some(&first) = applicable.first().or_else(|| all.first()) {
                target = Some((first, TargetStatus::Ambiguous { overloads }));
            }
            let why = if outside {
                Why::with_detail(
                    Reason::OverloadNotProved,
                    Reason::SupertypeNotIndexed.text(),
                )
            } else {
                Why::new(Reason::OverloadNotProved)
            };
            possible.extend(all.iter().map(|&h| (h, why.clone())));
            described.extend(all);
            if outside {
                completeness = completeness.and(IncompleteReason::TargetUnresolved);
            }
        }
        Outcome::Unproved(hits, unproved) => {
            let why = match unproved {
                Unproved::Supertype => Why::new(Reason::SupertypeNotIndexed),
                Unproved::Import => Why::with_detail(Reason::ImportUnresolved, "on-demand import"),
                Unproved::Ambiguous => {
                    Why::with_detail(Reason::NameAndArity, "declared more than once")
                }
                Unproved::Local => Why::with_detail(
                    Reason::NameAndArity,
                    "a type variable, local class or local variable may be meant",
                ),
                Unproved::SamePackage => Why::with_detail(
                    Reason::NameAndArity,
                    "same package: source sets and generated sources are not known yet",
                ),
                Unproved::ScopeRules => Why::with_detail(
                    Reason::NameAndArity,
                    "bound by scope rules read from syntax, not proved",
                ),
                Unproved::Overloads => Why::with_detail(
                    Reason::OverloadNotProved,
                    "owner bound by scope rules read from syntax, not proved",
                ),
            };
            possible.extend(hits.into_iter().map(|h| (h, why.clone())));
            completeness = completeness.and(IncompleteReason::TargetUnresolved);
        }
        Outcome::Receiver {
            hits,
            capped,
            method_ref,
        } => {
            let why = if method_ref {
                Why::new(Reason::MethodReference)
            } else {
                Why::new(Reason::ReceiverTypeUnknown)
            };
            possible.extend(hits.into_iter().map(|h| (h, why.clone())));
            completeness = completeness.and(IncompleteReason::TargetUnresolved);
            if capped {
                completeness = completeness.and(IncompleteReason::ResultsCapped {
                    limit: u32::try_from(config.candidates).unwrap_or(u32::MAX),
                });
            }
        }
        Outcome::Outside => completeness = completeness.and(IncompleteReason::TargetNotIndexed),
        Outcome::Unknown => completeness = completeness.and(IncompleteReason::TargetUnresolved),
    }
    let mut all_hits: Vec<Hit> = exact.clone();
    all_hits.extend(possible.iter().map(|(h, _)| *h));
    let (read, stale, cut) = texts(
        &answer.view,
        &snapshot,
        &all_hits,
        (request_file, &own),
        sources,
        deadline,
    )
    .await;
    if cut {
        completeness = completeness.deadline();
    }
    if stale > 0 {
        completeness = completeness.and(IncompleteReason::ModuleStale {
            module: ModuleName::worktree(),
            stale,
        });
    }
    let locate = |hit: Hit| -> Option<Location> {
        let f = snapshot.file(hit.file)?;
        let text = read.get(&hit.file)?;
        let d = snapshot.decl(hit)?;
        Some(Location {
            uri: answer.view.join(&f.path).ok()?,
            range: convert(text, d.name_range, encoding)?,
        })
    };
    let exact: Vec<Location> = exact.into_iter().filter_map(locate).collect();
    let possible: Vec<PossibleHit> = possible
        .into_iter()
        .filter_map(|(h, why)| {
            Some(PossibleHit {
                location: locate(h)?,
                why,
            })
        })
        .collect();
    // A target is named only with its location: a stale file or a cut read leaves it out.
    let target = target.and_then(|(hit, status)| {
        locate(hit)?;
        let symbol: SymbolId = snapshot.file(hit.file)?.summary.symbol_id(hit.decl).ok()?;
        Some(Target { symbol, status })
    });
    if exact.is_empty() && possible.is_empty() && completeness.is_complete() {
        // A definition always exists somewhere: an empty answer is never complete.
        completeness = completeness.and(IncompleteReason::TargetUnresolved);
    }
    let described = describe(&snapshot, &described, &read);
    Ok(Found {
        envelope: Envelope::new(EnvelopeParts {
            target,
            exact,
            possible,
            ..parts(completeness)
        }),
        range: hovered,
        described,
    })
}

/// **The allowlist of `exact`: the one place a definition may say `exact`.** A construct is on
/// it only when its binding needs no name resolution at all:
///
/// - the cursor on a declaration's own name ([`Cursor::Declaration`]).
///
/// Everything the scope rules of [`resolve`] bind is admitted only as `possible` (its candidates
/// kept, the owner of overloads no longer claimed): the rules read Java's structure from summary
/// ranges and a text scanner, and two adversarial reviews found false `exact`s there (anonymous
/// classes behind nested arguments, comments before keys and labels, class headers resolved in
/// the body's scope, member types against a type's own name, annotations before type
/// parameters). A construct joins the allowlist only with the syntax-tree resolver (the card's
/// next item) and a javac differential test of it.
///
/// Pinned by: `tests/queries.rs::should_match_javac_or_stay_possible_when_the_review_s_probes_are_asked`
/// and `tests/queries.rs::should_answer_exact_only_on_a_declaration_s_own_name_when_any_name_is_asked`.
fn admit(cursor: Cursor, outcome: Outcome) -> Outcome {
    match (cursor, outcome) {
        (Cursor::Declaration(_), outcome) => outcome,
        (Cursor::Site(_), Outcome::Exact(hit)) => {
            Outcome::Unproved(vec![hit], Unproved::ScopeRules)
        }
        (Cursor::Site(_), Outcome::Overloads { all, .. }) => {
            Outcome::Unproved(all, Unproved::Overloads)
        }
        (Cursor::Site(_), other) => other,
    }
}

/// A declaration hover describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverDeclaration {
    /// Its symbol, when it has one.
    pub symbol: Option<SymbolId>,
    /// Its kind.
    pub kind: DeclKind,
    /// Its signature as written (annotations included, body and Javadoc left out).
    pub signature: String,
    /// Its Javadoc's words, when it has Javadoc and its file's text was read.
    pub documentation: Option<String>,
}

/// `jfl hover` (plan §2.2): the definition's envelope, the hovered name's range, the
/// declarations it binds to (one, or a proved owner's overloads), and the module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    /// The definition's envelope: target, locations and completeness.
    pub envelope: Envelope,
    /// The hovered name's range in the request's file.
    pub range: Option<Range>,
    /// The declarations described: empty when the target is unresolved.
    pub declarations: Vec<HoverDeclaration>,
    /// The module of the declarations (before M3, the worktree, `:`).
    pub module: ModuleName,
}

impl Hover {
    /// The status line (plan §2.2): `exact`, `ambiguous (3 overloads)` or
    /// `unresolved: <reasons>`; ` (incomplete: …)` follows when the answer is not complete.
    ///
    /// Pinned by: `tests/queries.rs::should_describe_the_declaration_when_hovering_a_call`.
    #[must_use]
    pub fn status_line(&self) -> String {
        let reasons = || {
            self.envelope
                .incomplete_because()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        };
        match self.envelope.target().map(|t| t.status) {
            None => format!("unresolved: {}", reasons()),
            Some(status) => {
                let head = match status {
                    TargetStatus::Exact => "exact".to_owned(),
                    TargetStatus::Ambiguous { overloads } => {
                        format!("ambiguous ({overloads} overloads)")
                    }
                };
                if self.envelope.complete() {
                    head
                } else {
                    format!("{head} (incomplete: {})", reasons())
                }
            }
        }
    }
}

/// `jfl hover`: what [`definition`] finds, described.
///
/// Pinned by `tests/queries.rs`.
pub async fn hover<S: Sources, D: Deadline>(
    pinned: &Pinned<'_>,
    sources: &S,
    position: Position,
    encoding: PositionEncoding,
    config: &QueryConfig,
    deadline: &D,
    cache: &QueryCache,
) -> Result<Hover, QueryError> {
    let found = define(pinned, sources, position, encoding, config, deadline, cache).await?;
    Ok(Hover {
        envelope: found.envelope,
        range: found.range,
        declarations: found.described,
        module: ModuleName::worktree(),
    })
}

/// The hover description of each hit; Javadoc only from a file whose text was read.
fn describe(
    snapshot: &Snapshot,
    hits: &[Hit],
    read: &BTreeMap<u32, Arc<FileText>>,
) -> Vec<HoverDeclaration> {
    hits.iter()
        .filter_map(|&hit| {
            let f = snapshot.file(hit.file)?;
            let d = snapshot.decl(hit)?;
            let documentation = d.javadoc.and_then(|javadoc| {
                let text = read.get(&hit.file)?;
                let start = usize::try_from(javadoc.start().get()).ok()?;
                let end = usize::try_from(javadoc.end().get()).ok()?;
                Some(scan::javadoc_words(text.text.get(start..end)?))
            });
            Some(HoverDeclaration {
                symbol: f.summary.symbol_id(hit.decl).ok(),
                kind: d.kind,
                signature: f.summary.str(d.signature).to_owned(),
                documentation,
            })
        })
        .collect()
}
