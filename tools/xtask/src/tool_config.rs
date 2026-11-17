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
//! - ROOT/mise.toml: only `[tools]`, each key `just` or a quoted `aqua:<owner>/<repo>[/<name>]`
//!   or `github:<owner>/<repo>` (the backends that install a checksummed release asset; no plugin
//!   backend, whose plugin is code), each value a plain version string;
//! - ROOT/mise.lock: only `lockfile_version = 3`, `[[tools.<key>]]` with `version`, `backend`
//!   (aqua or github) and `specifiers`, and per-platform tables of linux-x64, linux-arm64 or
//!   macos-arm64 with a `sha256:` checksum, a `https://github.com/` url, a
//!   `https://api.github.com/repos/` url_api and an optional `github-attestations` provenance;
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
pub(crate) const ROOT_ALLOWED: [&str; 14] = [
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

/// A tool key of mise.toml or mise.lock: `just`, or a quoted aqua or github identifier.
const KEY: &str = r#"(just|"aqua:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)?"|"github:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+")"#;
/// A plain version string.
const VERSION: &str = r#""[A-Za-z0-9._+-]+""#;
/// The end of an accepted line: spaces and an optional comment.
const TAIL: &str = r"[[:space:]]*(#.*)?$";
/// A line that is blank or a comment.
const BLANK: &str = r"^[[:space:]]*(#.*)?$";

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
        "outside the shape (checksummed GitHub release assets of aqua or github tools only)",
        &mut refused,
    );
    refused
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
    let backend = r"(aqua:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)?|github:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+)";
    let patterns = [
        BLANK.to_owned(),
        format!("^lockfile_version = 3{TAIL}"),
        format!(r"^\[\[tools\.{KEY}\]\]{TAIL}"),
        format!(r#"^\[tools\.{KEY}\."platforms\.(linux-x64|linux-arm64|macos-arm64)"\]{TAIL}"#),
        format!("^version = {VERSION}{TAIL}"),
        format!(r#"^backend = "{backend}"{TAIL}"#),
        format!(r"^specifiers = \[{VERSION}\]{TAIL}"),
        format!(r#"^checksum = "sha256:[0-9a-f]+"{TAIL}"#),
        format!(r#"^url = "https://github\.com/[A-Za-z0-9._/+-]+"{TAIL}"#),
        format!(r#"^url_api = "https://api\.github\.com/repos/[A-Za-z0-9._/+-]+"{TAIL}"#),
        format!(r#"^provenance = "github-attestations"{TAIL}"#),
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
                version.is_match(line) || backend.is_match(line) || specifiers.is_match(line)
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
    fn should_refuse_a_toolchain_path_when_it_follows_the_table() {
        // given
        let text = "[toolchain]\nchannel = \"1.99.1\"\ncomponents = [\"rustfmt\", \"clippy\"]\npath = \"/x\"\n";

        // when
        let found = toolchain_shape(text);

        // then
        assert_eq!(found.as_deref(), Some("4: path = \"/x\""));
    }
}
