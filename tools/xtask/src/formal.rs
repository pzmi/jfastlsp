//! The formal stages (FORMAL-01, ver §6, ADR 0007): the `MODELS` registry `formal/models.toml`,
//! the `formal-typecheck` and `formal-test` stages of `check`, and `formal-verify`, which is not
//! a stage (a JVM per model; `formal.yml` runs it weekly and by hand).
//!
//! - **The registry** is read in a pinned line shape, like `tools/versions.toml` (a TOML parser
//!   reads it independently in crates/jfl-conformance/tests/models.rs): `[models.<name>]` tables,
//!   each with exactly `file` (`formal/<name>.qnt`), `main` (the module), `steps` (Apalache's
//!   bound) and `sim_steps` (the simulation's depth, also `conformance-replay`'s).
//!   Every model declares `init`, `step` and `val allInvariants` (ver §6.1).
//! - **`formal-typecheck`**: every `.qnt` under `formal/` must be registered (an unregistered model
//!   would be checked by nothing) and every registered file must exist; then `quint typecheck`
//!   per model.
//! - **`formal-test`**: `quint test --backend typescript --main <main>` per model; a model with
//!   no test fails, since zero tests passing is not a green test stage. The TypeScript backend runs
//!   inside the pinned npm package: Quint's default `rust` backend downloads an evaluator at run
//!   time (fnd §8 rule 5).
//! - **`formal-verify [MODEL]`**: per model (or the one named), Apalache's bounded verification of
//!   `allInvariants` for `steps` steps, then a Quint simulation of the same invariant ([`SAMPLES`]
//!   samples of `sim_steps` steps, its seed printed); for a model with a `progress` entry, then
//!   its liveness simulation and its liveness control ([`Progress`]). Apalache runs on Quint's JSON IR (`quint
//!   compile --target json --out`, checked whole before use: Quint's stdout to a pipe is cut at
//!   64 KiB) directly, in a scratch directory outside the repository (removed on success, kept and named on
//!   failure, for the counterexample): `quint verify` would start Apalache as a gRPC server on a
//!   TCP port and download it unless told otherwise.
//!
//! Each stage fails closed: a missing `node` or `quint` fails it with the install command before
//! anything runs. `formal-verify` probes Node, Quint, the JDK and Apalache at their pinned
//! versions first (`doctor` reports the JDK and Apalache as optional), and works in a fresh
//! private directory (W3 of the FORMAL-01 review: a predictable name on a shared `/tmp` could be
//! planted).
//!
//! Pinned by: the unit tests below and tools/xtask/tests/formal.rs.

use std::path::{Path, PathBuf};

use regex::Regex;

use crate::process::{Call, Context, on_path};
use crate::tools::{Found, probe};
use crate::versions::Tool;

/// The registry, relative to the working directory.
pub(crate) const REGISTRY: &str = "formal/models.toml";

/// What `formal-typecheck` stands for.
pub(crate) const TYPECHECK: &str =
    "quint typecheck <each model of formal/models.toml> (an unregistered formal/**/*.qnt fails)";

/// What `formal-test` stands for.
pub(crate) const TEST: &str = "quint test --backend typescript --main <main> <each model of formal/models.toml> (a model without a test fails)";

/// The invariant, initial action and step every model declares (ver §6.1).
pub(crate) const INVARIANT: &str = "allInvariants";
pub(crate) const INIT: &str = "init";
pub(crate) const STEP: &str = "step";

/// The simulation's sample count in `formal-verify` (FORMAL-03's review: 20k samples of a deep
/// enough simulation find what a short Apalache bound cannot reach).
const SAMPLES: &str = "20000";

/// The fix `formal-verify` names for a missing or wrong tool.
const VERIFY_HINT: &str = "run 'bash scripts/bootstrap-tools.sh --formal-verify' (or 'mise install --locked node npm:@informalsystems/quint java github:apalache-mc/apalache'); just doctor says what else is missing";

/// The fix a missing tool names.
const HINT: &str = "run 'mise install --locked' (or 'bash scripts/bootstrap-tools.sh'); just doctor says what else is missing";

