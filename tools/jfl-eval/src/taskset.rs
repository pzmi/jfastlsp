//! The task-set format (TOML): the tasks an E0 run asks the agent to do.
//!
//! ```toml
//! schema = 1
//! agent_timeout_sec = 3600      # optional, per task: Harbor's [agent] timeout
//! verifier_timeout_sec = 1800   # optional: Harbor's [verifier] timeout
//!
//! [[task]]
//! id = "fix-ledger-rounding"    # an id (`ids::Id`): the only part of a task a report shows
//! commit = "<40 hex>"           # the starting commit, checked out in the arm's image
//! prompt = "..."                # the instruction the agent gets, verbatim
//! test_command = "./gradlew :ledger-core:test"   # run from the work directory; exit 0 = success
//! known_good = "<40 hex>"       # a commit whose tree passes the test command (the oracle)
//! category = "bug-fix"          # bug-fix | signature-change | cross-module-feature | rename | other
//! ```
//!
//! Unknown keys are refused, so a typo is an error rather than a silently missing field.
//! A private target's task set stays on the owner's machine (ADR 0004); the public control set
//! (EVAL-12) uses the same format.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use crate::bounded::{MAX_TOML_FILE_BYTES, read_text};
use crate::error::{Context, EvalError, Result};
use crate::ids::{Id, is_commit};

/// The longest prompt accepted.
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
/// The longest test command accepted.
pub const MAX_TEST_COMMAND_BYTES: usize = 4 * 1024;
/// The most tasks in one set.
pub const MAX_TASKS: usize = 1000;
/// Harbor's agent timeout when the set names none.
pub const DEFAULT_AGENT_TIMEOUT_SEC: u32 = 3600;
/// Harbor's verifier timeout when the set names none.
pub const DEFAULT_VERIFIER_TIMEOUT_SEC: u32 = 1800;

/// What kind of change a task asks for (plan §9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// A bug fix.
    BugFix,
    /// A change of a method signature and its callers.
    SignatureChange,
    /// A feature that crosses modules.
    CrossModuleFeature,
    /// A rename.
    Rename,
    /// Anything else.
    Other,
}

impl Category {
    /// The category as written in the task set.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BugFix => "bug-fix",
            Self::SignatureChange => "signature-change",
            Self::CrossModuleFeature => "cross-module-feature",
            Self::Rename => "rename",
            Self::Other => "other",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTaskSet {
    schema: u32,
    agent_timeout_sec: Option<u32>,
    verifier_timeout_sec: Option<u32>,
    #[serde(default)]
    task: Vec<RawTask>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTask {
    id: String,
    commit: String,
    prompt: String,
    test_command: String,
    known_good: String,
    category: Category,
}

/// One task, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// The task's id.
    pub id: Id,
    /// The starting commit.
    pub commit: String,
    /// The instruction, verbatim.
    pub prompt: String,
    /// The test command, run from the work directory.
    pub test_command: String,
    /// A commit whose tree passes the test command.
    pub known_good: String,
    /// The kind of change.
    pub category: Category,
}

/// A validated task set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSet {
    /// Harbor's agent timeout, seconds.
    pub agent_timeout_sec: u32,
    /// Harbor's verifier timeout, seconds.
    pub verifier_timeout_sec: u32,
    /// The tasks, in file order.
    pub tasks: Vec<Task>,
}

impl TaskSet {
    /// Read and validate a task-set file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = read_text(path, MAX_TOML_FILE_BYTES)?;
        Self::parse(&text).context(format!("task set {}", path.display()))
    }

    /// Parse and validate task-set TOML.
    pub fn parse(text: &str) -> Result<Self> {
        let raw: RawTaskSet =
            toml::from_str(text).map_err(|e| toml_error("not a task set", text, &e))?;
        if raw.schema != 1 {
            return Err(EvalError::new(format!(
                "schema {} (expected 1)",
                raw.schema
            )));
        }
        if raw.task.is_empty() {
            return Err(EvalError::new("no [[task]]"));
        }
        if raw.task.len() > MAX_TASKS {
            return Err(EvalError::new(format!("more than {MAX_TASKS} tasks")));
        }
        let mut seen = BTreeSet::new();
        let mut tasks = Vec::with_capacity(raw.task.len());
        for (index, t) in raw.task.into_iter().enumerate() {
            let task = validate(t).context(format!("task #{}", index.saturating_add(1)))?;
            if !seen.insert(task.id.clone()) {
                return Err(EvalError::new(format!("task id {} twice", task.id)));
            }
            tasks.push(task);
        }
        Ok(Self {
            agent_timeout_sec: raw.agent_timeout_sec.unwrap_or(DEFAULT_AGENT_TIMEOUT_SEC),
            verifier_timeout_sec: raw
                .verifier_timeout_sec
                .unwrap_or(DEFAULT_VERIFIER_TIMEOUT_SEC),
            tasks,
        })
    }
}

