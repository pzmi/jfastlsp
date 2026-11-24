//! The daemon's service (LSP-02): the view service over the real adapters (the fs tree, the git
//! probe, the Java indexer, the summary store under the state directory), one [`QueryCache`]
//! beside it, and `jfl/definition` and `jfl/hover` answered over a pinned generation (IDX-09,
//! IDX-10's wiring notes).
//!
//! - **A request** binds its file to its view (opening it on first use), pins a generation, runs
//!   the query under its internal deadline (plan §2.7: 300 ms; at the deadline the answer is
//!   partial and says `deadline`), and lets the pin go. Pinned by
//!   `tests/daemon.rs::should_answer_a_definition_from_the_views_with_an_honest_envelope_when_a_client_asks`. The request's own stats run inline on the
//!   loop, the reads and parses on the blocking pool (IDX-09's decision). A file spelled through
//!   a link is bound by its canonical path and answered in the request's spelling of the root
//!   ([`super::spelling`]); pinned by
//!   `tests/daemon.rs::should_answer_in_the_request_s_spelling_when_the_worktree_is_reached_through_a_link`.
//! - **Between requests** ([`Service::background`]): `settle` once a request opened a view, in
//!   slices of [`SETTLE_SLICE`] files (the rest of the view read and published a slice at a time,
//!   IDX-09d), then `write_back` in slices of [`WRITE_SLICE`] summaries, each step run to its
//!   end, so a frame, a signal or a tick waits at most one slice. A rescan due during the settle
//!   (a file created during it, a linked worktree's `git add` or commit seen by a request's
//!   stat, a lost watch, an overflow, a `.git` hint) leaves the deferred files to the slices
//!   (IDX-09e): it lists and stats the view and reads what changed elsewhere, in the settle's
//!   step or in the request that finds it due, never the rest of the view in one build. Pinned by
//!   `tests/daemon.rs::should_answer_a_request_between_the_slices_of_a_settle_when_the_view_is_large`
//!   and, for the rescans, `jfl-application`'s
//!   `tests/view_service.rs::should_leave_the_deferred_files_to_the_settle_when_a_request_finds_a_commit`.
//! - **An open document that differs from the disk** is answered `ContentModified` rather than
//!   from the disk, whether it is the document asked about or one any hit (`exact` or `possible`)
//!   lands in: the index holds the disk's text, so a position in the client's buffer, or a hit's
//!   range shown in it, would belong to another text, and a wrong `exact` is the worst answer
//!   there is (ADR 0014). Plan §4.4's rule decides which text is current: an overlay whose file
//!   was written after the overlay arrived is replaced by the disk (the disk is answered);
//!   otherwise the overlay is the client's current text, which the index does not hold yet.
//!   Indexing overlays is a later step (plan §4.4). Pinned by `fixtures/lsp/document-sync.jsonl`
//!   and `crates/jfl-cli/tests/lsp_daemon.rs::should_answer_from_the_disk_when_it_was_written_after_the_open_document`.
//! - **A column past its line's end** is moved to the end when the request asks for it
//!   (`PositionParams::clamp`, which `jfl lsp` sets: LSP 3.17), on the request file's text the
//!   pinned generation holds, through `jfl-domain`'s `LineIndex::clamp`; otherwise refused
//!   (`PositionOutOfRange`, the CLI's rule). Pinned by
//!   `fixtures/lsp/position-encoding-utf16-astral.jsonl` (ids 5 and 7).
//! - **The view's `HEAD`** is read from its files after the pin (`jfl_adapter_git`'s
//!   `head_commit`, no spawn): the commit the worktree names then, which the pin's stat of
//!   `HEAD` and its ref has just checked.
//! - Every other method is answered as the host's [`super::service::Unwired`] would: `jfl/status`
//!   with the daemon (its views arrive with IDX-13), the rest with an error, never a claim.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_adapter_git::{GitConfig, GitRepoProbe};
use jfl_adapter_ipc::Call;
use jfl_adapter_store::{Blake3, DiskSummaryStore};
use jfl_application::Sources as _;
use jfl_application::queries::{definition, hover};
use jfl_application::syntax_indexer::{MAX_SOURCE_BYTES, strip_bom};
use jfl_application::{
    ClockDeadline, CountingClock, Hover, Pinned, QueryCache, QueryConfig, QueryError,
    ServiceConfig, ServiceError, Summary, ViewService,
};
use jfl_domain::{Envelope, FileUri, Freshness, Position};
use jfl_protocol::codec;
use jfl_protocol::envelope::{Operation, Query, WireRange, WireTarget, WireView};
use jfl_protocol::messages::{self, HoverResult, Method, PositionParams};
use jfl_protocol::rpc::ErrorCode;
use jfl_protocol::{SCHEMA_VERSION, WireEnvelope};

