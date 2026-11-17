//! The implicit configuration of the gate's tools, checked before any of them runs (FND-09, fnd
//! §3, §3.6): mise, shellcheck, actionlint, pinact and the like read files from the repository
//! that no command line names, and mise even runs code from its configuration (`[env] _.source`,
//! `{{exec(...)}}` templates) as soon as it loads a trusted file. So this check reads those files
//! itself and nothing calls mise or cargo on a tree it has not passed: `scripts/xtask.sh
//! tool-config` from the justfile's top-level lines (before `mise bin-paths`), from
//! `scripts/land.sh` (the landing checkout's own xtask, on each cherry-picked tree, before a
//! Cargo.lock resolution) and from `scripts/bootstrap-tools.sh` (before `mise install`); and the
//! preflight of every gate command. XT-01 ported it from `scripts/tool-config-check.sh` (bash and
//! awk), keeping its patterns. Every check is an allowlist or a pinned shape (fnd §3):
//!
//! - ROOT/mise.toml: only `[tools]`, each key `just`, `node`, `java` or a quoted
//!   `aqua:<owner>/<repo>[/<name>]`, `github:<owner>/<repo>` or `npm:@<scope>/<name>` (the
//!   backends that install a checksummed release asset, or an npm package whose whole graph is
//!   locked; no plugin backend, whose plugin is code), each value a plain version string;
//! - ROOT/mise.lock: only `lockfile_version = 3`, `[[tools.<key>]]` with `version`, `backend`
//!   (aqua, github, `core:node`, `core:java` or a scoped npm package), `specifiers` and an npm
//!   tool's `aube` (its lock directory under `.mise/locks/` and the digest mise checks before it
//!   installs), and per-platform tables of linux-x64, linux-arm64 or macos-arm64 with a `sha256:`
//!   checksum, a `https://github.com/` url (a `+` written `%2B` allowed: Temurin's tags) or a
//!   `https://nodejs.org/dist/` archive of Node, a `https://api.github.com/repos/` url_api and an
//!   optional `github-attestations` provenance (FORMAL-01);
//! - ROOT/.mise holds only `locks/npm-<name>/<version>/` with exactly `package.json` (mise's own
//!   one-dependency manifest, no `scripts`) and `aube-lock.yaml` (the npm graph: every package
//!   pinned by version and `sha512` integrity, never a tarball URL, a git or a file source); mise
//!   reads a `.mise/config.toml` or `.mise/conf.d/`, which this refuses (FORMAL-01);
//! - no cargo configuration file (`.cargo/config`, `.cargo/config.toml`, in any letter case) at any
//!   depth of ROOT, and no toolchain file but ROOT/rust-toolchain.toml, which holds only
//!   `[toolchain]` with plain `channel`, `profile`, `components` and `targets` (a `path` swaps the
//!   compiler; the FND-09 review's G2 and H6);
//! - the entries of ROOT whose names start with a dot or with `mise` are an allowlist, and so are
//!   the entries of ROOT/.github and ROOT/.config.
//!
//! The check reads the file system, so an untracked or ignored file counts as much as a committed
//! one (inside a git work tree: tracked and untracked files that are not ignored; elsewhere: every
//! file). ARCH-gate-config repeats these checks with a TOML parser.
//!
//! Pinned by: tools/xtask/tests/tool_config.rs (every case of the removed
//! `scripts/test-tool-config.sh`, the bootstrap's included) and tools/xtask/tests/preflight.rs.

use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

/// The entries allowed at the root among the dot-names and the mise names. A new one is added
/// here, in the commit that needs it, with ARCH-gate-config's `ROOT_ALLOWED`.
pub(crate) const ROOT_ALLOWED: [&str; 15] = [
    ".claude",
    ".config",
    ".DS_Store",
    ".editorconfig",
    ".git",
    ".gitattributes",
    ".github",
    ".gitignore",
    ".gitleaks.toml",
    ".idea",
    ".mise",
    ".vscode",
    ".work",
    "mise.lock",
    "mise.toml",
];

/// The entries allowed under `.github/` (ARCH-gate-config's `GITHUB_ALLOWED`).
pub(crate) const GITHUB_ALLOWED: [&str; 7] = [
    "actions",
    "CODEOWNERS",
    "dependabot.yml",
    "ISSUE_TEMPLATE",
    "pull_request_template.md",
    "workflows",
    "zizmor.yml",
];

