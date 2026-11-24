//! `file://` URIs (plan §2.5, §4.1): the form LSP clients name documents and workspace folders by.
//!
//! A [`FileUri`] holds the decoded absolute path, so two spellings of one file (`%41` and `A`,
//! `file:/a` and `file:///a`, `localhost` or no host) compare equal, and it is printed in one
//! canonical form. Parsing is strict where leniency could move a request to another file: a
//! malformed escape, an escaped `/`, a `.` or `..` segment (escaped or not), a host other than
//! `localhost`, a query or a fragment are refused, and the path must be UTF-8. A character a
//! client should have escaped but did not (a space, non-ASCII) is taken as itself: it names the
//! same file either way.
//!
//! jfl serves Unix only (ADR 0006): a Windows drive URI (`file:///c%3A/Users/x`) parses as the
//! POSIX path `/c:/Users/x`, a legal Linux name, and then matches no worktree. A UNC share
//! (`file:////server/share`) is refused as [`UriError::Unc`].
//!
//! Rejected alternatives: the `url` crate brings IDNA tables for hosts a `file` URI never has, and
//! normalises `..` away where we refuse it; `percent-encoding` decodes a malformed `%` as itself,
//! where we refuse it.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::path::{MAX_PATH_LEN, PathError, RelPath, check_components};

/// The longest URI accepted, in bytes: a path of [`MAX_PATH_LEN`] bytes, every byte escaped,
/// plus the scheme and a `localhost` authority. Checked before anything is allocated.
pub const MAX_URI_LEN: usize = MAX_PATH_LEN * 3 + "file://localhost".len();

/// Why a URI is refused. The values carry no part of the URI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UriError {
    /// Longer than [`MAX_URI_LEN`].
    #[error("URI of {len} bytes is longer than {MAX_URI_LEN}")]
    TooLong {
        /// The URI's length in bytes.
        len: usize,
    },
    /// The scheme is not `file`.
    #[error("not a file URI")]
    NotFileScheme,
    /// An authority other than empty or `localhost`: a file on another machine.
    #[error("file URI names a host other than localhost")]
    Host,
    /// The path is empty or relative.
    #[error("file URI path is not absolute")]
    NotAbsolute,
    /// A UNC share (`file:////server/share`): a file on another machine.
    #[error("file URI names a UNC share")]
    Unc,
    /// A `?` query.
    #[error("file URI has a query")]
    Query,
    /// A `#` fragment.
    #[error("file URI has a fragment")]
    Fragment,
    /// A `%` not followed by two hexadecimal digits.
    #[error("malformed percent escape at byte {index}")]
    MalformedEscape {
        /// The byte index of the `%`.
        index: usize,
    },
    /// `%2F`: a separator inside a segment, which no file name holds.
    #[error("escaped `/` in a file URI path")]
    EncodedSeparator,
    /// The decoded path is not UTF-8.
    #[error("file URI path is not UTF-8")]
    InvalidUtf8,
    /// The decoded path is refused as a path.
    #[error(transparent)]
    Path(#[from] PathError),
}

/// An absolute local path, from or for a `file://` URI.
///
/// Pinned by: `uri::tests::should_refuse_a_uri_when_it_could_name_another_file` and
/// `tests/properties.rs::should_parse_what_it_prints_when_a_path_is_valid`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileUri {
    /// `/` for the root, else `/` and checked components, no trailing `/`.
    path: String,
}

