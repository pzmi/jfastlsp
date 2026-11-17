//! The preflight every gate command runs first (fnd §3.6, FND-08, FND-09): it refuses an
//! environment that steers the build, ported from the justfile's `_preflight` recipe with every
//! rule kept. Variables of cargo, rustc, rustup, clippy, nextest and proptest are refused unless
//! allowlisted here; so is any cargo configuration file inside the repository, one outside it (an
//! ancestor of the working directory, CARGO_HOME) beyond a pinned plain shape, a tool
//! configuration outside its allowlist ([`crate::tool_config`]) and any mise configuration inside
//! the repository but the root `mise.toml`. Every check is an allowlist or a pinned shape, never a
//! list of bad patterns (fnd §3): the refused prefixes below are the set the allowlist is carved
//! from, and anything outside them (`PATH`, `HOME`, ...) reaches no tool of the gate as a steering
//! knob.
//!
//! The one addition: `cargo run -p xtask` sets the variables of [`cargo_run_values`] for the
//! binary it starts, so each is accepted at exactly the value cargo gives it (this binary's own,
//! fixed when it was compiled) and taken out of every stage's environment; any other value is
//! refused like the rest.
//!
//! Started from mayfly's `tools/xtask/src/repo.rs` (`preflight`, `steering`), whose environment
//! rule is a list of steering names; the justfile's allowlist, which FND-08 and FND-09 hardened
//! against this repository's reviews, is the one kept. Pinned by tools/xtask/tests/preflight.rs
//! and tools/jfl-architecture/tests/gate_negative_cases.rs (through the justfile).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

use crate::process::on_path;
use crate::tool_config;

/// The targets whose toolchain name may follow the pinned channel in `RUSTUP_TOOLCHAIN`
/// (ADR 0006).
const TRIPLES: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
];

/// The variables `cargo run` sets for the program it starts, each with the only value accepted:
/// this binary's own, as cargo set it when compiling it.
pub(crate) fn cargo_run_values() -> Vec<(&'static str, Option<&'static str>)> {
    vec![
        ("CARGO", option_env!("CARGO")),
        ("CARGO_MANIFEST_DIR", option_env!("CARGO_MANIFEST_DIR")),
        ("CARGO_MANIFEST_PATH", option_env!("CARGO_MANIFEST_PATH")),
        ("CARGO_CRATE_NAME", option_env!("CARGO_CRATE_NAME")),
        ("CARGO_BIN_NAME", option_env!("CARGO_BIN_NAME")),
        ("CARGO_PKG_NAME", option_env!("CARGO_PKG_NAME")),
        ("CARGO_PKG_VERSION", option_env!("CARGO_PKG_VERSION")),
        (
            "CARGO_PKG_VERSION_MAJOR",
            option_env!("CARGO_PKG_VERSION_MAJOR"),
        ),
        (
            "CARGO_PKG_VERSION_MINOR",
            option_env!("CARGO_PKG_VERSION_MINOR"),
        ),
        (
            "CARGO_PKG_VERSION_PATCH",
            option_env!("CARGO_PKG_VERSION_PATCH"),
        ),
        (
            "CARGO_PKG_VERSION_PRE",
            option_env!("CARGO_PKG_VERSION_PRE"),
        ),
        ("CARGO_PKG_AUTHORS", option_env!("CARGO_PKG_AUTHORS")),
        (
            "CARGO_PKG_DESCRIPTION",
            option_env!("CARGO_PKG_DESCRIPTION"),
        ),
        ("CARGO_PKG_HOMEPAGE", option_env!("CARGO_PKG_HOMEPAGE")),
        ("CARGO_PKG_REPOSITORY", option_env!("CARGO_PKG_REPOSITORY")),
        ("CARGO_PKG_LICENSE", option_env!("CARGO_PKG_LICENSE")),
        (
            "CARGO_PKG_LICENSE_FILE",
            option_env!("CARGO_PKG_LICENSE_FILE"),
        ),
        ("CARGO_PKG_README", option_env!("CARGO_PKG_README")),
        (
            "CARGO_PKG_RUST_VERSION",
            option_env!("CARGO_PKG_RUST_VERSION"),
        ),
    ]
}

/// The values rustup gives `RUSTUP_TOOLCHAIN_SOURCE` (informational: it names why the toolchain
/// was chosen; `RUSTUP_TOOLCHAIN` itself is held to the pin).
const TOOLCHAIN_SOURCES: [&str; 5] = ["toolchain-file", "env", "cli", "override", "default"];

