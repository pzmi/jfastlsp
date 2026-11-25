//! Wiring for the daemon and the proxies: runtime and task supervision and the daemon launcher.
//! Role: root.
//!
//! - [`languages`] (IDX-17): the syntax indexer of each indexed language.

pub mod daemon;
pub mod languages;

/// On Linux, every allocation of a binary that links composition (`jfl`, its tests and benches) is
/// mimalloc's, the C code's too: the `override` feature replaces `malloc` and `free` for the whole
/// process, so tree-sitter's parse trees (about three in four of a cold index's allocations) are
/// served by it as well as Rust's. On a cold full index of medium it cut the parse and summary's
/// CPU by about a tenth and the whole view's time by 0.2 to 0.3 s, for about 25 MiB more resident
/// memory (`just bench-views medium cold`; `doc/bench/README.md`, IDX-19). `no_thp`: with
/// transparent huge pages it held 50 to 70 MiB more again, for no gain in speed. Linux only: every
/// measurement was made there, and on macOS `override` would replace the default malloc zone with
/// nothing testing it; macOS keeps the system allocator until a `macos.yml` run says otherwise.
#[cfg(target_os = "linux")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;
