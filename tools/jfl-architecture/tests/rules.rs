//! The architecture test's own negative cases (fnd §3.4): one table of cases over a synthetic
//! workspace (`cargo metadata` JSON, the policy and the files in memory), each a small change to a
//! clean baseline that must produce its rule's id at its path; the clean baseline itself and a
//! few look-alikes must produce nothing. The meta-test fails when a rule fnd §3.3 assigns to
//! FND-07 or FND-05 has no negative case here, in `tests/fixture_workspaces.rs` or in
//! `tests/ci_lockstep.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::Path;

use jfl_architecture::tree::MemoryTree;
use jfl_architecture::{Finding, RULES, Scope, check, check_repository, parse_metadata};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error>>;

const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// A dependency in the synthetic graph.
#[derive(Clone)]
struct Dep {
    name: &'static str,
    kind: Option<&'static str>,
    external: bool,
    features: Vec<&'static str>,
}

fn dep(name: &'static str) -> Dep {
    Dep {
        name,
        kind: None,
        external: false,
        features: Vec::new(),
    }
}

fn dev(name: &'static str) -> Dep {
    Dep {
        kind: Some("dev"),
        ..dep(name)
    }
}

fn build(name: &'static str) -> Dep {
    Dep {
        kind: Some("build"),
        ..dep(name)
    }
}

fn ext(name: &'static str) -> Dep {
    Dep {
        external: true,
        ..dep(name)
    }
}

/// A synthetic workspace: members (name, deps), third-party packages (name, deps), the policy
/// and every other file.
#[derive(Clone)]
struct World {
    members: Vec<(&'static str, Vec<Dep>)>,
    externals: Vec<(&'static str, Vec<Dep>)>,
    files: BTreeMap<String, String>,
    /// A change that could not apply (a missing file, text or member): reported by `findings`,
    /// so a stale case fails instead of passing on the baseline.
    broken: Vec<String>,
    scratch: Vec<Dep>,
    /// Per member: its own `[features]` and its targets, as `cargo metadata` JSON.
    package_fields: BTreeMap<&'static str, Vec<(&'static str, Value)>>,
}

const POLICY: &str = r#"schema = 1

[workspace]
production = ["dom", "app", "ad1", "ad2", "comp"]
test_only = ["kit"]
tool_only = ["orc"]

[roles]
pure = ["dom"]
application = ["app"]
adapter = ["ad1", "ad2"]
root = ["comp"]

[grants]
fs = ["ad1"]
independent = ["orc"]

[dependencies.normal]
dom = []
app = ["dom"]
ad1 = ["dom", "app"]
ad2 = ["dom"]
comp = ["dom", "app", "ad1", "ad2"]
kit = ["dom"]
orc = []

[dependencies.build]

[external.normal]
ad1 = { ext = ["default"] }

[external.dev]
[external.build]

[exceptions]
"#;

const ROOT_MANIFEST: &str = r#"[workspace]
members = ["crates/dom", "crates/app", "crates/ad1", "crates/ad2", "crates/comp", "crates/kit", "tools/orc"]

[workspace.lints.rust]
unsafe_code = "forbid"
unused_must_use = "deny"
missing_docs = "warn"
unreachable_pub = "warn"
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(coverage)"] }

[workspace.lints.clippy]
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

const ROOT_CLIPPY: &str = r#"# The canonical list.
allow-unwrap-in-tests = true

disallowed-methods = [
  # grant: fs
  { path = "std::fs::read", reason = "fs" },
  # grant: none
  { path = "tokio::sync::mpsc::unbounded_channel", reason = "unbounded", allow-invalid = true },
]

disallowed-types = [
  # grant: none
  { path = "std::net::TcpStream", reason = "tcp" },
  #pure { path = "core::cell::Cell", reason = "pure" },
]
"#;

const AD1_CLIPPY: &str = r#"allow-unwrap-in-tests = true

disallowed-methods = [
  # grant: fs (granted to this crate: lifted)
  # grant: none
  { path = "tokio::sync::mpsc::unbounded_channel", reason = "unbounded", allow-invalid = true },
]

disallowed-types = [
  # grant: none
  { path = "std::net::TcpStream", reason = "tcp" },
]
"#;

const DOM_CLIPPY: &str = r#"allow-unwrap-in-tests = true

disallowed-methods = [
  # grant: fs
  { path = "std::fs::read", reason = "fs", allow-invalid = true },
  # grant: none
  { path = "tokio::sync::mpsc::unbounded_channel", reason = "unbounded", allow-invalid = true },
]

disallowed-types = [
  # grant: none
  { path = "std::net::TcpStream", reason = "tcp", allow-invalid = true },
  { path = "core::cell::Cell", reason = "pure" },
]
"#;

const GOOD_TEST: &str = r##"//! Tests.

#[test]
fn should_pass_when_nothing_is_wrong() {
    // given
    let value = "#[ignore] cfg!(target_os) // when";

    // when
    let length = value.len();

    // then
    assert!(length > 0, "{}", '{');
}
"##;

fn dir(name: &str) -> String {
    if name == "orc" {
        format!("tools/{name}")
    } else {
        format!("crates/{name}")
    }
}

fn baseline() -> World {
    let members = vec![
        ("dom", vec![]),
        ("app", vec![dep("dom")]),
        ("ad1", vec![dep("dom"), dep("app"), ext("ext")]),
        ("ad2", vec![dep("dom"), dev("kit")]),
        ("comp", vec![dep("dom"), dep("app"), dep("ad1"), dep("ad2")]),
        ("kit", vec![dep("dom")]),
        ("orc", vec![]),
    ];
    let mut files = BTreeMap::new();
    files.insert("architecture.toml".to_owned(), POLICY.to_owned());
    files.insert("Cargo.toml".to_owned(), ROOT_MANIFEST.to_owned());
    files.insert("clippy.toml".to_owned(), ROOT_CLIPPY.to_owned());
    files.insert("crates/ad1/clippy.toml".to_owned(), AD1_CLIPPY.to_owned());
    files.insert("crates/dom/clippy.toml".to_owned(), DOM_CLIPPY.to_owned());
    files.insert("doc/adr/0007-a-decision.md".to_owned(), "# 7.".to_owned());
    for (name, _) in &members {
        let d = dir(name);
        files.insert(
            format!("{d}/Cargo.toml"),
            format!("[package]\nname = \"{name}\"\npublish = false\n\n[lints]\nworkspace = true\n"),
        );
        let lib = if *name == "dom" {
            "#![no_std]\n//! dom.\n"
        } else {
            "//! A crate.\n"
        };
        files.insert(format!("{d}/src/lib.rs"), lib.to_owned());
    }
    files.insert("crates/kit/tests/good.rs".to_owned(), GOOD_TEST.to_owned());
    World {
        members,
        externals: vec![("ext", vec![])],
        files,
        broken: Vec::new(),
        scratch: Vec::new(),
        package_fields: BTreeMap::new(),
    }
}

fn id(name: &str, external: bool) -> String {
    if external {
        format!("{REGISTRY}#{name}@1.0.0")
    } else {
        format!("path+file:///ws/{}#{name}@0.1.0", dir(name))
    }
}

fn package(name: &str, deps: &[Dep], external: bool) -> Value {
    let manifest = if external {
        format!("/registry/{name}/Cargo.toml")
    } else {
        format!("/ws/{}/Cargo.toml", dir(name))
    };
    let dependencies: Vec<Value> = deps
        .iter()
        .map(|d| {
            json!({
                "name": d.name,
                "source": if d.external { json!(REGISTRY) } else { Value::Null },
                "req": "*",
                "kind": d.kind,
                "optional": false,
                "uses_default_features": true,
                "features": d.features,
                "target": null,
                "rename": null,
                "registry": null,
                "path": if d.external { Value::Null } else { json!(format!("/ws/{}", dir(d.name))) },
            })
        })
        .collect();
    json!({
        "name": name,
        "version": if external { "1.0.0" } else { "0.1.0" },
        "id": id(name, external),
        "source": if external { json!(REGISTRY) } else { Value::Null },
        "dependencies": dependencies,
        "targets": [],
        "features": {},
        "manifest_path": manifest,
        "edition": "2024",
    })
}

fn node(name: &str, deps: &[Dep], external: bool) -> Value {
    let edges: Vec<Value> = deps
        .iter()
        .map(|d| {
            json!({
                "name": d.name,
                "pkg": id(d.name, d.external),
                "dep_kinds": [{ "kind": d.kind, "target": null }],
            })
        })
        .collect();
    let ids: Vec<String> = deps.iter().map(|d| id(d.name, d.external)).collect();
    json!({ "id": id(name, external), "deps": edges, "dependencies": ids, "features": [] })
}

impl World {
    fn metadata_json(&self) -> String {
        let mut packages = Vec::new();
        let mut nodes = Vec::new();
        for (name, deps) in &self.members {
            let mut value = package(name, deps, false);
            if let (Some(fields), Some(object)) =
                (self.package_fields.get(name), value.as_object_mut())
            {
                for (key, field) in fields {
                    object.insert((*key).to_owned(), field.clone());
                }
            }
            packages.push(value);
            nodes.push(node(name, deps, false));
        }
        for (name, deps) in &self.externals {
            packages.push(package(name, deps, true));
            nodes.push(node(name, deps, true));
        }
        let members: Vec<String> = self.members.iter().map(|(n, _)| id(n, false)).collect();
        json!({
            "packages": packages,
            "workspace_members": members,
            "resolve": { "nodes": nodes, "root": null },
            "workspace_root": "/ws",
            "target_directory": "/ws/target",
            "version": 1,
        })
        .to_string()
    }

