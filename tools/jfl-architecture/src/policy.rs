//! `architecture.toml`, parsed strictly (fnd §3.1): an unknown key, a wrong type or another
//! `schema` fails `ARCH-policy-schema`; the structure of the policy itself (empty lists, a crate
//! in two categories, a cycle, an adapter edge to a sibling) is checked by [`structure`], before
//! any crate is looked at.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::Finding;

const FILE: &str = "architecture.toml";

/// The grants of fnd §2.4, which are also the `# grant: <name>` sections of the root
/// `clippy.toml`.
pub const GRANTS: [&str; 7] = ["process", "socket", "fs", "spawn", "sleep", "clock", "env"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    schema: u32,
    workspace: RawWorkspace,
    roles: RawRoles,
    grants: RawGrants,
    dependencies: RawDependencies,
    #[serde(default)]
    external: RawExternal,
    #[serde(default)]
    exceptions: RawExceptions,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkspace {
    production: Vec<String>,
    test_only: Option<Vec<String>>,
    tool_only: Option<Vec<String>>,
    fuzz_only: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoles {
    pure: Option<Vec<String>>,
    application: Option<Vec<String>>,
    support: Option<Vec<String>>,
    adapter: Option<Vec<String>>,
    root: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGrants {
    process: Option<Vec<String>>,
    socket: Option<Vec<String>>,
    fs: Option<Vec<String>>,
    spawn: Option<Vec<String>>,
    sleep: Option<Vec<String>>,
    clock: Option<Vec<String>>,
    env: Option<Vec<String>>,
    independent: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDependencies {
    normal: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    build: BTreeMap<String, Vec<String>>,
}

type ExternalTable = BTreeMap<String, BTreeMap<String, Vec<String>>>;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawExternal {
    #[serde(default)]
    normal: ExternalTable,
    #[serde(default)]
    dev: ExternalTable,
    #[serde(default)]
    build: ExternalTable,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawExceptions {
    #[serde(default)]
    lints: Vec<LintException>,
}

/// An `[[exceptions.lints]]` row: `#[allow]`/`#[expect]`/`#[macro_export]` naming `lint` is
/// allowed in the file `path`, backed by ADR `adr` (fnd §3.5).
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LintException {
    /// The file, relative to the workspace root.
    pub path: String,
    /// The lint (or `macro_export`) the attribute names.
    pub lint: String,
    /// The ADR number, `NNNN`; `doc/adr/NNNN-*.md` must exist.
    pub adr: String,
}

/// The parsed policy.
#[derive(Debug, Default)]
pub struct Policy {
    /// `[workspace]`: category name to its crates, in the file's order.
    pub categories: Vec<(&'static str, Vec<String>)>,
    /// `[roles]`: role name to its crates.
    pub roles: Vec<(&'static str, Vec<String>)>,
    /// `[grants]` without `independent`: grant name to its crates.
    pub grants: Vec<(&'static str, Vec<String>)>,
    /// `[grants] independent`: the independent oracles of fnd §2.7.
    pub independent: Option<Vec<String>>,
    /// `[dependencies.normal]`: the legal normal edges between workspace crates.
    pub normal: BTreeMap<String, Vec<String>>,
    /// `[dependencies.build]`: the declared build edges between workspace crates.
    pub build: BTreeMap<String, Vec<String>>,
    /// `[external.normal|dev|build]`: crate to third-party crate to allowed features.
    pub external: BTreeMap<&'static str, ExternalTable>,
    /// `[[exceptions.lints]]`.
    pub exceptions: Vec<LintException>,
}

impl Policy {
    /// The category of `name`, if any.
    pub fn category(&self, name: &str) -> Option<&'static str> {
        self.categories
            .iter()
            .find(|(_, crates)| crates.iter().any(|c| c == name))
            .map(|(category, _)| *category)
    }

    /// Whether `name` holds `role`.
    pub fn has_role(&self, name: &str, role: &str) -> bool {
        self.list(&self.roles, role).iter().any(|c| c == name)
    }

    /// The crates of `role` (empty when absent).
    pub fn role(&self, role: &str) -> &[String] {
        self.list(&self.roles, role)
    }

    /// The grants of `name`.
    pub fn grants_of(&self, name: &str) -> BTreeSet<&'static str> {
        self.grants
            .iter()
            .filter(|(_, crates)| crates.iter().any(|c| c == name))
            .map(|(grant, _)| *grant)
            .collect()
    }

    fn list<'a>(&'a self, table: &'a [(&'static str, Vec<String>)], key: &str) -> &'a [String] {
        table
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, crates)| crates.as_slice())
            .unwrap_or_default()
    }

    /// Every crate name the policy mentions, with where (for `ARCH-missing-crate`).
    pub fn mentioned(&self) -> Vec<(String, String)> {
        let mut names = Vec::new();
        let tables = [
            ("workspace", &self.categories),
            ("roles", &self.roles),
            ("grants", &self.grants),
        ];
        for (table, lists) in tables {
            for (key, crates) in lists {
                names.extend(
                    crates
                        .iter()
                        .map(|c| (c.clone(), format!("[{table}] {key}"))),
                );
            }
        }
        if let Some(crates) = &self.independent {
            names.extend(
                crates
                    .iter()
                    .map(|c| (c.clone(), "[grants] independent".to_owned())),
            );
        }
        for (kind, table) in [("normal", &self.normal), ("build", &self.build)] {
            for (from, tos) in table {
                names.push((from.clone(), format!("[dependencies.{kind}]")));
                names.extend(
                    tos.iter()
                        .map(|to| (to.clone(), format!("[dependencies.{kind}] {from}"))),
                );
            }
        }
        for (kind, table) in &self.external {
            names.extend(
                table
                    .keys()
                    .map(|from| (from.clone(), format!("[external.{kind}]"))),
            );
        }
        names
    }
}

