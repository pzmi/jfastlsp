//! The entry format (format version [`FORMAT_VERSION`]): a fixed header, then the head (every
//! part of the summary but its reference sites, the distinct names those sites use, since
//! format 2 whether the file had a syntax error, and since format 3 the file's language), then the
//! reference sites, each section a postcard record (ADR 0025).
//!
//! ```text
//! 0..4    magic "JFLS"
//! 4..8    format version, u32 little-endian
//! 8..40   the SyntaxKey the entry is stored under
//! 40..44  head length, u32 little-endian
//! 44..48  reference sites' length, u32 little-endian
//! 48..64  the first 16 bytes of blake3(bytes 0..48 ++ head ++ reference sites)
//! 64..    head, then reference sites
//! ```
//!
//! **Every read is checked before it is decoded**: the length (exactly the header's two lengths
//! after the header, at most [`MAX_ENTRY_BYTES`]), the magic, the version, the key and the digest.
//! A torn, truncated or flipped entry fails the digest; an entry of another format version is
//! refused by its version and never migrated (fnd §6); an entry copied under another key is
//! refused by the key. What passes is decoded into `jfl-domain`'s plain parts, checked again by
//! [`FileSummary::from_parts`] and [`Declarations::from_parts`], so no byte sequence becomes a
//! summary the domain would refuse, and no decode panics (`tests/properties.rs`).
//!
//! **Compact on purpose**: postcard writes integers as varints, a range as its start and length,
//! and a reference site's start as the difference from the previous site's (sites are in source
//! order), so a site takes about 6 bytes on disk against 16 in memory (`just bench-store`).
//!
//! Rejected alternatives: one digest per section (every read here reads the whole entry, which is
//! one or two pages, so a second digest buys nothing); a CRC (blake3 is already in the graph and
//! as fast at these sizes); serde derives on `jfl-domain`'s types (plan §16.2 keeps serde out of
//! the domain; IDX-02).

use jfl_application::{Declarations, DeclarationsError};
use jfl_domain::{
    Arity, ByteOffset, ByteRange, DeclId, DeclKind, Declaration, FileSummary, Import, Language,
    ListRef, Modifiers, RefShape, ReferenceSite, Span, StrId, SummaryError, SummaryParts,
    SyntaxKey,
};
use serde::{Deserialize, Serialize};

/// The format version written in every header. Bumped whenever the same summary would encode
/// differently or an old reader would misread a new entry; an entry of another version is a miss.
pub const FORMAT_VERSION: u32 = 3;

/// The header's magic bytes.
pub const MAGIC: [u8; 4] = *b"JFLS";

/// The header's length.
pub const HEADER_LEN: usize = 64;

/// The largest entry: a longer one is refused before it is read, and never written (the store
/// counts it in `StoreStats::refused`, and the file is re-summarised on every open instead of
/// cached). The pinned corpus's largest entry is 262 KB (`just bench-store lombok`); nothing in
/// a summary bounds its declarations or reference sites per file, so a pathological file may pass
/// this and simply not be cached.
pub const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

const DIGEST_AT: usize = 48;
const DIGEST_LEN: usize = 16;

/// Why an entry was not decoded. Never source text, only what was wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Corrupt {
    /// Shorter than its header or than the lengths it declares, longer than them, or longer than
    /// [`MAX_ENTRY_BYTES`].
    #[error("entry length does not match its header")]
    Length,
    /// Not this store's magic.
    #[error("entry has no store magic")]
    Magic,
    /// Another format version (fnd §6: discarded, never migrated).
    #[error("entry is of format version {0}")]
    Version(u32),
    /// Stored under another key.
    #[error("entry belongs to another key")]
    Key,
    /// The digest does not match: torn, truncated or flipped.
    #[error("entry digest does not match")]
    Digest,
    /// A section that postcard cannot read, or with bytes left over.
    #[error("entry section does not decode")]
    Decode,
    /// A value no summary holds: a range that ends before it starts, an unknown language, kind or
    /// shape, unknown modifier bits.
    #[error("entry holds a value out of range")]
    Value,
    /// The parts decode, and `jfl-domain` refuses them.
    #[error("entry parts refused: {0}")]
    Summary(#[from] SummaryError),
    /// The declarations' name list is refused.
    #[error("entry names refused: {0}")]
    Names(#[from] DeclarationsError),
}

#[derive(Serialize, Deserialize)]
struct Head<'a> {
    /// [`SummaryParts::language`]'s code (format 3, IDX-17).
    language: u8,
    #[serde(borrow)]
    text: &'a str,
    strings: Vec<(u32, u32)>,
    package: Option<u32>,
    imports: Vec<ImportRecord>,
    declarations: Vec<DeclarationRecord>,
    type_names: Vec<u32>,
    names: Vec<u32>,
    /// [`SummaryParts::syntax_errors`] (format 2, IDX-11).
    syntax_errors: bool,
}

