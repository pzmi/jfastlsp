//! The driver of `formal/diagnostics.qnt` (LSP-04; plan §3.2, §7.3, `doc/formal-models.md`,
//! "`diagnostics`"): the model's stimuli applied to `jfl-application`'s diagnostics
//! [`Coordinator`], with the testkit's [`ScriptedWorker`] as the javac worker, a scripted disk and
//! a scripted client that records every publication; the real state observed after every step.
//!
//! The world is the model's: files `a`, `b` and `c`; `a`'s compile reads `b` and `c` (the
//! coordinator is told so with [`Coordinator::track`]), `b`'s only itself; the client opens `a`
//! and `b`; three job slots (the coordinator's capacity). A text is its model number, as content
//! and as stamp, so a larger number is newer, as the model's texts are.
//!
//! What is observed from the real code: each document's state and the disk text seen of `c`
//! (the coordinator); each running or written job, by slot (the worker's state, the
//! coordinator's record of what the job was started for); the worker's incarnation and losses
//! (the worker); and what the client shows of each document, decoded from the diagnostics the
//! scripted compiler wrote ([`Echo`]) and the version the publication carried. What is the
//! driver's record: the disk, the slot of each job (the first free one, as the model's), and the
//! model's ghosts (`high`, `sinceEdit`, `sent`, `nextText`, `t`), kept by the model's own rules
//! from what the coordinator returned (the documents a change bumped, the publications).
//!
//! Every type of the observation, nested ones included, has `deny_unknown_fields` and a derived
//! `PartialEq` (obligation 10).
//!
//! The negative control ([`control`], plan §7.3): a coordinator shell that publishes every reply
//! as it arrives, stale ones included. It diverges deterministically at the `deliver` of a reply
//! older than the published one: step 12 of the committed
//! `diagnostics-two-quick-edits-reverse-order` trace. Pinned by
//! tests/replay.rs::should_diverge_at_the_stale_reply_when_the_diagnostics_control_publishes_every_reply.

use std::collections::{BTreeMap, BTreeSet};

use jfl_application::diagnostics::{
    Changed, Coordinator, Delivery, Document, Job, JobId, Publication, Text,
};
use jfl_testkit::fakes::compiler::{Echo, JobState, ScriptedWorker};
use serde::{Deserialize, Serialize};

use crate::driver::{Driver, Step, StepError};
use crate::replay::Replayer;

const DOCS: [&str; 2] = ["a", "b"];
const FILES: [&str; 3] = ["a", "b", "c"];
const SLOTS: [&str; 3] = ["j1", "j2", "j3"];
/// The model's `NO_VERSION`.
const NO_VERSION: i64 = -1;

/// The texts a compile read: `{ a, b, c }`, 0 for a file it does not read or did not find.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputsObs {
    /// `a`'s text.
    pub a: i64,
    /// `b`'s text.
    pub b: i64,
    /// `c`'s text.
    pub c: i64,
}

/// A document as the coordinator holds it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DocObs {
    /// Open.
    pub open: bool,
    /// Its open count.
    pub session: i64,
    /// The client's latest version.
    pub version: i64,
    /// That version's text, 0 when closed.
    pub overlay: i64,
    /// The disk text the coordinator has seen.
    pub seen_disk: i64,
    /// Its input epoch.
    pub epoch: i64,
    /// A compile is pending.
    pub dirty: bool,
}

/// The worker.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerObs {
    /// Up.
    pub up: bool,
    /// Its incarnation.
    pub inc: i64,
    /// The running jobs its crashes lost.
    pub lost: i64,
}

/// A job slot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JobObs {
    /// `free`, `running` or `done`.
    pub state: String,
    /// The document, `""` when free.
    pub doc: String,
    /// The session it was started in.
    pub session: i64,
    /// The version it is published with (-1: none).
    pub version: i64,
    /// The epoch it was started at.
    pub epoch: i64,
    /// The texts it read.
    pub inputs: InputsObs,
    /// The incarnation that runs it.
    pub inc: i64,
    /// It timed out.
    pub late: bool,
}

