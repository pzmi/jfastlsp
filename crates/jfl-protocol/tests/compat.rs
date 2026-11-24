//! Compatibility (plan §6.6, fnd §6): a newer minor's unknown members decode (additive), and a
//! payload of another major is refused with a typed error, whether or not the rest of it still
//! decodes. Fixtures in `tests/fixtures/compat/`.

use jfl_protocol::codec::{self, DecodeError, decode_payload, decode_request, decode_response};
use jfl_protocol::messages::{ClientKind, Definition, Handshake, HandshakeParams, Hover, Stop};
use jfl_protocol::{
    MajorMismatch, Outcome, PROTOCOL_VERSION, SCHEMA_VERSION, Version, WireEnvelope,
};

/// The fixtures and their decoding.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) const DEFINITION: &str = include_str!("fixtures/v1/jfl.definition.response.json");
    pub(super) const UNKNOWN_FIELDS: &str =
        include_str!("fixtures/compat/envelope.unknown-fields.json");
    pub(super) const HANDSHAKE_UNKNOWN: &str =
        include_str!("fixtures/compat/handshake.unknown-fields.json");
    pub(super) const MAJOR_2: &str = include_str!("fixtures/compat/envelope.major-2.json");
    pub(super) const MAJOR_2_SAME_SHAPE: &str =
        include_str!("fixtures/compat/envelope.major-2-same-shape.json");
    pub(super) const HANDSHAKE_MAJOR_2: &str =
        include_str!("fixtures/compat/handshake.major-2.json");

    pub(super) const V2: Version = Version { major: 2, minor: 0 };

    pub(super) fn result(text: &str) -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(text)
            .unwrap()
            .get("result")
            .cloned()
            .unwrap()
    }

    pub(super) fn envelope_of(text: &str) -> WireEnvelope {
        match decode_response::<Definition>(text.as_bytes())
            .unwrap()
            .outcome
        {
            Outcome::Result(envelope) => envelope,
            Outcome::Error(error) => panic!("an error response: {error:?}"),
        }
    }
}

use support::*;

#[test]
fn should_decode_an_envelope_when_a_newer_minor_adds_members() {
    // given
    let mut expected = envelope_of(DEFINITION);
    expected.schema_version.minor = 4;

    // when
    let decoded = envelope_of(UNKNOWN_FIELDS);

    // then
    assert_eq!(decoded, expected);
}

#[test]
fn should_decode_a_handshake_when_a_newer_minor_adds_members_and_a_client_kind() {
    // given
    let bytes = HANDSHAKE_UNKNOWN.as_bytes();

    // when
    let request = decode_request::<Handshake>(bytes).unwrap();

    // then
    let HandshakeParams { protocol, client } = request.params;
    assert_eq!(protocol, Version { major: 1, minor: 3 });
    assert_eq!(client.kind, ClientKind::Other);
    assert_eq!(client.version, "0.2.0");
}

#[test]
fn should_refuse_another_major_when_the_shape_changed_too() {
    // given
    let bytes = MAJOR_2.as_bytes();

    // when
    let decoded = decode_response::<Definition>(bytes);

    // then
    assert!(
        matches!(
            decoded,
            Err(DecodeError::MajorMismatch(MajorMismatch {
                ours: SCHEMA_VERSION,
                theirs: V2
            }))
        ),
        "{decoded:?}"
    );
}

#[test]
fn should_refuse_another_major_when_the_shape_is_unchanged() {
    // given
    let bytes = MAJOR_2_SAME_SHAPE.as_bytes();

    // when
    let decoded = decode_response::<Definition>(bytes);

    // then
    assert!(
        matches!(
            decoded,
            Err(DecodeError::MajorMismatch(MajorMismatch {
                ours: SCHEMA_VERSION,
                theirs: V2
            }))
        ),
        "{decoded:?}"
    );
}

#[test]
fn should_refuse_a_handshake_when_the_proxy_speaks_another_major() {
    // given
    let bytes = HANDSHAKE_MAJOR_2.as_bytes();

    // when
    let decoded = decode_request::<Handshake>(bytes);

    // then
    assert!(
        matches!(
            decoded,
            Err(DecodeError::MajorMismatch(MajorMismatch {
                ours: PROTOCOL_VERSION,
                theirs: V2
            }))
        ),
        "{decoded:?}"
    );
}

#[test]
fn should_refuse_another_major_when_a_json_document_is_decoded_alone() {
    // given
    let changed = serde_json::to_vec(&result(MAJOR_2)).unwrap();
    let same_shape = serde_json::to_vec(&result(MAJOR_2_SAME_SHAPE)).unwrap();

    // when
    let changed = decode_payload::<WireEnvelope>(&changed);
    let same_shape = decode_payload::<WireEnvelope>(&same_shape);

    // then
    assert!(
        matches!(changed, Err(DecodeError::MajorMismatch(_))),
        "{changed:?}"
    );
    assert!(
        matches!(same_shape, Err(DecodeError::MajorMismatch(_))),
        "{same_shape:?}"
    );
}

