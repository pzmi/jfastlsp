//! The conformance stages (FORMAL-02, ver §6.2, ADR 0007 as amended): `formal-freshness` and
//! `conformance-replay`, both stages of `check`, and `conformance [MODEL] [SEED] [TRACES]
//! [STEPS]`, which repeats a run (`just conformance smoke 0x2a 10 25`).
//!
//! - **The regression manifest** `formal/traces/regression/manifest.toml` is read in a pinned
//!   line shape, like the MODELS registry (crates/jfl-conformance/src/manifest.rs reads it again
//!   with a TOML parser): `[traces.<name>]` tables, each with exactly `model` (a registered
//!   model), `main`, `init`, `step`, `invariant`, `max_steps` and `seed`; every `<name>.itf.json`
//!   of the directory is listed, every listed one is there, and every registered model has one
//!   (plan §7).
//! - **`formal-freshness`** regenerates each committed trace with `quint run --mbt` on the
//!   TypeScript backend at its seed, and fails unless the new file equals the committed one byte
//!   for byte outside its top-level `#meta` (Quint's timestamp and source path), naming each stale
//!   trace and the command that regenerates it. A committed file that is not a trace Quint wrote
//!   is stale too. Pinned by tools/xtask/tests/conformance.rs.
//! - **`conformance-replay`** generates, per registered model, [`TRACES`] traces of the model's
//!   `sim_steps` steps (formal/models.toml: the simulation's depth, not Apalache's bound) with `quint run --mbt` at one seed (fresh unless given; printed with the command that
//!   repeats the run), generates them a second time and fails unless both runs are equal outside
//!   their `#meta` (the seed reproduces the run), then runs `jfl-conformance replay` over them and
//!   the committed traces (cargo, with the gate's compiler flags). The traces live in a fresh
//!   private scratch directory outside the repository, removed on success and kept and named on
//!   failure. xtask generates and jfl-conformance replays: that crate has no process grant.
//!
//! Each fails closed: a missing `node` or `quint` fails it with the install command before
//! anything runs; a malformed manifest or trace, an unlisted trace, a Quint failure (an invariant
//! violated while generating included) or a replay that is not green fails it.

use std::hash::{BuildHasher as _, RandomState};
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::catalog;
use crate::formal::{self, Model};
use crate::process::{Call, Context};

/// The committed regression traces, relative to the working directory.
pub(crate) const DIR: &str = "formal/traces/regression";

/// The manifest's file name in [`DIR`].
pub(crate) const MANIFEST: &str = "manifest.toml";

/// A trace file's suffix.
const SUFFIX: &str = ".itf.json";

/// The traces each model gets per run: the budget of ver §6.2 item 2, to be measured and
/// ratcheted. Their steps are each model's `sim_steps`.
pub(crate) const TRACES: u32 = 10;

/// The largest TRACES or STEPS `conformance` accepts.
const MOST: u32 = 1000;

/// What `formal-freshness` stands for.
pub(crate) const FRESHNESS: &str = "quint run --backend typescript --mbt --seed <seed> <each trace of formal/traces/regression/manifest.toml>, equal byte for byte to the committed trace outside its #meta";

/// What `conformance-replay` stands for.
pub(crate) const REPLAY: &str = "quint run --backend typescript --mbt --seed <fresh seed, printed> <each model of formal/models.toml> (10 traces of the model's sim_steps steps, generated twice and equal), then cargo run --locked -q -p jfl-conformance -- replay --seed <seed> --generated <dir> (and formal/traces/regression)";

/// One committed regression trace, as its manifest table says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Regression {
    /// The table's name; the file is `<name>.itf.json`.
    pub(crate) name: String,
    /// The model, a name of the MODELS registry.
    pub(crate) model: String,
    /// The module.
    pub(crate) main: String,
    /// The init action.
    pub(crate) init: String,
    /// The step action.
    pub(crate) step: String,
    /// The invariant checked while generating.
    pub(crate) invariant: String,
    /// `--max-steps`.
    pub(crate) max_steps: u32,
    /// `--seed`.
    pub(crate) seed: String,
}

