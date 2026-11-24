//! An open document's text as the client last described it (plan §2.5's document sync): the
//! `didOpen` text, then each `didChange` applied in order, ranged or whole, in the session's
//! negotiated encoding.
//!
//! Every position is converted by `jfl-domain` ([`LineIndex::offset`], through
//! [`crate::translate::offset`], which clamps a column past its line's end to that end as LSP 3.17
//! asks); no arithmetic on positions happens here (`ARCH-position-arithmetic`, fnd §3.3). A change
//! whose range is not in the text (a line past the end, a start after its end, a column inside a
//! character) is refused and leaves the text as it was, so the caller knows the client's text and
//! this one have parted; so is a version that does not increase, and a text that would grow past
//! [`MAX_DOCUMENT_BYTES`].

use gen_lsp_types::{Position, TextDocumentContentChangeEvent};
use jfl_domain::{ByteRange, LineIndex, PositionEncoding, PositionError};
use jfl_protocol::DocumentUri;

use crate::translate::offset;

/// The longest document kept: a sixth of the 16 MiB frame, so its whole text, every byte escaped
/// in JSON at worst as six (`\u0000`), fits the frame it is sent to the daemon in. About twice the
/// largest file of the bench corpus (kafka's 1.6 MB test file).
pub const MAX_DOCUMENT_BYTES: usize = jfl_protocol::frame::DEFAULT_MAX_FRAME_LEN / 6;

/// Why a document did not take what the client sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeError {
    /// A change's range is not in the text.
    Position(PositionError),
    /// The version does not increase (LSP 3.17: "the version number ... will increase after each
    /// change").
    Version {
        /// The document's version.
        had: i32,
        /// The version sent.
        got: i32,
    },
    /// The text would be longer than [`MAX_DOCUMENT_BYTES`].
    TooLarge {
        /// Its length in bytes.
        len: usize,
    },
}

impl From<PositionError> for ChangeError {
    fn from(e: PositionError) -> Self {
        Self::Position(e)
    }
}

impl core::fmt::Display for ChangeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Position(e) => write!(f, "the change does not fit the text: {e}"),
            Self::Version { had, got } => write!(f, "version {got} does not follow {had}"),
            Self::TooLarge { len } => write!(
                f,
                "the text of {len} bytes is over the limit of {MAX_DOCUMENT_BYTES}"
            ),
        }
    }
}

impl std::error::Error for ChangeError {}

/// One open document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The URI as the client sent it.
    pub uri: DocumentUri,
    /// The client's version, as sent.
    pub version: i32,
    text: String,
    lines: LineIndex,
}

impl Document {
    /// The document `didOpen` describes; refused for a text over [`MAX_DOCUMENT_BYTES`].
    pub fn open(uri: DocumentUri, version: i32, text: String) -> Result<Self, ChangeError> {
        if text.len() > MAX_DOCUMENT_BYTES {
            return Err(ChangeError::TooLarge { len: text.len() });
        }
        let lines = LineIndex::new(&text)?;
        Ok(Self {
            uri,
            version,
            text,
            lines,
        })
    }

    /// Its text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Its lines.
    #[must_use]
    pub const fn lines(&self) -> &LineIndex {
        &self.lines
    }

    /// Applies `changes` in order, each to the text the previous one left, and takes `version`.
    /// A change that does not fit the text, a `version` not above the document's, or a text grown
    /// past [`MAX_DOCUMENT_BYTES`] refuses the whole call and leaves the document as it was.
    ///
    /// Pinned by: `document::tests::should_apply_ranged_changes_in_order_when_they_fit` and
    /// `document::tests::should_leave_the_text_when_a_change_does_not_fit`.
    pub fn apply(
        &mut self,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
        encoding: PositionEncoding,
    ) -> Result<(), ChangeError> {
        if version <= self.version {
            return Err(ChangeError::Version {
                had: self.version,
                got: version,
            });
        }
        let mut text = self.text.clone();
        let mut lines = self.lines.clone();
        for change in changes {
            match change {
                TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(whole) => {
                    text = whole.text;
                }
                TextDocumentContentChangeEvent::TextDocumentContentChangePartial(partial) => {
                    let start = offset(&lines, partial.range.start, encoding)?;
                    let end = offset(&lines, partial.range.end, encoding)?;
                    let range = ByteRange::new(start, end)?;
                    let at = |o: jfl_domain::ByteOffset| usize::try_from(o.get()).ok();
                    let (Some(from), Some(to)) = (at(range.start()), at(range.end())) else {
                        return Err(ChangeError::TooLarge { len: text.len() });
                    };
                    text.replace_range(from..to, &partial.text);
                }
            }
            if text.len() > MAX_DOCUMENT_BYTES {
                return Err(ChangeError::TooLarge { len: text.len() });
            }
            lines = LineIndex::new(&text)?;
        }
        self.text = text;
        self.lines = lines;
        self.version = version;
        Ok(())
    }

