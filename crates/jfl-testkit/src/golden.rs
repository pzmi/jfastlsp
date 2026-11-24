//! The golden Java corpus (ver §4): every `.java` file under `fixtures/java/` (but
//! `fixtures/java/summaries/`) and its expected summary, `fixtures/java/summaries/<grammar>/<path>.json`.
//!
//! **The format** is JSON, one record per line so a review reads a diff line by line: the
//! language (IDX-17), the package,
//! the imports, the declarations (with their symbol identity, when they have one), the reference
//! sites and the syntax diagnostics. A range is `line:column-line:column`, 1-based, columns in
//! UTF-8 bytes (`jfl`'s CLI convention, plan §2.3), of the file's text after a dropped byte-order
//! mark. [`render`] writes it; [`parse`] reads it back into the [`Indexed`] it describes, building
//! the summary in the order the adapter does, so a parsed golden equals the adapter's value (the
//! canned fake answers with it). Derived fields (`symbol`) are written for the reader and
//! checked, not read.
//!
//! **Goldens are never rewritten to get green** (ver §4). [`check_corpus`] compares each corpus
//! file's summary with its golden and fails on any difference; a corpus file with no golden gets
//! one written and fails too ("review and commit it"), so a new fixture's golden is reviewed
//! before it passes. A deliberate change deletes the golden, reruns, reviews the new file and
//! moves its line in `fixtures.lock` (`just fixtures-lock`, `ARCH-fixtures-lock`).
//!
//! Pinned by: `golden::tests` (a summary survives render then parse) and the adapter's
//! `tests/goldens.rs`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use jfl_application::syntax_indexer::{Indexed, SyntaxDiagnostic, SyntaxProblem, source_text};
use jfl_domain::{
    Arity, ByteRange, DeclId, DeclKind, DeclarationInput, Language, LineIndex, Modifiers, Position,
    PositionEncoding, RefShape, SummaryBuilder,
};
use serde_json::{Map, Value, json};

/// The corpus root, `fixtures/java/`.
#[must_use]
pub fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java")
}

/// The goldens' directory under the corpus root.
pub const SUMMARIES: &str = "summaries";