/// The client's last notification of a document.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PubObs {
    /// `none`, `cleared` or `diags`.
    pub kind: String,
    /// Its version field (-1: none).
    pub version: i64,
    /// The session the diagnostics were computed in.
    pub session: i64,
    /// The epoch they were computed at.
    pub epoch: i64,
    /// The texts they read.
    pub inputs: InputsObs,
    /// The incarnation that computed them.
    pub inc: i64,
}

/// Ghost: the highest epoch and version published before the latest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HighObs {
    /// The epoch.
    pub epoch: i64,
    /// The version.
    pub version: i64,
}

/// Ghost: a text the client sent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SentObs {
    /// The document.
    pub doc: String,
    /// Its session.
    pub session: i64,
    /// The version.
    pub version: i64,
    /// The text.
    pub text: i64,
}

/// The diagnostics model's eleven variables, as the driver observes them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DiagnosticsState {
    /// The disk's texts, 0 for an absent file.
    pub disk: BTreeMap<String, i64>,
    /// The text of `c` the coordinator has seen.
    pub seen_c: i64,
    /// The documents.
    pub docs: BTreeMap<String, DocObs>,
    /// The worker.
    pub worker: WorkerObs,
    /// The job slots.
    pub jobs: BTreeMap<String, JobObs>,
    /// What the client shows.
    pub client: BTreeMap<String, PubObs>,
    /// Ghost: the highest published before the latest.
    pub high: BTreeMap<String, HighObs>,
    /// Ghost: a publication arrived after the latest change of the inputs.
    pub since_edit: BTreeMap<String, bool>,
    /// Ghost: every text the client sent.
    pub sent: BTreeSet<SentObs>,
    /// The next text's number.
    pub next_text: i64,
    /// The steps taken.
    pub t: i64,
}

const NO_HIGH: HighObs = HighObs {
    epoch: -1,
    version: -1,
};

fn no_pub(kind: &str) -> PubObs {
    PubObs {
        kind: kind.to_owned(),
        version: NO_VERSION,
        session: 0,
        epoch: 0,
        inputs: InputsObs::default(),
        inc: 0,
    }
}

/// The faults: none for the driver, one for the negative control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    /// Every reply is published as it arrives (plan §7.3's control).
    PublishEveryReply,
}

/// Drives the coordinator, the scripted worker, disk and client by `diagnostics.qnt`'s actions.
pub struct DiagnosticsDriver {
    fault: Fault,
    coordinator: Coordinator<String>,
    worker: ScriptedWorker,
    disk: BTreeMap<String, i64>,
    slots: BTreeMap<String, JobId>,
    client: BTreeMap<String, PubObs>,
    high: BTreeMap<String, HighObs>,
    since_edit: BTreeMap<String, bool>,
    sent: BTreeSet<SentObs>,
    next_text: i64,
    t: i64,
}

/// A text as the model numbers it: its number is its content and its stamp.
fn text(n: i64) -> Text {
    Text::new(u64::try_from(n).unwrap_or_default(), n.to_string())
}

/// A text's model number; -1 for one the driver did not write (a divergence, never a match).
fn number(text: Option<&Text>) -> i64 {
    text.map_or(0, |t| t.content().parse().unwrap_or(-1))
}

fn echoed(text: Option<&Option<String>>) -> i64 {
    match text {
        None | Some(None) => 0,
        Some(Some(t)) => t.parse().unwrap_or(-1),
    }
}

fn int<T: TryInto<i64>>(n: T) -> i64 {
    n.try_into().unwrap_or(-1)
}

