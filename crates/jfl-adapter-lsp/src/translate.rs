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

use gen_lsp_types::{
    Code, Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, Location, Position, PublishDiagnosticsParams, Range,
    TextDocumentContentChangeEvent, TextDocumentPositionParams, Uri,
};
use jfl_domain::{ByteOffset, FileUri, LineIndex, PositionEncoding, PositionError, UriError};
use jfl_protocol::DocumentUri;
use jfl_protocol::envelope::{WireEnvelope, WireLocation, WireRange, WireTargetStatus};
use jfl_protocol::messages::{self, DiagnosticSource, PositionParams, Severity, WireDiagnostic};
use serde_json::Value;

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
        clamp: true,
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

/// `textDocument/definition`'s result for an envelope (plan §2.5, the LSP-02 card's decision):
/// the `exact` locations; else, when the target is a proved owner's overload set
/// (`ambiguous`), those overloads; else nothing. The other `possible` candidates (same-name
/// members of any type, for an unknown receiver) are never sent: plan §2.2 forbids a same-name
/// match from another type, and LSP's result has no place to say they are unproved. LSP's
/// definition cannot carry completeness either: an empty, incomplete answer is `[]`; hover's
/// status line is LSP's channel for it (LSP-03), and `jfl/definition` and the CLI keep the whole
/// envelope.
///
/// Pinned by: `translate::tests::should_send_exact_hits_or_the_overload_set_and_nothing_else_when_a_definition_is_translated`.
#[must_use]
pub fn definition(envelope: &WireEnvelope, echo: &Echo<'_>) -> Vec<Location> {
    if !envelope.exact.is_empty() {
        return envelope
            .exact
            .iter()
            .map(|hit| location(hit, echo))
            .collect();
    }
    let overloads = envelope
        .target
        .as_ref()
        .is_some_and(|t| t.status == WireTargetStatus::Ambiguous);
    if overloads {
        return envelope
            .possible
            .iter()
            .map(|hit| {
                location(
                    &WireLocation {
                        uri: hit.uri.clone(),
                        range: hit.range,
                    },
                    echo,
                )
            })
            .collect();
    }
    Vec::new()
}

/// The encoding a session uses, from `initialize`'s params: `utf-8` when the client offers it in
/// `capabilities.general.positionEncodings`, `utf-16` otherwise (plan §2.3; `jfl-domain`'s
/// [`PositionEncoding::negotiate`]). Params of any shape are read: what is not an offer is none.
///
/// Pinned by: `translate::tests::should_pick_utf8_only_when_the_client_offers_it` and
/// `fixtures/lsp/position-encoding-utf8.jsonl`.
#[must_use]
pub fn negotiate(initialize: &Value) -> PositionEncoding {
    let offered = initialize
        .pointer("/capabilities/general/positionEncodings")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    PositionEncoding::negotiate(offered)
}

/// Whether the client takes pushed diagnostics: its `initialize` declares
/// `textDocument.publishDiagnostics` (LSP 3.17's `PublishDiagnosticsClientCapabilities`). A client
/// that declares nothing gets none, so a session that never asked for diagnostics is sent no
/// notification it did not expect (LSP-04's decision; Claude Code's capabilities are E0's to
/// confirm, plan §9.2).
///
/// Pinned by: `translate::tests::should_want_diagnostics_only_when_the_client_declares_them`.
#[must_use]
pub fn wants_diagnostics(initialize: &Value) -> bool {
    initialize
        .pointer("/capabilities/textDocument/publishDiagnostics")
        .is_some_and(Value::is_object)
}

/// `textDocument/publishDiagnostics`'s params for the daemon's publication: the URI as the
/// client opened it ([`Echo`]), the version as the daemon tagged it (none for the disk's
/// content), and each diagnostic with its range as computed (the daemon counts columns in the
/// negotiated encoding, on the very text it compiled).
///
/// Pinned by: `translate::tests::should_carry_the_version_and_ranges_when_diagnostics_are_published`.
#[must_use]
pub fn publish_diagnostics(
    published: &messages::PublishDiagnosticsParams,
    echo: &Echo<'_>,
) -> PublishDiagnosticsParams {
    let diagnostics = published.diagnostics.iter().map(diagnostic).collect();
    PublishDiagnosticsParams::new(
        Uri(echo.text(&published.uri).to_owned()),
        published.version,
        diagnostics,
    )
}

