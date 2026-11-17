//! E1 parse-speed spike: list -> read -> blake3 -> tree-sitter parse -> declaration/import walk.
//!
//! Usage: parse-spike <repo-root> [--threads N] [--upto read|hash|parse|extract]
//!                    [--grammar orchard|upstream] [--errors-out FILE]
//! Stages are cumulative: `--upto parse` reads, hashes and parses (but does not walk).
//! Prints one machine-readable `RESULT key=value ...` line.

use std::path::{Path, PathBuf};
use std::time::Instant;

use rayon::prelude::*;
use tree_sitter::{Node, Parser, Tree};

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Upto {
    Read = 0,
    Hash = 1,
    Parse = 2,
    Extract = 3,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum Kind {
    Package,
    Import,
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
    Method,
    Constructor,
    Field,
}

#[allow(dead_code)]
struct Decl {
    kind: Kind,
    name: Box<str>,
    start: (u32, u32),
    end: (u32, u32),
}

#[allow(dead_code)]
struct FileOut {
    hash: Option<[u8; 32]>,
    decls: Vec<Decl>,
    bytes: usize,
    has_error: bool,
    nanos: u64,
}

fn list_java(root: &Path) -> Vec<PathBuf> {
    // `ignore` honours .gitignore like an editor-side indexer would; hidden dirs skipped.
    let mut out = Vec::new();
    for e in ignore::WalkBuilder::new(root).build().flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "java") && e.file_type().is_some_and(|t| t.is_file())
        {
            out.push(p.to_path_buf());
        }
    }
    out.sort();
    out
}

fn text<'a>(n: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[n.byte_range()]).unwrap_or("")
}

fn push(decls: &mut Vec<Decl>, kind: Kind, n: Node, src: &[u8]) {
    let s = n.start_position();
    let e = n.end_position();
    decls.push(Decl {
        kind,
        name: text(n, src).into(),
        start: (s.row as u32, s.column as u32),
        end: (e.row as u32, e.column as u32),
    });
}

fn extract(tree: &Tree, src: &[u8]) -> Vec<Decl> {
    let mut decls = Vec::new();
    let mut c = tree.walk();
    'outer: loop {
        let n = c.node();
        let mut descend = true; // full walk: also finds local and anonymous classes
        let kind = match n.kind() {
            "package_declaration" | "import_declaration" => {
                // whole qualified name as one entry; no need to descend
                descend = false;
                let k = if n.kind() == "package_declaration" { Kind::Package } else { Kind::Import };
                let mut nc = n.walk();
                if let Some(q) = n
                    .named_children(&mut nc)
                    .find(|x| matches!(x.kind(), "scoped_identifier" | "identifier"))
                {
                    push(&mut decls, k, q, src);
                }
                None
            }
            "class_declaration" => Some(Kind::Class),
            "interface_declaration" => Some(Kind::Interface),
            "enum_declaration" => Some(Kind::Enum),
            "record_declaration" => Some(Kind::Record),
            "annotation_type_declaration" => Some(Kind::Annotation),
            "method_declaration" | "annotation_type_element_declaration" => Some(Kind::Method),
            "constructor_declaration" | "compact_constructor_declaration" => {
                Some(Kind::Constructor)
            }
            "field_declaration" | "constant_declaration" => {
                let mut nc = n.walk();
                for d in n.children_by_field_name("declarator", &mut nc) {
                    if let Some(name) = d.child_by_field_name("name") {
                        push(&mut decls, Kind::Field, name, src);
                    }
                }
                None
            }
            _ => None,
        };
        if let Some(k) = kind
            && let Some(name) = n.child_by_field_name("name")
        {
            push(&mut decls, k, name, src);
        }
        if descend && c.goto_first_child() {
            continue;
        }
        loop {
            if c.goto_next_sibling() {
                continue 'outer;
            }
            if !c.goto_parent() {
                break 'outer;
            }
        }
    }
    decls
}

fn process(parser: &mut Parser, path: &Path, upto: Upto) -> FileOut {
    let t0 = Instant::now();
    let src = std::fs::read(path).unwrap_or_default();
    let bytes = src.len();
    let mut out = FileOut { hash: None, decls: Vec::new(), bytes, has_error: false, nanos: 0 };
    if upto >= Upto::Hash {
        out.hash = Some(*blake3::hash(&src).as_bytes());
    }
    if upto >= Upto::Parse {
        let tree = parser.parse(&src, None).expect("parse");
        out.has_error = tree.root_node().has_error();
        if upto >= Upto::Extract {
            out.decls = extract(&tree, &src);
        }
    }
    out.nanos = t0.elapsed().as_nanos() as u64;
    out
}

fn pct(sorted: &[u64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i] as f64 / 1e6
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let mut threads = 1usize;
    let mut upto = Upto::Extract;
    let mut grammar = "orchard".to_string();
    let mut errors_out: Option<String> = None;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--threads" => threads = args[i + 1].parse().unwrap(),
            "--upto" => {
                upto = match args[i + 1].as_str() {
                    "read" => Upto::Read,
                    "hash" => Upto::Hash,
                    "parse" => Upto::Parse,
                    _ => Upto::Extract,
                }
            }
            "--grammar" => grammar = args[i + 1].clone(),
            "--errors-out" => errors_out = Some(args[i + 1].clone()),
            a => panic!("unknown arg {a}"),
        }
        i += 2;
    }
    let lang: tree_sitter::Language = match grammar.as_str() {
        "upstream" => tree_sitter_java::LANGUAGE.into(),
        _ => tree_sitter_java_orchard::LANGUAGE.into(),
    };
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();

    let t_all = Instant::now();
    let files = list_java(&root);
    let list_ms = t_all.elapsed().as_secs_f64() * 1e3;

    let t_work = Instant::now();
    let results: Vec<FileOut> = pool.install(|| {
        files
            .par_iter()
            .map_init(
                || {
                    let mut p = Parser::new();
                    p.set_language(&lang).unwrap();
                    p
                },
                |p, f| process(p, f, upto),
            )
            .collect()
    });
    let work_ms = t_work.elapsed().as_secs_f64() * 1e3;
    let total_ms = t_all.elapsed().as_secs_f64() * 1e3;

    let bytes: usize = results.iter().map(|r| r.bytes).sum();
    let decls: usize = results.iter().map(|r| r.decls.len()).sum();
    let errs = results.iter().filter(|r| r.has_error).count();
    let mut ns: Vec<u64> = results.iter().map(|r| r.nanos).collect();
    ns.sort_unstable();
    if let Some(path) = errors_out {
        let s: String = files
            .iter()
            .zip(&results)
            .filter(|(_, r)| r.has_error)
            .map(|(f, _)| format!("{}\n", f.display()))
            .collect();
        std::fs::write(path, s).unwrap();
    }
    let n = files.len() as f64;
    println!(
        "RESULT grammar={grammar} threads={threads} upto={} files={} mb={:.1} list_ms={:.0} work_ms={:.0} total_ms={:.0} files_per_s={:.0} mb_per_s={:.1} p50_ms={:.3} p95_ms={:.3} max_ms={:.1} err_files={} err_pct={:.2} decls={}",
        ["read", "hash", "parse", "extract"][upto as usize],
        files.len(),
        bytes as f64 / 1e6,
        list_ms,
        work_ms,
        total_ms,
        n / (work_ms / 1e3),
        bytes as f64 / 1e6 / (work_ms / 1e3),
        pct(&ns, 0.5),
        pct(&ns, 0.95),
        pct(&ns, 1.0),
        errs,
        100.0 * errs as f64 / n,
        decls
    );
}
