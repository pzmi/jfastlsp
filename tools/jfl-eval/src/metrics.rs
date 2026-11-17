//! Per-run metrics from a transcript's events (plan §1.2, §9.2; ver §11).
//!
//! | Metric | How it is read |
//! |:--|:--|
//! | tool calls by kind | every `ToolCall`, by [`ToolKind`] |
//! | tokens | the last usage of each model message, summed (Harbor's way: input includes cache) |
//! | edits followed by a compile failure | a Bash Java build whose result is an error that looks like a compilation failure, after at least one edit since the previous build |
//! | … and a pushed diagnostic before the fix | an error diagnostic between that last edit and the next edit (the fix) |
//! | edits caught by diagnostics | an edit followed by an error diagnostic before the next edit |
//! | the LSP operation histogram | every `LSP` call, by operation |
//! | empty references and the next action | a `findReferences` answer with nothing in it, and the kind of the agent's next tool call (`none` when the run ends) |
//! | time to first LSP answer | from the transcript's first timestamp to the first `LSP` result that is not an error |
//!
//! Success is not here: it is the verifier's reward, read by [`crate::run`].

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::transcript::{Event, ToolKind, Transcript};

/// What one run did, as numbers keyed by fixed names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RunMetrics {
    /// Tool calls of every kind.
    pub tool_calls: u64,
    /// Tool calls by [`ToolKind::as_str`].
    pub tool_calls_by_kind: BTreeMap<&'static str, u64>,
    /// Prompt tokens, cached included.
    pub tokens_input: u64,
    /// Cache reads among them.
    pub tokens_cached: u64,
    /// Completion tokens.
    pub tokens_output: u64,
    /// Edit, MultiEdit, Write and NotebookEdit calls.
    pub edits: u64,
    /// Java builds run by the agent.
    pub builds: u64,
    /// Builds that failed to compile after an edit.
    pub edits_then_compile_failure: u64,
    /// Of those, the ones where an error diagnostic was pushed before the next edit (the fix).
    pub compile_failures_with_diagnostic_before_fix: u64,
    /// Edits followed by an error diagnostic before the next edit.
    pub edits_with_error_diagnostic: u64,
    /// Diagnostics pushes seen, errors or not.
    pub diagnostics_events: u64,
    /// Whether the transcript could show diagnostics at all (not an ATIF trajectory).
    pub diagnostics_observable: bool,
    /// `LSP` calls.
    pub lsp_calls: u64,
    /// `LSP` results that were errors (Claude Code answers with an error when it cannot start
    /// the file's language server).
    pub lsp_errors: u64,
    /// `LSP` calls by operation.
    pub lsp_operations: BTreeMap<&'static str, u64>,
    /// `findReferences` answers with nothing in them.
    pub empty_references: u64,
    /// The agent's next tool call after each, by kind (`none` when the run ended).
    pub next_action_after_empty_references: BTreeMap<&'static str, u64>,
    /// Milliseconds from the transcript's start to the first successful `LSP` answer.
    pub time_to_first_lsp_answer_ms: Option<u64>,
}

struct Call {
    kind: ToolKind,
    lsp_op: Option<&'static str>,
    build: bool,
}

