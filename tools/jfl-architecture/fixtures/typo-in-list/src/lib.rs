//! Negative case of the clippy stage's path check: the crate is clean, its `clippy.toml` names
//! a path that does not exist.

/// The value.
pub fn value() -> u8 {
    1
}
