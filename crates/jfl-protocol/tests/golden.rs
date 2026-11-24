//! Goldens (ver §4): every message of protocol v1, both directions, pinned byte for byte in
//! `tests/fixtures/v1/` (compact JSON, one message per file, as it goes in a frame). Each case
//! builds the message from typed values (the envelopes from domain envelopes), encodes it and
//! compares the bytes with the golden, then decodes the golden through the typed decoder and
//! compares the values. A golden changes only in a commit that says why, with its `Impact:` line.

use jfl_domain::{
    Completeness, DeclKind, Envelope, EnvelopeParts, FileUri, Freshness, Generation,
    IncompleteReason, IndexState, Location, ModuleIndex, ModuleName, ObjectId, Position,
    PositionEncoding, PossibleHit, Range, Reason, SymbolId, Target, TargetStatus, UnixMillis, View,
    Why, completeness,
};
use jfl_protocol::codec::{
    self, decode_notification, decode_request, decode_response, notification, request,
};
use jfl_protocol::envelope::{Operation, Query, WireLocation, WireRange, WireTarget, WireView};
use jfl_protocol::messages::{
    self, CallDirection, CallsParams, Cancel, CancelParams, ClientInfo, ClientKind, ContentChange,
    DaemonStatus, DiagnosticSource, Diagnostics, DiagnosticsParams, DiagnosticsResult, DidChange,
    DidChangeParams, DidClose, DidCloseParams, DidOpen, DidOpenParams, Handshake, HandshakeParams,
    HandshakeResult, Hover, HoverResult, NOTIFICATION_METHODS, PositionParams, PublishDiagnostics,
    PublishDiagnosticsParams, REQUEST_METHODS, ReferenceSummary, References, ReferencesParams,
    ServerInfo, Severity, Status, StatusParams, StatusResult, Stop, StopParams, StopResult,
    SymbolFilter, SymbolKind, Symbols, SymbolsParams, SymbolsResult, Sync, SyncParams, SyncResult,
    ViewStatus, WatchDiagnostics, WatchDiagnosticsParams, WireDiagnostic, WireSymbol,
};
use jfl_protocol::{
    ClaimedCompleteness, DocumentUri, ErrorCode, ErrorObject, Method, Notify, Outcome,
    PROTOCOL_VERSION, RequestId, Response, SCHEMA_VERSION, Version, WireEnvelope,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// The goldens, the domain values they are made from and the comparison.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) const WT: &str = "file:///repo/.claude/worktrees/a";
    pub(super) const BILL: &str =
        "file:///repo/.claude/worktrees/a/src/main/java/com/acme/Billing.java";
    pub(super) const SHOP: &str =
        "file:///repo/.claude/worktrees/a/src/main/java/com/acme/Shop.java";
    pub(super) const CART: &str =
        "file:///repo/.claude/worktrees/a/src/main/java/com/acme/Cart.java";
    pub(super) const REFS_URI: &str =
        "file:///repo/.claude/worktrees/a/src/main/java/com/acme/a+b/Billing.java";
    pub(super) const HEAD: &str = "3f2c9a1b4d5e6f708192a3b4c5d6e7f8091a2b3c";
    pub(super) const SYMBOL: &str = "com.acme.Billing#charge(Money)";
    pub(super) const SYNCED_AT: u64 = 1_763_400_000_000;

    macro_rules! golden {
        ($name:literal) => {
            ($name, include_str!(concat!("fixtures/v1/", $name, ".json")))
        };
    }

    /// Every golden of `fixtures/v1/`, by name.
    pub(super) const GOLDENS: &[(&str, &str)] = &[
        golden!("jfl.handshake.request"),
        golden!("jfl.handshake.response"),
        golden!("jfl.definition.request"),
        golden!("jfl.definition.response"),
        golden!("jfl.definition.response.unwatched"),
        golden!("jfl.references.request"),
        golden!("jfl.references.response"),
        golden!("jfl.references.response.other-languages"),
        golden!("jfl.hover.request"),
        golden!("jfl.hover.response"),
        golden!("jfl.symbols.request"),
        golden!("jfl.symbols.response"),
        golden!("jfl.implementations.request"),
        golden!("jfl.implementations.response"),
        golden!("jfl.calls.request"),
        golden!("jfl.calls.response"),
        golden!("jfl.diagnostics.request"),
        golden!("jfl.diagnostics.response"),
        golden!("jfl.status.request"),
        golden!("jfl.status.response"),
        golden!("jfl.sync.request"),
        golden!("jfl.sync.response"),
        golden!("jfl.stop.request"),
        golden!("jfl.stop.response"),
        golden!("jfl.didOpen.notification"),
        golden!("jfl.didChange.notification"),
        golden!("jfl.didClose.notification"),
        golden!("cancelRequest.notification"),
        golden!("jfl.watchDiagnostics.notification"),
        golden!("jfl.publishDiagnostics.notification"),
        golden!("jfl.publishDiagnostics.notification.disk"),
        golden!("error.version-mismatch"),
        golden!("error.method-not-found"),
        golden!("error.no-symbol"),
        golden!("error.request-cancelled"),
        golden!("error.parse"),
    ];

    pub(super) fn text(name: &str) -> &'static str {
        GOLDENS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| *t)
            .unwrap_or_else(|| panic!("no golden {name}"))
    }

    pub(super) fn uri(text: &str) -> DocumentUri {
        DocumentUri::parse(text).unwrap()
    }

    pub(super) fn file(text: &str) -> FileUri {
        FileUri::parse(text).unwrap()
    }

    pub(super) fn range(a: u32, b: u32, c: u32, d: u32) -> Range {
        Range::new(Position::new(a, b), Position::new(c, d)).unwrap()
    }

    pub(super) fn location(text: &str, r: Range) -> Location {
        Location {
            uri: file(text),
            range: r,
        }
    }

    pub(super) fn view(generation: u64, head: Option<&str>) -> View {
        View {
            worktree: file(WT),
            head: head.map(|h| ObjectId::parse(h).unwrap()),
            generation: Generation(generation),
        }
    }

    pub(super) fn exact_target() -> Target {
        Target {
            symbol: SymbolId::parse(SYMBOL).unwrap(),
            status: TargetStatus::Exact,
        }
    }

    pub(super) fn fresh() -> Freshness {
        Freshness {
            synced_at: Some(UnixMillis(SYNCED_AT)),
            pending_events: 0,
        }
    }

    pub(super) fn complete_at(generation: u64) -> Completeness {
        let module =
            ModuleIndex::new(ModuleName::worktree(), Generation(generation), 10, 10, 0, 0).unwrap();
        let index = IndexState::new(Generation(generation), vec![module]).unwrap();
        completeness(&[ModuleName::worktree()], &index)
    }

    pub(super) fn position(text: &str, line: u32, col: u32) -> PositionParams {
        PositionParams {
            uri: uri(text),
            line,
            col,
            position_encoding: PositionEncoding::Utf16,
            clamp: false,
        }
    }

    pub(super) fn wire_view() -> WireView {
        WireView::from(&view(118, Some(HEAD)))
    }

    pub(super) fn definition_envelope() -> WireEnvelope {
        let envelope = Envelope::new(EnvelopeParts {
            view: view(118, Some(HEAD)),
            target: Some(exact_target()),
            exact: vec![location(BILL, range(40, 16, 40, 22))],
            possible: vec![],
            completeness: complete_at(118),
            freshness: fresh(),
            position_encoding: PositionEncoding::Utf16,
        });
        let query = Query {
            op: Operation::Def,
            uri: uri(SHOP),
            line: 12,
            col: 18,
        };
        WireEnvelope::from_domain(&envelope, Some(query))
    }

    /// A definition answered from a view that is not watched and lost events (IDX-09: a view on
    /// a filesystem a watch cannot trust, after an overflow): ADR 0037's two reasons, worded.
    pub(super) fn unwatched_definition_envelope() -> WireEnvelope {
        let envelope = Envelope::new(EnvelopeParts {
            view: view(118, Some(HEAD)),
            target: Some(exact_target()),
            exact: vec![location(BILL, range(40, 16, 40, 22))],
            possible: vec![],
            completeness: complete_at(118)
                .and(IncompleteReason::NotWatched)
                .and(IncompleteReason::WatcherLostEvents),
            freshness: fresh(),
            position_encoding: PositionEncoding::Utf16,
        });
        let query = Query {
            op: Operation::Def,
            uri: uri(SHOP),
            line: 12,
            col: 18,
        };
        WireEnvelope::from_domain(&envelope, Some(query))
    }

    /// References answered from a view with Kotlin, Groovy or other JVM-language sources, which
    /// the syntax index does not read (IDX-16, plan §1.4): any of them may use the target.
    pub(super) fn other_languages_references_envelope() -> WireEnvelope {
        let envelope = Envelope::new(EnvelopeParts {
            view: view(118, Some(HEAD)),
            target: Some(exact_target()),
            exact: vec![location(BILL, range(40, 16, 40, 22))],
            possible: vec![],
            completeness: complete_at(118)
                .and(IncompleteReason::OtherLanguageSources { files: 396 }),
            freshness: fresh(),
            position_encoding: PositionEncoding::Utf16,
        });
        let query = Query {
            op: Operation::Refs,
            uri: uri(SHOP),
            line: 12,
            col: 18,
        };
        WireEnvelope::from_domain(&envelope, Some(query))
    }

    pub(super) fn references_envelope() -> WireEnvelope {
        let reasons = Completeness::because(IncompleteReason::GeneratedSourcesMissing {
            module: ModuleName::new(":api").unwrap(),
            roots: 1,
        })
        .and(IncompleteReason::ModuleNotIndexed {
            module: ModuleName::new(":payments").unwrap(),
        });
        let envelope = Envelope::new(EnvelopeParts {
            view: view(118, Some(HEAD)),
            target: Some(exact_target()),
            exact: vec![
                location(REFS_URI, range(40, 16, 40, 22)),
                location(SHOP, range(12, 16, 12, 22)),
                location(CART, range(7, 9, 7, 15)),
            ],
            possible: vec![
                PossibleHit {
                    location: location(SHOP, range(30, 20, 30, 26)),
                    why: Why::new(Reason::MethodReference),
                },
                PossibleHit {
                    location: location(CART, range(21, 4, 21, 10)),
                    why: Why::with_detail(Reason::ReceiverTypeUnknown, "var in lambda"),
                },
                PossibleHit {
                    location: location(SHOP, range(12, 16, 12, 22)),
                    why: Why::new(Reason::NameAndArity),
                },
            ],
            completeness: reasons,
            freshness: fresh(),
            position_encoding: PositionEncoding::Utf16,
        });
        let query = Query {
            op: Operation::Refs,
            uri: uri(REFS_URI),
            line: 40,
            col: 18,
        };
        WireEnvelope::from_domain(&envelope, Some(query))
    }

    pub(super) fn implementations_envelope() -> WireEnvelope {
        let envelope = Envelope::new(EnvelopeParts {
            view: view(118, Some(HEAD)),
            target: None,
            exact: vec![],
            possible: vec![PossibleHit {
                location: location(CART, range(2, 13, 2, 17)),
                why: Why::new(Reason::NameAndArity),
            }],
            completeness: Completeness::because(IncompleteReason::IndexLoading),
            freshness: Freshness {
                synced_at: None,
                pending_events: 0,
            },
            position_encoding: PositionEncoding::Utf16,
        });
        let query = Query {
            op: Operation::Impls,
            uri: uri(BILL),
            line: 3,
            col: 17,
        };
        WireEnvelope::from_domain(&envelope, Some(query))
    }

    pub(super) fn calls_envelope() -> WireEnvelope {
        let envelope = Envelope::new(EnvelopeParts {
            view: view(1, None),
            target: Some(exact_target()),
            exact: vec![location(SHOP, range(12, 16, 12, 22))],
            possible: vec![],
            completeness: complete_at(1).deadline(),
            freshness: fresh(),
            position_encoding: PositionEncoding::Utf16,
        });
        let query = Query {
            op: Operation::Calls,
            uri: uri(BILL),
            line: 40,
            col: 18,
        };
        WireEnvelope::from_domain(&envelope, Some(query))
    }

    pub(super) fn ok<R>(id: impl Into<RequestId>, result: R) -> Response<R> {
        Response {
            id: Some(id.into()),
            outcome: Outcome::Result(result),
        }
    }

    pub(super) fn error(
        id: Option<RequestId>,
        code: ErrorCode,
        message: &str,
    ) -> Response<serde_json::Value> {
        Response {
            id,
            outcome: Outcome::Error(ErrorObject {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    /// One message's encoding and decoding against its golden.
    pub(super) struct GoldenCase {
        name: &'static str,
        encoded: String,
        decoded_matches: Result<bool, String>,
    }

    pub(super) fn encode_line<T: Serialize>(value: &T) -> String {
        let mut line = String::from_utf8(codec::encode(value).unwrap()).unwrap();
        line.push('\n');
        line
    }

    pub(super) fn compared<T: PartialEq, E: ToString>(
        decoded: Result<T, E>,
        expected: &T,
    ) -> Result<bool, String> {
        decoded.map(|d| &d == expected).map_err(|e| e.to_string())
    }

    pub(super) fn request_case<M: Method>(
        name: &'static str,
        id: impl Into<RequestId>,
        params: M::Params,
    ) -> GoldenCase
    where
        M::Params: PartialEq,
    {
        let expected = request::<M>(id.into(), params);
        GoldenCase {
            name,
            encoded: encode_line(&expected),
            decoded_matches: compared(decode_request::<M>(text(name).as_bytes()), &expected),
        }
    }

    pub(super) fn response_case<M: Method>(
        name: &'static str,
        expected: Response<M::Result>,
    ) -> GoldenCase
    where
        M::Result: PartialEq,
    {
        GoldenCase {
            name,
            encoded: encode_line(&expected),
            decoded_matches: compared(decode_response::<M>(text(name).as_bytes()), &expected),
        }
    }

    pub(super) fn notification_case<N: Notify>(name: &'static str, params: N::Params) -> GoldenCase
    where
        N::Params: PartialEq,
    {
        let expected = notification::<N>(params);
        GoldenCase {
            name,
            encoded: encode_line(&expected),
            decoded_matches: compared(decode_notification::<N>(text(name).as_bytes()), &expected),
        }
    }

    pub(super) fn plain_case<T: Serialize + DeserializeOwned + PartialEq>(
        name: &'static str,
        expected: &T,
    ) -> GoldenCase {
        GoldenCase {
            name,
            encoded: encode_line(expected),
            decoded_matches: compared(codec::decode::<T>(text(name).as_bytes()), expected),
        }
    }

    pub(super) fn assert_goldens(cases: &[GoldenCase]) {
        for case in cases {
            assert_eq!(
                case.encoded,
                text(case.name),
                "{}: encoding differs from the golden",
                case.name
            );
            assert_eq!(
                case.decoded_matches,
                Ok(true),
                "{}: decoding differs",
                case.name
            );
        }
    }
}

use support::*;

#[test]
fn should_match_the_golden_when_the_handshake_is_encoded() {
    // given
    let params = HandshakeParams {
        protocol: PROTOCOL_VERSION,
        client: ClientInfo {
            kind: ClientKind::Lsp,
            version: "0.1.0".into(),
        },
    };
    let result = HandshakeResult {
        protocol: PROTOCOL_VERSION,
        server: ServerInfo {
            version: "0.1.0".into(),
            pid: 4242,
        },
    };
    let mismatch = Response::<serde_json::Value> {
        id: Some(RequestId::Number(0)),
        outcome: Outcome::Error(messages::version_mismatch(
            PROTOCOL_VERSION,
            Version { major: 2, minor: 0 },
        )),
    };

    // when
    let cases = [
        request_case::<Handshake>("jfl.handshake.request", 0, params),
        response_case::<Handshake>("jfl.handshake.response", ok(0, result)),
        plain_case("error.version-mismatch", &mismatch),
    ];

    // then
    assert_goldens(&cases);
}

#[test]
fn should_match_the_golden_when_every_envelope_query_is_encoded() {
    // given
    let references = ReferencesParams {
        at: position(REFS_URI, 40, 18),
        exact_only: false,
    };
    let calls = CallsParams {
        at: position(BILL, 40, 18),
        direction: CallDirection::Incoming,
    };

    // when
    let cases = [
        request_case::<messages::Definition>("jfl.definition.request", 1, position(SHOP, 12, 18)),
        response_case::<messages::Definition>(
            "jfl.definition.response",
            ok(1, definition_envelope()),
        ),
        response_case::<messages::Definition>(
            "jfl.definition.response.unwatched",
            ok(10, unwatched_definition_envelope()),
        ),
        request_case::<References>("jfl.references.request", 2, references),
        response_case::<References>("jfl.references.response", ok(2, references_envelope())),
        response_case::<References>(
            "jfl.references.response.other-languages",
            ok(11, other_languages_references_envelope()),
        ),
        request_case::<messages::Implementations>(
            "jfl.implementations.request",
            5,
            position(BILL, 3, 17),
        ),
        response_case::<messages::Implementations>(
            "jfl.implementations.response",
            ok(5, implementations_envelope()),
        ),
        request_case::<messages::Calls>("jfl.calls.request", 6, calls),
        response_case::<messages::Calls>("jfl.calls.response", ok(6, calls_envelope())),
    ];

    // then
    assert_goldens(&cases);
}

#[test]
fn should_match_the_golden_when_hover_and_symbols_are_encoded() {
    // given
    let hover = HoverResult {
        schema_version: SCHEMA_VERSION,
        position_encoding: PositionEncoding::Utf16,
        view: wire_view(),
        range: Some(WireRange(range(40, 16, 40, 22))),
        target: Some(WireTarget::from(&Target {
            symbol: SymbolId::parse(SYMBOL).unwrap(),
            status: TargetStatus::Ambiguous { overloads: 3 },
        })),
        signature: Some("public Receipt charge(Money amount)".into()),
        documentation: Some("Charges the account.".into()),
        module: Some(":payments".into()),
        references: Some(ReferenceSummary {
            exact: 12,
            possible: 3,
            complete: false,
        }),
        unresolved: None,
    };
    let symbols_params = SymbolsParams {
        worktree: uri(WT),
        query: "Bill".into(),
        kind: Some(SymbolFilter::Class),
        limit: Some(50),
        position_encoding: PositionEncoding::Utf8,
    };
    let symbols = SymbolsResult {
        schema_version: SCHEMA_VERSION,
        position_encoding: PositionEncoding::Utf8,
        view: wire_view(),
        symbols: vec![
            WireSymbol {
                symbol: "com.acme.Billing".into(),
                kind: SymbolKind::from(DeclKind::Class),
                location: WireLocation::from(&location(BILL, range(5, 13, 5, 20))),
            },
            WireSymbol {
                symbol: SYMBOL.into(),
                kind: SymbolKind::from(DeclKind::Method),
                location: WireLocation::from(&location(BILL, range(40, 16, 40, 22))),
            },
        ],
        completeness: ClaimedCompleteness::from(&complete_at(118)),
    };

    // when
    let cases = [
        request_case::<Hover>("jfl.hover.request", "h-1", position(BILL, 40, 18)),
        response_case::<Hover>("jfl.hover.response", ok("h-1", hover)),
        request_case::<Symbols>("jfl.symbols.request", 4, symbols_params),
        response_case::<Symbols>("jfl.symbols.response", ok(4, symbols)),
    ];

    // then
    assert_goldens(&cases);
}

#[test]
fn should_match_the_golden_when_diagnostics_status_sync_and_stop_are_encoded() {
    // given
    let diagnostics = DiagnosticsResult {
        schema_version: SCHEMA_VERSION,
        position_encoding: PositionEncoding::Utf16,
        uri: uri(BILL),
        version: Some(7),
        diagnostics: vec![WireDiagnostic {
            range: WireRange(range(12, 4, 12, 5)),
            severity: Severity::Error,
            source: DiagnosticSource::Syntax,
            code: None,
            message: "';' expected".into(),
        }],
        completeness: ClaimedCompleteness::from(&Completeness::because(
            IncompleteReason::IndexLoading,
        )),
    };
    let partly = Completeness::because(IncompleteReason::PendingChanges { events: 2 })
        .and(IncompleteReason::ModuleStale {
            module: ModuleName::worktree(),
            stale: 3,
        })
        .and(IncompleteReason::ModulePartlyIndexed {
            module: ModuleName::worktree(),
            indexed: 9000,
            files: 9294,
        });
    let status = StatusResult {
        schema_version: SCHEMA_VERSION,
        daemon: DaemonStatus {
            version: "0.1.0".into(),
            pid: 4242,
            protocol: PROTOCOL_VERSION,
        },
        views: vec![ViewStatus {
            view: wire_view(),
            files: 9294,
            indexed: 9000,
            stale: 3,
            pending_events: 2,
            completeness: ClaimedCompleteness::from(&partly),
        }],
    };
    let sync = SyncResult {
        schema_version: SCHEMA_VERSION,
        views: vec![wire_view()],
    };

    // when
    let cases = [
        request_case::<Diagnostics>(
            "jfl.diagnostics.request",
            7,
            DiagnosticsParams {
                uri: uri(BILL),
                position_encoding: PositionEncoding::Utf16,
                deadline_ms: Some(3000),
            },
        ),
        response_case::<Diagnostics>("jfl.diagnostics.response", ok(7, diagnostics)),
        request_case::<Status>("jfl.status.request", 8, StatusParams { worktree: None }),
        response_case::<Status>("jfl.status.response", ok(8, status)),
        request_case::<Sync>("jfl.sync.request", 9, SyncParams { uri: Some(uri(WT)) }),
        response_case::<Sync>("jfl.sync.response", ok(9, sync)),
        request_case::<Stop>("jfl.stop.request", 10, StopParams {}),
        response_case::<Stop>("jfl.stop.response", ok(10, StopResult {})),
    ];

    // then
    assert_goldens(&cases);
}

#[test]
fn should_match_the_golden_when_document_sync_and_cancellation_are_encoded() {
    // given
    let change = DidChangeParams {
        uri: uri(BILL),
        version: 2,
        position_encoding: PositionEncoding::Utf16,
        changes: vec![
            ContentChange {
                range: Some(WireRange(range(2, 6, 2, 13))),
                text: "Invoice".into(),
            },
            ContentChange {
                range: None,
                text: "package com.acme;\n".into(),
            },
        ],
    };

    // when
    let cases = [
        notification_case::<DidOpen>(
            "jfl.didOpen.notification",
            DidOpenParams {
                uri: uri(BILL),
                version: 1,
                text: "package com.acme;\n\nclass Billing {}\n".into(),
            },
        ),
        notification_case::<DidChange>("jfl.didChange.notification", change),
        notification_case::<DidClose>(
            "jfl.didClose.notification",
            DidCloseParams { uri: uri(BILL) },
        ),
        notification_case::<Cancel>(
            "cancelRequest.notification",
            CancelParams {
                id: RequestId::Number(5),
            },
        ),
    ];

    // then
    assert_goldens(&cases);
}

#[test]
fn should_match_the_golden_when_pushed_diagnostics_are_encoded() {
    // given
    let diagnostic = |r: Range, code: &str, message: &str| WireDiagnostic {
        range: WireRange(r),
        severity: Severity::Error,
        source: DiagnosticSource::Syntax,
        code: Some(code.into()),
        message: message.into(),
    };

    // when
    let cases = [
        notification_case::<WatchDiagnostics>(
            "jfl.watchDiagnostics.notification",
            WatchDiagnosticsParams {
                position_encoding: PositionEncoding::Utf16,
            },
        ),
        notification_case::<PublishDiagnostics>(
            "jfl.publishDiagnostics.notification",
            PublishDiagnosticsParams {
                uri: uri(BILL),
                version: Some(3),
                seq: 5,
                position_encoding: PositionEncoding::Utf16,
                diagnostics: vec![
                    diagnostic(range(4, 28, 4, 28), "missing", "';' expected"),
                    diagnostic(range(8, 16, 8, 17), "unexpected", "unexpected text"),
                ],
            },
        ),
        notification_case::<PublishDiagnostics>(
            "jfl.publishDiagnostics.notification.disk",
            PublishDiagnosticsParams {
                uri: uri(BILL),
                version: None,
                seq: 9,
                position_encoding: PositionEncoding::Utf8,
                diagnostics: Vec::new(),
            },
        ),
    ];

    // then
    assert_goldens(&cases);
}

#[test]
fn should_match_the_golden_when_errors_are_encoded() {
    // given
    let responses = [
        (
            "error.method-not-found",
            error(
                Some(RequestId::Number(11)),
                ErrorCode::MethodNotFound,
                "method not found: jfl/typeDefinition",
            ),
        ),
        (
            "error.no-symbol",
            error(
                Some(RequestId::Number(1)),
                ErrorCode::NoSymbol,
                "no symbol at the position",
            ),
        ),
        (
            "error.request-cancelled",
            error(
                Some(RequestId::Number(5)),
                ErrorCode::RequestCancelled,
                "request cancelled",
            ),
        ),
        (
            "error.parse",
            error(None, ErrorCode::ParseError, "parse error"),
        ),
    ];

    // when
    let cases: Vec<GoldenCase> = responses
        .iter()
        .map(|(name, expected)| plain_case(name, expected))
        .collect();

    // then
    assert_goldens(&cases);
}

#[test]
fn should_have_goldens_when_a_method_is_in_the_catalogue() {
    // given
    let names: Vec<&str> = GOLDENS.iter().map(|(name, _)| *name).collect();

    // when
    let missing: Vec<String> = REQUEST_METHODS
        .iter()
        .flat_map(|m| {
            let stem = m.replace('/', ".");
            [format!("{stem}.request"), format!("{stem}.response")]
        })
        .chain(NOTIFICATION_METHODS.iter().map(|m| {
            format!(
                "{}.notification",
                m.trim_start_matches('$')
                    .trim_start_matches('/')
                    .replace('/', ".")
            )
        }))
        .filter(|name| !names.contains(&name.as_str()))
        .collect();

    // then
    assert!(missing.is_empty(), "methods without a golden: {missing:?}");
}

#[test]
fn should_decode_a_golden_through_the_untyped_value_when_an_adapter_holds_one() {
    // given
    let message: serde_json::Value = serde_json::from_str(text("jfl.references.response")).unwrap();
    let result = message.get("result").cloned().unwrap();

    // when
    let envelope = codec::payload_from_value::<WireEnvelope>(result).unwrap();

    // then
    assert_eq!(envelope, references_envelope());
    assert!(!envelope.completeness.complete());
    assert_eq!(
        envelope.query.map(|q| q.uri.as_str().to_owned()).as_deref(),
        Some(REFS_URI)
    );
}
