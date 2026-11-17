//! Negative cases for the authority paths a first review found missing from the root list
//! (fnd §2.4): a scoped thread (spawn), `park_timeout` (sleep), `elapsed` (clock) and the
//! environment reads that are not `var`. Clippy with the root `clippy.toml` must refuse each.

use std::time::{Duration, UNIX_EPOCH};

/// Spawns through a scope.
pub fn scoped() {
    std::thread::scope(|scope| {
        scope.spawn(|| ());
    });
}

/// Sleeps without calling `sleep`.
pub fn parked() {
    std::thread::park_timeout(Duration::from_millis(1));
}

/// Reads the clock without calling `now`.
pub fn since_epoch() -> bool {
    UNIX_EPOCH.elapsed().is_ok()
}

/// Reads the environment without calling `var`.
pub fn places() -> bool {
    std::env::home_dir().is_some() && std::env::current_dir().is_ok() && !std::env::temp_dir().as_os_str().is_empty()
}
