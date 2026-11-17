//! An ordinary std crate: it links std implicitly.

/// A length computed with std's `Vec`.
pub fn length() -> usize {
    std::vec::Vec::<u8>::new().len()
}