const STATIC: u8 = 1;
const ON_DEMAND: u8 = 0b10;
const MODULE: u8 = 0b100;

#[derive(Serialize, Deserialize)]
struct ImportRecord {
    name: u32,
    /// Bit 0 `static`, bit 1 on demand, bit 2 `import module`.
    flags: u8,
    range: (u32, u32),
}

#[derive(Serialize, Deserialize)]
struct DeclarationRecord {
    kind: u8,
    modifiers: u16,
    parent: Option<u32>,
    name: u32,
    name_range: (u32, u32),
    range: (u32, u32),
    signature: u32,
    params: (u32, u32),
    supertypes: (u32, u32),
    permits: (u32, u32),
    annotations: (u32, u32),
    javadoc: Option<(u32, u32)>,
}

#[derive(Serialize, Deserialize)]
struct ReferenceRecord {
    name: u32,
    /// This site's start minus the previous site's (0 for the first).
    delta: i64,
    len: u32,
    shape: u8,
    arity: u8,
}

const fn range_record(range: ByteRange) -> (u32, u32) {
    (range.start().get(), range.len())
}

fn range_of((start, len): (u32, u32)) -> Result<ByteRange, Corrupt> {
    let end = start.checked_add(len).ok_or(Corrupt::Value)?;
    ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).map_err(|_| Corrupt::Value)
}

const fn list_record(list: ListRef) -> (u32, u32) {
    (list.start, list.len)
}

const fn list_of((start, len): (u32, u32)) -> ListRef {
    ListRef { start, len }
}

fn import_record(import: &Import) -> ImportRecord {
    ImportRecord {
        name: import.name.0,
        flags: if import.is_static { STATIC } else { 0 }
            | if import.on_demand { ON_DEMAND } else { 0 }
            | if import.module { MODULE } else { 0 },
        range: range_record(import.range),
    }
}

fn import_of(record: &ImportRecord) -> Result<Import, Corrupt> {
    if record.flags & !(STATIC | ON_DEMAND | MODULE) != 0 {
        return Err(Corrupt::Value);
    }
    Ok(Import {
        name: StrId(record.name),
        is_static: record.flags & STATIC != 0,
        on_demand: record.flags & ON_DEMAND != 0,
        module: record.flags & MODULE != 0,
        range: range_of(record.range)?,
    })
}

fn declaration_record(d: &Declaration) -> DeclarationRecord {
    DeclarationRecord {
        kind: d.kind.code(),
        modifiers: d.modifiers.bits(),
        parent: d.parent.map(|p| p.0),
        name: d.name.0,
        name_range: range_record(d.name_range),
        range: range_record(d.range),
        signature: d.signature.0,
        params: list_record(d.params),
        supertypes: list_record(d.supertypes),
        permits: list_record(d.permits),
        annotations: list_record(d.annotations),
        javadoc: d.javadoc.map(range_record),
    }
}

fn declaration_of(r: &DeclarationRecord) -> Result<Declaration, Corrupt> {
    Ok(Declaration {
        kind: DeclKind::from_code(r.kind).ok_or(Corrupt::Value)?,
        modifiers: Modifiers::from_bits(r.modifiers).ok_or(Corrupt::Value)?,
        parent: r.parent.map(DeclId),
        name: StrId(r.name),
        name_range: range_of(r.name_range)?,
        range: range_of(r.range)?,
        signature: StrId(r.signature),
        params: list_of(r.params),
        supertypes: list_of(r.supertypes),
        permits: list_of(r.permits),
        annotations: list_of(r.annotations),
        javadoc: r.javadoc.map(range_of).transpose()?,
    })
}

/// The digest of the header's first [`DIGEST_AT`] bytes and the sections, in order.
fn checksum(sections: &[&[u8]]) -> [u8; DIGEST_LEN] {
    let mut hasher = blake3::Hasher::new();
    for section in sections {
        hasher.update(section);
    }
    let digest = hasher.finalize();
    let mut out = [0_u8; DIGEST_LEN];
    for (to, from) in out.iter_mut().zip(digest.as_bytes()) {
        *to = *from;
    }
    out
}

