//! The contract suites (ver §5): one per port, parameterised by the implementation under test,
//! run against the testkit fake here and against the real adapter from the adapter's crate.

pub mod process_runner;
pub mod repo_probe;
pub mod source_tree;