use super::ports::{SystemClock, TokioPool};
use super::service::{Context, Overlay, Service, Working, error_body, result_body, unwired};
use super::spelling::OpenDocuments;
use crate::languages::{self, Indexers};

/// The summary store's cap on disk: 2 GiB, about four times the large corpus's summaries.
pub const STORE_CAP: u64 = 2 << 30;

/// The deferred files one background step of a settle reads and publishes (IDX-09d): about
/// 100 ms of reads and parses on the bench VM's four threads (`doc/bench/README.md`), so a frame,
/// SIGTERM or a tick waits at most about that long behind one (a rescan during the settle leaves
/// the deferred files to the slices, IDX-09e). A publish
/// shares the untouched entries with the generation before, so a slice costs what it reads,
/// whatever the view's size.
pub const SETTLE_SLICE: usize = 512;

/// The summaries one background step writes to the store: about 10 to 60 ms of writes on the
/// bench VM (30 to 250 µs each, IDX-09), so a request waits at most that long behind one.
pub const WRITE_SLICE: usize = 256;

/// Where the summary store lives under the state directory.
pub const STORE_DIR: &str = "summaries";

type Engine = ViewService<
    FsSourceTree<TokioPool>,
    GitRepoProbe<TokioPool>,
    Indexers<TokioPool>,
    DiskSummaryStore<TokioPool>,
    SystemClock,
    Blake3,
>;

/// The views and their queries, as the daemon serves them.
pub struct Views {
    engine: Engine,
    cache: QueryCache,
    queries: QueryConfig,
    clock: SystemClock,
    /// The views a request has opened: a new one is settled after its first answer.
    opened: BTreeSet<FileUri>,
    settle: bool,
    write_back: bool,
}

impl std::fmt::Debug for Views {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Views")
            .field("opened", &self.opened)
            .field("settle", &self.settle)
            .field("write_back", &self.write_back)
            .finish_non_exhaustive()
    }
}

/// What one query found.
enum Found {
    Definition(Envelope),
    Hover(Hover),
}

impl Found {
    /// Every URI at or below `from` spelled below `to` ([`Envelope::respelled`]); kept in `from`'s
    /// spelling whole when one cannot be (too long).
    fn respelled(self, from: &FileUri, to: &FileUri) -> Self {
        match self {
            Self::Definition(envelope) => {
                Self::Definition(envelope.respelled(from, to).unwrap_or(envelope))
            }
            Self::Hover(hover) => Self::Hover(Hover {
                envelope: hover
                    .envelope
                    .respelled(from, to)
                    .unwrap_or_else(|| hover.envelope.clone()),
                ..hover
            }),
        }
    }
}

impl Views {
    /// The views of a daemon whose state directory is `state` (the store under
    /// `state/summaries`, created `0700` by the store), its blocking jobs on `pool`; `owner` names
    /// its cookies (ASCII letters and digits).
    pub fn open(state: &Path, pool: &TokioPool, owner: &str) -> Result<Self, String> {
        let (store, _opened) = DiskSummaryStore::open(
            &state.join(STORE_DIR),
            STORE_CAP,
            Arc::new(CountingClock::default()),
            pool.clone(),
        )
        .map_err(|e| format!("cannot open the summary store: {e}"))?;
        let engine = ViewService::new(
            (
                FsSourceTree::new(pool.clone(), FsConfig::default()),
                GitRepoProbe::new(pool.clone(), GitConfig::default()),
                languages::indexers(pool.clone())?,
                store,
            ),
            SystemClock,
            Blake3::default(),
            ServiceConfig::new(owner),
        );
        Ok(Self {
            engine,
            cache: QueryCache::new(),
            queries: QueryConfig::default(),
            clock: SystemClock,
            opened: BTreeSet::new(),
            settle: false,
            write_back: false,
        })
    }