/// One registered model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Model {
    /// The table name, e.g. `smoke`.
    pub(crate) name: String,
    /// The file, relative to the repository root: `formal/<name>.qnt`.
    pub(crate) file: String,
    /// The module the stages run.
    pub(crate) main: String,
    /// Apalache's bound (`--length`): its time grows steeply with it.
    pub(crate) steps: u32,
    /// The simulation's depth (`--max-steps`), in `formal-verify` and in `conformance-replay`'s
    /// generated traces: deep enough for the model's interleavings, which Apalache's bound may
    /// not reach.
    pub(crate) sim_steps: u32,
    /// The liveness simulation `formal-verify` runs after the safety one, if the model has one.
    pub(crate) progress: Option<Progress>,
}

/// A model's liveness simulation (FORMAL-04, doc/formal-models.md): a temporal property Apalache
/// cannot check at a useful bound, reduced to an invariant of a dedicated step. `formal-verify`
/// runs `samples` samples of `steps` steps of `step` against `invariant` and requires no violation,
/// then the same of `control` and requires a violation (a liveness check that cannot fail is
/// caught). Pinned by `tests::should_parse_a_model_when_the_shape_holds` and
/// `tests::should_see_a_violation_only_when_quint_reports_one`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Progress {
    /// The step of the liveness run, e.g. `progressStep`.
    pub(crate) step: String,
    /// The invariant it must keep, e.g. `allProgress`.
    pub(crate) invariant: String,
    /// The broken step whose run must violate it.
    pub(crate) control: String,
    /// The run's depth (`--max-steps`).
    pub(crate) steps: u32,
    /// Its sample count (`--max-samples`).
    pub(crate) samples: u32,
}

