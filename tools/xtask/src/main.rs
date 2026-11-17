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
//! - `doctor`: every tool of `tools/versions.toml` on the stages' PATH at its pinned version, and
//!   versions.toml agreeing with mise.lock (XT-02, [`tools`]).
//! - `install-tools [--archives DIR] [TOOL...]`: the pinned release assets, checksum-verified, into
//!   the tool cache outside the repository, which the stages' PATH puts first (XT-02).
//!
//! `--root DIR` before the command names the checkout whose gate runs (default: the one this
//! binary was built from). Exit: 0 green, 1 a refusal or a red stage (or the stage's own code), 2
//! usage. Ported from mayfly's `tools/xtask` (`main`, `cli`): its clap surface is replaced by a
//! hand parser for these eight commands, so the launcher's first build stays one crate (regex) deep.

mod catalog;
mod preflight;
mod process;
mod stages;
mod tool_config;
mod tools;
mod versions;

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
  tool-config [ROOT]       check the tool configuration of ROOT (default: the checkout)
  doctor                   every pinned tool present at its version (tools/versions.toml)
  install-tools [--archives DIR] [TOOL...]
                           install the pinned tools, checksum-verified, into the tool cache";

/// A parsed command line.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Check,
    Quick,
    CheckOnly(Vec<String>),
    List {
        json: bool,
    },
    Preflight,
    ToolConfig(Option<PathBuf>),
    Doctor,
    InstallTools {
        archives: Option<PathBuf>,
        names: Vec<String>,
    },
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
        [c] if c == "doctor" => Command::Doctor,
        [c, flag, dir, names @ ..]
            if c == "install-tools" && flag == "--archives" && !dir.starts_with('-') =>
        {
            install_tools(Some(PathBuf::from(dir)), names)?
        }
        [c, names @ ..] if c == "install-tools" => install_tools(None, names)?,
        _ => return Err(USAGE.to_owned()),
    };
    Ok((root, command))
}

/// `install-tools` with its tool names, each a plain name (no option).
fn install_tools(archives: Option<PathBuf>, names: &[String]) -> Result<Command, String> {
    if names.iter().any(|n| n.starts_with('-')) {
        return Err(USAGE.to_owned());
    }
    Ok(Command::InstallTools {
        archives,
        names: names.to_vec(),
    })
}

/// The checkout this binary was built from: two levels above `tools/xtask`.
fn built_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(2).unwrap_or(manifest);
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// The PATH the stages run with: first the tools `install-tools` put in the tool cache, at the
/// versions of `tools/versions.toml` (`tools_dirs`, XT-02); then the pinned tools by mise's
/// install directories (`mise bin-paths`, the justfile's top-level `PATH`), so a stage runs the
/// real binary and never a mise shim, which would apply a mise `[env]` after the preflight has
/// looked; then mise's shims, for a shell without mise's tools installed; then the inherited PATH.
/// Asked only after the tool configuration passed, so mise never loads a configuration outside its
/// allowlist. Pinned by tools/xtask/tests/tools.rs
/// (`should_run_the_tools_from_their_install_directories_when_both_paths_hold_them`).
pub(crate) fn stage_path(root: &Path, tools_dirs: &[PathBuf]) -> Option<OsString> {
    let inherited = std::env::var_os("PATH");
    let data = std::env::var_os("MISE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(|d| PathBuf::from(d).join("mise")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/mise")));
    let mut dirs: Vec<PathBuf> = tools_dirs.to_vec();
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
        Command::Doctor => {
            let tools = match tools::manifest(root) {
                Ok(tools) => tools,
                Err(message) => {
                    eprintln!("doctor: {message}");
                    return 1;
                }
            };
            let refused = tool_config::check(root);
            if !refused.is_empty() {
                for problem in &refused {
                    eprintln!("tool-config: {problem}");
                }
                eprintln!(
                    "doctor: the tool configuration is outside its allowlist; mise was not asked"
                );
                return 1;
            }
            let dirs = tools::path_dirs(tools::cache_dir().as_deref(), &tools);
            return tools::doctor(root, &tools, stage_path(root, &dirs).as_deref());
        }
        Command::InstallTools { archives, names } => {
            let tools = match tools::manifest(root) {
                Ok(tools) => tools,
                Err(message) => {
                    eprintln!("install-tools: {message}");
                    return 1;
                }
            };
            let source = archives
                .clone()
                .map_or(tools::Source::Download, tools::Source::Archives);
            return tools::install(root, &tools, names, tools::cache_dir().as_deref(), &source);
        }
    };
    let mut context = match preflight(root) {
        Ok(context) => context,
        Err(code) => return code,
    };
    if stages.is_empty() {
        return 0;
    }
    let tools = match tools::manifest(root) {
        Ok(tools) => tools,
        Err(message) => {
            eprintln!("xtask: {message}");
            return 1;
        }
    };
    let dirs = tools::path_dirs(tools::cache_dir().as_deref(), &tools);
    context.path = stage_path(root, &dirs);
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
            (vec!["doctor"], Command::Doctor),
            (
                vec!["install-tools"],
                Command::InstallTools {
                    archives: None,
                    names: vec![],
                },
            ),
            (
                vec!["install-tools", "--archives", "/a", "gitleaks"],
                Command::InstallTools {
                    archives: Some(PathBuf::from("/a")),
                    names: args(&["gitleaks"]),
                },
            ),
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
            vec!["doctor", "--json"],
            vec!["install-tools", "--archives"],
            vec!["install-tools", "--force", "x"],
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
