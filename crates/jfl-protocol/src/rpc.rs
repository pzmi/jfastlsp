//! JSON-RPC 2.0 messages (ADR 0021): the typed form of what `lsp_server::Message` carries
//! untyped in the adapters. Both write the same bytes: `jsonrpc`, then `id`, then `method` and
//! `params`, or `result` or `error`.
//!
//! A request id is a number or a string, as JSON-RPC and LSP allow; an error is an
//! [`ErrorObject`] whose code is an [`ErrorCode`], unknown codes kept rather than refused (a newer
//! daemon may add one).

use alloc::string::String;
use core::fmt;

use serde::de::{self, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// The `"jsonrpc": "2.0"` member: written always, and any other value refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TwoPointZero;

impl Serialize for TwoPointZero {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for TwoPointZero {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Exact;

        impl Visitor<'_> for Exact {
            type Value = TwoPointZero;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("\"2.0\"")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<TwoPointZero, E> {
                if text == "2.0" {
                    Ok(TwoPointZero)
                } else {
                    Err(E::invalid_value(de::Unexpected::Str(text), &self))
                }
            }
        }

        deserializer.deserialize_str(Exact)
    }
}

/// A request id: a number (an `i32`, LSP's `integer`) or a string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RequestId {
    /// A number.
    Number(i32),
    /// A string.
    String(String),
}

impl From<i32> for RequestId {
    fn from(id: i32) -> Self {
        Self::Number(id)
    }
}

impl From<&str> for RequestId {
    fn from(id: &str) -> Self {
        Self::String(id.into())
    }
}

impl Serialize for RequestId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Number(id) => serializer.serialize_i32(*id),
            Self::String(id) => serializer.serialize_str(id),
        }
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct IdVisitor;

        impl Visitor<'_> for IdVisitor {
            type Value = RequestId;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a 32-bit integer or a string")
            }

            fn visit_i64<E: de::Error>(self, id: i64) -> Result<RequestId, E> {
                i32::try_from(id)
                    .map(RequestId::Number)
                    .map_err(|_| E::invalid_value(de::Unexpected::Signed(id), &self))
            }

            fn visit_u64<E: de::Error>(self, id: u64) -> Result<RequestId, E> {
                i32::try_from(id)
                    .map(RequestId::Number)
                    .map_err(|_| E::invalid_value(de::Unexpected::Unsigned(id), &self))
            }

            fn visit_str<E: de::Error>(self, id: &str) -> Result<RequestId, E> {
                Ok(RequestId::String(id.into()))
            }

            fn visit_string<E: de::Error>(self, id: String) -> Result<RequestId, E> {
                Ok(RequestId::String(id))
            }
        }

        deserializer.deserialize_any(IdVisitor)
    }
}

/// A request: `{"jsonrpc":"2.0","id":…,"method":…,"params":…}`. `params` may be omitted or
/// `null` (JSON-RPC 2.0 §4.2), which reads as the params type's own reading of `null`: only
/// [`crate::messages::StopParams`] accepts it.
///
/// Pinned by: `tests/compat.rs::should_accept_a_stop_when_its_params_are_omitted_or_null`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Request<P> {
    /// `"2.0"`.
    pub jsonrpc: TwoPointZero,
    /// The id the response carries back.
    pub id: RequestId,
    /// The method name ([`crate::messages`]).
    pub method: String,
    /// The parameters.
    pub params: P,
}

/// A notification: `{"jsonrpc":"2.0","method":…,"params":…}`, never answered. `params` may be
/// omitted or `null`, as for a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notification<P> {
    /// `"2.0"`.
    pub jsonrpc: TwoPointZero,
    /// The method name ([`crate::messages`]).
    pub method: String,
    /// The parameters.
    pub params: P,
}

/// What a response carries: a result or an error, never both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<R> {
    /// `result`.
    Result(R),
    /// `error`.
    Error(ErrorObject),
}

/// A response: `{"jsonrpc":"2.0","id":…,"result":…}` or `{…,"error":{…}}`. The id is `null`
/// only when the request's own id could not be read (JSON-RPC 2.0 §5); a response without the
/// `id` member is refused.
///
/// Pinned by: `rpc::tests::should_refuse_a_response_when_it_has_both_or_neither_outcome` and
/// `rpc::tests::should_refuse_a_response_when_it_has_no_id_member`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response<R> {
    /// The request's id.
    pub id: Option<RequestId>,
    /// The result or the error.
    pub outcome: Outcome<R>,
}

