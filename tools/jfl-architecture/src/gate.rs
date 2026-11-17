//! `ARCH-gate-config` (fnd §3.6, FND-08): the gate's own configuration is pinned, so a commit
//! cannot neuter the gate it is judged by without changing a pinned expectation in plain sight.
//! Every check is an allowlist or a pinned shape, never a list of bad patterns (fnd §3):
//!
//! - **the pin manifest** [`PINS`]: one SHA-256 per justfile recipe of the gate's closure (the
//!   roots [`ROOTS`], every stage of xtask's catalog, `list --json`, and their header
//!   dependencies), one for the justfile's top-level lines (settings, exports, variables,
//!   aliases), one per file of [`GATE_FILES`], per `scripts/test-*.sh` (which `script-tests` runs
//!   by glob; FND-09) and per xtask composition source ([`XTASK_SOURCES`]: the manifest and every
//!   file under `src/`, where the stage catalog, its commands and the preflight live since XT-01);
//!   an entry missing, extra or different fails, naming the line to write. A `scripts/...` path
//!   named in a pinned recipe or a top-level line must be pinned;
//! - **`.config/nextest.toml`**: only the keys of [`nextest_shape`] (no `default-filter`, no
//!   `[scripts]`, `retries` 0, overrides that set a test group and nothing else);
//! - **`mise.toml`**: only `[tools]`, each tool `just` or an `aqua:` or `github:` backend (no
//!   plugin backend, whose plugin is code) pinned by a plain version string (no `[env]`,
//!   `[tasks]`, `[hooks]` or settings), and no other mise configuration file anywhere in the tree;
//! - **`mise.lock`**: [`mise_lock_shape`], one entry per tool of `mise.toml` and nothing else,
//!   each platform a checksummed GitHub release asset (FND-09);
//! - **the implicit configuration of the gate's tools** (FND-09): the root's dot-entries and
//!   `mise*` entries, `.github/`'s entries and `.config/`'s entries are allowlists
//!   ([`ROOT_ALLOWED`], [`GITHUB_ALLOWED`], [`CONFIG_ALLOWED`]; xtask's `tool-config`,
//!   tools/xtask/src/tool_config.rs, holds the same lists and runs before any tool), and no
//!   `rustfmt.toml` may exist;
//! - **`rust-toolchain.toml`**: only `[toolchain]` with `channel`, `profile`, `components` and
//!   `targets` (no `path`), and no other toolchain file in the tree;
//! - **no cargo configuration file** (`.cargo/config`, `.cargo/config.toml`) anywhere in the tree:
//!   the allowlist of committed cargo configuration is empty (xtask's preflight refuses one before
//!   any stage's cargo runs, and scripts/xtask.sh builds xtask from outside the tree; this is the
//!   second line).
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
/// The justfile recipes the gate starts from, besides the recipe of every stage of xtask's catalog.
pub const ROOTS: [&str; 6] = [
    "quick",
    "check",
    "check-only",
    "ci-parity",
    "platform-tests",
    "_preflight",
];
/// mise's lockfile.
pub const MISE_LOCK: &str = "mise.lock";
/// The files the gate's recipes run or read, pinned whole: the scripts, the policies a stage
/// reads by path (`.github/zizmor.yml`, `deny.toml`) and the tool pins (`mise.toml`, whose new
/// tool would land on the stages' PATH, and `mise.lock`; FND-09), and what decides landing and
/// guards a session (`scripts/land.sh`, `scripts/dco-check.sh`, the SessionStart and Stop hooks;
/// the FND-09 review's G5), the launcher that builds and runs xtask (`scripts/xtask.sh`,
/// XT-01) and xtask's tool pins (`tools/versions.toml`, whose URLs and digests `install-tools`
/// trusts and whose tools lead the stages' PATH; XT-02). Every `scripts/test-*.sh` and every xtask composition source ([`XTASK_SOURCES`]) is
/// pinned too, found by [`gate_files`]. Not pinned, each for its reason (fnd §3.6):
/// `architecture.toml` and the `clippy.toml` files change with every new crate and are held to
/// their schema by the architecture test itself (`ARCH-policy-schema`, `ARCH-clippy-lists`);
/// this checker's own sources are code under its meta-test (`tests/rules.rs`, one negative case
/// per rule) and review; `.gitleaks.toml` is compared byte for byte with what the pinned
/// `scripts/gitleaks-config.sh` generates.
pub const GATE_FILES: [&str; 16] = [
    ".claude/hooks/session-start.sh",
    ".claude/hooks/stop-gate.sh",
    ".github/zizmor.yml",
    "deny.toml",
    "mise.lock",
    "mise.toml",
    "scripts/adr-index.sh",
    "scripts/bootstrap-tools.sh",
    "scripts/clippy-lists.sh",
    "scripts/dco-check.sh",
    "scripts/gitleaks-config.sh",
    "scripts/land.sh",
    "scripts/secrets-check.sh",
    "scripts/workflows-check.sh",
    "scripts/xtask.sh",
    "tools/versions.toml",
];
/// The xtask composition (XT-01, ADR 0035): its manifest and every file under its `src/` (the
/// stage catalog, each stage's command, the preflight and the tool configuration check), pinned
/// whole by [`gate_files`]; its tests are code under review, like this checker's.
pub const XTASK_SOURCES: (&str, &str) = ("tools/xtask/Cargo.toml", "tools/xtask/src");
/// The entries allowed at the root among the names that start with a dot or with `mise`
/// (tools/xtask/src/tool_config.rs holds the same list): `.shellcheckrc`, `.tool-versions`,
/// `.mise.toml`, `mise.local.toml`, `.pinact.yaml`, `.yamllint`, `.envrc` and the like are
/// refused, since a tool of the gate would read them without a command line naming them.
pub const ROOT_ALLOWED: [&str; 14] = [
    ".DS_Store",
    ".claude",
    ".config",
    ".editorconfig",
    ".git",
    ".gitattributes",
    ".github",
    ".gitignore",
    ".gitleaks.toml",
    ".idea",
    ".vscode",
    ".work",
    "mise.lock",
    "mise.toml",
];
/// The entries allowed in `.github/` (no `actionlint.yaml`, `pinact.yaml`, ...).
pub const GITHUB_ALLOWED: [&str; 7] = [
    "CODEOWNERS",
    "ISSUE_TEMPLATE",
    "actions",
    "dependabot.yml",
    "pull_request_template.md",
    "workflows",
    "zizmor.yml",
];
/// The entries allowed in `.config/` (no `mise/`, `mise.toml`, ...).
pub const CONFIG_ALLOWED: [&str; 2] = ["gate.sha256", "nextest.toml"];
/// The manifest's name for the justfile's top-level lines.
pub const TOP_LEVEL: &str = "justfile::(top-level)";

