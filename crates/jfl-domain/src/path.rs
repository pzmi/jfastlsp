//! Worktree-relative paths (plan §4.1): validated so that no path can leave its worktree.
//!
//! A [`RelPath`] is UTF-8, `/`-separated, and holds no empty, `.` or `..` component and no NUL:
//! joined to any root it names a file under that root, never beside or above it. A file whose
//! name is not UTF-8 has no `RelPath`; the adapter that lists it skips it and says so.

use alloc::string::String;
use core::fmt;

/// The longest path accepted, in bytes: Linux's `PATH_MAX`, so a bound on untrusted input that
/// no real file exceeds.
pub const MAX_PATH_LEN: usize = 4096;

/// Why a path is refused. The values carry no part of the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// The empty path: the root itself is not a file under it.
    #[error("empty path")]
    Empty,
    /// Longer than [`MAX_PATH_LEN`].
    #[error("path of {len} bytes is longer than {MAX_PATH_LEN}")]
    TooLong {
        /// The path's length in bytes.
        len: usize,
    },
    /// A relative path that starts with `/`.
    #[error("path is absolute where a relative one is required")]
    Absolute,
    /// Two separators in a row, or one at the end.
    #[error("path has an empty component")]
    EmptyComponent,
    /// A `.` component.
    #[error("path has a `.` component")]
    DotComponent,
    /// A `..` component: it would leave the root.
    #[error("path has a `..` component")]
    DotDotComponent,
    /// A NUL byte, which no file name holds.
    #[error("path holds a NUL byte")]
    Nul,
}

/// Checks the components of a relative path (no leading `/`).
pub(crate) fn check_components(path: &str) -> Result<(), PathError> {
    if path.is_empty() {
        return Err(PathError::Empty);
    }
    if path.len() > MAX_PATH_LEN {
        return Err(PathError::TooLong { len: path.len() });
    }
    if path.starts_with('/') {
        return Err(PathError::Absolute);
    }
    if path.contains('\0') {
        return Err(PathError::Nul);
    }
    for component in path.split('/') {
        match component {
            "" => return Err(PathError::EmptyComponent),
            "." => return Err(PathError::DotComponent),
            ".." => return Err(PathError::DotDotComponent),
            _ => {}
        }
    }
    Ok(())
}

/// A path relative to a worktree root, such as `src/main/java/a/A.java`.
///
/// Pinned by: `path::tests::should_refuse_a_relative_path_when_it_could_escape_its_root` and
/// `tests/properties.rs::should_hold_no_dot_dot_when_a_relative_path_is_accepted`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(String);

impl RelPath {
    /// Validates `path`: non-empty, at most [`MAX_PATH_LEN`] bytes, not absolute, and no empty,
    /// `.` or `..` component and no NUL.
    pub fn new(path: &str) -> Result<Self, PathError> {
        check_components(path)?;
        Ok(Self(String::from(path)))
    }

    /// A path already checked by [`check_components`].
    pub(crate) const fn from_checked(path: String) -> Self {
        Self(path)
    }

    /// The path as text, `/`-separated.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The components, first to last.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// The last component.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// The path without its last component, or `None` for a single component.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        let (parent, _) = self.0.rsplit_once('/')?;
        Some(Self(String::from(parent)))
    }

    /// `self/other`; refused only when the result is longer than [`MAX_PATH_LEN`].
    ///
    /// Pinned by: `path::tests::should_join_two_paths_when_the_result_fits`.
    pub fn join(&self, other: &Self) -> Result<Self, PathError> {
        let len = self.0.len().saturating_add(1).saturating_add(other.0.len());
        if len > MAX_PATH_LEN {
            return Err(PathError::TooLong { len });
        }
        let mut joined = String::with_capacity(len);
        joined.push_str(&self.0);
        joined.push('/');
        joined.push_str(&other.0);
        Ok(Self(joined))
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn should_accept_a_relative_path_when_its_components_are_plain() {
        // given
        let text = "src/main/java/a/A.java";
        // when
        let path = RelPath::new(text).unwrap();
        // then
        assert_eq!(path.as_str(), text);
        assert_eq!(
            path.components().collect::<Vec<_>>(),
            ["src", "main", "java", "a", "A.java"]
        );
        assert_eq!(path.file_name(), "A.java");
        assert_eq!(path.parent().unwrap().as_str(), "src/main/java/a");
        assert_eq!(RelPath::new("A.java").unwrap().parent(), None);
        assert!(RelPath::new("..a/b..c/...").is_ok());
    }

    #[test]
    fn should_refuse_a_relative_path_when_it_could_escape_its_root() {
        // given
        let cases = [
            ("", PathError::Empty),
            ("/etc/passwd", PathError::Absolute),
            ("a//b", PathError::EmptyComponent),
            ("a/b/", PathError::EmptyComponent),
            ("./a", PathError::DotComponent),
            ("a/./b", PathError::DotComponent),
            ("..", PathError::DotDotComponent),
            ("a/../../b", PathError::DotDotComponent),
            ("a/b\0c", PathError::Nul),
        ];
        // when
        let refused: Vec<_> = cases.iter().map(|(text, _)| RelPath::new(text)).collect();
        // then
        let expected: Vec<_> = cases.iter().map(|&(_, error)| Err(error)).collect();
        assert_eq!(refused, expected);
    }

    #[test]
    fn should_refuse_a_relative_path_when_it_is_longer_than_path_max() {
        // given
        let long = "a".repeat(MAX_PATH_LEN.saturating_add(1));
        // when
        let refused = RelPath::new(&long);
        // then
        assert_eq!(refused, Err(PathError::TooLong { len: long.len() }));
    }

    #[test]
    fn should_join_two_paths_when_the_result_fits() {
        // given
        let base = RelPath::new("src/main").unwrap();
        let file = RelPath::new("A.java").unwrap();
        let long = RelPath::new(&"a".repeat(MAX_PATH_LEN)).unwrap();
        // when
        let joined = base.join(&file).unwrap();
        let refused = base.join(&long);
        // then
        assert_eq!(joined.as_str(), "src/main/A.java");
        assert!(matches!(refused, Err(PathError::TooLong { .. })));
    }
}
