//! Negative case of `ARCH-process-authority` (fnd §2.4): a crate without the process grant names
//! `std::process::Command`, which clippy with the root `clippy.toml` refuses.

/// Runs `true`; the child is the violation.
pub fn run_child() -> bool {
    std::process::Command::new("true").status().is_ok()
}