/// Whether `name` with `value` is refused, and whether it is one of the variables taken out of the
/// stages' environment (accepted at its `cargo run` value), given the pinned `channel`.
fn judge(name: &str, value: &OsStr, channel: &str) -> (bool, bool) {
    if let Some((_, expected)) = cargo_run_values().into_iter().find(|(n, _)| *n == name) {
        let ok = expected.is_some_and(|e| value == OsStr::new(e));
        return (!ok, ok);
    }
    let value = value.to_string_lossy();
    let refused = match name {
        "CARGO_TARGET_DIR"
        | "CARGO_BUILD_JOBS"
        | "CARGO_INCREMENTAL"
        | "CARGO_HOME"
        | "CARGO_REGISTRIES_CRATES_IO_PROTOCOL"
        | "RUSTUP_HOME"
        | "RUSTUP_DIST_SERVER"
        | "RUSTUP_UPDATE_ROOT"
        | "RUSTUP_MAX_RETRIES"
        | "RUSTUP_IO_THREADS" => false,
        n if n.starts_with("CARGO_TERM_")
            || n.starts_with("CARGO_NET_")
            || n.starts_with("CARGO_HTTP_") =>
        {
            false
        }
        "RUSTUP_TOOLCHAIN" => {
            value != channel
                && !TRIPLES
                    .iter()
                    .any(|t| value == format!("{channel}-{t}").as_str())
        }
        "RUSTUP_TOOLCHAIN_SOURCE" => {
            return (
                !TOOLCHAIN_SOURCES.contains(&value.as_ref()),
                TOOLCHAIN_SOURCES.contains(&value.as_ref()),
            );
        }
        "RUSTFLAGS" => !matches!(value.as_ref(), "" | "-D warnings"),
        "NEXTEST_PROFILE" => !matches!(value.as_ref(), "default" | "ci" | "serial"),
        n => {
            n == "CARGO"
                || n.starts_with("CARGO_")
                || n.starts_with("__CARGO_")
                || n == "RUSTC"
                || n.starts_with("RUSTC_")
                || n == "RUSTDOC"
                || n.starts_with("RUSTDOC_")
                || n == "RUSTDOCFLAGS"
                || n.starts_with("RUSTUP_")
                || n.starts_with("CLIPPY_")
                || n.starts_with("NEXTEST_")
                || n.starts_with("PROPTEST_")
        }
    };
    (refused, false)
}

/// The channel of `root`'s rust-toolchain.toml (the recipe's `sed`): the first line
/// `channel = "<value>"`.
pub(crate) fn pinned_channel(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("rust-toolchain.toml")).ok()?;
    let line = Regex::new(r#"^channel[[:space:]]*=[[:space:]]*"(.*)"$"#).ok()?;
    text.split_terminator('\n')
        .find_map(|l| line.captures(l))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
        .filter(|c| !c.is_empty())
}

/// Whether a cargo configuration file outside the repository keeps the plain shape: every line a
/// comment, a blank, one of the headers `[net]`, `[http]`, `[term]`, `[build]` or
/// `[registries.crates-io]`, or `key = value` with a bare key that table allows and a plain value
/// (a string without escapes, a boolean or a number). Anything else, valid TOML or not (a quoted
/// or escaped key, an inline table, a dotted key, a multi-line value), is refused: the shape is
/// pinned, no bad key is listed.
pub(crate) fn plain_config(text: &str) -> bool {
    let (Ok(blank), Ok(header), Ok(pair)) = (
        Regex::new(r"^[[:space:]]*(#.*)?$"),
        Regex::new(r"^\[(net|http|term|build|registries\.crates-io)\][[:space:]]*(#.*)?$"),
        Regex::new(
            r#"^([a-z][a-z0-9-]*)[[:space:]]*=[[:space:]]*("[^"\\]*"|true|false|[0-9]+)[[:space:]]*(#.*)?$"#,
        ),
    ) else {
        return false;
    };
    let allowed = |table: &str, key: &str| match table {
        "net" => matches!(key, "retry" | "git-fetch-with-cli" | "offline"),
        "http" => matches!(
            key,
            "proxy"
                | "timeout"
                | "cainfo"
                | "check-revoke"
                | "low-speed-limit"
                | "multiplexing"
                | "user-agent"
        ),
        "term" => matches!(key, "quiet" | "verbose" | "color"),
        "build" => matches!(key, "jobs" | "target-dir"),
        "registries.crates-io" => key == "protocol",
        _ => false,
    };
    let mut table = String::new();
    for line in text.split_terminator('\n') {
        if blank.is_match(line) {
            continue;
        }
        if let Some(name) = header.captures(line).and_then(|c| c.get(1)) {
            table = name.as_str().to_owned();
            continue;
        }
        let key = pair.captures(line).and_then(|c| c.get(1));
        if key.is_some_and(|k| allowed(&table, k.as_str())) {
            continue;
        }
        return false;
    }
    true
}

