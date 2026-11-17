//! Claude Code transcripts and Harbor trajectories into one stream of [`Event`]s.
//!
//! Three inputs, read in this order of preference by [`crate::run`]:
//!
//! 1. **Claude Code's session JSONL** (`$CLAUDE_CONFIG_DIR/projects/<cwd-slug>/<session>.jsonl`,
//!    and `<session>/subagents/*.jsonl` for subagents; Harbor 0.26.0 sets `CLAUDE_CONFIG_DIR` to
//!    the trial's `agent/sessions`). One JSON object per line; the lines read here are
//!    `{"type":"assistant","timestamp":…,"message":{"id":…,"content":[{"type":"tool_use","id":…,
//!    "name":…,"input":{…}}],"usage":{…}}}`, `{"type":"user","message":{"content":[{"type":
//!    "tool_result","tool_use_id":…,"is_error":…,"content":…}]}}` and `{"type":"attachment",
//!    "attachment":{"type":…}}`. Streaming repeats a message's usage under one `message.id`; the
//!    last one counts (as Harbor does).
//! 2. **Harbor's ATIF trajectory** (`agent/trajectory.json`, ATIF-v1.0 to v1.8): `steps[]` with
//!    `tool_calls[]`, `observation.results[]` and `metrics`, and `subagent_trajectories[]`.
//!    Harbor's converter drops attachment lines, so pushed diagnostics are not observable here.
//! 3. **`claude -p --output-format stream-json`** (`agent/claude-code.txt`): the same assistant
//!    and user lines as 1, without timestamps; the thin runner of ADR 0029 would write this.
//!
//! **Assumptions the smoke task checks** (`jfl-eval inspect` prints the shapes): the LSP tool is
//! named `LSP` and names its operation in `input.operation`; pushed diagnostics arrive as an
//! `attachment` whose `type` contains `diagnostic`, with LSP `severity` values (`1` or `"Error"`)
//! on its entries; a failing Bash command's result has `is_error: true`.
//!
//! The reader is bounded and total: a line over [`MAX_LINE_BYTES`] is skipped and counted, a line
//! that is not a JSON object (garbage, or the truncated last line of a killed run) is skipped and
//! counted, and no input makes it panic (pinned by
//! `tests::should_read_without_panic_when_a_transcript_is_cut_or_corrupted_anywhere`). Text from the transcript stays inside the reader: an
//! [`Event`] holds kinds, flags, counts and timestamps, plus the transcript's own call ids for
//! pairing a result with its call.

use std::collections::{BTreeMap, HashSet};
use std::io::BufRead;

use serde::Serialize;
use serde_json::Value;

use crate::bounded::{Line, MAX_LINE_BYTES, next_line};

/// What kind of tool a call used. Only these names reach a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolKind {
    /// `Read`.
    Read,
    /// `Edit`, `MultiEdit`, `Write`, `NotebookEdit`.
    Edit,
    /// `Bash`.
    Bash,
    /// `Grep`.
    Grep,
    /// `Glob`.
    Glob,
    /// `LSP`.
    Lsp,
    /// `Task` or `Agent` (a subagent).
    Agent,
    /// `TodoWrite`.
    Todo,
    /// `WebFetch`, `WebSearch`.
    Web,
    /// `Skill`.
    Skill,
    /// Any `mcp__…` tool.
    Mcp,
    /// Anything else.
    Other,
}

impl ToolKind {
    /// The kind of a tool name.
    pub fn of(name: &str) -> Self {
        match name {
            "Read" => Self::Read,
            "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => Self::Edit,
            "Bash" => Self::Bash,
            "Grep" => Self::Grep,
            "Glob" => Self::Glob,
            "LSP" => Self::Lsp,
            "Task" | "Agent" => Self::Agent,
            "TodoWrite" => Self::Todo,
            "WebFetch" | "WebSearch" => Self::Web,
            "Skill" => Self::Skill,
            n if n.starts_with("mcp__") => Self::Mcp,
            _ => Self::Other,
        }
    }

    /// The kind's report name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Bash => "bash",
            Self::Grep => "grep",
            Self::Glob => "glob",
            Self::Lsp => "lsp",
            Self::Agent => "agent",
            Self::Todo => "todo",
            Self::Web => "web",
            Self::Skill => "skill",
            Self::Mcp => "mcp",
            Self::Other => "other",
        }
    }
}