/// Parses the registry in its pinned shape: comments, blanks and `[models.<name>]` tables, each
/// with `file`, `main`, `steps` and `sim_steps` exactly once and `progress` at most once; at least
/// one model; no file twice.
pub(crate) fn parse(text: &str) -> Result<Vec<Model>, String> {
    let r = |p: &str| Regex::new(p).map_err(|e| format!("{REGISTRY}: pattern {p}: {e}"));
    let blank = r(r"^[ \t]*(#.*)?$")?;
    let header = r(r"^\[models\.([a-z][a-z0-9_-]*)\]$")?;
    let file = r(r#"^file = "(formal/[a-z0-9_-]+\.qnt)"$"#)?;
    let main = r(r#"^main = "([A-Za-z_][A-Za-z0-9_]*)"$"#)?;
    let steps = r(r"^steps = ([1-9][0-9]{0,3})$")?;
    let sim_steps = r(r"^sim_steps = ([1-9][0-9]{0,3})$")?;
    let progress = r(&format!("^progress = {PROGRESS}$"))?;
    let mut models: Vec<Model> = Vec::new();
    let mut open: Option<(usize, String, [Option<String>; 5])> = None;
    let close = |open: Option<(usize, String, [Option<String>; 5])>,
                 models: &mut Vec<Model>|
     -> Result<(), String> {
        let Some((at, name, [file, main, steps, sim_steps, liveness])) = open else {
            return Ok(());
        };
        let missing = |key: &str| format!("{REGISTRY}:{at}: models.{name} has no `{key}`");
        let file = file.ok_or_else(|| missing("file"))?;
        if models.iter().any(|m| m.file == file || m.name == name) {
            return Err(format!(
                "{REGISTRY}:{at}: models.{name} repeats a name or the file {file}"
            ));
        }
        models.push(Model {
            main: main.ok_or_else(|| missing("main"))?,
            steps: steps
                .ok_or_else(|| missing("steps"))?
                .parse()
                .map_err(|e| format!("{REGISTRY}:{at}: models.{name}.steps: {e}"))?,
            sim_steps: sim_steps
                .ok_or_else(|| missing("sim_steps"))?
                .parse()
                .map_err(|e| format!("{REGISTRY}:{at}: models.{name}.sim_steps: {e}"))?,
            progress: liveness
                .map(|line| parse_progress(&line))
                .transpose()
                .map_err(|e| format!("{REGISTRY}:{at}: models.{name}.progress: {e}"))?,
            file,
            name,
        });
        Ok(())
    };
    for (number, line) in (1..).zip(text.split('\n')) {
        if blank.is_match(line) {
            continue;
        }
        if let Some(caps) = header.captures(line) {
            close(open.take(), &mut models)?;
            let name = caps.get(1).map_or("", |m| m.as_str()).to_owned();
            open = Some((number, name, [None, None, None, None, None]));
            continue;
        }
        let Some((_, _, fields)) = open.as_mut() else {
            return Err(format!(
                "{REGISTRY}:{number}: outside a [models.<name>] table: {line}"
            ));
        };
        let (slot, value) = if let Some(c) = file.captures(line) {
            (0, c.get(1).map(|m| m.as_str()))
        } else if let Some(c) = main.captures(line) {
            (1, c.get(1).map(|m| m.as_str()))
        } else if let Some(c) = steps.captures(line) {
            (2, c.get(1).map(|m| m.as_str()))
        } else if let Some(c) = sim_steps.captures(line) {
            (3, c.get(1).map(|m| m.as_str()))
        } else if progress.is_match(line) {
            (4, line.strip_prefix("progress = "))
        } else {
            return Err(format!(
                "{REGISTRY}:{number}: outside the pinned shape (file = \"formal/<name>.qnt\", main = \"<module>\", steps = <1..9999>, sim_steps = <1..9999>, optionally progress = {{ step = \"<step>\", invariant = \"<invariant>\", control = \"<step>\", steps = <1..9999>, samples = <1..999999> }}): {line}"
            ));
        };
        let Some(field) = fields.get_mut(slot) else {
            return Err(format!("{REGISTRY}:{number}: no such field"));
        };
        if field.replace(value.unwrap_or("").to_owned()).is_some() {
            return Err(format!("{REGISTRY}:{number}: a key twice in one table"));
        }
    }
    close(open.take(), &mut models)?;
    if models.is_empty() {
        return Err(format!("{REGISTRY}: no model registered"));
    }
    Ok(models)
}

/// The pinned shape of a `progress` entry's value, an inline table with its keys in this order.
const PROGRESS: &str = r#"\{ step = "([A-Za-z_][A-Za-z0-9_]*)", invariant = "([A-Za-z_][A-Za-z0-9_]*)", control = "([A-Za-z_][A-Za-z0-9_]*)", steps = ([1-9][0-9]{0,3}), samples = ([1-9][0-9]{0,5}) \}"#;

/// A `progress` entry's value, in [`PROGRESS`]'s shape.
fn parse_progress(text: &str) -> Result<Progress, String> {
    let shape = Regex::new(&format!("^{PROGRESS}$")).map_err(|e| e.to_string())?;
    let caps = shape
        .captures(text)
        .ok_or_else(|| format!("outside the pinned shape: {text}"))?;
    let at = |i: usize| caps.get(i).map_or("", |m| m.as_str()).to_owned();
    let number = |i: usize| at(i).parse::<u32>().map_err(|e| e.to_string());
    Ok(Progress {
        step: at(1),
        invariant: at(2),
        control: at(3),
        steps: number(4)?,
        samples: number(5)?,
    })
}

/// The registry of the working directory.
pub(crate) fn registry() -> Result<Vec<Model>, String> {
    let text =
        std::fs::read_to_string(REGISTRY).map_err(|e| format!("cannot read {REGISTRY}: {e}"))?;
    parse(&text)
}

/// Every `.qnt` file under `dir`, as `dir`-relative paths prefixed with `prefix`, sorted.
fn qnt_files(dir: &Path, prefix: &str, found: &mut Vec<String>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}/{name}");
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => qnt_files(&entry.path(), &rel, found),
            // Not followed, and never registrable (a registered file is `formal/<name>.qnt`), so
            // a model behind a symlinked directory fails the registry check instead of hiding.
            Ok(kind) if kind.is_symlink() && entry.path().is_dir() => found.push(rel),
            Ok(_) if name.ends_with(".qnt") => found.push(rel),
            _ => {}
        }
    }
    found.sort();
}

/// What is wrong between the registry and the files: an unregistered `.qnt`, or a registered file
/// that is missing. Empty when they agree.
pub(crate) fn mismatches(models: &[Model], files: &[String]) -> Vec<String> {
    let unregistered = files
        .iter()
        .filter(|f| !models.iter().any(|m| &m.file == *f))
        .map(|f| format!("{f}: not registered in {REGISTRY}; add a [models.<name>] table for it (or remove it)"));
    let missing = models.iter().filter(|m| !files.contains(&m.file)).map(|m| {
        format!(
            "{}: registered as models.{}, but there is no such file",
            m.file, m.name
        )
    });
    unregistered.chain(missing).collect()
}

