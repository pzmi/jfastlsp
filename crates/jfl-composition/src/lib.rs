//! Wiring for the daemon and the proxies: runtime and task supervision and the daemon launcher.
//! Role: root.
//!
//! - [`languages`] (IDX-17): the syntax indexer of each indexed language.

pub mod daemon;
pub mod languages;