/// The metrics of one transcript.
pub fn compute(transcript: &Transcript) -> RunMetrics {
    let mut m = RunMetrics {
        diagnostics_observable: transcript.source.shows_diagnostics(),
        ..RunMetrics::default()
    };
    let mut calls: HashMap<&str, Call> = HashMap::new();
    let mut usage_by_message: BTreeMap<&str, (u64, u64, u64)> = BTreeMap::new();
    let mut usage_unkeyed = (0u64, 0u64, 0u64);
    // The broken-build state machine of the table above.
    let mut edited_since_build = false;
    let mut error_diagnostic_since_edit = false;
    let mut edit_window_counted = true;
    let mut awaiting_fix: Option<bool> = None;
    let mut awaiting_next_action = 0u64;

    for event in &transcript.events {
        match event {
            Event::ToolCall {
                id,
                kind,
                lsp_op,
                build,
                ..
            } => {
                inc(&mut m.tool_calls);
                inc(m.tool_calls_by_kind.entry(kind.as_str()).or_insert(0));
                for _ in 0..awaiting_next_action {
                    inc(m
                        .next_action_after_empty_references
                        .entry(kind.as_str())
                        .or_insert(0));
                }
                awaiting_next_action = 0;
                if let Some(op) = lsp_op {
                    inc(&mut m.lsp_calls);
                    inc(m.lsp_operations.entry(op).or_insert(0));
                }
                if *build {
                    inc(&mut m.builds);
                }
                if *kind == ToolKind::Edit {
                    inc(&mut m.edits);
                    if awaiting_fix.take() == Some(true) {
                        inc(&mut m.compile_failures_with_diagnostic_before_fix);
                    }
                    edited_since_build = true;
                    error_diagnostic_since_edit = false;
                    edit_window_counted = false;
                }
                calls.insert(
                    id.as_str(),
                    Call {
                        kind: *kind,
                        lsp_op: *lsp_op,
                        build: *build,
                    },
                );
            }
            Event::ToolResult {
                id,
                is_error,
                empty,
                compile_failure,
                at_ms,
            } => {
                let Some(call) = calls.get(id.as_str()) else {
                    continue;
                };
                if call.kind == ToolKind::Lsp {
                    if *is_error {
                        inc(&mut m.lsp_errors);
                    } else if m.time_to_first_lsp_answer_ms.is_none() {
                        m.time_to_first_lsp_answer_ms = elapsed(transcript.start_ms, *at_ms);
                    }
                    if !*is_error && *empty && call.lsp_op == Some("findReferences") {
                        inc(&mut m.empty_references);
                        awaiting_next_action = awaiting_next_action.saturating_add(1);
                    }
                }
                if call.build {
                    if *compile_failure && edited_since_build {
                        inc(&mut m.edits_then_compile_failure);
                        awaiting_fix = Some(error_diagnostic_since_edit);
                    }
                    edited_since_build = false;
                }
            }
            Event::Diagnostics { errors, .. } => {
                inc(&mut m.diagnostics_events);
                if *errors > 0 {
                    error_diagnostic_since_edit = true;
                    if !edit_window_counted {
                        inc(&mut m.edits_with_error_diagnostic);
                        edit_window_counted = true;
                    }
                    if let Some(seen) = awaiting_fix.as_mut() {
                        *seen = true;
                    }
                }
            }
            Event::Usage { message_id, tokens } => {
                let t = (tokens.input, tokens.cached, tokens.output);
                match message_id {
                    Some(mid) => {
                        usage_by_message.insert(mid.as_str(), t);
                    }
                    None => {
                        usage_unkeyed = (
                            usage_unkeyed.0.saturating_add(t.0),
                            usage_unkeyed.1.saturating_add(t.1),
                            usage_unkeyed.2.saturating_add(t.2),
                        );
                    }
                }
            }
        }
    }
    if awaiting_next_action > 0 {
        let none = m
            .next_action_after_empty_references
            .entry("none")
            .or_insert(0);
        *none = none.saturating_add(awaiting_next_action);
    }
    let (i, c, o) = usage_by_message
        .values()
        .fold(usage_unkeyed, |(i, c, o), (a, b, d)| {
            (
                i.saturating_add(*a),
                c.saturating_add(*b),
                o.saturating_add(*d),
            )
        });
    m.tokens_input = i;
    m.tokens_cached = c;
    m.tokens_output = o;
    m
}

fn inc(n: &mut u64) {
    *n = n.saturating_add(1);
}

fn elapsed(start: Option<i64>, at: Option<i64>) -> Option<u64> {
    u64::try_from(at?.checked_sub(start?)?).ok()
}

#[cfg(test)]
mod tests {
    //! One test per metric, over synthetic session JSONL in Claude Code's documented shape.

    use super::*;
    use crate::transcript::Source;
    use std::io::Cursor;

    /// A tiny builder for synthetic transcript lines. Timestamps advance one second per line.
    struct T {
        lines: Vec<String>,
        second: u32,
    }