/// The number `quint test` reports as passing (`N passing`), or 0.
pub(crate) fn passing(output: &str) -> u32 {
    output
        .lines()
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            let n = words.next()?.parse::<u32>().ok()?;
            (words.next()? == "passing").then_some(n)
        })
        .fold(0, u32::saturating_add)
}

/// Each tool of `tools` on the stages' PATH, or 1 after naming the missing ones and the fix.
pub(crate) fn require(context: &Context, stage: &str, tools: &[&str]) -> Result<(), i32> {
    let missing: Vec<&str> = tools
        .iter()
        .copied()
        .filter(|t| !on_path(t, context.path.as_deref()))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    eprintln!(
        "{stage}: {} missing from the stages' PATH: {HINT}",
        missing.join(" and ")
    );
    Err(1)
}

/// The tools of `tools/versions.toml` that `formal-verify` runs, each probed at its pinned
/// version before anything runs (the JDK and Apalache are optional in `doctor`, so this is where
/// a missing or wrong one is caught).
const VERIFY_TOOLS: [&str; 4] = ["node", "quint", "jdk", "apalache"];

/// Each tool of `names` resolved on the stages' PATH at its pinned version, or 1 after naming
/// each one missing or wrong and the fix.
fn require_pinned(
    context: &Context,
    stage: &str,
    tools: &[Tool],
    names: &[&str],
) -> Result<(), i32> {
    let mut problems = Vec::new();
    for name in names {
        let Some(tool) = tools.iter().find(|t| t.name == *name) else {
            problems.push(format!("{name} is not pinned in tools/versions.toml"));
            continue;
        };
        match probe(tool, context.path.as_deref()) {
            Found::Ok(_) => {}
            Found::Missing => problems.push(format!("{} missing", tool.program())),
            Found::Wrong(at, said) => problems.push(format!(
                "{} says '{said}', not {}",
                at.display(),
                tool.expected_word()
            )),
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    eprintln!("{stage}: {}: {VERIFY_HINT}", problems.join("; "));
    Err(1)
}

/// A new directory `<parent>/<prefix>-<pid>-<n>`, created with `create_dir` (which fails on any
/// existing entry, a planted symlink included, so the first free name is taken and nothing is
/// reused) and made private (0700). Pinned by
/// `should_take_a_fresh_private_directory_when_the_name_is_taken`.
pub(crate) fn unique_dir(parent: &Path, prefix: &str) -> Result<PathBuf, String> {
    for n in 0..1000_u32 {
        let dir = parent.join(format!("{prefix}-{}-{n}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                private(&dir)?;
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("cannot create {}: {e}", dir.display())),
        }
    }
    Err(format!(
        "no free {prefix}-{}-<n> directory in {}",
        std::process::id(),
        parent.display()
    ))
}

#[cfg(unix)]
fn private(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("cannot make {} private: {e}", dir.display()))
}

#[cfg(not(unix))]
fn private(_dir: &Path) -> Result<(), String> {
    Ok(())
}

/// The registry, checked against the `.qnt` files of `formal/`; prints what is wrong.
pub(crate) fn checked_registry(stage: &str) -> Result<Vec<Model>, i32> {
    let models = registry().map_err(|e| {
        eprintln!("{stage}: {e}");
        1
    })?;
    let mut files = Vec::new();
    qnt_files(Path::new("formal"), "formal", &mut files);
    let wrong = mismatches(&models, &files);
    if wrong.is_empty() {
        return Ok(models);
    }
    for problem in &wrong {
        eprintln!("{stage}: {problem}");
    }
    Err(1)
}

/// The `formal-typecheck` stage.
pub(crate) fn typecheck(context: &Context) -> i32 {
    let stage = "formal-typecheck";
    let models = match checked_registry(stage) {
        Ok(models) => models,
        Err(code) => return code,
    };
    if let Err(code) = require(context, stage, &["node", "quint"]) {
        return code;
    }
    for model in models {
        let code = Call::new("quint", &["typecheck", &model.file]).run(context);
        if code != 0 {
            eprintln!("{stage}: {} does not typecheck", model.file);
            return code;
        }
    }
    0
}

/// The `formal-test` stage.
pub(crate) fn test(context: &Context) -> i32 {
    let stage = "formal-test";
    let models = match checked_registry(stage) {
        Ok(models) => models,
        Err(code) => return code,
    };
    if let Err(code) = require(context, stage, &["node", "quint"]) {
        return code;
    }
    for model in models {
        let call = Call::new(
            "quint",
            &[
                "test",
                "--backend",
                "typescript",
                "--main",
                &model.main,
                &model.file,
            ],
        );
        let (code, stdout) = match call.output(context) {
            Ok(result) => result,
            Err(message) => {
                eprintln!("{message}");
                return 1;
            }
        };
        let text = String::from_utf8_lossy(&stdout);
        print!("{text}");
        if code != 0 {
            return code;
        }
        if passing(&text) == 0 {
            eprintln!(
                "{stage}: {} ran no test; every model has at least one (a `run <name>Test`), and zero tests is not green",
                model.file
            );
            return 1;
        }
    }
    0
}

/// `formal-verify [MODEL]`: Apalache and a simulation over each registered model, or the one
/// named. Returns the exit code.
pub(crate) fn verify(context: &Context, tools: &[Tool], only: Option<&str>) -> i32 {
    let stage = "formal-verify";
    let models = match checked_registry(stage) {
        Ok(models) => models,
        Err(code) => return code,
    };
    let chosen: Vec<&Model> = models
        .iter()
        .filter(|m| only.is_none_or(|name| m.name == name))
        .collect();
    if chosen.is_empty() {
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        eprintln!(
            "{stage}: no model {}; registered: {}",
            only.unwrap_or_default(),
            names.join(" ")
        );
        return 2;
    }
    if let Err(code) = require_pinned(context, stage, tools, &VERIFY_TOOLS) {
        return code;
    }
    let scratch = match unique_dir(&std::env::temp_dir(), "jfastlsp-formal-verify") {
        Ok(dir) => dir,
        Err(message) => {
            eprintln!("{stage}: {message}");
            return 1;
        }
    };
    let code = verify_in(context, &chosen, &scratch);
    if code != 0 {
        eprintln!(
            "{stage}: kept {} (Apalache's counterexample, `violation1.itf.json`, is under it)",
            scratch.display()
        );
        return code;
    }
    if let Err(e) = std::fs::remove_dir_all(&scratch)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("{stage}: cannot remove {}: {e}", scratch.display());
    }
    code
}