impl<R: Serialize> Serialize for Response<R> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("Response", 3)?;
        out.serialize_field("jsonrpc", &TwoPointZero)?;
        out.serialize_field("id", &self.id)?;
        match &self.outcome {
            Outcome::Result(result) => out.serialize_field("result", result)?,
            Outcome::Error(error) => out.serialize_field("error", error)?,
        }
        out.end()
    }
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "P: Deserialize<'de>"))]
struct RawRequest<P> {
    #[serde(rename = "jsonrpc")]
    _jsonrpc: TwoPointZero,
    id: RequestId,
    method: String,
    #[serde(default)]
    params: Option<P>,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "P: Deserialize<'de>"))]
struct RawNotification<P> {
    #[serde(rename = "jsonrpc")]
    _jsonrpc: TwoPointZero,
    method: String,
    #[serde(default)]
    params: Option<P>,
}

/// Params that were omitted or `null`, as the params type reads `null`.
fn absent_params<'de, P: Deserialize<'de>, E: de::Error>(params: Option<P>) -> Result<P, E> {
    match params {
        Some(params) => Ok(params),
        None => P::deserialize(Value::Null).map_err(E::custom),
    }
}

impl<'de, P: Deserialize<'de>> Deserialize<'de> for Request<P> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawRequest::<P>::deserialize(deserializer)?;
        Ok(Self {
            jsonrpc: TwoPointZero,
            id: raw.id,
            method: raw.method,
            params: absent_params(raw.params)?,
        })
    }
}

impl<'de, P: Deserialize<'de>> Deserialize<'de> for Notification<P> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawNotification::<P>::deserialize(deserializer)?;
        Ok(Self {
            jsonrpc: TwoPointZero,
            method: raw.method,
            params: absent_params(raw.params)?,
        })
    }
}

/// The `id` member, required (JSON-RPC 2.0 §5): `null` reads as `None`, an absent member is
/// refused, where serde would read it as `None` too.
fn required_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<RequestId>, D::Error> {
    Option::<RequestId>::deserialize(deserializer)
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
struct RawResponse<R> {
    #[serde(rename = "jsonrpc")]
    _jsonrpc: TwoPointZero,
    #[serde(deserialize_with = "required_id")]
    id: Option<RequestId>,
    result: Option<R>,
    error: Option<ErrorObject>,
}

impl<'de, R: Deserialize<'de>> Deserialize<'de> for Response<R> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawResponse::<R>::deserialize(deserializer)?;
        let outcome = match (raw.result, raw.error) {
            (Some(result), None) => Outcome::Result(result),
            (None, Some(error)) => Outcome::Error(error),
            (Some(_), Some(_)) => {
                return Err(de::Error::custom("a response with both result and error"));
            }
            (None, None) => {
                return Err(de::Error::custom(
                    "a response with neither result nor error",
                ));
            }
        };
        Ok(Self {
            id: raw.id,
            outcome,
        })
    }
}

/// A JSON-RPC error code: the standard ones, LSP's, and jfl's own in the range JSON-RPC leaves to
/// the implementation (-32099..=-32000, avoiding LSP's -32002 and -32001).
///
/// Pinned by: `rpc::tests::should_read_back_every_code_when_it_is_written`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// -32700: the frame's body is not JSON.
    ParseError,
    /// -32600: JSON, but not a JSON-RPC message.
    InvalidRequest,
    /// -32601: no such method.
    MethodNotFound,
    /// -32602: the params do not decode.
    InvalidParams,
    /// -32603: the daemon failed.
    InternalError,
    /// -32002 (LSP's `ServerNotInitialized`): a request before the handshake.
    HandshakeRequired,
    /// -32800 (LSP): cancelled by `$/cancelRequest`.
    RequestCancelled,
    /// -32801 (LSP): the document changed while the request ran.
    ContentModified,
    /// -32010: the handshake's protocol major is not the daemon's (plan §6.6).
    VersionMismatch,
    /// -32011: no symbol at the position (the CLI's exit code 1, plan §2.4).
    NoSymbol,
    /// -32012: a position past the end of its line or file (plan §2.3).
    PositionOutOfRange,
    /// -32013: the URI is in no worktree (plan §4.1).
    NotInWorktree,
    /// -32014: the deadline passed with nothing to answer (the CLI's exit code 4, plan §2.7).
    DeadlineExceeded,
    /// Any other code, kept as sent.
    Other(i32),
}