/// A seed Quint takes: `0x` and 1 to 16 lowercase hex digits, or 1 to 20 decimal digits.
pub(crate) fn valid_seed(seed: &str) -> bool {
    let hex = seed.strip_prefix("0x").is_some_and(|h| {
        (1..=16).contains(&h.len()) && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    });
    let decimal = (1..=20).contains(&seed.len()) && seed.bytes().all(|b| b.is_ascii_digit());
    hex || decimal
}

/// A fresh seed: 64 bits from std's per-process random hash keys (no clock, no new dependency).
pub(crate) fn random_seed() -> String {
    format!("0x{:016x}", RandomState::new().hash_one(std::process::id()))
}

/// The manifest's keys, each with the shape of its value.
const KEYS: [(&str, &str); 7] = [
    ("model", r#"^"([a-z][a-z0-9_-]*)"$"#),
    ("main", r#"^"([A-Za-z_][A-Za-z0-9_]*)"$"#),
    ("init", r#"^"([A-Za-z_][A-Za-z0-9_]*)"$"#),
    ("step", r#"^"([A-Za-z_][A-Za-z0-9_]*)"$"#),
    ("invariant", r#"^"([A-Za-z_][A-Za-z0-9_]*)"$"#),
    ("max_steps", r"^([1-9][0-9]{0,3})$"),
    ("seed", r#"^"(0x[0-9a-f]{1,16}|[0-9]{1,20})"$"#),
];

/// A manifest table as read: its line, its name and its `key = value` pairs.
type Table = (usize, String, Vec<(String, String)>);

/// Parses the manifest in its pinned shape: comments, blanks and `[traces.<name>]` tables, each
/// with every key of [`KEYS`] exactly once; no name twice. An empty manifest parses; [`mismatches`]
/// then names every registered model as uncovered.
pub(crate) fn parse_manifest(text: &str) -> Result<Vec<Regression>, String> {
    let at = format!("{DIR}/{MANIFEST}");
    let r = |p: &str| Regex::new(p).map_err(|e| format!("{at}: pattern {p}: {e}"));
    let blank = r(r"^[ \t]*(#.*)?$")?;
    let header = r(r"^\[traces\.([a-z][a-z0-9-]*)\]$")?;
    let line = r(r"^([a-z_]+) = (.+)$")?;
    let shapes: Vec<(&str, Regex)> = KEYS
        .iter()
        .map(|(k, p)| r(p).map(|re| (*k, re)))
        .collect::<Result<_, _>>()?;
    let mut tables: Vec<Table> = Vec::new();
    for (number, text) in (1..).zip(text.split('\n')) {
        if blank.is_match(text) {
            continue;
        }
        if let Some(caps) = header.captures(text) {
            let name = caps.get(1).map_or("", |m| m.as_str()).to_owned();
            if tables.iter().any(|(_, n, _)| *n == name) {
                return Err(format!("{at}:{number}: traces.{name} twice"));
            }
            tables.push((number, name, Vec::new()));
            continue;
        }
        let Some((_, name, fields)) = tables.last_mut() else {
            return Err(format!(
                "{at}:{number}: outside a [traces.<name>] table: {text}"
            ));
        };
        let (key, value) = line
            .captures(text)
            .and_then(|c| Some((c.get(1)?.as_str(), c.get(2)?.as_str())))
            .ok_or_else(|| format!("{at}:{number}: outside the pinned shape: {text}"))?;
        let Some((_, shape)) = shapes.iter().find(|(k, _)| *k == key) else {
            return Err(format!(
                "{at}:{number}: traces.{name}: no key `{key}` in the pinned shape"
            ));
        };
        let Some(value) = shape.captures(value).and_then(|c| c.get(1)) else {
            return Err(format!(
                "{at}:{number}: traces.{name}.{key} is outside its shape ({}): {value}",
                shape.as_str()
            ));
        };
        if fields.iter().any(|(k, _)| k == key) {
            return Err(format!("{at}:{number}: traces.{name}.{key} twice"));
        }
        fields.push((key.to_owned(), value.as_str().to_owned()));
    }
    tables
        .into_iter()
        .map(|(number, name, fields)| {
            let get = |key: &str| {
                fields
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| format!("{at}:{number}: traces.{name} has no `{key}`"))
            };
            Ok(Regression {
                model: get("model")?,
                main: get("main")?,
                init: get("init")?,
                step: get("step")?,
                invariant: get("invariant")?,
                max_steps: get("max_steps")?
                    .parse()
                    .map_err(|e| format!("{at}:{number}: traces.{name}.max_steps: {e}"))?,
                seed: get("seed")?,
                name,
            })
        })
        .collect()
}

/// What is wrong between the manifest's traces and the files of [`DIR`] (`files`: names other
/// than the manifest), and with the models: a model they name that is not registered, or a
/// registered model with no regression trace (an empty manifest is never a fresh one). Empty when
/// they agree.
pub(crate) fn mismatches(traces: &[Regression], files: &[String], models: &[Model]) -> Vec<String> {
    let unlisted = files
        .iter()
        .filter(|f| !traces.iter().any(|t| format!("{}{SUFFIX}", t.name) == **f))
        .map(|f| format!("{DIR}/{f}: not listed in {DIR}/{MANIFEST} (every file there is the manifest or a listed <name>{SUFFIX})"));
    let missing = traces
        .iter()
        .filter(|t| !files.contains(&format!("{}{SUFFIX}", t.name)))
        .map(|t| {
            format!(
                "{DIR}/{}{SUFFIX}: listed as traces.{}, but there is no such file",
                t.name, t.name
            )
        });
    let unknown = traces
        .iter()
        .filter(|t| !models.iter().any(|m| m.name == t.model))
        .map(|t| format!("{DIR}/{MANIFEST}: traces.{} names the model {}, which formal/models.toml does not register", t.name, t.model));
    let uncovered = models
        .iter()
        .filter(|m| !traces.iter().any(|t| t.model == m.name))
        .map(|m| format!("{DIR}/{MANIFEST}: the model {} has no regression trace (plan §7: every model has one; generate it with quint run --mbt and list it)", m.name));
    unlisted
        .chain(missing)
        .chain(unknown)
        .chain(uncovered)
        .collect()
}

/// The text after a trace's top-level `#meta` object, which holds Quint's timestamp and source
/// path: what freshness compares. `Err` when the text does not start the way Quint writes a trace
/// (`{"#meta":{`) or the object never closes.
pub(crate) fn strip_meta(text: &str) -> Result<&str, String> {
    let meta = text
        .strip_prefix(r##"{"#meta":"##)
        .filter(|rest| rest.starts_with('{'))
        .ok_or("it does not start with {\"#meta\":{ as Quint writes a trace")?;
    let (mut depth, mut in_string, mut escaped) = (0_usize, false, false);
    for (at, c) in meta.char_indices() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return meta
                        .get(at.saturating_add(1)..)
                        .ok_or_else(|| "its #meta ends at a bad offset".to_owned());
                }
            }
            _ => {}
        }
    }
    Err("its #meta never closes".to_owned())
}

/// The files of [`DIR`] but the manifest, sorted; an entry that is not a regular file is named
/// as such, so it fails the listing check.
fn regression_files() -> Result<Vec<String>, String> {
    let read = std::fs::read_dir(DIR).map_err(|e| format!("cannot list {DIR}: {e}"))?;
    let mut files = Vec::new();
    for entry in read {
        let entry = entry.map_err(|e| format!("cannot list {DIR}: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let regular = entry.file_type().is_ok_and(|t| t.is_file());
        match (name == MANIFEST, regular) {
            (true, true) => {}
            (_, true) => files.push(name),
            (_, false) => files.push(format!("{name} (not a regular file)")),
        }
    }
    files.sort();
    Ok(files)
}

/// The manifest, checked against its directory and the registry; prints what is wrong.
fn checked_manifest(stage: &str, models: &[Model]) -> Result<Vec<Regression>, i32> {
    let fail = |message: String| {
        eprintln!("{stage}: {message}");
        1
    };
    let text = std::fs::read_to_string(format!("{DIR}/{MANIFEST}"))
        .map_err(|e| fail(format!("cannot read {DIR}/{MANIFEST}: {e}")))?;
    let traces = parse_manifest(&text).map_err(fail)?;
    let files = regression_files().map_err(fail)?;
    let wrong = mismatches(&traces, &files, models);
    if wrong.is_empty() {
        return Ok(traces);
    }
    for problem in wrong {
        eprintln!("{stage}: {problem}");
    }
    Err(1)
}

/// `quint run` on the TypeScript backend with Quint's `--mbt` metadata, quiet, writing ITF to
/// `out`; `extra` are the run's own flags.
fn quint_run(file: &str, out: &Path, extra: &[&str]) -> Call {
    let out = out.to_string_lossy();
    let mut args = vec![
        "run",
        "--backend",
        "typescript",
        "--mbt",
        "--verbosity",
        "0",
    ];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["--out-itf", &out, file]);
    Call::new("quint", &args)
}

/// Removes `dir`, saying so when it cannot.
fn remove(stage: &str, dir: &Path) {
    if let Err(e) = std::fs::remove_dir_all(dir)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("{stage}: cannot remove {}: {e}", dir.display());
    }
}

/// The `formal-freshness` stage.
pub(crate) fn freshness(context: &Context) -> i32 {
    let stage = "formal-freshness";
    let models = match formal::checked_registry(stage) {
        Ok(models) => models,
        Err(code) => return code,
    };
    let traces = match checked_manifest(stage, &models) {
        Ok(traces) => traces,
        Err(code) => return code,
    };
    if let Err(code) = formal::require(context, stage, &["node", "quint"]) {
        return code;
    }
    let scratch = match formal::unique_dir(&std::env::temp_dir(), "jfastlsp-formal-freshness") {
        Ok(dir) => dir,
        Err(message) => {
            eprintln!("{stage}: {message}");
            return 1;
        }
    };
    let mut stale = Vec::new();
    for trace in &traces {
        let Some(model) = models.iter().find(|m| m.name == trace.model) else {
            continue;
        };
        let committed = format!("{DIR}/{}{SUFFIX}", trace.name);
        let fresh = scratch.join(format!("{}{SUFFIX}", trace.name));
        let max_steps = trace.max_steps.to_string();
        let flags = [
            "--main",
            trace.main.as_str(),
            "--init",
            trace.init.as_str(),
            "--step",
            trace.step.as_str(),
            "--invariant",
            trace.invariant.as_str(),
            "--max-steps",
            max_steps.as_str(),
            "--seed",
            trace.seed.as_str(),
        ];
        let code = quint_run(&model.file, &fresh, &flags).run(context);
        if code != 0 {
            eprintln!(
                "{stage}: quint run failed for {committed} (exit {code}); kept {}",
                scratch.display()
            );
            return code;
        }
        let read = |path: &Path| {
            std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))
        };
        let verdict = read(Path::new(&committed)).and_then(|old| {
            let new = read(&fresh)?;
            let new = strip_meta(&new).map_err(|e| format!("Quint's new trace: {e}"))?;
            let old = strip_meta(&old).map_err(|e| format!("the committed trace: {e}"))?;
            Ok(old == new)
        });
        let problem = match verdict {
            Ok(true) => continue,
            Ok(false) => format!(
                "{committed} is stale: Quint now generates {} at seed {}",
                fresh.display(),
                trace.seed
            ),
            Err(why) => format!("{committed} cannot be compared: {why}"),
        };
        stale.push(problem);
        let line = quint_run(&model.file, Path::new(&committed), &flags).display();
        stale.push(format!(
            "  regenerate it (and review the diff) with: {line}"
        ));
    }
    if !stale.is_empty() {
        for problem in &stale {
            eprintln!("{stage}: {problem}");
        }
        eprintln!("{stage}: kept {}", scratch.display());
        return 1;
    }
    remove(stage, &scratch);
    eprintln!(
        "{stage}: {} regression traces regenerate byte for byte",
        traces.len()
    );
    0
}