    fn findings(&self) -> Result<Vec<Finding>, Box<dyn Error>> {
        if !self.broken.is_empty() {
            return Err(self.broken.join("; ").into());
        }
        let metadata = parse_metadata(&self.metadata_json()).map_err(|f| f.to_string())?;
        let mut tree = MemoryTree::new();
        for (path, contents) in &self.files {
            tree = tree.with(path, contents);
        }
        Ok(check(
            &tree,
            &[("x86_64-unknown-linux-gnu".to_owned(), metadata)],
            Scope::ALL,
        ))
    }

    fn edit(&mut self, path: &str, from: &str, to: &str) {
        match self.files.get_mut(path) {
            Some(text) if text.contains(from) => *text = text.replacen(from, to, 1),
            _ => self.broken.push(format!("{path} has no `{from}`")),
        }
    }

    fn field(&mut self, member: &'static str, key: &'static str, value: Value) {
        self.package_fields
            .entry(member)
            .or_default()
            .push((key, value));
    }

    fn put(&mut self, path: &str, contents: &str) {
        self.files.insert(path.to_owned(), contents.to_owned());
    }

    fn deps(&mut self, member: &str) -> &mut Vec<Dep> {
        match self.members.iter_mut().find(|(n, _)| *n == member) {
            Some((_, deps)) => deps,
            None => {
                self.broken.push(format!("no member {member}"));
                &mut self.scratch
            }
        }
    }
}

/// A negative case: what to change, and the rule and path it must report.
struct Case {
    rule: &'static str,
    at: &'static str,
    change: fn(&mut World),
}

const fn case(rule: &'static str, at: &'static str, change: fn(&mut World)) -> Case {
    Case { rule, at, change }
}

const TEST_FILE: &str = "crates/kit/tests/good.rs";

fn cases() -> Vec<Case> {
    vec![
        // The policy and the inventory.
        case("ARCH-policy-schema", "architecture.toml", |w| {
            w.edit(
                "architecture.toml",
                "[roles]",
                "colour = \"blue\"\n\n[roles]",
            );
        }),
        case("ARCH-policy-schema", "architecture.toml", |w| {
            w.edit("architecture.toml", "schema = 1", "schema = 2");
        }),
        case("ARCH-policy-schema", "architecture.toml", |w| {
            w.files.remove("architecture.toml");
        }),
        case("ARCH-empty-selector", "architecture.toml", |w| {
            w.edit(
                "architecture.toml",
                "fs = [\"ad1\"]",
                "fs = [\"ad1\"]\nsleep = []",
            );
        }),
        case("ARCH-duplicate-category", "architecture.toml", |w| {
            w.edit(
                "architecture.toml",
                "test_only = [\"kit\"]",
                "test_only = [\"kit\", \"dom\"]",
            );
        }),
        case("ARCH-duplicate-category", "architecture.toml", |w| {
            w.edit(
                "architecture.toml",
                "root = [\"comp\"]",
                "root = [\"comp\", \"kit\"]",
            );
        }),
        case("ARCH-missing-crate", "architecture.toml", |w| {
            w.edit(
                "architecture.toml",
                "fs = [\"ad1\"]",
                "fs = [\"ad1\", \"ghost\"]",
            );
        }),
        case("ARCH-missing-member", "crates/stray/Cargo.toml", |w| {
            w.members.push(("stray", vec![]));
        }),
        case("ARCH-missing-member", "crates/app/Cargo.toml", |w| {
            w.edit("architecture.toml", "application = [\"app\"]\n", "");
        }),
        case("ARCH-policy-cycle", "architecture.toml", |w| {
            w.edit("architecture.toml", "dom = []", "dom = [\"app\"]");
        }),
        case("ARCH-layer-isolation", "architecture.toml", |w| {
            w.edit(
                "architecture.toml",
                "ad2 = [\"dom\"]",
                "ad2 = [\"dom\", \"ad1\"]",
            );
        }),
        // Edges.
        case("ARCH-forbidden-edge", "crates/dom/Cargo.toml", |w| {
            w.deps("dom").push(dep("app"));
        }),
        case("ARCH-forbidden-dev-edge", "crates/dom/Cargo.toml", |w| {
            w.deps("dom").push(dev("app"));
        }),
        case("ARCH-undeclared-build-dep", "crates/comp/Cargo.toml", |w| {
            w.deps("comp").push(build("dom"));
        }),
        case("ARCH-layer-isolation", "crates/ad2/Cargo.toml", |w| {
            w.deps("ad2").push(dev("ad1"));
        }),
        case("ARCH-production-closure", "crates/ad1/Cargo.toml", |w| {
            // Through a third-party package: ad1 -> ext -> kit.
            w.externals = vec![("ext", vec![dep("kit")])];
        }),
        case("ARCH-production-closure", "crates/app/Cargo.toml", |w| {
            w.deps("app").push(dep("orc"));
        }),
        case("ARCH-independent-oracle", "tools/orc/Cargo.toml", |w| {
            w.deps("orc").push(dev("kit"));
        }),
        case("ARCH-unlisted-external", "crates/ad2/Cargo.toml", |w| {
            w.deps("ad2").push(ext("ext"));
        }),
        case("ARCH-unlisted-external", "crates/ad1/Cargo.toml", |w| {
            // Listed for normal edges only.
            w.deps("ad1").push(Dep {
                kind: Some("dev"),
                ..ext("ext")
            });
        }),
        case("ARCH-external-feature", "crates/ad1/Cargo.toml", |w| {
            let Some(e) = w.deps("ad1").iter_mut().find(|d| d.name == "ext") else {
                return;
            };
            e.features.push("zstd");
        }),
        case("ARCH-metadata", "crates/app/Cargo.toml", |w| {
            w.files.remove("crates/app/Cargo.toml");
        }),
        // Manifests.
        case("ARCH-lint-config", "Cargo.toml", |w| {
            w.edit(
                "Cargo.toml",
                "unwrap_used = \"deny\"",
                "unwrap_used = \"warn\"",
            );
        }),
        case("ARCH-lint-config", "Cargo.toml", |w| {
            w.edit(
                "Cargo.toml",
                "large_futures = \"deny\"",
                "large_futures = \"deny\"\nshadow_reuse = \"allow\"",
            );
        }),
        case("ARCH-lint-config", "crates/app/Cargo.toml", |w| {
            w.edit(
                "crates/app/Cargo.toml",
                "workspace = true",
                "workspace = true\n\n[lints.rust]\nunsafe_code = \"allow\"",
            );
        }),
        case("ARCH-lint-config", "crates/app/Cargo.toml", |w| {
            w.edit("crates/app/Cargo.toml", "publish = false\n", "");
        }),
        case("ARCH-not-a-member", "Cargo.toml", |w| {
            w.edit(
                "Cargo.toml",
                "\"tools/orc\"]",
                "\"tools/orc\", \"extras/*\"]",
            );
        }),
        case("ARCH-not-a-member", "crates/stray/Cargo.toml", |w| {
            w.put("crates/stray/Cargo.toml", "[package]\nname = \"stray\"\n");
        }),
        // The clippy lists.
        case("ARCH-clippy-lists", "crates/ad1/clippy.toml", |w| {
            w.edit(
                "crates/ad1/clippy.toml",
                "  { path = \"std::net::TcpStream\", reason = \"tcp\" },\n",
                "",
            );
        }),
        case("ARCH-clippy-lists", "crates/ad1/clippy.toml", |w| {
            w.edit(
                "crates/ad1/clippy.toml",
                "reason = \"tcp\" }",
                "reason = \"tcp\", allow-invalid = true }",
            );
        }),
        case("ARCH-clippy-lists", "crates/ad1/clippy.toml", |w| {
            w.edit(
                "crates/ad1/clippy.toml",
                "allow-unwrap-in-tests = true",
                "allow-unwrap-in-tests = true\navoid-breaking-exported-api = false",
            );
        }),
        case("ARCH-clippy-lists", "crates/dom/clippy.toml", |w| {
            w.edit(
                "crates/dom/clippy.toml",
                "  { path = \"core::cell::Cell\", reason = \"pure\" },\n",
                "",
            );
        }),
        case("ARCH-clippy-lists", "crates/dom/clippy.toml", |w| {
            w.files.remove("crates/dom/clippy.toml");
        }),
        case("ARCH-clippy-lists", "crates/app/.clippy.toml", |w| {
            w.put("crates/app/.clippy.toml", "disallowed-methods = []\n");
        }),
        case("ARCH-clippy-lists", "crates/app/clippy.toml", |w| {
            w.put(
                "crates/app/clippy.toml",
                "allow-unwrap-in-tests = true\ndisallowed-methods = []\n",
            );
        }),
        case("ARCH-clippy-lists", "clippy.toml:13", |w| {
            w.edit(
                "clippy.toml",
                "reason = \"tcp\" }",
                "reason = \"tcp\", allow-invalid = true }",
            );
        }),
        case("ARCH-clippy-lists", "clippy.toml:5", |w| {
            w.edit("clippy.toml", "# grant: fs", "# grant: fly");
        }),
        // The source scan.
        case("ARCH-test-name", "crates/kit/tests/good.rs:4", |w| {
            w.edit(
                TEST_FILE,
                "fn should_pass_when_nothing_is_wrong",
                "fn passes",
            );
        }),
        case("ARCH-test-name", "crates/kit/tests/good.rs:4", |w| {
            w.edit(
                TEST_FILE,
                "fn should_pass_when_nothing_is_wrong",
                "fn should_pass",
            );
        }),
        case("ARCH-test-name", "crates/app/src/lib.rs:5", |w| {
            w.put(
                "crates/app/src/lib.rs",
                "//! A crate.\n#[cfg(test)]\nmod tests {\n    #[tokio::test(flavor = \"current_thread\")]\n    async fn works() {\n        // given\n        // when\n        // then\n    }\n}\n",
            );
        }),
        case("ARCH-test-name", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\n#[test] fn quick() {\n    // given\n    // when\n    // then\n}\n",
            );
        }),
        case("ARCH-test-structure", "crates/kit/tests/good.rs:4", |w| {
            w.edit(TEST_FILE, "    // when\n", "");
        }),
        case("ARCH-test-structure", "crates/kit/tests/good.rs:4", |w| {
            w.edit(TEST_FILE, "    // then\n", "    // given\n");
        }),
        case("ARCH-ignored-test", "crates/kit/tests/good.rs:3", |w| {
            w.edit(TEST_FILE, "#[test]", "#[test] #[ignore = \"slow\"]");
        }),
        case("ARCH-ignored-test", "crates/kit/tests/good.rs:4", |w| {
            w.edit(TEST_FILE, "#[test]", "#[test]\n#[cfg_attr(unix, ignore)]");
        }),
        case("ARCH-lint-suppression", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\n#![allow(clippy::unwrap_used)]\n",
            );
        }),
        case("ARCH-lint-suppression", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\n#[cfg_attr(test, expect(dead_code))]\nfn f() {}\n",
            );
        }),
        case("ARCH-lint-suppression", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\n#[macro_export]\nmacro_rules! m { () => {} }\n",
            );
        }),
        case("ARCH-lint-suppression", "architecture.toml", |w| {
            // An exception citing an ADR without a file.
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\n#![allow(clippy::unwrap_used)]\n",
            );
            w.edit("architecture.toml", "[exceptions]\n", "[exceptions]\nlints = [{ path = \"crates/app/src/lib.rs\", lint = \"clippy::unwrap_used\", adr = \"0099\" }]\n");
        }),
        case("ARCH-lint-suppression", "architecture.toml", |w| {
            // An exception that matches nothing any more.
            w.edit("architecture.toml", "[exceptions]\n", "[exceptions]\nlints = [{ path = \"crates/app/src/lib.rs\", lint = \"clippy::unwrap_used\", adr = \"0007\" }]\n");
        }),
        case("ARCH-platform-branch", "crates/app/src/lib.rs:2", |w| {
            w.edit("crates/app/src/lib.rs", "//! A crate.\n", "//! A crate.\npub fn sep() -> char { if cfg!(target_os = \"windows\") { '\\\\' } else { '/' } }\n");
        }),
        case("ARCH-platform-branch", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\npub const UNIX: bool = cfg!(unix);\n",
            );
        }),
        case("ARCH-domain-purity", "crates/dom/src/lib.rs:1", |w| {
            w.edit(
                "crates/dom/src/lib.rs",
                "#![no_std]\n//! dom.\n",
                "//! dom.\n#![no_std]\n",
            );
        }),
        // Second round (FND-07's review): each reproduces a bypass the reviewer found.
        case("ARCH-clippy-lists", "crates/clippy.toml", |w| {
            w.put("crates/clippy.toml", "");
        }),
        case("ARCH-clippy-lists", ".clippy.toml", |w| {
            w.put(".clippy.toml", "allow-unwrap-in-tests = true\n");
        }),
        case("ARCH-clippy-lists", "clippy.toml", |w| {
            // A string-form entry the line model would not see.
            w.edit(
                "clippy.toml",
                "  # grant: none\n  { path = \"tokio",
                "  \"std::process::exit\",\n  # grant: none\n  { path = \"tokio",
            );
        }),
        case("ARCH-lint-suppression", "crates/app/src/lib.rs:2", |w| {
            // One row for dead_code does not cover a second lint in the same attribute.
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\n#![allow(dead_code, clippy::unwrap_used)]\n",
            );
            w.edit("architecture.toml", "[exceptions]\n", "[exceptions]\nlints = [{ path = \"crates/app/src/lib.rs\", lint = \"dead_code\", adr = \"0007\" }]\n");
        }),
        case("ARCH-lint-suppression", "crates/app/src/lib.rs:2", |w| {
            // ...nor a lint whose name merely appears later on the line.
            w.edit("crates/app/src/lib.rs", "//! A crate.\n", "//! A crate.\n#[allow(clippy::unwrap_used)] fn dead_code_x() {}\n#[allow(dead_code)] fn y() {}\n");
            w.edit("architecture.toml", "[exceptions]\n", "[exceptions]\nlints = [{ path = \"crates/app/src/lib.rs\", lint = \"dead_code\", adr = \"0007\" }]\n");
        }),
        case("ARCH-lint-suppression", "crates/app/src/lib.rs:2", |w| {
            // ...nor a lint that only starts with the row's name.
            w.edit("crates/app/src/lib.rs", "//! A crate.\n", "//! A crate.\n#[allow(unused_imports)] use core::fmt;\n#[allow(unused)] fn y() {}\n");
            w.edit("architecture.toml", "[exceptions]\n", "[exceptions]\nlints = [{ path = \"crates/app/src/lib.rs\", lint = \"unused\", adr = \"0007\" }]\n");
        }),
        case("ARCH-ignored-test", "crates/kit/tests/good.rs:4", |w| {
            w.edit(
                TEST_FILE,
                "#[test]",
                "#[test]\n#[cfg_attr(\n    unix,\n    ignore\n)]",
            );
        }),
        case("ARCH-ignored-test", "crates/kit/tests/good.rs:4", |w| {
            // `ignore` behind another attribute in the same cfg_attr.
            w.edit(
                TEST_FILE,
                "#[test]",
                "#[test]\n#[cfg_attr(unix, should_panic, ignore)]",
            );
        }),
        case("ARCH-lint-suppression", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\n#[cfg_attr(\n    test,\n    allow(dead_code)\n)]\nfn f() {}\n",
            );
        }),
        case("ARCH-lint-suppression", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\n#\n[allow(dead_code)]\nfn f() {}\n",
            );
        }),
        case("ARCH-platform-branch", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\npub const LINUX: bool = cfg!(\n    target_os = \"linux\"\n);\n",
            );
        }),
        case("ARCH-platform-branch", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\npub const X86: bool = cfg!(target_arch = \"x86_64\");\n",
            );
        }),
        case("ARCH-test-name", "crates/app/src/lib.rs:2", |w| {
            w.edit("crates/app/src/lib.rs", "//! A crate.\n", "//! A crate.\n#[cfg(test)] #[test] fn quick() {\n    // given\n    // when\n    // then\n}\n");
        }),
        case("ARCH-hidden-source", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\n#[path = \"../hidden/h.rs\"]\nmod h;\n",
            );
        }),
        case("ARCH-hidden-source", "crates/app/src/lib.rs:2", |w| {
            w.edit(
                "crates/app/src/lib.rs",
                "//! A crate.\n",
                "//! A crate.\ninclude!(\"../hidden/h.rs\");\n",
            );
        }),
        case(
            "ARCH-lint-suppression",
            "crates/app/src/target/mod.rs:1",
            |w| {
                // A module directory named `target` is source, not build output.
                w.put(
                    "crates/app/src/target/mod.rs",
                    "#[allow(dead_code)]\nfn f() {}\n",
                );
            },
        ),
        case("ARCH-ignored-test", "crates/app/src/inner/mod.rs:1", |w| {
            // A stray Cargo.toml inside a crate hides nothing below it.
            w.put(
                "crates/app/src/inner/Cargo.toml",
                "[package]\nname = \"inner\"\n",
            );
            w.put("crates/app/src/inner/mod.rs", "#[ignore]\nfn f() {}\n");
        }),
        case("ARCH-ignored-test", "crates/app/hidden/h.rs:1", |w| {
            // A file outside src/ and tests/ is scanned too.
            w.put("crates/app/hidden/h.rs", "#[ignore]\nfn f() {}\n");
        }),
        case("ARCH-hidden-source", "crates/app/Cargo.toml", |w| {
            w.field(
                "app",
                "targets",
                json!([{
                    "name": "app", "kind": ["lib"], "crate_types": ["lib"],
                    "src_path": "/ws/elsewhere/lib.rs", "edition": "2024",
                    "doctest": true, "test": true, "doc": true,
                }]),
            );
        }),
        case("ARCH-forbidden-edge", "architecture.toml", |w| {
            // The application on an adapter, though the graph has no such edge yet.
            w.edit(
                "architecture.toml",
                "app = [\"dom\"]",
                "app = [\"dom\", \"ad1\"]",
            );
        }),
        case("ARCH-forbidden-edge", "architecture.toml", |w| {
            w.edit("architecture.toml", "dom = []", "dom = [\"kit\"]");
        }),
        case("ARCH-forbidden-edge", "architecture.toml", |w| {
            // The reviewer's probe: a pure crate on the application.
            w.edit("architecture.toml", "dom = []", "dom = [\"app\"]");
            w.edit("architecture.toml", "app = [\"dom\"]", "app = []");
        }),
        case("ARCH-forbidden-edge", "architecture.toml", |w| {
            // The reviewer's probe: the support crate (jfl-process) naming a workspace crate.
            w.edit(
                "architecture.toml",
                "adapter = [\"ad1\", \"ad2\"]",
                "adapter = [\"ad1\"]\nsupport = [\"ad2\"]",
            );
        }),
        case("ARCH-forbidden-edge", "architecture.toml", |w| {
            w.edit("architecture.toml", "orc = []", "orc = [\"dom\"]");
        }),
        case("ARCH-production-closure", "crates/comp/Cargo.toml", |w| {
            // A build edge counts (fnd §2.2 rule 4), declared or not.
            w.edit(
                "architecture.toml",
                "[dependencies.build]\n",
                "[dependencies.build]\ncomp = [\"kit\"]\n",
            );
            w.deps("comp").push(build("kit"));
        }),
        case("ARCH-missing-member", "tools/orc/Cargo.toml", |w| {
            w.edit("architecture.toml", "orc = []\n", "");
        }),
        case("ARCH-layer-isolation", "crates/ad1/Cargo.toml", |w| {
            w.deps("ad1").push(dev("comp"));
        }),
        case("ARCH-duplicate-category", "architecture.toml", |w| {
            w.edit(
                "architecture.toml",
                "test_only = [\"kit\"]",
                "test_only = [\"kit\", \"kit\"]",
            );
        }),
        case("ARCH-external-feature", "crates/ad1/Cargo.toml", |w| {
            w.field("ad1", "features", json!({ "default": ["ext/zstd"] }));
        }),
        case("ARCH-external-feature", "crates/ad1/Cargo.toml", |w| {
            w.field("ad1", "features", json!({ "fast": ["ext?/simd"] }));
        }),
    ]
}