fn verify_in(context: &Context, models: &[&Model], scratch: &Path) -> i32 {
    let stage = "formal-verify";
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for model in models {
        let dir = scratch.join(&model.name);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("{stage}: cannot create {}: {e}", dir.display());
            return 1;
        }
        let file = root.join(&model.file).to_string_lossy().into_owned();
        let steps = model.steps.to_string();
        eprintln!("{stage}: {}: Apalache, {steps} steps", model.name);
        // Quint writes the IR with `--out`: its stdout is cut at 64 KiB when it is a pipe (Node
        // exits before the pipe drains), which truncated any model larger than the smoke one. The
        // copy it still prints is captured and dropped.
        let json = dir.join(format!("{}.qnt.json", model.name));
        let json_arg = json.to_string_lossy().into_owned();
        let compile = Call::new(
            "quint",
            &[
                "compile",
                "--target",
                "json",
                "--main",
                &model.main,
                "--out",
                &json_arg,
                &file,
            ],
        );
        match compile.output(context) {
            Ok((0, _)) => {}
            Ok((code, _)) => return code,
            Err(message) => {
                eprintln!("{message}");
                return 1;
            }
        }
        let whole = std::fs::read_to_string(&json).map(|text| json_complete(&text));
        if !matches!(whole, Ok(true)) {
            eprintln!(
                "{stage}: quint compile wrote no whole JSON IR to {} ({whole:?})",
                json.display()
            );
            return 1;
        }
        let out = dir.join("apalache-out").to_string_lossy().into_owned();
        let check = Call::new(
            "apalache-mc",
            &[
                "check",
                &format!("--out-dir={out}"),
                &format!("--init={INIT}"),
                &format!("--next={STEP}"),
                &format!("--inv={INVARIANT}"),
                &format!("--length={steps}"),
                &json.to_string_lossy(),
            ],
        )
        .in_dir(&dir);
        let code = check.run(context);
        if code != 0 {
            eprintln!(
                "{stage}: {}: Apalache found a violation of {INVARIANT} or failed (exit {code})",
                model.name
            );
            return code;
        }
        let sim_steps = model.sim_steps.to_string();
        eprintln!(
            "{stage}: {}: simulation, {SAMPLES} samples of {sim_steps} steps",
            model.name
        );
        let code = simulate(model, STEP, INVARIANT, &sim_steps, SAMPLES)
            .in_dir(&dir)
            .run(context);
        if code != 0 {
            return code;
        }
        if let Some(progress) = &model.progress
            && let Err(code) = liveness(context, model, progress, &dir)
        {
            return code;
        }
    }
    0
}

