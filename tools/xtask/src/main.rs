//! `xtask`: the gate's composition (fnd §9, ADR 0035), run as `bash scripts/xtask.sh <command>`
//! (or `cargo run --locked -q -p xtask -- <command>`; the `cargo xtask` alias waits for FND-09's
//! G2 rule to admit one pinned cargo configuration file). The justfile's `quick`, `check`,
//! `check-only` and `_preflight` delegate here.
//!
//! - `check`: every stage of the catalog, in order; the first failure stops the run with its exit
//!   code. `quick`: the catalog's quick stages. `check-only <stage>...`: the named stages, in the
//!   order given. Each runs the preflight first.
//! - `list [--json]`: the stage catalog (ids one per line, or JSON with each stage's command).
//! - `preflight`: the preflight alone (the justfile's `_preflight`).
//! - `tool-config [ROOT]`: the tool configuration check alone, of ROOT (default: the checkout),
//!   read-only and without the preflight, so it can judge a tree before anything else runs there.
//!
//! `--root DIR` before the command names the checkout whose gate runs (default: the one this
//! binary was built from). Exit: 0 green, 1 a refusal or a red stage (or the stage's own code), 2
//! usage. Ported from mayfly's `tools/xtask` (`main`, `cli`): its clap surface is replaced by a
//! hand parser for these six commands, so the launcher's first build stays one crate (regex) deep.

mod catalog;
mod preflight;
mod process;
mod stages;
mod tool_config;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use process::Context;

const USAGE: &str = "usage: xtask [--root DIR] <command>
commands:
  check                    every stage of the gate, in order
  quick                    fmt-check, architecture, clippy, unit-tests, script-tests (not the gate)
  check-only <stage>...    the named stages, in the order given
  list [--json]            the stage catalog
  preflight                refuse an environment that steers the build
  tool-config [ROOT]       check the tool configuration of ROOT (default: the checkout)";

/// A parsed command line.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Check,
    Quick,
    CheckOnly(Vec<String>),
    List { json: bool },
    Preflight,
    ToolConfig(Option<PathBuf>),
}

/// Parses the arguments after the program name: `[--root DIR] <command> [args]`.
fn parse(args: &[String]) -> Result<(Option<PathBuf>, Command), String> {
    let mut rest = args;
    let mut root = None;
    if let [flag, dir, tail @ ..] = rest
        && flag == "--root"
    {
        root = Some(PathBuf::from(dir));
        rest = tail;
    }
    let command = match rest {
        [c] if c == "check" => Command::Check,
        [c] if c == "quick" => Command::Quick,
        [c, stages @ ..] if c == "check-only" && !stages.is_empty() => {
            Command::CheckOnly(stages.to_vec())
        }
        [c] if c == "list" => Command::List { json: false },
        [c, j] if c == "list" && j == "--json" => Command::List { json: true },
        [c] if c == "preflight" => Command::Preflight,
        [c] if c == "tool-config" => Command::ToolConfig(None),
        [c, dir] if c == "tool-config" && !dir.starts_with('-') => {
            Command::ToolConfig(Some(PathBuf::from(dir)))
        }
        _ => return Err(USAGE.to_owned()),
    };
    Ok((root, command))
}

/// The checkout this binary was built from: two levels above `tools/xtask`.
fn built_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(2).unwrap_or(manifest);
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// The PATH the stages run with (the justfile's top-level `PATH`): the pinned tools by their
/// install directories (`mise bin-paths`), so a stage runs the real binary and never a mise shim,
/// which would apply a mise `[env]` after the preflight has looked; then mise's shims, for a shell
/// without mise's tools installed; then the inherited PATH. Asked only after the preflight passed
/// the tool configuration, so mise never loads a configuration outside its allowlist.
fn stage_path(root: &Path) -> Option<OsString> {
    let inherited = std::env::var_os("PATH");
    let data = std::env::var_os("MISE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(|d| PathBuf::from(d).join("mise")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/mise")));
    let mut dirs: Vec<PathBuf> = Vec::new();
    if process::on_path("mise", inherited.as_deref())
        && let Ok(out) = std::process::Command::new("mise")
            .arg("bin-paths")
            .current_dir(root)
            .stderr(std::process::Stdio::null())
            .output()
        && out.status.success()
    {
        dirs.extend(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(PathBuf::from),
        );
    }
    dirs.extend(data.map(|d| d.join("shims")));
    dirs.extend(inherited.iter().flat_map(std::env::split_paths));
    std::env::join_paths(dirs).ok().or(inherited)
}

