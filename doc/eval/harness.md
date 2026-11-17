# jfl-eval: the E0 harness around Harbor

`tools/jfl-eval` (EVAL-01, ADR 0029) runs the agent evaluation of plan §9.2 (E0) and ver §11 on
[Harbor](https://github.com/harbor-framework/harbor), the Terminal-Bench 2 harness. Harbor runs
each task in Docker with its built-in Claude Code agent; `jfl-eval` writes the tasks, defines the
arms, and turns the trials Harbor leaves into per-run metrics and an aggregate report of **ids and
numbers only**. It is a tool crate and imports no production crate (fnd §2.7).

Harbor, Docker and Claude Code run only on the owner's machine: Claude Code sessions cost money,
and a private target's tasks stay outside this repository (ADR 0004). Nothing here runs in the
gate except the tests, which use synthetic fixtures.

## The Harbor version matched

Harbor **0.26.0** (PyPI, 2026-11-13; tag `v0.26.0` at commit
`3cb627394b95d3ae605f31e0d694edf508cf4b7b`; sdist sha256
`ec75294b7e2a9f9c9419ff9e18033f4e587a8aa312a79e93a5958cf17ef746f8`). Its formats change between
releases, so `jfl-eval` names the version it was written against (`harbor::HARBOR_VERSION`), and
`jfl-eval --help` prints it. Install that version (`uv tool install harbor==0.26.0`) or re-check
these against the newer one:

| What | Harbor 0.26.0 source | `jfl-eval` |
|:--|:--|:--|
| Task directory: `task.toml`, `instruction.md`, `environment/Dockerfile`, `tests/test.sh`, `solution/solve.sh` | `models/task/paths.py` | `harbor.rs` writes it |
| `task.toml` `schema_version = "1.4"`, `[metadata]`, `[agent]`, `[verifier]`, `[environment]` (`workdir`, `build_timeout_sec`) | `models/task/config.py` | `harbor.rs` |
| Reward: `tests/test.sh` writes `/logs/verifier/reward.txt`; the trial records `verifier_result.rewards.reward` | `verifier/verifier.py`, `models/verifier/result.py` | `run.rs` reads it |
| Trial directory: `result.json` (`TrialResult`, `schema_version` 1) and `agent/` | `models/trial/paths.py`, `models/trial/result.py` | `run.rs` |
| Claude Code: `CLAUDE_CONFIG_DIR=/logs/agent/sessions`, so the transcript lands in `agent/sessions/projects/<cwd>/<session>.jsonl` (and `<session>/subagents/*.jsonl`); `claude --verbose --output-format=stream-json --print`, teed to `agent/claude-code.txt`; `--ak config=<settings.json>` is passed as `claude --settings` | `agents/installed/claude_code.py` | `transcript.rs`, `run.rs` |
| ATIF trajectory `agent/trajectory.json` (ATIF-v1.0 to v1.8) | `models/trajectories/*.py` | `transcript.rs` (fallback) |

## Formats

**A task set** (TOML; `taskset.rs` documents every field): `schema = 1`, optional
`agent_timeout_sec` and `verifier_timeout_sec`, and `[[task]]` tables with `id`, `commit` (the
starting commit, 40 hex digits), `prompt` (verbatim), `test_command` (run from the work directory;
exit 0 is success), `known_good` (a commit whose tree passes the test command) and `category`
(`bug-fix`, `signature-change`, `cross-module-feature`, `rename`, `other`). Unknown keys are
refused. EVAL-02 writes the target's set (kept owner-side if the target is private); EVAL-12 the
public Multi-SWE-bench control set.

**An arm** (TOML; `arm.rs`): `schema = 1`, `id`, `image` (the Docker image: the target repository
at `workdir` with a pre-warmed Gradle cache and the language server installed; pin it by digest),
`workdir` (default `/app`), `setup` (Dockerfile `RUN` lines), `plugins` (Claude Code
`enabledPlugins` entries), `[marketplaces]` (`extraKnownMarketplaces`), `[settings]` (any other
Claude Code settings) and `[env]` (the settings' `env` block). Everything the agent sees goes into
one `claude-settings.json` per arm. EVAL-03 writes the E0 arms and their images.

**Ids.** Arm and task ids are lowercase ASCII words joined by single hyphens, at most 64
characters. They are the only strings from the input a report shows: that shape cannot hold a
prompt, a path or a line of Java.

## Commands

```bash
# Harbor tasks: out/<arm>/tasks/<arm>__<task>/ for every task and arm, and out/<arm>/claude-settings.json.
cargo run --locked -p jfl-eval -- harbor --tasks tasks.toml --arm grep-only.toml --arm jdtls.toml --out out
# The smoke task for one arm: out/<arm>/tasks/<arm>__smoke-lsp/.
cargo run --locked -p jfl-eval -- smoke --arm jdtls.toml --out smoke
# The report over every trial under a jobs directory (Markdown, or --json).
cargo run --locked -p jfl-eval -- report jobs/ [--json]
# One trial's metrics and the transcript's schema names, as JSON.
cargo run --locked -p jfl-eval -- inspect jobs/<job>/<trial>
```

Each task's `environment/Dockerfile` checks out the starting commit and then strips every way
back to a later one: the arm image holds the history of all tasks, so its branches, tags and
reflogs would let the agent read the fix with `git log --all` or `git show <known_good>`. Every
ref is deleted, `ORIG_HEAD` and `FETCH_HEAD` removed, the reflogs expired and `git gc
--prune=now` run; the image build then fails if the known-good commit is still in the object
store (`tests/harbor.rs::should_strip_history_past_the_starting_commit_when_writing_the_dockerfile`).
So Harbor's oracle agent cannot check that commit out either: `solution/solve.sh` applies
`solution/known-good.patch`, which the owner writes once per task from the target checkout
(`git diff --binary <commit> <known_good> > <task>/solution/known-good.patch`; the command is in
each `solve.sh`). Run the oracle (`harbor run -p <tasks> -a oracle`) before E0 to check that
every task's test command passes on its known-good tree.

`harbor` and `smoke` write into an empty or absent directory only, and print each arm's command:

```bash
harbor run -p out/<arm>/tasks -a claude-code -m "$MODEL" -k 3 \
  --ak config=out/<arm>/claude-settings.json --ak version="$CLAUDE_CODE_VERSION" -o "$JOBS"
```

`-k 3` is the three runs per task per arm of plan §9.2. Record the Claude Code version and the
`jfl` version with the report (ver §11.3). A generated task is named `<arm>__<task>`, which Harbor
records as each trial's `task_name`; that is how `report` gets the arm and task back, and a trial
of any other task is counted as skipped, never read.

## What the owner runs

1. **The smoke task first** (ADR 0029): `jfl-eval smoke --arm <the jdtls arm> --out smoke`, then the
   printed `harbor run` with `-k 1`, then `jfl-eval inspect <the trial>`. The task asks for one
   `goToDefinition` on a two-file synthetic project; the verifier rewards 1 only when the
   transcript holds an `LSP` call and the answer names the definition. Read `inspect`:
   - `metrics.lsp_calls > 0` and `time_to_first_lsp_answer_ms` set: language servers start under
     `claude -p` in a Harbor container; E0 runs on Harbor.
   - `lsp_errors > 0` and no answer: Claude Code could not start the server (it answers an LSP
     call with an error then). Check the image (the server binary on `PATH`, the plugin enabled in
     `claude-settings.json`) once; if it still fails, E0 uses the thin `claude -p
     --output-format stream-json` runner of ADR 0029 instead, which `report` already reads
     (`agent/claude-code.txt`).
   - `shapes` lists the transcript's line types, attachment types, tool names (an MCP tool as
     `mcp__*`), the `LSP` tool's input keys and its operations (the known ones or `other`): check
     them against the assumptions below and record what was found in the report of EVAL-04. This also settles plan §9.2's open question for `claude -p`.
2. **E0** (EVAL-04): `jfl-eval harbor` over the task set and the control set with every arm, each
   arm's `harbor run`, then `jfl-eval report <jobs>`; the Markdown report (or `--json`) is the
   only thing that enters this repository.

## Metrics

Per run (`metrics.rs`), aggregated per arm and per arm and task with n, mean, sample standard
deviation, min and max (`report.rs`):

| Metric | Read from | Test |
|:--|:--|:--|
| success | the verifier's reward (1 = the test command passed); no reward is "no verdict" | `tests/report.rs` (golden) |
| tool calls by kind | every `tool_use` (`read`, `edit`, `bash`, `grep`, `glob`, `lsp`, `agent`, `todo`, `web`, `skill`, `mcp`, `other`) | `metrics::tests::should_count_tool_calls_by_kind_when_the_agent_uses_several_tools` |
| tokens | each model message's last usage (input includes cache reads and writes, as Harbor counts) | `metrics::tests::should_sum_the_last_usage_of_each_message_when_streaming_repeats_it` |
| edits then a compile failure | a Bash Java build (`gradle`, `gradlew`, `mvn`, `mvnw`, `javac`) whose result is an error that reads like a compilation failure, after an edit | `metrics::tests::should_count_an_edit_then_compile_failure_when_the_next_build_fails_to_compile` |
| … with a diagnostic before the fix | an error diagnostic pushed between the broken edit and the next edit | `metrics::tests::should_record_a_diagnostic_before_the_fix_when_one_was_pushed_after_the_broken_edit` |
| edits caught by diagnostics | an edit followed by an error diagnostic before the next edit (plan §1.2, metric 3) | `metrics::tests::should_count_an_edit_caught_by_diagnostics_when_an_error_is_pushed_before_the_next_edit` |
| LSP operation histogram | every `LSP` call by `input.operation` (unknown values are `other`) | `metrics::tests::should_build_the_lsp_histogram_when_the_agent_calls_lsp_operations` |
| empty references and the next action | a `findReferences` answer that is empty, `[]` or "No references found", and the kind of the next tool call (`none` at the end) | `metrics::tests::should_record_the_next_action_when_find_references_answers_empty` |
| time to first LSP answer | from the transcript's first timestamp to the first `LSP` result that is not an error | `metrics::tests::should_time_the_first_successful_lsp_answer_when_an_earlier_one_failed` |

Diagnostics metrics are computed only over runs whose transcript can show them (Harbor's ATIF
drops attachment lines), and `n` says over how many; a metric no run had (`n = 0`, for example the
time to an LSP answer in an arm without LSP) shows `-` in Markdown and `null` in JSON, never 0.

The success rate counts a run without a verdict (the trial errored before the verifier: an
infrastructure fault or an environment timeout) as a failure; `success_rate_with_verdict` (in
Markdown, "over the runs with one") leaves those runs out, and `no_verdict` counts them.

**Assumptions the smoke task checks**, since Claude Code's transcript format is not a published
schema: the LSP tool is named `LSP` and names its operation in `input.operation`; pushed
diagnostics arrive as an `attachment` line whose `attachment.type` contains `diagnostic`, with LSP
severities (`1` or `"Error"`) on its entries; a failing Bash command's `tool_result` has
`is_error: true`. If a real transcript differs, `transcript.rs` is the one place to change, with a
synthetic fixture of the new shape.

## What a report never holds

No prompt, path, source line, diagnostic message, model name or error text: a report's strings
are arm and task ids and fixed metric names, and the types say so (every string in
`report::Report` is an `ids::Id` or a `&'static str`). `inspect` adds identifier-shaped schema
names: line and attachment types, built-in tool names, MCP tools folded to `mcp__*` (a server's
name is configuration), the `LSP` tool's input keys, and its operation only when it is a known
one (anything else is `other`). Pinned by
`tests/report.rs::should_leave_out_every_planted_string_when_the_input_carries_them`, whose
fixtures plant a prompt, paths, a source line, a diagnostic, a model name and an exception.

## Untrusted input

Transcripts and results come from an agent's run. Reads are bounded (`bounded.rs`: 32 MiB a line,
512 MiB a transcript file, 256 MiB a JSON document, 4 MiB a TOML file); a line that is not a JSON
object (garbage, or the truncated last line of a killed run) is skipped and counted under data
quality, as are over-long lines, cut files and lines repeated by `uuid`. Nothing is read through a
symlink: the walk types entries without following links, and `result.json`, `agent/`,
`agent/sessions`, `agent/sessions/projects` and the transcript files are each checked with
`symlink_metadata`, since the agent writes `agent/` from inside its container
(`run::tests::should_read_no_transcript_when_a_directory_on_the_way_is_a_symlink`). Nothing in
the input makes it panic (`transcript::tests`).
