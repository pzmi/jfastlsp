//! The `jfl-conformance` binary (ver §6.2 item 2): `replay [--model NAME] [--seed SEED
//! --generated DIR]` replays the committed regression traces and, with `--generated`, the seeded
//! traces xtask's `conformance-replay` stage (or `just conformance`) wrote under `DIR/<model>/`,
//! through each model's driver, then the driver's negative control over those generated traces
//! and the model's committed regression traces, which must diverge on one of them: the committed
//! traces make the control's divergence independent of the seed, so a run cannot fail by the
//! luck of its batch ([`control_diverges`]). It runs no Quint itself (this crate holds no
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
            generated(model, seed, dir, root, out)?;
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
/// through the model's driver, then its negative control over them and the model's committed
/// regression traces ([`control_diverges`]); a pending model's are read only.
fn generated(
    model: &str,
    seed: &str,
    dir: &Path,
    root: &Path,
    out: &mut dyn Write,
) -> Result<(), Failure> {
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
    let mut committed = Vec::new();
    for regression in manifest::read(root)?.iter().filter(|r| r.model == model) {
        let name = regression.file.display().to_string();
        let steps = trace::read(&root.join(&regression.file), &name)?;
        committed.push(Tried {
            name,
            seed: regression.seed.clone(),
            steps,
        });
    }
    let generated: Vec<Tried> = traces
        .into_iter()
        .map(|(name, steps)| Tried {
            name,
            seed: seed.to_owned(),
            steps,
        })
        .collect();
    let divergence = control_diverges(entry.control, model, seed, &generated, &committed)?;
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

/// One trace a negative control is tried on.
#[derive(Debug, Clone)]
pub struct Tried {
    /// The trace file.
    pub name: String,
    /// The seed it was generated with.
    pub seed: String,
    /// Its steps.
    pub steps: Vec<trace::RawStep>,
}

/// The first divergence of `control` over the `generated` traces (of `seed`), then the model's
/// `committed` regression traces: a control must diverge on one of them, or it proves nothing
/// (ver §6.2 item 4). The committed traces are fixed, so a control that diverges on one of them
/// passes whatever the batch the seed drew. A harness error names every trace tried.
///
/// Pinned by: tests/replay.rs::should_pass_the_control_when_it_diverges_only_on_a_regression_trace
/// and `cli::tests::should_name_every_trace_tried_when_the_control_never_diverges`.
pub fn control_diverges(
    control: fn() -> replay::Replayer,
    model: &str,
    seed: &str,
    generated: &[Tried],
    committed: &[Tried],
) -> Result<replay::Divergence, HarnessError> {
    let caught = generated.iter().chain(committed).find_map(|tried| {
        let context = Context {
            model,
            seed: &tried.seed,
            trace: &tried.name,
        };
        match replay::replay(control(), &context, tried.steps.clone()) {
            Err(Failure::Diverged(divergence)) => Some(*divergence),
            _ => None,
        }
    });
    caught.ok_or_else(|| {
        HarnessError::new(format!(
            "{model}: the negative control conformed on all {} generated traces of seed {seed} and all {} committed regression traces ({}): they do not exercise the driver",
            generated.len(),
            committed.len(),
            committed
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
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

    use crate::smoke;

    const THREE: &str = r##"{"#meta":{"format":"ITF"},"vars":["count","mbt::actionTaken","mbt::nondetPicks"],"states":[{"#meta":{"index":0},"count":{"#bigint":"0"},"mbt::actionTaken":"init","mbt::nondetPicks":{}},{"#meta":{"index":1},"count":{"#bigint":"1"},"mbt::actionTaken":"step","mbt::nondetPicks":{}},{"#meta":{"index":2},"count":{"#bigint":"2"},"mbt::actionTaken":"step","mbt::nondetPicks":{}}]}"##;

    #[test]
    fn should_name_every_trace_tried_when_the_control_never_diverges() -> Result<(), HarnessError> {
        // given: traces too short to reach the value the smoke control skips
        let short = |name: &str| -> Result<Tried, HarnessError> {
            Ok(Tried {
                name: name.to_owned(),
                seed: "0x1".to_owned(),
                steps: trace::parse(name, THREE)?,
            })
        };
        let generated = [short("trace_0.itf.json")?, short("trace_1.itf.json")?];
        let committed = [short("formal/traces/regression/short.itf.json")?];

        // when
        let caught = control_diverges(smoke::control, "smoke", "0x7", &generated, &committed);

        // then
        let Err(error) = caught else {
            return Err(HarnessError::new(
                "the control diverged on a trace too short to",
            ));
        };
        assert!(
            error.to_string().contains(
                "smoke: the negative control conformed on all 2 generated traces of seed 0x7 and all 1 committed regression traces (formal/traces/regression/short.itf.json)"
            ),
            "{error}"
        );
        Ok(())
    }
}
