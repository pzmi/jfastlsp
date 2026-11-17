//! The `jfl-conformance` binary (ver §6.2 item 2): `replay [--model NAME] [--seed SEED
//! --generated DIR]` replays the committed regression traces and, with `--generated`, the seeded
//! traces xtask's `conformance-replay` stage (or `just conformance`) wrote under `DIR/<model>/`,
//! through each model's driver, then the driver's negative control over the same generated
//! traces, which must diverge on one of them. It runs no Quint itself (this crate holds no
//! process grant): xtask generates, this replays.
//!
//! Exit codes: 0 every trace conforms; 1 a divergence (`DIVERGENCE: ...`, with model, seed, trace,
//! step, action, expected, actual and a diff); 2 usage; 3 a harness error (`HARNESS_ERROR: ...`):
//! a malformed trace, a model without a driver or without a committed regression trace, a
//! pending model whose task is unknown or done, a generated model with no trace, or a negative
//! control that passed. A pending model is named in a warning on stderr. Pinned by
//! tests/replay.rs.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::HarnessError;
use crate::manifest;
use crate::registry;
use crate::replay::{self, Context, Failure};
use crate::trace;

/// The usage text.
pub const USAGE: &str = "usage: jfl-conformance replay [--model NAME] [--seed SEED --generated DIR]
  replays formal/traces/regression (every model, or NAME's) and, with --generated, DIR/<model>/*.itf.json
  exit: 0 conforms, 1 divergence, 2 usage, 3 harness error";

/// Every trace conformed.
pub const OK: u8 = 0;
/// A divergence.
pub const DIVERGED: u8 = 1;
/// Usage.
pub const USAGE_ERROR: u8 = 2;
/// A harness error.
pub const HARNESS: u8 = 3;

/// A parsed `replay` command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    /// Only this model.
    pub model: Option<String>,
    /// The generated traces and their seed.
    pub generated: Option<(String, PathBuf)>,
}

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Replay, String> {
    let Some((command, mut rest)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };
    if command != "replay" {
        return Err(USAGE.to_owned());
    }
    let (mut model, mut seed, mut dir) = (None, None, None);
    while let [flag, value, tail @ ..] = rest {
        let slot = match flag.as_str() {
            "--model" => &mut model,
            "--seed" => &mut seed,
            "--generated" => &mut dir,
            _ => return Err(USAGE.to_owned()),
        };
        if value.starts_with('-') || slot.replace(value.clone()).is_some() {
            return Err(USAGE.to_owned());
        }
        rest = tail;
    }
    if !rest.is_empty() {
        return Err(USAGE.to_owned());
    }
    let generated = match (seed, dir) {
        (Some(seed), Some(dir)) => Some((seed, PathBuf::from(dir))),
        (None, None) => None,
        _ => return Err(USAGE.to_owned()),
    };
    Ok(Replay { model, generated })
}

/// Runs `args` against the repository at `root`, printing progress to `out` and failures to
/// `err`; returns the exit code.
pub fn run(args: &[String], root: &Path, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let command = match parse(args) {
        Ok(command) => command,
        Err(usage) => {
            let _ = writeln!(err, "{usage}");
            return USAGE_ERROR;
        }
    };
    match replay_all(&command, root, out, err) {
        Ok(()) => OK,
        Err(failure) => {
            let _ = writeln!(err, "{failure}");
            match failure {
                Failure::Diverged(_) => DIVERGED,
                Failure::Harness(_) => HARNESS,
            }
        }
    }
}

fn replay_all(
    command: &Replay,
    root: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), Failure> {
    let models = registry::models(root)?;
    registry::check(&models)?;
    registry::check_pending(&registry::tasks(root)?)?;
    let chosen: Vec<&String> = models
        .iter()
        .filter(|m| command.model.as_ref().is_none_or(|only| only == *m))
        .collect();
    for model in &chosen {
        if let Some(task) = registry::pending(model) {
            let _ = writeln!(
                err,
                "warning: {model} is pending: its driver lands with {task}; its traces are read, not replayed"
            );
        }
    }
    if chosen.is_empty() {
        return Err(HarnessError::new(format!(
            "no model {} in {}; registered: {}",
            command.model.as_deref().unwrap_or_default(),
            registry::MODELS,
            models.join(" ")
        ))
        .into());
    }
    if let Some((seed, dir)) = &command.generated {
        for model in &chosen {
            generated(model, seed, dir, out)?;
        }
    }
    let regressions = manifest::read(root)?;
    if let Some(bare) = chosen
        .iter()
        .find(|m| !regressions.iter().any(|r| &r.model == **m))
    {
        return Err(HarnessError::new(format!(
            "{bare} has no committed regression trace in {}/{} (plan §7: every model has one)",
            manifest::DIR,
            manifest::MANIFEST
        ))
        .into());
    }
    for regression in &regressions {
        if !models.contains(&regression.model) {
            return Err(HarnessError::new(format!(
                "{}: traces.{}: the model {} is not in {}",
                manifest::DIR,
                regression.name,
                regression.model,
                registry::MODELS
            ))
            .into());
        }
        if !chosen.contains(&&regression.model) {
            continue;
        }
        let file = regression.file.display().to_string();
        let steps = trace::read(&root.join(&regression.file), &file)?;
        let context = Context {
            model: &regression.model,
            seed: &regression.seed,
            trace: &file,
        };
        match registry::driver(&regression.model) {
            Some(entry) => {
                let n = replay::replay((entry.driver)(), &context, steps)?;
                let _ = writeln!(
                    out,
                    "conformance: {}: {file}: {n} states conform",
                    regression.model
                );
            }
            None => {
                let task = registry::pending(&regression.model).unwrap_or_default();
                let _ = writeln!(
                    out,
                    "conformance: {}: {file}: read, not replayed (its driver lands with {task})",
                    regression.model
                );
            }
        }
    }
    Ok(())
}

