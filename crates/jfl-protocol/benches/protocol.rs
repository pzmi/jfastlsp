//! The protocol's share of a query's budget, in process (ver §10, doc/bench/README.md's in-process
//! convention): a `jfl/references` response carrying an envelope of 1,000 and 10,000 locations,
//! made from the domain envelope and encoded (the daemon's side), its frame decoded from 64 KiB
//! reads (a socket's), and its body decoded into the wire envelope (the proxy's). Each way twice:
//! the typed path (straight between bytes and `Response<WireEnvelope>`), and the path through a
//! `serde_json::Value`, which is what `lsp_server::Message` holds as a response's `result`
//! (`to_value`, then the message to bytes; bytes to a `Value` message, then
//! `payload_from_value`). Result payloads take the typed path (plan §5.3); the second pair
//! measures what that decision saves.
//!
//! Inputs are synthetic and deterministic (a fixed xorshift seed), shaped like `jfl-domain`'s
//! envelope bench: hits over 1,600 files of a 40-module repository (BENCH-03's fan-out of one
//! widely used type), a fifth of them exact, every `possible` hit with a reason and half with a
//! detail. Before timing, the benchmark prints each response's size in bytes. Not run by `check`;
//! `just bench-protocol`.

use std::sync::OnceLock;

use divan::Bencher;
use divan::counter::{BytesCount, ItemsCount};
use jfl_domain::{
    Completeness, Envelope, EnvelopeParts, FileUri, Freshness, Generation, IncompleteReason,
    Location, ObjectId, Position, PositionEncoding, PossibleHit, Range, Reason, SymbolId, Target,
    TargetStatus, UnixMillis, View, Why,
};
use jfl_protocol::codec::{self, decode_response};
use jfl_protocol::envelope::{Operation, Query};
use jfl_protocol::frame::{DEFAULT_MAX_FRAME_LEN, FrameDecoder, encode_frame};
use jfl_protocol::messages::References;
use jfl_protocol::{DocumentUri, Outcome, RequestId, Response, WireEnvelope};

/// The location counts of an answer: a common one and a widely used method's.
const LOCATIONS: [usize; 2] = [1_000, 10_000];
/// A socket read.
const READ: usize = 64 * 1024;

struct Case {
    locations: usize,
    envelope: Envelope,
    body: Vec<u8>,
    frame: Vec<u8>,
}

static CASES: OnceLock<Vec<Case>> = OnceLock::new();

fn main() {
    let cases = match LOCATIONS
        .iter()
        .map(|&n| case(n))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(cases) => cases,
        Err(why) => {
            eprintln!("bench inputs failed: {why}");
            std::process::exit(1);
        }
    };
    for case in &cases {
        println!(
            "jfl/references response: {} locations, {} bytes ({} bytes per location)",
            case.locations,
            case.body.len(),
            case.body.len().checked_div(case.locations).unwrap_or(0)
        );
    }
    let _ = CASES.set(cases);
    divan::main();
}

fn cases() -> &'static [Case] {
    // `main` sets it before any bench runs, and exits when it cannot.
    CASES.get().map_or(&[], Vec::as_slice)
}

fn find(locations: usize) -> Option<&'static Case> {
    cases().iter().find(|case| case.locations == locations)
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

fn case(locations: usize) -> Result<Case, String> {
    let envelope = envelope(locations)?;
    let response = response(&envelope)?;
    let body = codec::encode(&response).map_err(|e| e.to_string())?;
    let frame = encode_frame(&body, DEFAULT_MAX_FRAME_LEN).map_err(|e| e.to_string())?;
    Ok(Case {
        locations,
        envelope,
        body,
        frame,
    })
}

fn response(envelope: &Envelope) -> Result<Response<WireEnvelope>, String> {
    let query = Query {
        op: Operation::Refs,
        uri: DocumentUri::parse("file:///repo/module0/src/main/java/com/acme/File0.java")
            .map_err(|e| e.to_string())?,
        line: 41,
        col: 17,
    };
    Ok(Response {
        id: Some(RequestId::Number(7)),
        outcome: Outcome::Result(WireEnvelope::from_domain(envelope, Some(query))),
    })
}

fn envelope(locations: usize) -> Result<Envelope, String> {
    let mut rng = Rng(0x1DC0_0003);
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
    let exact_count = locations / 5;
    let exact = (0..exact_count)
        .map(|_| location(&mut rng))
        .collect::<Result<_, _>>()?;
    let possible = (0..locations.saturating_sub(exact_count))
        .map(|_| {
            Ok(PossibleHit {
                location: location(&mut rng)?,
                why: if rng.below(2) == 0 {
                    Why::new(Reason::NameAndArity)
                } else {
                    Why::with_detail(Reason::ReceiverTypeUnknown, "var in lambda")
                },
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(Envelope::new(EnvelopeParts {
        view: View {
            worktree: FileUri::from_path("/repo").map_err(|e| e.to_string())?,
            head: Some(
                ObjectId::parse("3f2c9a1b4d5e6f708192a3b4c5d6e7f8091a2b3c")
                    .map_err(|e| e.to_string())?,
            ),
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
    }))
}

#[divan::bench(args = LOCATIONS)]
fn encode_a_references_response(bencher: Bencher, locations: usize) {
    let Some(case) = find(locations) else {
        return;
    };
    bencher.counter(ItemsCount::new(locations)).bench(|| {
        response(&case.envelope).and_then(|r| codec::encode(&r).map_err(|e| e.to_string()))
    });
}

#[divan::bench(args = LOCATIONS)]
fn decode_a_frame_from_socket_reads(bencher: Bencher, locations: usize) {
    let Some(case) = find(locations) else {
        return;
    };
    bencher
        .counter(BytesCount::new(case.frame.len()))
        .bench(|| {
            let mut decoder = FrameDecoder::new(DEFAULT_MAX_FRAME_LEN);
            let mut body = None;
            for read in case.frame.chunks(READ) {
                let mut rest = read;
                while !rest.is_empty() {
                    let Ok(progress) = decoder.feed(rest) else {
                        return None;
                    };
                    rest = rest.get(progress.consumed..).unwrap_or_default();
                    body = body.or(progress.frame);
                }
            }
            body
        });
}

#[divan::bench(args = LOCATIONS)]
fn decode_a_references_response(bencher: Bencher, locations: usize) {
    let Some(case) = find(locations) else {
        return;
    };
    bencher
        .counter(ItemsCount::new(locations))
        .bench(|| decode_response::<References>(&case.body).ok());
}

#[divan::bench(args = LOCATIONS)]
fn encode_a_references_response_through_a_json_value(bencher: Bencher, locations: usize) {
    let Some(case) = find(locations) else {
        return;
    };
    bencher.counter(ItemsCount::new(locations)).bench(|| {
        let response = response(&case.envelope).ok()?;
        let Outcome::Result(envelope) = response.outcome else {
            return None;
        };
        let message = Response {
            id: response.id,
            outcome: Outcome::Result(serde_json::to_value(envelope).ok()?),
        };
        codec::encode(&message).ok()
    });
}

#[divan::bench(args = LOCATIONS)]
fn decode_a_references_response_through_a_json_value(bencher: Bencher, locations: usize) {
    let Some(case) = find(locations) else {
        return;
    };
    bencher.counter(ItemsCount::new(locations)).bench(|| {
        let message = serde_json::from_slice::<Response<serde_json::Value>>(&case.body).ok()?;
        let Outcome::Result(result) = message.outcome else {
            return None;
        };
        codec::payload_from_value::<WireEnvelope>(result).ok()
    });
}