/// `quint run` of `model` from `init`: `samples` samples of `steps` steps of `step`, checking
/// `invariant`, on the TypeScript backend (its seed printed).
fn simulate(model: &Model, step: &str, invariant: &str, steps: &str, samples: &str) -> Call {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let file = root.join(&model.file).to_string_lossy().into_owned();
    Call::new(
        "quint",
        &[
            "run",
            "--backend",
            "typescript",
            "--main",
            &model.main,
            "--init",
            INIT,
            "--step",
            step,
            "--invariant",
            invariant,
            "--max-steps",
            steps,
            "--max-samples",
            samples,
            &file,
        ],
    )
}

/// The model's liveness simulation, which must hold, then its control, which must violate it:
/// a liveness check its control cannot fail is no check. Fails closed: a run that cannot start
/// (an unknown step or invariant) fails the first, and a control that ends without Quint's
/// violation report fails the second.
fn liveness(context: &Context, model: &Model, progress: &Progress, dir: &Path) -> Result<(), i32> {
    let stage = "formal-verify";
    let steps = progress.steps.to_string();
    let samples = progress.samples.to_string();
    eprintln!(
        "{stage}: {}: liveness, {samples} samples of {steps} steps of {} keeping {}",
        model.name, progress.step, progress.invariant
    );
    let code = simulate(model, &progress.step, &progress.invariant, &steps, &samples)
        .in_dir(dir)
        .run(context);
    if code != 0 {
        eprintln!(
            "{stage}: {}: the liveness simulation failed (exit {code})",
            model.name
        );
        return Err(code);
    }
    eprintln!(
        "{stage}: {}: liveness control, {} must violate {}",
        model.name, progress.control, progress.invariant
    );
    match simulate(
        model,
        &progress.control,
        &progress.invariant,
        &steps,
        &samples,
    )
    .in_dir(dir)
    .output(context)
    {
        Ok((code, out)) if code != 0 && violated(&String::from_utf8_lossy(&out)) => Ok(()),
        Ok((code, out)) => {
            let text = String::from_utf8_lossy(&out);
            eprintln!(
                "{stage}: {}: the liveness control {} did not violate {} (exit {code}):\n{text}",
                model.name, progress.control, progress.invariant
            );
            Err(1)
        }
        Err(message) => {
            eprintln!("{message}");
            Err(1)
        }
    }
}

/// Whether Quint's run output reports an invariant violation (and not some other failure).
/// Pinned by `tests::should_see_a_violation_only_when_quint_reports_one`.
pub(crate) fn violated(output: &str) -> bool {
    output
        .lines()
        .any(|l| l.trim_start().starts_with("[violation]"))
}

