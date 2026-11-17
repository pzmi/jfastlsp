//! `ARCH-gate-config` (fnd §3.6, FND-08): the gate's own configuration is pinned, so a commit
//! cannot neuter the gate it is judged by without changing a pinned expectation in plain sight.
//! Every check is an allowlist or a pinned shape, never a list of bad patterns (fnd §3):
//!
//! - **the pin manifest** [`PINS`]: one SHA-256 per justfile recipe of the gate's closure (the
//!   roots [`ROOTS`], every stage of `check_stages` and their header dependencies), one for the
//!   justfile's top-level lines (settings, exports, variables, aliases), and one per file of
//!   [`GATE_FILES`]; an entry missing, extra or different fails, naming the line to write. A
//!   `scripts/...` path named in a pinned recipe must be one of [`GATE_FILES`];
//! - **`.config/nextest.toml`**: only the keys of [`nextest_shape`] (no `default-filter`, no
//!   `[scripts]`, `retries` 0, overrides that set a test group and nothing else);
//! - **`mise.toml`**: only `[tools]` with string versions (no `[env]`, `[tasks]`, `[hooks]` or
//!   settings), and no other mise configuration file anywhere in the tree;
//! - **`rust-toolchain.toml`**: only `[toolchain]` with `channel`, `profile`, `components` and
//!   `targets` (no `path`), and no other toolchain file in the tree;
//! - **no cargo configuration file** (`.cargo/config`, `.cargo/config.toml`) anywhere in the tree:
//!   the allowlist of committed cargo configuration is empty (the `_preflight` recipe refuses one
//!   before any cargo runs; this is the second line).
//!
//! Comment lines at column 0 of the justfile are documentation and are not hashed; a comment
//! inside a recipe body is.
//!
//! Pinned by: tests/gate_config.rs (the repository clean, and one negative case per item above).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use toml::{Table, Value};

use crate::Finding;
use crate::ci;
use crate::tree::Tree;

/// The rule id.
pub const RULE: &str = "ARCH-gate-config";
/// The pin manifest.
pub const PINS: &str = ".config/gate.sha256";
/// The test runner's configuration.
pub const NEXTEST: &str = ".config/nextest.toml";
/// mise's configuration, the only one allowed.
pub const MISE: &str = "mise.toml";
/// rustup's toolchain file.
pub const TOOLCHAIN: &str = "rust-toolchain.toml";
/// The justfile recipes the gate starts from, besides every stage of `check_stages`.
pub const ROOTS: [&str; 5] = [
    "quick",
    "check",
    "check-only",
    "ci-parity",
    "platform-tests",
];
/// The files the gate's recipes run, pinned whole.
pub const GATE_FILES: [&str; 8] = [
    "scripts/adr-index.sh",
    "scripts/bootstrap-tools.sh",
    "scripts/clippy-lists.sh",
    "scripts/gitleaks-config.sh",
    "scripts/private-names-check.sh",
    "scripts/private-names-fold.pl",
    "scripts/secrets-check.sh",
    "scripts/workflows-check.sh",
];
/// The manifest's name for the justfile's top-level lines.
pub const TOP_LEVEL: &str = "justfile::(top-level)";

/// Runs every check of the rule over `tree`.
pub fn check(tree: &dyn Tree) -> Vec<Finding> {
    let mut findings = Vec::new();
    let files = tree.files_under("");
    pins(tree, &mut findings);
    match tree.read(NEXTEST) {
        Some(text) => findings.extend(nextest_shape(&text)),
        None => findings.push(Finding::new(RULE, NEXTEST, "the file is missing")),
    }
    match tree.read(MISE) {
        Some(text) => findings.extend(mise_shape(&text)),
        None => findings.push(Finding::new(RULE, MISE, "the file is missing")),
    }
    match tree.read(TOOLCHAIN) {
        Some(text) => findings.extend(toolchain_shape(&text)),
        None => findings.push(Finding::new(RULE, TOOLCHAIN, "the file is missing")),
    }
    for path in &files {
        if let Some(why) = stray(path) {
            findings.push(Finding::new(RULE, path.as_str(), why));
        }
    }
    findings
}