fn le_u32(bytes: &[u8], at: usize) -> u32 {
    let mut word = [0_u8; 4];
    if let Some(slice) = bytes.get(at..at.saturating_add(4)) {
        word.copy_from_slice(slice);
    }
    u32::from_le_bytes(word)
}

/// The entry of `summary` under `key`.
///
/// Refused only when a section passes the 32-bit lengths of the header, which no summary of a
/// file at `MAX_SOURCE_BYTES` reaches.
///
/// Pinned by: `codec::tests::should_read_back_the_summary_when_an_entry_round_trips`.
pub fn encode(key: &SyntaxKey, summary: &FileSummary) -> Result<Vec<u8>, Corrupt> {
    let parts = summary.parts();
    let names = Declarations::names_of(summary);
    let head = Head {
        language: parts.language.code(),
        text: &parts.text,
        strings: parts.strings.iter().map(|s| (s.start, s.len)).collect(),
        package: parts.package.map(|p| p.0),
        imports: parts.imports.iter().map(import_record).collect(),
        declarations: parts.declarations.iter().map(declaration_record).collect(),
        type_names: parts.type_names.iter().map(|t| t.0).collect(),
        names: names.iter().map(|n| n.0).collect(),
        syntax_errors: parts.syntax_errors,
    };
    let mut previous = 0_i64;
    let references: Vec<ReferenceRecord> = parts
        .references
        .iter()
        .map(|r| {
            let start = i64::from(r.range.start().get());
            let delta = start.wrapping_sub(previous);
            previous = start;
            ReferenceRecord {
                name: r.name.0,
                delta,
                len: r.range.len(),
                shape: r.shape.code(),
                arity: r.arity.0,
            }
        })
        .collect();
    let head = postcard::to_allocvec(&head).map_err(|_| Corrupt::Decode)?;
    let references = postcard::to_allocvec(&references).map_err(|_| Corrupt::Decode)?;
    seal(key, &head, &references)
}

/// An entry of raw `head` and `references` sections under `key`, with its header and digest.
/// [`encode`] seals the sections it writes; the property tests seal arbitrary ones, so the
/// decoders meet bytes past the digest (`tests/properties.rs`).
pub fn seal(key: &SyntaxKey, head: &[u8], references: &[u8]) -> Result<Vec<u8>, Corrupt> {
    let head_len = u32::try_from(head.len()).map_err(|_| Corrupt::Length)?;
    let refs_len = u32::try_from(references.len()).map_err(|_| Corrupt::Length)?;
    let mut entry = Vec::with_capacity(
        HEADER_LEN
            .saturating_add(head.len())
            .saturating_add(references.len()),
    );
    entry.extend_from_slice(&MAGIC);
    entry.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    entry.extend_from_slice(key.digest().as_bytes());
    entry.extend_from_slice(&head_len.to_le_bytes());
    entry.extend_from_slice(&refs_len.to_le_bytes());
    let digest = checksum(&[&entry, head, references]);
    entry.extend_from_slice(&digest);
    entry.extend_from_slice(head);
    entry.extend_from_slice(references);
    Ok(entry)
}

/// The head and reference-site sections of a checked entry.
fn sections<'e>(key: &SyntaxKey, entry: &'e [u8]) -> Result<(&'e [u8], &'e [u8]), Corrupt> {
    if u64::try_from(entry.len()).map_or(true, |len| len > MAX_ENTRY_BYTES) {
        return Err(Corrupt::Length);
    }
    let (header, payload) = entry.split_at_checked(HEADER_LEN).ok_or(Corrupt::Length)?;
    if header.get(..4) != Some(&MAGIC[..]) {
        return Err(Corrupt::Magic);
    }
    let version = le_u32(header, 4);
    if version != FORMAT_VERSION {
        return Err(Corrupt::Version(version));
    }
    if header.get(8..40) != Some(&key.digest().as_bytes()[..]) {
        return Err(Corrupt::Key);
    }
    let head_len = usize::try_from(le_u32(header, 40)).map_err(|_| Corrupt::Length)?;
    let refs_len = usize::try_from(le_u32(header, 44)).map_err(|_| Corrupt::Length)?;
    if head_len.checked_add(refs_len) != Some(payload.len()) {
        return Err(Corrupt::Length);
    }
    let stored = header.get(DIGEST_AT..HEADER_LEN).ok_or(Corrupt::Length)?;
    let signed = header.get(..DIGEST_AT).ok_or(Corrupt::Length)?;
    if stored != checksum(&[signed, payload]) {
        return Err(Corrupt::Digest);
    }
    payload.split_at_checked(head_len).ok_or(Corrupt::Length)
}

