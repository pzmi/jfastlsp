//! The socket path and its length limit (plan §5.3).
//!
//! A Unix socket's path must fit `sockaddr_un.sun_path` with its terminating NUL: 108 bytes on
//! Linux, 104 on macOS. A path that does not fit is refused with the limit named, never truncated.
//! Plan §5.3's proposed fallback to a short private directory under `/tmp` is not built yet (the
//! IDX-12 card lists it as remaining), so a state directory under a very long path is refused.
//!
//! Pinned by `socket::tests`.

use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::state::StateDir;

/// The longest socket path in bytes: `sun_path` (108 bytes on Linux) less its NUL.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub const MAX_SOCKET_PATH: usize = 107;
/// The longest socket path in bytes: `sun_path` (104 bytes on macOS and the BSDs) less its NUL.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub const MAX_SOCKET_PATH: usize = 103;

/// A socket path too long for `sun_path`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("socket path {} is {len} bytes, over the limit of {limit} bytes: use a shorter --state-dir", path.display())]
pub struct PathTooLong {
    /// The path.
    pub path: PathBuf,
    /// Its length in bytes.
    pub len: usize,
    /// [`MAX_SOCKET_PATH`].
    pub limit: usize,
}

/// A path that fits `sun_path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketPath(PathBuf);

impl SocketPath {
    /// `path`, if it fits.
    pub fn new(path: &Path) -> Result<Self, PathTooLong> {
        let len = path.as_os_str().as_bytes().len();
        if len > MAX_SOCKET_PATH {
            return Err(PathTooLong {
                path: path.to_owned(),
                len,
                limit: MAX_SOCKET_PATH,
            });
        }
        Ok(Self(path.to_owned()))
    }

    /// The daemon's socket in `state`, once every socket the directory holds fits: `sock` and the
    /// longest notify socket ([`crate::state::NOTIFY_NAME_LEN`] bytes of name), so a state
    /// directory accepted here never refuses a spawn's notify path later.
    pub fn of_state(state: &StateDir) -> Result<Self, PathTooLong> {
        fits_state(state.path())?;
        Self::new(&state.socket_path())
    }

    /// The path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// Whether every socket a state directory at `dir` holds fits `sun_path`: the longest is a
/// notify socket.
pub fn fits_state(dir: &Path) -> Result<(), PathTooLong> {
    let longest = dir.join(format!(
        "{}{}",
        crate::state::NOTIFY_PREFIX,
        "0".repeat(crate::state::NOTIFY_NAME_LEN.saturating_sub(2))
    ));
    SocketPath::new(&longest).map(|_| ())
}

impl AsRef<Path> for SocketPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl fmt::Display for SocketPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_refuse_a_state_directory_when_its_notify_socket_would_not_fit() {
        // given: `sock` fits under both; a notify name (14 bytes) fits only under the shorter
        let fits = PathBuf::from(format!("/{}", "s".repeat(MAX_SOCKET_PATH - 16)));
        let near = PathBuf::from(format!("/{}", "s".repeat(MAX_SOCKET_PATH - 10)));
        assert!(SocketPath::new(&near.join("sock")).is_ok());

        // when
        let accepted = fits_state(&fits);
        let refused = fits_state(&near);

        // then
        assert_eq!(accepted, Ok(()));
        let error = refused.unwrap_err();
        assert_eq!(error.limit, MAX_SOCKET_PATH);
        assert!(error.path.starts_with(&near), "{error}");
    }

    #[test]
    fn should_refuse_a_path_naming_the_limit_when_it_does_not_fit_sun_path() {
        // given
        let fits = PathBuf::from(format!("/{}", "a".repeat(MAX_SOCKET_PATH - 1)));
        let over = PathBuf::from(format!("/{}", "a".repeat(MAX_SOCKET_PATH)));

        // when
        let accepted = SocketPath::new(&fits);
        let refused = SocketPath::new(&over);

        // then
        assert_eq!(accepted.map(|p| p.path().to_owned()), Ok(fits));
        let error = refused.unwrap_err();
        assert_eq!(error.limit, MAX_SOCKET_PATH);
        assert_eq!(error.len, MAX_SOCKET_PATH + 1);
        assert!(
            error
                .to_string()
                .contains(&format!("over the limit of {MAX_SOCKET_PATH} bytes")),
            "{error}"
        );
    }
}