impl DiagnosticsDriver {
    fn new(fault: Fault) -> Self {
        let disk: BTreeMap<String, i64> = [("a", 1), ("b", 2), ("c", 0)]
            .into_iter()
            .map(|(f, n)| (f.to_owned(), n))
            .collect();
        let seen = disk
            .iter()
            .filter(|(_, n)| **n != 0)
            .map(|(f, n)| (f.clone(), text(*n)));
        let mut coordinator = Coordinator::new(SLOTS.len(), seen);
        let _a = coordinator.track("a".to_owned(), ["b".to_owned(), "c".to_owned()]);
        let _b = coordinator.track("b".to_owned(), []);
        Self {
            fault,
            coordinator,
            worker: ScriptedWorker::new(),
            disk,
            slots: BTreeMap::new(),
            client: DOCS
                .iter()
                .map(|d| ((*d).to_owned(), no_pub("none")))
                .collect(),
            high: per_doc(NO_HIGH),
            since_edit: per_doc(false),
            sent: BTreeSet::new(),
            next_text: 3,
            t: 0,
        }
    }

    fn doc(step: &Step, among: &[&str]) -> Result<String, StepError> {
        let doc: String = step
            .pick("doc")?
            .ok_or_else(|| StepError::Harness(format!("step {}: no `doc` pick", step.index())))?;
        if among.contains(&doc.as_str()) {
            Ok(doc)
        } else {
            Err(StepError::Harness(format!(
                "step {}: `{doc}` is not one of {among:?}",
                step.index()
            )))
        }
    }

    fn slot(&self, step: &Step) -> Result<(String, JobId), StepError> {
        let slot: String = step
            .pick("job")?
            .ok_or_else(|| StepError::Harness(format!("step {}: no `job` pick", step.index())))?;
        let id = self.slots.get(&slot).copied().ok_or_else(|| {
            StepError::Refused(format!("step {}: slot {slot} holds no job", step.index()))
        })?;
        Ok((slot, id))
    }

    fn refused(step: &Step, why: impl std::fmt::Display) -> StepError {
        StepError::Refused(format!("step {}: {}: {why}", step.index(), step.action()))
    }

    /// The model's `staleAfter`: a change of a document's inputs waits for a new publication.
    fn stale(&mut self, changed: &Changed<String>) {
        for doc in changed {
            self.since_edit.insert(doc.clone(), false);
        }
    }

    fn next(&mut self) -> i64 {
        let n = self.next_text;
        self.next_text = n.saturating_add(1);
        n
    }

    fn client_version(&self, doc: &str) -> i64 {
        self.coordinator
            .document(&doc.to_owned())
            .map_or(0, |d| i64::from(d.version))
    }

    fn sent_text(&mut self, doc: &str, version: i64, n: i64) {
        let session = self
            .coordinator
            .document(&doc.to_owned())
            .map_or(0, |d| i64::from(d.session.0));
        self.sent.insert(SentObs {
            doc: doc.to_owned(),
            session,
            version,
            text: n,
        });
    }

    fn change(&mut self, step: &Step, doc: &str, n: i64) -> Result<(), StepError> {
        let version = self.client_version(doc).saturating_add(1);
        let lsp = i32::try_from(version).map_err(|e| Self::refused(step, e))?;
        let changed = self
            .coordinator
            .did_change(doc.to_owned(), lsp, text(n))
            .map_err(|e| Self::refused(step, e))?;
        self.stale(&changed);
        self.sent_text(doc, version, n);
        Ok(())
    }

    /// The client receives `publication` (the model's `deliverWith` accepting it).
    fn publish(&mut self, publication: Publication<String, Vec<String>>) {
        let echo = publication.diagnostics.first().and_then(|m| Echo::parse(m));
        let shown = match echo {
            Some(echo) => PubObs {
                kind: "diags".to_owned(),
                version: publication.version.map_or(NO_VERSION, i64::from),
                session: int(echo.session),
                epoch: int(echo.epoch),
                inputs: InputsObs {
                    a: echoed(echo.inputs.get("a")),
                    b: echoed(echo.inputs.get("b")),
                    c: echoed(echo.inputs.get("c")),
                },
                inc: int(echo.incarnation),
            },
            None => no_pub("unreadable"),
        };
        let doc = publication.document;
        if let Some(previous) = self.client.get(&doc).filter(|p| p.kind == "diags") {
            let high = self.high.get(&doc).copied().unwrap_or(NO_HIGH);
            let raised = HighObs {
                epoch: high.epoch.max(previous.epoch),
                version: high.version.max(previous.version),
            };
            self.high.insert(doc.clone(), raised);
        }
        self.since_edit.insert(doc.clone(), true);
        self.client.insert(doc, shown);
    }

