//! The request seam: what the daemon answers once a connection is open.
//!
//! The host answers the handshake and `jfl/stop` itself; every other request goes to a
//! [`Service`], with its connection's overlays. The view registry's shell (IDX-09) and the LSP
//! proxy's queries (LSP-02) plug in here. Until then [`Unwired`] answers `jfl/status` with the
//! daemon and no view, and every query with an error that says nothing is wired: it never claims
//! `complete=true`, having no view to prove it at (ADR 0014). Results are serialized from the
//! typed struct straight to the frame body (plan §5.3).
//!
//! Pinned by `service::tests`.

use std::collections::BTreeMap;

use jfl_adapter_ipc::Call;
use jfl_protocol::codec;
use jfl_protocol::messages::{
    DaemonStatus, Method, REQUEST_METHODS, ServerInfo, Status, StatusParams, StatusResult,
};
use jfl_protocol::rpc::{ErrorCode, ErrorObject, Outcome, RequestId, Response};
use jfl_protocol::{DocumentUri, PROTOCOL_VERSION, SCHEMA_VERSION};
use serde::Serialize;

/// An open document's overlay, as the client last sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    /// The client's version, kept as sent (never renumbered).
    pub version: i32,
    /// The text after the last full-text change.
    pub text: String,
}

/// What a request's handler sees of the daemon and the request's connection.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The daemon.
    pub server: &'a ServerInfo,
    /// The connection's open documents.
    pub overlays: &'a BTreeMap<DocumentUri, Overlay>,
}

/// Answers the requests the host does not.
pub trait Service {
    /// The serialized response to `call`.
    fn answer(&mut self, call: &Call, context: &Context<'_>) -> Vec<u8>;
}

/// The service until the views are wired: status without views, an error for every query.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unwired;

/// What [`Unwired`] answers a query with.
pub const UNWIRED: &str = "no view is wired into the daemon yet";

impl Service for Unwired {
    fn answer(&mut self, call: &Call, context: &Context<'_>) -> Vec<u8> {
        if call.method == Status::NAME {
            return match codec::payload_from_value::<StatusParams>(call.params.clone()) {
                Ok(_) => result_body(
                    call.id.clone(),
                    &StatusResult {
                        schema_version: SCHEMA_VERSION,
                        daemon: DaemonStatus {
                            version: context.server.version.clone(),
                            pid: context.server.pid,
                            protocol: PROTOCOL_VERSION,
                        },
                        views: Vec::new(),
                    },
                ),
                Err(e) => error_body(
                    Some(call.id.clone()),
                    ErrorCode::InvalidParams,
                    &e.to_string(),
                ),
            };
        }
        if REQUEST_METHODS.contains(&call.method.as_str()) {
            return error_body(Some(call.id.clone()), ErrorCode::InternalError, UNWIRED);
        }
        error_body(
            Some(call.id.clone()),
            ErrorCode::MethodNotFound,
            "no such method",
        )
    }
}

/// A response carrying `result`, serialized on the typed path.
pub fn result_body<R: Serialize>(id: RequestId, result: &R) -> Vec<u8> {
    let response = Response {
        id: Some(id.clone()),
        outcome: Outcome::Result(result),
    };
    codec::encode(&response)
        .unwrap_or_else(|e| error_body(Some(id), ErrorCode::InternalError, &e.to_string()))
}

/// An error response.
pub fn error_body(id: Option<RequestId>, code: ErrorCode, message: &str) -> Vec<u8> {
    let response: Response<()> = Response {
        id,
        outcome: Outcome::Error(ErrorObject {
            code,
            message: message.to_owned(),
            data: None,
        }),
    };
    // A unit result and a string message always serialize.
    codec::encode(&response).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jfl_protocol::messages::Definition;

    fn call(method: &str, params: serde_json::Value) -> Call {
        Call {
            id: RequestId::Number(3),
            method: method.to_owned(),
            params,
        }
    }

    #[test]
    fn should_answer_status_without_a_view_and_never_claim_completeness_when_unwired() {
        // given
        let server = ServerInfo {
            version: "0.1.0".to_owned(),
            pid: 9,
        };
        let overlays = BTreeMap::new();
        let context = Context {
            server: &server,
            overlays: &overlays,
        };

        // when
        let status = Unwired.answer(
            &call(Status::NAME, serde_json::json!({"worktree": null})),
            &context,
        );
        let definition = Unwired.answer(&call(Definition::NAME, serde_json::json!({})), &context);
        let unknown = Unwired.answer(&call("jfl/nothing", serde_json::Value::Null), &context);

        // then
        let status = codec::decode_response::<Status>(&status).unwrap();
        assert!(
            matches!(&status.outcome, Outcome::Result(r) if r.views.is_empty() && r.daemon.pid == 9)
        );
        let definition = codec::decode_response::<Definition>(&definition).unwrap();
        assert!(
            matches!(&definition.outcome, Outcome::Error(e) if e.code == ErrorCode::InternalError && e.message == UNWIRED)
        );
        let unknown = codec::decode_response::<Status>(&unknown).unwrap();
        assert!(
            matches!(&unknown.outcome, Outcome::Error(e) if e.code == ErrorCode::MethodNotFound)
        );
    }
}