    /// `position` as the daemon is asked it: a column past its line's end clamped to that end
    /// (LSP 3.17); a line past the end or a column inside a character is refused.
    ///
    /// Pinned by: `document::tests::should_clamp_a_column_past_the_line_end_when_asked`.
    pub fn clamp(
        &self,
        position: Position,
        encoding: PositionEncoding,
    ) -> Result<jfl_domain::Position, PositionError> {
        let at = offset(&self.lines, position, encoding)?;
        self.lines.position(at, encoding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gen_lsp_types::{
        Range, TextDocumentContentChangePartial, TextDocumentContentChangeWholeDocument,
    };

    fn doc(text: &str) -> Document {
        Document::open(
            DocumentUri::parse("file:///w/A.java").unwrap(),
            1,
            text.to_owned(),
        )
        .unwrap()
    }

    fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangePartial::new(
            Range::new(Position::new(start.0, start.1), Position::new(end.0, end.1)),
            None,
            text.to_owned(),
        )
        .into()
    }

    #[test]
    fn should_apply_ranged_changes_in_order_when_they_fit() {
        // given: "😀" is two UTF-16 units and four bytes
        let mut document = doc("class 😀A {\n}\n");

        // when
        let utf16 = document.apply(
            2,
            vec![
                edit((0, 8), (0, 9), "B"),
                edit((1, 0), (1, 0), "  int x;\n"),
            ],
            PositionEncoding::Utf16,
        );
        let mut utf8 = doc("class 😀A {}\n");
        let bytes = utf8.apply(3, vec![edit((0, 10), (0, 11), "C")], PositionEncoding::Utf8);
        let mut whole = doc("class A {}\n");
        let replaced = whole.apply(
            4,
            vec![
                TextDocumentContentChangeWholeDocument::new("class Z {}\n".to_owned()).into(),
                edit((0, 6), (0, 7), "Y"),
            ],
            PositionEncoding::Utf16,
        );

        // then
        assert_eq!(utf16, Ok(()));
        assert_eq!(document.text(), "class 😀B {\n  int x;\n}\n");
        assert_eq!(document.version, 2);
        assert_eq!(bytes, Ok(()));
        assert_eq!(utf8.text(), "class 😀C {}\n");
        assert_eq!(replaced, Ok(()));
        assert_eq!(whole.text(), "class Y {}\n");
    }

    #[test]
    fn should_leave_the_text_when_a_change_does_not_fit() {
        // given
        let mut document = doc("class 😀A {}\n");

        // when
        let inside = document.apply(5, vec![edit((0, 7), (0, 7), "x")], PositionEncoding::Utf16);
        let past = document.apply(
            5,
            vec![edit((0, 0), (0, 1), "k"), edit((9, 0), (9, 0), "x")],
            PositionEncoding::Utf16,
        );
        let backwards = document.apply(5, vec![edit((0, 3), (0, 1), "x")], PositionEncoding::Utf16);
        let stale = document.apply(1, vec![edit((0, 0), (0, 0), "x")], PositionEncoding::Utf16);
        let huge = document.apply(
            6,
            vec![edit((0, 0), (0, 0), &"x".repeat(MAX_DOCUMENT_BYTES))],
            PositionEncoding::Utf16,
        );
        let opened = Document::open(
            DocumentUri::parse("file:///w/B.java").unwrap(),
            1,
            "y".repeat(MAX_DOCUMENT_BYTES.saturating_add(1)),
        );

        // then
        assert!(inside.is_err());
        assert!(past.is_err());
        assert!(backwards.is_err());
        assert_eq!(stale, Err(ChangeError::Version { had: 1, got: 1 }));
        assert!(matches!(huge, Err(ChangeError::TooLarge { .. })));
        assert!(matches!(opened, Err(ChangeError::TooLarge { .. })));
        assert_eq!(document.text(), "class 😀A {}\n");
        assert_eq!(document.version, 1);
    }

    #[test]
    fn should_clamp_a_column_past_the_line_end_when_asked() {
        // given
        let document = doc("a😀\nb\n");

        // when
        let past = document.clamp(Position::new(0, 40), PositionEncoding::Utf16);
        let inside = document.clamp(Position::new(0, 2), PositionEncoding::Utf16);
        let line = document.clamp(Position::new(7, 0), PositionEncoding::Utf16);

        // then
        assert_eq!(past, Ok(jfl_domain::Position::new(0, 3)));
        assert!(inside.is_err());
        assert!(line.is_err());
    }
}