/// One corpus file.
#[derive(Debug, Clone)]
pub struct CorpusFile {
    /// Its path under `fixtures/java/`, `/`-separated.
    pub path: String,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

/// Every `.java` file under `fixtures/java/<dir>` (`""` for all), sorted, `summaries/` left out.
pub fn corpus(dir: &str) -> Result<Vec<CorpusFile>, String> {
    let root = corpus_root();
    let mut found = Vec::new();
    let mut pending = vec![root.join(dir)];
    while let Some(current) = pending.pop() {
        let entries =
            std::fs::read_dir(&current).map_err(|e| format!("{}: {e}", current.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let rel = path
                .strip_prefix(&root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if kind.is_dir() {
                if rel != SUMMARIES {
                    pending.push(path);
                }
            } else if rel.ends_with(".java") {
                let bytes = std::fs::read(&path).map_err(|e| format!("{rel}: {e}"))?;
                found.push(CorpusFile { path: rel, bytes });
            }
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

/// The golden of `file` for `grammar` (`orchard`, `upstream`).
#[must_use]
pub fn golden_path(grammar: &str, file: &str) -> PathBuf {
    corpus_root()
        .join(SUMMARIES)
        .join(grammar)
        .join(format!("{file}.json"))
}

fn kind_name(kind: DeclKind) -> &'static str {
    match kind {
        DeclKind::Class => "class",
        DeclKind::Interface => "interface",
        DeclKind::Enum => "enum",
        DeclKind::Record => "record",
        DeclKind::Annotation => "annotation",
        DeclKind::Method => "method",
        DeclKind::Constructor => "constructor",
        DeclKind::Field => "field",
        DeclKind::EnumConstant => "enum_constant",
        DeclKind::RecordComponent => "record_component",
        DeclKind::AnnotationElement => "annotation_element",
    }
}

fn shape_name(shape: RefShape) -> &'static str {
    match shape {
        RefShape::Call => "call",
        RefShape::QualifiedCall => "qualified_call",
        RefShape::New => "new",
        RefShape::MethodRef => "method_ref",
        RefShape::TypeUse => "type",
        RefShape::Name => "name",
        RefShape::QualifiedName => "qualified_name",
        RefShape::Annotation => "annotation",
    }
}

const MODIFIERS: [(Modifiers, &str); 15] = [
    (Modifiers::PUBLIC, "public"),
    (Modifiers::PROTECTED, "protected"),
    (Modifiers::PRIVATE, "private"),
    (Modifiers::STATIC, "static"),
    (Modifiers::FINAL, "final"),
    (Modifiers::ABSTRACT, "abstract"),
    (Modifiers::DEFAULT, "default"),
    (Modifiers::SEALED, "sealed"),
    (Modifiers::NON_SEALED, "non-sealed"),
    (Modifiers::SYNCHRONIZED, "synchronized"),
    (Modifiers::NATIVE, "native"),
    (Modifiers::TRANSIENT, "transient"),
    (Modifiers::VOLATILE, "volatile"),
    (Modifiers::STRICTFP, "strictfp"),
    (Modifiers::VARARGS, "varargs"),
];

fn problem_value(problem: &SyntaxProblem) -> Value {
    match problem {
        SyntaxProblem::Unexpected => json!("unexpected"),
        SyntaxProblem::Missing(what) => json!({ "missing": what.as_ref() }),
        SyntaxProblem::StringTemplate => json!("string_template"),
    }
}

struct Lines<'a> {
    index: LineIndex,
    text: &'a str,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Result<Self, String> {
        Ok(Self {
            index: LineIndex::new(text).map_err(|e| e.to_string())?,
            text,
        })
    }

    fn at(&self, range: ByteRange) -> Result<String, String> {
        let pos = |offset| {
            self.index
                .position(offset, PositionEncoding::Utf8)
                .map_err(|e| format!("{offset:?} of {range:?}: {e}"))
        };
        let (start, end) = (pos(range.start())?, pos(range.end())?);
        Ok(format!(
            "{}:{}-{}:{}",
            start.line().saturating_add(1),
            start.column().saturating_add(1),
            end.line().saturating_add(1),
            end.column().saturating_add(1)
        ))
    }

    fn range(&self, at: &str) -> Result<ByteRange, String> {
        let bad = || format!("bad range {at:?}");
        let (start, end) = at.split_once('-').ok_or_else(bad)?;
        let offset = |text: &str| -> Result<_, String> {
            let (line, column) = text.split_once(':').ok_or_else(bad)?;
            let line: u32 = line.parse().map_err(|_| bad())?;
            let column: u32 = column.parse().map_err(|_| bad())?;
            let position = Position::new(line.saturating_sub(1), column.saturating_sub(1));
            self.index
                .offset(position, PositionEncoding::Utf8)
                .map_err(|e| format!("{at:?}: {e}"))
        };
        ByteRange::new(offset(start)?, offset(end)?).map_err(|e| e.to_string())
    }

    fn slice(&self, range: ByteRange) -> &str {
        let start = usize::try_from(range.start().get()).unwrap_or(usize::MAX);
        let end = usize::try_from(range.end().get()).unwrap_or(usize::MAX);
        self.text.get(start..end).unwrap_or("")
    }
}

/// `indexed`, the summary of `bytes` (a whole file, byte-order mark included), in the golden
/// format.
pub fn render(indexed: &Indexed, bytes: &[u8]) -> Result<String, String> {
    let text = source_text(bytes).map_err(|e| e.to_string())?;
    let lines = Lines::new(text)?;
    let summary = &indexed.summary;
    let mut imports = Vec::new();
    for import in summary.imports() {
        imports.push(json!({
            "name": summary.str(import.name),
            "static": import.is_static,
            "on_demand": import.on_demand,
            "module": import.module,
            "at": lines.at(import.range)?,
        }));
    }
    let mut declarations = Vec::new();
    for (id, d) in summary.declarations() {
        let list = |l| summary.list(l).collect::<Vec<&str>>();
        let modifiers: Vec<&str> = MODIFIERS
            .iter()
            .filter(|(m, _)| d.modifiers.contains(*m))
            .map(|(_, name)| *name)
            .collect();
        let symbol = summary
            .symbol_id(id)
            .map_or_else(|e| format!("({e})"), |s| s.to_string());
        declarations.push(json!({
            "id": id.0,
            "kind": kind_name(d.kind),
            "parent": d.parent.map(|p| p.0),
            "name": summary.str(d.name),
            "name_at": lines.at(d.name_range)?,
            "at": lines.at(d.range)?,
            "signature": summary.str(d.signature),
            "modifiers": modifiers,
            "params": list(d.params),
            "supertypes": list(d.supertypes),
            "permits": list(d.permits),
            "annotations": list(d.annotations),
            "javadoc": d.javadoc.map(|j| lines.at(j)).transpose()?,
            "symbol": symbol,
        }));
    }
    let mut references = Vec::new();
    for r in summary.references() {
        let mut site = json!({
            "name": summary.str(r.name),
            "at": lines.at(r.range)?,
            "shape": shape_name(r.shape),
        });
        if r.shape.has_arity()
            && let Some(map) = site.as_object_mut()
        {
            map.insert("arity".to_owned(), json!(r.arity.0));
        }
        references.push(site);
    }
    let mut diagnostics = Vec::new();
    for d in &indexed.diagnostics {
        diagnostics.push(json!({
            "at": lines.at(d.range)?,
            "problem": problem_value(&d.problem),
            "text": truncate(lines.slice(d.range), 40),
        }));
    }
    let mut out = String::from("{\n");
    let _ = writeln!(out, "  \"language\": {},", json!(summary.language().name()));
    let package = json!(summary.package());
    let _ = writeln!(out, "  \"package\": {package},");
    for (key, items) in [
        ("imports", &imports),
        ("declarations", &declarations),
        ("references", &references),
        ("diagnostics", &diagnostics),
    ] {
        let _ = write!(out, "  \"{key}\": [");
        for (i, item) in items.iter().enumerate() {
            let sep = if i == 0 { "\n" } else { ",\n" };
            let _ = write!(out, "{sep}    {item}");
        }
        out.push_str(if items.is_empty() { "],\n" } else { "\n  ],\n" });
    }
    let _ = writeln!(
        out,
        "  \"diagnostics_dropped\": {}",
        indexed.diagnostics_dropped
    );
    out.push_str("}\n");
    Ok(out)
}

fn truncate(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().nth(max).is_some() {
        out.push('…');
    }
    out
}

fn field<'v>(object: &'v Map<String, Value>, key: &str) -> Result<&'v Value, String> {
    object.get(key).ok_or_else(|| format!("missing `{key}`"))
}

