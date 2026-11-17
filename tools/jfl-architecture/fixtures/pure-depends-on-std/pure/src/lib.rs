#![no_std]
//! Negative case of `ARCH-domain-purity`'s closure half (fnd §2.3): a `#![no_std]` crate whose
//! own code is clean but which depends on a std crate. It builds on the host and fails for
//! `x86_64-unknown-none`.

/// Delegates to the std crate.
pub fn length() -> usize {
    uses_std::length()
}
