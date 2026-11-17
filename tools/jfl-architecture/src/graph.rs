//! The graph rules of fnd §2.1, §2.2, §2.5 and §2.7 over one `cargo metadata` result (one
//! target): classification, the edge table, dev and build edges, the production closure, the
//! independent oracles, adapter isolation and the external allowlist.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cargo_metadata::{DependencyKind, Metadata};

use crate::Finding;
use crate::policy::{self, Policy};

/// An edge kind, as `cargo metadata` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// `[dependencies]`.
    Normal,
    /// `[dev-dependencies]`.
    Dev,
    /// `[build-dependencies]`.
    Build,
}

impl Kind {
    fn of(kind: DependencyKind) -> Option<Self> {
        match kind {
            DependencyKind::Normal => Some(Self::Normal),
            DependencyKind::Development => Some(Self::Dev),
            DependencyKind::Build => Some(Self::Build),
            _ => None,
        }
    }

    /// The policy table's key for this kind.
    pub fn key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Dev => "dev",
            Self::Build => "build",
        }
    }
}

/// A direct dependency of a member, as declared in its manifest.
#[derive(Debug, Clone)]
pub struct Direct {
    /// The package name (not the rename).
    pub name: String,
    /// The edge kind.
    pub kind: Kind,
    /// Whether it names a workspace member (a path dependency on one).
    pub workspace: bool,
    /// The features it enables, `default` included when default features are on.
    pub features: BTreeSet<String>,
}

/// A workspace member.
#[derive(Debug, Clone)]
pub struct Member {
    /// Its package name.
    pub name: String,
    /// Its directory relative to the workspace root (`crates/jfl-domain`).
    pub dir: String,
    /// Its manifest, relative to the workspace root.
    pub manifest: String,
    /// Its direct dependencies.
    pub direct: Vec<Direct>,
    /// Its targets' root source files, relative to the workspace root.
    pub sources: Vec<String>,
}

/// The resolved workspace of one target.
#[derive(Debug, Default)]
pub struct Workspace {
    /// Members by name.
    pub members: BTreeMap<String, Member>,
    /// Resolved edges: package id to (dependency id, kinds).
    pub edges: BTreeMap<String, Vec<(String, BTreeSet<Kind>)>>,
    /// Package id to its name.
    pub names: BTreeMap<String, String>,
    /// Member name to its package id.
    pub ids: BTreeMap<String, String>,
    /// Whether `cargo metadata` resolved the graph.
    pub resolved: bool,
}