/// A TOML or schema error as its line only. The error's text quotes the offending line and serde's
/// type and variant messages quote the value, which may be prompt text, so neither is kept.
/// Pinned by: `tests::should_keep_the_prompt_out_of_the_error_when_the_toml_is_malformed` and
/// `tests::should_keep_the_value_out_of_the_error_when_a_variant_is_unknown`.
pub(crate) fn toml_error(what: &str, text: &str, e: &toml::de::Error) -> EvalError {
    match e.span() {
        Some(span) => {
            let before = text.get(..span.start).unwrap_or(text);
            let line = before
                .bytes()
                .filter(|&b| b == b'\n')
                .count()
                .saturating_add(1);
            EvalError::new(format!(
                "{what}: a syntax, unknown-key or wrong-value error at line {line} \
                 (the value is not echoed)"
            ))
        }
        None => EvalError::new(format!(
            "{what}: a missing key or wrong value (the value is not echoed)"
        )),
    }
}

fn validate(t: RawTask) -> Result<Task> {
    let id = Id::parse(&t.id).ok_or_else(|| {
        EvalError::new("id: not [a-z0-9] words joined by single hyphens, 1 to 64 characters")
    })?;
    for (name, value) in [("commit", &t.commit), ("known_good", &t.known_good)] {
        if !is_commit(value) {
            return Err(EvalError::new(format!(
                "{id}: {name}: not a full 40-digit lowercase commit id"
            )));
        }
    }
    text_field(&id, "prompt", &t.prompt, MAX_PROMPT_BYTES)?;
    text_field(&id, "test_command", &t.test_command, MAX_TEST_COMMAND_BYTES)?;
    Ok(Task {
        id,
        commit: t.commit,
        prompt: t.prompt,
        test_command: t.test_command,
        known_good: t.known_good,
        category: t.category,
    })
}

fn text_field(id: &Id, name: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() {
        return Err(EvalError::new(format!("{id}: {name} is empty")));
    }
    if value.len() > max {
        return Err(EvalError::new(format!("{id}: {name} over {max} bytes")));
    }
    if value.contains('\0') {
        return Err(EvalError::new(format!("{id}: {name} holds a NUL")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
schema = 1
agent_timeout_sec = 1200

[[task]]
id = "fix-rounding"
commit = "1111111111111111111111111111111111111111"
prompt = "Fix the rounding of Money.add."
test_command = "./gradlew :core:test"
known_good = "2222222222222222222222222222222222222222"
category = "bug-fix"
"#;

    #[test]
    fn should_read_every_field_when_the_task_set_is_well_formed() {
        // given
        // when
        let set = TaskSet::parse(GOOD).unwrap();
        // then
        assert_eq!(set.agent_timeout_sec, 1200);
        assert_eq!(set.verifier_timeout_sec, DEFAULT_VERIFIER_TIMEOUT_SEC);
        let task = &set.tasks[0];
        assert_eq!(task.id.as_str(), "fix-rounding");
        assert_eq!(task.category, Category::BugFix);
        assert_eq!(task.test_command, "./gradlew :core:test");
    }

    #[test]
    fn should_refuse_the_set_when_a_key_is_unknown() {
        // given
        let text = GOOD.replace("category =", "categroy =");
        // when
        let err = TaskSet::parse(&text).unwrap_err();
        // then
        assert!(err.to_string().contains("not a task set"), "{err}");
    }

    #[test]
    fn should_refuse_the_set_when_a_commit_is_abbreviated() {
        // given
        let text = GOOD.replace("1111111111111111111111111111111111111111", "1111111");
        // when
        let err = TaskSet::parse(&text).unwrap_err();
        // then
        assert!(err.to_string().contains("commit"), "{err}");
    }

    #[test]
    fn should_refuse_the_set_when_an_id_repeats() {
        // given
        let second = &GOOD[GOOD.find("[[task]]").unwrap()..];
        let text = format!("{GOOD}\n{second}");
        // when
        let err = TaskSet::parse(&text).unwrap_err();
        // then
        assert!(err.to_string().contains("twice"), "{err}");
    }

    #[test]
    fn should_keep_the_value_out_of_the_error_when_a_variant_is_unknown() {
        // given
        let text = GOOD.replace(
            "category = \"bug-fix\"",
            "category = \"SECRET PROMPT TEXT\"",
        );
        // when
        let err = TaskSet::parse(&text).unwrap_err().to_string();
        // then
        assert!(!err.contains("SECRET"), "{err}");
        assert!(err.contains("line 11"), "{err}");
    }

    #[test]
    fn should_keep_the_prompt_out_of_the_error_when_the_toml_is_malformed() {
        // given: a planted prompt on the line that breaks
        let text = "schema = 1\n[[task]]\nprompt = \"secret prompt text\" junk\n";
        // when
        let err = TaskSet::parse(text).unwrap_err();
        // then
        assert!(!err.to_string().contains("secret prompt text"), "{err}");
    }
}
