//! A scripted fake language server, the backend of the component and integration tests (and of
//! `jfl-jdtls-fake-lsp`). It answers `initialize` and `shutdown`, and:
//!
//! - `fake/echo`: the request's `params`, so a test can tell whose request an answer is;
//! - `fake/slow`: no answer until cancelled (then `RequestCancelled`); announces itself with a
//!   `fake/slowStarted` notification so a test knows the backend holds it; every
//!   `$/cancelRequest` is announced with a `fake/cancelled` notification;
//! - `fake/garbage`: an answer that is not valid JSON (the id first, then a broken value);
//! - `fake/stats`: what it has seen (initialize count, cancelled ids, document events, its
//!   arguments, working directory and process id).

use std::io::{self, BufRead, Write};

use crate::framing::{MAX_FRAME_BYTES, read_frame, write_frame};
use crate::json::{self, Message, Value, json};

/// What the fake reports about itself in `fake/stats`.
#[derive(Debug, Clone, Default)]
pub struct FakeInfo {
    /// Its arguments.
    pub argv: Vec<String>,
    /// Its working directory.
    pub cwd: String,
    /// Its process id (0 in process).
    pub pid: u32,
}

fn send<W: Write>(writer: &mut W, msg: &Message) -> io::Result<()> {
    let body = json::encode(msg).map_err(io::Error::other)?;
    write_frame(writer, &body)
}

/// Serves until the input ends or `exit` arrives; returns the exit code LSP prescribes (0 after
/// `shutdown`, 1 otherwise).
///
/// # Errors
///
/// When writing fails or a frame is malformed.
pub fn serve<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    info: &FakeInfo,
) -> io::Result<i32> {
    let mut initialize_count = 0u64;
    let mut shutdown = false;
    let mut slow: Vec<Value> = Vec::new();
    let mut cancelled: Vec<Value> = Vec::new();
    let mut documents: Vec<Value> = Vec::new();
    while let Some(body) = read_frame(reader, MAX_FRAME_BYTES)? {
        let msg = json::parse(&body).map_err(io::Error::other)?;
        let method = msg.method.clone().unwrap_or_default();
        let params = msg.params_value().unwrap_or(Value::Null);
        let Some(id) = msg.id.clone() else {
            match method.as_str() {
                "exit" => return Ok(if shutdown { 0 } else { 1 }),
                "$/cancelRequest" => {
                    let target = params.get("id").cloned().unwrap_or(Value::Null);
                    if let Some(at) = slow.iter().position(|s| *s == target) {
                        slow.remove(at);
                        send(writer, &Message::error(target.clone(), -32800, "cancelled"))?;
                    }
                    send(
                        writer,
                        &Message::notification("fake/cancelled", &json!({ "id": target })),
                    )?;
                    cancelled.push(target);
                }
                m if m.starts_with("textDocument/did") => {
                    documents.push(json!({
                        "event": m.trim_start_matches("textDocument/"),
                        "uri": params.pointer("/textDocument/uri"),
                        "version": params.pointer("/textDocument/version"),
                    }));
                }
                _ => {}
            }
            continue;
        };
        let result = match method.as_str() {
            "initialize" => {
                initialize_count = initialize_count.saturating_add(1);
                json!({
                    "capabilities": { "textDocumentSync": 2, "hoverProvider": true },
                    "serverInfo": { "name": "fake" },
                })
            }
            "shutdown" => {
                shutdown = true;
                Value::Null
            }
            "fake/echo" => params,
            "fake/slow" => {
                slow.push(id.clone());
                send(
                    writer,
                    &Message::notification("fake/slowStarted", &json!({ "id": id })),
                )?;
                continue;
            }
            "fake/garbage" => {
                let id = serde_json::to_string(&id).map_err(io::Error::other)?;
                write_frame(
                    writer,
                    format!(r#"{{"jsonrpc":"2.0","id":{id},"result":tru}}"#).as_bytes(),
                )?;
                continue;
            }
            "fake/stats" => json!({
                "initializeCount": initialize_count,
                "cancelled": cancelled,
                "documents": documents,
                "argv": info.argv,
                "cwd": info.cwd,
                "pid": info.pid,
            }),
            _ => {
                send(writer, &Message::error(id, -32601, "method not found"))?;
                continue;
            }
        };
        send(writer, &Message::response(id, &result))?;
    }
    Ok(1)
}
