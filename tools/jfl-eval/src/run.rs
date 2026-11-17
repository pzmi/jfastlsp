//! Harbor trial directories into runs: an arm, a task, a verdict and the transcript's metrics.
//!
//! A Harbor 0.26.0 trial directory (`models/trial/paths.py`) holds `result.json` (a
//! `TrialResult`, `schema_version` 1 or absent) and `agent/`, where the `claude-code` agent leaves
//! `sessions/projects/<cwd-slug>/*.jsonl` (with `<session>/subagents/*.jsonl`), `trajectory.json`
//! (ATIF) and `claude-code.txt` (stream-json). A job directory holds trial directories and its own
//! `result.json` without an `agent/` beside it, which is how the two are told apart.
//!
//! Success is the verifier's reward (`verifier_result.rewards.reward`, written by the generated
//! `tests/test.sh` from the test command's exit status): 1 is success, anything else failure, and
//! no reward at all (the trial errored before the verifier) is counted apart as no verdict.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::bounded::{MAX_JSON_FILE_BYTES, MAX_JSONL_FILE_BYTES, open_lines, read_file};
use crate::error::{Context, EvalError, Result};
use crate::ids::{Id, split_task_name};
use crate::metrics::{RunMetrics, compute};
use crate::transcript::{Shapes, Source, Transcript};

/// The deepest a trial directory is looked for under the report's input.
const MAX_WALK_DEPTH: u32 = 6;
/// The most directory entries the walk visits.
const MAX_WALK_ENTRIES: usize = 200_000;
/// The most transcript files read for one run.
const MAX_SESSION_FILES: usize = 4096;

/// The verifier's verdict on a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The test command passed.
    Success,
    /// The test command failed.
    Failure,
    /// No reward was recorded.
    NoVerdict,
}

impl Verdict {
    /// The verdict's report name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::NoVerdict => "no-verdict",
        }
    }
}

/// How well a run's transcript could be read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DataQuality {
    /// Lines that were not JSON objects.
    pub malformed_lines: u64,
    /// Lines over the line limit.
    pub oversized_lines: u64,
    /// Files read only up to their limit.
    pub cut_files: u64,
    /// Lines repeated by uuid.
    pub duplicate_lines: u64,
}

/// One trial, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The arm.
    pub arm: Id,
    /// The task.
    pub task: Id,
    /// The verdict.
    pub verdict: Verdict,
    /// Which transcript the metrics came from; `None` when the trial left none.
    pub source: Option<Source>,
    /// The metrics.
    pub metrics: RunMetrics,
    /// How well the transcript read.
    pub quality: DataQuality,
    /// The schema names seen in the transcript.
    pub shapes: Shapes,
}

/// What a walk over a report's input found.
#[derive(Debug, Default)]
pub struct Found {
    /// The runs read.
    pub runs: Vec<Run>,
    /// Trial directories whose `task_name` was not one `jfl-eval harbor` writes.
    pub skipped_foreign: u64,
    /// Trial directories that could not be read: a `result.json` that is missing, a symlink,
    /// too large or not JSON, or a transcript whose read failed (I/O, too many files).
    pub skipped_unreadable: u64,
}

/// Every trial directory under `root` (or `root` itself), in path order.
pub fn find_trials(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut visited = 0usize;
    walk(root, 0, &mut visited, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, depth: u32, visited: &mut usize, out: &mut Vec<PathBuf>) -> Result<()> {
    if is_trial(dir) {
        out.push(dir.to_path_buf());
        return Ok(());
    }
    if depth >= MAX_WALK_DEPTH {
        return Ok(());
    }
    let entries = fs::read_dir(dir).context(format!("read {}", dir.display()))?;
    let mut children = Vec::new();
    for entry in entries {
        let entry = entry.context(format!("read {}", dir.display()))?;
        *visited = visited.saturating_add(1);
        if *visited > MAX_WALK_ENTRIES {
            return Err(EvalError::new(format!(
                "more than {MAX_WALK_ENTRIES} entries under the input; point at one job"
            )));
        }
        // Symlinks are not followed: a run's directory cannot lead the walk elsewhere.
        let file_type = entry.file_type().context("file type")?;
        if file_type.is_dir() {
            children.push(entry.path());
        }
    }
    children.sort();
    for child in children {
        walk(&child, depth.saturating_add(1), visited, out)?;
    }
    Ok(())
}

fn is_trial(dir: &Path) -> bool {
    is_plain_file(&dir.join("result.json")) && is_plain_dir(&dir.join("agent"))
}

/// `base` joined with `parts`, if every component below `base` is a real directory and none is a
/// symlink. A run's `agent/` is written by the agent inside its container, so a symlink anywhere
/// in it (`agent/sessions` → `/`) must not lead a read onto the host. `symlink_metadata` checks
/// one component, so each is checked. Pinned by:
/// `tests::should_read_no_transcript_when_a_directory_on_the_way_is_a_symlink`.
fn plain_dir_chain(base: &Path, parts: &[&str]) -> Option<PathBuf> {
    let mut at = base.to_path_buf();
    if !is_plain_dir(&at) {
        return None;
    }
    for part in parts {
        at.push(part);
        if !is_plain_dir(&at) {
            return None;
        }
    }
    Some(at)
}

fn is_plain_file(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file())
}