fn whole<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, Corrupt> {
    match postcard::take_from_bytes(bytes) {
        Ok((value, [])) => Ok(value),
        _ => Err(Corrupt::Decode),
    }
}

/// The parts of a head, without reference sites, and its name list.
fn head_parts(head: &[u8]) -> Result<(SummaryParts, Box<[StrId]>), Corrupt> {
    let head: Head<'_> = whole(head)?;
    let parts = SummaryParts {
        language: Language::from_code(head.language).ok_or(Corrupt::Value)?,
        text: head.text.into(),
        strings: head
            .strings
            .iter()
            .map(|&(start, len)| Span { start, len })
            .collect(),
        package: head.package.map(StrId),
        imports: head
            .imports
            .iter()
            .map(import_of)
            .collect::<Result<_, _>>()?,
        declarations: head
            .declarations
            .iter()
            .map(declaration_of)
            .collect::<Result<_, _>>()?,
        type_names: head.type_names.iter().copied().map(StrId).collect(),
        references: Box::new([]),
        syntax_errors: head.syntax_errors,
    };
    Ok((parts, head.names.into_iter().map(StrId).collect()))
}

fn references_of(bytes: &[u8]) -> Result<Box<[ReferenceSite]>, Corrupt> {
    let records: Vec<ReferenceRecord> = whole(bytes)?;
    let mut previous = 0_i64;
    records
        .iter()
        .map(|r| {
            let start = previous.checked_add(r.delta).ok_or(Corrupt::Value)?;
            previous = start;
            let start = u32::try_from(start).map_err(|_| Corrupt::Value)?;
            Ok(ReferenceSite {
                name: StrId(r.name),
                range: range_of((start, r.len))?,
                shape: RefShape::from_code(r.shape).ok_or(Corrupt::Value)?,
                arity: Arity(r.arity),
            })
        })
        .collect()
}

/// The whole summary in `entry`, checked as the module documentation says.
///
/// Pinned by: `codec::tests` and `tests/properties.rs`.
pub fn decode(key: &SyntaxKey, entry: &[u8]) -> Result<FileSummary, Corrupt> {
    let (head, references) = sections(key, entry)?;
    let (mut parts, _names) = head_parts(head)?;
    parts.references = references_of(references)?;
    Ok(FileSummary::from_parts(parts)?)
}

/// The declarations in `entry`, without decoding its reference sites.
///
/// **A known limit, for IDX-11:** the name list is taken from the head as written, without the
/// sites it summarises, so it is only as complete as the writer made it. [`encode`] derives it from
/// the sites, and the digest covers it, so a damaged entry cannot under-list names; but the digest
/// is not keyed, and an entry forged with a valid digest (only by the store's own user, plan
/// §5.3) could leave a name out and so a file out of the postings. Before `refs` claims
/// `complete=true` from the postings, IDX-11 decides whether that is acceptable or re-derives the
/// names from the sites.
///
/// Pinned by: `codec::tests` and `tests/properties.rs`.
pub fn decode_declarations(key: &SyntaxKey, entry: &[u8]) -> Result<Declarations, Corrupt> {
    let (head, _) = sections(key, entry)?;
    let (parts, names) = head_parts(head)?;
    Ok(Declarations::from_parts(
        FileSummary::from_parts(parts)?,
        names,
    )?)
}

#[cfg(test)]
mod tests {
    use jfl_domain::{Digest, SummaryBuilder};

    use super::*;

    fn key(n: u8) -> SyntaxKey {
        SyntaxKey::from_digest(Digest::from_bytes([n; 32]))
    }