fn string<'v>(object: &'v Map<String, Value>, key: &str) -> Result<&'v str, String> {
    field(object, key)?
        .as_str()
        .ok_or_else(|| format!("`{key}` is not a string"))
}

fn strings<'v>(object: &'v Map<String, Value>, key: &str) -> Result<Vec<&'v str>, String> {
    field(object, key)?
        .as_array()
        .ok_or_else(|| format!("`{key}` is not a list"))?
        .iter()
        .map(|v| {
            v.as_str()
                .ok_or_else(|| format!("`{key}` holds a non-string"))
        })
        .collect()
}

fn objects<'v>(
    object: &'v Map<String, Value>,
    key: &str,
) -> Result<Vec<&'v Map<String, Value>>, String> {
    field(object, key)?
        .as_array()
        .ok_or_else(|| format!("`{key}` is not a list"))?
        .iter()
        .map(|v| {
            v.as_object()
                .ok_or_else(|| format!("`{key}` holds a non-object"))
        })
        .collect()
}

/// The [`Indexed`] a golden describes, for `bytes` (the file it is the golden of).
pub fn parse(golden: &str, bytes: &[u8]) -> Result<Indexed, String> {
    let text = source_text(bytes).map_err(|e| e.to_string())?;
    let lines = Lines::new(text)?;
    let root: Value = serde_json::from_str(golden).map_err(|e| e.to_string())?;
    let root = root.as_object().ok_or("the golden is not an object")?;
    let language = string(root, "language")?;
    let language =
        Language::from_name(language).ok_or_else(|| format!("unknown language {language}"))?;
    let mut builder = SummaryBuilder::new(language);
    if let Some(package) = field(root, "package")?.as_str() {
        builder.package(package).map_err(|e| e.to_string())?;
    }
    for import in objects(root, "imports")? {
        let flag = |key| field(import, key).map(|v| v.as_bool() == Some(true));
        let range = lines.range(string(import, "at")?)?;
        let name = string(import, "name")?;
        if flag("module")? {
            builder.module_import(name, range)
        } else {
            builder.import(name, flag("static")?, flag("on_demand")?, range)
        }
        .map_err(|e| e.to_string())?;
    }
    let kinds: BTreeMap<&str, DeclKind> =
        DeclKind::ALL.iter().map(|&k| (kind_name(k), k)).collect();
    for d in objects(root, "declarations")? {
        let kind = *kinds
            .get(string(d, "kind")?)
            .ok_or("unknown declaration kind")?;
        let mut modifiers = Modifiers::empty();
        for name in strings(d, "modifiers")? {
            let (bit, _) = MODIFIERS
                .iter()
                .find(|(_, n)| *n == name)
                .ok_or_else(|| format!("unknown modifier {name}"))?;
            modifiers = modifiers.with(*bit);
        }
        let parent = match field(d, "parent")?.as_u64() {
            Some(p) => Some(DeclId(u32::try_from(p).map_err(|e| e.to_string())?)),
            None => None,
        };
        let javadoc = match field(d, "javadoc")?.as_str() {
            Some(at) => Some(lines.range(at)?),
            None => None,
        };
        let (params, supertypes) = (strings(d, "params")?, strings(d, "supertypes")?);
        let (permits, annotations) = (strings(d, "permits")?, strings(d, "annotations")?);
        builder
            .declaration(&DeclarationInput {
                kind,
                modifiers,
                parent,
                name: string(d, "name")?,
                name_range: lines.range(string(d, "name_at")?)?,
                range: lines.range(string(d, "at")?)?,
                signature: string(d, "signature")?,
                params: &params,
                supertypes: &supertypes,
                permits: &permits,
                annotations: &annotations,
                javadoc,
            })
            .map_err(|e| e.to_string())?;
    }
    let shapes: BTreeMap<&str, RefShape> =
        RefShape::ALL.iter().map(|&s| (shape_name(s), s)).collect();
    for r in objects(root, "references")? {
        let shape = *shapes.get(string(r, "shape")?).ok_or("unknown shape")?;
        let arity = match r.get("arity").and_then(Value::as_u64) {
            Some(a) => Arity(u8::try_from(a).map_err(|e| e.to_string())?),
            None => Arity(0),
        };
        builder
            .reference(
                string(r, "name")?,
                lines.range(string(r, "at")?)?,
                shape,
                arity,
            )
            .map_err(|e| e.to_string())?;
    }
    let mut diagnostics = Vec::new();
    for d in objects(root, "diagnostics")? {
        let problem = match field(d, "problem")? {
            Value::String(s) if s == "unexpected" => SyntaxProblem::Unexpected,
            Value::String(s) if s == "string_template" => SyntaxProblem::StringTemplate,
            Value::Object(o) => SyntaxProblem::Missing(string(o, "missing")?.into()),
            other => return Err(format!("unknown problem {other}")),
        };
        diagnostics.push(SyntaxDiagnostic {
            range: lines.range(string(d, "at")?)?,
            problem,
        });
    }
    let dropped = field(root, "diagnostics_dropped")?
        .as_u64()
        .and_then(|d| u32::try_from(d).ok())
        .ok_or("bad `diagnostics_dropped`")?;
    Ok(Indexed {
        summary: builder.finish(),
        diagnostics,
        diagnostics_dropped: dropped,
    })
}

