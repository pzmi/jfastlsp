//! The envelope, completeness and summaries in process (ver §10, doc/bench/README.md's in-process
//! convention): assembling an [`Envelope`] from shuffled candidates, the [`completeness`] check
//! over many modules, building a [`FileSummary`], checking it back from its parts, and digesting
//! its declarations' APIs.
//!
//! Inputs are synthetic and deterministic (a fixed xorshift seed). The candidate counts bracket
//! BENCH-03's fan-out of 1,586 dependent files of one type, spread over 1,600 files. The "typical
//! file" summary has the mean shape measured on spring-framework at the bench pin for IDX-02:
//! 25 declarations (the E1 runs' 237,869 declarations over 9,294 files) and 240 identifier uses
//! (an upper bound on reference sites: comments, strings and keywords excluded) over 60 distinct
//! names per file; the "heavy file" is the p95, 900 uses. Before timing, the benchmark prints
//! both summaries' bytes ([`FileSummary::heap_bytes`] plus the value itself) and the projection
//! of the typical one to 30,000 files. Not run
//! by `check`; `just bench-domain-envelope`.

use std::sync::OnceLock;

use divan::Bencher;
use divan::counter::ItemsCount;
use jfl_domain::{
    Arity, ByteOffset, ByteRange, Completeness, DeclKind, DeclarationInput, Digest, Envelope,
    EnvelopeParts, FileSummary, FileUri, Freshness, Generation, IncompleteReason, IndexState,
    KeyHasher, Location, Modifiers, ModuleIndex, ModuleName, Position, PositionEncoding,
    PossibleHit, Range, Reason, RefShape, SummaryBuilder, View, Why, completeness,
};

/// Declarations, identifier uses and distinct names of the typical file (module documentation).
const TYPICAL_DECLARATIONS: u32 = 25;
const TYPICAL_REFERENCES: u32 = 240;
const TYPICAL_VOCABULARY: u64 = 60;
/// Identifier uses of the heavy (p95) file.
const HEAVY_REFERENCES: u32 = 900;
/// Files a large repository holds (plan §5.5's "30k+").
const FILES: usize = 30_000;
/// The candidate counts of the envelope bench.
const CANDIDATES: [usize; 3] = [1_000, 10_000, 100_000];
/// The visible-module counts of the completeness bench.
const MODULES: [usize; 3] = [1, 50, 500];

struct Inputs {
    envelopes: Vec<(usize, EnvelopeParts)>,
    modules: Vec<(usize, Vec<ModuleName>, IndexState)>,
    summary: FileSummary,
}

static INPUTS: OnceLock<Inputs> = OnceLock::new();

fn main() {
    let inputs = match build_inputs() {
        Ok(inputs) => inputs,
        Err(why) => {
            eprintln!("bench inputs failed: {why}");
            std::process::exit(1);
        }
    };
    let bytes = |summary: &FileSummary| {
        summary
            .heap_bytes()
            .saturating_add(size_of::<FileSummary>())
    };
    let per_file = bytes(&inputs.summary);
    println!(
        "summary size: typical ({TYPICAL_DECLARATIONS} declarations, {TYPICAL_REFERENCES} references) \
         {per_file} bytes ({} heap, {} inline), {} KiB for {FILES} files",
        inputs.summary.heap_bytes(),
        size_of::<FileSummary>(),
        per_file.saturating_mul(FILES) / 1024,
    );
    match summary_of(HEAVY_REFERENCES) {
        Ok(heavy) => println!(
            "summary size: heavy ({TYPICAL_DECLARATIONS} declarations, {HEAVY_REFERENCES} references) {} bytes",
            bytes(&heavy)
        ),
        Err(why) => eprintln!("heavy summary failed: {why}"),
    }
    let _ = INPUTS.set(inputs);
    divan::main();
}

fn inputs() -> &'static Inputs {
    // `main` sets it before any bench runs, and exits when it cannot.
    INPUTS.get_or_init(|| build_inputs().unwrap_or_else(|_| std::process::exit(1)))
}