impl Workspace {
    /// The workspace of one `cargo metadata` result.
    pub fn from_metadata(data: &Metadata) -> Self {
        let root = data.workspace_root.as_str();
        let relative = |path: &str| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .trim_start_matches('/')
                .to_owned()
        };
        let member_ids: BTreeSet<&str> = data
            .workspace_members
            .iter()
            .map(|id| id.repr.as_str())
            .collect();
        let member_names: BTreeSet<&str> = data
            .packages
            .iter()
            .filter(|p| member_ids.contains(p.id.repr.as_str()))
            .map(|p| p.name.as_str())
            .collect();
        let mut workspace = Self::default();
        for package in &data.packages {
            workspace
                .names
                .insert(package.id.repr.clone(), package.name.to_string());
            if !member_ids.contains(package.id.repr.as_str()) {
                continue;
            }
            let manifest = relative(package.manifest_path.as_str());
            let dir = manifest
                .strip_suffix("Cargo.toml")
                .unwrap_or(&manifest)
                .trim_end_matches('/')
                .to_owned();
            // A feature of the crate's own `[features]` may enable a dependency's feature
            // (`dep/feat`, `dep?/feat`): that is enabling it too (fnd §2.5).
            let mut through_features: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
            for value in package.features.values().flatten() {
                if let Some((dep, feature)) = value.split_once('/') {
                    through_features
                        .entry(dep.trim_end_matches('?'))
                        .or_default()
                        .insert(feature.to_owned());
                }
            }
            let direct = package
                .dependencies
                .iter()
                .filter_map(|dep| {
                    let kind = Kind::of(dep.kind)?;
                    let mut features: BTreeSet<String> = dep.features.iter().cloned().collect();
                    if dep.uses_default_features {
                        features.insert("default".to_owned());
                    }
                    let key = dep.rename.as_deref().unwrap_or(dep.name.as_str());
                    let key = key.replace('-', "_");
                    for (named, enabled) in &through_features {
                        if named.replace('-', "_") == key {
                            features.extend(enabled.iter().cloned());
                        }
                    }
                    Some(Direct {
                        name: dep.name.clone(),
                        kind,
                        workspace: dep.source.is_none() && member_names.contains(dep.name.as_str()),
                        features,
                    })
                })
                .collect();
            workspace
                .ids
                .insert(package.name.to_string(), package.id.repr.clone());
            workspace.members.insert(
                package.name.to_string(),
                Member {
                    name: package.name.to_string(),
                    dir,
                    manifest,
                    direct,
                    sources: package
                        .targets
                        .iter()
                        .map(|t| relative(t.src_path.as_str()))
                        .collect(),
                },
            );
        }
        if let Some(resolve) = &data.resolve {
            workspace.resolved = true;
            for node in &resolve.nodes {
                let deps = node
                    .deps
                    .iter()
                    .map(|dep| {
                        let kinds = dep
                            .dep_kinds
                            .iter()
                            .filter_map(|info| Kind::of(info.kind))
                            .collect();
                        (dep.pkg.repr.clone(), kinds)
                    })
                    .collect();
                workspace.edges.insert(node.id.repr.clone(), deps);
            }
        }
        workspace
    }

    fn name_of<'a>(&'a self, id: &'a str) -> &'a str {
        self.names.get(id).map(String::as_str).unwrap_or(id)
    }

    /// Breadth-first search from `start` over edges of `kinds`; the shortest path (as names) to
    /// each reachable workspace member other than `start`.
    fn reachable_members(&self, start: &str, kinds: &[Kind]) -> BTreeMap<String, Vec<String>> {
        let mut found = BTreeMap::new();
        let Some(start_id) = self.ids.get(start) else {
            return found;
        };
        let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
        let mut queue: VecDeque<&str> = VecDeque::from([start_id.as_str()]);
        let mut seen: BTreeSet<&str> = BTreeSet::from([start_id.as_str()]);
        while let Some(id) = queue.pop_front() {
            for (dep, dep_kinds) in self.edges.get(id).into_iter().flatten() {
                if !kinds.iter().any(|k| dep_kinds.contains(k)) || !seen.insert(dep.as_str()) {
                    continue;
                }
                parent.insert(dep.as_str(), id);
                queue.push_back(dep.as_str());
                let name = self.name_of(dep);
                if self.members.contains_key(name) && self.ids.get(name) == Some(dep) {
                    let mut path = vec![name.to_owned()];
                    let mut cursor = dep.as_str();
                    while let Some(up) = parent.get(cursor) {
                        path.push(self.name_of(up).to_owned());
                        cursor = up;
                    }
                    path.reverse();
                    found.insert(name.to_owned(), path);
                }
            }
        }
        found
    }
}

/// Every graph rule over `workspace`, the policy's own structure included.
pub fn check(policy: &Policy, workspace: &Workspace) -> Vec<Finding> {
    let mut findings = policy::structure(policy);
    if !workspace.resolved {
        findings.push(Finding::new(
            "ARCH-metadata",
            "Cargo.toml",
            "`cargo metadata` returned no resolved graph",
        ));
    }
    classification(policy, workspace, &mut findings);
    for member in workspace.members.values() {
        let prefix = format!("{}/", member.dir);
        for source in &member.sources {
            if !member.dir.is_empty() && !source.starts_with(&prefix) {
                findings.push(Finding::new(
                    "ARCH-hidden-source",
                    member.manifest.as_str(),
                    format!("`{}` builds a target from `{source}`, outside its directory, where the source scan does not look", member.name),
                ));
            }
        }
    }
    edges(policy, workspace, &mut findings);
    closure(policy, workspace, &mut findings);
    externals(policy, workspace, &mut findings);
    findings
}

fn classification(policy: &Policy, workspace: &Workspace, findings: &mut Vec<Finding>) {
    for (name, place) in policy.mentioned() {
        if !workspace.members.contains_key(&name) {
            findings.push(Finding::new(
                "ARCH-missing-crate",
                "architecture.toml",
                format!("{place} names `{name}`, which is not a workspace member"),
            ));
        }
    }
    for member in workspace.members.values() {
        let at = member.manifest.as_str();
        let name = member.name.as_str();
        match policy.category(name) {
            None => findings.push(Finding::new(
                "ARCH-missing-member",
                at,
                format!("`{name}` is a workspace member in no [workspace] category"),
            )),
            Some("production") => {
                if !policy
                    .roles
                    .iter()
                    .any(|(_, crates)| crates.iter().any(|c| c == name))
                {
                    findings.push(Finding::new(
                        "ARCH-missing-member",
                        at,
                        format!("the production crate `{name}` has no role in [roles]"),
                    ));
                }
            }
            Some(_) => {}
        }
        if !policy.normal.contains_key(name) {
            findings.push(Finding::new(
                "ARCH-missing-member",
                at,
                format!("`{name}` has no row in [dependencies.normal] (an empty list if it names no workspace crate)"),
            ));
        }
    }
}