/// The LSP operations Claude Code's `LSP` tool offers; anything else is `other`.
pub const LSP_OPERATIONS: [&str; 9] = [
    "goToDefinition",
    "findReferences",
    "hover",
    "documentSymbol",
    "workspaceSymbol",
    "goToImplementation",
    "prepareCallHierarchy",
    "incomingCalls",
    "outgoingCalls",
];

/// The report name of an LSP operation: one of [`LSP_OPERATIONS`] or `other`.
pub fn lsp_operation(input: &Value) -> &'static str {
    let op = input.get("operation").and_then(Value::as_str).unwrap_or("");
    LSP_OPERATIONS
        .iter()
        .copied()
        .find(|known| *known == op)
        .unwrap_or("other")
}

/// Tokens of one model call, Harbor's way: `input` includes cache reads and cache writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    /// Prompt tokens, cached and uncached.
    pub input: u64,
    /// The cache reads among them.
    pub cached: u64,
    /// Completion tokens.
    pub output: u64,
}

/// One thing that happened in a run, in transcript order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The agent called a tool.
    ToolCall {
        /// The transcript's id of the call, to pair it with its result.
        id: String,
        /// The tool's kind.
        kind: ToolKind,
        /// The LSP operation, for an `LSP` call.
        lsp_op: Option<&'static str>,
        /// A Bash command that runs a Java build (`gradle`, `gradlew`, `javac`, `mvn`, `mvnw`).
        build: bool,
        /// Milliseconds since the Unix epoch, if the line had a timestamp.
        at_ms: Option<i64>,
    },
    /// A tool answered.
    ToolResult {
        /// The id of the call it answers.
        id: String,
        /// The tool reported failure.
        is_error: bool,
        /// The answer holds nothing (empty, `[]`, `null`, or "No references found").
        empty: bool,
        /// The answer looks like a Java compilation failure.
        compile_failure: bool,
        /// Milliseconds since the Unix epoch, if the line had a timestamp.
        at_ms: Option<i64>,
    },
    /// Claude Code pushed diagnostics to the agent.
    Diagnostics {
        /// Entries of error severity among them.
        errors: u32,
        /// Milliseconds since the Unix epoch, if the line had a timestamp.
        at_ms: Option<i64>,
    },
    /// A model call's token usage; a later one with the same message id replaces it.
    Usage {
        /// The model message's id, if any.
        message_id: Option<String>,
        /// Its tokens.
        tokens: Tokens,
    },
}

impl Event {
    /// The event's timestamp, if it had one.
    pub fn at_ms(&self) -> Option<i64> {
        match self {
            Self::ToolCall { at_ms, .. }
            | Self::ToolResult { at_ms, .. }
            | Self::Diagnostics { at_ms, .. } => *at_ms,
            Self::Usage { .. } => None,
        }
    }
}

/// Which input a transcript came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Claude Code's session JSONL.
    Session,
    /// Harbor's ATIF trajectory.
    Atif,
    /// `claude -p --output-format stream-json`.
    StreamJson,
}

impl Source {
    /// The source's report name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Atif => "atif",
            Self::StreamJson => "stream-json",
        }
    }

    /// Whether pushed diagnostics can appear in this source at all.
    pub fn shows_diagnostics(self) -> bool {
        !matches!(self, Self::Atif)
    }
}

/// The schema names seen while reading, for checking this module's assumptions against a real
/// run: line and attachment types, tool names (an MCP tool as `mcp__*`), the `LSP` tool's input
/// keys and its operations (one of [`LSP_OPERATIONS`] or `other`). Only identifier-shaped names
/// are kept, the rest count as `other`, so no prose or path reaches it. Pinned by
/// `tests::should_keep_only_schema_names_when_recording_shapes` and `tests/report.rs`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Shapes {
    /// Top-level `type` of each line, with counts.
    pub line_types: BTreeMap<String, u64>,
    /// `attachment.type` of attachment lines, with counts.
    pub attachment_types: BTreeMap<String, u64>,
    /// Tool names called, with counts.
    pub tool_names: BTreeMap<String, u64>,
    /// Input keys of `LSP` calls, with counts.
    pub lsp_input_keys: BTreeMap<String, u64>,
    /// `input.operation` of `LSP` calls, with counts.
    pub lsp_operations: BTreeMap<String, u64>,
}