/// The entries allowed under `.config/` (ARCH-gate-config's `CONFIG_ALLOWED`).
pub(crate) const CONFIG_ALLOWED: [&str; 2] = ["gate.sha256", "nextest.toml"];

/// A tool key of mise.toml or mise.lock: `just`, `node`, `java`, or a quoted aqua, github or
/// scoped npm identifier.
const KEY: &str = r#"(just|node|java|"aqua:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)?"|"github:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+"|"npm:@[a-z0-9._-]+/[a-z0-9._-]+")"#;
/// A plain version string.
const VERSION: &str = r#""[A-Za-z0-9._+-]+""#;
/// The end of an accepted line: spaces and an optional comment.
const TAIL: &str = r"[[:space:]]*(#.*)?$";
/// A line that is blank or a comment.
const BLANK: &str = r"^[[:space:]]*(#.*)?$";
/// Node's release archive for a platform of ADR 0006 (`nodejs.org/dist`, which mise's core node
/// backend downloads; FORMAL-01).
const NODE_URL: &str = r"https://nodejs\.org/dist/v[0-9]+\.[0-9]+\.[0-9]+/node-v[0-9]+\.[0-9]+\.[0-9]+-(linux-x64|linux-arm64|darwin-arm64)\.tar\.(gz|xz)";
/// An npm package name, scoped or not.
const NPM_NAME: &str = r"(@[a-z0-9._-]+/)?[a-z0-9._-]+";
/// An exact npm version.
const NPM_VERSION: &str = r"[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.]+)?";

/// Every refusal for the tree at `root`, one message each, empty when the tree passes.
pub(crate) fn check(root: &Path) -> Vec<String> {
    let at = |rest: &str| format!("{}/{rest}", root.display());
    let mut refused = Vec::new();
    for name in entries(root) {
        if (name.starts_with('.') || name.starts_with("mise"))
            && !ROOT_ALLOWED.contains(&name.as_str())
        {
            refused.push(format!(
                "{}: not an allowed entry at the root (dotfiles and mise* names are an allowlist)",
                at(&name)
            ));
        }
    }
    for name in entries(&root.join(".github")) {
        if !GITHUB_ALLOWED.contains(&name.as_str()) {
            refused.push(format!(
                "{}: not an allowed entry of .github/",
                at(&format!(".github/{name}"))
            ));
        }
    }
    for name in entries(&root.join(".config")) {
        if !CONFIG_ALLOWED.contains(&name.as_str()) {
            refused.push(format!(
                "{}: not an allowed entry of .config/",
                at(&format!(".config/{name}"))
            ));
        }
    }
    let files = listing(root).unwrap_or_else(|why| {
        refused.push(format!(
            "{}: {why}, so its cargo and toolchain files are unknown",
            root.display()
        ));
        Vec::new()
    });
    for file in files {
        let lower = format!("/{}", file.to_ascii_lowercase());
        if lower.ends_with("/.cargo/config") || lower.ends_with("/.cargo/config.toml") {
            refused.push(format!(
                "{}: a cargo configuration file; none may exist in the repository (it can set a rustc-wrapper or a runner that any cargo command runs)",
                at(&file)
            ));
        } else if (lower.ends_with("/rust-toolchain") || lower.ends_with("/rust-toolchain.toml"))
            && file != "rust-toolchain.toml"
        {
            refused.push(format!(
                "{}: a toolchain file; only the root rust-toolchain.toml may exist",
                at(&file)
            ));
        }
    }
    shaped(
        &root.join("rust-toolchain.toml"),
        toolchain_shape,
        "outside the shape (only [toolchain] with plain channel, profile, components and targets)",
        &mut refused,
    );
    shaped(
        &root.join("mise.toml"),
        mise_toml_shape,
        "outside the shape (only [tools], each tool `just` or an aqua or github backend, pinned by a plain version)",
        &mut refused,
    );
    shaped(
        &root.join("mise.lock"),
        mise_lock_shape,
        "outside the shape (checksummed GitHub release assets of aqua or github tools, Node's archives, and locked npm graphs only)",
        &mut refused,
    );
    mise_locks(root, &mut refused);
    refused
}