/// One `conformance` run: the model (every one when `None`), the seed (fresh when `None`), and
/// the traces per model and steps per trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Run {
    /// Only this model.
    pub(crate) model: Option<String>,
    /// The seed.
    pub(crate) seed: Option<String>,
    /// Traces per model.
    pub(crate) traces: u32,
    /// Steps per trace; each model's `sim_steps` when `None`.
    pub(crate) steps: Option<u32>,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            model: None,
            seed: None,
            traces: TRACES,
            steps: None,
        }
    }
}

/// Parses `conformance [MODEL] [SEED] [TRACES] [STEPS]`: MODEL `all` is every model; SEED a
/// seed Quint takes; TRACES and STEPS 1..=1000.
pub(crate) fn parse_run(words: &[String]) -> Result<Run, String> {
    let mut run = Run::default();
    let mut words = words.iter();
    if let Some(model) = words.next() {
        run.model = (model != "all").then(|| model.clone());
    }
    if let Some(seed) = words.next() {
        if !valid_seed(seed) {
            return Err(format!(
                "conformance: the seed {seed} is not 0x<1-16 hex digits> or 1-20 decimal digits"
            ));
        }
        run.seed = Some(seed.clone());
    }
    let mut count = |what: &str, slot: &mut u32| -> Result<(), String> {
        if let Some(word) = words.next() {
            *slot = word
                .parse()
                .ok()
                .filter(|n| (1..=MOST).contains(n))
                .ok_or_else(|| format!("conformance: {what} {word} is not 1..={MOST}"))?;
        }
        Ok(())
    };
    count("TRACES", &mut run.traces)?;
    let mut steps = 0;
    count("STEPS", &mut steps)?;
    run.steps = (steps > 0).then_some(steps);
    if words.next().is_some() {
        return Err("conformance: at most MODEL SEED TRACES STEPS".to_owned());
    }
    Ok(run)
}

