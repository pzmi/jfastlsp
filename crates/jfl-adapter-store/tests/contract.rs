//! The `summary_store` contract suite (`jfl_testkit::contracts::summary_store`, ver §5) against the
//! on-disk store, on the inline pool, in a scratch directory.

#[cfg(test)]
mod support;

use jfl_testkit::contracts::summary_store::check;
use support::{DiskHarness, Scratch};

#[test]
fn should_keep_the_summary_store_contract_when_the_disk_store_answers() {
    // given
    let mut harness = DiskHarness(Scratch::new("contract"));
    // when
    let violations = check(&mut harness);
    // then
    assert!(violations.is_empty(), "{violations:#?}");
}
