//! The adapter's share of a request's budget, in process (ver §10, doc/bench/README.md's
//! in-process convention).
//!
//! - **Round trip:** one request through a running session over in-memory pipes: the client's
//!   frame written, read and decoded by the adapter's reader, parsed, dispatched, answered
//!   (`MethodNotFound` on the main loop, or a small result from the pool), written by the writer,
//!   and read back by the client. The adapter's fixed cost per request, framing included.
//! - **A large answer:** a `jfl/references` response of 1,000 and 10,000 locations as the daemon
//!   sends it, turned into the LSP result the client gets: the body decoded
//!   (`codec::decode_response`), the hits translated to LSP locations with their URIs echoed, and
//!   the result framed. Twice: on the typed path the adapter takes (plan §5.3), and through a
//!   `serde_json::Value` both ways (`payload_from_value`, then `lsp_server::Response::new_ok`),
//!   which is what the decision avoids.
//!
//! Inputs are synthetic and deterministic, shaped like `jfl-protocol`'s benchmark (hits over 1,600
//! files of 40 modules, a fifth exact). Not run by `check`; `just bench-lsp`.

use std::io::{Read, Write};
use std::num::NonZeroUsize;
use std::sync::{Arc, OnceLock};
use std::thread;

use divan::Bencher;
use divan::counter::ItemsCount;
use jfl_adapter_lsp::memory::{PipeReader, pipe};
use jfl_adapter_lsp::translate::{Echo, locations};
use jfl_adapter_lsp::transport::{Outgoing, write_one};
use jfl_adapter_lsp::{Config, Dispatch, Handler, Job, Reply, serve};
use jfl_domain::{
    Completeness, Envelope, EnvelopeParts, FileUri, Freshness, Generation, IncompleteReason,
    Location, Position, PositionEncoding, PossibleHit, Range, Reason, SymbolId, Target,
    TargetStatus, UnixMillis, View, Why,
};
use jfl_protocol::codec::{self, decode_response};
use jfl_protocol::frame::FrameDecoder;
use jfl_protocol::messages::References;
use jfl_protocol::{Outcome, RequestId, Response, WireEnvelope};

const LOCATIONS: [usize; 2] = [1_000, 10_000];

static BODIES: OnceLock<Vec<(usize, Vec<u8>)>> = OnceLock::new();

fn main() {
    let bodies = match LOCATIONS
        .iter()
        .map(|&n| response_body(n).map(|body| (n, body)))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(bodies) => bodies,
        Err(why) => {
            eprintln!("bench inputs failed: {why}");
            std::process::exit(1);
        }
    };
    for (n, body) in &bodies {
        println!(
            "jfl/references response: {n} locations, {} bytes",
            body.len()
        );
    }
    let _ = BODIES.set(bodies);
    divan::main();
}

fn body(locations: usize) -> Option<&'static [u8]> {
    BODIES
        .get()?
        .iter()
        .find(|(n, _)| *n == locations)
        .map(|(_, body)| body.as_slice())
}

/// Answers `pooled` from the pool with a small result; everything else is `MethodNotFound`.
struct Small;

impl Handler for Small {
    type Snapshot = ();

    fn snapshot(&self) -> Arc<()> {
        Arc::new(())
    }

    fn request(&mut self, request: lsp_server::Request) -> Dispatch<()> {
        if request.method == "pooled" {
            Dispatch::Pool(Job::new(|(), _| Reply::result(&[1_u8, 2, 3])))
        } else {
            Dispatch::MethodNotFound
        }
    }

    fn notification(&mut self, _: lsp_server::Notification) {}
}

/// Reads one frame's body from the server's output.
fn recv(
    from: &mut PipeReader,
    decoder: &mut FrameDecoder,
    pending: &mut Vec<u8>,
) -> Option<Vec<u8>> {
    loop {
        while !pending.is_empty() {
            let progress = decoder.feed(pending).ok()?;
            pending.drain(..progress.consumed.min(pending.len()));
            if progress.frame.is_some() {
                return progress.frame;
            }
        }
        let mut chunk = [0_u8; 4096];
        let n = from.read(&mut chunk).ok().filter(|n| *n > 0)?;
        pending.extend_from_slice(chunk.get(..n)?);
    }
}

