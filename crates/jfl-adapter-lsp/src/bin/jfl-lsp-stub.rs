//! A test-only LSP server over stdio, built on the adapter, for the black-box transcripts of
//! `fixtures/lsp/` (`tests/lsp_transcripts.rs`) until `jfl lsp` exists: LSP-02 runs the same
//! transcripts against `jfl lsp` and deletes this binary.
//!
//! It knows one request and two notifications. `textDocument/definition` runs on the pool and
//! answers as a stand-in daemon would, with canonical URIs that the adapter then echoes: the
//! position asked about in the document named, the start of `Opened.java` beside it and the start
//! of `Other.java` beside it (`fixtures/lsp/uri-echo.jsonl`). `didOpen` and `didClose` keep the
//! client's spelling of each open document. Every other request is `MethodNotFound`, every other
//! notification is ignored. It exits 0 when its input ends between frames and 1 when the framing
//! is lost or a read or write fails.

use std::io;
use std::process::ExitCode;
use std::sync::Arc;

use gen_lsp_types::{
    DefinitionParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, Location,
};
use jfl_adapter_lsp::server::Dispatch;
use jfl_adapter_lsp::translate::{Echo, UriBook, location, position_params};
use jfl_adapter_lsp::transport::End;
use jfl_adapter_lsp::{Config, Handler, Job, Reply, serve};
use jfl_domain::{FileUri, Position, PositionEncoding, Range};
use jfl_protocol::DocumentUri;
use jfl_protocol::envelope::{WireLocation, WireRange};
use jfl_protocol::messages::PositionParams;
use jfl_protocol::rpc::ErrorCode;
use lsp_server::{Notification, Request};

#[derive(Default)]
struct Stub {
    opened: Arc<UriBook>,
}

impl Handler for Stub {
    type Snapshot = UriBook;

    fn snapshot(&self) -> Arc<UriBook> {
        Arc::clone(&self.opened)
    }

    fn request(&mut self, request: Request) -> Dispatch<UriBook> {
        if request.method != "textDocument/definition" {
            return Dispatch::MethodNotFound;
        }
        let invalid = |why: &str| Dispatch::Now(Reply::error(ErrorCode::InvalidParams, why));
        let Ok(params) = serde_json::from_value::<DefinitionParams>(request.params) else {
            return invalid("not definition params");
        };
        let Ok(at) = position_params(
            &params.text_document_position_params,
            PositionEncoding::Utf16,
        ) else {
            return invalid("not a file URI");
        };
        Dispatch::Pool(Job::new(move |opened: &UriBook, _| {
            let echo = Echo {
                named: Some(&at.uri),
                opened: Some(opened),
            };
            match daemon(&at) {
                Some(hits) => {
                    let result: Vec<Location> =
                        hits.iter().map(|hit| location(hit, &echo)).collect();
                    Reply::result(&result)
                }
                None => Reply::error(ErrorCode::InternalError, "no sibling path"),
            }
        }))
    }

    fn notification(&mut self, notification: Notification) {
        match notification.method.as_str() {
            "textDocument/didOpen" => {
                if let Ok(params) =
                    serde_json::from_value::<DidOpenTextDocumentParams>(notification.params)
                    && let Ok(uri) = DocumentUri::parse(&params.text_document.uri.0)
                {
                    Arc::make_mut(&mut self.opened).open(&uri);
                }
            }
            "textDocument/didClose" => {
                if let Ok(params) =
                    serde_json::from_value::<DidCloseTextDocumentParams>(notification.params)
                    && let Ok(uri) = DocumentUri::parse(&params.text_document.uri.0)
                {
                    Arc::make_mut(&mut self.opened).close(uri.file());
                }
            }
            _ => {}
        }
    }
}

/// The stand-in daemon's answer, in canonical URIs as the daemon writes them.
fn daemon(at: &PositionParams) -> Option<Vec<WireLocation>> {
    let point = |position: Position| Range::new(position, position).ok().map(WireRange);
    let here = WireLocation {
        uri: DocumentUri::from(at.uri.file().clone()),
        range: point(Position::new(at.line, at.col))?,
    };
    let (dir, _) = at.uri.file().path().rsplit_once('/')?;
    let sibling = |name: &str| -> Option<WireLocation> {
        let file = FileUri::from_path(&format!("{dir}/{name}")).ok()?;
        Some(WireLocation {
            uri: DocumentUri::from(file),
            range: point(Position::new(0, 0))?,
        })
    };
    Some(vec![here, sibling("Opened.java")?, sibling("Other.java")?])
}

fn main() -> ExitCode {
    let ended = serve(
        io::stdin(),
        io::stdout(),
        Stub::default(),
        &Config::default(),
    );
    match (ended.input, ended.output) {
        (End::Eof, None) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
