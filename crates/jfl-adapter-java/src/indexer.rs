//! [`JavaIndexer`]: the `SyntaxIndexer` port over a [`Grammar`], with one parser per thread,
//! reused for every file that thread indexes, and the last few trees the queries' syntax facts
//! were read from ([`crate::trees`]).

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
use crate::trees::{TREE_CACHE_BYTES, TREE_CACHE_FILES, Trees};

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

/// [`site_on_thread`] for each of `names`, over the tree `trees` keeps for `text` (parsed on this
/// thread's worker when none is kept): the same facts, a walk instead of a parse for a text asked
/// before; none for a file past [`scopes::MAX_FACTS_BYTES`], as for one site.
fn sites_kept(
    grammar: &Grammar,
    trees: &Trees,
    text: &Arc<FileText>,
    names: &[ByteRange],
) -> Vec<Option<SiteSyntax>> {
    if text.text.len() > scopes::MAX_FACTS_BYTES {
        return vec![None; names.len()];
    }
    let parsed = trees.get_or_parse(text, || {
        WORKER.with(|cell| match cell.try_borrow_mut() {
            Ok(mut slot) => {
                scopes::parse_for_facts(grammar, slot.get_or_insert_with(Worker::new), &text.text)
            }
            Err(_) => scopes::parse_for_facts(grammar, &mut Worker::new(), &text.text),
        })
    });
    scopes::facts_of(&parsed, &text.text, names).0
}

/// The tree-sitter `SyntaxIndexer`: parsing runs on the injected [`BlockingPool`] (fnd §4.4).
#[derive(Debug, Clone)]
pub struct JavaIndexer<P> {
    grammar: Arc<Grammar>,
    pool: P,
    trees: Arc<Trees>,
}

impl<P> JavaIndexer<P> {
    /// An indexer of `grammar` whose jobs run on `pool`, keeping the last
    /// [`TREE_CACHE_FILES`] trees of at most [`TREE_CACHE_BYTES`] of text for the queries.
    pub fn new(grammar: Arc<Grammar>, pool: P) -> Self {
        Self::with_trees(grammar, pool, TREE_CACHE_FILES, TREE_CACHE_BYTES)
    }

    /// An indexer that keeps at most `files` trees of at most `bytes` of text (`0`: none), for a
    /// daemon whose memory budget asks for other bounds.
    pub fn with_trees(grammar: Arc<Grammar>, pool: P, files: usize, bytes: usize) -> Self {
        Self {
            grammar,
            pool,
            trees: Arc::new(Trees::new(files, bytes)),
        }
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
        let trees = Arc::clone(&self.trees);
        let job = self
            .pool
            .run(move || sites_kept(&grammar, &trees, &text, &[name]).pop().flatten());
        async move { job.await.ok().flatten() }
    }

    fn sites_syntax(
        &self,
        text: Arc<FileText>,
        names: Vec<ByteRange>,
    ) -> impl Future<Output = Vec<Option<SiteSyntax>>> + Send {
        let grammar = Arc::clone(&self.grammar);
        let trees = Arc::clone(&self.trees);
        let count = names.len();
        let job = self
            .pool
            .run(move || sites_kept(&grammar, &trees, &text, &names));
        async move { job.await.unwrap_or_else(|_| vec![None; count]) }
    }
}
