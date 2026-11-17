//! Depends on `wrapped`.

/// Re-exports the wrapped value.
pub fn value() -> u8 {
    wrapped::value()
}
