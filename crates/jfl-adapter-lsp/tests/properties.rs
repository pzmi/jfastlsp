//! Properties (ver §2, §8): the adapter's reader on arbitrary bytes in arbitrary reads, and a whole
//! session on generated traffic.
//!
//! Seeded (ver §3): every run starts from `SEED`; a failure is shrunk and persisted to
//! `proptest-regressions/`, to be committed with the fix.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::num::NonZeroUsize;
use std::sync::Arc;

use jfl_adapter_lsp::transport::{FrameReader, Incoming};
use jfl_adapter_lsp::{Config, Dispatch, Handler, Job, Reply, serve};
use jfl_protocol::frame::FrameDecoder;
use lsp_server::{Message, Notification, Request};
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};
use serde_json::{Value, json};

#[cfg(test)]
mod support {
    use super::*;

    /// A small limit, so generated inputs cross it often.
    pub(super) const LIMIT: usize = 256;

    /// The fixed seed of every property here (ver §3 rule 2).
    pub(super) const SEED: u64 = 0x1DC0_0001;

    pub(super) fn config() -> ProptestConfig {
        ProptestConfig {
            rng_seed: RngSeed::Fixed(SEED),
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "proptest-regressions/properties.txt",
            ))),
            ..ProptestConfig::default()
        }
    }

    /// Input handed out in reads of the given sizes (cycled), as a pipe or a socket would.
    pub(super) struct Chunked {
        pub(super) data: Vec<u8>,
        pub(super) sizes: Vec<usize>,
        pub(super) at: usize,
        pub(super) read: usize,
    }

    impl Read for Chunked {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let size = self.sizes[self.read.checked_rem(self.sizes.len()).unwrap_or(0)].max(1);
            self.read = self.read.saturating_add(1);
            let rest = &self.data[self.at..];
            let n = rest.len().min(size).min(buf.len());
            buf[..n].copy_from_slice(&rest[..n]);
            self.at = self.at.saturating_add(n);
            Ok(n)
        }
    }

    /// Bytes that look like headers often enough to reach every state of the decoder.
    pub(super) fn header_like() -> impl Strategy<Value = Vec<u8>> {
        let piece = prop_oneof![
            Just(b"Content-Length: ".to_vec()),
            Just(b"content-length:".to_vec()),
            Just(b"Content-Type: application/vscode-jsonrpc; charset=utf-8".to_vec()),
            Just(b"\r\n".to_vec()),
            Just(b"\r\n\r\n".to_vec()),
            Just(b"\n".to_vec()),
            Just(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"m\"}".to_vec()),
            (0_u32..1000).prop_map(|n| n.to_string().into_bytes()),
            Just(b"999999999999999999999999".to_vec()),
            proptest::collection::vec(any::<u8>(), 0..16),
        ];
        proptest::collection::vec(piece, 0..40).prop_map(|pieces| pieces.concat())
    }

    /// The client traffic of a session, each item framed correctly or deliberately not.
    #[derive(Debug, Clone)]
    pub(super) enum Item {
        /// A request with a unique id (`n` or `"s<n>"`), answered on the main loop or the pool.
        Request { string_id: bool, pooled: bool },
        /// A request for a method nobody serves.
        Unknown { string_id: bool },
        /// A notification (`inc`, or one nobody knows).
        Notify { known: bool },
        /// A well-framed body that is not a message.
        Garbage(Vec<u8>),
        /// A frame over the limit.
        Oversized(usize),
    }

    pub(super) fn item() -> impl Strategy<Value = Item> {
        prop_oneof![
            (any::<bool>(), any::<bool>())
                .prop_map(|(string_id, pooled)| Item::Request { string_id, pooled }),
            any::<bool>().prop_map(|string_id| Item::Unknown { string_id }),
            any::<bool>().prop_map(|known| Item::Notify { known }),
            proptest::collection::vec(any::<u8>(), 0..64).prop_map(Item::Garbage),
            (LIMIT.saturating_add(1)..LIMIT.saturating_mul(4)).prop_map(Item::Oversized),
        ]
    }

    /// Counts `inc`s; `now` is answered on the main loop, `pooled` on the pool.
    pub(super) struct Counter(pub(super) u64);

    impl Handler for Counter {
        type Snapshot = u64;

        fn snapshot(&self) -> Arc<u64> {
            Arc::new(self.0)
        }

        fn request(&mut self, request: Request) -> Dispatch<u64> {
            match request.method.as_str() {
                "now" => Dispatch::Now(Reply::result(&self.0)),
                "pooled" => Dispatch::Pool(Job::new(|n: &u64, _| Reply::result(n))),
                _ => Dispatch::MethodNotFound,
            }
        }

        fn notification(&mut self, notification: Notification) {
            if notification.method == "inc" {
                self.0 = self.0.saturating_add(1);
            }
        }
    }

    pub(super) fn frame(body: &[u8]) -> Vec<u8> {
        let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        out.extend(body);
        out
    }

    pub(super) fn session_config() -> Config {
        Config {
            max_frame_len: LIMIT,
            workers: NonZeroUsize::new(3).unwrap(),
            queue: 2,
        }
    }

    pub(super) fn bodies(bytes: &[u8]) -> Vec<Value> {
        let mut decoder = FrameDecoder::new(1 << 20);
        let mut rest = bytes;
        let mut out = Vec::new();
        while !rest.is_empty() {
            let progress = decoder.feed(rest).expect("the server's own framing");
            rest = &rest[progress.consumed..];
            if let Some(body) = progress.frame {
                out.push(serde_json::from_slice(&body).expect("the server's own JSON"));
            }
        }
        decoder
            .finish()
            .expect("the server's output ends between frames");
        out
    }
}