    fn range(start: u32, end: u32) -> ByteRange {
        ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).unwrap()
    }

    fn summary() -> FileSummary {
        let mut b = SummaryBuilder::new(Language::Java);
        b.package("a.b").unwrap();
        b.import("java.util.List", false, false, range(10, 30))
            .unwrap();
        b.module_import("java.base", range(31, 50)).unwrap();
        let class = b
            .declaration(&jfl_domain::DeclarationInput {
                kind: DeclKind::Class,
                modifiers: Modifiers::PUBLIC.with(Modifiers::FINAL),
                parent: None,
                name: "A",
                name_range: range(60, 61),
                range: range(51, 200),
                signature: "@Data public final class A extends B implements C",
                params: &[],
                supertypes: &["B", "C"],
                permits: &[],
                annotations: &["Data"],
                javadoc: Some(range(40, 50)),
            })
            .unwrap();
        b.declaration(&jfl_domain::DeclarationInput {
            kind: DeclKind::Method,
            modifiers: Modifiers::STATIC,
            parent: Some(class),
            name: "run",
            name_range: range(80, 83),
            range: range(70, 150),
            signature: "static void run(List<String> xs)",
            params: &["List"],
            supertypes: &[],
            permits: &[],
            annotations: &[],
            javadoc: None,
        })
        .unwrap();
        for (name, at, end, shape, arity) in [
            ("List", 90, 94, RefShape::TypeUse, 0),
            ("size", 120, 124, RefShape::QualifiedCall, 2),
            ("List", 130, 134, RefShape::Name, 0),
            ("ok", 100, 102, RefShape::Call, 255),
        ] {
            b.reference(name, range(at, end), shape, Arity(arity))
                .unwrap();
        }
        b.finish()
    }

    #[test]
    fn should_read_back_the_summary_when_an_entry_round_trips() {
        // given
        let summary = summary();
        // when
        let entry = encode(&key(1), &summary).unwrap();
        // then
        assert_eq!(decode(&key(1), &entry), Ok(summary.clone()));
        assert_eq!(
            decode_declarations(&key(1), &entry),
            Ok(Declarations::of(&summary))
        );
    }

    #[test]
    fn should_keep_the_syntax_error_mark_when_an_entry_round_trips() {
        // given: a summary of a file with a syntax error
        let marked = summary().with_syntax_errors(true);
        // when
        let entry = encode(&key(1), &marked).unwrap();
        // then
        let back = decode(&key(1), &entry).unwrap();
        assert!(back.syntax_errors());
        assert_eq!(back, marked);
        assert!(
            !decode(&key(1), &encode(&key(1), &summary()).unwrap())
                .unwrap()
                .syntax_errors()
        );
    }

    #[test]
    fn should_refuse_an_entry_when_its_language_code_is_unknown() {
        // given: a well-sealed entry whose head names a language no build knows (the language
        // is the head's first field, one byte)
        let entry = encode(&key(1), &summary()).unwrap();
        let head_len = usize::try_from(u32::from_le_bytes([
            entry[40], entry[41], entry[42], entry[43],
        ]))
        .unwrap();
        let (head, references) = entry[HEADER_LEN..].split_at(head_len);
        assert_eq!(head[0], Language::Java.code());
        let mut unknown = head.to_vec();
        unknown[0] = 0x7f;
        let resealed = seal(&key(1), &unknown, references).unwrap();
        // when
        let summary = decode(&key(1), &resealed);
        let declarations = decode_declarations(&key(1), &resealed);
        // then
        assert_eq!(summary, Err(Corrupt::Value));
        assert!(matches!(declarations, Err(Corrupt::Value)));
    }

    #[test]
    fn should_refuse_an_entry_when_its_header_or_bytes_disagree() {
        // given
        let entry = encode(&key(1), &summary()).unwrap();
        let mut version = entry.clone();
        version[4] = 1;
        let mut flipped = entry.clone();
        if let Some(last) = flipped.last_mut() {
            *last ^= 1;
        }
        let shorter = entry.len().checked_sub(1).unwrap();
        let mut magic = entry.clone();
        magic[0] = b'X';
        // when
        let refused = [
            decode(&key(2), &entry),
            decode(&key(1), &entry[..shorter]),
            decode(&key(1), &entry[..63]),
            decode(&key(1), &version),
            decode(&key(1), &flipped),
            decode(&key(1), &magic),
            decode(&key(1), &[]),
        ];
        // then
        assert_eq!(
            refused.map(|r| r.err()),
            [
                Some(Corrupt::Key),
                Some(Corrupt::Length),
                Some(Corrupt::Length),
                Some(Corrupt::Version(1)),
                Some(Corrupt::Digest),
                Some(Corrupt::Magic),
                Some(Corrupt::Length),
            ]
        );
    }

    #[test]
    fn should_keep_an_entry_small_when_it_encodes_reference_sites() {
        // given
        let summary = summary();
        // when
        let entry = encode(&key(1), &summary).unwrap();
        // then
        assert!(
            entry.len() < summary.heap_bytes(),
            "{} encoded against {} in memory",
            entry.len(),
            summary.heap_bytes()
        );
    }
}
