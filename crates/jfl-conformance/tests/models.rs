//! The MODELS registry (`formal/models.toml`, FORMAL-01, ver §6.1) read with a real TOML parser,
//! independently of xtask's pinned-shape reader (tools/xtask/src/formal.rs): every model has a
//! file under `formal/`, a main module and a positive step bound, every `.qnt` of `formal/` is
//! registered, each model declares what `formal-verify` runs (`init`, `step`, `allInvariants`),
//! and `.github/workflows/formal.yml` verifies exactly the registered models, one job each.

use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn Error>>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One model of the registry: (name, file, main, steps).
type Model = (String, String, String, i64);

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
            if keys != BTreeSet::from(["file", "main", "steps"]) {
                return Err(format!("models.{name} has the keys {keys:?}").into());
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
    let lines: Vec<&str> = workflow
        .lines()
        .filter_map(|l| l.trim().strip_prefix("model: ["))
        .collect();
    let [line] = lines.as_slice() else {
        return Err(format!("{} `model: [...]` lines in formal.yml", lines.len()).into());
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
    for (name, file, main, steps) in &models {
        let source = std::fs::read_to_string(root().join(file))?;
        assert!(*steps > 0, "models.{name}.steps");
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

#[test]
fn should_see_a_difference_when_the_matrix_and_the_registry_part() -> TestResult {
    // given: the control for the test above, a registry with one model more
    let text = std::fs::read_to_string(root().join("formal/models.toml"))?;
    let grown = format!(
        "{text}\n[models.extra]\nfile = \"formal/extra.qnt\"\nmain = \"extra\"\nsteps = 1\n"
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