/// Why a file may not exist in the tree, if it may not.
fn stray(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let parent = path
        .strip_suffix(name)
        .unwrap_or_default()
        .trim_end_matches('/');
    let in_dir = |dir: &str| parent == dir || parent.ends_with(&format!("/{dir}"));
    if in_dir(".cargo") && matches!(name, "config" | "config.toml") {
        return Some(
            "a cargo configuration file: none may be committed (fnd §3.6; it can set a runner, an alias or flags before any check runs)",
        );
    }
    let mise_named = (name.starts_with("mise.") || name.starts_with(".mise."))
        && name.ends_with(".toml")
        || matches!(name, ".tool-versions" | ".rtx.toml");
    // mise also reads `mise/config.toml`, `.mise/config.toml` and `.config/mise/conf.d/*.toml`.
    let mise_dir = name.ends_with(".toml")
        && parent
            .split('/')
            .any(|part| matches!(part, "mise" | ".mise"));
    if (mise_named || mise_dir) && path != MISE && path != "mise.lock" {
        return Some(
            "a mise configuration file: only the root mise.toml (shape-checked) may exist (fnd §3.6; another could add [env])",
        );
    }
    if matches!(name, "rust-toolchain" | "rust-toolchain.toml") && path != TOOLCHAIN {
        return Some(
            "a toolchain file: only the root rust-toolchain.toml (shape-checked) may exist (fnd §3.6)",
        );
    }
    None
}

/// The pin manifest's expected lines, computed from the tree: `(entry, sha256 hex)`.
pub fn expected_pins(tree: &dyn Tree) -> Result<BTreeMap<String, String>, Finding> {
    let justfile = tree
        .read(ci::JUSTFILE)
        .ok_or_else(|| Finding::new(RULE, ci::JUSTFILE, "the file is missing"))?;
    let stages = ci::check_stages(&justfile).ok_or_else(|| {
        Finding::new(
            RULE,
            ci::JUSTFILE,
            "no `check_stages := \"...\"` line: the gate's recipes are unknown",
        )
    })?;
    let parsed = Justfile::parse(&justfile);
    let mut pins = BTreeMap::new();
    pins.insert(TOP_LEVEL.to_owned(), sha256(&parsed.top_level));
    let mut queue: Vec<String> = ROOTS
        .iter()
        .map(|r| (*r).to_owned())
        .chain(stages)
        .collect();
    let mut seen = BTreeSet::new();
    while let Some(name) = queue.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(recipe) = parsed.recipes.get(&name) else {
            return Err(Finding::new(
                RULE,
                ci::JUSTFILE,
                format!("the gate names the recipe `{name}`, which the justfile does not have"),
            ));
        };
        pins.insert(format!("justfile::{name}"), sha256(&recipe.text));
        queue.extend(
            recipe
                .dependencies
                .iter()
                .filter(|d| parsed.recipes.contains_key(*d))
                .cloned(),
        );
    }
    for file in GATE_FILES {
        let text = tree
            .read(file)
            .ok_or_else(|| Finding::new(RULE, file, "a file of the gate is missing"))?;
        pins.insert(file.to_owned(), sha256(&text));
    }
    Ok(pins)
}

fn pins(tree: &dyn Tree, findings: &mut Vec<Finding>) {
    let expected = match expected_pins(tree) {
        Ok(expected) => expected,
        Err(finding) => {
            findings.push(finding);
            return;
        }
    };
    let justfile = tree.read(ci::JUSTFILE).unwrap_or_default();
    let parsed = Justfile::parse(&justfile);
    for (name, recipe) in &parsed.recipes {
        if !expected.contains_key(&format!("justfile::{name}")) {
            continue;
        }
        for word in recipe
            .text
            .split(|c: char| c.is_whitespace() || "\"'(){}".contains(c))
        {
            if word.starts_with("scripts/") && !word.contains('*') {
                let file = word.trim_end_matches([';', ',', ')']);
                if !GATE_FILES.contains(&file) {
                    findings.push(Finding::new(
                        RULE,
                        ci::JUSTFILE,
                        format!("the gate recipe `{name}` runs `{file}`, which gate.rs's GATE_FILES does not pin"),
                    ));
                }
            }
        }
    }
    let Some(text) = tree.read(PINS) else {
        findings.push(Finding::new(
            RULE,
            PINS,
            "the pin manifest is missing; it lists one `<sha256>  <entry>` line per pinned recipe and file",
        ));
        return;
    };
    let mut listed: BTreeMap<String, String> = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let at = format!("{PINS}:{}", index.saturating_add(1));
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((hash, entry)) = line.split_once("  ") else {
            findings.push(Finding::new(
                RULE,
                at,
                "a line that is not `<sha256>  <entry>`",
            ));
            continue;
        };
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            findings.push(Finding::new(RULE, at, "a hash that is not 64 hex digits"));
            continue;
        }
        if listed
            .insert(entry.to_owned(), hash.to_ascii_lowercase())
            .is_some()
        {
            findings.push(Finding::new(RULE, at, format!("`{entry}` is listed twice")));
        }
    }
    for (entry, hash) in &expected {
        match listed.remove(entry) {
            Some(pinned) if pinned == *hash => {}
            Some(_) => findings.push(Finding::new(
                RULE,
                PINS,
                format!("`{entry}` changed: the gate's own configuration moved. If that is the change under review, pin it with the line `{hash}  {entry}`"),
            )),
            None => findings.push(Finding::new(
                RULE,
                PINS,
                format!("`{entry}` is part of the gate but not pinned: add `{hash}  {entry}`"),
            )),
        }
    }
    for entry in listed.keys() {
        findings.push(Finding::new(
            RULE,
            PINS,
            format!("`{entry}` is pinned but is no part of the gate: remove its line"),
        ));
    }
}