/// Runs every check of the rule over `tree`, with `catalog` the JSON of xtask's `list --json`.
pub fn check(tree: &dyn Tree, catalog: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let files = tree.files_under("");
    pins(tree, catalog, &mut findings);
    match tree.read(NEXTEST) {
        Some(text) => findings.extend(nextest_shape(&text)),
        None => findings.push(Finding::new(RULE, NEXTEST, "the file is missing")),
    }
    match tree.read(MISE) {
        Some(text) => findings.extend(mise_shape(&text)),
        None => findings.push(Finding::new(RULE, MISE, "the file is missing")),
    }
    match (tree.read(MISE), tree.read(MISE_LOCK)) {
        (Some(mise), Some(lock)) => findings.extend(mise_lock_shape(&mise, &lock)),
        (_, None) => findings.push(Finding::new(RULE, MISE_LOCK, "the file is missing")),
        (None, Some(_)) => {}
    }
    findings.extend(entries(&files));
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

/// The allowlists of [`ROOT_ALLOWED`], [`GITHUB_ALLOWED`] and [`CONFIG_ALLOWED`] over the tree's
/// files: one finding per entry outside them.
fn entries(files: &[String]) -> Vec<Finding> {
    let mut refused = BTreeSet::new();
    for path in files {
        let mut parts = path.split('/');
        let first = parts.next().unwrap_or_default();
        let second = parts.next();
        let checked = first.starts_with('.') || first.starts_with("mise");
        if checked && !ROOT_ALLOWED.contains(&first) {
            refused.insert((
                first.to_owned(),
                "not an allowed entry at the root: its dot-names and mise* names are an allowlist (fnd §3.6; a tool of the gate could read it without being told)",
            ));
        }
        if let Some(entry) = second {
            if first == ".github" && !GITHUB_ALLOWED.contains(&entry) {
                refused.insert((
                    format!(".github/{entry}"),
                    "not an allowed entry of .github/ (fnd §3.6; actionlint, pinact and others read their configuration there by themselves)",
                ));
            }
            if first == ".config" && !CONFIG_ALLOWED.contains(&entry) {
                refused.insert((
                    format!(".config/{entry}"),
                    "not an allowed entry of .config/ (fnd §3.6; mise, nextest and others read their configuration there by themselves)",
                ));
            }
        }
    }
    refused
        .into_iter()
        .map(|(at, why)| Finding::new(RULE, at, why))
        .collect()
}

/// Why a file may not exist in the tree, if it may not.
fn stray(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let parent = path
        .strip_suffix(name)
        .unwrap_or_default()
        .trim_end_matches('/');
    // Cargo and toolchain names case-insensitively: a case-insensitive file system (macOS) opens
    // `.Cargo/Config.toml` as `.cargo/config.toml` (the FND-09 review's H6).
    let (lower_name, lower_parent) = (name.to_ascii_lowercase(), parent.to_ascii_lowercase());
    let in_cargo_dir = lower_parent == ".cargo" || lower_parent.ends_with("/.cargo");
    if in_cargo_dir && matches!(lower_name.as_str(), "config" | "config.toml") {
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
    if name == ".gitleaks.toml" && path != ".gitleaks.toml" {
        return Some(
            "a gitleaks policy: only the root .gitleaks.toml may exist (fnd §7.4; the secrets stage reads the root one only)",
        );
    }
    if matches!(name, "rustfmt.toml" | ".rustfmt.toml") {
        return Some(
            "a rustfmt configuration file: none may exist (fnd §3.6; it would change what fmt-check accepts without the justfile naming it)",
        );
    }
    if matches!(
        lower_name.as_str(),
        "rust-toolchain" | "rust-toolchain.toml"
    ) && path != TOOLCHAIN
    {
        return Some(
            "a toolchain file: only the root rust-toolchain.toml (shape-checked) may exist (fnd §3.6)",
        );
    }
    None
}

/// The pin manifest's expected lines, computed from the tree and the stage catalog (the JSON of
/// xtask's `list --json`): `(entry, sha256 hex)`.
pub fn expected_pins(tree: &dyn Tree, catalog: &str) -> Result<BTreeMap<String, String>, Finding> {
    let justfile = tree
        .read(ci::JUSTFILE)
        .ok_or_else(|| Finding::new(RULE, ci::JUSTFILE, "the file is missing"))?;
    let stages = ci::check_stages(catalog).map_err(|finding| {
        Finding::new(
            RULE,
            finding.at,
            format!("{}: the gate's recipes are unknown", finding.message),
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
    for file in gate_files(tree) {
        let text = tree
            .read(&file)
            .ok_or_else(|| Finding::new(RULE, file.as_str(), "a file of the gate is missing"))?;
        pins.insert(file, sha256(&text));
    }
    Ok(pins)
}

/// [`GATE_FILES`], every `scripts/test-*.sh` of the tree (the `script-tests` stage runs them by
/// glob, so a new one is a change of the gate) and every xtask composition source
/// ([`XTASK_SOURCES`]: a new module of xtask is a change of the gate too), sorted.
pub fn gate_files(tree: &dyn Tree) -> Vec<String> {
    let mut files: BTreeSet<String> = GATE_FILES.iter().map(|f| (*f).to_owned()).collect();
    files.insert(XTASK_SOURCES.0.to_owned());
    files.extend(tree.files_under(XTASK_SOURCES.1));
    for path in tree.files_under("scripts") {
        let name = path.strip_prefix("scripts/").unwrap_or_default();
        if name.starts_with("test-") && name.ends_with(".sh") && !name.contains('/') {
            files.insert(path);
        }
    }
    files.into_iter().collect()
}

fn pins(tree: &dyn Tree, catalog: &str, findings: &mut Vec<Finding>) {
    let expected = match expected_pins(tree, catalog) {
        Ok(expected) => expected,
        Err(finding) => {
            findings.push(finding);
            return;
        }
    };
    let justfile = tree.read(ci::JUSTFILE).unwrap_or_default();
    let parsed = Justfile::parse(&justfile);
    let top = (TOP_LEVEL.to_owned(), parsed.top_level.as_str());
    let pinned_recipes = parsed
        .recipes
        .iter()
        .filter(|(name, _)| expected.contains_key(&format!("justfile::{name}")))
        .map(|(name, recipe)| (format!("`{name}`"), recipe.text.as_str()));
    for (name, text) in std::iter::once(top).chain(pinned_recipes) {
        for word in text.split(|c: char| c.is_whitespace() || "\"'(){}".contains(c)) {
            if word.starts_with("scripts/") && !word.contains('*') {
                let file = word.trim_end_matches([';', ',', ')']);
                if !expected.contains_key(file) {
                    findings.push(Finding::new(
                        RULE,
                        ci::JUSTFILE,
                        format!("the gate recipe {name} runs `{file}`, which gate.rs's GATE_FILES does not pin"),
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

/// A tool key mise.toml and mise.lock may hold: `just`, `aqua:<owner>/<repo>[/<name>]` or
/// `github:<owner>/<repo>`: the backends that install a checksummed release asset, never a
/// plugin backend (asdf, vfox), whose plugin is code that mise runs.
pub fn allowed_tool(key: &str) -> bool {
    let word = |w: &str| {
        !w.is_empty()
            && w.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    if key == "just" {
        return true;
    }
    if let Some(rest) = key.strip_prefix("aqua:") {
        let parts: Vec<&str> = rest.split('/').collect();
        return (2..=3).contains(&parts.len()) && parts.iter().all(|p| word(p));
    }
    if let Some(rest) = key.strip_prefix("github:") {
        let parts: Vec<&str> = rest.split('/').collect();
        return parts.len() == 2 && parts.iter().all(|p| word(p));
    }
    false
}

/// A plain version string: letters, digits, `.`, `_`, `+` and `-` (no template, no range).
fn plain_version(value: &Value) -> bool {
    value.as_str().is_some_and(|v| {
        !v.is_empty()
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
    })
}

/// `mise.lock`'s allowlisted shape, against `mise.toml`'s tools: `lockfile_version = 3` and one
/// `[[tools.<key>]]` per tool of `mise.toml`, each with exactly `version`, `backend` (an allowed
/// tool key), `specifiers` and per-platform tables of linux-x64, linux-arm64 or macos-arm64 that
/// hold a `sha256:` checksum, a `https://github.com/` url, a `https://api.github.com/repos/`
/// url_api and optionally `provenance = "github-attestations"`.
pub fn mise_lock_shape(mise: &str, lock: &str) -> Vec<Finding> {
    let table = match parse(MISE_LOCK, lock) {
        Ok(table) => table,
        Err(finding) => return vec![finding],
    };
    let mut problems = Vec::new();
    for (key, value) in &table {
        match key.as_str() {
            "lockfile_version" if value.as_integer() == Some(3) => {}
            "tools" => {}
            _ => problems.push(format!(
                "the top-level key `{key}` is not allowed (only lockfile_version = 3 and tools)"
            )),
        }
    }
    let locked: BTreeMap<String, Value> = table
        .get("tools")
        .and_then(Value::as_table)
        .map(|t| t.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    for (tool, entries) in &locked {
        if !allowed_tool(tool) {
            problems.push(format!(
                "tool `{tool}` is not `just`, an aqua or a github backend"
            ));
        }
        let Some(entries) = entries.as_array().filter(|a| a.len() == 1) else {
            problems.push(format!(
                "tool `{tool}` must have exactly one [[tools.{tool}]] entry"
            ));
            continue;
        };
        for entry in entries.iter() {
            let Some(entry) = entry.as_table() else {
                problems.push(format!("tool `{tool}`: an entry that is not a table"));
                continue;
            };
            if !(entry.contains_key("version") && entry.contains_key("backend")) {
                problems.push(format!(
                    "tool `{tool}`: an entry without its version or backend"
                ));
            }
            for (field, v) in entry {
                let ok = match field.as_str() {
                    "version" => plain_version(v),
                    "backend" => v.as_str().is_some_and(|b| b != "just" && allowed_tool(b)),
                    "specifiers" => v.as_array().is_some_and(|a| a.iter().all(plain_version)),
                    "platforms.linux-x64" | "platforms.linux-arm64" | "platforms.macos-arm64" => {
                        lock_platform(v)
                    }
                    _ => false,
                };
                if !ok {
                    problems.push(format!("`tools.{tool}.{field}` is not allowed with that value (a checksummed GitHub release asset of an aqua or github tool)"));
                }
            }
        }
    }
    if let Ok(mise) = mise.parse::<Table>() {
        let pinned: BTreeSet<&String> = mise
            .get("tools")
            .and_then(Value::as_table)
            .map(|t| t.keys().collect())
            .unwrap_or_default();
        let have: BTreeSet<&String> = locked.keys().collect();
        for tool in pinned.difference(&have) {
            problems.push(format!(
                "tool `{tool}` of mise.toml has no entry: run `mise lock`"
            ));
        }
        for tool in have.difference(&pinned) {
            problems.push(format!(
                "tool `{tool}` is not in mise.toml: remove its entry"
            ));
        }
    }
    problems
        .into_iter()
        .map(|p| Finding::new(RULE, MISE_LOCK, p))
        .collect()
}

fn lock_platform(value: &Value) -> bool {
    let url = |v: &Value, prefix: &str| {
        v.as_str().is_some_and(|u| {
            u.strip_prefix(prefix).is_some_and(|rest| {
                !rest.is_empty()
                    && rest.chars().all(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '+' | '-')
                    })
            })
        })
    };
    value.as_table().is_some_and(|t| {
        t.get("checksum").is_some()
            && t.iter().all(|(k, v)| match k.as_str() {
                "checksum" => v.as_str().is_some_and(|c| {
                    c.strip_prefix("sha256:").is_some_and(|h| {
                        h.len() == 64
                            && h.chars()
                                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                    })
                }),
                "url" => url(v, "https://github.com/"),
                "url_api" => url(v, "https://api.github.com/repos/"),
                "provenance" => v.as_str() == Some("github-attestations"),
                _ => false,
            })
    })
}

/// `mise.toml`'s allowlisted shape: `[tools]` of allowed tool keys ([`allowed_tool`]) pinned by
/// plain version strings, nothing else.
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
            if !allowed_tool(tool) {
                findings.push(Finding::new(
                    RULE,
                    MISE,
                    format!("tool `{tool}` is not `just`, an aqua or a github backend (a plugin backend runs its plugin's code)"),
                ));
            }
            if !plain_version(version) {
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