/// `ROOT/.mise`, when it exists: only `locks/npm-<name>/<version>/` holding exactly
/// `package.json` and `aube-lock.yaml`, each a regular file in its shape. Anything else (a
/// `config.toml`, a `conf.d/`, a symlink, a third file) is refused, since mise reads
/// configuration from `.mise/` and installs an npm tool from its lock directory.
fn mise_locks(root: &Path, refused: &mut Vec<String>) {
    let dir = root.join(".mise");
    let Ok(meta) = std::fs::symlink_metadata(&dir) else {
        return;
    };
    let bad = |refused: &mut Vec<String>, path: &Path, why: &str| {
        refused.push(format!("{}: {why}", path.display()));
    };
    if !meta.is_dir() {
        bad(refused, &dir, "not a directory");
        return;
    }
    let (Some(npm), Some(version)) = (
        regex(r"^npm-[a-z0-9._-]+$"),
        regex(r"^[0-9][A-Za-z0-9._+-]*$"),
    ) else {
        bad(refused, &dir, "(the shape's patterns do not compile)");
        return;
    };
    for name in entries(&dir) {
        if name != "locks" {
            bad(
                refused,
                &dir.join(&name),
                "not an allowed entry of .mise/ (only locks/)",
            );
        }
    }
    let locks = dir.join("locks");
    for tool in entries(&locks) {
        let tool_dir = locks.join(&tool);
        if !npm.is_match(&tool) || !is_real_dir(&tool_dir) {
            bad(
                refused,
                &tool_dir,
                "not an npm-<name> directory of .mise/locks/",
            );
            continue;
        }
        for ver in entries(&tool_dir) {
            let ver_dir = tool_dir.join(&ver);
            if !version.is_match(&ver) || !is_real_dir(&ver_dir) {
                bad(
                    refused,
                    &ver_dir,
                    "not a <version> directory of .mise/locks/npm-<name>/",
                );
                continue;
            }
            for file in entries(&ver_dir) {
                if file != "package.json" && file != "aube-lock.yaml" {
                    bad(
                        refused,
                        &ver_dir.join(&file),
                        "not package.json or aube-lock.yaml (an npm lock directory holds those two)",
                    );
                }
            }
            shaped(
                &ver_dir.join("package.json"),
                npm_manifest_shape,
                "outside the shape (mise's manifest: one exact dependency, no scripts)",
                refused,
            );
            shaped(
                &ver_dir.join("aube-lock.yaml"),
                aube_lock_shape,
                "outside the shape (packages by exact version and sha512 integrity only)",
                refused,
            );
        }
    }
}

fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// `<number>: <line>` of the first line of `text` outside the `package.json` mise writes for an
/// npm tool: its fixed name, `private`, and one dependency at an exact version, nothing else (no
/// `scripts`, no other source).
pub(crate) fn npm_manifest_shape(text: &str) -> Option<String> {
    let Some(dependency) = regex(&format!(r#"^    "{NPM_NAME}": "{NPM_VERSION}"$"#)) else {
        return Some("0: (the shape's patterns do not compile)".to_owned());
    };
    let fixed = [
        "{",
        r#"  "name": "mise-npm-install","#,
        r#"  "private": true,"#,
        r#"  "dependencies": {"#,
        "",
        "  }",
        "}",
    ];
    let body: Vec<(usize, &str)> = numbered(text).collect();
    if body.len() != fixed.len() {
        let (number, line) = body.last().copied().unwrap_or((0, ""));
        return Some(format!(
            "{number}: {line} (not the 7 lines of mise's manifest)"
        ));
    }
    for (&(number, line), want) in body.iter().zip(fixed) {
        let ok = if want.is_empty() {
            dependency.is_match(line)
        } else {
            line == want
        };
        if !ok {
            return Some(format!("{number}: {line}"));
        }
    }
    None
}

/// `<number>: <line>` of the first line of `text` outside the shape of an `aube-lock.yaml` (pnpm's
/// lockfile 9.0 as mise writes it): the settings, the importer's dependencies, and per package its
/// `resolution: {integrity: sha512-...}`, `engines` and `hasBin`, and its snapshot's exact
/// dependencies. A `tarball`, `directory`, `git` or `file:` source is outside it, so every
/// package comes from the registry by version and is checked against its integrity.
pub(crate) fn aube_lock_shape(text: &str) -> Option<String> {
    let name = format!("('{NPM_NAME}'|{NPM_NAME})");
    let package = format!("('{NPM_NAME}@{NPM_VERSION}'|{NPM_NAME}@{NPM_VERSION})");
    let patterns = [
        r"^$".to_owned(),
        r"^lockfileVersion: '9\.0'$".to_owned(),
        r"^(settings|importers|packages|snapshots):$".to_owned(),
        r"^  (autoInstallPeers|excludeLinksFromLockfile): (true|false)$".to_owned(),
        r"^  \.:$".to_owned(),
        r"^    (dependencies|optionalDependencies):$".to_owned(),
        format!(r"^      {name}:$"),
        format!(r"^        (specifier|version): {NPM_VERSION}$"),
        format!(r"^  {package}:( \{{\}})?$"),
        r"^    resolution: \{integrity: sha512-[A-Za-z0-9+/]+={0,2}\}$".to_owned(),
        r"^    engines: \{node: [^{}]*\}$".to_owned(),
        r"^    hasBin: true$".to_owned(),
        format!(r"^      {name}: {NPM_VERSION}$"),
    ];
    let compiled: Vec<Regex> = patterns.iter().filter_map(|p| regex(p)).collect();
    if compiled.len() != patterns.len() {
        return Some("0: (the shape's patterns do not compile)".to_owned());
    }
    numbered(text)
        .find(|(_, line)| !compiled.iter().any(|r| r.is_match(line)))
        .map(|(number, line)| format!("{number}: {line}"))
}

/// Checks `file` against `shape` when it exists: a symlink or a non-file is refused outright; a
/// regular file outside the shape is refused naming its first bad line (`<number>: <text>`).
fn shaped(file: &Path, shape: fn(&str) -> Option<String>, why: &str, refused: &mut Vec<String>) {
    let Ok(meta) = std::fs::symlink_metadata(file) else {
        return;
    };
    if !meta.is_file() {
        refused.push(format!("{}: not a regular file", file.display()));
        return;
    }
    match std::fs::read(file) {
        Ok(bytes) => {
            if let Some(line) = shape(&String::from_utf8_lossy(&bytes)) {
                refused.push(format!("{}:{line}: {why}", file.display()));
            }
        }
        Err(err) => refused.push(format!("{}: cannot be read: {err}", file.display())),
    }
}

/// The names in `dir` (dot-names included), sorted; nothing when it is not a directory.
fn entries(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The files of the tree, relative to `root`, sorted and unique: inside a git work tree the
/// tracked and untracked files that are not ignored; elsewhere every regular file outside the
/// top-level `.git`. A git listing that fails is an error, so the check fails closed.
fn listing(root: &Path) -> Result<Vec<String>, String> {
    let inside = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|o| o.status.success() && o.stdout.starts_with(b"true"));
    let mut files = if inside {
        match Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ])
            .output()
        {
            Ok(out) if out.status.success() => out
                .stdout
                .split(|b| *b == 0)
                .filter(|f| !f.is_empty())
                .map(|f| String::from_utf8_lossy(f).into_owned())
                .collect(),
            _ => return Err("`git ls-files` failed".to_owned()),
        }
    } else {
        let mut found = Vec::new();
        walk(root, root, &mut found);
        found
    };
    files.sort();
    files.dedup();
    Ok(files)
}

fn walk(root: &Path, dir: &Path, found: &mut Vec<String>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.filter_map(Result::ok) {
        let path: PathBuf = entry.path();
        if dir == root && entry.file_name() == ".git" {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(root, &path, found);
        } else if kind.is_file()
            && let Ok(rel) = path.strip_prefix(root)
        {
            found.push(rel.to_string_lossy().into_owned());
        }
    }
}

