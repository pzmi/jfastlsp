//! The JSON-RPC envelope: only `jsonrpc`, `id` and `method` are decoded; `params`, `result` and
//! `error` stay raw JSON (`serde_json`'s `RawValue`) and are forwarded byte for byte unless the
//! multiplexer must rewrite them, so the proxy never alters a payload it does not understand
//! (no number precision lost, no key reordered). Ids are `serde_json` numbers or strings and
//! round-trip exactly. `serde_json` refuses nesting deeper than 128 wherever it builds a value,
//! and skips a raw value without recursion. Pinned by the tests below.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::{RawValue, to_raw_value};

pub use serde_json::{Map, Value, json};

/// One JSON-RPC message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Always `"2.0"` on output.
    #[serde(default = "two")]
    pub jsonrpc: String,
    /// The id; `Some(Value::Null)` when the message carries `"id": null`.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub id: Option<Value>,
    /// The method of a request or notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Parameters, raw.
    #[serde(
        default,
        deserialize_with = "present_raw",
        skip_serializing_if = "Option::is_none"
    )]
    pub params: Option<Box<RawValue>>,
    /// A response's result, raw (`null` is kept).
    #[serde(
        default,
        deserialize_with = "present_raw",
        skip_serializing_if = "Option::is_none"
    )]
    pub result: Option<Box<RawValue>>,
    /// A response's error, raw.
    #[serde(
        default,
        deserialize_with = "present_raw",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<Box<RawValue>>,
}

fn two() -> String {
    "2.0".to_owned()
}

fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

fn present_raw<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Box<RawValue>>, D::Error> {
    Box::<RawValue>::deserialize(d).map(Some)
}

impl Message {
    fn empty() -> Self {
        Self {
            jsonrpc: two(),
            id: None,
            method: None,
            params: None,
            result: None,
            error: None,
        }
    }

    /// A request.
    #[must_use]
    pub fn request(id: Value, method: &str, params: &Value) -> Self {
        Self {
            id: Some(id),
            method: Some(method.to_owned()),
            params: raw(params),
            ..Self::empty()
        }
    }

    /// A notification.
    #[must_use]
    pub fn notification(method: &str, params: &Value) -> Self {
        Self {
            method: Some(method.to_owned()),
            params: raw(params),
            ..Self::empty()
        }
    }

    /// A successful response.
    #[must_use]
    pub fn response(id: Value, result: &Value) -> Self {
        Self {
            id: Some(id),
            result: raw(result).or_else(null_raw),
            ..Self::empty()
        }
    }

    /// An error response with a code and message.
    #[must_use]
    pub fn error(id: Value, code: i32, message: &str) -> Self {
        Self {
            id: Some(id),
            error: raw(&json!({ "code": code, "message": message })),
            ..Self::empty()
        }
    }

    /// The parameters decoded, `None` when absent or not decodable.
    #[must_use]
    pub fn params_value(&self) -> Option<Value> {
        self.params
            .as_ref()
            .and_then(|p| serde_json::from_str(p.get()).ok())
    }

    /// Replaces the parameters.
    pub fn set_params(&mut self, params: &Value) {
        self.params = raw(params);
    }

    /// The result decoded, `None` when absent or not decodable.
    #[must_use]
    pub fn result_value(&self) -> Option<Value> {
        self.result
            .as_ref()
            .and_then(|r| serde_json::from_str(r.get()).ok())
    }

    /// Replaces the result.
    pub fn set_result(&mut self, result: &Value) {
        self.result = raw(result).or_else(null_raw);
    }
}

fn raw(value: &Value) -> Option<Box<RawValue>> {
    to_raw_value(value).ok()
}

fn null_raw() -> Option<Box<RawValue>> {
    raw(&Value::Null)
}

/// Decodes a body.
///
/// # Errors
///
/// When the body is not a JSON object of the envelope's shape.
pub fn parse(body: &[u8]) -> Result<Message, serde_json::Error> {
    serde_json::from_slice(body)
}

/// Encodes a message.
///
/// # Errors
///
/// Never for messages built from parsed or constructed values; kept as a `Result` because
/// `serde_json` declares one.
pub fn encode(msg: &Message) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(msg)
}

