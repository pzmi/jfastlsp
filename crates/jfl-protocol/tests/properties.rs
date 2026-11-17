//! Properties (ver §2, §8): the frame decoder on arbitrary bytes and chunkings, the lying
//! `Content-Length`, and encode-then-decode identity for frames and generated messages.
//!
//! Seeded (ver §3): every run starts from `SEED`; a failure is shrunk and written to
//! `proptest-regressions/`, which is committed.

use jfl_domain::{
    Completeness, Envelope, EnvelopeParts, FileUri, Freshness, Generation, IncompleteReason,
    IndexState, Location, ModuleIndex, ModuleName, ObjectId, Position, PositionEncoding,
    PossibleHit, Range, Reason, SymbolId, Target, TargetStatus, UnixMillis, View, Why,
    completeness,
};
use jfl_protocol::codec::{self, decode_notification, decode_response, notification};
use jfl_protocol::envelope::{Operation, Query, WireRange};
use jfl_protocol::frame::{FrameDecoder, FrameError, Oversized, encode_frame};
use jfl_protocol::messages::{
    ContentChange, Definition, DidChange, DidChangeParams, PositionParams, References,
    ReferencesParams,
};
use jfl_protocol::{DocumentUri, Outcome, RequestId, Response, WireEnvelope};
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};

/// The decoder driver and the generators.
#[cfg(test)]
mod support {
    use super::*;

    pub(super) const LIMIT: usize = 4096;
    /// The smallest refused length.
    pub(super) const ABOVE_LIMIT: u64 = 4097;

    /// The fixed seed of every property here (ver §3 rule 2).
    pub(super) const SEED: u64 = 0x1DC0_0003;