    impl T {
        fn new() -> Self {
            Self {
                lines: vec![],
                second: 0,
            }
        }
        fn ts(&mut self) -> String {
            self.second = self.second.saturating_add(1);
            format!("2026-11-17T09:00:{:02}.000Z", self.second)
        }
        fn prompt(mut self) -> Self {
            let ts = self.ts();
            self.lines.push(format!(
                r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":"do the task"}}}}"#
            ));
            self
        }
        fn call(mut self, id: &str, name: &str, input: &str) -> Self {
            let ts = self.ts();
            self.lines.push(format!(
                r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"msg-{id}","role":"assistant","content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{input}}}],"usage":{{"input_tokens":10,"cache_read_input_tokens":100,"cache_creation_input_tokens":5,"output_tokens":7}}}}}}"#
            ));
            self
        }
        fn result(mut self, id: &str, is_error: bool, text: &str) -> Self {
            let ts = self.ts();
            let text = serde_json::to_string(text).unwrap();
            self.lines.push(format!(
                r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","is_error":{is_error},"content":{text}}}]}}}}"#
            ));
            self
        }
        fn diagnostics(mut self, severity: &str) -> Self {
            let ts = self.ts();
            self.lines.push(format!(
                r#"{{"type":"attachment","timestamp":"{ts}","attachment":{{"type":"diagnostics","files":[{{"uri":"file:///x/A.java","diagnostics":[{{"severity":{severity},"message":"m"}}]}}]}}}}"#
            ));
            self
        }
        fn metrics(self) -> RunMetrics {
            let mut t = Transcript::new(Source::Session);
            t.read_jsonl(&mut Cursor::new(self.lines.join("\n")))
                .unwrap();
            t.finish();
            compute(&t)
        }
    }

    const EDIT: &str = r#"{"file_path":"/app/A.java","old_string":"a","new_string":"b"}"#;
    const BUILD: &str = r#"{"command":"./gradlew :core:compileJava"}"#;
    const FAIL: &str = "> Task :core:compileJava FAILED\nA.java:3: error: ';' expected";

