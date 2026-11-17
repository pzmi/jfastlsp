//! LSP 3.17 ↔ `jfl-protocol`, field by field, with the two rules the proxy owes the client.
//!
//! - **URIs are echoed as the client sent them** (LSP-01's decision, plan §2.3). Clients compare
//!   URIs as strings, so `file:///a+b/A.java` and `file:///a%2Bb/A.java`, one file, do not match
//!   in a client that sent the first. A location in a document the client named in the request,
//!   or opened, carries the client's spelling ([`Echo`]); any other carries the daemon's canonical
//!   form. Without the `url` or `fluent-uri` feature, `gen-lsp-types`' `Uri` is the string as
//!   received, so nothing on the way normalises it.
//! - **A column past the end of its line is clamped to the line end** (LSP 3.17, `Position`:
//!   "if the character value is greater than the line length it defaults back to the line
//!   length"), using the length `jfl-domain` reports ([`offset`]). The CLI refuses the same
//!   position (plan §2.3). No arithmetic on positions happens here: conversions stay in
//!   `jfl-domain`.

use std::collections::HashMap;

use gen_lsp_types::{Location, Position, Range, TextDocumentPositionParams, Uri};
use jfl_domain::{ByteOffset, FileUri, LineIndex, PositionEncoding, PositionError, UriError};
use jfl_protocol::DocumentUri;
use jfl_protocol::envelope::{WireEnvelope, WireLocation, WireRange};
use jfl_protocol::messages::PositionParams;

/// The client's spelling of each document it opened (`didOpen`), by the file it names.
///
/// Keyed on the [`FileUri`], so a `didClose` spelled otherwise than its `didOpen` still finds the
/// entry (`jfl_protocol::DocumentUri`'s rule).
#[derive(Debug, Clone, Default)]
pub struct UriBook {
    spellings: HashMap<FileUri, Box<str>>,
}

impl UriBook {
    /// Records how the client spells `uri`'s file; a later spelling replaces an earlier one.
    pub fn open(&mut self, uri: &DocumentUri) {
        self.spellings
            .insert(uri.file().clone(), uri.as_str().into());
    }

    /// Forgets `file`.
    pub fn close(&mut self, file: &FileUri) {
        self.spellings.remove(file);
    }

    /// The client's spelling of `file`, if it opened it.
    #[must_use]
    pub fn spelling(&self, file: &FileUri) -> Option<&str> {
        self.spellings.get(file).map(AsRef::as_ref)
    }
}

/// Where a location's URI text comes from: the document named in the request first, then the
/// documents the client opened, else the canonical form.
#[derive(Debug, Clone, Copy, Default)]
pub struct Echo<'a> {
    /// The document the request names, as sent.
    pub named: Option<&'a DocumentUri>,
    /// The documents the client opened.
    pub opened: Option<&'a UriBook>,
}

impl Echo<'_> {
    /// The text to send for `uri`.
    ///
    /// Pinned by: `translate::tests::should_echo_the_client_spelling_when_it_named_the_file` and
    /// `fixtures/lsp/uri-echo.jsonl`.
    #[must_use]
    pub fn text<'u>(&'u self, uri: &'u DocumentUri) -> &'u str {
        if let Some(named) = self.named
            && named.file() == uri.file()
        {
            return named.as_str();
        }
        self.opened
            .and_then(|book| book.spelling(uri.file()))
            .unwrap_or(uri.as_str())
    }
}

/// The client's position params as `jfl-protocol`'s, the URI kept as sent.
///
/// Pinned by: `translate::tests::should_keep_the_uri_text_when_params_are_translated`.
pub fn position_params(
    params: &TextDocumentPositionParams,
    encoding: PositionEncoding,
) -> Result<PositionParams, UriError> {
    let uri = DocumentUri::parse(&params.text_document.uri.0)?;
    Ok(PositionParams {
        uri,
        line: params.position.line,
        col: params.position.character,
        position_encoding: encoding,
    })
}

/// A wire range as an LSP range, in the same encoding.
#[must_use]
pub fn range(range: WireRange) -> Range {
    let WireRange(range) = range;
    let position = |p: jfl_domain::Position| Position::new(p.line(), p.column());
    Range::new(position(range.start()), position(range.end()))
}

/// A wire location as an LSP location, its URI echoed.
#[must_use]
pub fn location(location: &WireLocation, echo: &Echo<'_>) -> Location {
    Location::new(
        Uri(echo.text(&location.uri).to_owned()),
        range(location.range),
    )
}

/// The envelope's hits as LSP locations: `exact` first, then `possible`, so a client that reads
/// a prefix still sees the proven ones (plan §2.5).
///
/// Pinned by: `translate::tests::should_put_exact_hits_first_when_an_envelope_is_translated`.
#[must_use]
pub fn locations(envelope: &WireEnvelope, echo: &Echo<'_>) -> Vec<Location> {
    let exact = envelope.exact.iter().map(|hit| location(hit, echo));
    let possible = envelope
        .possible
        .iter()
        .map(|hit| Location::new(Uri(echo.text(&hit.uri).to_owned()), range(hit.range)));
    exact.chain(possible).collect()
}