fn sha256(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// One recipe: its text (attributes, header and body) and the recipes its header depends on.
#[derive(Debug, Default)]
struct Recipe {
    text: String,
    dependencies: Vec<String>,
}

/// The justfile, split into recipes and the top-level lines (column-0 comments dropped).
#[derive(Debug, Default)]
struct Justfile {
    recipes: BTreeMap<String, Recipe>,
    top_level: String,
}

impl Justfile {
    fn parse(text: &str) -> Self {
        let mut parsed = Self::default();
        let mut attributes: Vec<&str> = Vec::new();
        let mut current: Option<(String, Vec<&str>, Vec<String>)> = None;
        let mut top: Vec<&str> = Vec::new();
        for line in text.lines() {
            let indented = line.is_empty() || line.starts_with([' ', '\t']);
            if indented {
                match current.as_mut() {
                    Some((_, lines, _)) => lines.push(line),
                    None if line.is_empty() => {}
                    None => top.push(line),
                }
                continue;
            }
            if let Some((name, lines, deps)) = current.take() {
                parsed.finish(name, &lines, deps);
            }
            if line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                attributes.push(line);
                continue;
            }
            match header(line) {
                Some((name, deps)) => {
                    let mut lines = std::mem::take(&mut attributes);
                    lines.push(line);
                    current = Some((name, lines, deps));
                }
                None => {
                    top.append(&mut attributes);
                    top.push(line);
                }
            }
        }
        if let Some((name, lines, deps)) = current.take() {
            parsed.finish(name, &lines, deps);
        }
        top.append(&mut attributes);
        parsed.top_level = top.join("\n");
        parsed
    }

    fn finish(&mut self, name: String, lines: &[&str], dependencies: Vec<String>) {
        let mut lines = lines.to_vec();
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        self.recipes.insert(
            name,
            Recipe {
                text: lines.join("\n"),
                dependencies,
            },
        );
    }
}

/// A recipe header at column 0, `[@]name [params]: [deps]`: its name and the words of its
/// dependencies (an assignment, `name := ...`, is not one).
fn header(line: &str) -> Option<(String, Vec<String>)> {
    let head = line.strip_prefix('@').unwrap_or(line);
    if !head.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    let (before, after) = head.split_once(':')?;
    if after.starts_with('=') {
        return None;
    }
    let name = before.split_whitespace().next()?;
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return None;
    }
    let deps = after.split('#').next().unwrap_or_default();
    let words = deps
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|w| !w.is_empty() && !w.starts_with('"') && w != &"&&")
        .map(str::to_owned)
        .collect();
    Some((name.to_owned(), words))
}

fn parse(path: &str, text: &str) -> Result<Table, Finding> {
    text.parse::<Table>().map_err(|error| {
        Finding::new(
            RULE,
            path,
            format!("does not parse: {}", error.to_string().trim()),
        )
    })
}