    fn apply(&mut self, step: &Step) -> Result<(), StepError> {
        match step.action() {
            "init" => {
                *self = Self::new(self.fault);
                return Ok(());
            }
            "didOpen" => {
                let doc = Self::doc(step, &DOCS)?;
                let version: i64 = step.pick("version")?.ok_or_else(|| {
                    StepError::Harness(format!("step {}: no `version` pick", step.index()))
                })?;
                let lsp = i32::try_from(version).map_err(|e| Self::refused(step, e))?;
                let n = self.disk.get(&doc).copied().unwrap_or_default();
                let changed = self
                    .coordinator
                    .did_open(doc.clone(), lsp, text(n))
                    .map_err(|e| Self::refused(step, e))?;
                self.stale(&changed);
                self.high.insert(doc.clone(), NO_HIGH);
                self.sent_text(&doc, version, n);
            }
            "didChange" => {
                let doc = Self::doc(step, &DOCS)?;
                let n = self.next();
                self.change(step, &doc, n)?;
            }
            "toolEdit" => {
                let doc = Self::doc(step, &DOCS)?;
                let n = self.next();
                self.disk.insert(doc.clone(), n);
                self.change(step, &doc, n)?;
            }
            "diskWrite" => {
                let doc = Self::doc(step, &FILES)?;
                let n = self.next();
                self.disk.insert(doc, n);
            }
            "diskDelete" => {
                let doc = Self::doc(step, &["c"])?;
                self.disk.insert(doc, 0);
            }
            "didClose" => {
                let doc = Self::doc(step, &DOCS)?;
                let closed = self
                    .coordinator
                    .did_close(doc)
                    .map_err(|e| Self::refused(step, e))?;
                self.stale(&closed.changed);
                self.client.insert(closed.clear, no_pub("cleared"));
            }
            "noticeDisk" => {
                let doc = Self::doc(step, &FILES)?;
                let n = self.disk.get(&doc).copied().unwrap_or_default();
                let changed = self.coordinator.see_disk(doc, (n != 0).then(|| text(n)));
                self.stale(&changed);
            }
            "startJob" => {
                let doc = Self::doc(step, &DOCS)?;
                let slot = SLOTS
                    .iter()
                    .find(|s| !self.slots.contains_key(**s))
                    .ok_or_else(|| Self::refused(step, "every slot is taken"))?;
                let job = self
                    .coordinator
                    .start(&doc)
                    .map_err(|e| Self::refused(step, e))?;
                self.worker
                    .submit(&job)
                    .map_err(|e| Self::refused(step, e))?;
                self.slots.insert((*slot).to_owned(), job.id);
            }
            "deliver" => {
                let (slot, id) = self.slot(step)?;
                let reply = self
                    .worker
                    .take(id)
                    .ok_or_else(|| Self::refused(step, "no reply is written"))?;
                self.slots.remove(&slot);
                match self.coordinator.deliver(reply) {
                    Delivery::Publish(publication) => self.publish(publication),
                    Delivery::Stale(publication) => {
                        if self.fault == Fault::PublishEveryReply {
                            self.publish(publication);
                        }
                    }
                    Delivery::Unknown(_) => {
                        return Err(Self::refused(step, "the coordinator holds no such job"));
                    }
                }
            }
            "complete" => {
                let (_, id) = self.slot(step)?;
                self.worker
                    .complete(id)
                    .map_err(|e| Self::refused(step, e))?;
            }
            "restart" => {
                if !self.coordinator.needs_worker() {
                    return Err(Self::refused(step, "no compile is pending"));
                }
                let inc = self.worker.spawn().map_err(|e| Self::refused(step, e))?;
                self.coordinator
                    .worker_started(inc)
                    .map_err(|e| Self::refused(step, e))?;
            }
            "crash" => {
                let lost = self.worker.crash().map_err(|e| Self::refused(step, e))?;
                self.slots.retain(|_, id| !lost.contains(id));
                self.coordinator
                    .worker_lost(lost)
                    .map_err(|e| Self::refused(step, e))?;
            }
            "timeout" => {
                let (_, id) = self.slot(step)?;
                if self.worker.state(id) != JobState::Running {
                    return Err(Self::refused(step, "the job is not running"));
                }
                self.coordinator
                    .timed_out(id)
                    .map_err(|e| Self::refused(step, e))?;
            }
            _ => return Err(StepError::unknown(step)),
        }
        self.t = self.t.saturating_add(1);
        Ok(())
    }