/// The byte offset of an LSP position: a column past the end of its line is the line's end
/// (LSP 3.17); a line past the end of the text, or a column inside a character, is still refused.
///
/// Pinned by: `translate::tests::should_clamp_the_column_when_it_is_past_the_line_end`.
pub fn offset(
    index: &LineIndex,
    position: Position,
    encoding: PositionEncoding,
) -> Result<ByteOffset, PositionError> {
    let at = jfl_domain::Position::new(position.line, position.character);
    match index.offset(at, encoding) {
        Err(PositionError::ColumnPastLineEnd {
            line, line_length, ..
        }) => index.offset(jfl_domain::Position::new(line, line_length), encoding),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gen_lsp_types::TextDocumentIdentifier;

    fn doc(text: &str) -> DocumentUri {
        DocumentUri::parse(text).unwrap()
    }

    fn wire(uri: &str, line: u32) -> WireLocation {
        let at = jfl_domain::Position::new(line, 0);
        WireLocation {
            uri: DocumentUri::from(FileUri::parse(uri).unwrap()),
            range: WireRange(jfl_domain::Range::new(at, at).unwrap()),
        }
    }

    #[test]
    fn should_keep_the_uri_text_when_params_are_translated() {
        // given
        let params = TextDocumentPositionParams::new(
            TextDocumentIdentifier::new(Uri("file:///w/a+b/c@d/A.java".to_owned())),
            Position::new(3, 7),
        );

        // when
        let got = position_params(&params, PositionEncoding::Utf16).unwrap();

        // then
        assert_eq!(got.uri.as_str(), "file:///w/a+b/c@d/A.java");
        assert_eq!(got.uri.file(), doc("file:///w/a%2Bb/c%40d/A.java").file());
        assert_eq!((got.line, got.col), (3, 7));
        assert_eq!(got.position_encoding, PositionEncoding::Utf16);
    }

    #[test]
    fn should_refuse_params_when_the_uri_is_not_a_file_uri() {
        // given
        let params = TextDocumentPositionParams::new(
            TextDocumentIdentifier::new(Uri("https://example.com/A.java".to_owned())),
            Position::new(0, 0),
        );

        // when
        let got = position_params(&params, PositionEncoding::Utf16);

        // then
        assert!(got.is_err());
    }

    #[test]
    fn should_echo_the_client_spelling_when_it_named_the_file() {
        // given
        let named = doc("file:///w/a+b/A.java");
        let mut book = UriBook::default();
        book.open(&doc("file:///w/a%2bb/B.java"));
        let echo = Echo {
            named: Some(&named),
            opened: Some(&book),
        };

        // when
        let texts: Vec<String> = [
            "file:///w/a%2Bb/A.java",
            "file:///w/a+b/B.java",
            "file:///w/a+b/C.java",
        ]
        .iter()
        .map(|uri| location(&wire(uri, 0), &echo).uri.0)
        .collect();

        // then
        assert_eq!(
            texts,
            [
                "file:///w/a+b/A.java",
                "file:///w/a%2bb/B.java",
                "file:///w/a%2Bb/C.java"
            ]
        );
    }

    #[test]
    fn should_forget_a_spelling_when_the_document_is_closed() {
        // given
        let mut book = UriBook::default();
        book.open(&doc("file:///w/a+b/B.java"));

        // when
        book.close(doc("file:///w/a%2Bb/B.java").file());

        // then
        assert_eq!(book.spelling(doc("file:///w/a+b/B.java").file()), None);
    }

    #[test]
    fn should_put_exact_hits_first_when_an_envelope_is_translated() {
        // given
        let json = r#"{"schema_version":{"major":1,"minor":0},"position_encoding":"utf-16","query":null,"view":{"worktree":"file:///w","head":null,"generation":1},"target":null,"exact":[{"uri":"file:///w/E.java","range":[5,0,5,3]}],"possible":[{"uri":"file:///w/P.java","range":[1,2,1,4],"reason":"name_and_arity","why":"name and arity"}],"complete":false,"incomplete_because":["index_loading"],"freshness":{"synced_at":null,"pending_events":0}}"#;
        let envelope: WireEnvelope = serde_json::from_str(json).unwrap();

        // when
        let got = locations(&envelope, &Echo::default());

        // then
        let uris: Vec<&str> = got.iter().map(|l| l.uri.0.as_str()).collect();
        assert_eq!(uris, ["file:///w/E.java", "file:///w/P.java"]);
        assert_eq!(
            got[1].range,
            Range::new(Position::new(1, 2), Position::new(1, 4))
        );
    }

    #[test]
    fn should_clamp_the_column_when_it_is_past_the_line_end() {
        // given
        let index = LineIndex::new("ab\u{1F600}\ncd").unwrap();

        // when
        let clamped = offset(&index, Position::new(0, 99), PositionEncoding::Utf16);
        let inside = offset(&index, Position::new(0, 3), PositionEncoding::Utf16);
        let past_text = offset(&index, Position::new(5, 0), PositionEncoding::Utf16);

        // then
        assert_eq!(
            clamped,
            index.offset(jfl_domain::Position::new(0, 4), PositionEncoding::Utf16)
        );
        assert_eq!(clamped.map(|o| o.get()), Ok(6));
        assert!(matches!(
            inside,
            Err(PositionError::InsideSurrogatePair { .. })
        ));
        assert!(matches!(past_text, Err(PositionError::LinePastEnd { .. })));
    }
}