/// `textDocument/publishDiagnostics`'s params that clear `uri`'s diagnostics (a close, plan
/// §3.2): the URI as the client sent it, no version, no diagnostic.
///
/// Pinned by: `translate::tests::should_carry_the_version_and_ranges_when_diagnostics_are_published`.
#[must_use]
pub fn cleared(uri: &DocumentUri) -> PublishDiagnosticsParams {
    PublishDiagnosticsParams::new(Uri(uri.as_str().to_owned()), None, Vec::new())
}

fn diagnostic(d: &WireDiagnostic) -> Diagnostic {
    let severity = match d.severity {
        Severity::Error => DiagnosticSeverity::Error,
        Severity::Warning => DiagnosticSeverity::Warning,
        Severity::Information => DiagnosticSeverity::Information,
        Severity::Hint | Severity::Other => DiagnosticSeverity::Hint,
    };
    let source = match d.source {
        DiagnosticSource::Syntax => "jfl syntax",
        DiagnosticSource::Javac => "javac",
        DiagnosticSource::Other => "jfl",
    };
    Diagnostic {
        range: range(d.range),
        severity: Some(severity),
        code: d.code.clone().map(Code::String),
        source: Some(source.to_owned()),
        message: d.message.clone().into(),
        ..Diagnostic::default()
    }
}

/// What the server can do, as `initialize` answers it: capabilities only for what exists (the
/// negotiated encoding, open and close with incremental changes, definition), and the server's
/// name and version.
///
/// Pinned by: `fixtures/lsp/lifecycle-shutdown-exit.jsonl`.
#[must_use]
pub fn initialize_result(encoding: PositionEncoding, version: &str) -> Value {
    serde_json::json!({
        "capabilities": {
            "positionEncoding": encoding.lsp_name(),
            "textDocumentSync": { "openClose": true, "change": 2 },
            "definitionProvider": true,
        },
        "serverInfo": { "name": "jfl", "version": version },
    })
}

/// `textDocument/didOpen`'s document: its URI as sent, its version and text.
#[must_use]
pub fn did_open(params: Value) -> Option<(DocumentUri, i32, String)> {
    let params = serde_json::from_value::<DidOpenTextDocumentParams>(params).ok()?;
    let item = params.text_document;
    Some((
        DocumentUri::parse(&item.uri.0).ok()?,
        item.version,
        item.text,
    ))
}

/// `textDocument/didChange`'s document, its new version and its changes, in order.
#[must_use]
pub fn did_change(
    params: Value,
) -> Option<(DocumentUri, i32, Vec<TextDocumentContentChangeEvent>)> {
    let params = serde_json::from_value::<DidChangeTextDocumentParams>(params).ok()?;
    let uri = DocumentUri::parse(&params.text_document.text_document_identifier.uri.0).ok()?;
    Some((uri, params.text_document.version, params.content_changes))
}

/// `textDocument/didClose`'s document.
#[must_use]
pub fn did_close(params: Value) -> Option<DocumentUri> {
    let params = serde_json::from_value::<DidCloseTextDocumentParams>(params).ok()?;
    DocumentUri::parse(&params.text_document.uri.0).ok()
}

/// A position request's document and position (`textDocument/definition` and the others).
#[must_use]
pub fn text_document_position(params: Value) -> Option<(DocumentUri, Position)> {
    let params = serde_json::from_value::<TextDocumentPositionParams>(params).ok()?;
    let uri = DocumentUri::parse(&params.text_document.uri.0).ok()?;
    Some((uri, params.position))
}