impl ErrorCode {
    /// The number on the wire.
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::ParseError => -32700,
            Self::InvalidRequest => -32600,
            Self::MethodNotFound => -32601,
            Self::InvalidParams => -32602,
            Self::InternalError => -32603,
            Self::HandshakeRequired => -32002,
            Self::RequestCancelled => -32800,
            Self::ContentModified => -32801,
            Self::VersionMismatch => -32010,
            Self::NoSymbol => -32011,
            Self::PositionOutOfRange => -32012,
            Self::NotInWorktree => -32013,
            Self::DeadlineExceeded => -32014,
            Self::Other(code) => code,
        }
    }

    /// The code a number names; an unknown one is [`ErrorCode::Other`].
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code {
            -32700 => Self::ParseError,
            -32600 => Self::InvalidRequest,
            -32601 => Self::MethodNotFound,
            -32602 => Self::InvalidParams,
            -32603 => Self::InternalError,
            -32002 => Self::HandshakeRequired,
            -32800 => Self::RequestCancelled,
            -32801 => Self::ContentModified,
            -32010 => Self::VersionMismatch,
            -32011 => Self::NoSymbol,
            -32012 => Self::PositionOutOfRange,
            -32013 => Self::NotInWorktree,
            -32014 => Self::DeadlineExceeded,
            other => Self::Other(other),
        }
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i32(self.code())
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        i32::deserialize(deserializer).map(Self::from_code)
    }
}

/// A JSON-RPC error: `{"code":…,"message":…,"data":…}`, `data` left out when absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorObject {
    /// What went wrong, as a number.
    pub code: ErrorCode,
    /// What went wrong, in words.
    pub message: String,
    /// Details, by code: [`crate::messages::VersionMismatchData`] for
    /// [`ErrorCode::VersionMismatch`]. A JSON value, so its members are written in name order
    /// (`tests/fixtures/v1/error.version-mismatch.json`), as long as no normal dependency turns
    /// on serde_json's `preserve_order` (the golden fails if one does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_back_every_code_when_it_is_written() {
        // given
        let codes = [
            ErrorCode::ParseError,
            ErrorCode::InvalidRequest,
            ErrorCode::MethodNotFound,
            ErrorCode::InvalidParams,
            ErrorCode::InternalError,
            ErrorCode::HandshakeRequired,
            ErrorCode::RequestCancelled,
            ErrorCode::ContentModified,
            ErrorCode::VersionMismatch,
            ErrorCode::NoSymbol,
            ErrorCode::PositionOutOfRange,
            ErrorCode::NotInWorktree,
            ErrorCode::DeadlineExceeded,
            ErrorCode::Other(7),
        ];

        for code in codes {
            // when
            let back = ErrorCode::from_code(code.code());

            // then
            assert_eq!(back, code);
        }
    }

    #[test]
    fn should_refuse_a_response_when_it_has_both_or_neither_outcome() {
        // given
        let both = r#"{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":1,"message":"x"}}"#;
        let neither = r#"{"jsonrpc":"2.0","id":1}"#;

        // when
        let both = serde_json::from_str::<Response<Value>>(both);
        let neither = serde_json::from_str::<Response<Value>>(neither);

        // then
        assert!(both.is_err());
        assert!(neither.is_err());
    }

    #[test]
    fn should_refuse_a_response_when_it_has_no_id_member() {
        // given
        let missing = r#"{"jsonrpc":"2.0","result":{}}"#;
        let null = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"x"}}"#;

        // when
        let missing = serde_json::from_str::<Response<Value>>(missing);
        let null = serde_json::from_str::<Response<Value>>(null);

        // then
        assert!(missing.is_err());
        assert_eq!(null.map(|r| r.id).ok(), Some(None));
    }

    #[test]
    fn should_refuse_a_message_when_jsonrpc_is_not_two_point_zero() {
        // given
        let text = r#"{"jsonrpc":"1.0","method":"jfl/didClose","params":{}}"#;

        // when
        let decoded = serde_json::from_str::<Notification<Value>>(text);

        // then
        assert!(decoded.is_err());
    }

    #[test]
    fn should_refuse_a_request_id_when_it_is_neither_an_i32_nor_a_string() {
        // given
        let ids = ["2147483648", "1.5", "null", "[]"];

        for text in ids {
            // when
            let id = serde_json::from_str::<RequestId>(text);

            // then
            assert!(id.is_err(), "{text}");
        }
    }
}