/// The top-level `id` of a body that does not decode, found by a single bounded scan: the first
/// key `"id"` at depth one whose value is a number or a plain string. Used to answer the request
/// an undecodable message belongs to instead of leaving it hanging. Pinned by
/// `tests::should_find_the_same_id_as_the_decoder_when_scanning_generated_messages`.
#[must_use]
pub fn scan_id(body: &[u8]) -> Option<Value> {
    let mut depth = 0usize;
    let mut i = 0usize;
    let len = body.len();
    while i < len {
        let b = *body.get(i)?;
        match b {
            b'"' => {
                let end = string_end(body, i)?;
                let is_key = depth == 1 && body.get(i..=end) == Some(&b"\"id\""[..]);
                i = end.checked_add(1)?;
                if is_key {
                    i = skip_ws(body, i);
                    if body.get(i) == Some(&b':') {
                        return scalar(body, skip_ws(body, i.checked_add(1)?));
                    }
                }
                continue;
            }
            b'{' | b'[' => depth = depth.saturating_add(1),
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i = i.checked_add(1)?;
    }
    None
}

/// The index of the closing quote of the string opening at `start`.
fn string_end(body: &[u8], start: usize) -> Option<usize> {
    let mut i = start.checked_add(1)?;
    loop {
        match *body.get(i)? {
            b'\\' => i = i.checked_add(2)?,
            b'"' => return Some(i),
            _ => i = i.checked_add(1)?,
        }
    }
}

fn skip_ws(body: &[u8], mut i: usize) -> usize {
    while matches!(body.get(i), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        i = i.saturating_add(1);
    }
    i
}

fn scalar(body: &[u8], start: usize) -> Option<Value> {
    let end = match *body.get(start)? {
        b'"' => string_end(body, start)?.checked_add(1)?,
        b'-' | b'0'..=b'9' => {
            let rest = body.get(start..)?;
            let n = rest
                .iter()
                .take_while(|c| matches!(c, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
                .count();
            start.checked_add(n)?
        }
        _ => return None,
    };
    serde_json::from_slice(body.get(start..end)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_forward_payloads_byte_for_byte_when_round_tripped() {
        // given: a large integer, key order and a null result
        let body = br#"{"jsonrpc":"2.0","id":18446744073709551615,"result":{"z":123456789012345678901234567890,"a":[1.50,null]}}"#;

        // when
        let msg = parse(body).unwrap();
        let out = encode(&msg).unwrap();

        // then
        assert_eq!(out, body.to_vec());
        let null = parse(br#"{"jsonrpc":"2.0","id":"x","result":null}"#).unwrap();
        assert_eq!(null.result.as_deref().map(RawValue::get), Some("null"));
        assert_eq!(null.id, Some(json!("x")));
    }

    #[test]
    fn should_keep_a_null_id_apart_from_a_missing_one_when_decoding() {
        // given
        let with_null = br#"{"jsonrpc":"2.0","id":null,"error":{"code":1,"message":"m"}}"#;
        let without = br#"{"jsonrpc":"2.0","method":"m"}"#;

        // when
        let with = parse(with_null).unwrap();
        let without = parse(without).unwrap();

        // then
        assert_eq!(with.id, Some(Value::Null));
        assert_eq!(without.id, None);
    }

    #[test]
    fn should_forward_but_not_decode_a_payload_when_it_nests_past_the_limit() {
        // given
        let deep = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"m","params":{}{}}}"#,
            "[".repeat(100_000),
            "]".repeat(100_000)
        );

        // when
        let msg = parse(deep.as_bytes()).unwrap();

        // then: forwarded untouched, but never decoded into a value
        assert!(msg.params.is_some());
        assert!(msg.params_value().is_none());
    }

    #[test]
    fn should_scan_the_top_level_id_when_the_body_does_not_decode() {
        // given: an id after a nested "id", then a broken value
        let body = br#"{"jsonrpc":"2.0","x":{"id":9,"s":"\"id\":8"},"id" : 42,"result":tru}"#;

        // when
        let id = scan_id(body);

        // then
        assert!(parse(body).is_err());
        assert_eq!(id, Some(json!(42)));
        assert_eq!(scan_id(br#"{"id":"a\"b","result":["#), Some(json!("a\"b")));
        assert_eq!(scan_id(br#"{"result":"#), None);
        assert_eq!(scan_id(br#"{"id":"#), None);
    }

    /// A small deterministic generator: messages with nested objects and decoy "id" keys.
    fn generated(seed: u64) -> (Vec<u8>, Value) {
        let mut state = seed;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state >> 33
        };
        let id = if next() % 2 == 0 {
            json!(next())
        } else {
            json!(format!("s{}\"\\{}", next(), next()))
        };
        let mut nested = json!({ "id": next(), "t": format!("\"id\":{}", next()) });
        for _ in 0..(next() % 5) {
            nested = json!({ "id": [nested, "x"], "k": "{[" });
        }
        // Written by hand so the decoy keys come before the real one, in any order.
        let nested = serde_json::to_string(&nested).unwrap();
        let params = if next() % 2 == 0 {
            format!(r#""params":{nested},"#)
        } else {
            String::new()
        };
        let body = format!(
            r#"{{"jsonrpc":"2.0",{params}"id" : {},"result":{nested}}}"#,
            serde_json::to_string(&id).unwrap()
        );
        (body.into_bytes(), id)
    }

    #[test]
    fn should_find_the_same_id_as_the_decoder_when_scanning_generated_messages() {
        for seed in 0..2000 {
            // given
            let (body, id) = generated(seed);

            // when
            let scanned = scan_id(&body);
            let decoded = parse(&body).unwrap().id;

            // then
            assert_eq!(scanned.as_ref(), Some(&id), "seed {seed}");
            assert_eq!(decoded, Some(id), "seed {seed}");
        }
    }

    #[test]
    fn should_never_panic_when_scanning_any_prefix_of_a_message() {
        // given
        let (body, id) = generated(7);

        // when
        let found: Vec<Option<Value>> = (0..=body.len()).map(|cut| scan_id(&body[..cut])).collect();

        // then: no prefix panics, the empty one finds nothing and the whole body finds the id
        assert_eq!(found.last().cloned().flatten(), Some(id));
        assert_eq!(found[0], None);
    }
}