    pub(super) fn config() -> ProptestConfig {
        ProptestConfig {
            rng_seed: RngSeed::Fixed(SEED),
            // ver §3 rule 6: the crate's `proptest-regressions/`, committed.
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "proptest-regressions/properties.txt",
            ))),
            ..ProptestConfig::default()
        }
    }

    /// Feeds `input` in chunks of the given sizes (cycled), collecting frames until an error.
    pub(super) fn feed_all(
        decoder: &mut FrameDecoder,
        input: &[u8],
        chunks: &[usize],
    ) -> (Vec<Vec<u8>>, Option<FrameError>) {
        let (frames, error, _) = feed_all_reporting(decoder, input, chunks);
        (frames, error)
    }

    /// Feeds `input` in chunks of the given sizes (cycled), collecting frames and oversized
    /// reports until an error.
    pub(super) fn feed_all_reporting(
        decoder: &mut FrameDecoder,
        input: &[u8],
        chunks: &[usize],
    ) -> (Vec<Vec<u8>>, Option<FrameError>, Vec<Oversized>) {
        let mut frames = Vec::new();
        let mut reports = Vec::new();
        let mut rest = input;
        let mut sizes = chunks.iter().copied().cycle();
        while !rest.is_empty() {
            let size = sizes.next().unwrap_or(1).clamp(1, rest.len());
            let (mut chunk, tail) = rest.split_at(size);
            rest = tail;
            while !chunk.is_empty() {
                match decoder.feed(chunk) {
                    Ok(progress) => {
                        assert!(progress.consumed > 0, "no progress on a non-empty input");
                        assert!(progress.consumed <= chunk.len());
                        assert!(
                            decoder.buffered_capacity() <= LIMIT,
                            "buffered past the limit"
                        );
                        frames.extend(progress.frame);
                        reports.extend(progress.oversized);
                        chunk = &chunk[progress.consumed..];
                    }
                    Err(err) => return (frames, Some(err), reports),
                }
            }
        }
        (frames, None, reports)
    }

    pub(super) fn frame_bytes() -> impl Strategy<Value = Vec<u8>> {
        let header = prop_oneof![
            Just(b"Content-Length: ".to_vec()),
            Just(b"content-length:".to_vec()),
            Just(b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n".to_vec()),
            Just(b"\r\n".to_vec()),
            Just(b"\r\n\r\n".to_vec()),
            "[0-9]{1,25}".prop_map(String::into_bytes),
            proptest::collection::vec(any::<u8>(), 0..64),
        ];
        proptest::collection::vec(header, 0..12).prop_map(|parts| parts.concat())
    }

    pub(super) fn segment() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9_+ é-]{1,10}"
    }

    pub(super) fn file_uri() -> impl Strategy<Value = FileUri> {
        proptest::collection::vec(segment(), 1..4).prop_filter_map("a valid path", |parts| {
            FileUri::from_path(&format!("/repo/{}.java", parts.join("/"))).ok()
        })
    }

    pub(super) fn range() -> impl Strategy<Value = Range> {
        (any::<u32>(), any::<u32>(), any::<u32>(), any::<u32>()).prop_map(|(a, b, c, d)| {
            let (start, end) = if (a, b) <= (c, d) {
                ((a, b), (c, d))
            } else {
                ((c, d), (a, b))
            };
            Range::new(Position::new(start.0, start.1), Position::new(end.0, end.1)).unwrap()
        })
    }

    pub(super) fn location() -> impl Strategy<Value = Location> {
        (file_uri(), range()).prop_map(|(uri, range)| Location { uri, range })
    }

    pub(super) fn reason() -> impl Strategy<Value = Reason> {
        prop_oneof![
            Just(Reason::ReceiverTypeUnknown),
            Just(Reason::OverloadNotProved),
            Just(Reason::NameAndArity),
            Just(Reason::MethodReference),
            Just(Reason::ImportUnresolved),
            Just(Reason::GeneratedMember),
            Just(Reason::NoClasspath),
            Just(Reason::SyntaxError),
        ]
    }

    pub(super) fn possible() -> impl Strategy<Value = PossibleHit> {
        (location(), reason(), proptest::option::of("[ -~é]{0,20}")).prop_map(
            |(location, reason, detail)| PossibleHit {
                location,
                why: detail.map_or(Why::new(reason), |d| Why::with_detail(reason, &d)),
            },
        )
    }

    pub(super) fn completeness_at(generation: u64) -> impl Strategy<Value = Completeness> {
        prop_oneof![
            Just(()).prop_map(move |()| {
                let module =
                    ModuleIndex::new(ModuleName::worktree(), Generation(generation), 3, 3, 0, 0)
                        .unwrap();
                completeness(
                    &[ModuleName::worktree()],
                    &IndexState::new(Generation(generation), vec![module]).unwrap(),
                )
            }),
            Just(Completeness::because(IncompleteReason::Deadline)),
            any::<u32>().prop_map(|events| {
                Completeness::because(IncompleteReason::IndexLoading)
                    .and(IncompleteReason::PendingChanges { events })
            }),
        ]
    }

    pub(super) fn encoding() -> impl Strategy<Value = PositionEncoding> {
        prop_oneof![
            Just(PositionEncoding::Utf8),
            Just(PositionEncoding::Utf16),
            Just(PositionEncoding::Utf32),
        ]
    }

    pub(super) fn envelope() -> impl Strategy<Value = WireEnvelope> {
        let target = proptest::option::of((any::<bool>(), 1_u32..9).prop_map(
            |(exact, overloads)| Target {
                symbol: SymbolId::parse("com.acme.Billing#charge(Money)").unwrap(),
                status: if exact {
                    TargetStatus::Exact
                } else {
                    TargetStatus::Ambiguous { overloads }
                },
            },
        ));
        let head = proptest::option::of(proptest::collection::vec(any::<u8>(), 20)).prop_map(|b| {
            b.map(|bytes| {
                let hex: String = bytes.iter().map(|x| format!("{x:02x}")).collect();
                ObjectId::parse(&hex).unwrap()
            })
        });
        (
            file_uri(),
            head,
            0_u64..1000,
            target,
            proptest::collection::vec(location(), 0..12),
            proptest::collection::vec(possible(), 0..12),
            proptest::option::of(any::<u64>()),
            any::<u32>(),
            encoding(),
            any::<bool>(),
        )
            .prop_flat_map(
                |(
                    worktree,
                    head,
                    generation,
                    target,
                    exact,
                    possible,
                    synced,
                    pending,
                    enc,
                    query,
                )| {
                    completeness_at(generation).prop_map(move |completeness| {
                        let envelope = Envelope::new(EnvelopeParts {
                            view: View {
                                worktree: worktree.clone(),
                                head,
                                generation: Generation(generation),
                            },
                            target: target.clone(),
                            exact: exact.clone(),
                            possible: possible.clone(),
                            completeness,
                            freshness: Freshness {
                                synced_at: synced.map(UnixMillis),
                                pending_events: pending,
                            },
                            position_encoding: enc,
                        });
                        let query = query.then(|| Query {
                            op: Operation::Refs,
                            uri: DocumentUri::from(&worktree),
                            line: 1,
                            col: 2,
                        });
                        WireEnvelope::from_domain(&envelope, query)
                    })
                },
            )
    }
}

