//! `ARCH-clippy-lists` (fnd §2.4): every crate's `clippy.toml` equals the root list minus exactly
//! its grants, plus the `#pure ` entries for a pure crate. Computed here from the root file's
//! `# grant: <name>` sections and `architecture.toml`'s `[grants]`, independently of
//! `scripts/clippy-lists.sh`, so a generator bug cannot agree with itself.
//!
//! `allow-invalid` is part of an entry: the root may set it only on a path outside `std`, `core`
//! and `alloc` (Tokio, crossbeam: crates absent from most graphs), and a pure crate's list sets it
//! on every `std` path (a `no_std` crate cannot resolve them). Anywhere else it would hide a typo.

use std::collections::{BTreeMap, BTreeSet};

use toml::{Table, Value};

use crate::Finding;
use crate::policy::{GRANTS, Policy};
use crate::tree::Tree;

const ROOT: &str = "clippy.toml";
const ARRAYS: [&str; 3] = [
    "disallowed-methods",
    "disallowed-types",
    "disallowed-macros",
];

/// One list entry: array, path, reason, `allow-invalid`.
type Entry = (String, String, String, bool);

/// A canonical entry with the section it sits in (`none` for no grant) and whether it is a
/// `#pure ` line.
#[derive(Debug)]
struct Canonical {
    entry: Entry,
    grant: String,
    pure: bool,
}

/// The rule over the root list and each member's (`members`: name to directory).
pub fn check(tree: &dyn Tree, policy: &Policy, members: &BTreeMap<String, String>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Some(text) = tree.read(ROOT) else {
        return vec![Finding::new(
            "ARCH-clippy-lists",
            ROOT,
            "the canonical clippy.toml is missing",
        )];
    };
    let Ok(root_table) = text.parse::<Table>() else {
        return vec![Finding::new(
            "ARCH-clippy-lists",
            ROOT,
            "the canonical clippy.toml does not parse",
        )];
    };
    let canonical = canonical(&text, &mut findings);
    stray_configs(tree, members, &mut findings);
    // The line model must see every entry the TOML holds: a string-form entry or one on a line
    // of its own shape would otherwise escape the comparison below.
    let modelled: BTreeSet<(String, String)> = canonical
        .iter()
        .filter(|c| !c.pure)
        .map(|c| (c.entry.0.clone(), c.entry.1.clone()))
        .collect();
    for array in ARRAYS {
        for item in root_table
            .get(array)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let path = item
                .as_str()
                .or_else(|| item.get("path").and_then(Value::as_str))
                .unwrap_or_default();
            if !modelled.contains(&(array.to_owned(), path.to_owned())) {
                findings.push(Finding::new(
                    "ARCH-clippy-lists",
                    ROOT,
                    format!("{array} entry `{path}` is not a one-line `{{ path = ..., reason = ... }}` under a `# grant:` section"),
                ));
            }
        }
    }
    let root_settings = settings(&root_table);
    for (name, dir) in members {
        let grants = policy.grants_of(name);
        let pure = policy.has_role(name, "pure");
        let path = if dir.is_empty() {
            ROOT.to_owned()
        } else {
            format!("{dir}/{ROOT}")
        };
        let expected: BTreeSet<Entry> = canonical
            .iter()
            .filter(|c| !grants.contains(c.grant.as_str()) && (pure || !c.pure))
            .map(|c| {
                let (array, entry_path, reason, invalid) = c.entry.clone();
                let invalid = invalid || (pure && entry_path.starts_with("std::"));
                (array, entry_path, reason, invalid)
            })
            .collect();
        let Some(own) = (!dir.is_empty()).then(|| tree.read(&path)).flatten() else {
            if !grants.is_empty() || pure {
                findings.push(Finding::new(
                    "ARCH-clippy-lists",
                    path.as_str(),
                    format!("`{name}` holds {} but has no clippy.toml of its own: run `just clippy-lists`", describe(&grants, pure)),
                ));
            }
            continue;
        };
        let Ok(table) = own.parse::<Table>() else {
            findings.push(Finding::new(
                "ARCH-clippy-lists",
                path.as_str(),
                "the list does not parse",
            ));
            continue;
        };
        if settings(&table) != root_settings {
            findings.push(Finding::new(
                "ARCH-clippy-lists",
                path.as_str(),
                "its settings outside the lists differ from the root clippy.toml's",
            ));
        }
        let actual = entries(&table);
        for (array, entry_path, _, invalid) in expected.difference(&actual) {
            let differs_only_in_flag = actual
                .iter()
                .any(|(a, p, _, _)| a == array && p == entry_path);
            let message = if differs_only_in_flag {
                format!(
                    "{array} `{entry_path}` differs from the canonical entry (allow-invalid must be {invalid}, reason as in the root)"
                )
            } else {
                format!(
                    "drops {array} `{entry_path}`, which its grants ({}) do not cover",
                    describe(&grants, pure)
                )
            };
            findings.push(Finding::new("ARCH-clippy-lists", path.as_str(), message));
        }
        for (array, entry_path, _, _) in actual.difference(&expected) {
            if !expected
                .iter()
                .any(|(a, p, _, _)| a == array && p == entry_path)
            {
                findings.push(Finding::new(
                    "ARCH-clippy-lists",
                    path.as_str(),
                    format!("has {array} `{entry_path}`, which the canonical list minus its grants does not"),
                ));
            }
        }
    }
    findings
}

