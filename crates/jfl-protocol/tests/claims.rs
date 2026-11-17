//! `complete=true` stays the domain's to prove (ADR 0014, `src/envelope.rs`'s module
//! documentation): this crate turns a domain envelope into its wire form and never the other way.
//! An allowlist pins what it may name of `jfl_domain`: the items it imports or names by path,
//! and the associated items (constructors, variants) it reaches through them. A new name, a
//! domain completeness or envelope builder above all, fails until someone adds it here and says
//! why. The check reads the crate's own sources (code lines, not comments, before any
//! `#[cfg(test)]` module) at compile time.

const SOURCES: &[(&str, &str)] = &[
    ("codec.rs", include_str!("../src/codec.rs")),
    ("envelope.rs", include_str!("../src/envelope.rs")),
    ("frame.rs", include_str!("../src/frame.rs")),
    ("lib.rs", include_str!("../src/lib.rs")),
    ("messages.rs", include_str!("../src/messages.rs")),
    ("rpc.rs", include_str!("../src/rpc.rs")),
    ("uri.rs", include_str!("../src/uri.rs")),
    ("version.rs", include_str!("../src/version.rs")),
];

const LIB: &str = include_str!("../src/lib.rs");

/// The `jfl_domain` items this crate may name. `Completeness` and `Envelope` are read only
/// (`From<&Completeness>`, `WireEnvelope::from_domain`); building one is not allowed below.
const ALLOWED_ITEMS: &[&str] = &[
    "Completeness",
    "DeclKind",
    "Envelope",
    "FileUri",
    "Location",
    "ObjectId",
    "Position",
    "PositionEncoding",
    "PossibleHit",
    "Range",
    "Reason",
    "Target",
    "TargetStatus",
    "UriError",
    "View",
];

/// The associated items reached through them: parsers and value constructors, and enum
/// variants (`*`). Nothing that builds a `Completeness` or an `Envelope`.
const ALLOWED_PATHS: &[&str] = &[
    "DeclKind::*",
    "FileUri::parse",
    "ObjectId::parse",
    "Position::new",
    "PositionEncoding::from_lsp_name",
    "Range::new",
    "Reason::*",
    "TargetStatus::*",
];

/// The source reading and the allowlist check.
#[cfg(test)]
mod support {
    use super::*;

    /// The code of a source: lines before the first `#[cfg(test)]`, comment lines dropped.
    pub(super) fn code(text: &str) -> String {
        text.lines()
            .take_while(|line| !line.starts_with("#[cfg(test)]"))
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Every `a::b::c` path in `code`.
    pub(super) fn paths(code: &str) -> Vec<Vec<String>> {
        let mut found = Vec::new();
        let mut token = String::new();
        for ch in code.chars().chain(std::iter::once(' ')) {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == ':' {
                token.push(ch);
                continue;
            }
            if token.contains("::") {
                let segments: Vec<String> = token
                    .split("::")
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                found.push(segments);
            }
            token.clear();
        }
        found
    }

    /// The names `use jfl_domain::…;` brings in.
    pub(super) fn imports(code: &str) -> Vec<String> {
        code.match_indices("use jfl_domain::")
            .flat_map(|(at, prefix)| {
                let rest = &code[at.saturating_add(prefix.len())..];
                let list = &rest[..rest.find(';').unwrap_or(rest.len())];
                list.trim_matches(|c: char| c == '{' || c == '}' || c.is_whitespace())
                    .split(',')
                    .map(|name| name.trim().to_owned())
                    .filter(|name| !name.is_empty())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub(super) fn allowed_path(path: &[String]) -> bool {
        let [item, member, ..] = path else {
            return true;
        };
        ALLOWED_PATHS.contains(&format!("{item}::{member}").as_str())
            || ALLOWED_PATHS.contains(&format!("{item}::*").as_str())
    }

    /// The domain part of `path`: after `jfl_domain::`, or the whole path when it starts with
    /// an imported name; `None` when it is not a domain path.
    pub(super) fn domain_part<'a>(path: &'a [String], imported: &[String]) -> Option<&'a [String]> {
        match path.first().map(String::as_str) {
            Some("jfl_domain") => path.get(1..).filter(|rest| !rest.is_empty()),
            Some(first) if imported.iter().any(|name| name == first) => Some(path),
            _ => None,
        }
    }
}

use support::*;

#[test]
fn should_name_only_allowed_domain_items_when_the_wire_form_is_built() {
    // given
    let sources: Vec<(&str, String)> = SOURCES.iter().map(|(f, t)| (*f, code(t))).collect();

    // when
    let mut refused = Vec::new();
    for (file, code) in &sources {
        let imported = imports(code);
        for name in &imported {
            if !ALLOWED_ITEMS.contains(&name.as_str()) {
                refused.push(format!("{file}: imports jfl_domain::{name}"));
            }
        }
        for path in paths(code) {
            let Some(domain_path) = domain_part(&path, &imported) else {
                continue;
            };
            let item = domain_path.first().map(String::as_str).unwrap_or("");
            if !ALLOWED_ITEMS.contains(&item) || !allowed_path(domain_path) {
                refused.push(format!(
                    "{file}: names jfl_domain::{}",
                    domain_path.join("::")
                ));
            }
        }
    }

    // then
    assert!(
        refused.is_empty(),
        "jfl_domain names outside the allowlist: {refused:#?}"
    );
}

#[test]
fn should_refuse_a_proof_builder_when_the_allowlist_reads_it() {
    // given
    let code = "use jfl_domain::{completeness, IndexState};\nlet c = Completeness::because(r);\nlet e = jfl_domain::Envelope::new(p);";

    // when
    let imported = imports(code);
    let named: Vec<bool> = paths(code)
        .iter()
        .filter_map(|path| domain_part(path, &[String::from("Completeness")]))
        .filter(|path| path.len() > 1)
        .map(allowed_path)
        .collect();

    // then
    assert_eq!(imported, ["completeness", "IndexState"]);
    assert!(
        imported
            .iter()
            .all(|n| !ALLOWED_ITEMS.contains(&n.as_str()))
    );
    assert_eq!(named, [false, false]);
}

#[test]
fn should_scan_every_module_when_the_crate_gains_one() {
    // given
    let modules = LIB
        .lines()
        .filter_map(|line| line.strip_prefix("pub mod "))
        .filter_map(|rest| rest.strip_suffix(';'));

    // when
    let unscanned: Vec<String> = modules
        .map(|name| format!("{name}.rs"))
        .filter(|file| !SOURCES.iter().any(|(scanned, _)| scanned == file))
        .collect();

    // then
    assert!(unscanned.is_empty(), "add to SOURCES: {unscanned:?}");
}