/// Whether `text` is one whole JSON value: its brackets balance (outside strings, escapes
/// honoured) and close exactly at its end. A truncated IR never is. Pinned by
/// `tests::should_take_only_a_whole_ir_when_its_json_is_checked`.
pub(crate) fn json_complete(text: &str) -> bool {
    let text = text.trim_end();
    let (mut depth, mut in_string, mut escaped, mut closed_at) = (0_usize, false, false, None);
    for (at, c) in text.char_indices() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        if c.is_whitespace() {
            continue;
        }
        if closed_at.is_some() || (depth == 0 && c != '{' && c != '[') {
            return false;
        }
        match c {
            '"' => in_string = true,
            '{' | '[' => depth = depth.saturating_add(1),
            '}' | ']' => {
                let Some(less) = depth.checked_sub(1) else {
                    return false;
                };
                depth = less;
                if depth == 0 {
                    closed_at = Some(at);
                }
            }
            _ => {}
        }
    }
    !in_string && depth == 0 && closed_at.is_some_and(|at| at.saturating_add(1) == text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = "# c\n[models.smoke]\nfile = \"formal/smoke.qnt\"\nmain = \"smoke\"\nsteps = 10\nsim_steps = 25\n";
    const PROGRESS_LINE: &str = "progress = { step = \"progressStep\", invariant = \"allProgress\", control = \"progressStepBroken\", steps = 92, samples = 3000 }";

    #[test]
    fn should_take_only_a_whole_ir_when_its_json_is_checked() {
        // given: an IR over 64 KiB with strings holding brackets, quotes and escapes, and its
        // first 64 KiB, which is what Quint's piped stdout holds
        let entry = r#"{"name":"a\"]}{","value":[1,2,{"k":"\\"}]},"#;
        let whole = format!("{{\"modules\":[{}{{}}]}}\n", entry.repeat(2000));
        let cut = whole.get(..65536).unwrap_or_default();

        // when
        let verdicts = [
            json_complete(&whole),
            json_complete(cut),
            json_complete(""),
            json_complete("{} {}"),
            json_complete("{\"a\":\"}"),
            json_complete("]["),
            json_complete("x{}"),
        ];

        // then
        assert!(whole.len() > 65536);
        assert_eq!(verdicts, [true, false, false, false, false, false, false]);
    }

    #[test]
    fn should_read_the_repository_s_registry_when_it_is_parsed() {
        // given
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(root.join(REGISTRY)).unwrap_or_default();

        // when
        let models = parse(&text);

        // then
        let models = models.unwrap_or_default();
        assert!(models.iter().any(|m| m.name == "smoke" && m.steps > 0));
        let mut files = Vec::new();
        qnt_files(&root.join("formal"), "formal", &mut files);
        assert_eq!(mismatches(&models, &files), Vec::<String>::new());
    }

    #[test]
    fn should_parse_a_model_when_the_shape_holds() {
        // given
        let text = ONE;

        // when
        let models = parse(text);

        // then
        assert_eq!(
            models,
            Ok(vec![Model {
                name: "smoke".to_owned(),
                file: "formal/smoke.qnt".to_owned(),
                main: "smoke".to_owned(),
                steps: 10,
                sim_steps: 25,
                progress: None,
            }])
        );
        let live = parse(&format!("{ONE}{PROGRESS_LINE}\n"));
        assert_eq!(
            live.map(|m| m.into_iter().next().and_then(|m| m.progress)),
            Ok(Some(Progress {
                step: "progressStep".to_owned(),
                invariant: "allProgress".to_owned(),
                control: "progressStepBroken".to_owned(),
                steps: 92,
                samples: 3000,
            }))
        );
    }

    #[test]
    fn should_see_a_violation_only_when_quint_reports_one() {
        // given: Quint's report of a violated invariant, of a clean run, and of an error
        let reports = [
            "An example execution:\n...\n[violation] Found an issue (92ms at 11 traces/second).\n",
            "[ok] No violation found (3054ms at 98 traces/second).\n",
            "error: [QNT404] Name 'progressStepBroken' not found\n",
            "",
        ];

        // when
        let verdicts = reports.map(violated);

        // then
        assert_eq!(verdicts, [true, false, false, false]);
    }

    #[test]
    fn should_refuse_the_registry_when_a_line_leaves_the_shape() {
        // given: each edit, and what the refusal names
        let edits: [(&str, &str, &str); 10] = [
            (
                "formal/smoke.qnt",
                "../smoke.qnt",
                "outside the pinned shape",
            ),
            (
                "formal/smoke.qnt",
                "formal/x/smoke.qnt",
                "outside the pinned shape",
            ),
            ("steps = 10", "steps = 0", "outside the pinned shape"),
            ("steps = 10", "steps = \"10\"", "outside the pinned shape"),
            ("main = \"smoke\"\n", "", "has no `main`"),
            ("sim_steps = 25\n", "", "has no `sim_steps`"),
            (
                "sim_steps = 25",
                "sim_steps = 0",
                "outside the pinned shape",
            ),
            ("steps = 10", "steps = 10\nsteps = 11", "twice"),
            (
                "# c",
                "file = \"formal/a.qnt\"",
                "outside a [models.<name>] table",
            ),
            (
                "[models.smoke]",
                "[models.Smoke]",
                "outside a [models.<name>] table",
            ),
        ];

        // when
        let results: Vec<_> = edits
            .iter()
            .map(|(from, to, _)| parse(&ONE.replacen(from, to, 1)))
            .collect();

        // then
        for ((from, to, why), result) in edits.iter().zip(&results) {
            let refusal = result.as_ref().err().cloned().unwrap_or_default();
            assert!(refusal.contains(why), "{from} -> {to}: {result:?}");
        }
        assert!(parse("# none\n").is_err_and(|e| e.contains("no model")));
        let twice =
            format!("{ONE}[models.other]\nfile = \"formal/smoke.qnt\"\nmain = \"o\"\nsteps = 1\n");
        assert!(parse(&twice).is_err_and(|e| e.contains("repeats")));
        let progress_refusals = [
            PROGRESS_LINE.replace("control = \"progressStepBroken\", ", ""),
            PROGRESS_LINE.replace("samples = 3000", "samples = 0"),
            PROGRESS_LINE.replace("steps = 92, samples", "samples = 3000, steps"),
        ];
        for line in &progress_refusals {
            assert!(
                parse(&format!("{ONE}{line}\n"))
                    .is_err_and(|e| e.contains("outside the pinned shape")),
                "{line}"
            );
        }
        assert!(
            parse(&format!("{ONE}{PROGRESS_LINE}\n{PROGRESS_LINE}\n"))
                .is_err_and(|e| e.contains("twice"))
        );
    }

    #[test]
    fn should_name_the_unregistered_and_the_missing_when_registry_and_files_differ() {
        // given
        let models = parse(ONE).unwrap_or_default();
        let files = vec!["formal/extra.qnt".to_owned()];

        // when
        let found = mismatches(&models, &files);

        // then
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found
                .iter()
                .any(|f| f.starts_with("formal/extra.qnt: not registered"))
        );
        assert!(
            found
                .iter()
                .any(|f| f.starts_with("formal/smoke.qnt: registered"))
        );
    }

    #[test]
    fn should_take_a_fresh_private_directory_when_the_name_is_taken() {
        // given: the first name already taken, by a symlink to elsewhere
        let parent = std::env::temp_dir().join(format!("xtask-unique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&parent);
        let _ = std::fs::create_dir_all(parent.join("elsewhere"));
        let taken = parent.join(format!("p-{}-0", std::process::id()));
        let _ = std::os::unix::fs::symlink(parent.join("elsewhere"), &taken);

        // when
        let first = unique_dir(&parent, "p");
        let second = unique_dir(&parent, "p");

        // then
        let mode = first
            .as_ref()
            .ok()
            .and_then(|d| std::fs::metadata(d).ok())
            .map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o777);
        let _ = std::fs::remove_dir_all(&parent);
        assert_eq!(
            first,
            Ok(parent.join(format!("p-{}-1", std::process::id())))
        );
        assert_eq!(
            second,
            Ok(parent.join(format!("p-{}-2", std::process::id())))
        );
        assert_eq!(mode, Some(0o700));
    }

    #[test]
    fn should_count_the_passing_tests_when_quint_reports_them() {
        // given
        let reports = [
            (
                "\n  smoke\n    ok countsUpTest passed 1 test(s)\n\n  1 passing (6ms)\n",
                1,
            ),
            ("\n  empty\n\n", 0),
            ("  2 passing (1ms)\n  1 failed\n", 2),
            ("passing 3\n", 0),
        ];

        // when
        let counted: Vec<u32> = reports.iter().map(|(text, _)| passing(text)).collect();

        // then
        let expected: Vec<u32> = reports.iter().map(|(_, n)| *n).collect();
        assert_eq!(counted, expected);
    }
}