/// The `conformance-replay` stage: every model, a fresh seed, the default budget.
pub(crate) fn replay_stage(context: &Context) -> i32 {
    conformance(context, &Run::default())
}

/// `conformance [MODEL] [SEED] [TRACES] [STEPS]`. Returns the exit code.
pub(crate) fn conformance(context: &Context, run: &Run) -> i32 {
    let stage = "conformance-replay";
    let models = match formal::checked_registry(stage) {
        Ok(models) => models,
        Err(code) => return code,
    };
    let chosen: Vec<&Model> = models
        .iter()
        .filter(|m| run.model.as_ref().is_none_or(|name| m.name == *name))
        .collect();
    if chosen.is_empty() {
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        eprintln!(
            "{stage}: no model {}; registered: {} (or all)",
            run.model.as_deref().unwrap_or_default(),
            names.join(" ")
        );
        return 2;
    }
    let seed = run.seed.clone().unwrap_or_else(random_seed);
    if !valid_seed(&seed) {
        eprintln!("{stage}: the seed {seed} is not one Quint takes");
        return 2;
    }
    if let Err(code) = formal::require(context, stage, &["node", "quint"]) {
        return code;
    }
    let traces = run.traces.to_string();
    eprintln!("{stage}: seed {seed}: {traces} traces per model");
    for model in &chosen {
        let steps = run.steps.unwrap_or(model.sim_steps);
        eprintln!(
            "{stage}: repeat {} ({traces} traces of {steps} steps) with: just conformance {} {seed} {traces} {steps}",
            model.name, model.name
        );
    }
    let scratch = match formal::unique_dir(&std::env::temp_dir(), "jfastlsp-conformance") {
        Ok(dir) => dir,
        Err(message) => {
            eprintln!("{stage}: {message}");
            return 1;
        }
    };
    let code = generate_and_replay(context, run, &chosen, &seed, &scratch);
    if code != 0 {
        eprintln!(
            "{stage}: FAILED at seed {seed}; kept {} (the generated traces are under run/<model>/)",
            scratch.display()
        );
        return code;
    }
    remove(stage, &scratch);
    0
}

