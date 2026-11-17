//! Encoding and decoding messages: compact JSON in, typed messages out, with the version rule of
//! plan §6.6 applied on the way.
//!
//! Decoding is additive: an unknown member is ignored. A versioned payload ([`Payload::VERSIONED`])
//! of another major is refused as [`DecodeError::MajorMismatch`] whether or not the rest of it
//! still decodes: when the typed decode fails, the bytes are read once more as plain JSON to see
//! whether the version is why, so a v2 message with a retyped field says "another major", not a
//! confusing type error. The successful path parses once.

use alloc::string::String;
use alloc::vec::Vec;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::messages::{Method, Notify, Payload};
use crate::rpc::{Notification, Outcome, Request, RequestId, Response, TwoPointZero};
use crate::version::{MajorMismatch, Version};

/// Why bytes are not the message expected.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// Not JSON, or not the message's shape.
    #[error("malformed message: {0}")]
    Malformed(serde_json::Error),
    /// A versioned payload of another major (plan §6.6).
    #[error(transparent)]
    MajorMismatch(#[from] MajorMismatch),
    /// Another method than the one expected.
    #[error("method {found} where {expected} was expected")]
    UnexpectedMethod {
        /// The method expected.
        expected: &'static str,
        /// The method sent.
        found: String,
    },
}

/// A message that cannot be written as JSON (a map with non-string keys; none of ours).
#[derive(Debug, thiserror::Error)]
#[error("cannot encode a message: {0}")]
pub struct EncodeError(serde_json::Error);

/// The compact JSON of `value`: what goes in a frame.
pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, EncodeError> {
    serde_json::to_vec(value).map_err(EncodeError)
}

/// A `M` request with `id`.
#[must_use]
pub fn request<M: Method>(id: RequestId, params: M::Params) -> Request<M::Params> {
    Request {
        jsonrpc: TwoPointZero,
        id,
        method: M::NAME.into(),
        params,
    }
}

/// An `N` notification.
#[must_use]
pub fn notification<N: Notify>(params: N::Params) -> Notification<N::Params> {
    Notification {
        jsonrpc: TwoPointZero,
        method: N::NAME.into(),
        params,
    }
}

/// A payload alone (a `--json` document, say).
///
/// Pinned by: `tests/compat.rs`.
pub fn decode_payload<T: Payload>(bytes: &[u8]) -> Result<T, DecodeError> {
    match serde_json::from_slice::<T>(bytes) {
        Ok(payload) => check(&payload).map(|()| payload),
        Err(err) => Err(classify::<T>(bytes, None, err)),
    }
}

/// A payload from a JSON value (`lsp_server::Message`'s params or result, ADR 0021).
///
/// Pinned by: `tests/compat.rs::should_refuse_another_major_when_a_payload_comes_as_a_value`.
pub fn payload_from_value<T: Payload>(value: Value) -> Result<T, DecodeError> {
    let found = T::VERSIONED.and_then(|(member, ours)| other_major(value.get(member), ours));
    match T::deserialize(value) {
        Ok(payload) => check(&payload).map(|()| payload),
        Err(err) => Err(found.map_or(DecodeError::Malformed(err), DecodeError::MajorMismatch)),
    }
}

/// An `M` request.
pub fn decode_request<M: Method>(bytes: &[u8]) -> Result<Request<M::Params>, DecodeError> {
    let request = serde_json::from_slice::<Request<M::Params>>(bytes)
        .map_err(|err| classify_call::<M::Params>(bytes, M::NAME, err))?;
    check(&request.params)?;
    expect_method(M::NAME, &request.method)?;
    Ok(request)
}

/// The response to an `M` request; an error response decodes too.
pub fn decode_response<M: Method>(bytes: &[u8]) -> Result<Response<M::Result>, DecodeError> {
    let response = serde_json::from_slice::<Response<M::Result>>(bytes)
        .map_err(|err| classify::<M::Result>(bytes, Some("result"), err))?;
    if let Outcome::Result(result) = &response.outcome {
        check(result)?;
    }
    Ok(response)
}

/// An `N` notification.
pub fn decode_notification<N: Notify>(
    bytes: &[u8],
) -> Result<Notification<N::Params>, DecodeError> {
    let notification = serde_json::from_slice::<Notification<N::Params>>(bytes)
        .map_err(|err| classify_call::<N::Params>(bytes, N::NAME, err))?;
    check(&notification.params)?;
    expect_method(N::NAME, &notification.method)?;
    Ok(notification)
}

fn expect_method(expected: &'static str, found: &str) -> Result<(), DecodeError> {
    if found == expected {
        Ok(())
    } else {
        Err(DecodeError::UnexpectedMethod {
            expected,
            found: found.into(),
        })
    }
}

fn check<T: Payload>(payload: &T) -> Result<(), DecodeError> {
    match (T::VERSIONED, payload.version()) {
        (Some((_, ours)), Some(theirs)) => ours.check(theirs).map_err(DecodeError::from),
        _ => Ok(()),
    }
}

/// Why a typed decode of a request or notification failed: another method when its `method`
/// says so (its params need not decode as ours), else as [`classify`].
fn classify_call<T: Payload>(
    bytes: &[u8],
    expected: &'static str,
    err: serde_json::Error,
) -> DecodeError {
    let method = serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| {
            value
                .get("method")
                .and_then(Value::as_str)
                .map(String::from)
        });
    match method {
        Some(found) if found != expected => DecodeError::UnexpectedMethod { expected, found },
        _ => classify::<T>(bytes, Some("params"), err),
    }
}

/// Why a typed decode failed: another major when the payload's version says so, else the
/// decode error itself.
fn classify<T: Payload>(bytes: &[u8], within: Option<&str>, err: serde_json::Error) -> DecodeError {
    let Some((member, ours)) = T::VERSIONED else {
        return DecodeError::Malformed(err);
    };
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return DecodeError::Malformed(err);
    };
    let payload = match within {
        Some(name) => value.get(name),
        None => Some(&value),
    };
    other_major(payload.and_then(|p| p.get(member)), ours)
        .map_or(DecodeError::Malformed(err), DecodeError::MajorMismatch)
}

fn other_major(version: Option<&Value>, ours: Version) -> Option<MajorMismatch> {
    let theirs = Version::deserialize(version?).ok()?;
    ours.check(theirs).err()
}

/// Decodes `bytes` as `T` with no version rule: for messages that carry no versioned payload.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DecodeError> {
    serde_json::from_slice(bytes).map_err(DecodeError::Malformed)
}