fn build_inputs() -> Result<Inputs, String> {
    Ok(Inputs {
        envelopes: CANDIDATES
            .iter()
            .map(|&n| Ok((n, envelope_parts(n)?)))
            .collect::<Result<_, String>>()?,
        modules: MODULES
            .iter()
            .map(|&n| {
                let (visible, index) = modules(n)?;
                Ok((n, visible, index))
            })
            .collect::<Result<_, String>>()?,
        summary: typical_summary()?,
    })
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

    fn below(&mut self, n: u64) -> u64 {
        self.next().checked_rem(n).unwrap_or(0)
    }

    fn below_u32(&mut self, n: u32) -> u32 {
        u32::try_from(self.below(u64::from(n))).unwrap_or(0)
    }
}

fn byte_range(start: u32, len: u32) -> Result<ByteRange, String> {
    ByteRange::new(
        ByteOffset::new(start),
        ByteOffset::new(start.saturating_add(len)),
    )
    .map_err(|e| e.to_string())
}

/// `candidates` hits over 1,600 files, a fifth exact, in shuffled order, with some repeats.
fn envelope_parts(candidates: usize) -> Result<EnvelopeParts, String> {
    let mut rng = Rng(0x1DC0_0002);
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
        let start = Position::new(rng.below_u32(2_000), rng.below_u32(120));
        let end = Position::new(start.line(), start.column().saturating_add(6));
        let uri = usize::try_from(rng.below_u32(file_count))
            .ok()
            .and_then(|at| uris.get(at))
            .ok_or("no file")?;
        Ok(Location {
            uri: uri.clone(),
            range: Range::new(start, end).map_err(|e| e.to_string())?,
        })
    };
    let exact_count = candidates / 5;
    let exact = (0..exact_count)
        .map(|_| location(&mut rng))
        .collect::<Result<_, _>>()?;
    let possible = (0..candidates.saturating_sub(exact_count))
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
    Ok(EnvelopeParts {
        view: View {
            worktree: FileUri::from_path("/repo").map_err(|e| e.to_string())?,
            head: None,
            generation: Generation(118),
        },
        target: None,
        exact,
        possible,
        completeness: Completeness::because(IncompleteReason::IndexLoading),
        freshness: Freshness {
            synced_at: None,
            pending_events: 0,
        },
        position_encoding: PositionEncoding::Utf16,
    })
}

#[divan::bench(args = CANDIDATES)]
fn assemble_an_envelope_from_shuffled_candidates(bencher: Bencher, candidates: usize) {
    let Some((_, parts)) = inputs().envelopes.iter().find(|(n, _)| *n == candidates) else {
        return;
    };
    bencher
        .counter(ItemsCount::new(candidates))
        .with_inputs(|| parts.clone())
        .bench_values(Envelope::new);
}