/// Parses `architecture.toml` strictly.
pub fn parse(text: &str) -> Result<Policy, Finding> {
    let raw: Raw = toml::from_str(text).map_err(|error| {
        Finding::new(
            "ARCH-policy-schema",
            FILE,
            format!(
                "the policy does not parse strictly: {}",
                error.to_string().trim()
            ),
        )
    })?;
    if raw.schema != 1 {
        return Err(Finding::new(
            "ARCH-policy-schema",
            FILE,
            format!("schema = {} is not the supported schema 1", raw.schema),
        ));
    }
    let present = |name: &'static str, list: Option<Vec<String>>| list.map(|crates| (name, crates));
    let categories = [
        Some(("production", raw.workspace.production)),
        present("test_only", raw.workspace.test_only),
        present("tool_only", raw.workspace.tool_only),
        present("fuzz_only", raw.workspace.fuzz_only),
    ];
    let roles = [
        present("pure", raw.roles.pure),
        present("application", raw.roles.application),
        present("support", raw.roles.support),
        present("adapter", raw.roles.adapter),
        present("root", raw.roles.root),
    ];
    let grants = [
        present("process", raw.grants.process),
        present("socket", raw.grants.socket),
        present("fs", raw.grants.fs),
        present("spawn", raw.grants.spawn),
        present("sleep", raw.grants.sleep),
        present("clock", raw.grants.clock),
        present("env", raw.grants.env),
    ];
    Ok(Policy {
        categories: categories.into_iter().flatten().collect(),
        roles: roles.into_iter().flatten().collect(),
        grants: grants.into_iter().flatten().collect(),
        independent: raw.grants.independent,
        normal: raw.dependencies.normal,
        build: raw.dependencies.build,
        external: BTreeMap::from([
            ("normal", raw.external.normal),
            ("dev", raw.external.dev),
            ("build", raw.external.build),
        ]),
        exceptions: raw.exceptions.lints,
    })
}

/// The policy's own structure: `ARCH-empty-selector`, `ARCH-duplicate-category`,
/// `ARCH-policy-cycle` and `ARCH-layer-isolation` in the edge table.
pub fn structure(policy: &Policy) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut lists: Vec<(String, &Vec<String>)> = Vec::new();
    for (table, entries) in [
        ("workspace", &policy.categories),
        ("roles", &policy.roles),
        ("grants", &policy.grants),
    ] {
        lists.extend(
            entries
                .iter()
                .map(|(key, crates)| (format!("[{table}] {key}"), crates)),
        );
    }
    if let Some(crates) = &policy.independent {
        lists.push(("[grants] independent".to_owned(), crates));
    }
    for (name, crates) in &lists {
        if crates.is_empty() {
            findings.push(Finding::new(
                "ARCH-empty-selector",
                FILE,
                format!("{name} is present but empty: remove it or name its crates"),
            ));
        }
        let mut seen = BTreeSet::new();
        for c in crates.iter() {
            if !seen.insert(c) {
                findings.push(Finding::new(
                    "ARCH-duplicate-category",
                    FILE,
                    format!("{name} names `{c}` twice"),
                ));
            }
        }
    }
    findings.extend(duplicates(&policy.categories, "a category"));
    findings.extend(duplicates(&policy.roles, "a role"));
    for (role, crates) in &policy.roles {
        for c in crates {
            if policy.category(c) != Some("production") {
                findings.push(Finding::new(
                    "ARCH-duplicate-category",
                    FILE,
                    format!("`{c}` has the role {role} but is not in [workspace] production"),
                ));
            }
        }
    }
    findings.extend(cycle(&policy.normal));
    findings.extend(ranks(policy));
    for adapter in policy.role("adapter") {
        for to in policy.normal.get(adapter).into_iter().flatten() {
            if policy.has_role(to, "adapter") || policy.has_role(to, "root") {
                findings.push(Finding::new(
                    "ARCH-layer-isolation",
                    FILE,
                    format!(
                        "[dependencies.normal] lets the adapter `{adapter}` depend on `{to}`; adapters never name a sibling adapter, composition or the CLI (fnd §2.2 rule 2)"
                    ),
                ));
            }
        }
    }
    findings
}