/// `.config/nextest.toml`'s allowlisted shape: `[test-groups]` of `max-threads`, and the profiles
/// `default`, `ci` and `serial` (the values `_preflight` accepts for `NEXTEST_PROFILE`) with only
/// `retries = 0`, `fail-fast`, `slow-timeout`, `test-threads`, `junit.path` and overrides of
/// exactly `filter` and `test-group`.
pub fn nextest_shape(text: &str) -> Vec<Finding> {
    let table = match parse(NEXTEST, text) {
        Ok(table) => table,
        Err(finding) => return vec![finding],
    };
    let mut problems = Vec::new();
    for (key, value) in &table {
        match key.as_str() {
            "test-groups" => {
                for (group, body) in value.as_table().into_iter().flatten() {
                    let ok = body.as_table().is_some_and(|t| {
                        t.len() == 1 && t.get("max-threads").is_some_and(Value::is_integer)
                    });
                    if !ok {
                        problems.push(format!(
                            "test group `{group}` must be exactly `{{ max-threads = <n> }}`"
                        ));
                    }
                }
            }
            "profile" => {
                for (name, body) in value.as_table().into_iter().flatten() {
                    if !matches!(name.as_str(), "default" | "ci" | "serial") {
                        problems.push(format!(
                            "profile `{name}` is not one of default, ci, serial"
                        ));
                        continue;
                    }
                    problems.extend(nextest_profile(name, body));
                }
            }
            _ => problems.push(format!(
                "the top-level key `{key}` is not allowed (only test-groups and profile)"
            )),
        }
    }
    problems
        .into_iter()
        .map(|p| Finding::new(RULE, NEXTEST, p))
        .collect()
}

fn nextest_profile(name: &str, body: &Value) -> Vec<String> {
    let Some(body) = body.as_table() else {
        return vec![format!("profile `{name}` is not a table")];
    };
    let mut problems = Vec::new();
    for (key, value) in body {
        let ok = match key.as_str() {
            "retries" => value.as_integer() == Some(0),
            "fail-fast" => value.is_bool(),
            "test-threads" => value.is_integer(),
            "slow-timeout" => value.as_table().is_some_and(|t| {
                t.iter().all(|(k, v)| match k.as_str() {
                    "period" => v.is_str(),
                    "terminate-after" => v.is_integer(),
                    _ => false,
                })
            }),
            "junit" => value
                .as_table()
                .is_some_and(|t| t.len() == 1 && t.get("path").is_some_and(Value::is_str)),
            "overrides" => value.as_array().is_some_and(|overrides| {
                overrides.iter().all(|o| {
                    o.as_table().is_some_and(|t| {
                        t.len() == 2
                            && t.get("filter").is_some_and(Value::is_str)
                            && t.get("test-group").is_some_and(Value::is_str)
                    })
                })
            }),
            _ => false,
        };
        if !ok {
            problems.push(format!(
                "`profile.{name}.{key}` is not allowed with that value (gate.rs nextest_shape: retries 0, fail-fast, test-threads, slow-timeout, junit.path, overrides of filter and test-group only)"
            ));
        }
    }
    problems
}

/// `mise.toml`'s allowlisted shape: `[tools]` with string versions, nothing else.
pub fn mise_shape(text: &str) -> Vec<Finding> {
    let table = match parse(MISE, text) {
        Ok(table) => table,
        Err(finding) => return vec![finding],
    };
    let mut findings = Vec::new();
    for (key, value) in &table {
        if key != "tools" {
            findings.push(Finding::new(
                RULE,
                MISE,
                format!("`{key}` is not allowed: mise.toml holds only [tools] (an [env] reaches every tool the gate runs through a shim)"),
            ));
            continue;
        }
        for (tool, version) in value.as_table().into_iter().flatten() {
            if !version.is_str() {
                findings.push(Finding::new(
                    RULE,
                    MISE,
                    format!("tool `{tool}` must be pinned by a plain version string"),
                ));
            }
        }
    }
    findings
}

/// `rust-toolchain.toml`'s allowlisted shape.
pub fn toolchain_shape(text: &str) -> Vec<Finding> {
    let table = match parse(TOOLCHAIN, text) {
        Ok(table) => table,
        Err(finding) => return vec![finding],
    };
    let strings = |v: &Value| v.as_array().is_some_and(|a| a.iter().all(Value::is_str));
    let mut findings = Vec::new();
    for (key, value) in &table {
        let Some(toolchain) = value.as_table().filter(|_| key == "toolchain") else {
            findings.push(Finding::new(
                RULE,
                TOOLCHAIN,
                format!("`{key}` is not allowed: only [toolchain]"),
            ));
            continue;
        };
        for (field, v) in toolchain {
            let ok = match field.as_str() {
                "channel" | "profile" => v.is_str(),
                "components" | "targets" => strings(v),
                _ => false,
            };
            if !ok {
                findings.push(Finding::new(
                    RULE,
                    TOOLCHAIN,
                    format!("`toolchain.{field}` is not allowed (only channel, profile, components and targets; a `path` would swap the compiler)"),
                ));
            }
        }
    }
    findings
}