impl FileUri {
    /// Parses `file:///path`, `file://localhost/path` or `file:/path` (scheme and host in any
    /// case); a trailing `/` is dropped.
    pub fn parse(uri: &str) -> Result<Self, UriError> {
        if uri.len() > MAX_URI_LEN {
            return Err(UriError::TooLong { len: uri.len() });
        }
        let scheme = uri.get(..5).ok_or(UriError::NotFileScheme)?;
        if !scheme.eq_ignore_ascii_case("file:") {
            return Err(UriError::NotFileScheme);
        }
        let rest = uri.get(5..).ok_or(UriError::NotFileScheme)?;
        let (path, path_at) = match rest.strip_prefix("//") {
            Some(after) => {
                let host_end = after.find('/').unwrap_or(after.len());
                let (host, path) = after.split_at(host_end);
                if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                    return Err(UriError::Host);
                }
                (path, uri.len().saturating_sub(path.len()))
            }
            None => (rest, 5),
        };
        if path.contains('?') {
            return Err(UriError::Query);
        }
        if path.contains('#') {
            return Err(UriError::Fragment);
        }
        let decoded = decode(path, path_at)?;
        if decoded.starts_with("//") {
            return Err(UriError::Unc);
        }
        Self::from_path(&decoded)
    }

    /// An absolute path such as `/home/u/repo/A.java`; a trailing `/` is dropped.
    ///
    /// Pinned by: `uri::tests::should_accept_an_absolute_path_when_its_components_are_plain`.
    pub fn from_path(path: &str) -> Result<Self, UriError> {
        let relative = path.strip_prefix('/').ok_or(UriError::NotAbsolute)?;
        let relative = relative.strip_suffix('/').unwrap_or(relative);
        if relative.is_empty() {
            return Ok(Self {
                path: String::from("/"),
            });
        }
        check_components(relative)?;
        let mut owned = String::with_capacity(relative.len().saturating_add(1));
        owned.push('/');
        owned.push_str(relative);
        Ok(Self { path: owned })
    }

    /// The decoded absolute path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The path of `self` under `root`, or `None` when `self` is not strictly below it.
    ///
    /// Pinned by: `uri::tests::should_relativise_a_uri_when_it_is_under_the_root`.
    #[must_use]
    pub fn relative_to(&self, root: &Self) -> Option<RelPath> {
        let below = if root.path == "/" {
            self.path.strip_prefix('/')?
        } else {
            self.path
                .strip_prefix(root.path.as_str())?
                .strip_prefix('/')?
        };
        if below.is_empty() {
            return None;
        }
        Some(RelPath::from_checked(String::from(below)))
    }

    /// `self/path`; refused only when the result is longer than [`MAX_PATH_LEN`].
    ///
    /// Pinned by: `uri::tests::should_relativise_a_uri_when_it_is_under_the_root`.
    pub fn join(&self, path: &RelPath) -> Result<Self, PathError> {
        let base = if self.path == "/" {
            ""
        } else {
            self.path.as_str()
        };
        let len = base
            .len()
            .saturating_add(1)
            .saturating_add(path.as_str().len());
        if len > MAX_PATH_LEN.saturating_add(1) {
            return Err(PathError::TooLong { len });
        }
        let mut joined = String::with_capacity(len);
        joined.push_str(base);
        joined.push('/');
        joined.push_str(path.as_str());
        Ok(Self { path: joined })
    }

    /// `self` spelled under `to` in place of `from`: `to` for `from` itself, `to/<path>` for a
    /// path below `from`; `None` for any other path, or when the result is too long.
    ///
    /// Pinned by: `uri::tests::should_respell_a_uri_at_or_below_the_root_when_it_is_respelled`.
    #[must_use]
    pub fn respelled(&self, from: &Self, to: &Self) -> Option<Self> {
        if self == from {
            return Some(to.clone());
        }
        to.join(&self.relative_to(from)?).ok()
    }
}

/// Prints `file://` and the path with every byte but the unreserved ones (RFC 3986 §2.3) and `/`
/// escaped, in upper-case hexadecimal: VS Code's form, which every client accepts.
impl fmt::Display for FileUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("file://")?;
        for chunk in self.path.as_bytes().chunk_by(|a, b| plain(*a) == plain(*b)) {
            if chunk.first().is_some_and(|&b| plain(b)) {
                // Plain bytes are ASCII, so the chunk is UTF-8.
                f.write_str(core::str::from_utf8(chunk).map_err(|_| fmt::Error)?)?;
            } else {
                for byte in chunk {
                    write!(f, "%{byte:02X}")?;
                }
            }
        }
        Ok(())
    }
}

/// A byte printed as itself: RFC 3986's unreserved characters and the separator.
const fn plain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
}

/// Percent-decodes `path`, whose first byte is byte `at` of the URI (for error indices).
fn decode(path: &str, at: usize) -> Result<String, UriError> {
    let mut bytes = Vec::with_capacity(path.len());
    let mut rest = path.as_bytes().iter().enumerate();
    while let Some((i, &byte)) = rest.next() {
        if byte != b'%' {
            bytes.push(byte);
            continue;
        }
        let index = at.saturating_add(i);
        let hi = rest.next().and_then(|(_, &h)| hex(h));
        let lo = rest.next().and_then(|(_, &l)| hex(l));
        let (Some(hi), Some(lo)) = (hi, lo) else {
            return Err(UriError::MalformedEscape { index });
        };
        let decoded = (hi << 4) | lo;
        if decoded == b'/' {
            return Err(UriError::EncodedSeparator);
        }
        bytes.push(decoded);
    }
    String::from_utf8(bytes).map_err(|_| UriError::InvalidUtf8)
}

