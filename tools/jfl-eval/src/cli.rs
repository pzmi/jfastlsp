//! The command line: `harbor`, `smoke`, `report` and `inspect`. See `doc/eval/harness.md`.

use std::ffi::OsString;
use std::path::PathBuf;

use serde_json::json;

use crate::arm::Arm;
use crate::error::{Context, EvalError, Result};
use crate::harbor::{self, HARBOR_COMMIT, HARBOR_VERSION};
use crate::report;
use crate::run::{self, read_trial};
use crate::taskset::TaskSet;

const USAGE: &str = "\
usage: jfl-eval <command>

  harbor --tasks FILE --arm FILE [--arm FILE]... --out DIR
                         write one Harbor task per task and arm, each arm's Claude Code settings,
                         and print the `harbor run` command of each arm
  smoke --arm FILE --out DIR
                         write the smoke task (does an LSP answer under claude -p in a container?)
  report DIR [--json]    aggregate every trial under DIR per arm and per task; ids and numbers only
  inspect TRIAL_DIR      one trial's metrics and the transcript's schema names, as JSON

Formats: Harbor HARBOR_VERSION (commit HARBOR_COMMIT). Harbor and Docker run on the owner's
machine only (ADR 0029).";

fn usage() -> String {
    USAGE
        .replace("HARBOR_VERSION", HARBOR_VERSION)
        .replace("HARBOR_COMMIT", HARBOR_COMMIT)
}

#[derive(Default)]
struct Parsed {
    positional: Vec<String>,
    flags: Vec<(String, String)>,
    switches: Vec<String>,
}

fn parse(args: impl Iterator<Item = OsString>, switches: &[&str]) -> Result<Parsed> {
    let mut out = Parsed::default();
    let mut it = args;
    while let Some(arg) = it.next() {
        let s = arg
            .into_string()
            .map_err(|_| EvalError::new("an argument is not UTF-8"))?;
        if let Some(flag) = s.strip_prefix("--") {
            if switches.contains(&flag) {
                out.switches.push(flag.to_string());
            } else {
                let value = it
                    .next()
                    .ok_or_else(|| EvalError::new(format!("--{flag} needs a value")))?
                    .into_string()
                    .map_err(|_| EvalError::new(format!("--{flag}: not UTF-8")))?;
                out.flags.push((flag.to_string(), value));
            }
        } else {
            out.positional.push(s);
        }
    }
    Ok(out)
}

impl Parsed {
    fn all(&self, name: &str) -> Vec<&str> {
        self.flags
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    fn one(&self, name: &str) -> Result<&str> {
        match self.all(name).as_slice() {
            [v] => Ok(v),
            [] => Err(EvalError::new(format!("--{name} is required"))),
            _ => Err(EvalError::new(format!("--{name} given twice"))),
        }
    }

    fn only(&self, flags: &[&str], positional: usize) -> Result<()> {
        if let Some((n, _)) = self
            .flags
            .iter()
            .find(|(n, _)| !flags.contains(&n.as_str()))
        {
            return Err(EvalError::new(format!(
                "unknown option --{n}\n\n{}",
                usage()
            )));
        }
        if self.positional.len() != positional {
            return Err(EvalError::new(format!(
                "expected {positional} argument(s)\n\n{}",
                usage()
            )));
        }
        Ok(())
    }
}

/// Run the command line (without the program name); what to print on standard output.
pub fn run(args: Vec<OsString>) -> Result<String> {
    let mut it = args.into_iter();
    let Some(command) = it.next() else {
        return Ok(format!("{}\n", usage()));
    };
    let command = command.to_string_lossy().into_owned();
    match command.as_str() {
        "-h" | "--help" | "help" => Ok(format!("{}\n", usage())),
        "harbor" => {
            let p = parse(it, &[])?;
            p.only(&["tasks", "arm", "out"], 0)?;
            let set = TaskSet::load(&PathBuf::from(p.one("tasks")?))?;
            let arm_paths = p.all("arm");
            if arm_paths.is_empty() {
                return Err(EvalError::new("--arm is required (once per arm)"));
            }
            let mut arms = Vec::new();
            for path in arm_paths {
                let arm = Arm::load(&PathBuf::from(path))?;
                if arms.iter().any(|a: &Arm| a.id == arm.id) {
                    return Err(EvalError::new(format!("arm id {} twice", arm.id)));
                }
                arms.push(arm);
            }
            let commands = harbor::write_tasks(&set, &arms, &PathBuf::from(p.one("out")?))?;
            Ok(commands.join("\n") + "\n")
        }
        "smoke" => {
            let p = parse(it, &[])?;
            p.only(&["arm", "out"], 0)?;
            let arm = Arm::load(&PathBuf::from(p.one("arm")?))?;
            let command = harbor::write_smoke(&arm, &PathBuf::from(p.one("out")?))?;
            Ok(command + "\n")
        }
        "report" => {
            let p = parse(it, &["json"])?;
            p.only(&[], 1)?;
            let root = PathBuf::from(p.positional.first().map_or("", String::as_str));
            let found = run::read_all(&root)?;
            let r = report::build(&found);
            Ok(if p.switches.iter().any(|s| s == "json") {
                report::to_json(&r)
            } else {
                report::to_markdown(&r)
            })
        }
        "inspect" => {
            let p = parse(it, &[])?;
            p.only(&[], 1)?;
            let dir = PathBuf::from(p.positional.first().map_or("", String::as_str));
            let run = read_trial(&dir)?.ok_or_else(|| {
                EvalError::new(format!(
                    "{}: its task_name is not <arm>__<task> as jfl-eval writes it",
                    dir.display()
                ))
            })?;
            let value = json!({
                "arm": run.arm,
                "task": run.task,
                "verdict": run.verdict.as_str(),
                "source": run.source.map(|s| s.as_str()),
                "metrics": run.metrics,
                "data_quality": run.quality,
                "shapes": run.shapes,
            });
            let mut s = serde_json::to_string_pretty(&value).context("inspect")?;
            s.push('\n');
            Ok(s)
        }
        other => Err(EvalError::new(format!(
            "unknown command {other:?}\n\n{}",
            usage()
        ))),
    }
}