fn is_plain_dir(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir())
}

/// Read every trial under `root`.
pub fn read_all(root: &Path) -> Result<Found> {
    let mut found = Found::default();
    for trial in find_trials(root)? {
        match read_trial(&trial) {
            Ok(Some(run)) => found.runs.push(run),
            Ok(None) => found.skipped_foreign = found.skipped_foreign.saturating_add(1),
            Err(_) => found.skipped_unreadable = found.skipped_unreadable.saturating_add(1),
        }
    }
    Ok(found)
}

/// Read one trial directory; `None` when its task was not written by `jfl-eval harbor`.
pub fn read_trial(dir: &Path) -> Result<Option<Run>> {
    let result_path = dir.join("result.json");
    if !is_plain_file(&result_path) {
        return Err(EvalError::new(format!(
            "{}: not a regular file (missing, or a symlink)",
            result_path.display()
        )));
    }
    let bytes = read_file(&result_path, MAX_JSON_FILE_BYTES)?;
    let result: Value = serde_json::from_slice(&bytes)
        .map_err(|_| EvalError::new(format!("{}: not JSON", result_path.display())))?;
    match result.get("schema_version") {
        None | Some(Value::Null) => {}
        Some(v) if v.as_u64() == Some(1) => {}
        Some(_) => {
            return Err(EvalError::new(format!(
                "{}: a TrialResult schema newer than 1",
                result_path.display()
            )));
        }
    }
    let task_name = result
        .get("task_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let Some((arm, task)) = split_task_name(task_name) else {
        return Ok(None);
    };
    let verdict = verdict(&result);
    let transcript = read_transcript(&dir.join("agent"))?;
    let (source, metrics, quality, shapes) = match transcript {
        Some(t) => (
            Some(t.source),
            compute(&t),
            DataQuality {
                malformed_lines: t.malformed_lines,
                oversized_lines: t.oversized_lines,
                cut_files: t.cut_files,
                duplicate_lines: t.duplicate_lines,
            },
            t.shapes,
        ),
        None => (
            None,
            RunMetrics::default(),
            DataQuality::default(),
            Shapes::default(),
        ),
    };
    Ok(Some(Run {
        arm,
        task,
        verdict,
        source,
        metrics,
        quality,
        shapes,
    }))
}

fn verdict(result: &Value) -> Verdict {
    let reward = result
        .get("verifier_result")
        .and_then(|v| v.get("rewards"))
        .and_then(|r| r.get("reward"))
        .and_then(Value::as_f64);
    match reward {
        Some(r) if r.is_finite() && r >= 1.0 => Verdict::Success,
        Some(r) if r.is_finite() => Verdict::Failure,
        _ => Verdict::NoVerdict,
    }
}

/// The run's transcript: the session JSONL if there is any, else the ATIF trajectory, else the
/// stream-json log; `None` when the agent left none of them.
///
/// Nothing is read through a symlink: not `agent` itself, not a directory below it, not a file.
pub fn read_transcript(agent: &Path) -> Result<Option<Transcript>> {
    if !is_plain_dir(agent) {
        return Ok(None);
    }
    let sessions = match plain_dir_chain(agent, &["sessions", "projects"]) {
        Some(projects) => session_files(&projects)?,
        None => Vec::new(),
    };
    if !sessions.is_empty() {
        let mut t = Transcript::new(Source::Session);
        for file in &sessions {
            read_jsonl_file(&mut t, file)?;
        }
        t.finish();
        return Ok(Some(t));
    }
    let atif = agent.join("trajectory.json");
    if is_plain_file(&atif) {
        let mut t = Transcript::new(Source::Atif);
        let bytes = read_file(&atif, MAX_JSON_FILE_BYTES)?;
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) => t.read_atif(&v),
            Err(_) => t.malformed_lines = 1,
        }
        t.finish();
        return Ok(Some(t));
    }
    let stream = agent.join("claude-code.txt");
    if is_plain_file(&stream) {
        let mut t = Transcript::new(Source::StreamJson);
        read_jsonl_file(&mut t, &stream)?;
        t.finish();
        return Ok(Some(t));
    }
    Ok(None)
}

