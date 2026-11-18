//! `ARCH-fixtures-lock` (fnd §3.3, ver §4, IDX-07): every golden fixture's SHA-256 is in
//! `fixtures.lock`, so a golden cannot be rewritten to get green without a visible change of the
//! lock in the same commit, which review reads as a change of the promise.
//!
//! The locked set is an allowlist of places ([`LOCKED_DIRS`] and every crate's
//! `tests/fixtures/`): every file under them is listed, with its digest, sorted by path, each
//! once, and nothing else is. A finding names the line to write. `just fixtures-lock` rewrites the
//! file from the tree for a deliberate change.
//!
//! Pinned by: tests/fixtures_lock.rs (the repository clean, an edited golden with a stale lock,
//! a new fixture unlisted, a listed fixture gone, the order).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::Finding;
use crate::tree::Tree;

/// The rule id.
pub const RULE: &str = "ARCH-fixtures-lock";
/// The lock file.
pub const LOCK: &str = "fixtures.lock";
/// Directories whose every file is a golden fixture (besides each crate's `tests/fixtures/`).
pub const LOCKED_DIRS: [&str; 1] = ["fixtures"];

/// Every locked fixture of `tree`, sorted.
pub fn locked_files(tree: &dyn Tree) -> Vec<String> {
    let mut files: Vec<String> = LOCKED_DIRS
        .iter()
        .flat_map(|dir| tree.files_under(dir))
        .collect();
    files.extend(tree.files_under("crates").into_iter().filter(|path| {
        let mut parts = path.split('/');
        parts.next() == Some("crates")
            && parts.next().is_some()
            && parts.next() == Some("tests")
            && parts.next() == Some("fixtures")
    }));
    files.sort();
    files.dedup();
    files
}

fn digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The lock's text for `tree`: what `just fixtures-lock` writes.
pub fn expected(tree: &dyn Tree) -> Result<String, Finding> {
    let mut out = String::from(
        "# The golden fixtures' SHA-256 (ver §4, ARCH-fixtures-lock in tools/jfl-architecture/src/fixtures_lock.rs):\n\
         # every file under fixtures/ and under each crate's tests/fixtures/, sorted. A golden is\n\
         # never rewritten to get green: a deliberate change moves its line here in the same commit\n\
         # (just fixtures-lock), and review reads the line as a change of the promise.\n",
    );
    for path in locked_files(tree) {
        let bytes = tree
            .read_bytes(&path)
            .ok_or_else(|| Finding::new(RULE, path.as_str(), "a fixture cannot be read"))?;
        let _ = writeln!(out, "{}  {path}", digest(&bytes));
    }
    Ok(out)
}

/// The rule over `tree`.
pub fn check(tree: &dyn Tree) -> Vec<Finding> {
    let Some(lock) = tree.read(LOCK) else {
        return vec![Finding::new(
            RULE,
            LOCK,
            "the lock is missing: run `just fixtures-lock` and review it",
        )];
    };
    let mut findings = Vec::new();
    let mut listed: BTreeMap<&str, &str> = BTreeMap::new();
    let mut previous = "";
    for (number, line) in lock.lines().enumerate() {
        let at = format!("{LOCK}:{}", number.saturating_add(1));
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some((hash, path)) = line.split_once("  ") else {
            findings.push(Finding::new(RULE, at, "not `<sha256>  <path>`"));
            continue;
        };
        if hash.len() != 64 || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            findings.push(Finding::new(RULE, at, format!("`{hash}` is not a SHA-256")));
            continue;
        }
        if path <= previous {
            findings.push(Finding::new(
                RULE,
                at,
                format!("`{path}` is out of order or listed twice: the lock is sorted by path"),
            ));
        }
        previous = path;
        listed.insert(path, hash);
    }
    let files = locked_files(tree);
    for path in &files {
        let Some(bytes) = tree.read_bytes(path) else {
            findings.push(Finding::new(
                RULE,
                path.as_str(),
                "a fixture cannot be read",
            ));
            continue;
        };
        let actual = digest(&bytes);
        match listed.get(path.as_str()) {
            None => findings.push(Finding::new(
                RULE,
                LOCK,
                format!(
                    "`{path}` is not locked: if it is a reviewed golden, add `{actual}  {path}`"
                ),
            )),
            Some(&hash) if hash != actual => findings.push(Finding::new(
                RULE,
                LOCK,
                format!(
                    "`{path}` changed: a golden is never rewritten to get green; if the change is \
                     deliberate, write `{actual}  {path}`"
                ),
            )),
            Some(_) => {}
        }
    }
    for path in listed.keys() {
        if files.binary_search_by(|f| f.as_str().cmp(path)).is_err() {
            findings.push(Finding::new(
                RULE,
                LOCK,
                format!("`{path}` is locked but is not a fixture: remove its line"),
            ));
        }
    }
    findings
}