    fn doc_obs(&self, doc: Option<&Document<String>>, name: &str) -> DocObs {
        let seen = number(self.coordinator.seen(&name.to_owned()));
        doc.map_or(
            DocObs {
                open: false,
                session: 0,
                version: 0,
                overlay: 0,
                seen_disk: seen,
                epoch: 0,
                dirty: false,
            },
            |d| DocObs {
                open: d.open,
                session: i64::from(d.session.0),
                version: i64::from(d.version),
                overlay: number(d.overlay.as_ref()),
                seen_disk: seen,
                epoch: int(d.epoch.0),
                dirty: d.dirty,
            },
        )
    }

    fn job_obs(&self, job: Option<&Job<String>>, state: JobState) -> JobObs {
        let Some(job) = job.filter(|_| state != JobState::Absent) else {
            return JobObs {
                state: "free".to_owned(),
                doc: String::new(),
                session: 0,
                version: 0,
                epoch: 0,
                inputs: InputsObs::default(),
                inc: 0,
                late: false,
            };
        };
        let read = |f: &str| number(job.inputs.get(f).and_then(Option::as_ref));
        JobObs {
            state: match state {
                JobState::Running => "running",
                JobState::Done => "done",
                JobState::Absent => "free",
            }
            .to_owned(),
            doc: job.document.clone(),
            session: i64::from(job.session.0),
            version: job.version.map_or(NO_VERSION, i64::from),
            epoch: int(job.epoch.0),
            inputs: InputsObs {
                a: read("a"),
                b: read("b"),
                c: read("c"),
            },
            inc: i64::from(job.incarnation.0),
            late: job.late,
        }
    }
}

impl Driver for DiagnosticsDriver {
    type State = DiagnosticsState;

    fn step(&mut self, step: &Step) -> Result<(), StepError> {
        self.apply(step)
    }

    fn observe(&self) -> DiagnosticsState {
        DiagnosticsState {
            disk: self.disk.clone(),
            seen_c: number(self.coordinator.seen(&"c".to_owned())),
            docs: DOCS
                .iter()
                .map(|d| {
                    let name = (*d).to_owned();
                    let obs = self.doc_obs(self.coordinator.document(&name), d);
                    (name, obs)
                })
                .collect(),
            worker: WorkerObs {
                up: self.worker.up(),
                inc: i64::from(self.worker.incarnation().0),
                lost: i64::from(self.worker.lost()),
            },
            jobs: SLOTS
                .iter()
                .map(|s| {
                    let obs = match self.slots.get(*s) {
                        Some(id) => self.job_obs(self.coordinator.job(*id), self.worker.state(*id)),
                        None => self.job_obs(None, JobState::Absent),
                    };
                    ((*s).to_owned(), obs)
                })
                .collect(),
            client: self.client.clone(),
            high: self.high.clone(),
            since_edit: self.since_edit.clone(),
            sent: self.sent.clone(),
            next_text: self.next_text,
            t: self.t,
        }
    }
}

/// The driver over the coordinator.
pub fn driver() -> Replayer {
    Replayer::new(DiagnosticsDriver::new(Fault::None))
}

/// The negative control: every reply is published as it arrives, stale ones included.
pub fn control() -> Replayer {
    Replayer::new(DiagnosticsDriver::new(Fault::PublishEveryReply))
}

/// `value` for each document.
fn per_doc<V: Clone>(value: V) -> BTreeMap<String, V> {
    DOCS.iter()
        .map(|d| ((*d).to_owned(), value.clone()))
        .collect()
}