/// The committed known-gaps list of a grammar (`summaries/<grammar>/known-gaps.txt`): the corpus
/// files it is expected to report syntax errors for, one path per line, `#` comments allowed.
pub fn known_gaps(grammar: &str) -> Result<Vec<String>, String> {
    let path = corpus_root()
        .join(SUMMARIES)
        .join(grammar)
        .join("known-gaps.txt");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut gaps: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    gaps.sort();
    Ok(gaps)
}

/// A corpus file whose summary differs from its golden, or has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// The corpus file.
    pub path: String,
    /// What differs, as the first differing line of each, or why it could not be compared.
    pub detail: String,
}

/// The first line where `want` and `got` differ, with its number.
#[must_use]
pub fn first_difference(want: &str, got: &str) -> Option<String> {
    let mut want_lines = want.lines();
    let mut got_lines = got.lines();
    for number in 1_usize.. {
        match (want_lines.next(), got_lines.next()) {
            (None, None) => return None,
            (w, g) if w == g => {}
            (w, g) => {
                return Some(format!(
                    "line {number}:\n  golden: {}\n  actual: {}",
                    w.unwrap_or("<end>"),
                    g.unwrap_or("<end>")
                ));
            }
        }
    }
    None
}

/// Compares the summary `summarise` gives each of `files` with its golden for `grammar`; writes
/// the golden of a file that has none (and reports it, so the run fails until it is reviewed).
pub fn check_corpus(
    grammar: &str,
    files: &[CorpusFile],
    mut summarise: impl FnMut(&CorpusFile) -> Result<String, String>,
) -> Vec<Mismatch> {
    let mut out = Vec::new();
    for file in files {
        let got = match summarise(file) {
            Ok(got) => got,
            Err(detail) => {
                out.push(Mismatch {
                    path: file.path.clone(),
                    detail,
                });
                continue;
            }
        };
        let path = golden_path(grammar, &file.path);
        match std::fs::read_to_string(&path) {
            Ok(want) => {
                if let Some(detail) = first_difference(&want, &got) {
                    out.push(Mismatch {
                        path: file.path.clone(),
                        detail,
                    });
                }
            }
            Err(_) => {
                let written = path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| std::fs::write(&path, &got));
                out.push(Mismatch {
                    path: file.path.clone(),
                    detail: match written {
                        Ok(()) => format!(
                            "no golden: wrote {}; review it, then commit it with its fixtures.lock line",
                            path.display()
                        ),
                        Err(e) => format!("no golden, and writing one failed: {e}"),
                    },
                });
            }
        }
    }
    out
}