fn generate_and_replay(
    context: &Context,
    run: &Run,
    models: &[&Model],
    seed: &str,
    scratch: &Path,
) -> i32 {
    let stage = "conformance-replay";
    let traces = run.traces.to_string();
    for model in models {
        let steps = run.steps.unwrap_or(model.sim_steps).to_string();
        let mut runs: Vec<PathBuf> = Vec::new();
        for pass in ["run", "again"] {
            let dir = scratch.join(pass).join(&model.name);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("{stage}: cannot create {}: {e}", dir.display());
                return 1;
            }
            let flags = [
                "--main",
                model.main.as_str(),
                "--init",
                formal::INIT,
                "--step",
                formal::STEP,
                "--invariant",
                formal::INVARIANT,
                "--max-steps",
                steps.as_str(),
                "--max-samples",
                traces.as_str(),
                "--n-traces",
                traces.as_str(),
                "--seed",
                seed,
            ];
            let code = quint_run(
                &model.file,
                &dir.join(format!("trace_{{seq}}{SUFFIX}")),
                &flags,
            )
            .run(context);
            if code != 0 {
                eprintln!(
                    "{stage}: {}: quint run failed (exit {code}): an invariant violated, or the model does not run",
                    model.name
                );
                return code;
            }
            runs.push(dir);
        }
        if let [first, second] = runs.as_slice()
            && let Err(why) = same_run(first, second)
        {
            eprintln!(
                "{stage}: {}: the seed {seed} does not reproduce the run: {why}",
                model.name
            );
            return 1;
        }
    }
    let generated = scratch.join("run").to_string_lossy().into_owned();
    let mut args = vec![
        "run",
        "--locked",
        "-q",
        "-p",
        "jfl-conformance",
        "--",
        "replay",
        "--seed",
        seed,
        "--generated",
        &generated,
    ];
    if let Some(model) = &run.model {
        args.extend_from_slice(&["--model", model]);
    }
    catalog::cargo(&args).run(context)
}

