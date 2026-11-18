//! The `syntax_indexer` contract suite (`jfl_testkit::contracts::syntax_indexer`, ver §5) against
//! the tree-sitter adapter with tree-sitter-java-orchard, on the inline pool.

use jfl_adapter_java::{Grammar, GrammarSpec, JavaIndexer};
use jfl_testkit::contracts::syntax_indexer::check;
use jfl_testkit::exec::InlinePool;

#[test]
fn should_keep_the_syntax_indexer_contract_when_orchard_indexes() {
    // given
    let indexer = JavaIndexer::new(Grammar::new(&GrammarSpec::orchard()).unwrap(), InlinePool);
    // when
    let violations = check(&indexer);
    // then
    assert!(violations.is_empty(), "{violations:#?}");
}
