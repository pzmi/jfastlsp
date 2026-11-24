//! The syntax indexers by language (IDX-17): the one place that names each indexed language's
//! adapter. The view service reaches them as one
//! [`SyntaxIndexers`](jfl_application::SyntaxIndexers) set and routes each file to its language's
//! (`jfl_application::languages`).
//!
//! With Java the only indexed language, the set is the Java adapter itself (every
//! [`SyntaxIndexer`](jfl_application::SyntaxIndexer) is the set of its one language). A second
//! language replaces [`Indexers`] by a struct holding one indexer per language, whose
//! `SyntaxIndexers` methods `match` on the [`Language`](jfl_domain::Language) and call that
//! language's adapter; [`indexers`] builds it, and the test below fails until every language of
//! the registry has one.
//!
//! The daemon's syntax diagnostics (`daemon::diagnostics`) take their indexer from here too, as
//! `Indexers<()>`, and call it on their own thread. Until that coordinator routes by language
//! (IDX-17's second slice), it parses every open document with this set's Java indexer, as it did
//! before IDX-17.

use jfl_adapter_java::{Grammar, GrammarSpec, JavaIndexer};

/// Every indexed language's indexer, its blocking jobs on `P` (`()` for a caller that runs each
/// parse on its own thread).
pub type Indexers<P> = JavaIndexer<P>;

/// The indexers the daemon serves with, their parses on `pool`. Fails only when a grammar cannot
/// be loaded (a build of the wrong ABI), which the daemon reports and does not start past.
///
/// Pinned by: `languages::tests::should_wire_an_indexer_for_every_indexed_language_when_built`.
pub fn indexers<P>(pool: P) -> Result<Indexers<P>, String> {
    let java = Grammar::new(&GrammarSpec::orchard()).map_err(|e| e.to_string())?;
    Ok(JavaIndexer::new(java, pool))
}

#[cfg(test)]
mod tests {
    use jfl_application::{INDEXED_LANGUAGES, SyntaxIndexers};
    use jfl_testkit::exec::InlinePool;

    use super::*;

    #[test]
    fn should_wire_an_indexer_for_every_indexed_language_when_built() {
        // given
        let set = indexers(InlinePool).unwrap();
        // when
        let unwired: Vec<&str> = INDEXED_LANGUAGES
            .iter()
            .filter(|indexed| set.key_inputs(indexed.language).is_none())
            .map(|indexed| indexed.language.name())
            .collect();
        // then
        assert_eq!(unwired, Vec::<&str>::new());
    }
}