fn regex(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

/// `<number>: <line>` of the first line of `text` outside rust-toolchain.toml's shape.
pub(crate) fn toolchain_shape(text: &str) -> Option<String> {
    let (Some(blank), Some(header), Some(plain), Some(list)) = (
        regex(BLANK),
        regex(r"^\[toolchain\][[:space:]]*(#.*)?$"),
        regex(r#"^(channel|profile)[[:space:]]*=[[:space:]]*"[A-Za-z0-9._-]+"[[:space:]]*(#.*)?$"#),
        regex(
            r#"^(components|targets)[[:space:]]*=[[:space:]]*\[[[:space:]]*("[A-Za-z0-9._-]+"[[:space:]]*,[[:space:]]*)*("[A-Za-z0-9._-]+")?[[:space:]]*\][[:space:]]*(#.*)?$"#,
        ),
    ) else {
        return Some("0: (the shape's patterns do not compile)".to_owned());
    };
    let mut seen = false;
    for (number, line) in numbered(text) {
        if blank.is_match(line) {
            continue;
        }
        if header.is_match(line) && !seen {
            seen = true;
            continue;
        }
        if seen && (plain.is_match(line) || list.is_match(line)) {
            continue;
        }
        return Some(format!("{number}: {line}"));
    }
    None
}

/// `<number>: <line>` of the first line of `text` outside mise.toml's shape.
pub(crate) fn mise_toml_shape(text: &str) -> Option<String> {
    let (Some(blank), Some(header), Some(tool)) = (
        regex(BLANK),
        regex(&format!(r"^\[tools\]{TAIL}")),
        regex(&format!(r"^{KEY}[[:space:]]*=[[:space:]]*{VERSION}{TAIL}")),
    ) else {
        return Some("0: (the shape's patterns do not compile)".to_owned());
    };
    let mut tools = false;
    for (number, line) in numbered(text) {
        if blank.is_match(line) {
            continue;
        }
        if header.is_match(line) {
            tools = true;
            continue;
        }
        if tools && tool.is_match(line) {
            continue;
        }
        return Some(format!("{number}: {line}"));
    }
    None
}

/// `<number>: <line>` (or the reason) of the first line of `text` outside mise.lock's shape.
pub(crate) fn mise_lock_shape(text: &str) -> Option<String> {
    let backend = r"(aqua:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)?|github:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+|core:node|core:java|npm:@[a-z0-9._-]+/[a-z0-9._-]+)";
    let patterns = [
        BLANK.to_owned(),
        format!("^lockfile_version = 3{TAIL}"),
        format!(r"^\[\[tools\.{KEY}\]\]{TAIL}"),
        format!(r#"^\[tools\.{KEY}\."platforms\.(linux-x64|linux-arm64|macos-arm64)"\]{TAIL}"#),
        format!("^version = {VERSION}{TAIL}"),
        format!(r#"^backend = "{backend}"{TAIL}"#),
        format!(r"^specifiers = \[{VERSION}\]{TAIL}"),
        format!(r#"^checksum = "sha256:[0-9a-f]+"{TAIL}"#),
        format!(r#"^url = "(https://github\.com/([A-Za-z0-9._/+-]|%2B)+|{NODE_URL})"{TAIL}"#),
        format!(r#"^url_api = "https://api\.github\.com/repos/[A-Za-z0-9._/+-]+"{TAIL}"#),
        format!(r#"^provenance = "github-attestations"{TAIL}"#),
        format!(
            r#"^aube = \{{ path = "\.mise/locks/npm-[a-z0-9._-]+/[0-9][A-Za-z0-9._+-]*", digest = "sha256:[0-9a-f]{{64}}" \}}{TAIL}"#
        ),
    ];
    let compiled: Vec<Regex> = patterns.iter().filter_map(|p| regex(p)).collect();
    let [
        blank,
        lockfile,
        tool_header,
        platform_header,
        version,
        backend,
        specifiers,
        checksum,
        url,
        url_api,
        provenance,
        aube,
    ] = compiled.as_slice()
    else {
        return Some("0: (the shape's patterns do not compile)".to_owned());
    };
    #[derive(PartialEq)]
    enum Table {
        None,
        Tool,
        Platform,
    }
    let mut table = Table::None;
    let mut has_checksum = false;
    let mut last = 0;
    let unchecked = |n: usize| format!("{n}: a platform table without a sha256 checksum");
    for (number, line) in numbered(text) {
        last = number;
        if blank.is_match(line) {
            continue;
        }
        if lockfile.is_match(line) && table == Table::None {
            continue;
        }
        if tool_header.is_match(line) || platform_header.is_match(line) {
            if table == Table::Platform && !has_checksum {
                return Some(unchecked(number));
            }
            table = if tool_header.is_match(line) {
                Table::Tool
            } else {
                has_checksum = false;
                Table::Platform
            };
            continue;
        }
        let ok = match table {
            Table::Tool => {
                version.is_match(line)
                    || backend.is_match(line)
                    || specifiers.is_match(line)
                    || aube.is_match(line)
            }
            Table::Platform => {
                // awk's `length($3) == 73`: the quoted `sha256:` and 64 hex digits, no more.
                let third = line.split_whitespace().nth(2).unwrap_or_default();
                if checksum.is_match(line) && third.len() == 73 {
                    has_checksum = true;
                    true
                } else {
                    url.is_match(line) || url_api.is_match(line) || provenance.is_match(line)
                }
            }
            Table::None => false,
        };
        if !ok {
            return Some(format!("{number}: {line}"));
        }
    }
    if table == Table::Platform && !has_checksum {
        return Some(unchecked(last));
    }
    None
}

/// The lines of `text` with their 1-based numbers (awk's NR), split at `\n` only: a `\r` stays in
/// its line and fails the shape, as it did under awk.
fn numbered(text: &str) -> impl Iterator<Item = (usize, &str)> {
    (1..).zip(text.split_terminator('\n'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_accept_the_repository_s_own_shapes_when_its_files_are_read() {
        // given
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let read = |name: &str| std::fs::read_to_string(root.join(name)).unwrap_or_default();

        // when
        let found = (
            toolchain_shape(&read("rust-toolchain.toml")),
            mise_toml_shape(&read("mise.toml")),
            mise_lock_shape(&read("mise.lock")),
        );

        // then
        assert_eq!(found, (None, None, None));
    }

    #[test]
    fn should_name_the_line_when_a_platform_table_has_no_checksum() {
        // given
        let lock = "[[tools.just]]\nversion = \"1.0.0\"\n\n[tools.just.\"platforms.linux-x64\"]\nurl = \"https://github.com/a/b\"\n\n[[tools.just]]\nversion = \"1.0.0\"\n";

        // when
        let found = mise_lock_shape(lock);

        // then
        assert_eq!(
            found.as_deref(),
            Some("7: a platform table without a sha256 checksum")
        );
    }

    #[test]
    fn should_refuse_a_checksum_of_the_wrong_length_when_the_hex_is_short() {
        // given
        let lock = "[tools.just.\"platforms.linux-x64\"]\nchecksum = \"sha256:0123\"\n";

        // when
        let found = mise_lock_shape(lock);

        // then
        assert_eq!(found.as_deref(), Some("2: checksum = \"sha256:0123\""));
    }

    #[test]
    fn should_accept_the_repository_s_npm_lock_when_its_files_are_read() {
        // given
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut refused = Vec::new();

        // when
        mise_locks(&root, &mut refused);

        // then
        assert_eq!(refused, Vec::<String>::new());
        assert!(root.join(".mise/locks").is_dir());
    }

    #[test]
    fn should_refuse_a_lock_line_when_it_leaves_the_formal_tools_shapes() {
        // given: each line outside mise.lock's shape (the first two inside it, as controls)
        let head = "[[tools.node]]\nversion = \"26.13.1\"\n\n[tools.node.\"platforms.linux-x64\"]\nchecksum = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n";
        let lines = [
            (
                "url = \"https://nodejs.org/dist/v26.13.1/node-v26.13.1-linux-x64.tar.gz\"",
                true,
            ),
            (
                "url = \"https://github.com/a/b/releases/download/jdk-21%2B9/x.tar.gz\"",
                true,
            ),
            (
                "url = \"https://nodejs.org/dist/v26.13.1/../x.tar.gz\"",
                false,
            ),
            (
                "url = \"https://nodejs.org.evil.example/dist/v1.0.0/node-v1.0.0-linux-x64.tar.gz\"",
                false,
            ),
            (
                "url = \"https://github.com/a/b/releases/download/x%2Fy/z\"",
                false,
            ),
            (
                "url = \"https://registry.npmjs.org/x/-/x-1.0.0.tgz\"",
                false,
            ),
        ];

        // when
        let found: Vec<bool> = lines
            .iter()
            .map(|(line, _)| mise_lock_shape(&format!("{head}{line}\n")).is_none())
            .collect();

        // then
        let expected: Vec<bool> = lines.iter().map(|(_, ok)| *ok).collect();
        assert_eq!(found, expected);
        let aube = "[[tools.\"npm:@o/q\"]]\nversion = \"1.0.0\"\naube = { path = \".mise/locks/npm-o-q/1.0.0\", digest = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" }\nbackend = \"npm:@o/q\"\n";
        assert_eq!(mise_lock_shape(aube), None);
        let elsewhere = aube.replace(".mise/locks/", "../elsewhere/");
        assert!(mise_lock_shape(&elsewhere).is_some_and(|l| l.starts_with("3: aube")));
        let up = aube.replace("npm-o-q/1.0.0", "npm-o-q/..");
        assert!(mise_lock_shape(&up).is_some_and(|l| l.starts_with("3: aube")));
        assert_eq!(
            mise_toml_shape("[tools]\nnode = \"1\"\n\"npm:@o/q\" = \"1.0.0\"\n"),
            None
        );
        assert!(mise_toml_shape("[tools]\n\"npm:left-pad\" = \"1.0.0\"\n").is_some());
    }

    #[test]
    fn should_refuse_an_npm_manifest_when_it_carries_scripts() {
        // given
        let manifest = "{\n  \"name\": \"mise-npm-install\",\n  \"private\": true,\n  \"dependencies\": {\n    \"@o/q\": \"1.0.0\"\n  }\n}\n";
        let scripted = manifest.replace(
            "  \"private\": true,\n",
            "  \"private\": true,\n  \"scripts\": { \"preinstall\": \"x\" },\n",
        );
        let ranged = manifest.replace("\"1.0.0\"", "\"^1.0.0\"");

        // when
        let found = (
            npm_manifest_shape(manifest),
            npm_manifest_shape(&scripted),
            npm_manifest_shape(&ranged),
        );

        // then
        assert_eq!(found.0, None);
        assert!(found.1.is_some(), "{found:?}");
        assert_eq!(found.2.as_deref(), Some("5:     \"@o/q\": \"^1.0.0\""));
    }

    #[test]
    fn should_refuse_an_aube_lock_line_when_a_package_has_another_source() {
        // given: a package resolved from a tarball URL, from git, and from a file
        let lock = "lockfileVersion: '9.0'\n\npackages:\n\n  x@1.0.0:\n    resolution: {integrity: sha512-AAAA==}\n";
        let others = [
            "    resolution: {tarball: https://evil.example/x.tgz}",
            "    resolution: {type: git, repo: https://evil.example/x, commit: abc}",
            "    resolution: {directory: ../x, type: directory}",
            "      x: file:../x",
        ];

        // when
        let found: Vec<Option<String>> = others
            .iter()
            .map(|line| aube_lock_shape(&format!("{lock}{line}\n")))
            .collect();

        // then
        assert_eq!(aube_lock_shape(lock), None);
        for (line, refusal) in others.iter().zip(&found) {
            assert_eq!(refusal.as_deref(), Some(format!("7: {line}").as_str()));
        }
    }

    #[test]
    fn should_refuse_mise_s_configuration_when_it_is_put_under_dot_mise() {
        // given: a scratch tree with a .mise/config.toml and a third file in a lock directory
        let root = std::env::temp_dir().join(format!("xtask-mise-locks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let lock = root.join(".mise/locks/npm-o-q/1.0.0");
        let _ = std::fs::create_dir_all(&lock);
        let _ = std::fs::write(root.join(".mise/config.toml"), "[env]\n");
        let _ = std::fs::write(lock.join("extra.js"), "");
        let mut refused = Vec::new();

        // when
        mise_locks(&root, &mut refused);
        let _ = std::fs::remove_dir_all(&root);

        // then
        let all = refused.join("\n");
        assert!(
            all.contains(".mise/config.toml: not an allowed entry"),
            "{all}"
        );
        assert!(
            all.contains("extra.js: not package.json or aube-lock.yaml"),
            "{all}"
        );
    }

    #[test]
    fn should_refuse_a_toolchain_path_when_it_follows_the_table() {
        // given
        let text = "[toolchain]\nchannel = \"1.99.1\"\ncomponents = [\"rustfmt\", \"clippy\"]\npath = \"/x\"\n";

        // when
        let found = toolchain_shape(text);

        // then
        assert_eq!(found.as_deref(), Some("4: path = \"/x\""));
    }
}
