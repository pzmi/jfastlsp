//! The committed regression traces (ver §6.2 item 1): `formal/traces/regression/manifest.toml`
//! lists each `<name>.itf.json` of that directory, one `[traces.<name>]` table each, with the
//! model (a name of the MODELS registry), its main module, the init and step actions, the
//! invariant the generation checked, the step bound and the seed: what `formal-freshness`
//! (xtask, which reads the same file in a pinned line shape) passes to `quint run --mbt` to
//! regenerate the trace byte for byte. Read strictly: an unknown key, a trace file the manifest
//! does not list, or a listed trace that is missing is a [`HarnessError`]. Pinned by
//! tests/replay.rs::should_refuse_the_manifest_when_it_and_the_directory_disagree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::HarnessError;

/// The directory of the committed traces, relative to the repository root.
pub const DIR: &str = "formal/traces/regression";

/// The manifest's file name in [`DIR`].
pub const MANIFEST: &str = "manifest.toml";

/// The suffix of a trace file.
pub const SUFFIX: &str = ".itf.json";

/// One table of the manifest, as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    model: String,
    main: String,
    init: String,
    step: String,
    invariant: String,
    max_steps: u32,
    seed: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    traces: BTreeMap<String, Table>,
}

/// One committed regression trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regression {
    /// The table's name; the file is `<name>.itf.json`.
    pub name: String,
    /// The model, a name of the MODELS registry.
    pub model: String,
    /// The module Quint ran.
    pub main: String,
    /// The init action.
    pub init: String,
    /// The step action.
    pub step: String,
    /// The invariant checked while generating.
    pub invariant: String,
    /// The step bound.
    pub max_steps: u32,
    /// The seed.
    pub seed: String,
    /// The trace file, relative to the repository root.
    pub file: PathBuf,
}

/// The manifest `text`, checked against the trace files `present` (file names in [`DIR`]).
pub fn parse(text: &str, present: &[String]) -> Result<Vec<Regression>, HarnessError> {
    let at = format!("{DIR}/{MANIFEST}");
    let file: File = toml::from_str(text)
        .map_err(|e| HarnessError::new(format!("{at}: {}", e.to_string().trim())))?;
    let mut traces = Vec::new();
    for (name, table) in file.traces {
        let well_named = name.starts_with(|c: char| c.is_ascii_lowercase())
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !well_named {
            return Err(HarnessError::new(format!(
                "{at}: traces.{name}: a name is [a-z][a-z0-9-]*"
            )));
        }
        if table.max_steps == 0 {
            return Err(HarnessError::new(format!(
                "{at}: traces.{name}: max_steps is at least 1"
            )));
        }
        let file_name = format!("{name}{SUFFIX}");
        if !present.contains(&file_name) {
            return Err(HarnessError::new(format!(
                "{at}: traces.{name}: {DIR}/{file_name} is missing"
            )));
        }
        traces.push(Regression {
            file: Path::new(DIR).join(&file_name),
            name,
            model: table.model,
            main: table.main,
            init: table.init,
            step: table.step,
            invariant: table.invariant,
            max_steps: table.max_steps,
            seed: table.seed,
        });
    }
    for file_name in present {
        let listed = traces
            .iter()
            .any(|t| format!("{}{SUFFIX}", t.name) == *file_name);
        if !listed {
            return Err(HarnessError::new(format!(
                "{DIR}/{file_name}: not listed in {at} (every committed trace is, so formal-freshness regenerates it)"
            )));
        }
    }
    Ok(traces)
}

/// The manifest of the repository at `root`, checked against its directory: every entry of
/// [`DIR`] is the manifest or a listed `.itf.json` file.
pub fn read(root: &Path) -> Result<Vec<Regression>, HarnessError> {
    let dir = root.join(DIR);
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| HarnessError::new(format!("{DIR}: cannot list: {e}")))?;
    let mut present = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| HarnessError::new(format!("{DIR}: cannot list: {e}")))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_file = entry.file_type().is_ok_and(|t| t.is_file());
        if name == MANIFEST && is_file {
            continue;
        }
        if !is_file || !name.ends_with(SUFFIX) {
            return Err(HarnessError::new(format!(
                "{DIR}/{name}: neither the manifest nor a trace file (<name>{SUFFIX})"
            )));
        }
        present.push(name);
    }
    present.sort();
    let text = std::fs::read_to_string(dir.join(MANIFEST))
        .map_err(|e| HarnessError::new(format!("{DIR}/{MANIFEST}: cannot read: {e}")))?;
    parse(&text, &present)
}