/// A transcript read into events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// Where it came from.
    pub source: Source,
    /// The events, in order.
    pub events: Vec<Event>,
    /// The earliest timestamp of any line: the session's start.
    pub start_ms: Option<i64>,
    /// Lines that were not a JSON object (garbage, truncation).
    pub malformed_lines: u64,
    /// Lines over [`MAX_LINE_BYTES`].
    pub oversized_lines: u64,
    /// Files read only up to their limit.
    pub cut_files: u64,
    /// Lines skipped because their `uuid` was seen before (a resumed session repeats lines).
    pub duplicate_lines: u64,
    /// The schema names seen.
    pub shapes: Shapes,
    seen_uuids: HashSet<String>,
}

impl Transcript {
    /// An empty transcript of `source`.
    pub fn new(source: Source) -> Self {
        Self {
            source,
            events: Vec::new(),
            start_ms: None,
            malformed_lines: 0,
            oversized_lines: 0,
            cut_files: 0,
            duplicate_lines: 0,
            shapes: Shapes::default(),
            seen_uuids: HashSet::new(),
        }
    }

    /// Read one JSONL file's lines (a session file, a subagent's, or stream-json) into this
    /// transcript.
    pub fn read_jsonl(&mut self, reader: &mut impl BufRead) -> std::io::Result<()> {
        self.read_jsonl_with_limit(reader, MAX_LINE_BYTES)
    }

    fn read_jsonl_with_limit(
        &mut self,
        reader: &mut impl BufRead,
        max_line: usize,
    ) -> std::io::Result<()> {
        while let Some(line) = next_line(reader, max_line)? {
            match line {
                Line::TooLong => bump(&mut self.oversized_lines),
                Line::Kept(bytes) => {
                    if bytes.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    match serde_json::from_slice::<Value>(&bytes) {
                        Ok(Value::Object(map)) => self.session_line(&Value::Object(map)),
                        _ => bump(&mut self.malformed_lines),
                    }
                }
            }
        }
        Ok(())
    }

    /// Order the events of several files by time, when every event that can carry a time has one;
    /// otherwise keep reading order. Stable, so one file's order survives equal timestamps.
    pub fn finish(&mut self) {
        let timed = self
            .events
            .iter()
            .filter(|e| !matches!(e, Event::Usage { .. }))
            .all(|e| e.at_ms().is_some());
        if timed {
            // Usage events have no time; keep each next to the event before it.
            let mut last = i64::MIN;
            let mut keyed: Vec<(i64, Event)> = Vec::with_capacity(self.events.len());
            for e in self.events.drain(..) {
                if let Some(t) = e.at_ms() {
                    last = t;
                }
                keyed.push((last, e));
            }
            keyed.sort_by_key(|(t, _)| *t);
            self.events = keyed.into_iter().map(|(_, e)| e).collect();
        }
    }