/// What the preflight found: the refusals, and the variables to take out of every stage.
#[derive(Debug, Default)]
pub(crate) struct Verdict {
    /// One entry per refused variable or file.
    pub(crate) refused: Vec<String>,
    /// Variables accepted at their `cargo run` value, removed from every stage's environment.
    pub(crate) removed: Vec<OsString>,
}

/// The preflight of the checkout `root`, run in `cwd` with the variables `vars`: `Err` with the
/// message for a setup it cannot judge (no pinned channel, not a git work tree), otherwise the
/// verdict. The tool configuration's own refusals are printed to stderr as they are found.
pub(crate) fn check(
    root: &Path,
    cwd: &Path,
    vars: &[(OsString, OsString)],
) -> Result<Verdict, String> {
    let channel = pinned_channel(root).ok_or("preflight: no channel in rust-toolchain.toml")?;
    let mut verdict = Verdict::default();
    let mut sorted: Vec<&(OsString, OsString)> = vars.iter().collect();
    sorted.sort();
    for (name, value) in sorted {
        let Some(text) = name.to_str() else {
            verdict.refused.push(name.to_string_lossy().into_owned());
            continue;
        };
        let (refused, removed) = judge(text, value, &channel);
        if refused {
            verdict.refused.push(text.to_owned());
        }
        if removed {
            verdict.removed.push(name.clone());
        }
    }
    let git_dir = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-dir"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !git_dir {
        return Err(format!(
            "preflight: {} is not a git work tree",
            root.display()
        ));
    }
    cargo_configs(root, cwd, vars, &mut verdict.refused);
    let problems = tool_config::check(root);
    for problem in &problems {
        eprintln!("tool-config: {problem}");
    }
    if !problems.is_empty() {
        verdict.refused.push(format!(
            "{} (its tool configuration, above)",
            root.display()
        ));
    }
    mise_configs(root, cwd, vars, &mut verdict.refused);
    Ok(verdict)
}

/// Cargo reads `.cargo/config{,.toml}` in every ancestor of the working directory and in
/// CARGO_HOME. Inside the repository: no such file at all (tracked or untracked, at any depth,
/// and in the working directory's ancestors); outside it: only the plain shape.
fn cargo_configs(
    root: &Path,
    cwd: &Path,
    vars: &[(OsString, OsString)],
    refused: &mut Vec<String>,
) {
    let listed = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
        ])
        .args([
            ":(glob,icase)**/.cargo/config",
            ":(glob,icase)**/.cargo/config.toml",
        ])
        .output();
    match listed {
        Ok(out) if out.status.success() => {
            for file in out.stdout.split(|b| *b == 0).filter(|f| !f.is_empty()) {
                refused.push(format!(
                    "{}/{}",
                    root.display(),
                    String::from_utf8_lossy(file)
                ));
            }
        }
        _ => refused.push(format!("{} (git ls-files failed)", root.display())),
    }
    let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    for dir in cwd.ancestors() {
        for name in [".cargo/config", ".cargo/config.toml"] {
            let file = dir.join(name);
            if !file.is_file() {
                continue;
            }
            let inside = std::fs::canonicalize(dir).is_ok_and(|d| d.starts_with(&canonical_root));
            if inside || !read_plain(&file) {
                refused.push(file.display().to_string());
            }
        }
    }
    // `${CARGO_HOME:-$HOME/.cargo}`: an empty value counts as unset.
    let set = |name| var(vars, name).filter(|v| !v.is_empty());
    let home = set("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| set("HOME").map(|h| PathBuf::from(h).join(".cargo")));
    if let Some(home) = home {
        for name in ["config", "config.toml"] {
            let file = home.join(name);
            if file.is_file() && !read_plain(&file) {
                refused.push(file.display().to_string());
            }
        }
    }
}

