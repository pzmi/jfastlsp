//! The manifests: `[workspace.lints]` exactly as fnd §4.2 (plus the authority lists' three
//! lints), every member with `[lints] workspace = true` and nothing else, `publish = false`
//! (`ARCH-lint-config`); an explicit `members` list and no crate directory left out of it
//! (`ARCH-not-a-member`, fnd §2.2 rule 1).

use std::collections::BTreeMap;

use toml::{Table, Value};

use crate::Finding;
use crate::tree::Tree;

/// fnd §4.2's `[workspace.lints]`, plus `disallowed_*` denied for the authority lists (fnd §2.4).
/// A change here is a change of fnd §4.2, made in the same commit.
pub const WORKSPACE_LINTS: &str = r#"
[rust]
unsafe_code = "forbid"
unused_must_use = "deny"
missing_docs = "warn"
unreachable_pub = "warn"
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(coverage)"] }

[clippy]
all = { level = "deny", priority = -1 }
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
indexing_slicing = "deny"
arithmetic_side_effects = "deny"
await_holding_lock = "deny"
large_futures = "deny"
disallowed_macros = "deny"
disallowed_methods = "deny"
disallowed_types = "deny"
"#;

/// The directories whose immediate subdirectories hold crates.
const CRATE_PARENTS: [&str; 2] = ["crates", "tools"];

/// Every manifest rule; `members` maps each workspace member to its directory.
pub fn check(tree: &dyn Tree, members: &BTreeMap<String, String>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Some(root) = parse(tree, "Cargo.toml", &mut findings) else {
        return findings;
    };
    workspace_lints(&root, &mut findings);
    members_list(tree, &root, &mut findings);
    for (name, dir) in members {
        let path = if dir.is_empty() {
            "Cargo.toml".to_owned()
        } else {
            format!("{dir}/Cargo.toml")
        };
        let Some(manifest) = parse(tree, &path, &mut findings) else {
            continue;
        };
        let lints = manifest.get("lints");
        let expected = Value::Table(Table::from_iter([(
            "workspace".to_owned(),
            Value::Boolean(true),
        )]));
        if lints != Some(&expected) {
            findings.push(Finding::new(
                "ARCH-lint-config",
                path.as_str(),
                format!("`{name}` must carry exactly `[lints] workspace = true` (fnd §3.5: workspace lints only); found {}", show(lints)),
            ));
        }
        let publish = manifest.get("package").and_then(|p| p.get("publish"));
        if publish != Some(&Value::Boolean(false)) {
            findings.push(Finding::new(
                "ARCH-lint-config",
                path.as_str(),
                format!(
                    "`{name}` must set `publish = false` (fnd §4.1); found {}",
                    show(publish)
                ),
            ));
        }
    }
    findings
}

fn parse(tree: &dyn Tree, path: &str, findings: &mut Vec<Finding>) -> Option<Table> {
    let Some(text) = tree.read(path) else {
        findings.push(Finding::new(
            "ARCH-metadata",
            path,
            "the manifest cannot be read",
        ));
        return None;
    };
    match text.parse::<Table>() {
        Ok(table) => Some(table),
        Err(error) => {
            findings.push(Finding::new(
                "ARCH-metadata",
                path,
                format!("the manifest does not parse: {}", error.to_string().trim()),
            ));
            None
        }
    }
}

fn show(value: Option<&Value>) -> String {
    value.map_or_else(
        || "nothing".to_owned(),
        |v| format!("`{v}`").replace('\n', " "),
    )
}

fn workspace_lints(root: &Table, findings: &mut Vec<Finding>) {
    let Ok(expected) = WORKSPACE_LINTS.parse::<Table>() else {
        findings.push(Finding::new(
            "ARCH-lint-config",
            "Cargo.toml",
            "the expected lint table does not parse",
        ));
        return;
    };
    let actual = root
        .get("workspace")
        .and_then(|w| w.get("lints"))
        .and_then(Value::as_table)
        .cloned()
        .unwrap_or_default();
    for tool in ["rust", "clippy"] {
        let want = expected
            .get(tool)
            .and_then(Value::as_table)
            .cloned()
            .unwrap_or_default();
        let have = actual
            .get(tool)
            .and_then(Value::as_table)
            .cloned()
            .unwrap_or_default();
        for (lint, level) in &want {
            if have.get(lint) != Some(level) {
                findings.push(Finding::new(
                    "ARCH-lint-config",
                    "Cargo.toml",
                    format!(
                        "[workspace.lints.{tool}] {lint} must be `{level}` (fnd §4.2); found {}",
                        show(have.get(lint))
                    ),
                ));
            }
        }
        for lint in have.keys().filter(|lint| !want.contains_key(*lint)) {
            findings.push(Finding::new(
                "ARCH-lint-config",
                "Cargo.toml",
                format!("[workspace.lints.{tool}] {lint} is not in fnd §4.2: amend fnd §4.2 and this check together"),
            ));
        }
    }
    for tool in actual.keys().filter(|tool| !expected.contains_key(*tool)) {
        findings.push(Finding::new(
            "ARCH-lint-config",
            "Cargo.toml",
            format!("[workspace.lints.{tool}] is not in fnd §4.2"),
        ));
    }
}

fn members_list(tree: &dyn Tree, root: &Table, findings: &mut Vec<Finding>) {
    let listed: Vec<String> = root
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(Value::as_str)
                .map(|m| m.trim_end_matches('/').to_owned())
                .collect()
        })
        .unwrap_or_default();
    for member in listed.iter().filter(|m| m.contains(['*', '?', '['])) {
        findings.push(Finding::new(
            "ARCH-not-a-member",
            "Cargo.toml",
            format!("workspace.members entry `{member}` is a glob; members are an explicit list (fnd §2.2 rule 1)"),
        ));
    }
    for parent in CRATE_PARENTS {
        for file in tree.files_under(parent) {
            let Some(dir) = file.strip_suffix("/Cargo.toml") else {
                continue;
            };
            // Only `<parent>/<crate>/Cargo.toml`: a fixture workspace deeper down is not a crate
            // of this workspace.
            if dir.matches('/').count() != 1 || listed.iter().any(|m| m == dir) {
                continue;
            }
            findings.push(Finding::new(
                "ARCH-not-a-member",
                file.as_str(),
                format!("`{dir}` holds a crate that is not in workspace.members, so no check sees it; add it with its architecture.toml rows (fnd §2.2 rule 1)"),
            ));
        }
    }
}
