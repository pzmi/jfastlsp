//! The MODELS registry (`formal/models.toml`, FORMAL-01, ver §6.1) read with a real TOML parser,
//! independently of xtask's pinned-shape reader (tools/xtask/src/formal.rs): every model has a
//! file under `formal/`, a main module and a positive step bound, every `.qnt` of `formal/` is
//! registered, each model declares what `formal-verify` runs (`init`, `step`, `allInvariants`),
//! and `.github/workflows/formal.yml` verifies exactly the registered models, one job per model
//! and part: Apalache, and the simulations (IDX-09e), each half of `xtask formal-verify`.

use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn Error>>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One model of the registry: (name, file, main, steps, sim_steps).
type Model = (String, String, String, i64, i64);

/// The registry's models.
fn registry(text: &str) -> Result<Vec<Model>, Box<dyn Error>> {
    let table: toml::Table = text.parse()?;
    let mut models = Vec::new();
    for (key, value) in &table {
        if key != "models" {
            return Err(format!("unexpected top-level key {key}").into());
        }
        for (name, model) in value.as_table().ok_or("models is not a table")? {
            let model = model.as_table().ok_or("a model is not a table")?;
            let keys: BTreeSet<&str> = model.keys().map(String::as_str).collect();
            let required = BTreeSet::from(["file", "main", "sim_steps", "steps"]);
            let optional = BTreeSet::from(["progress"]);
            if !required.is_subset(&keys)
                || !keys
                    .iter()
                    .all(|k| required.contains(k) || optional.contains(k))
            {
                return Err(format!("models.{name} has the keys {keys:?}").into());
            }
            if let Some(progress) = model.get("progress") {
                let progress = progress.as_table().ok_or("progress is not a table")?;
                let keys: BTreeSet<&str> = progress.keys().map(String::as_str).collect();
                if keys != BTreeSet::from(["control", "invariant", "samples", "step", "steps"]) {
                    return Err(format!("models.{name}.progress has the keys {keys:?}").into());
                }
                for key in ["step", "invariant", "control"] {
                    progress
                        .get(key)
                        .and_then(toml::Value::as_str)
                        .ok_or("a progress name is not a string")?;
                }
                for key in ["steps", "samples"] {
                    let n = progress
                        .get(key)
                        .and_then(toml::Value::as_integer)
                        .ok_or("a progress bound is not an integer")?;
                    if n < 1 {
                        return Err(format!("models.{name}.progress.{key} is {n}").into());
                    }
                }
            }
            let text = |k: &str| {
                model
                    .get(k)
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned)
            };
            models.push((
                name.clone(),
                text("file").ok_or("file is not a string")?,
                text("main").ok_or("main is not a string")?,
                model
                    .get("steps")
                    .and_then(toml::Value::as_integer)
                    .ok_or("steps is not an integer")?,
                model
                    .get("sim_steps")
                    .and_then(toml::Value::as_integer)
                    .ok_or("sim_steps is not an integer")?,
            ));
        }
    }
    Ok(models)
}

/// The `.qnt` files under `dir`, as paths relative to the repository root.
fn qnt_files(dir: &Path, rel: &str, found: &mut BTreeSet<String>) -> TestResult {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{rel}/{name}");
        if entry.file_type()?.is_dir() {
            qnt_files(&entry.path(), &path, found)?;
        } else if name.ends_with(".qnt") {
            found.insert(path);
        }
    }
    Ok(())
}

/// The names of formal.yml's `model:` matrix line (`model: [a, b]`), the only one.
fn matrix(workflow: &str) -> Result<BTreeSet<String>, Box<dyn Error>> {
    axis(workflow, "model")
}

/// The values of formal.yml's `<key>:` matrix line (`<key>: [a, b]`), the only one.
fn axis(workflow: &str, key: &str) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let prefix = format!("{key}: [");
    let lines: Vec<&str> = workflow
        .lines()
        .filter_map(|l| l.trim().strip_prefix(prefix.as_str()))
        .collect();
    let [line] = lines.as_slice() else {
        return Err(format!("{} `{key}: [...]` lines in formal.yml", lines.len()).into());
    };
    let inner = line
        .strip_suffix(']')
        .ok_or("the matrix line does not end in ]")?;
    Ok(inner
        .split(',')
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty())
        .collect())
}

#[test]
fn should_register_every_model_file_when_the_registry_is_read() -> TestResult {
    // given
    let models = registry(&std::fs::read_to_string(root().join("formal/models.toml"))?)?;

    // when
    let mut files = BTreeSet::new();
    qnt_files(&root().join("formal"), "formal", &mut files)?;

    // then
    let registered: BTreeSet<String> = models.iter().map(|m| m.1.clone()).collect();
    assert!(!models.is_empty());
    assert_eq!(registered, files);
    assert_eq!(registered.len(), models.len(), "a file registered twice");
    for (name, file, main, steps, sim_steps) in &models {
        let source = std::fs::read_to_string(root().join(file))?;
        assert!(*steps > 0, "models.{name}.steps");
        assert!(
            sim_steps >= steps,
            "models.{name}.sim_steps below its Apalache bound"
        );
        assert!(
            source.contains(&format!("module {main} {{")),
            "{file}: no module {main}"
        );
        for declared in ["action init", "action step", "val allInvariants", "Test ="] {
            assert!(source.contains(declared), "{file}: no `{declared}`");
        }
    }
    Ok(())
}

