//! What a peer sent, one frame at a time: frames come through `jfl-protocol`'s bounded decoder
//! (the declared length checked before anything is allocated) and are classified as
//! `lsp_server::Message`s, the one message type of ADR 0021. Results go out on the typed path:
//! the caller serializes `jfl-protocol`'s typed `Response` to the frame body, never through a
//! `serde_json::Value` (plan §5.3).
//!
//! Pinned by `message::tests`.

use jfl_protocol::frame::{FrameError, Oversized};
use jfl_protocol::rpc::{ErrorCode, RequestId};
use serde_json::Value;

/// A request: its id, method and params, the params left as JSON for the caller to decode into
/// the method's typed params (`jfl_protocol::codec::payload_from_value`).
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// The id the answer carries.
    pub id: RequestId,
    /// The method.
    pub method: String,
    /// The params.
    pub params: Value,
}

/// A notification.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// The method.
    pub method: String,
    /// The params.
    pub params: Value,
}

/// A body that is not a request or a notification, and the error it is answered with when it
/// had an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Malformed {
    /// The id, when one could be read.
    pub id: Option<RequestId>,
    /// [`ErrorCode::ParseError`] for a body that is not JSON, [`ErrorCode::InvalidRequest`] for
    /// JSON that is not a request or a notification.
    pub code: ErrorCode,
}

/// Why a connection's input ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// End of stream between two frames.
    Eof,
    /// The framing is lost: the decoder stays failed rather than resynchronise on a guess.
    Framing(FrameError),
    /// The read failed (a reset connection).
    Read(std::io::ErrorKind),
}

/// One frame from a peer.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A request.
    Call(Call),
    /// A notification.
    Note(Note),
    /// A body that is neither; the framing is intact.
    Malformed(Malformed),
    /// A frame over the limit, skipped unread; the framing is intact.
    Oversized(Oversized),
    /// The input ended.
    End(End),
}

/// Classifies one frame's body.
#[must_use]
pub fn parse(body: &[u8]) -> Incoming {
    let message = match serde_json::from_slice::<lsp_server::Message>(body) {
        Ok(message) => message,
        Err(_) => {
            return Incoming::Malformed(match serde_json::from_slice::<Value>(body) {
                // JSON that is not a message: answered with its id when one can be read, so a
                // client waiting on that id is not left to its deadline.
                Ok(value) => Malformed {
                    id: value
                        .get("id")
                        .and_then(|id| serde_json::from_value::<RequestId>(id.clone()).ok()),
                    code: ErrorCode::InvalidRequest,
                },
                Err(_) => Malformed {
                    id: None,
                    code: ErrorCode::ParseError,
                },
            });
        }
    };
    match message {
        lsp_server::Message::Request(request) => {
            let id = serde_json::to_value(&request.id)
                .ok()
                .and_then(|v| serde_json::from_value::<RequestId>(v).ok());
            match id {
                Some(id) => Incoming::Call(Call {
                    id,
                    method: request.method,
                    params: request.params,
                }),
                None => Incoming::Malformed(Malformed {
                    id: None,
                    code: ErrorCode::InvalidRequest,
                }),
            }
        }
        lsp_server::Message::Notification(notification) => Incoming::Note(Note {
            method: notification.method,
            params: notification.params,
        }),
        lsp_server::Message::Response(_) => Incoming::Malformed(Malformed {
            id: None,
            code: ErrorCode::InvalidRequest,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_classify_each_body_when_it_is_a_request_a_notification_or_neither() {
        // given
        let bodies: [&[u8]; 6] = [
            br#"{"jsonrpc":"2.0","id":7,"method":"jfl/status","params":{"worktree":null}}"#,
            br#"{"jsonrpc":"2.0","method":"jfl/didClose","params":{"uri":"file:///a"}}"#,
            br#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            br#"[1,2]"#,
            b"not json",
            br#"{"jsonrpc":"2.0","id":9,"method":7}"#,
        ];

        // when
        let parsed: Vec<Incoming> = bodies.iter().map(|b| parse(b)).collect();

        // then
        assert!(
            matches!(&parsed[0], Incoming::Call(call) if call.id == RequestId::Number(7) && call.method == "jfl/status")
        );
        assert!(matches!(&parsed[1], Incoming::Note(note) if note.method == "jfl/didClose"));
        assert!(matches!(
            &parsed[2],
            Incoming::Malformed(Malformed {
                code: ErrorCode::InvalidRequest,
                ..
            })
        ));
        assert!(matches!(
            &parsed[3],
            Incoming::Malformed(Malformed {
                code: ErrorCode::InvalidRequest,
                ..
            })
        ));
        assert!(matches!(
            &parsed[4],
            Incoming::Malformed(Malformed {
                code: ErrorCode::ParseError,
                ..
            })
        ));
        assert_eq!(
            parsed[5],
            Incoming::Malformed(Malformed {
                id: Some(RequestId::Number(9)),
                code: ErrorCode::InvalidRequest,
            })
        );
    }
}