fn read_plain(file: &Path) -> bool {
    std::fs::read(file).is_ok_and(|bytes| std::str::from_utf8(&bytes).is_ok_and(plain_config))
}

fn var<'a>(vars: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsStr> {
    vars.iter()
        .rev()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_os_str())
}

/// mise loads no configuration file from inside the repository but the root `mise.toml` (an
/// `[env]`, a `[hooks]` or a task there would reach the tools of the gate); a configuration mise
/// cannot load (an untrusted one with `[env]`, say) fails closed. Only after the tool
/// configuration passed is mise asked.
fn mise_configs(root: &Path, cwd: &Path, vars: &[(OsString, OsString)], refused: &mut Vec<String>) {
    if refused
        .iter()
        .any(|r| r.ends_with("(its tool configuration, above)"))
    {
        return;
    }
    if !on_path("mise", var(vars, "PATH")) {
        return;
    }
    const FAILED: &str = "mise config ls failed";
    let listing = Command::new("mise")
        .args(["config", "ls", "--json"])
        .current_dir(cwd)
        .stderr(std::process::Stdio::null())
        .output();
    let text = match listing {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        _ => format!("\"path\": \"{FAILED}\""),
    };
    let Ok(path_line) = Regex::new(r#"^[[:space:]]*"path":[[:space:]]*"(.*)",?$"#) else {
        refused.push(FAILED.to_owned());
        return;
    };
    let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mise_toml = canonical_root.join("mise.toml");
    for file in text
        .split_terminator('\n')
        .filter_map(|l| path_line.captures(l))
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
    {
        if file == FAILED {
            refused.push(file);
            continue;
        }
        let path = Path::new(&file);
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if canonical == mise_toml {
            continue;
        }
        if canonical.starts_with(&canonical_root) || path.starts_with(root) {
            refused.push(file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_accept_only_the_plain_shape_when_a_cargo_config_is_judged() {
        // given
        let plain = "# c\n[net]\ngit-fetch-with-cli = true\n\n[build]\njobs = 2\ntarget-dir = \"elsewhere\" # x\n";
        let steering = [
            "[build]\nrustflags = [\"--cap-lints=allow\"]\n",
            "[build]\nrustc-wrapper = \"sccache\"\n",
            "net.retry = 2\n",
            "[net]\nretry = \"\\u0032\"\n",
            "[paths]\n",
            "jobs = 2\n",
            "[\"t\\u0061rget\".x86_64-unknown-linux-gnu]\n\"r\\u0075nner\" = [\"bash\"]\n",
        ];

        // when
        let accepted: Vec<&str> = steering
            .iter()
            .copied()
            .filter(|s| plain_config(s))
            .collect();

        // then
        assert!(plain_config(plain));
        assert!(accepted.is_empty(), "{accepted:?}");
    }

    #[test]
    fn should_refuse_a_lookalike_toolchain_when_rustup_toolchain_is_judged() {
        // given
        let channel = "1.99.1";

        // when
        let verdicts: Vec<bool> = [
            "1.99.1",
            "1.99.1-x86_64-unknown-linux-gnu",
            "1.99.1-aarch64-apple-darwin",
            "1.99.1-evil",
            "nightly",
        ]
        .iter()
        .map(|v| judge("RUSTUP_TOOLCHAIN", OsStr::new(v), channel).0)
        .collect();

        // then
        assert_eq!(verdicts, [false, false, false, true, true]);
    }

    #[test]
    fn should_accept_a_cargo_run_variable_only_at_its_own_value_when_judged() {
        // given
        let own = env!("CARGO_MANIFEST_DIR");

        // when
        let at_own = judge("CARGO_MANIFEST_DIR", OsStr::new(own), "1.99.1");
        let elsewhere = judge("CARGO_MANIFEST_DIR", OsStr::new("/elsewhere"), "1.99.1");
        let cargo_elsewhere = judge("CARGO", OsStr::new("/elsewhere/cargo"), "1.99.1");

        // then
        assert_eq!(at_own, (false, true));
        assert_eq!(elsewhere, (true, false));
        assert_eq!(cargo_elsewhere, (true, false));
    }

    #[test]
    fn should_read_the_channel_when_the_toolchain_file_pins_one() {
        // given
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

        // when
        let channel = pinned_channel(&root);

        // then
        assert!(channel.is_some_and(|c| c.split('.').count() == 3));
    }
}