#[test]
fn should_find_nothing_when_the_synthetic_workspace_is_clean() -> TestResult {
    // given
    let world = baseline();

    // when
    let findings = world.findings()?;

    // then
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    Ok(())
}

#[test]
fn should_report_each_rule_with_its_id_and_path_when_its_negative_case_runs() -> TestResult {
    // given
    let cases = cases();
    let mut missed = Vec::new();

    // when
    for (index, case) in cases.iter().enumerate() {
        let mut world = baseline();
        (case.change)(&mut world);
        let findings = world.findings()?;
        let hit = findings.iter().any(|f| {
            f.rule == case.rule && f.at == case.at && f.to_string().starts_with(case.rule)
        });
        if !hit {
            let got: Vec<String> = findings.iter().map(ToString::to_string).collect();
            missed.push(format!(
                "case {index} ({} at {}): got\n  {}",
                case.rule,
                case.at,
                got.join("\n  ")
            ));
        }
    }

    // then
    assert!(missed.is_empty(), "{}", missed.join("\n"));
    Ok(())
}

#[test]
fn should_find_nothing_when_an_exception_backs_a_suppression() -> TestResult {
    // given
    let mut world = baseline();
    world.edit(
        "crates/app/src/lib.rs",
        "//! A crate.\n",
        "//! A crate.\n#![allow(clippy::unwrap_used)]\n",
    );
    world.edit(
        "architecture.toml",
        "[exceptions]\n",
        "[exceptions]\nlints = [{ path = \"crates/app/src/lib.rs\", lint = \"clippy::unwrap_used\", adr = \"0007\" }]\n",
    );

    // when
    let findings = world.findings()?;

    // then
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    Ok(())
}

