//! The manifests: `[workspace.lints]` exactly as fnd §4.2 (plus the authority lists' three
//! lints), every member with `[lints] workspace = true` and nothing else, `publish = false`
//! (`ARCH-lint-config`); an explicit `members` list and no crate directory left out of it
//! (`ARCH-not-a-member`, fnd §2.2 rule 1).
//!
//! The manifests' shape is an allowlist (FND-08, fnd §3): the root holds only [`ROOT_KEYS`],
//! `[workspace]` only [`WORKSPACE_KEYS`], `exclude` exactly [`EXCLUDE`] and profiles only
//! [`PROFILES`] (`ARCH-not-a-member`); a member only [`MEMBER_KEYS`]; `[patch]` and `[replace]`
//! anywhere are `ARCH-unlisted-external`, since they swap a dependency's source.
//!
//! Pinned by: tests/rules.rs::should_report_each_rule_with_its_id_and_path_when_its_negative_case_runs.

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

/// The root manifest's top-level tables.
pub const ROOT_KEYS: [&str; 2] = ["workspace", "profile"];
/// The keys of the root's `[workspace]`.
pub const WORKSPACE_KEYS: [&str; 6] = [
    "members",
    "exclude",
    "resolver",
    "package",
    "lints",
    "dependencies",
];
/// `workspace.exclude`, exactly: the archived spike, outside the workspace on purpose.
pub const EXCLUDE: [&str; 1] = ["spikes"];
/// The profiles the root may define: none of the gate's own (`dev`, `test`), so a profile can
/// never weaken what the gate builds (`bench-tool` is BENCH-02's).
pub const PROFILES: [&str; 1] = ["bench-tool"];
/// A member manifest's top-level tables.
pub const MEMBER_KEYS: [&str; 11] = [
    "package",
    "lints",
    "dependencies",
    "dev-dependencies",
    "build-dependencies",
    "features",
    "lib",
    "bin",
    "test",
    "bench",
    "target",
];

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
    root_shape(&root, &mut findings);
    for (name, dir) in members {
        let path = if dir.is_empty() {
            "Cargo.toml".to_owned()
        } else {
            format!("{dir}/Cargo.toml")
        };
        let Some(manifest) = parse(tree, &path, &mut findings) else {
            continue;
        };
        for key in manifest.keys() {
            let (rule, why) = match key.as_str() {
                "patch" | "replace" => (
                    "ARCH-unlisted-external",
                    "swaps a dependency's source (fnd §2.5)",
                ),
                _ if MEMBER_KEYS.contains(&key.as_str()) => continue,
                _ => (
                    "ARCH-not-a-member",
                    "is not a table manifests.rs's MEMBER_KEYS allows",
                ),
            };
            findings.push(Finding::new(
                rule,
                path.as_str(),
                format!("`[{key}]` in `{name}`'s manifest {why}"),
            ));
        }
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

/// The root manifest's allowlisted shape: its tables, `[workspace]`'s keys, `exclude` and the
/// profiles.
fn root_shape(root: &Table, findings: &mut Vec<Finding>) {
    let mut push = |rule: &'static str, message: String| {
        findings.push(Finding::new(rule, "Cargo.toml", message));
    };
    for key in root.keys() {
        match key.as_str() {
            "patch" | "replace" => push(
                "ARCH-unlisted-external",
                format!(
                    "`[{key}]` swaps a dependency's source: every package is a member or comes from crates.io (fnd §2.5)"
                ),
            ),
            _ if ROOT_KEYS.contains(&key.as_str()) => {}
            _ => push(
                "ARCH-not-a-member",
                format!(
                    "the root manifest holds only {ROOT_KEYS:?}, not `[{key}]` (manifests.rs ROOT_KEYS)"
                ),
            ),
        }
    }
    let workspace = root.get("workspace").and_then(Value::as_table);
    for key in workspace.into_iter().flat_map(Table::keys) {
        if !WORKSPACE_KEYS.contains(&key.as_str()) {
            push(
                "ARCH-not-a-member",
                format!("`workspace.{key}` is not a key manifests.rs's WORKSPACE_KEYS allows"),
            );
        }
    }
    if let Some(exclude) = workspace.and_then(|w| w.get("exclude")) {
        let listed: Option<Vec<&str>> = exclude
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect());
        if listed.as_deref() != Some(EXCLUDE.as_slice()) {
            push(
                "ARCH-not-a-member",
                format!(
                    "`workspace.exclude` must be exactly {EXCLUDE:?}: an excluded directory is a crate no check sees; found {}",
                    show(Some(exclude))
                ),
            );
        }
    }
    let profiles = root.get("profile").and_then(Value::as_table);
    for name in profiles.into_iter().flat_map(Table::keys) {
        if !PROFILES.contains(&name.as_str()) {
            push(
                "ARCH-not-a-member",
                format!(
                    "`[profile.{name}]` is not in manifests.rs's PROFILES: a profile could change what the gate builds"
                ),
            );
        }
    }
}