    #[test]
    fn should_count_tool_calls_by_kind_when_the_agent_uses_several_tools() {
        // given
        let t = T::new()
            .prompt()
            .call("1", "Read", r#"{"file_path":"/app/A.java"}"#)
            .call("2", "Grep", r#"{"pattern":"x"}"#)
            .call("3", "Read", r#"{"file_path":"/app/B.java"}"#)
            .call("4", "mcp__srv__tool", "{}")
            .call("5", "Frobnicate", "{}");
        // when
        let m = t.metrics();
        // then
        assert_eq!(m.tool_calls, 5);
        assert_eq!(
            m.tool_calls_by_kind,
            BTreeMap::from([("grep", 1), ("mcp", 1), ("other", 1), ("read", 2)])
        );
    }

    #[test]
    fn should_sum_the_last_usage_of_each_message_when_streaming_repeats_it() {
        // given: message msg-1 twice (streamed), msg-2 once
        let t = T::new()
            .call("1", "Read", "{}")
            .call("1", "Read", "{}")
            .call("2", "Read", "{}");
        // when
        let m = t.metrics();
        // then: two messages of 10 + 100 + 5 in, 100 cached, 7 out
        assert_eq!(
            (m.tokens_input, m.tokens_cached, m.tokens_output),
            (230, 200, 14)
        );
    }

    #[test]
    fn should_count_an_edit_then_compile_failure_when_the_next_build_fails_to_compile() {
        // given
        let t = T::new()
            .call("e1", "Edit", EDIT)
            .result("e1", false, "ok")
            .call("b1", "Bash", BUILD)
            .result("b1", true, FAIL)
            .call("e2", "Edit", EDIT)
            .result("e2", false, "ok")
            .call("b2", "Bash", BUILD)
            .result("b2", false, "BUILD SUCCESSFUL");
        // when
        let m = t.metrics();
        // then
        assert_eq!((m.edits, m.builds, m.edits_then_compile_failure), (2, 2, 1));
        assert_eq!(m.compile_failures_with_diagnostic_before_fix, 0);
    }

    #[test]
    fn should_not_count_a_compile_failure_when_no_edit_preceded_the_build() {
        // given: a failing build before any edit, and a failing test run after one
        let t = T::new()
            .call("b1", "Bash", BUILD)
            .result("b1", true, FAIL)
            .call("e1", "Edit", EDIT)
            .call("b2", "Bash", r#"{"command":"./gradlew test"}"#)
            .result("b2", true, "3 tests completed, 1 failed");
        // when
        let m = t.metrics();
        // then
        assert_eq!(m.edits_then_compile_failure, 0);
    }

    #[test]
    fn should_record_a_diagnostic_before_the_fix_when_one_was_pushed_after_the_broken_edit() {
        // given
        let t = T::new()
            .call("e1", "Edit", EDIT)
            .result("e1", false, "ok")
            .diagnostics("1")
            .call("b1", "Bash", BUILD)
            .result("b1", true, FAIL)
            .call("e2", "Edit", EDIT);
        // when
        let m = t.metrics();
        // then
        assert_eq!(m.edits_then_compile_failure, 1);
        assert_eq!(m.compile_failures_with_diagnostic_before_fix, 1);
    }

    #[test]
    fn should_count_an_edit_caught_by_diagnostics_when_an_error_is_pushed_before_the_next_edit() {
        // given: an error after e1, a warning only after e2
        let t = T::new()
            .call("e1", "Edit", EDIT)
            .diagnostics("\"Error\"")
            .diagnostics("1")
            .call("e2", "Edit", EDIT)
            .diagnostics("2");
        // when
        let m = t.metrics();
        // then
        assert_eq!(m.edits_with_error_diagnostic, 1);
        assert_eq!(m.diagnostics_events, 3);
        assert!(m.diagnostics_observable);
    }

    #[test]
    fn should_build_the_lsp_histogram_when_the_agent_calls_lsp_operations() {
        // given
        let t = T::new()
            .call(
                "l1",
                "LSP",
                r#"{"operation":"goToDefinition","filePath":"/app/A.java","line":3,"character":5}"#,
            )
            .call("l2", "LSP", r#"{"operation":"findReferences"}"#)
            .call("l3", "LSP", r#"{"operation":"goToDefinition"}"#)
            .call("l4", "LSP", r#"{"operation":"a planted operation"}"#);
        // when
        let m = t.metrics();
        // then
        assert_eq!(m.lsp_calls, 4);
        assert_eq!(
            m.lsp_operations,
            BTreeMap::from([("findReferences", 1), ("goToDefinition", 2), ("other", 1)])
        );
    }

    #[test]
    fn should_record_the_next_action_when_find_references_answers_empty() {
        // given: one empty answer followed by a Grep, one at the end of the run
        let t = T::new()
            .call("l1", "LSP", r#"{"operation":"findReferences"}"#)
            .result("l1", false, "No references found")
            .call("g1", "Grep", r#"{"pattern":"total"}"#)
            .call("l2", "LSP", r#"{"operation":"findReferences"}"#)
            .result("l2", false, "")
            .call("l3", "LSP", r#"{"operation":"findReferences"}"#)
            .result("l3", false, "Found 2 references:\nA.java:3\nB.java:9");
        // when
        let m = t.metrics();
        // then
        assert_eq!(m.empty_references, 2);
        assert_eq!(
            m.next_action_after_empty_references,
            BTreeMap::from([("grep", 1), ("lsp", 1)])
        );
    }

    #[test]
    fn should_record_none_as_the_next_action_when_the_run_ends_after_an_empty_answer() {
        // given
        let t = T::new()
            .call("l1", "LSP", r#"{"operation":"findReferences"}"#)
            .result("l1", false, "[]");
        // when
        let m = t.metrics();
        // then
        assert_eq!(
            m.next_action_after_empty_references,
            BTreeMap::from([("none", 1)])
        );
    }

    #[test]
    fn should_time_the_first_successful_lsp_answer_when_an_earlier_one_failed() {
        // given: prompt at 1 s, an error at 3 s, the first answer at 5 s
        let t = T::new()
            .prompt()
            .call("l1", "LSP", r#"{"operation":"hover"}"#)
            .result("l1", true, "No LSP server available for file type: .java")
            .call("l2", "LSP", r#"{"operation":"hover"}"#)
            .result("l2", false, "long total()");
        // when
        let m = t.metrics();
        // then
        assert_eq!(m.lsp_errors, 1);
        assert_eq!(m.time_to_first_lsp_answer_ms, Some(4000));
    }

    #[test]
    fn should_leave_the_lsp_time_unset_when_no_lsp_call_succeeded() {
        // given
        let t = T::new()
            .prompt()
            .call("l1", "LSP", r#"{"operation":"hover"}"#)
            .result("l1", true, "error");
        // when
        let m = t.metrics();
        // then
        assert_eq!(m.time_to_first_lsp_answer_ms, None);
    }
}