const fn hex(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(digit.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(digit.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn should_decode_a_uri_when_it_escapes_spaces_and_non_ascii() {
        // given
        let uri = "file:///home/u/my%20repo/Caf%C3%A9.java";
        // when
        let parsed = FileUri::parse(uri).unwrap();
        // then
        assert_eq!(parsed.path(), "/home/u/my repo/Café.java");
        assert_eq!(format!("{parsed}"), uri);
    }

    #[test]
    fn should_compare_equal_when_two_spellings_name_one_file() {
        // given
        let spellings = [
            "file:///a/B.java",
            "FILE:///a/B.java",
            "file://localhost/a/B.java",
            "file://LocalHost/a/%42.java",
            "file:/a/B.java",
            "file:///a/B.java/",
        ];
        // when
        let parsed: Vec<_> = spellings
            .iter()
            .map(|s| FileUri::parse(s).unwrap())
            .collect();
        // then
        assert!(parsed.iter().all(|p| p.path() == "/a/B.java"));
        assert_eq!(
            format!("{}", FileUri::parse("file:///").unwrap()),
            "file:///"
        );
    }

    #[test]
    fn should_refuse_a_uri_when_it_could_name_another_file() {
        // given
        let cases = [
            ("http:///a", UriError::NotFileScheme),
            ("file", UriError::NotFileScheme),
            ("file://host/a", UriError::Host),
            ("file://", UriError::NotAbsolute),
            ("file:a/b", UriError::NotAbsolute),
            ("file:///a?x=1", UriError::Query),
            ("file:///a#frag", UriError::Fragment),
            ("file:///a/%2", UriError::MalformedEscape { index: 10 }),
            ("file:///a/%zz", UriError::MalformedEscape { index: 10 }),
            ("file:///a%2Fb", UriError::EncodedSeparator),
            ("file:///a/%FF", UriError::InvalidUtf8),
            (
                "file:///a/../etc",
                UriError::Path(PathError::DotDotComponent),
            ),
            (
                "file:///a/%2E%2E/etc",
                UriError::Path(PathError::DotDotComponent),
            ),
            ("file:///a/./b", UriError::Path(PathError::DotComponent)),
            ("file:///a//b", UriError::Path(PathError::EmptyComponent)),
            ("file:///a%00b", UriError::Path(PathError::Nul)),
            ("file:////server/share", UriError::Unc),
            ("file:///%2Fserver", UriError::EncodedSeparator),
        ];
        // when
        let refused: Vec<_> = cases.iter().map(|(uri, _)| FileUri::parse(uri)).collect();
        // then
        let expected: Vec<_> = cases.iter().map(|&(_, error)| Err(error)).collect();
        assert_eq!(refused, expected);
    }

    #[test]
    fn should_refuse_a_uri_when_it_is_longer_than_the_bound() {
        // given
        let long = format!("file:///{}", "a".repeat(MAX_URI_LEN));
        // when
        let refused = FileUri::parse(&long);
        // then
        assert_eq!(refused, Err(UriError::TooLong { len: long.len() }));
    }

    #[test]
    fn should_accept_an_absolute_path_when_its_components_are_plain() {
        // given
        let path = "/home/u/repo/a b+c.java";
        // when
        let uri = FileUri::from_path(path).unwrap();
        // then
        assert_eq!(format!("{uri}"), "file:///home/u/repo/a%20b%2Bc.java");
        assert_eq!(FileUri::from_path("rel/a"), Err(UriError::NotAbsolute));
        assert_eq!(
            FileUri::from_path("/"),
            Ok(FileUri::parse("file:///").unwrap())
        );
    }

    #[test]
    fn should_relativise_a_uri_when_it_is_under_the_root() {
        // given
        let root = FileUri::parse("file:///repo/wt").unwrap();
        let file = FileUri::parse("file:///repo/wt/src/A.java").unwrap();
        let sibling = FileUri::parse("file:///repo/wt2/src/A.java").unwrap();
        // when
        let relative = file.relative_to(&root).unwrap();
        // then
        assert_eq!(relative.as_str(), "src/A.java");
        assert_eq!(sibling.relative_to(&root), None);
        assert_eq!(root.relative_to(&root), None);
        assert_eq!(root.join(&relative), Ok(file.clone()));
        let fs_root = FileUri::from_path("/").unwrap();
        assert_eq!(
            file.relative_to(&fs_root).unwrap().as_str(),
            "repo/wt/src/A.java"
        );
        assert_eq!(fs_root.join(&relative).unwrap().path(), "/src/A.java");
    }

    #[test]
    fn should_respell_a_uri_at_or_below_the_root_when_it_is_respelled() {
        // given: macOS's temporary directory, spelled through `/var` and as it is
        let canonical = FileUri::from_path("/private/var/t/wt").unwrap();
        let client = FileUri::from_path("/var/t/wt").unwrap();
        let file = FileUri::from_path("/private/var/t/wt/src/A.java").unwrap();
        let sibling = FileUri::from_path("/private/var/t/wt2/A.java").unwrap();

        // when
        let respelled = file.respelled(&canonical, &client);

        // then
        assert_eq!(respelled.unwrap().path(), "/var/t/wt/src/A.java");
        assert_eq!(
            canonical.respelled(&canonical, &client),
            Some(client.clone())
        );
        assert_eq!(sibling.respelled(&canonical, &client), None);
    }
}