fn edges(policy: &Policy, workspace: &Workspace, findings: &mut Vec<Finding>) {
    for member in workspace.members.values() {
        let from = member.name.as_str();
        let at = member.manifest.as_str();
        let allowed = policy
            .normal
            .get(from)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let build = policy
            .build
            .get(from)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let is_adapter = policy.has_role(from, "adapter");
        for dep in member.direct.iter().filter(|d| d.workspace) {
            let to = dep.name.as_str();
            let listed = allowed.iter().any(|a| a == to);
            match dep.kind {
                Kind::Normal if !listed => findings.push(Finding::new(
                    "ARCH-forbidden-edge",
                    at,
                    format!("`{from}` depends on `{to}`, which plan §6.3 ([dependencies.normal]) does not allow"),
                )),
                Kind::Dev if !listed && policy.category(to) != Some("test_only") => {
                    findings.push(Finding::new(
                        "ARCH-forbidden-dev-edge",
                        at,
                        format!("`{from}` dev-depends on `{to}`, which is neither test-only nor in its [dependencies.normal] row (fnd §2.2 rule 3)"),
                    ));
                }
                Kind::Build if !build.iter().any(|b| b == to) => findings.push(Finding::new(
                    "ARCH-undeclared-build-dep",
                    at,
                    format!("`{from}` build-depends on `{to}`, which [dependencies.build] does not declare (fnd §2.2 rule 5)"),
                )),
                _ => {}
            }
            if is_adapter && (policy.has_role(to, "adapter") || policy.has_role(to, "root")) {
                findings.push(Finding::new(
                    "ARCH-layer-isolation",
                    at,
                    format!("the adapter `{from}` names `{to}` ({} edge); adapters never name a sibling adapter, composition or the CLI (fnd §2.2 rule 2)", dep.kind.key()),
                ));
            }
        }
    }
}

fn closure(policy: &Policy, workspace: &Workspace, findings: &mut Vec<Finding>) {
    for member in workspace.members.values() {
        let name = member.name.as_str();
        if policy.category(name) == Some("production") {
            for (reached, path) in workspace.reachable_members(name, &[Kind::Normal, Kind::Build]) {
                if let Some(category @ ("test_only" | "tool_only")) = policy.category(&reached) {
                    findings.push(Finding::new(
                        "ARCH-production-closure",
                        member.manifest.as_str(),
                        format!(
                            "the production crate `{name}` reaches the {category} crate `{reached}`: {}",
                            path.join(" -> ")
                        ),
                    ));
                }
            }
        }
        let independent = policy.independent.as_deref().unwrap_or_default();
        if independent.iter().any(|c| c == name) {
            let mut reached = workspace.reachable_members(name, &[Kind::Normal, Kind::Build]);
            // A dev edge counts too, though only directly: an oracle's tests must not lean on
            // the product either.
            for dep in member
                .direct
                .iter()
                .filter(|d| d.workspace && d.kind == Kind::Dev)
            {
                reached
                    .entry(dep.name.clone())
                    .or_insert_with(|| vec![name.to_owned(), dep.name.clone()]);
            }
            for (to, path) in reached {
                findings.push(Finding::new(
                    "ARCH-independent-oracle",
                    member.manifest.as_str(),
                    format!(
                        "the independent tool `{name}` reaches the workspace crate `{to}`: {} (fnd §2.7)",
                        path.join(" -> ")
                    ),
                ));
            }
        }
    }
}

fn externals(policy: &Policy, workspace: &Workspace, findings: &mut Vec<Finding>) {
    for member in workspace.members.values() {
        let from = member.name.as_str();
        let at = member.manifest.as_str();
        for dep in member.direct.iter().filter(|d| !d.workspace) {
            let kind = dep.kind.key();
            let allowed = policy
                .external
                .get(kind)
                .and_then(|table| table.get(from))
                .and_then(|deps| deps.get(&dep.name));
            let Some(allowed) = allowed else {
                findings.push(Finding::new(
                    "ARCH-unlisted-external",
                    at,
                    format!("`{from}` names the third-party crate `{}` ({kind}), which [external.{kind}] does not list for it (fnd §2.5)", dep.name),
                ));
                continue;
            };
            if allowed.iter().any(|f| f == "*") {
                continue;
            }
            for feature in &dep.features {
                if !allowed.contains(feature) {
                    findings.push(Finding::new(
                        "ARCH-external-feature",
                        at,
                        format!("`{from}` enables the feature `{feature}` of `{}` ({kind}), which [external.{kind}] does not allow (allowed: {allowed:?})", dep.name),
                    ));
                }
            }
        }
    }
}