/// `count` modules, all indexed and fresh (the full check runs), visible in reverse order.
fn modules(count: usize) -> Result<(Vec<ModuleName>, IndexState), String> {
    let names: Vec<ModuleName> = (0..count)
        .map(|i| ModuleName::new(&format!(":module-{i}")).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let states = names
        .iter()
        .map(|name| {
            ModuleIndex::new(name.clone(), Generation(118), 400, 400, 0, 0)
                .map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    let mut visible = names;
    visible.reverse();
    let index = IndexState::new(Generation(118), states).map_err(|e| e.to_string())?;
    Ok((visible, index))
}

#[divan::bench(args = MODULES)]
fn check_completeness_over_visible_modules(bencher: Bencher, count: usize) {
    let Some((_, visible, index)) = inputs().modules.iter().find(|(n, _, _)| *n == count) else {
        return;
    };
    bencher
        .counter(ItemsCount::new(count))
        .bench(|| completeness(divan::black_box(visible), divan::black_box(index)));
}

/// The typical file: one top-level class, members with two parameters or none, and reference
/// sites over a fixed vocabulary (repeats are what the arena shares).
fn typical_summary() -> Result<FileSummary, String> {
    summary_of(TYPICAL_REFERENCES)
}

/// The typical file with `references` reference sites.
fn summary_of(references: u32) -> Result<FileSummary, String> {
    let mut rng = Rng(0x1DC0_0003);
    let err = |e: jfl_domain::SummaryError| e.to_string();
    let mut builder = SummaryBuilder::new();
    builder
        .package("org.example.payments.billing")
        .map_err(err)?;
    for i in 0..12_u32 {
        builder
            .import(
                &format!("org.example.payments.model.Type{i}"),
                false,
                false,
                byte_range(i.saturating_mul(40), 38)?,
            )
            .map_err(err)?;
    }
    let class = builder
        .declaration(&DeclarationInput {
            kind: DeclKind::Class,
            modifiers: Modifiers::PUBLIC,
            parent: None,
            name: "BillingService",
            name_range: byte_range(500, 14)?,
            range: byte_range(480, 20_000)?,
            signature: "@Service public class BillingService extends AbstractService implements Billing",
            params: &[],
            supertypes: &["AbstractService", "Billing"],
            permits: &[],
            annotations: &["Service"],
            javadoc: Some(byte_range(400, 70)?),
        })
        .map_err(err)?;
    for i in 1..TYPICAL_DECLARATIONS {
        let at = i.saturating_mul(700);
        let field = i.checked_rem(4) == Some(0);
        let name = format!("member{i}");
        let signature = format!("public Money member{i}(final Account account, int count)");
        let name_len = u32::try_from(name.len()).map_err(|e| e.to_string())?;
        builder
            .declaration(&DeclarationInput {
                kind: if field {
                    DeclKind::Field
                } else {
                    DeclKind::Method
                },
                modifiers: Modifiers::PUBLIC,
                parent: Some(class),
                name: &name,
                name_range: byte_range(at.saturating_add(600), name_len)?,
                range: byte_range(at.saturating_add(590), 690)?,
                signature: &signature,
                params: if field { &[] } else { &["Account", "int"] },
                supertypes: &[],
                permits: &[],
                annotations: &[],
                javadoc: Some(byte_range(at.saturating_add(560), 30)?),
            })
            .map_err(err)?;
    }
    for i in 0..references {
        let name = format!("identifier{}", rng.below(TYPICAL_VOCABULARY));
        let shape = usize::try_from(rng.below(8))
            .ok()
            .and_then(|at| RefShape::ALL.get(at))
            .copied()
            .unwrap_or(RefShape::Name);
        let name_len = u32::try_from(name.len()).map_err(|e| e.to_string())?;
        let arity = Arity::of(usize::try_from(rng.below(3)).unwrap_or(0));
        builder
            .reference(
                &name,
                byte_range(i.saturating_mul(25).saturating_add(700), name_len)?,
                shape,
                arity,
            )
            .map_err(err)?;
    }
    Ok(builder.finish())
}

#[divan::bench]
fn build_a_typical_file_summary() -> Option<FileSummary> {
    typical_summary().ok()
}

#[divan::bench]
fn check_a_typical_summary_from_its_parts(bencher: Bencher) {
    let summary = &inputs().summary;
    bencher
        .with_inputs(|| summary.parts().clone())
        .bench_values(FileSummary::from_parts);
}

/// A 64-bit FNV-1a standing in for blake3, so the bench times the encoding, not the hash.
#[derive(Default)]
struct Fnv(u64);

impl KeyHasher for Fnv {
    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn finish(self) -> Digest {
        let mut out = [0; 32];
        for (slot, byte) in out.iter_mut().zip(self.0.to_le_bytes()) {
            *slot = byte;
        }
        Digest::from_bytes(out)
    }
}

#[divan::bench]
fn digest_the_api_of_every_declaration_of_a_typical_file(bencher: Bencher) {
    let summary = &inputs().summary;
    bencher
        .counter(ItemsCount::new(summary.declarations().count()))
        .bench(|| {
            summary
                .declarations()
                .filter_map(|(id, _)| summary.api_digest(id, Fnv::default()))
                .count()
        });
}
