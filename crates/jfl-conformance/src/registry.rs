//! The drivers, by model (plan §7): each registered model of `formal/models.toml` has a driver
//! with its negative control, or is [`PENDING`], naming the task that brings its driver (the
//! models land before their drivers: FORMAL-03..05, then IDX-09, IDX-12, LSP-04). Fails closed:
//! a registered model with neither, a pending model that has a driver, or a driver of a model the
//! registry does not hold is a [`HarnessError`] ([`check`]). Pinned by
//! tests/replay.rs::should_refuse_the_registry_when_a_model_has_no_driver.

use std::path::Path;

use crate::HarnessError;
use crate::replay::{Context, Failure};
use crate::smoke;
use crate::trace::RawStep;

/// The MODELS registry, relative to the repository root (FORMAL-01).
pub const MODELS: &str = "formal/models.toml";

/// Replays one trace's steps through a driver.
pub type Replay = fn(&Context<'_>, Vec<RawStep>) -> Result<usize, Failure>;

/// One model's driver.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    /// The model's name in the MODELS registry.
    pub model: &'static str,
    /// The replay through the real code.
    pub replay: Replay,
    /// The negative control (ver §6.2 item 4): the same driver over a deliberately faulty
    /// component, which must diverge.
    pub control: Replay,
}

/// Every driver.
pub const DRIVERS: [Entry; 1] = [Entry {
    model: "smoke",
    replay: smoke::replay,
    control: smoke::control,
}];

/// Registered models whose driver has not landed yet, each with the task that brings it.
pub const PENDING: [(&str, &str); 0] = [];

/// The driver of `model`.
pub fn driver(model: &str) -> Option<&'static Entry> {
    DRIVERS.iter().find(|d| d.model == model)
}

/// The task that brings `model`'s driver, when it is pending.
pub fn pending(model: &str) -> Option<&'static str> {
    PENDING
        .iter()
        .find(|(m, _)| *m == model)
        .map(|(_, task)| *task)
}

/// The model names of the registry text `text`.
pub fn parse_models(text: &str) -> Result<Vec<String>, HarnessError> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| {
        HarnessError::new(format!("{MODELS}: {}", e.to_string().trim()))
    })?;
    let models = table
        .get("models")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| HarnessError::new(format!("{MODELS}: no [models.<name>] table")))?;
    Ok(models.keys().cloned().collect())
}

/// The model names of the registry at `root`.
pub fn models(root: &Path) -> Result<Vec<String>, HarnessError> {
    let text = std::fs::read_to_string(root.join(MODELS))
        .map_err(|e| HarnessError::new(format!("{MODELS}: cannot read: {e}")))?;
    parse_models(&text)
}

/// The registry `models` and the drivers agree: every model has a driver or is pending, never
/// both, and every driver's model is registered.
pub fn check(models: &[String]) -> Result<(), HarnessError> {
    for model in models {
        match (driver(model), pending(model)) {
            (Some(_), None) | (None, Some(_)) => {}
            (Some(_), Some(task)) => {
                return Err(HarnessError::new(format!(
                    "{model} has a driver and is still pending ({task}): remove it from PENDING"
                )));
            }
            (None, None) => {
                return Err(HarnessError::new(format!(
                    "{model} is registered in {MODELS} but has no driver and is not pending (crates/jfl-conformance/src/registry.rs)"
                )));
            }
        }
    }
    let orphans = DRIVERS
        .iter()
        .map(|d| d.model)
        .chain(PENDING.iter().map(|(m, _)| *m))
        .filter(|m| !models.iter().any(|r| r == m));
    if let Some(orphan) = orphans.into_iter().next() {
        return Err(HarnessError::new(format!(
            "{orphan} has a driver or a pending entry but no model in {MODELS}"
        )));
    }
    Ok(())
}