    async fn answer_call(&mut self, call: &Call, context: &Context<'_>) -> Vec<u8> {
        let op = match call.method.as_str() {
            messages::Definition::NAME => Operation::Def,
            messages::Hover::NAME => Operation::Hover,
            _ => return unwired(call, context),
        };
        let id = call.id.clone();
        let params = match codec::payload_from_value::<PositionParams>(call.params.clone()) {
            Ok(params) => params,
            Err(e) => return error_body(Some(id), ErrorCode::InvalidParams, &e.to_string()),
        };
        let modified = |id| {
            error_body(
                Some(id),
                ErrorCode::ContentModified,
                "an open document differs from the file on disk, which is what is indexed: \
                 save it, or ask again once the disk holds it (plan §4.4)",
            )
        };
        // Open documents by spelling or canonical path (FND-17): whatever the spellings, a
        // modified one is found.
        let open = OpenDocuments::new(context.overlays);
        if overlay_differs(&open, params.uri.file()) {
            return modified(id);
        }
        let found = self.query(op, &params).await;
        self.write_back = true;
        // A hit in an open document the disk does not hold would be placed by the disk's
        // positions in a text the client does not have: refused like the request's own file.
        let hits = match &found {
            Ok(Found::Definition(envelope)) => Some(envelope),
            Ok(Found::Hover(found)) => Some(&found.envelope),
            Err(_) => None,
        };
        if hits.is_some_and(|envelope| hit_in_modified(&open, envelope)) {
            return modified(id);
        }
        match found {
            Ok(Found::Definition(envelope)) => {
                let query = Query {
                    op,
                    uri: params.uri.clone(),
                    line: params.line,
                    col: params.col,
                };
                result_body(id, &WireEnvelope::from_domain(&envelope, Some(query)))
            }
            Ok(Found::Hover(found)) => result_body(id, &hover_result(&found)),
            Err((code, message)) => error_body(Some(id), code, &message),
        }
    }

    async fn query(
        &mut self,
        op: Operation,
        params: &PositionParams,
    ) -> Result<Found, (ErrorCode, String)> {
        // Bound by its canonical path, answered in the request's spelling of the root (FND-17).
        let asked = params.uri.file();
        let file = super::spelling::canonical(asked).unwrap_or_else(|| asked.clone());
        let answer = self.engine.request(&file).await.map_err(|e| match e {
            ServiceError::NotInWorktree => (ErrorCode::NotInWorktree, e.to_string()),
            other => (ErrorCode::InternalError, other.to_string()),
        })?;
        if self.opened.insert(answer.view.clone()) {
            self.settle = true;
        }
        let head = self
            .engine
            .ports()
            .1
            .resolve_blocking(&file)
            .ok()
            .flatten()
            .and_then(|worktree| jfl_adapter_git::snapshot::head_commit(&worktree).ok())
            .flatten()
            .and_then(|id| jfl_domain::ObjectId::parse(id.as_str()).ok());
        let pending = self
            .engine
            .registry()
            .view(&answer.view)
            .map_or(0, |v| u32::try_from(v.dirty.len()).unwrap_or(u32::MAX));
        let pinned = Pinned {
            answer: &answer,
            head,
            // When the generation was reconciled is not recorded yet: `None`, never a guess.
            freshness: Freshness {
                synced_at: None,
                pending_events: pending,
            },
        };
        let deadline = ClockDeadline::new(&self.clock, self.queries.definition_budget);
        let encoding = params.position_encoding;
        let mut position = Position::new(params.line, params.col);
        if params.clamp
            && let Some(clamped) = self.clamped(&answer, position, encoding).await
        {
            position = clamped;
        }
        let sources = self.engine.sources();
        let found = if op == Operation::Hover {
            hover(
                &pinned,
                &sources,
                position,
                encoding,
                &self.queries,
                &deadline,
                &self.cache,
            )
            .await
            .map(Found::Hover)
        } else {
            definition(
                &pinned,
                &sources,
                position,
                encoding,
                &self.queries,
                &deadline,
                &self.cache,
            )
            .await
            .map(Found::Definition)
        };
        let id = answer.id;
        let respell =
            super::spelling::client_root(asked, &answer.view).map(|to| (answer.view.clone(), to));
        drop(answer);
        // The pin is let go whatever the query found; a registry that refuses is this service's
        // defect, and the answer stands.
        let _released = self.engine.finish(id);
        let found = match respell {
            Some((from, to)) => found.map(|found| found.respelled(&from, &to)),
            None => found,
        };
        found.map_err(|e| {
            let code = match e {
                QueryError::NoSymbol => ErrorCode::NoSymbol,
                QueryError::Position(_) => ErrorCode::PositionOutOfRange,
                QueryError::NotInGeneration | QueryError::Changed => ErrorCode::ContentModified,
            };
            (code, e.to_string())
        })
    }
}