/// Whether two runs' directories hold the same trace files, equal outside their `#meta`.
fn same_run(first: &Path, second: &Path) -> Result<(), String> {
    let names = |dir: &Path| -> Result<Vec<String>, String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map_err(|e| format!("cannot list {}: {e}", dir.display()))?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Ok(names)
    };
    let (a, b) = (names(first)?, names(second)?);
    if a != b {
        return Err(format!("the runs wrote {a:?} and {b:?}"));
    }
    if a.is_empty() {
        return Err("Quint wrote no trace".to_owned());
    }
    for name in &a {
        let read = |dir: &Path| {
            std::fs::read_to_string(dir.join(name)).map_err(|e| format!("cannot read {name}: {e}"))
        };
        let (x, y) = (read(first)?, read(second)?);
        if strip_meta(&x)? != strip_meta(&y)? {
            return Err(format!("{name} differs"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = "# c\n[traces.smoke-up]\nmodel = \"smoke\"\nmain = \"smoke\"\ninit = \"init\"\nstep = \"step\"\ninvariant = \"allInvariants\"\nmax_steps = 6\nseed = \"0x2a\"\n";

    #[test]
    fn should_read_the_repository_s_manifest_when_it_is_parsed() {
        // given
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(root.join(DIR).join(MANIFEST)).unwrap_or_default();

        // when
        let traces = parse_manifest(&text);

        // then
        let traces = traces.unwrap_or_default();
        assert!(traces.iter().any(|t| t.model == "smoke" && t.max_steps > 0));
    }

    #[test]
    fn should_refuse_the_manifest_when_a_line_leaves_the_shape() {
        // given: each edit, and what the refusal names
        let edits: [(&str, &str, &str); 9] = [
            ("seed = \"0x2a\"", "seed = \"0x2A\"", "outside its shape"),
            ("seed = \"0x2a\"", "seed = \"--help\"", "outside its shape"),
            ("max_steps = 6", "max_steps = 0", "outside its shape"),
            ("main = \"smoke\"", "main = \"a b\"", "outside its shape"),
            ("init = \"init\"\n", "", "has no `init`"),
            (
                "step = \"step\"",
                "step = \"step\"\nstep = \"step\"",
                "twice",
            ),
            ("step = \"step\"", "colour = \"red\"", "no key `colour`"),
            (
                "# c",
                "model = \"smoke\"",
                "outside a [traces.<name>] table",
            ),
            (
                "[traces.smoke-up]",
                "[traces.Smoke]",
                "outside a [traces.<name>] table",
            ),
        ];

        // when
        let results: Vec<_> = edits
            .iter()
            .map(|(from, to, _)| parse_manifest(&ONE.replacen(from, to, 1)))
            .collect();

        // then
        for ((from, to, why), result) in edits.iter().zip(&results) {
            let refusal = result.as_ref().err().cloned().unwrap_or_default();
            assert!(refusal.contains(why), "{from} -> {to}: {result:?}");
        }
        assert!(
            parse_manifest(&format!("{ONE}{}", ONE.replace("# c", "")))
                .is_err_and(|e| e.contains("twice"))
        );
        assert_eq!(parse_manifest("# none\n"), Ok(vec![]));
    }

    #[test]
    fn should_name_the_unlisted_the_missing_and_the_unknown_when_manifest_and_files_differ() {
        // given
        let traces = parse_manifest(&ONE.replace("model = \"smoke\"", "model = \"views\""))
            .unwrap_or_default();
        let files = vec!["stray.itf.json".to_owned()];
        let models = vec![Model {
            name: "smoke".to_owned(),
            file: "formal/smoke.qnt".to_owned(),
            main: "smoke".to_owned(),
            steps: 10,
            sim_steps: 25,
        }];

        // when
        let found = mismatches(&traces, &files, &models);

        // then
        assert_eq!(found.len(), 4, "{found:?}");
        assert!(
            found
                .iter()
                .any(|f| f.contains("the model smoke has no regression trace"))
        );
        assert!(
            found
                .iter()
                .any(|f| f.contains("stray.itf.json: not listed"))
        );
        assert!(
            found
                .iter()
                .any(|f| f.contains("smoke-up.itf.json: listed"))
        );
        assert!(found.iter().any(|f| f.contains("names the model views")));
    }

    #[test]
    fn should_compare_what_follows_the_meta_when_a_trace_is_stripped() {
        // given
        let traces = [
            (
                r##"{"#meta":{"a":"}{\"","t":1},"states":[]}"##,
                Ok(r#","states":[]}"#),
            ),
            (r##"{"#meta":{"n":{"x":1}},"s":2}"##, Ok(r#","s":2}"#)),
            (r#"{"states":[]}"#, Err(())),
            (r##"{"#meta":"x","states":[]}"##, Err(())),
            (r##"{"#meta":{"a":1"##, Err(())),
            ("", Err(())),
        ];

        // when
        let stripped: Vec<Result<&str, ()>> = traces
            .iter()
            .map(|(t, _)| strip_meta(t).map_err(|_| ()))
            .collect();

        // then
        let expected: Vec<Result<&str, ()>> = traces.iter().map(|(_, s)| *s).collect();
        assert_eq!(stripped, expected);
    }

    #[test]
    fn should_take_only_seeds_quint_takes_when_a_seed_is_checked() {
        // given
        let seeds = [
            ("0x2a", true),
            ("0x0123456789abcdef", true),
            ("42", true),
            ("0x", false),
            ("0x0123456789abcdef0", false),
            ("0X2a", false),
            ("-1", false),
            ("", false),
            ("0x2g", false),
        ];

        // when
        let verdicts: Vec<bool> = seeds.iter().map(|(s, _)| valid_seed(s)).collect();

        // then
        let expected: Vec<bool> = seeds.iter().map(|(_, v)| *v).collect();
        assert_eq!(verdicts, expected);
        assert!(valid_seed(&random_seed()));
    }

    #[test]
    fn should_parse_a_run_when_its_words_are_well_formed() {
        // given
        let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();

        // when
        let parsed = [
            parse_run(&words(&[])),
            parse_run(&words(&["smoke"])),
            parse_run(&words(&["all", "0x2a", "3", "7"])),
        ];
        let refused = [
            parse_run(&words(&["smoke", "zz"])),
            parse_run(&words(&["smoke", "1", "0"])),
            parse_run(&words(&["smoke", "1", "2", "1001"])),
            parse_run(&words(&["smoke", "1", "2", "3", "4"])),
        ];

        // then
        assert_eq!(parsed[0], Ok(Run::default()));
        assert_eq!(
            parsed[1],
            Ok(Run {
                model: Some("smoke".to_owned()),
                ..Run::default()
            })
        );
        assert_eq!(
            parsed[2],
            Ok(Run {
                model: None,
                seed: Some("0x2a".to_owned()),
                traces: 3,
                steps: Some(7)
            })
        );
        assert!(refused.iter().all(Result::is_err), "{refused:?}");
    }
}
