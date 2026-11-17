//! The drivers, by model (plan §7): each registered model of `formal/models.toml` has a driver
//! with its negative control, or is [`PENDING`], naming the task that brings its driver (the
//! models land before their drivers: FORMAL-03..05, then IDX-09, IDX-12, LSP-04). Fails closed:
//! a registered model with neither, a pending model that has a driver, or a driver of a model the
//! registry does not hold is a [`HarnessError`] ([`check`]); so is a pending entry whose task has
//! no card in `doc/tasks.md` or whose card's `Accept:` list has every box ticked ([`check_pending`]): a
//! model cannot stay pending past the task that was to bring its driver. Pinned by
//! tests/replay.rs::should_refuse_the_registry_when_a_model_has_no_driver and
//! tests/replay.rs::should_refuse_a_pending_entry_when_its_task_is_unknown_or_done.
//!
//! An entry holds factories of [`Replayer`]s, never a replay function: the comparison after every
//! step belongs to [`crate::replay`], whatever the driver (ver §6.2 item 3).

use std::path::Path;

use crate::HarnessError;
use crate::replay::Replayer;
use crate::smoke;

/// The MODELS registry, relative to the repository root (FORMAL-01).
pub const MODELS: &str = "formal/models.toml";

/// The task list a pending entry's task must be open in, relative to the repository root.
pub const TASKS: &str = "doc/tasks.md";

/// One model's driver.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    /// The model's name in the MODELS registry.
    pub model: &'static str,
    /// The driver over the real code.
    pub driver: fn() -> Replayer,
    /// The negative control (ver §6.2 item 4): the same driver over a deliberately faulty
    /// component, which must diverge.
    pub control: fn() -> Replayer,
}

/// Every driver.
pub const DRIVERS: [Entry; 1] = [Entry {
    model: "smoke",
    driver: smoke::driver,
    control: smoke::control,
}];

/// Registered models whose driver has not landed yet, each with the task that brings it. Each
/// task must have an open card in [`TASKS`] ([`check_pending`]).
pub const PENDING: [(&str, &str); 1] = [("views", "IDX-09")];

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

/// Every [`PENDING`] entry's task has a card in the task list `tasks` (the text of [`TASKS`])
/// that is not done: at least one of its `Accept:` boxes is unticked.
pub fn check_pending(tasks: &str) -> Result<(), HarnessError> {
    check_pending_in(&PENDING, tasks)
}

/// [`check_pending`] over the entries `pending`.
pub fn check_pending_in(pending: &[(&str, &str)], tasks: &str) -> Result<(), HarnessError> {
    for (model, task) in pending {
        let refuse = |why: &str| {
            Err(HarnessError::new(format!(
                "{model} is pending on {task}, but {why} in {TASKS}: bring {model}'s driver, or name the open task that brings it (crates/jfl-conformance/src/registry.rs)"
            )))
        };
        let Some(card) = card(tasks, task) else {
            return refuse(&format!("{task} has no card (`#### {task}:`)"));
        };
        let (mut open, mut ticked) = (0_usize, 0_usize);
        for line in accept_items(&card) {
            let line = line.trim_start();
            if line.starts_with("- [ ]") {
                open = open.saturating_add(1);
            } else if line.starts_with("- [x]") || line.starts_with("- [X]") {
                ticked = ticked.saturating_add(1);
            }
        }
        if open == 0 {
            return refuse(&if ticked == 0 {
                format!("{task}'s card has no `Accept:` box")
            } else {
                format!("{task} is done (every `Accept:` box is ticked)")
            });
        }
    }
    Ok(())
}

/// The items of a card's `- **Accept:**` list: the indented lines after it, up to the card's next
/// top-level item. A box anywhere else on the card (an owner's note, say) does not count.
fn accept_items<'a>(card: &[&'a str]) -> Vec<&'a str> {
    card.iter()
        .skip_while(|line| !line.starts_with("- **Accept:**"))
        .skip(1)
        .take_while(|line| line.is_empty() || line.starts_with(char::is_whitespace))
        .copied()
        .collect()
}

/// The lines of `task`'s card in `tasks`: from its `#### <task>:` heading to the next heading.
fn card<'a>(tasks: &'a str, task: &str) -> Option<Vec<&'a str>> {
    let heading = format!("#### {task}:");
    let mut lines = tasks.lines().skip_while(|line| !line.starts_with(&heading));
    lines.next()?;
    Some(
        lines
            .take_while(|line| !["## ", "### ", "#### "].iter().any(|h| line.starts_with(h)))
            .collect(),
    )
}

/// The task list at `root`.
pub fn tasks(root: &Path) -> Result<String, HarnessError> {
    std::fs::read_to_string(root.join(TASKS))
        .map_err(|e| HarnessError::new(format!("{TASKS}: cannot read: {e}")))
}