#[test]
fn should_find_nothing_when_rule_words_sit_in_literals_and_comments() -> TestResult {
    // given: look-alikes the line scan must not trip on
    let mut world = baseline();
    world.put(
        "crates/app/src/lib.rs",
        "//! A crate. #[ignore] #[allow(x)] cfg!(unix)\n/* #[allow(y)]\n /* nested */ #[ignore] */\npub const A: &str = r##\"#[allow(z)] \"# cfg!(windows)\"##;\npub const B: char = '\"';\npub fn f<'a>(s: &'a str) -> &'a str { s } // #[test]\n",
    );

    // when
    let findings = world.findings()?;

    // then
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    Ok(())
}

#[test]
fn should_name_the_targets_when_a_finding_holds_on_only_some() -> TestResult {
    // given: the edge exists on one target's graph only
    let clean = parse_metadata(&baseline().metadata_json()).map_err(|f| f.to_string())?;
    let mut steered = baseline();
    steered.deps("dom").push(dep("app"));
    let dirty = parse_metadata(&steered.metadata_json()).map_err(|f| f.to_string())?;
    let mut tree = MemoryTree::new();
    for (path, contents) in &baseline().files {
        tree = tree.with(path, contents);
    }
    let metadata = [
        ("x86_64-unknown-linux-gnu".to_owned(), clean),
        ("aarch64-apple-darwin".to_owned(), dirty),
    ];

    // when
    let findings = check(&tree, &metadata, Scope::GRAPH);

    // then
    assert!(
        findings
            .iter()
            .any(|f| f.rule == "ARCH-forbidden-edge"
                && f.message.ends_with("(on aarch64-apple-darwin)")),
        "{findings:?}"
    );
    Ok(())
}