#[divan::bench(args = ["main-loop", "pool"])]
fn round_trip_one_request_in_memory(bencher: Bencher, path: &str) {
    let (mut to_server, server_in) = pipe();
    let (server_out, mut from_server) = pipe();
    let config = Config {
        workers: NonZeroUsize::new(2).unwrap_or(NonZeroUsize::MIN),
        ..Config::default()
    };
    let server = thread::spawn(move || serve(server_in, server_out, Small, &config));
    let method = if path == "pool" { "pooled" } else { "unknown" };
    let body = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}"}}"#);
    let frame = format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes();
    let mut decoder = FrameDecoder::new(1 << 20);
    let mut pending = Vec::new();
    bencher.bench_local(|| {
        to_server.write_all(&frame).ok()?;
        recv(&mut from_server, &mut decoder, &mut pending)
    });
    drop(to_server);
    let _ = server.join();
}

#[divan::bench(args = LOCATIONS)]
fn answer_references_on_the_typed_path(bencher: Bencher, locations_count: usize) {
    let Some(body) = body(locations_count) else {
        return;
    };
    bencher.counter(ItemsCount::new(locations_count)).bench(|| {
        let response = decode_response::<References>(body).ok()?;
        let Outcome::Result(envelope) = response.outcome else {
            return None;
        };
        let result = serde_json::to_vec(&locations(&envelope, &Echo::default())).ok()?;
        let mut out = Vec::new();
        let outgoing = Outgoing::Result {
            id: lsp_server::RequestId::from(7),
            body: result,
        };
        write_one(&mut out, outgoing).ok()?;
        Some(out)
    });
}

#[divan::bench(args = LOCATIONS)]
fn answer_references_through_a_json_value(bencher: Bencher, locations_count: usize) {
    let Some(body) = body(locations_count) else {
        return;
    };
    bencher.counter(ItemsCount::new(locations_count)).bench(|| {
        let message = serde_json::from_slice::<Response<serde_json::Value>>(body).ok()?;
        let Outcome::Result(result) = message.outcome else {
            return None;
        };
        let envelope = codec::payload_from_value::<WireEnvelope>(result).ok()?;
        let value = serde_json::to_value(locations(&envelope, &Echo::default())).ok()?;
        let response = lsp_server::Response {
            id: lsp_server::RequestId::from(7),
            response_result: Ok(value),
        };
        // Serialized as lsp-server would (its `Message::write` is denied: it logs the text).
        let body = serde_json::to_vec(&response).ok()?;
        let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        out.extend(body);
        Some(out)
    });
}

/// A deterministic xorshift64 stream.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u32) -> u32 {
        u32::try_from(self.next().checked_rem(u64::from(n)).unwrap_or(0)).unwrap_or(0)
    }
}

/// The daemon's `jfl/references` response body with `count` hits.
fn response_body(count: usize) -> Result<Vec<u8>, String> {
    let mut rng = Rng(0x1DC0_0001);
    let uris: Vec<FileUri> = (0..1_600_u32)
        .map(|i| {
            let module = i.checked_rem(40).unwrap_or(0);
            FileUri::from_path(&format!(
                "/repo/module{module}/src/main/java/com/acme/File{i}.java"
            ))
            .map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    let file_count = u32::try_from(uris.len()).map_err(|e| e.to_string())?;
    let location = |rng: &mut Rng| -> Result<Location, String> {
        let start = Position::new(rng.below(2_000), rng.below(120));
        let end = Position::new(start.line(), start.column().saturating_add(6));
        let uri = usize::try_from(rng.below(file_count))
            .ok()
            .and_then(|at| uris.get(at))
            .ok_or("no file")?;
        Ok(Location {
            uri: uri.clone(),
            range: Range::new(start, end).map_err(|e| e.to_string())?,
        })
    };
    let exact_count = count / 5;
    let exact = (0..exact_count)
        .map(|_| location(&mut rng))
        .collect::<Result<_, _>>()?;
    let possible = (0..count.saturating_sub(exact_count))
        .map(|_| {
            Ok(PossibleHit {
                location: location(&mut rng)?,
                why: Why::new(Reason::NameAndArity),
            })
        })
        .collect::<Result<_, String>>()?;
    let envelope = Envelope::new(EnvelopeParts {
        view: View {
            worktree: FileUri::from_path("/repo").map_err(|e| e.to_string())?,
            head: None,
            generation: Generation(118),
        },
        target: Some(Target {
            symbol: SymbolId::parse("com.acme.Billing#charge(Money)").map_err(|e| e.to_string())?,
            status: TargetStatus::Exact,
        }),
        exact,
        possible,
        completeness: Completeness::because(IncompleteReason::IndexLoading),
        freshness: Freshness {
            synced_at: Some(UnixMillis(1_763_400_000_000)),
            pending_events: 0,
        },
        position_encoding: PositionEncoding::Utf16,
    });
    let response = Response {
        id: Some(RequestId::Number(7)),
        outcome: Outcome::Result(WireEnvelope::from_domain(&envelope, None)),
    };
    codec::encode(&response).map_err(|e| e.to_string())
}