impl Views {
    /// `position` with a column past its line's end moved to the end (`PositionParams::clamp`,
    /// LSP 3.17), on the text of the request's file the pinned generation holds; `None` when that
    /// text cannot be read (the query then refuses or finds the file changed).
    async fn clamped(
        &self,
        answer: &jfl_application::Answer,
        position: Position,
        encoding: jfl_domain::PositionEncoding,
    ) -> Option<Position> {
        let entry = answer.generation.index.get(&answer.path)?;
        let Some(Summary::Indexed { key, .. }) = &entry.summary else {
            return None;
        };
        let text = self
            .engine
            .sources()
            .text(&answer.view, &answer.path, key)
            .await?;
        text.lines.clamp(position, encoding).ok()
    }
}

impl Service for Views {
    fn answer<'a>(&'a mut self, call: &'a Call, context: &'a Context<'a>) -> Working<'a, Vec<u8>> {
        Box::pin(self.answer_call(call, context))
    }

    fn background(&mut self) -> Option<Working<'_, ()>> {
        if self.settle {
            return Some(Box::pin(async move {
                // One slice per step; a slice that fails stops the settle, and its view stays
                // partly indexed and says so.
                let more = self.engine.settle(SETTLE_SLICE).await;
                self.settle = matches!(more, Ok(true));
                self.write_back = true;
            }));
        }
        if self.write_back {
            return Some(Box::pin(async move {
                let waiting = self.engine.write_back(WRITE_SLICE).await;
                self.write_back = waiting > 0;
            }));
        }
        None
    }
}

/// `jfl/hover`'s result for what [`hover`] found: the first declaration described, and the
/// status line's reasons when the target is unresolved. The reference summary arrives with
/// IDX-11.
fn hover_result(found: &Hover) -> HoverResult {
    let envelope = &found.envelope;
    let first = found.declarations.first();
    let target = envelope.target().map(WireTarget::from);
    let status = found.status_line();
    HoverResult {
        schema_version: SCHEMA_VERSION,
        position_encoding: envelope.position_encoding(),
        view: WireView::from(envelope.view()),
        range: found.range.map(WireRange),
        signature: first.map(|d| d.signature.clone()),
        documentation: first.and_then(|d| d.documentation.clone()),
        module: Some(found.module.to_string()),
        references: None,
        unresolved: target.is_none().then(|| {
            status
                .strip_prefix("unresolved: ")
                .unwrap_or(&status)
                .to_owned()
        }),
        target,
    }
}

/// Whether any hit of `envelope` (`exact` or `possible`) is in an open document that differs from
/// the disk ([`overlay_differs`]), each file checked once.
///
/// Pinned by: `fixtures/lsp/document-sync.jsonl` (Greeter.java open with lines inserted, a
/// definition from Main.java) and, whatever the spellings,
/// `tests/daemon.rs::should_refuse_a_hit_in_a_modified_document_when_it_was_opened_through_a_link`,
/// `tests/daemon.rs::should_refuse_a_hit_in_a_modified_document_when_the_request_goes_through_a_link_inside_the_worktree`
/// and `tests/daemon.rs::should_refuse_a_hit_in_a_modified_document_when_a_link_leads_from_another_worktree`.
fn hit_in_modified(open: &OpenDocuments<'_, Overlay>, envelope: &Envelope) -> bool {
    if open.is_empty() {
        return false;
    }
    let files: BTreeSet<&FileUri> = envelope
        .exact()
        .iter()
        .map(|l| &l.uri)
        .chain(envelope.possible().iter().map(|p| &p.location.uri))
        .collect();
    files.into_iter().any(|file| overlay_differs(open, file))
}

/// Whether the client's open document `file` holds another text than the file on disk, and the
/// disk was not written after that text arrived (plan §4.4: then the disk wins).
fn overlay_differs(open: &OpenDocuments<'_, Overlay>, file: &FileUri) -> bool {
    let Some(overlay) = open.get(file) else {
        return false;
    };
    let path = Path::new(file.path());
    let Ok(meta) = std::fs::metadata(path) else {
        // No file: the buffer is the only text, and the index holds none of it.
        return true;
    };
    if disk_written_after(&meta, overlay) {
        return false;
    }
    let limit = u64::try_from(MAX_SOURCE_BYTES.saturating_add(4)).unwrap_or(u64::MAX);
    let mut bytes = Vec::new();
    let read = File::open(path).and_then(|file| file.take(limit).read_to_end(&mut bytes));
    read.is_err() || strip_bom(&bytes) != overlay.text.as_bytes()
}

fn disk_written_after(meta: &std::fs::Metadata, overlay: &Overlay) -> bool {
    match (meta.modified().ok(), overlay.disk) {
        (Some(now), Some(then)) => now > then,
        (Some(_), None) => true,
        (None, _) => false,
    }
}