/// Every file under `dir` whose name ends with `suffix`: its path relative to `dir`
/// (`/`-separated) and its bytes, sorted by path. For fixtures outside `fixtures/java/` (a crate's
/// `tests/fixtures/`), read by tests of crates that hold no filesystem grant.
pub fn files_under(dir: &Path, suffix: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries =
            std::fs::read_dir(&current).map_err(|e| format!("{}: {e}", current.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                pending.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(dir)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if rel.ends_with(suffix) {
                let bytes = std::fs::read(&path).map_err(|e| format!("{rel}: {e}"))?;
                found.push((rel, bytes));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

/// Compares `got` with the file at `path`; writes it when there is none and fails, so a new
/// generated fixture is reviewed (and given its `fixtures.lock` line) before it passes.
pub fn check_file(path: &Path, got: &str) -> Result<(), String> {
    match std::fs::read_to_string(path) {
        Ok(want) => {
            first_difference(&want, got).map_or(Ok(()), |d| Err(format!("{}: {d}", path.display())))
        }
        Err(_) => {
            path.parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(path, got))
                .map_err(|e| format!("{}: writing it failed: {e}", path.display()))?;
            Err(format!(
                "{}: no such file: wrote it; review it, then commit it with its fixtures.lock line",
                path.display()
            ))
        }
    }
}

/// Every golden of `grammar` whose corpus file is not among `files`: a deleted fixture's golden.
pub fn stale_goldens(grammar: &str, files: &[CorpusFile]) -> Result<Vec<String>, String> {
    let root = corpus_root().join(SUMMARIES).join(grammar);
    let mut stale = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(&root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let Some(java) = rel.strip_suffix(".json") else {
                continue;
            };
            if !files.iter().any(|f| f.path == java) {
                stale.push(rel);
            }
        }
    }
    stale.sort();
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jfl_domain::ByteOffset;

    fn range(start: u32, end: u32) -> ByteRange {
        ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).unwrap()
    }

    #[test]
    fn should_read_back_what_it_wrote_when_a_summary_is_rendered() {
        // given
        let text = b"package p;\nimport static a.B.c;\nclass A {\n  void f(int x) { g(x); }\n}\n";
        let mut builder = SummaryBuilder::new(Language::Java);
        builder.package("p").unwrap();
        builder.import("a.B.c", true, false, range(11, 32)).unwrap();
        let class = builder
            .declaration(&DeclarationInput {
                kind: DeclKind::Class,
                modifiers: Modifiers::empty(),
                parent: None,
                name: "A",
                name_range: range(39, 40),
                range: range(33, 67),
                signature: "class A",
                params: &[],
                supertypes: &[],
                permits: &[],
                annotations: &[],
                javadoc: None,
            })
            .unwrap();
        builder
            .declaration(&DeclarationInput {
                kind: DeclKind::Method,
                modifiers: Modifiers::VARARGS,
                parent: Some(class),
                name: "f",
                name_range: range(50, 51),
                range: range(45, 65),
                signature: "void f(int x)",
                params: &["int"],
                supertypes: &[],
                permits: &[],
                annotations: &["Deprecated"],
                javadoc: Some(range(0, 3)),
            })
            .unwrap();
        builder
            .reference("g", range(61, 62), RefShape::Call, Arity(1))
            .unwrap();
        let indexed = Indexed {
            summary: builder.finish(),
            diagnostics: vec![SyntaxDiagnostic {
                range: range(62, 62),
                problem: SyntaxProblem::Missing(";".into()),
            }],
            diagnostics_dropped: 2,
        };
        // when
        let golden = render(&indexed, text).unwrap();
        let back = parse(&golden, text).unwrap();
        // then
        assert_eq!(back, indexed, "{golden}");
        assert!(golden.contains("\"symbol\":\"p.A#f(int)\""), "{golden}");
    }

    #[test]
    fn should_name_the_first_differing_line_when_texts_differ() {
        // given
        let (want, got) = ("a\nb\nc\n", "a\nx\nc\n");
        // when
        let difference = first_difference(want, got);
        // then
        assert_eq!(
            difference.as_deref(),
            Some("line 2:\n  golden: b\n  actual: x")
        );
        assert_eq!(first_difference(want, want), None);
    }
}