use support::*;

proptest! {
    #![proptest_config(config())]

    #[test]
    fn should_never_panic_or_buffer_past_the_limit_when_fed_arbitrary_bytes(
        input in proptest::collection::vec(any::<u8>(), 0..2048),
        chunks in proptest::collection::vec(1_usize..64, 1..8),
    ) {
        // given
        let mut decoder = FrameDecoder::new(LIMIT);

        // when
        let (frames, _) = feed_all(&mut decoder, &input, &chunks);

        // then
        prop_assert!(frames.iter().all(|frame| frame.len() <= LIMIT));
        prop_assert!(decoder.buffered_capacity() <= LIMIT);
    }

    #[test]
    fn should_never_panic_or_buffer_past_the_limit_when_fed_header_like_bytes(
        input in frame_bytes(),
        chunks in proptest::collection::vec(1_usize..16, 1..8),
    ) {
        // given
        let mut decoder = FrameDecoder::new(LIMIT);

        // when
        let (frames, _) = feed_all(&mut decoder, &input, &chunks);

        // then
        prop_assert!(frames.iter().all(|frame| frame.len() <= LIMIT));
        prop_assert!(decoder.buffered_capacity() <= LIMIT);
    }

    #[test]
    fn should_refuse_before_allocating_when_content_length_lies(
        declared in ABOVE_LIMIT..=u64::MAX,
        extra_digits in "[0-9]{0,6}",
    ) {
        // given
        let mut decoder = FrameDecoder::new(LIMIT);
        let header = format!("Content-Length: {declared}{extra_digits}\r\n\r\n");

        let parsed = format!("{declared}{extra_digits}").parse::<u64>().ok();

        // when
        let result = decoder.feed(header.as_bytes());

        // then
        match parsed {
            Some(declared) => prop_assert_eq!(
                result.map(|p| p.oversized),
                Ok(Some(Oversized { declared, limit: LIMIT }))
            ),
            None => prop_assert!(
                matches!(result, Err(FrameError::TooLarge { limit: LIMIT, .. })),
                "{result:?}"
            ),
        }
        prop_assert_eq!(decoder.buffered_capacity(), 0);
    }

    #[test]
    fn should_skip_an_oversized_frame_without_buffering_when_valid_frames_surround_it(
        before in proptest::collection::vec(any::<u8>(), 0..64),
        oversized in (LIMIT + 1)..(3 * LIMIT),
        after in proptest::collection::vec(any::<u8>(), 0..64),
        chunks in proptest::collection::vec(1_usize..700, 1..8),
    ) {
        // given
        let mut decoder = FrameDecoder::new(LIMIT);
        let mut stream = encode_frame(&before, LIMIT).unwrap();
        stream.extend(format!("Content-Length: {oversized}\r\n\r\n").into_bytes());
        stream.extend(std::iter::repeat_n(b'z', oversized));
        stream.extend(encode_frame(&after, LIMIT).unwrap());

        // when
        let (frames, error, reports) = feed_all_reporting(&mut decoder, &stream, &chunks);

        // then
        prop_assert_eq!(error, None);
        prop_assert_eq!(frames, vec![before, after]);
        prop_assert_eq!(reports, vec![Oversized { declared: oversized as u64, limit: LIMIT }]);
        prop_assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn should_hold_at_most_twice_what_arrived_when_a_body_is_promised_but_not_sent(
        declared in 1..=LIMIT,
        sent in 0..=LIMIT,
        chunks in proptest::collection::vec(1_usize..300, 1..8),
    ) {
        // given
        let mut decoder = FrameDecoder::new(LIMIT);
        let sent = sent.min(declared.saturating_sub(1));
        let mut input = format!("Content-Length: {declared}\r\n\r\n").into_bytes();
        input.extend(std::iter::repeat_n(b'x', sent));

        // when
        let (frames, error) = feed_all(&mut decoder, &input, &chunks);

        // then
        prop_assert!(frames.is_empty());
        prop_assert!(error.is_none());
        prop_assert!(decoder.buffered_capacity() <= declared);
        prop_assert!(decoder.buffered_capacity() <= sent.saturating_mul(2));
        prop_assert_eq!(decoder.finish(), Err(FrameError::Truncated));
    }

    #[test]
    fn should_decode_what_it_encodes_when_bodies_are_arbitrary(
        bodies in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..512), 1..6),
        chunks in proptest::collection::vec(1_usize..97, 1..8),
    ) {
        // given
        let mut decoder = FrameDecoder::new(LIMIT);
        let stream: Vec<u8> = bodies
            .iter()
            .flat_map(|body| encode_frame(body, LIMIT).unwrap())
            .collect();

        // when
        let (frames, error) = feed_all(&mut decoder, &stream, &chunks);

        // then
        prop_assert_eq!(error, None);
        prop_assert_eq!(frames, bodies);
        prop_assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn should_decode_what_it_encodes_when_envelopes_are_generated(
        envelope in envelope(),
        id in prop_oneof![any::<i32>().prop_map(RequestId::Number), "[ -~]{0,12}".prop_map(RequestId::String)],
    ) {
        // given
        let response = Response { id: Some(id), outcome: Outcome::Result(envelope) };
        let body = codec::encode(&response).unwrap();
        let mut decoder = FrameDecoder::new(jfl_protocol::DEFAULT_MAX_FRAME_LEN);

        // when
        let framed = encode_frame(&body, jfl_protocol::DEFAULT_MAX_FRAME_LEN).unwrap();
        let frame = decoder.feed(&framed).unwrap().frame.unwrap();
        let decoded = decode_response::<References>(&frame).unwrap();
        let as_definition = decode_response::<Definition>(&frame).unwrap();

        // then
        prop_assert_eq!(&decoded, &response);
        prop_assert_eq!(as_definition, response);
    }

    #[test]
    fn should_decode_what_it_encodes_when_params_are_generated(
        uri in file_uri(),
        line in any::<u32>(),
        col in any::<u32>(),
        encoding in encoding(),
        exact_only in any::<bool>(),
        changes in proptest::collection::vec((proptest::option::of(range()), "\\PC{0,40}"), 0..6),
        version in any::<i32>(),
    ) {
        // given
        let references = ReferencesParams {
            at: PositionParams { uri: DocumentUri::from(&uri), line, col, position_encoding: encoding },
            exact_only,
        };
        let change = notification::<DidChange>(DidChangeParams {
            uri: DocumentUri::from(&uri),
            version,
            position_encoding: encoding,
            changes: changes
                .into_iter()
                .map(|(range, text)| ContentChange { range: range.map(WireRange), text })
                .collect(),
        });

        // when
        let references_back: ReferencesParams =
            codec::decode(&codec::encode(&references).unwrap()).unwrap();
        let change_back =
            decode_notification::<DidChange>(&codec::encode(&change).unwrap()).unwrap();

        // then
        prop_assert_eq!(references_back, references);
        prop_assert_eq!(change_back, change);
    }
}
