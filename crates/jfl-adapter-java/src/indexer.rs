//! [`JavaIndexer`]: the `SyntaxIndexer` port over a [`Grammar`], with one parser per thread,
//! reused for every file that thread indexes.

use core::future::Future;
use std::cell::RefCell;
use std::sync::Arc;

use jfl_application::syntax_indexer::{
    GrammarId, Indexed, SiteSyntax, SyntaxIndexError, SyntaxIndexer,
};
use jfl_application::{BlockingPool, FileText};
use jfl_domain::ByteRange;
use jfl_domain::Language;

use crate::extract::{self, Work, Worker};
use crate::grammar::Grammar;
use crate::scopes;

/// The summariser's format version (a `SyntaxKey` input): bumped whenever the same bytes would
/// summarise differently.
pub const SUMMARISER_VERSION: u32 = 1;

thread_local! {
    /// This thread's parser. A parser holds a few kilobytes of tables and its parse stack; making
    /// one per file would cost an allocation burst per file.
    static WORKER: RefCell<Option<Worker>> = const { RefCell::new(None) };
}

/// Summarises `bytes` with `grammar` on this thread's worker.
fn summarise_on_thread(
    grammar: &Grammar,
    bytes: &[u8],
) -> (Result<Indexed, SyntaxIndexError>, Work) {
    WORKER.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => {
            extract::index_counted(grammar, slot.get_or_insert_with(Worker::new), bytes)
        }
        // Only a re-entrant call could find it borrowed; it gets a worker of its own.
        Err(_) => extract::index_counted(grammar, &mut Worker::new(), bytes),
    })
}

/// The syntax facts of the site at `name` of `text` ([`scopes`]), parsed on this thread's worker.
fn site_on_thread(grammar: &Grammar, text: &str, name: ByteRange) -> Option<SiteSyntax> {
    WORKER.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => {
            scopes::site_syntax(grammar, slot.get_or_insert_with(Worker::new), text, name)
        }
        Err(_) => scopes::site_syntax(grammar, &mut Worker::new(), text, name),
    })
}

/// [`site_on_thread`] for each of `names`, over one parse; no facts for a file past
/// [`scopes::MAX_FACTS_BYTES`], as for one site.
fn sites_on_thread(grammar: &Grammar, text: &str, names: &[ByteRange]) -> Vec<Option<SiteSyntax>> {
    if text.len() > scopes::MAX_FACTS_BYTES {
        return vec![None; names.len()];
    }
    WORKER.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => {
            scopes::sites_syntax(grammar, slot.get_or_insert_with(Worker::new), text, names)
        }
        Err(_) => scopes::sites_syntax(grammar, &mut Worker::new(), text, names),
    })
}

/// The tree-sitter `SyntaxIndexer`: parsing runs on the injected [`BlockingPool`] (fnd §4.4).
#[derive(Debug, Clone)]
pub struct JavaIndexer<P> {
    grammar: Arc<Grammar>,
    pool: P,
}

impl<P> JavaIndexer<P> {
    /// An indexer of `grammar` whose jobs run on `pool`.
    pub const fn new(grammar: Arc<Grammar>, pool: P) -> Self {
        Self { grammar, pool }
    }

    /// Indexes `bytes` on the calling thread: what a pool job runs, for callers that already are
    /// on a thread that may block (composition's benchmark, tests).
    pub fn index_blocking(&self, bytes: &[u8]) -> Result<Indexed, SyntaxIndexError> {
        summarise_on_thread(&self.grammar, bytes).0
    }

    /// [`JavaIndexer::index_blocking`], and what it spent of the file's work budget: the
    /// benchmark reports the headroom.
    pub fn index_blocking_with_work(
        &self,
        bytes: &[u8],
    ) -> (Result<Indexed, SyntaxIndexError>, Work) {
        summarise_on_thread(&self.grammar, bytes)
    }

    /// [`SyntaxIndexer::site_syntax`] on the calling thread: for the benchmark and tests.
    pub fn site_syntax_blocking(&self, text: &str, name: ByteRange) -> Option<SiteSyntax> {
        site_on_thread(&self.grammar, text, name)
    }

    /// The same for each of `names` over one parse, on the calling thread: what keeping the facts
    /// in the summary at index time would cost (`just bench-queries` measures it).
    pub fn sites_syntax_blocking(
        &self,
        text: &str,
        names: &[ByteRange],
    ) -> Vec<Option<SiteSyntax>> {
        WORKER.with(|cell| match cell.try_borrow_mut() {
            Ok(mut slot) => scopes::sites_syntax(
                &self.grammar,
                slot.get_or_insert_with(Worker::new),
                text,
                names,
            ),
            Err(_) => scopes::sites_syntax(&self.grammar, &mut Worker::new(), text, names),
        })
    }
}

impl<P: BlockingPool> SyntaxIndexer for JavaIndexer<P> {
    fn language(&self) -> Language {
        Language::Java
    }

    fn grammar(&self) -> GrammarId {
        self.grammar.id()
    }

    fn summariser_version(&self) -> u32 {
        SUMMARISER_VERSION
    }

    fn index(
        &self,
        bytes: Vec<u8>,
    ) -> impl Future<Output = Result<Indexed, SyntaxIndexError>> + Send {
        let grammar = Arc::clone(&self.grammar);
        let job = self
            .pool
            .run(move || summarise_on_thread(&grammar, &bytes).0);
        async move { job.await.unwrap_or(Err(SyntaxIndexError::Interrupted)) }
    }

    fn site_syntax(
        &self,
        text: Arc<FileText>,
        name: ByteRange,
    ) -> impl Future<Output = Option<SiteSyntax>> + Send {
        let grammar = Arc::clone(&self.grammar);
        let job = self
            .pool
            .run(move || site_on_thread(&grammar, &text.text, name));
        async move { job.await.ok().flatten() }
    }

    fn sites_syntax(
        &self,
        text: Arc<FileText>,
        names: Vec<ByteRange>,
    ) -> impl Future<Output = Vec<Option<SiteSyntax>>> + Send {
        let grammar = Arc::clone(&self.grammar);
        let count = names.len();
        let job = self
            .pool
            .run(move || sites_on_thread(&grammar, &text.text, &names));
        async move { job.await.unwrap_or_else(|_| vec![None; count]) }
    }
}