/// Runs the preflight and prints its refusals: the stages' context when it passed.
fn preflight(root: &Path) -> Result<Context, i32> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    match preflight::check(root, &cwd, &vars) {
        Err(message) => {
            eprintln!("{message}");
            Err(1)
        }
        Ok(verdict) if !verdict.refused.is_empty() => {
            eprintln!(
                "preflight: refusing to run the gate: {} is outside the allowlist of what may steer the build (fnd §3.6); unset or remove it",
                verdict.refused.join(" ")
            );
            Err(1)
        }
        Ok(verdict) => Ok(Context {
            root: root.to_path_buf(),
            removed: verdict.removed,
            path: None,
        }),
    }
}

fn run(root: &Path, command: Command) -> i32 {
    let stages = match &command {
        Command::List { json } => {
            let rows = catalog::rows();
            if *json {
                print!("{}", catalog::to_json(&rows));
            } else {
                print!("{}", catalog::to_lines(&rows));
            }
            return 0;
        }
        Command::ToolConfig(dir) => {
            let tree = dir.clone().unwrap_or_else(|| root.to_path_buf());
            if !tree.is_dir() {
                eprintln!("tool-config: {} is not a directory", tree.display());
                return 2;
            }
            let refused = tool_config::check(&tree);
            if refused.is_empty() {
                return 0;
            }
            for problem in &refused {
                eprintln!("tool-config: {problem}");
            }
            eprintln!(
                "tool-config: refusing to run mise or the gate's tools on this tree (fnd §3.6): their implicit configuration is an allowlist"
            );
            return 1;
        }
        Command::Check => catalog::check(),
        Command::Quick => catalog::quick(),
        Command::CheckOnly(names) => match catalog::select(names) {
            Ok(stages) => stages,
            Err(message) => {
                eprintln!("{message}");
                return 2;
            }
        },
        Command::Preflight => Vec::new(),
    };
    let mut context = match preflight(root) {
        Ok(context) => context,
        Err(code) => return code,
    };
    if stages.is_empty() {
        return 0;
    }
    context.path = stage_path(root);
    process::run_stages(&stages, &context)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match parse(&args) {
        Ok((root, command)) => run(&root.unwrap_or_else(built_root), command),
        Err(usage) => {
            eprintln!("{usage}");
            2
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn should_parse_every_command_when_the_surface_is_given() {
        // given
        let lines = [
            (vec!["check"], Command::Check),
            (vec!["quick"], Command::Quick),
            (
                vec!["check-only", "a", "b c"],
                Command::CheckOnly(args(&["a", "b c"])),
            ),
            (vec!["list"], Command::List { json: false }),
            (vec!["list", "--json"], Command::List { json: true }),
            (vec!["preflight"], Command::Preflight),
            (vec!["tool-config"], Command::ToolConfig(None)),
            (
                vec!["tool-config", "/t"],
                Command::ToolConfig(Some(PathBuf::from("/t"))),
            ),
        ];

        // when
        let parsed: Vec<_> = lines
            .iter()
            .map(|(words, _)| parse(&args(words)).map(|(_, c)| c))
            .collect();

        // then
        for ((_, want), got) in lines.iter().zip(&parsed) {
            assert_eq!(got.as_ref().ok(), Some(want));
        }
    }

    #[test]
    fn should_refuse_usage_when_the_command_is_unknown_or_malformed() {
        // given
        let bad = [
            vec![],
            vec!["nope"],
            vec!["check-only"],
            vec!["check", "extra"],
            vec!["list", "--yaml"],
            vec!["tool-config", "--x"],
            vec!["--root"],
        ];

        // when
        let refused = bad.iter().filter(|w| parse(&args(w)).is_err()).count();

        // then
        assert_eq!(refused, bad.len());
    }

    #[test]
    fn should_take_the_root_when_it_precedes_the_command() {
        // given
        let line = args(&["--root", "/elsewhere", "preflight"]);

        // when
        let parsed = parse(&line);

        // then
        assert_eq!(
            parsed.ok(),
            Some((Some(PathBuf::from("/elsewhere")), Command::Preflight))
        );
    }

    #[test]
    fn should_find_the_workspace_when_the_root_is_built_in() {
        // given
        let root = built_root();

        // when
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();

        // then
        assert!(manifest.contains("[workspace]"), "{}", root.display());
        assert!(root.join("tools/xtask/Cargo.toml").is_file());
    }
}