    fn session_line(&mut self, line: &Value) {
        if let Some(uuid) = line.get("uuid").and_then(Value::as_str)
            && !self.seen_uuids.insert(uuid.to_string())
        {
            bump(&mut self.duplicate_lines);
            return;
        }
        let kind = line.get("type").and_then(Value::as_str).unwrap_or("");
        count_name(&mut self.shapes.line_types, kind);
        let at_ms = line
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp_ms);
        if let Some(t) = at_ms {
            self.start_ms = Some(self.start_ms.map_or(t, |s| s.min(t)));
        }
        let message = line.get("message");
        let content = message.and_then(|m| m.get("content"));
        match kind {
            "assistant" => {
                if let Some(blocks) = content.and_then(Value::as_array) {
                    for block in blocks {
                        if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                            let id = str_field(block, "id");
                            let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                            let input = block.get("input").unwrap_or(&Value::Null);
                            self.tool_call(id, name, input, at_ms);
                        }
                    }
                }
                if let Some(usage) = message.and_then(|m| m.get("usage")) {
                    let message_id = message
                        .and_then(|m| m.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    self.events.push(Event::Usage {
                        message_id,
                        tokens: anthropic_tokens(usage),
                    });
                }
            }
            "user" => {
                if let Some(blocks) = content.and_then(Value::as_array) {
                    for block in blocks {
                        if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                            let id = str_field(block, "tool_use_id");
                            let is_error =
                                block.get("is_error").and_then(Value::as_bool) == Some(true);
                            let text = content_text(block.get("content").unwrap_or(&Value::Null));
                            self.events.push(result_event(id, is_error, &text, at_ms));
                        }
                    }
                }
            }
            "attachment" => {
                let attachment = line.get("attachment").unwrap_or(&Value::Null);
                let sub = attachment.get("type").and_then(Value::as_str).unwrap_or("");
                count_name(&mut self.shapes.attachment_types, sub);
                if sub.to_ascii_lowercase().contains("diagnostic") {
                    self.events.push(Event::Diagnostics {
                        errors: error_severities(attachment, 0),
                        at_ms,
                    });
                }
            }
            _ => {}
        }
    }

    /// Read a Harbor ATIF trajectory (`trajectory.json`, parsed) into this transcript, subagent
    /// trajectories included.
    pub fn read_atif(&mut self, trajectory: &Value) {
        self.atif(trajectory, 0);
    }

    fn atif(&mut self, trajectory: &Value, depth: u32) {
        if depth > MAX_SUBAGENT_DEPTH {
            return;
        }
        let steps = trajectory.get("steps").and_then(Value::as_array);
        for step in steps.into_iter().flatten() {
            let at_ms = step
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_timestamp_ms);
            if let Some(t) = at_ms {
                self.start_ms = Some(self.start_ms.map_or(t, |s| s.min(t)));
            }
            let calls = step.get("tool_calls").and_then(Value::as_array);
            for call in calls.into_iter().flatten() {
                let id = str_field(call, "tool_call_id");
                let name = call
                    .get("function_name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let input = call.get("arguments").unwrap_or(&Value::Null);
                self.tool_call(id, name, input, at_ms);
            }
            let results = step
                .get("observation")
                .and_then(|o| o.get("results"))
                .and_then(Value::as_array);
            for result in results.into_iter().flatten() {
                let id = str_field(result, "source_call_id");
                let extra = result.get("extra");
                let is_error = extra.is_some_and(|x| {
                    x.get("tool_result_is_error").and_then(Value::as_bool) == Some(true)
                        || x.get("metadata")
                            .and_then(|m| m.get("is_error"))
                            .and_then(Value::as_bool)
                            == Some(true)
                });
                let text = content_text(result.get("content").unwrap_or(&Value::Null));
                self.events.push(result_event(id, is_error, &text, at_ms));
            }
            if let Some(m) = step.get("metrics") {
                let n = |k: &str| m.get(k).and_then(Value::as_u64).unwrap_or(0);
                self.events.push(Event::Usage {
                    message_id: None,
                    tokens: Tokens {
                        input: n("prompt_tokens"),
                        cached: n("cached_tokens"),
                        output: n("completion_tokens"),
                    },
                });
            }
        }
        let subagents = trajectory
            .get("subagent_trajectories")
            .and_then(Value::as_array);
        for sub in subagents.into_iter().flatten() {
            self.atif(sub, depth.saturating_add(1));
        }
    }

    fn tool_call(&mut self, id: String, name: &str, input: &Value, at_ms: Option<i64>) {
        // An MCP tool's name carries its server's name, which is configuration, not schema.
        if name.starts_with("mcp__") {
            bump(
                self.shapes
                    .tool_names
                    .entry("mcp__*".to_string())
                    .or_insert(0),
            );
        } else {
            count_name(&mut self.shapes.tool_names, name);
        }
        let kind = ToolKind::of(name);
        let lsp_op = (kind == ToolKind::Lsp).then(|| {
            if let Some(map) = input.as_object() {
                for key in map.keys() {
                    count_name(&mut self.shapes.lsp_input_keys, key);
                }
            }
            // Only the known operations: any other value is the agent's text.
            if input.get("operation").is_some() {
                count_name(&mut self.shapes.lsp_operations, lsp_operation(input));
            }
            lsp_operation(input)
        });
        let build = kind == ToolKind::Bash
            && input
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(runs_java_build);
        self.events.push(Event::ToolCall {
            id,
            kind,
            lsp_op,
            build,
            at_ms,
        });
    }
}