/// Clippy reads the nearest `clippy.toml` or `.clippy.toml` walking up from a crate, a dotted one
/// first: any such file other than the root list and the members' own lists could replace a
/// crate's list unseen (a parent directory's, or a root `.clippy.toml`). The fixture workspaces
/// and the archived spikes are not this workspace's crates.
fn stray_configs(tree: &dyn Tree, members: &BTreeMap<String, String>, findings: &mut Vec<Finding>) {
    let own: BTreeSet<String> = members
        .values()
        .filter(|dir| !dir.is_empty())
        .map(|dir| format!("{dir}/{ROOT}"))
        .collect();
    for file in tree.files_under("") {
        let name = file.rsplit('/').next().unwrap_or_default();
        let outside = ["spikes/", "tools/jfl-architecture/fixtures/"]
            .iter()
            .any(|p| file.starts_with(p));
        if (name == ROOT || name == ".clippy.toml")
            && file != ROOT
            && !own.contains(&file)
            && !outside
        {
            findings.push(Finding::new(
                "ARCH-clippy-lists",
                file.as_str(),
                "a clippy configuration outside the root and the members' own lists would replace a crate's list; remove it",
            ));
        }
    }
}

fn describe(grants: &BTreeSet<&str>, pure: bool) -> String {
    let mut parts: Vec<String> = grants.iter().map(|g| format!("grant {g}")).collect();
    if pure {
        parts.push("the pure role".to_owned());
    }
    if parts.is_empty() {
        "no grant".to_owned()
    } else {
        parts.join(", ")
    }
}

/// The keys outside the three lists (`allow-unwrap-in-tests` and the like).
fn settings(table: &Table) -> Table {
    table
        .iter()
        .filter(|(key, _)| !ARRAYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn entries(table: &Table) -> BTreeSet<Entry> {
    let mut found = BTreeSet::new();
    for array in ARRAYS {
        for item in table
            .get(array)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(entry) = entry(array, item) {
                found.insert(entry);
            }
        }
    }
    found
}

fn entry(array: &str, item: &Value) -> Option<Entry> {
    let path = item.get("path")?.as_str()?.to_owned();
    let reason = item
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let invalid = item
        .get("allow-invalid")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some((array.to_owned(), path, reason, invalid))
}

/// The root file, line by line: the sections are comments, which a TOML parser drops.
fn canonical(text: &str, findings: &mut Vec<Finding>) -> Vec<Canonical> {
    let mut found = Vec::new();
    let mut array: Option<&str> = None;
    let mut grant: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        let at = format!("{ROOT}:{}", index.saturating_add(1));
        let trimmed = line.trim();
        if let Some(name) = ARRAYS
            .iter()
            .find(|a| trimmed.starts_with(&format!("{a} = [")))
        {
            array = Some(name);
            grant = None;
            continue;
        }
        if trimmed == "]" {
            array = None;
            continue;
        }
        let Some(array) = array else { continue };
        if let Some(name) = trimmed.strip_prefix("# grant: ") {
            if name != "none" && !GRANTS.contains(&name) {
                findings.push(Finding::new(
                    "ARCH-clippy-lists",
                    at,
                    format!("unknown grant section `{name}`; the grants are {GRANTS:?} and none"),
                ));
            }
            grant = Some(name.to_owned());
            continue;
        }
        let (pure, body) = match trimmed.strip_prefix("#pure ") {
            Some(body) => (true, body),
            None => (false, trimmed),
        };
        if !body.starts_with('{') {
            continue;
        }
        let value = format!("x = {}", body.trim_end_matches(','))
            .parse::<Table>()
            .ok()
            .and_then(|t| t.get("x").cloned());
        let Some(entry) = value.as_ref().and_then(|v| entry(array, v)) else {
            findings.push(Finding::new(
                "ARCH-clippy-lists",
                at,
                "an entry that does not parse as `{ path = ..., reason = ... }`",
            ));
            continue;
        };
        let Some(section) = grant.clone() else {
            findings.push(Finding::new(
                "ARCH-clippy-lists",
                at,
                format!("`{}` sits before any `# grant:` section", entry.1),
            ));
            continue;
        };
        let std_path = ["std::", "core::", "alloc::"]
            .iter()
            .any(|p| entry.1.starts_with(p));
        if entry.3 && std_path {
            findings.push(Finding::new(
                "ARCH-clippy-lists",
                at,
                format!(
                    "`{}` carries allow-invalid, which would hide a typo in a std path",
                    entry.1
                ),
            ));
        }
        found.push(Canonical {
            entry,
            grant: section,
            pure,
        });
    }
    found
}
