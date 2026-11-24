//! The request seam: what the daemon answers once a connection is open.
//!
//! The host answers the handshake and `jfl/stop` itself; every other request goes to a
//! [`Service`], with its connection's overlays. The daemon serves the views and their queries
//! through [`super::views::Views`] (LSP-02); [`Unwired`] stays for the host's conformance driver,
//! which steps the lifecycle without views: it answers `jfl/status` with the daemon and no view,
//! and every query with an error that says nothing is wired, never claiming `complete=true`,
//! having no view to prove it at (ADR 0014). Results are serialized from the typed struct straight
//! to the frame body (plan §5.3).
//!
//! **Asynchronous, one request at a time.** [`Service::answer`] returns a future the run loop
//! awaits on its own task, so a service awaits its ports (the view service's blocking pool) without
//! a second runtime; the loop takes the next frame only once it is done, which is how the view
//! service's requests are serialised (it takes `&mut self`). [`Service::background`] is the work
//! the daemon does between requests (the view service's `settle` and `write_back`), one bounded
//! step at a time, each run to its end: a step is never dropped half done (IDX-09's wiring notes).
//!
//! Pinned by `service::tests`.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

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
    /// The file's modification time on disk when this text arrived (`None`: no file, or no
    /// time): a disk write after it replaces the overlay (plan §4.4, disk wins).
    pub disk: Option<std::time::SystemTime>,
}

/// What a request's handler sees of the daemon and the request's connection.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The daemon.
    pub server: &'a ServerInfo,
    /// The connection's open documents.
    pub overlays: &'a BTreeMap<DocumentUri, Overlay>,
}

/// A service's work in progress: an answer's serialized response, or a background step.
pub type Working<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Answers the requests the host does not.
pub trait Service {
    /// The serialized response to `call`.
    fn answer<'a>(&'a mut self, call: &'a Call, context: &'a Context<'a>) -> Working<'a, Vec<u8>>;

    /// The next step of the work the daemon does between requests, if there is any: the run loop
    /// awaits it to its end when no frame waits, then asks again. None by default.
    fn background(&mut self) -> Option<Working<'_, ()>> {
        None
    }
}

/// The service until the views are wired: status without views, an error for every query.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unwired;

/// What [`Unwired`] answers a query with.
pub const UNWIRED: &str = "no view is wired into the daemon yet";

impl Service for Unwired {
    fn answer<'a>(&'a mut self, call: &'a Call, context: &'a Context<'a>) -> Working<'a, Vec<u8>> {
        Box::pin(std::future::ready(unwired(call, context)))
    }
}

/// [`Unwired`]'s answer, which a wired service gives for every method it does not serve itself.
pub(crate) fn unwired(call: &Call, context: &Context<'_>) -> Vec<u8> {
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
        let status = unwired(
            &call(Status::NAME, serde_json::json!({"worktree": null})),
            &context,
        );
        let definition = unwired(&call(Definition::NAME, serde_json::json!({})), &context);
        let unknown = unwired(&call("jfl/nothing", serde_json::Value::Null), &context);

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