#[test]
fn should_report_arch_metadata_when_the_metadata_does_not_parse() {
    // given
    let json = "{\"packages\": [";

    // when
    let result = parse_metadata(json);

    // then
    assert!(matches!(result, Err(f) if f.rule == "ARCH-metadata"));
}

#[test]
fn should_report_arch_metadata_when_cargo_metadata_fails() -> TestResult {
    // given: a directory with no workspace at all
    let empty = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rules-no-workspace");
    std::fs::create_dir_all(&empty)?;

    // when
    let findings = check_repository(&empty, Scope::ALL);

    // then
    assert!(
        findings
            .iter()
            .any(|f| f.rule == "ARCH-metadata" && f.at == "Cargo.toml"),
        "{findings:?}"
    );
    Ok(())
}

/// The negative cases that live outside the table: (rule, test function) in
/// `tests/fixture_workspaces.rs`, `tests/ci_lockstep.rs` and this file. The meta-test checks only that each function
/// exists; that it asserts its rule is the function's own business (and review's).
const OTHER_CASES: [(&str, &str); 8] = [
    (
        "ARCH-forbidden-edge",
        "should_fail_forbidden_edge_when_the_domain_depends_on_the_application",
    ),
    (
        "ARCH-layer-isolation",
        "should_fail_layer_isolation_when_an_adapter_depends_on_an_adapter",
    ),
    (
        "ARCH-production-closure",
        "should_fail_production_closure_when_the_testkit_is_reachable_from_production",
    ),
    (
        "ARCH-independent-oracle",
        "should_fail_independent_oracle_when_the_oracle_imports_the_domain",
    ),
    (
        "ARCH-unlisted-external",
        "should_fail_unlisted_external_when_a_crate_names_an_unlisted_crate",
    ),
    (
        "ARCH-domain-purity",
        "should_fail_to_compile_when_a_no_std_crate_names_std_fs",
    ),
    (
        "ARCH-metadata",
        "should_report_arch_metadata_when_cargo_metadata_fails",
    ),
    (
        "ARCH-ci-lockstep",
        "should_fail_lockstep_when_a_stage_runs_in_two_jobs",
    ),
];