fn read_jsonl_file(t: &mut Transcript, file: &Path) -> Result<()> {
    let (mut reader, cut) = open_lines(file, MAX_JSONL_FILE_BYTES)?;
    if cut {
        t.cut_files = t.cut_files.saturating_add(1);
    }
    t.read_jsonl(&mut reader)
        .context(format!("read {}", file.display()))
}

/// Every `*.jsonl` under `projects` (checked by the caller), in path order. Entries are typed
/// without following links (`DirEntry::file_type`), so a symlinked file or directory is skipped.
fn session_files(projects: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_jsonl(projects, 0, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_jsonl(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) -> Result<()> {
    if depth > 4 {
        return Ok(());
    }
    for entry in fs::read_dir(dir).context(format!("read {}", dir.display()))? {
        let entry = entry.context(format!("read {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry.file_type().context("file type")?;
        if file_type.is_dir() {
            collect_jsonl(&path, depth.saturating_add(1), out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
            if out.len() >= MAX_SESSION_FILES {
                return Err(EvalError::new(format!(
                    "{}: more than {MAX_SESSION_FILES} transcript files",
                    dir.display()
                )));
            }
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn agent_dir(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("jfl-eval-run-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("agent")).unwrap();
        Dir(dir)
    }

    #[test]
    fn should_read_the_stream_json_log_when_the_run_left_no_session_or_trajectory() {
        // given: `claude -p --output-format stream-json` lines, which carry no timestamps
        let dir = agent_dir("stream");
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"s","tools":["LSP","Bash"]}"#,
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"LSP","input":{"operation":"hover"}}],"usage":{"input_tokens":4,"output_tokens":2}}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"long total()"}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"num_turns":2,"usage":{"input_tokens":4,"output_tokens":2}}"#,
        ];
        fs::write(dir.0.join("agent/claude-code.txt"), lines.join("\n")).unwrap();
        // when
        let t = read_transcript(&dir.0.join("agent")).unwrap().unwrap();
        let m = compute(&t);
        // then
        assert_eq!(t.source, Source::StreamJson);
        assert_eq!((m.lsp_calls, m.tokens_input, m.tokens_output), (1, 4, 2));
        assert_eq!(m.time_to_first_lsp_answer_ms, None);
    }

    #[cfg(unix)]
    #[test]
    fn should_read_no_transcript_when_a_directory_on_the_way_is_a_symlink() {
        // given: agent/sessions links to a directory outside the run that holds a transcript
        let dir = agent_dir("symlink");
        let outside = dir.0.join("outside");
        fs::create_dir_all(outside.join("projects/-app")).unwrap();
        fs::write(
            outside.join("projects/-app/s.jsonl"),
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t","name":"Read","input":{}}]}}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, dir.0.join("agent/sessions")).unwrap();
        // when
        let t = read_transcript(&dir.0.join("agent")).unwrap();
        // then
        assert_eq!(t, None);
    }

    #[cfg(unix)]
    #[test]
    fn should_refuse_a_trial_when_its_result_or_agent_is_a_symlink() {
        // given: a result.json that is a link, and an agent/ that is a link
        let dir = agent_dir("symlink-result");
        let real = dir.0.join("real.json");
        fs::write(&real, r#"{"task_name":"arm__task"}"#).unwrap();
        std::os::unix::fs::symlink(&real, dir.0.join("result.json")).unwrap();
        let linked_agent = dir.0.join("linked");
        std::os::unix::fs::symlink(dir.0.join("agent"), &linked_agent).unwrap();
        // when
        let trial = read_trial(&dir.0);
        let agent = read_transcript(&linked_agent).unwrap();
        // then
        assert!(trial.is_err());
        assert_eq!(agent, None);
    }

    #[test]
    fn should_give_no_verdict_when_the_reward_is_missing_or_not_finite() {
        // given
        let none = serde_json::json!({"verifier_result": null});
        let text = serde_json::json!({"verifier_result": {"rewards": {"reward": "1"}}});
        let half = serde_json::json!({"verifier_result": {"rewards": {"reward": 0.5}}});
        let one = serde_json::json!({"verifier_result": {"rewards": {"reward": 1}}});
        // when
        let verdicts = [
            verdict(&none),
            verdict(&text),
            verdict(&half),
            verdict(&one),
        ];
        // then
        assert_eq!(
            verdicts,
            [
                Verdict::NoVerdict,
                Verdict::NoVerdict,
                Verdict::Failure,
                Verdict::Success
            ]
        );
    }
}