use support::*;

proptest! {
    #![proptest_config(config())]

    #[test]
    fn should_never_panic_or_buffer_past_the_limit_when_fed_arbitrary_bytes(
        data in prop_oneof![proptest::collection::vec(any::<u8>(), 0..2048), header_like()],
        sizes in proptest::collection::vec(1_usize..300, 1..8),
    ) {
        // given
        let input = Chunked { data, sizes, at: 0, read: 0 };
        let mut reader = FrameReader::new(input, LIMIT);

        // when
        let mut seen = 0_usize;
        let mut last = None;
        while let Some(incoming) = reader.next_incoming() {
            // then
            prop_assert!(reader.buffered_capacity() <= LIMIT);
            seen = seen.saturating_add(1);
            prop_assert!(seen <= 4096, "the reader does not end");
            last = Some(incoming);
        }
        prop_assert!(matches!(last, Some(Incoming::End(_))), "{last:?}");
    }

    #[test]
    fn should_answer_every_request_once_when_traffic_is_mixed(
        items in proptest::collection::vec(item(), 0..40),
        sizes in proptest::collection::vec(1_usize..200, 1..6),
    ) {
        // given
        let mut data = Vec::new();
        let mut expected = BTreeMap::new();
        let mut nulls = 0_usize;
        for (n, item) in items.iter().enumerate() {
            let id = |string_id: bool| if string_id { json!(format!("s{n}")) } else { json!(n) };
            match item {
                Item::Request { string_id, pooled } => {
                    let method = if *pooled { "pooled" } else { "now" };
                    let body = json!({"jsonrpc": "2.0", "id": id(*string_id), "method": method});
                    data.extend(frame(body.to_string().as_bytes()));
                    expected.insert(id(*string_id).to_string(), None);
                }
                Item::Unknown { string_id } => {
                    let body = json!({"jsonrpc": "2.0", "id": id(*string_id), "method": "x/y"});
                    data.extend(frame(body.to_string().as_bytes()));
                    expected.insert(id(*string_id).to_string(), Some(-32601));
                }
                Item::Notify { known } => {
                    let method = if *known { "inc" } else { "x/unknown" };
                    let body = json!({"jsonrpc": "2.0", "method": method});
                    data.extend(frame(body.to_string().as_bytes()));
                }
                Item::Garbage(bytes) => {
                    let parsed = serde_json::from_slice::<Message>(bytes);
                    // A generated body that happens to be a message is not garbage.
                    prop_assume!(parsed.is_err());
                    let id_readable = serde_json::from_slice::<Value>(bytes)
                        .ok()
                        .and_then(|v| v.get("id").cloned())
                        .is_some_and(|id| id.is_i64() || id.is_string());
                    prop_assume!(!id_readable);
                    data.extend(frame(bytes));
                    nulls = nulls.saturating_add(1);
                }
                Item::Oversized(len) => {
                    data.extend(format!("Content-Length: {len}\r\n\r\n").into_bytes());
                    data.extend(std::iter::repeat_n(b' ', *len));
                    nulls = nulls.saturating_add(1);
                }
            }
        }
        let input = Chunked { data, sizes, at: 0, read: 0 };
        let mut output = Vec::new();

        // when
        let ended = serve(input, &mut output, Counter(0), &session_config());

        // then
        prop_assert_eq!(ended.output, None);
        let messages = bodies(&output);
        let mut answered = BTreeMap::new();
        let mut null_ids = 0_usize;
        for message in &messages {
            prop_assert_eq!(&message["jsonrpc"], "2.0");
            if message["id"].is_null() {
                null_ids = null_ids.saturating_add(1);
                continue;
            }
            let key = message["id"].to_string();
            prop_assert!(answered.insert(key.clone(), message.clone()).is_none(), "{key} twice");
        }
        prop_assert_eq!(null_ids, nulls);
        prop_assert_eq!(answered.len(), expected.len());
        for (id, error) in &expected {
            let answer = &answered[id];
            match error {
                Some(code) => prop_assert_eq!(&answer["error"]["code"], &json!(code)),
                None => prop_assert!(answer["result"].is_u64(), "{answer}"),
            }
        }
    }
}