#[test]
fn should_declare_the_liveness_steps_when_a_model_has_a_progress_entry() -> TestResult {
    // given
    let table: toml::Table = std::fs::read_to_string(root().join("formal/models.toml"))?.parse()?;
    let models = table
        .get("models")
        .and_then(toml::Value::as_table)
        .ok_or("no models table")?;

    // when
    let live: Vec<(&String, &toml::Value)> = models
        .iter()
        .filter_map(|(name, model)| model.get("progress").map(|p| (name, p)))
        .collect();

    // then: the daemon model has one, and every name it gives is declared in its file
    assert!(live.iter().any(|(name, _)| *name == "daemon"));
    for (name, progress) in live {
        let file = models
            .get(name)
            .and_then(|m| m.get("file"))
            .and_then(toml::Value::as_str)
            .ok_or("no file")?;
        let source = std::fs::read_to_string(root().join(file))?;
        let name_of = |key: &str| {
            progress
                .get(key)
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
        };
        for declared in [
            format!("action {} =", name_of("step")),
            format!("val {} =", name_of("invariant")),
            format!("action {} =", name_of("control")),
        ] {
            assert!(source.contains(&declared), "{file}: no `{declared}`");
        }
    }
    Ok(())
}

#[test]
fn should_verify_exactly_the_registered_models_when_formal_yml_is_read() -> TestResult {
    // given
    let models = registry(&std::fs::read_to_string(root().join("formal/models.toml"))?)?;

    // when
    let jobs = matrix(&std::fs::read_to_string(
        root().join(".github/workflows/formal.yml"),
    )?)?;

    // then
    let names: BTreeSet<String> = models.into_iter().map(|m| m.0).collect();
    assert_eq!(jobs, names);
    Ok(())
}

/// The parts `xtask formal-verify MODEL PART` takes, read from xtask's own list
/// (tools/xtask/src/formal.rs, `pub(crate) const PARTS: [&str; N] = ["a", "b"];`, which
/// `Part::parse` matches): each half of the verification, which formal.yml runs as jobs of their
/// own so each has the job's time limit to itself (IDX-09e).
fn xtask_parts() -> Result<BTreeSet<String>, Box<dyn Error>> {
    let source = std::fs::read_to_string(root().join("tools/xtask/src/formal.rs"))?;
    let lines: Vec<&str> = source
        .lines()
        .filter(|l| l.starts_with("pub(crate) const PARTS: [&str; "))
        .collect();
    let [line] = lines.as_slice() else {
        return Err(format!("{} PARTS lines in xtask's formal.rs", lines.len()).into());
    };
    let (_, list) = line.split_once("= [").ok_or("no list in the PARTS line")?;
    let list = list
        .strip_suffix("];")
        .ok_or("the PARTS line does not end in ];")?;
    Ok(list
        .split(',')
        .map(|p| p.trim().trim_matches('"').to_owned())
        .filter(|p| !p.is_empty())
        .collect())
}

/// Whether formal.yml's matrix drops or adds combinations (`exclude:`, `include:`): every model's
/// every half must run.
fn reshaped(workflow: &str) -> bool {
    workflow.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with("exclude:") || l.starts_with("include:")
    })
}

#[test]
fn should_run_both_halves_of_every_model_when_formal_yml_is_read() -> TestResult {
    // given
    let workflow = std::fs::read_to_string(root().join(".github/workflows/formal.yml"))?;
    let excluding = workflow.replace(
        "        part: [apalache, simulation]\n",
        "        part: [apalache, simulation]\n        exclude:\n          - model: views\n            part: simulation\n",
    );

    // when
    let parts = axis(&workflow, "part")?;
    let without_simulation = axis(&workflow.replace(", simulation]", "]"), "part")?;

    // then: xtask's parts, each passed to `just formal-verify`, the matrix crossed whole (the
    // controls: one half dropped from the axis, or one model's half excluded, differ)
    assert_eq!(parts, xtask_parts()?);
    assert_eq!(parts.len(), 2);
    assert_ne!(without_simulation, parts);
    assert!(!reshaped(&workflow));
    assert_ne!(excluding, workflow);
    assert!(reshaped(&excluding));
    assert!(workflow.contains(r#"just formal-verify "$MODEL" "$PART""#));
    Ok(())
}

#[test]
fn should_see_a_difference_when_the_matrix_and_the_registry_part() -> TestResult {
    // given: the control for the test above, a registry with one model more
    let text = std::fs::read_to_string(root().join("formal/models.toml"))?;
    let grown = format!(
        "{text}\n[models.extra]\nfile = \"formal/extra.qnt\"\nmain = \"extra\"\nsteps = 1\nsim_steps = 1\n"
    );
    let workflow = std::fs::read_to_string(root().join(".github/workflows/formal.yml"))?;

    // when
    let names: BTreeSet<String> = registry(&grown)?.into_iter().map(|m| m.0).collect();
    let jobs = matrix(&workflow)?;

    // then
    assert_ne!(jobs, names);
    assert!(registry("[models.x]\nfile = \"formal/x.qnt\"\n").is_err());
    Ok(())
}