/// The rule ids of fnd §3.3's rows whose Task column names FND-07 or FND-05 (the tasks that put
/// rules in this crate).
fn fnd_rules() -> Result<BTreeSet<String>, Box<dyn Error>> {
    let fnd = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../doc/foundation.md"),
    )?;
    let section = fnd
        .split("\n### 3.3 Rules")
        .nth(1)
        .and_then(|rest| rest.split("\n### ").next())
        .ok_or("fnd §3.3 not found")?;
    let mut rules = BTreeSet::new();
    for row in section.lines().filter(|l| l.starts_with("| `ARCH-")) {
        let cells: Vec<&str> = row
            .split('|')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        let (Some(ids), Some(task)) = (cells.first(), cells.last()) else {
            continue;
        };
        if !task.contains("FND-07") && !task.contains("FND-05") {
            continue;
        }
        for id in ids.split('`').filter(|part| part.starts_with("ARCH-")) {
            rules.insert(id.to_owned());
        }
    }
    Ok(rules)
}

#[test]
fn should_cover_every_rule_of_fnd_3_3_when_the_negative_cases_are_counted() -> TestResult {
    // given
    let fnd = fnd_rules()?;
    let fixture_source = include_str!("fixture_workspaces.rs");
    let ci_source = include_str!("ci_lockstep.rs");
    let this_source = include_str!("rules.rs");
    let mut covered: BTreeSet<String> = cases().iter().map(|c| c.rule.to_owned()).collect();
    for (rule, test) in OTHER_CASES {
        let declared = format!("fn {test}(");
        assert!(
            fixture_source.contains(&declared)
                || ci_source.contains(&declared)
                || this_source.contains(&declared),
            "{test} is listed for {rule} but does not exist"
        );
        covered.insert(rule.to_owned());
    }
    let reported: BTreeSet<String> = RULES.iter().map(|r| (*r).to_owned()).collect();

    // when
    let uncovered: Vec<&String> = fnd.difference(&covered).collect();
    let undocumented: Vec<&String> = reported.difference(&fnd).collect();
    let unreported: Vec<&String> = fnd.difference(&reported).collect();

    // then
    assert!(!fnd.is_empty(), "fnd §3.3 names no FND-07 or FND-05 rule");
    assert!(
        uncovered.is_empty(),
        "rules without a negative case: {uncovered:?}"
    );
    assert!(
        undocumented.is_empty(),
        "rules the test reports but fnd §3.3 does not assign to FND-07 or FND-05: {undocumented:?}"
    );
    assert!(
        unreported.is_empty(),
        "rules fnd §3.3 assigns to FND-07 or FND-05 that the test never reports: {unreported:?}"
    );
    Ok(())
}

