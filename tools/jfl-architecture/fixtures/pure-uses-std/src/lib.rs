#![no_std]
//! ARCH-domain-purity (fnd §2.3): a `no_std` crate naming `std::fs` must not compile.

/// Reads a file, which a pure crate may never do.
pub fn read(path: &str) -> usize {
    std::fs::read(path).map_or(0, |bytes| bytes.len())
}