/// The layer law of fnd §2.1 and §2.2 over the edge table itself, so a widened row fails here
/// and not only in review: pure crates depend on pure crates, the application on pure crates,
/// `jfl-process` (support) on nothing, adapters on pure, application and support crates, roots on
/// production crates; test-only crates never on tool-only ones, tool-only crates on nothing, and
/// production never on either. A table change past this needs fnd §2.1 and this check amended.
fn ranks(policy: &Policy) -> Vec<Finding> {
    let role_of = |name: &str| {
        policy
            .roles
            .iter()
            .find(|(_, crates)| crates.iter().any(|c| c == name))
            .map(|(role, _)| *role)
    };
    let mut findings = Vec::new();
    for (from, tos) in policy.normal.iter().chain(policy.build.iter()) {
        for to in tos {
            let allowed = match (policy.category(from), role_of(from)) {
                (Some("production"), Some(role)) => {
                    let target = role_of(to);
                    policy.category(to) == Some("production")
                        && match role {
                            "pure" => target == Some("pure"),
                            "application" => target == Some("pure"),
                            "support" => false,
                            "adapter" => matches!(target, Some("pure" | "application" | "support")),
                            "root" => true,
                            _ => false,
                        }
                }
                (Some("test_only"), _) => {
                    matches!(policy.category(to), Some("production" | "test_only"))
                }
                // Unclassified crates are ARCH-missing-member's; tool-only crates name nothing.
                (None, _) | (Some("production"), None) => true,
                _ => false,
            };
            if !allowed {
                findings.push(Finding::new(
                    "ARCH-forbidden-edge",
                    FILE,
                    format!(
                        "the table lets `{from}` ({}) depend on `{to}` ({}), against the layer law of fnd §2.1-§2.2",
                        role_of(from).or(policy.category(from)).unwrap_or("unclassified"),
                        role_of(to).or(policy.category(to)).unwrap_or("unclassified"),
                    ),
                ));
            }
        }
    }
    findings
}

fn duplicates(lists: &[(&'static str, Vec<String>)], what: &str) -> Vec<Finding> {
    let mut owner: BTreeMap<&str, &str> = BTreeMap::new();
    let mut findings = Vec::new();
    for (key, crates) in lists {
        for c in crates {
            match owner.get(c.as_str()) {
                Some(first) if first != key => findings.push(Finding::new(
                    "ARCH-duplicate-category",
                    FILE,
                    format!("`{c}` is in {what} twice: {first} and {key}"),
                )),
                _ => {
                    owner.insert(c, key);
                }
            }
        }
    }
    findings
}

/// The first cycle in the edge table, as a path.
fn cycle(edges: &BTreeMap<String, Vec<String>>) -> Option<Finding> {
    // Depth-first search with an explicit stack: the path holds the crates being visited.
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for start in edges.keys() {
        if done.contains(start.as_str()) {
            continue;
        }
        let mut path: Vec<&str> = Vec::new();
        let mut stack: Vec<(&str, usize)> = vec![(start.as_str(), 0)];
        while let Some((node, next)) = stack.pop() {
            if next == 0 {
                path.push(node);
            }
            let children = edges.get(node).map(Vec::as_slice).unwrap_or_default();
            if let Some(child) = children.get(next) {
                stack.push((node, next.saturating_add(1)));
                if let Some(at) = path.iter().position(|p| *p == child.as_str()) {
                    let mut cycle: Vec<&str> = path.iter().skip(at).copied().collect();
                    cycle.push(child);
                    return Some(Finding::new(
                        "ARCH-policy-cycle",
                        FILE,
                        format!("[dependencies.normal] has a cycle: {}", cycle.join(" -> ")),
                    ));
                }
                if !done.contains(child.as_str()) {
                    stack.push((child.as_str(), 0));
                }
            } else {
                path.pop();
                done.insert(node);
            }
        }
    }
    None
}