#[test]
fn should_fail_the_meta_test_when_a_rule_has_no_negative_case() -> TestResult {
    // given: the table without its ARCH-platform-branch cases
    let fnd = fnd_rules()?;
    let covered: BTreeSet<String> = cases()
        .iter()
        .filter(|c| c.rule != "ARCH-platform-branch")
        .map(|c| c.rule.to_owned())
        .chain(OTHER_CASES.iter().map(|(rule, _)| (*rule).to_owned()))
        .collect();

    // when
    let uncovered: Vec<&String> = fnd.difference(&covered).collect();

    // then
    assert_eq!(uncovered, vec!["ARCH-platform-branch"]);
    Ok(())
}

fn repo_file(path: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path),
    )?)
}

/// The rows of the markdown table in the section headed `heading` of `text`, as cells.
fn table_rows(text: &str, heading: &str) -> Result<Vec<Vec<String>>, Box<dyn Error>> {
    let section = text
        .split(heading)
        .nth(1)
        .and_then(|rest| rest.split("\n### ").next())
        .ok_or_else(|| format!("{heading} not found"))?;
    Ok(section
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("| ") && !l.starts_with("|:"))
        .map(|row| {
            row.split('|')
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .collect())
}

fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

#[test]
fn should_match_plan_6_3_when_the_edge_table_is_read() -> TestResult {
    // given
    let policy = jfl_architecture::policy::parse(&repo_file("architecture.toml")?)
        .map_err(|f| f.to_string())?;
    let rows = table_rows(&repo_file("doc/plan.md")?, "\n### 6.3 Dependency law")?;
    let production: BTreeSet<String> = policy
        .categories
        .iter()
        .filter(|(c, _)| *c == "production")
        .flat_map(|(_, crates)| crates.iter().cloned())
        .collect();
    let mut planned: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in &rows {
        let (Some(from), Some(to)) = (row.first(), row.get(1)) else {
            continue;
        };
        for crate_name in backticked(from) {
            // "— (reads `cargo metadata`; ...)": nothing.
            let mut deps: BTreeSet<String> = if to.starts_with('—') {
                BTreeSet::new()
            } else {
                backticked(to).into_iter().collect()
            };
            if let Some(except) = to.strip_prefix("every production crate except") {
                let excluded = backticked(except);
                deps = production
                    .iter()
                    .filter(|c| **c != crate_name && !excluded.contains(c))
                    .cloned()
                    .collect();
            }
            if to.contains("the pure and application crates") {
                deps.extend(policy.role("pure").iter().cloned());
                deps.extend(policy.role("application").iter().cloned());
            }
            planned.insert(crate_name, deps);
        }
    }

    // when
    let table: BTreeMap<String, BTreeSet<String>> = policy
        .normal
        .iter()
        .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
        .collect();

    // then
    assert!(!planned.is_empty(), "plan §6.3 has no rows");
    assert_eq!(
        table, planned,
        "[dependencies.normal] differs from plan §6.3"
    );
    Ok(())
}

#[test]
fn should_match_fnd_2_4_when_the_grants_are_read() -> TestResult {
    // given
    let policy = jfl_architecture::policy::parse(&repo_file("architecture.toml")?)
        .map_err(|f| f.to_string())?;
    let rows = table_rows(&repo_file("doc/foundation.md")?, "\n### 2.4 Authority")?;
    let mut documented: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in &rows {
        let (Some(crates), Some(grants)) = (row.first(), row.get(1)) else {
            continue;
        };
        let grants: BTreeSet<String> = grants
            .split(',')
            .map(|g| g.trim().to_owned())
            .filter(|g| !g.is_empty())
            .collect();
        for name in backticked(crates) {
            let name = match name.strip_prefix('-') {
                Some(short) => format!("jfl-adapter-{short}"),
                None => name,
            };
            documented.insert(name, grants.clone());
        }
    }

    // when
    let mut granted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (grant, crates) in &policy.grants {
        for name in crates {
            granted
                .entry(name.clone())
                .or_default()
                .insert((*grant).to_owned());
        }
    }

    // then
    assert!(!documented.is_empty(), "fnd §2.4 has no grant rows");
    assert_eq!(
        granted, documented,
        "[grants] differs from fnd §2.4's table"
    );
    Ok(())
}

#[test]
fn should_report_arch_metadata_when_the_walk_meets_a_symlinked_directory() -> TestResult {
    // given
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rules-symlinked-dir");
    if root.try_exists()? {
        std::fs::remove_dir_all(&root)?;
    }
    std::fs::create_dir_all(root.join("real"))?;
    std::fs::write(root.join("real/a.rs"), "")?;
    std::os::unix::fs::symlink(root.join("real"), root.join("linked"))?;
    let tree = jfl_architecture::tree::DiskTree::new(&root);

    // when
    let files = jfl_architecture::tree::Tree::files_under(&tree, "");
    let problems = jfl_architecture::tree::Tree::problems(&tree);

    // then
    assert_eq!(files, vec!["real/a.rs".to_owned()]);
    assert!(
        problems.iter().any(|p| p.starts_with("linked")),
        "{problems:?}"
    );
    Ok(())
}

#[test]
fn should_walk_a_nested_target_directory_when_only_the_root_one_is_build_output() -> TestResult {
    // given
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rules-nested-target");
    if root.try_exists()? {
        std::fs::remove_dir_all(&root)?;
    }
    for dir in ["target/debug", ".work/x", "crates/a/src/target", "out"] {
        std::fs::create_dir_all(root.join(dir))?;
    }
    for file in [
        "target/debug/build.rs",
        ".work/x/notes.rs",
        "crates/a/src/target/mod.rs",
        "out/built.rs",
    ] {
        std::fs::write(root.join(file), "")?;
    }
    let tree = jfl_architecture::tree::DiskTree::new(&root).skipping(&root.join("out"));

    // when
    let files = jfl_architecture::tree::Tree::files_under(&tree, "");

    // then
    assert_eq!(files, vec!["crates/a/src/target/mod.rs".to_owned()]);
    Ok(())
}