/// Subagent trajectories nest at most this deep (Claude Code's own limit is lower).
const MAX_SUBAGENT_DEPTH: u32 = 8;
/// The most distinct names a [`Shapes`] map keeps; the rest count as `other`.
const MAX_SHAPE_NAMES: usize = 256;

fn bump(n: &mut u64) {
    *n = n.saturating_add(1);
}

/// Count `name` if it looks like a schema name (`[A-Za-z0-9_-]`, 1 to 64 characters, starting
/// with a letter); anything else, which could be text, counts as `other`.
fn count_name(map: &mut BTreeMap<String, u64>, name: &str) {
    let shaped = name.len() <= 64
        && name.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    let key = if shaped && (map.contains_key(name) || map.len() < MAX_SHAPE_NAMES) {
        name
    } else {
        "other"
    };
    bump(map.entry(key.to_string()).or_insert(0));
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn anthropic_tokens(usage: &Value) -> Tokens {
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let cached = n("cache_read_input_tokens");
    Tokens {
        input: n("input_tokens")
            .saturating_add(cached)
            .saturating_add(n("cache_creation_input_tokens")),
        cached,
        output: n("output_tokens"),
    }
}

/// A result's text: a string, or the `text` of an array of content blocks.
fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str).or_else(|| p.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn result_event(id: String, is_error: bool, text: &str, at_ms: Option<i64>) -> Event {
    Event::ToolResult {
        id,
        is_error,
        empty: is_empty_answer(text),
        compile_failure: is_error && looks_like_compile_failure(text),
        at_ms,
    }
}

/// An answer with nothing in it: blank, `[]`, `null`, or a "no references" message.
pub fn is_empty_answer(text: &str) -> bool {
    let t = text.trim();
    t.is_empty()
        || t == "[]"
        || t == "null"
        || t.to_ascii_lowercase().starts_with("no references found")
}

/// Gradle's, Maven's or javac's words for a compilation that failed.
pub fn looks_like_compile_failure(text: &str) -> bool {
    text.contains("Compilation failed")
        || text.contains("COMPILATION ERROR")
        || text.contains("compileJava FAILED")
        || text.contains("compileTestJava FAILED")
        || (text.contains(".java:") && text.contains(": error:"))
}

/// Whether a shell command runs a Java build: some simple command in it (split at `;`, `&`, `|`,
/// parentheses and newlines) whose program, after `env`, `time`, `timeout <n>`, `nice` or
/// `nohup` and `NAME=value` words, has `gradle`, `gradlew`, `javac`, `mvn` or `mvnw` as its last
/// path segment. A word elsewhere (`grep gradle build.gradle`) is not a build.
pub fn runs_java_build(command: &str) -> bool {
    command
        .split(|c: char| ";&|()\n`".contains(c))
        .filter_map(|simple| {
            simple.split_whitespace().find(|word| {
                !word.contains('=')
                    && !matches!(
                        *word,
                        "env" | "time" | "timeout" | "nice" | "nohup" | "exec"
                    )
                    && !word
                        .bytes()
                        .all(|b| b.is_ascii_digit() || b"smh.".contains(&b))
            })
        })
        .filter_map(|program| program.rsplit('/').next())
        .any(|base| matches!(base, "gradle" | "gradlew" | "javac" | "mvn" | "mvnw"))
}

/// Entries of error severity anywhere under `v`: an object whose `severity` is `1`, `"1"`,
/// `"error"` or `"Error"` (LSP's `DiagnosticSeverity.Error`).
fn error_severities(v: &Value, depth: u32) -> u32 {
    if depth > 32 {
        return 0;
    }
    let next = depth.saturating_add(1);
    match v {
        Value::Object(map) => {
            let own = match map.get("severity") {
                Some(Value::Number(n)) => u32::from(n.as_u64() == Some(1)),
                Some(Value::String(s)) => u32::from(s == "1" || s.eq_ignore_ascii_case("error")),
                _ => 0,
            };
            map.values().fold(own, |acc, child| {
                acc.saturating_add(error_severities(child, next))
            })
        }
        Value::Array(items) => items.iter().fold(0, |acc, child| {
            acc.saturating_add(error_severities(child, next))
        }),
        _ => 0,
    }
}

/// An RFC 3339 timestamp (`2026-11-17T09:00:00.123Z`, or with a `±hh:mm` offset) in
/// milliseconds since the Unix epoch; `None` for anything else.
pub fn parse_timestamp_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let digits = b.get(from..from.checked_add(len)?)?;
        digits.iter().try_fold(0i64, |acc, &d| {
            d.is_ascii_digit()
                .then(|| {
                    acc.checked_mul(10)?
                        .checked_add(i64::from(d.wrapping_sub(b'0')))
                })
                .flatten()
        })
    };
    let sep = |at: usize, c: u8| b.get(at) == Some(&c);
    if !(sep(4, b'-') && sep(7, b'-') && (sep(10, b'T') || sep(10, b't') || sep(10, b' '))) {
        return None;
    }
    if !(sep(13, b':') && sep(16, b':')) {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hour, minute, second) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut at = 19usize;
    let mut millis = 0i64;
    if sep(at, b'.') {
        at = at.checked_add(1)?;
        let mut scale = 100i64;
        while let Some(d) = b.get(at).filter(|d| d.is_ascii_digit()) {
            millis = millis.checked_add(i64::from(d.wrapping_sub(b'0')).checked_mul(scale)?)?;
            scale = scale.checked_div(10)?;
            at = at.checked_add(1)?;
        }
    }
    let offset_ms = match b.get(at) {
        Some(b'Z' | b'z') if at.checked_add(1)? == b.len() => 0,
        Some(&c @ (b'+' | b'-'))
            if at.checked_add(6)? == b.len() && sep(at.checked_add(3)?, b':') =>
        {
            let mins = num(at.checked_add(1)?, 2)?
                .checked_mul(60)?
                .checked_add(num(at.checked_add(4)?, 2)?)?;
            let ms = mins.checked_mul(60_000)?;
            if c == b'+' { ms } else { ms.checked_neg()? }
        }
        _ => return None,
    };
    let days = days_from_civil(year, month, day)?;
    days.checked_mul(86_400_000)?
        .checked_add(hour.checked_mul(3_600_000)?)?
        .checked_add(minute.checked_mul(60_000)?)?
        .checked_add(second.checked_mul(1000)?)?
        .checked_add(millis)?
        .checked_sub(offset_ms)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let y = if month <= 2 {
        year.checked_sub(1)?
    } else {
        year
    };
    let era = y.div_euclid(400);
    let yoe = y.checked_sub(era.checked_mul(400)?)?;
    let mp = if month > 2 {
        month.checked_sub(3)?
    } else {
        month.checked_add(9)?
    };
    let doy = mp
        .checked_mul(153)?
        .checked_add(2)?
        .checked_div(5)?
        .checked_add(day)?
        .checked_sub(1)?;
    let doe = yoe
        .checked_mul(365)?
        .checked_add(yoe.checked_div(4)?)?
        .checked_sub(yoe.checked_div(100)?)?
        .checked_add(doy)?;
    era.checked_mul(146_097)?
        .checked_add(doe)?
        .checked_sub(719_468)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const CALL: &str = r#"{"type":"assistant","uuid":"u1","timestamp":"2026-11-17T09:00:01.000Z","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"LSP","input":{"operation":"hover","filePath":"/app/src/Secret.java"}}],"usage":{"input_tokens":3,"output_tokens":4}}}"#;

    fn read(text: &str) -> Transcript {
        let mut t = Transcript::new(Source::Session);
        t.read_jsonl(&mut Cursor::new(text.as_bytes().to_vec()))
            .unwrap();
        t.finish();
        t
    }

    #[test]
    fn should_count_and_skip_garbage_when_a_line_is_not_json() {
        // given
        let text = format!("not json at all\n{CALL}\n[1,2,3]\n\u{0}\u{1}\u{2}\n");
        // when
        let t = read(&text);
        // then
        assert_eq!(t.malformed_lines, 3);
        assert_eq!(t.events.len(), 2, "{:?}", t.events);
    }

    #[test]
    fn should_skip_the_cut_line_when_the_file_is_truncated_mid_line() {
        // given: a killed run leaves half a line
        let half = &CALL[..CALL.len() / 2];
        let text = format!("{CALL}\n{half}");
        // when
        let t = read(&text);
        // then
        assert_eq!(t.malformed_lines, 1);
        assert_eq!(t.events.len(), 2);
    }

    #[test]
    fn should_count_and_skip_a_line_when_it_is_over_the_limit() {
        // given
        let limit = CALL.len().saturating_add(1);
        let long = format!(r#"{{"type":"user","pad":"{}"}}"#, "x".repeat(limit));
        let mut t = Transcript::new(Source::Session);
        // when
        t.read_jsonl_with_limit(&mut Cursor::new(format!("{long}\n{CALL}\n")), limit)
            .unwrap();
        // then
        assert_eq!(t.oversized_lines, 1);
        assert_eq!(t.events.len(), 2);
    }

    #[test]
    fn should_read_nothing_when_fields_have_the_wrong_types() {
        // given: every field of interest present with a wrong type
        let text = r#"{"type":"assistant","timestamp":7,"message":{"content":"x","usage":[]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":5,"is_error":"yes","content":{"a":1}}]}}
{"type":"attachment","attachment":"diagnostics"}
{"type":7}"#;
        // when
        let t = read(text);
        // then: a usage of zeros and an unpaired result, no panic
        assert_eq!(t.malformed_lines, 0);
        assert_eq!(t.start_ms, None);
    }

    #[test]
    fn should_skip_a_line_when_its_uuid_was_seen() {
        // given
        let text = format!("{CALL}\n{CALL}\n");
        // when
        let t = read(&text);
        // then
        assert_eq!(t.duplicate_lines, 1);
        assert_eq!(t.events.len(), 2);
    }

    #[test]
    fn should_keep_only_schema_names_when_recording_shapes() {
        // given: a planted path in an LSP input and a planted sentence as a line type
        let mcp = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t9","name":"mcp__AcmeServer__search","input":{}},{"type":"tool_use","id":"t8","name":"LSP","input":{"operation":"LedgerServiceImpl"}}]}}"#;
        let text = format!("{CALL}\n{{\"type\":\"a planted sentence /etc/passwd\"}}\n{mcp}\n");
        // when
        let t = read(&text);
        // then
        let s = &t.shapes;
        assert_eq!(
            s.lsp_input_keys.keys().collect::<Vec<_>>(),
            ["filePath", "operation"]
        );
        assert_eq!(
            s.line_types.keys().collect::<Vec<_>>(),
            ["assistant", "other"]
        );
        let all = format!("{s:?}");
        assert_eq!(s.tool_names.keys().collect::<Vec<_>>(), ["LSP", "mcp__*"]);
        assert_eq!(
            s.lsp_operations.keys().collect::<Vec<_>>(),
            ["hover", "other"]
        );
        for planted in ["Secret", "passwd", "Acme", "LedgerServiceImpl"] {
            assert!(!all.contains(planted), "{planted} in {all}");
        }
    }

    #[test]
    fn should_read_calls_results_and_tokens_when_given_an_atif_trajectory() {
        // given: Harbor 0.26.0's ATIF, with one subagent
        let atif = serde_json::json!({
            "schema_version": "ATIF-v1.8",
            "agent": {"name": "claude-code", "version": "2.1.0"},
            "steps": [
                {"step_id": 1, "source": "user", "message": "task", "timestamp": "2026-11-17T09:00:00Z"},
                {"step_id": 2, "source": "agent", "message": "", "timestamp": "2026-11-17T09:00:02Z",
                 "tool_calls": [{"tool_call_id": "c1", "function_name": "LSP", "arguments": {"operation": "findReferences"}}],
                 "observation": {"results": [{"source_call_id": "c1", "content": "No references found"}]},
                 "metrics": {"prompt_tokens": 100, "completion_tokens": 9, "cached_tokens": 60}}
            ],
            "subagent_trajectories": [{
                "trajectory_id": "s1",
                "agent": {"name": "claude-code", "version": "2.1.0"},
                "steps": [{"step_id": 1, "source": "agent", "message": "", "timestamp": "2026-11-17T09:00:03Z",
                  "tool_calls": [{"tool_call_id": "c2", "function_name": "Bash", "arguments": {"command": "gradle build"}}],
                  "observation": {"results": [{"source_call_id": "c2", "content": "x", "extra": {"tool_result_is_error": true}}]}}]
            }]
        });
        let mut t = Transcript::new(Source::Atif);
        // when
        t.read_atif(&atif);
        t.finish();
        let m = crate::metrics::compute(&t);
        // then
        assert_eq!(m.tool_calls, 2);
        assert_eq!(m.empty_references, 1);
        assert_eq!(m.builds, 1);
        assert_eq!(
            (m.tokens_input, m.tokens_cached, m.tokens_output),
            (100, 60, 9)
        );
        assert_eq!(m.time_to_first_lsp_answer_ms, Some(2000));
        assert!(!m.diagnostics_observable);
    }

    #[test]
    fn should_parse_rfc3339_when_it_has_millis_or_an_offset() {
        // given
        // when
        // then
        assert_eq!(parse_timestamp_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_timestamp_ms("1970-01-01T00:00:01.250Z"), Some(1250));
        assert_eq!(
            parse_timestamp_ms("2026-11-17T10:00:00+01:00"),
            parse_timestamp_ms("2026-11-17T09:00:00Z")
        );
        assert_eq!(
            parse_timestamp_ms("2000-03-01T00:00:00Z"),
            Some(951_868_800_000)
        );
    }

    #[test]
    fn should_refuse_a_timestamp_when_it_is_not_rfc3339() {
        // given
        let bad = [
            "",
            "2026-11-17",
            "2026-13-01T00:00:00Z",
            "2026-11-17T09:00:00",
            "2026-11-17T09:00:00Zjunk",
            "９９９９-11-17T09:00:00Z",
        ];
        // when
        let parsed: Vec<Option<i64>> = bad.iter().map(|s| parse_timestamp_ms(s)).collect();
        // then
        assert!(parsed.iter().all(Option::is_none), "{parsed:?}");
    }

    #[test]
    fn should_recognise_a_java_build_when_the_command_runs_gradle_maven_or_javac() {
        // given
        let yes = [
            "./gradlew :core:test",
            "cd /app && gradle build",
            "mvn -q compile",
            "javac A.java",
            "(./mvnw verify)",
            "timeout 600 ./gradlew test",
            "JAVA_HOME=/j /app/gradlew build",
        ];
        let no = ["grep gradle build.gradle", "cat gradlew.bat", "echo javacx"];
        // when
        // then
        assert!(yes.iter().all(|c| runs_java_build(c)), "{yes:?}");
        assert!(!no.iter().any(|c| runs_java_build(c)), "{no:?}");
    }

    /// A seeded xorshift, so a failing case reproduces (ver §3.2).
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    #[test]
    fn should_read_without_panic_when_a_transcript_is_cut_or_corrupted_anywhere() {
        // given: a transcript with every line kind this reader handles
        let whole = [
            CALL,
            r#"{"type":"user","uuid":"u2","timestamp":"2026-11-17T09:00:02.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","is_error":false,"content":[{"type":"text","text":"No references found"}]}]}}"#,
            r#"{"type":"attachment","uuid":"u3","timestamp":"2026-11-17T09:00:03+00:00","attachment":{"type":"diagnostics","files":[{"diagnostics":[{"severity":"Error"}]}]}}"#,
        ]
        .join("\n");
        let bytes = whole.as_bytes();
        let seed = 0x5eed_e7a1_0001_u64;
        let mut rng = Rng(seed);
        // when: every prefix, then 2000 seeded corruptions of 1 to 8 bytes each
        for cut in 0..=bytes.len() {
            let mut t = Transcript::new(Source::Session);
            t.read_jsonl(&mut Cursor::new(bytes[..cut].to_vec()))
                .unwrap();
            t.finish();
            let _ = crate::metrics::compute(&t);
            // then
            assert!(t.events.len() <= 4, "seed {seed:#x}, cut {cut}");
        }
        for case in 0..2000 {
            let mut mutated = bytes.to_vec();
            for _ in 0..(1 + rng.next() % 8) {
                let at = (rng.next() % mutated.len() as u64) as usize;
                mutated[at] = (rng.next() & 0xff) as u8;
            }
            let lines = mutated.iter().filter(|&&b| b == b'\n').count() as u64 + 1;
            let mut t = Transcript::new(Source::Session);
            t.read_jsonl(&mut Cursor::new(mutated)).unwrap();
            t.finish();
            let _ = crate::metrics::compute(&t);
            // then: a skipped line is never counted twice
            assert!(t.malformed_lines <= lines, "seed {seed:#x}, case {case}");
        }
    }
}