/// The byte offset of an LSP position: a column past the end of its line is the line's end
/// (LSP 3.17, `jfl-domain`'s [`LineIndex::clamp`]); a line past the end of the text, or a column
/// inside a character, is still refused.
///
/// Pinned by: `translate::tests::should_clamp_the_column_when_it_is_past_the_line_end`.
pub fn offset(
    index: &LineIndex,
    position: Position,
    encoding: PositionEncoding,
) -> Result<ByteOffset, PositionError> {
    let at = index.clamp(
        jfl_domain::Position::new(position.line, position.character),
        encoding,
    )?;
    index.offset(at, encoding)
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
    fn should_pick_utf8_only_when_the_client_offers_it() {
        // given
        let offers = [
            serde_json::json!({"capabilities": {"general": {"positionEncodings": ["utf-16", "utf-8"]}}}),
            serde_json::json!({"capabilities": {"general": {"positionEncodings": ["utf-32", "utf-16"]}}}),
            serde_json::json!({"capabilities": {}}),
            serde_json::json!({"capabilities": {"general": {"positionEncodings": "utf-8"}}}),
            Value::Null,
        ];

        // when
        let picked: Vec<PositionEncoding> = offers.iter().map(negotiate).collect();

        // then
        assert_eq!(
            picked,
            [
                PositionEncoding::Utf8,
                PositionEncoding::Utf16,
                PositionEncoding::Utf16,
                PositionEncoding::Utf16,
                PositionEncoding::Utf16,
            ]
        );
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
    fn should_send_exact_hits_or_the_overload_set_and_nothing_else_when_a_definition_is_translated()
    {
        // given
        let envelope = |target: &str, exact: &str| -> WireEnvelope {
            serde_json::from_str(&format!(
                r#"{{"schema_version":{{"major":1,"minor":0}},"position_encoding":"utf-16","query":null,"view":{{"worktree":"file:///w","head":null,"generation":1}},"target":{target},"exact":[{exact}],"possible":[{{"uri":"file:///w/P.java","range":[1,2,1,4],"reason":"overload_not_proved","why":"overload not proved"}}],"complete":false,"incomplete_because":["deadline"],"freshness":{{"synced_at":null,"pending_events":0}}}}"#
            ))
            .unwrap()
        };
        let hit = r#"{"uri":"file:///w/E.java","range":[5,0,5,3]}"#;
        let exact = envelope(r#"{"symbol":"a.E#m()","status":"exact"}"#, hit);
        let overloads = envelope(
            r#"{"symbol":"a.P#m()","status":"ambiguous","overloads":1}"#,
            "",
        );
        let unresolved = envelope("null", "");

        // when
        let uris = |e: &WireEnvelope| -> Vec<String> {
            definition(e, &Echo::default())
                .into_iter()
                .map(|l| l.uri.0)
                .collect()
        };

        // then: never the possible candidates of an unproved receiver
        assert_eq!(uris(&exact), ["file:///w/E.java"]);
        assert_eq!(uris(&overloads), ["file:///w/P.java"]);
        assert!(uris(&unresolved).is_empty());
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
    #[test]
    fn should_want_diagnostics_only_when_the_client_declares_them() {
        // given
        let declared = serde_json::json!({"capabilities": {"textDocument": {"publishDiagnostics": {"versionSupport": true}}}});
        let bare = serde_json::json!({"capabilities": {"textDocument": {}}});
        let none = serde_json::json!({"capabilities": {}});

        // when
        let wanted = [declared, bare, none].map(|p| wants_diagnostics(&p));

        // then
        assert_eq!(wanted, [true, false, false]);
    }

    #[test]
    fn should_carry_the_version_and_ranges_when_diagnostics_are_published() {
        // given
        let sent = DocumentUri::parse("file:///w/a+b/A.java").unwrap();
        let mut book = UriBook::default();
        book.open(&sent);
        let at = |a, b, c, d| {
            WireRange(
                jfl_domain::Range::new(
                    jfl_domain::Position::new(a, b),
                    jfl_domain::Position::new(c, d),
                )
                .unwrap(),
            )
        };
        let published = messages::PublishDiagnosticsParams {
            uri: DocumentUri::parse("file:///w/a%2Bb/A.java").unwrap(),
            version: Some(4),
            seq: 3,
            position_encoding: PositionEncoding::Utf16,
            diagnostics: vec![WireDiagnostic {
                range: at(2, 3, 2, 3),
                severity: Severity::Error,
                source: DiagnosticSource::Syntax,
                code: Some("missing".to_owned()),
                message: "';' expected".to_owned(),
            }],
        };
        let disk = messages::PublishDiagnosticsParams {
            version: None,
            diagnostics: Vec::new(),
            ..published.clone()
        };
        let echo = Echo {
            named: None,
            opened: Some(&book),
        };

        // when
        let with = serde_json::to_value(publish_diagnostics(&published, &echo)).unwrap();
        let without = serde_json::to_value(publish_diagnostics(&disk, &echo)).unwrap();

        // then
        assert_eq!(
            with,
            serde_json::json!({
                "uri": "file:///w/a+b/A.java",
                "version": 4,
                "diagnostics": [{
                    "range": {"start": {"line": 2, "character": 3}, "end": {"line": 2, "character": 3}},
                    "severity": 1,
                    "code": "missing",
                    "source": "jfl syntax",
                    "message": "';' expected"
                }]
            })
        );
        assert_eq!(
            without,
            serde_json::json!({"uri": "file:///w/a+b/A.java", "diagnostics": []})
        );
        assert_eq!(
            serde_json::to_value(cleared(&sent)).unwrap(),
            serde_json::json!({"uri": "file:///w/a+b/A.java", "diagnostics": []})
        );
    }
}