#[test]
fn should_refuse_another_major_when_a_payload_comes_as_a_value() {
    // given
    let changed = result(MAJOR_2);
    let same_shape = result(MAJOR_2_SAME_SHAPE);

    // when
    let changed = codec::payload_from_value::<WireEnvelope>(changed);
    let same_shape = codec::payload_from_value::<WireEnvelope>(same_shape);

    // then
    assert!(
        matches!(changed, Err(DecodeError::MajorMismatch(_))),
        "{changed:?}"
    );
    assert!(
        matches!(same_shape, Err(DecodeError::MajorMismatch(_))),
        "{same_shape:?}"
    );
}

#[test]
fn should_refuse_a_message_when_it_is_another_method() {
    // given
    let bytes = include_bytes!("fixtures/v1/jfl.definition.request.json");

    // when
    let decoded = decode_request::<Hover>(bytes);

    // then
    assert!(
        matches!(
            &decoded,
            Err(DecodeError::UnexpectedMethod { expected: "jfl/hover", found }) if found == "jfl/definition"
        ),
        "{decoded:?}"
    );
}

#[test]
fn should_report_a_malformed_message_when_the_version_is_ours() {
    // given
    let truncated = DEFINITION.get(..DEFINITION.len() / 2).unwrap();
    let retyped = DEFINITION.replace("\"generation\":118", "\"generation\":\"118\"");

    // when
    let truncated = decode_response::<Definition>(truncated.as_bytes());
    let retyped = decode_response::<Definition>(retyped.as_bytes());

    // then
    assert!(
        matches!(truncated, Err(DecodeError::Malformed(_))),
        "{truncated:?}"
    );
    assert!(
        matches!(retyped, Err(DecodeError::Malformed(_))),
        "{retyped:?}"
    );
}

#[test]
fn should_refuse_a_message_as_another_method_when_its_params_do_not_decode_as_ours() {
    // given
    let bytes = include_bytes!("fixtures/v1/jfl.symbols.request.json");

    // when
    let decoded = decode_request::<Hover>(bytes);

    // then
    assert!(
        matches!(
            &decoded,
            Err(DecodeError::UnexpectedMethod { expected: "jfl/hover", found }) if found == "jfl/symbols"
        ),
        "{decoded:?}"
    );
}

#[test]
fn should_keep_the_members_complete_covers_when_a_minor_is_made() {
    // given
    let envelope = envelope_of(DEFINITION);

    // when
    let encoded = serde_json::to_value(&envelope).unwrap();
    let members: Vec<&str> = encoded
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();

    // then
    // `exact` and `possible` are what `complete` covers. A member added to this list by a minor
    // must not carry hits or change what `complete=true` asserts: an older reader drops it and
    // would forward the claim over less than it covers. That change is a new major
    // (src/messages.rs, plan §6.6).
    let mut expected = [
        "complete",
        "exact",
        "freshness",
        "incomplete_because",
        "position_encoding",
        "possible",
        "query",
        "schema_version",
        "target",
        "view",
    ];
    expected.sort_unstable();
    assert_eq!(members, expected);
}

#[test]
fn should_accept_a_stop_when_its_params_are_omitted_or_null() {
    // given
    let messages = [
        r#"{"jsonrpc":"2.0","id":3,"method":"jfl/stop"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"jfl/stop","params":null}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"jfl/stop","params":{"reason":"major 2"}}"#,
    ];

    // when
    let decoded: Vec<bool> = messages
        .iter()
        .map(|text| decode_request::<Stop>(text.as_bytes()).is_ok())
        .collect();
    let position_without_params =
        decode_request::<Hover>(br#"{"jsonrpc":"2.0","id":3,"method":"jfl/hover"}"#);

    // then
    assert_eq!(decoded, [true, true, true]);
    assert!(matches!(
        position_without_params,
        Err(DecodeError::Malformed(_))
    ));
}

#[test]
fn should_read_clamp_as_false_when_it_is_absent_and_write_it_only_when_true() {
    // given
    let absent = r#"{"uri":"file:///w/A.java","line":1,"col":2,"position_encoding":"utf-16"}"#;
    let set =
        r#"{"uri":"file:///w/A.java","line":1,"col":2,"position_encoding":"utf-16","clamp":true}"#;

    // when
    let read_absent: jfl_protocol::messages::PositionParams = serde_json::from_str(absent).unwrap();
    let read_set: jfl_protocol::messages::PositionParams = serde_json::from_str(set).unwrap();

    // then
    assert!(!read_absent.clamp);
    assert!(read_set.clamp);
    assert_eq!(serde_json::to_string(&read_absent).unwrap(), absent);
    assert_eq!(serde_json::to_string(&read_set).unwrap(), set);
}