/// Reads `dir/<model>/*.itf.json` (failing closed on any that is not a trace) and replays them
/// through the model's driver, then its negative control; a pending model's are read only.
fn generated(model: &str, seed: &str, dir: &Path, out: &mut dyn Write) -> Result<(), Failure> {
    let files = trace_files(&dir.join(model))?;
    let mut traces = Vec::with_capacity(files.len());
    for file in &files {
        let name = file.display().to_string();
        let steps = trace::read(file, &name)?;
        traces.push((name, steps));
    }
    let Some(entry) = registry::driver(model) else {
        let task = registry::pending(model).unwrap_or_default();
        let _ = writeln!(
            out,
            "conformance: {model}: {} generated traces read, not replayed (its driver lands with {task})",
            traces.len()
        );
        return Ok(());
    };
    let mut states = 0_usize;
    for (name, steps) in &traces {
        let context = Context {
            model,
            seed,
            trace: name,
        };
        states = states.saturating_add(replay::replay((entry.driver)(), &context, steps.clone())?);
    }
    let _ = writeln!(
        out,
        "conformance: {model}: seed {seed}: {} generated traces ({states} states) conform",
        traces.len()
    );
    let caught = traces.iter().find_map(|(name, steps)| {
        let context = Context {
            model,
            seed,
            trace: name,
        };
        match replay::replay((entry.control)(), &context, steps.clone()) {
            Err(Failure::Diverged(divergence)) => Some(divergence),
            _ => None,
        }
    });
    let Some(divergence) = caught else {
        return Err(HarnessError::new(format!(
            "{model}: the negative control conformed on all {} generated traces of seed {seed}: they do not exercise the driver (a longer budget?)",
            traces.len()
        ))
        .into());
    };
    let _ = writeln!(
        out,
        "conformance: {model}: the negative control diverged as it must, at step {} of {}",
        divergence.step, divergence.trace
    );
    Ok(())
}

/// The `.itf.json` files of `dir`, sorted; none, or anything else there, is a harness error.
fn trace_files(dir: &Path) -> Result<Vec<PathBuf>, HarnessError> {
    let shown = dir.display();
    let entries = std::fs::read_dir(dir)
        .map_err(|e| HarnessError::new(format!("{shown}: cannot list: {e}")))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| HarnessError::new(format!("{shown}: cannot list: {e}")))?;
        let path = entry.path();
        let is_trace = entry.file_type().is_ok_and(|t| t.is_file())
            && entry
                .file_name()
                .to_string_lossy()
                .ends_with(manifest::SUFFIX);
        if !is_trace {
            return Err(HarnessError::new(format!(
                "{}: not a trace file (*{})",
                path.display(),
                manifest::SUFFIX
            )));
        }
        files.push(path);
    }
    if files.is_empty() {
        return Err(HarnessError::new(format!(
            "{shown}: no generated trace (an empty run never passes)"
        )));
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn should_parse_the_replay_command_when_its_flags_are_well_formed() {
        // given
        let lines = [
            (vec!["replay"], Some((None, None))),
            (
                vec!["replay", "--model", "smoke"],
                Some((Some("smoke"), None)),
            ),
            (
                vec!["replay", "--seed", "0x2a", "--generated", "/g"],
                Some((None, Some(("0x2a", "/g")))),
            ),
            (vec![], None),
            (vec!["run"], None),
            (vec!["replay", "--seed", "0x2a"], None),
            (vec!["replay", "--generated", "/g"], None),
            (vec!["replay", "--model", "a", "--model", "b"], None),
            (vec!["replay", "--model"], None),
            (vec!["replay", "--model", "--seed"], None),
            (vec!["replay", "--force", "x"], None),
        ];

        // when
        let parsed: Vec<_> = lines.iter().map(|(words, _)| parse(&args(words))).collect();

        // then
        for ((words, want), got) in lines.iter().zip(parsed) {
            let want = want.map(|(model, generated)| Replay {
                model: model.map(str::to_owned),
                generated: generated.map(|(s, d)| (s.to_owned(), PathBuf::from(d))),
            });
            assert_eq!(got.ok(), want, "{words:?}");
        }
    }
}
