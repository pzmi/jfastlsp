---
name: work
description: >
  This skill should be used when the user asks to "work on IDX-04", "pick up a task",
  "implement task LSP-02", "do the next task", or names a task id from doc/tasks.md
  (or a GitHub issue number). Runs the full station sequence: intake, plan,
  implement, test, polish, review, qa, ship, babysit. Ends with one DCO-signed-off
  commit, landed on main only by whoever owns the push.
---

# Work: task orchestrator

One task from `doc/tasks.md` (or one GitHub issue) to a green, signed-off commit. Thin routing
here; the detail lives in `stations/{name}.md`, read on entry to each station. An agent told to
follow this skill by reading it (rather than through the Skill tool) reads this file,
`processes.md`, `work-order.md` and each station file from `.claude/skills/work/`, and follows
them exactly as if the skill had been loaded. Subagents this skill dispatches get the same
instruction: "read `.claude/skills/work/stations/<station>.md` and follow it".

## The gate

Every station that says "the gate" means the command `doc/tasks.md` names as live:

`just quick` while iterating, `just check` before commit (tools from
`bash scripts/bootstrap-tools.sh`); `just ci-parity` too when the task changes tests that start
cargo, nextest or a scratch workspace. A change to a gate recipe, a gate script or the gate's
configuration moves its pin in `.config/gate.sha256` in the same commit (fnd §3.6), and every
new check is an allowlist or a pinned shape, never a list of bad patterns (fnd §3).

`quick` is not `check` (fnd §9). Never report `quick` as the full gate.

Run the gate in the foreground, with the lane's own `CARGO_TARGET_DIR` when the caller names one
(else one under the session scratchpad), `CARGO_BUILD_JOBS=2` and `CARGO_INCREMENTAL=0` (the
sandbox has 4 CPUs and at most two lanes), and delete that dir when the task ends. Local green but
CI red, or the reverse: `just doctor` first. Keep the gate's exit code: no `| tail` or
`| head` inside an `&&` chain without `set -o pipefail`.

## Tooling without `gh`

| | Agent without `gh` (sandbox) | With `gh` |
|:--|:--|:--|
| CI verdict | MCP `actions_list`, `actions_get` | `gh run list`, `gh run watch` |
| CI failure logs | MCP `get_job_logs` (`failed_only`) | `gh run view --log-failed` |
| Issue text | MCP `issue_read` | `gh issue view` |

Detect `gh` with `command -v gh`. An implementing agent handed a worktree and told "do not push"
stops after the commit and reports; landing, pushing and CI belong to the orchestrator (ADR 0002).
Agents never push, by any means: `.claude/settings.json` denies the common forms of `git push`
(ADR 0018, amended), and a form it does not catch is still forbidden. `scripts/land.sh` is the only
pusher.

## Constraints

Never:

- Ask "shall I proceed?" or confirm commits or checklists. A question is for a real ambiguity only, phrased so one short reply answers it
- Skip a station, or implement without a plan checklist
- Run a repeatable job as a bare ad-hoc command once a `just` recipe covers it (`just check-only architecture`, not a hand-rolled `cargo metadata | jq`); a missing repeatable job becomes a recipe
- Loosen `architecture.toml`, `[workspace.lints]`, `deny.toml`, a Quint invariant, a performance budget or a test to get green, unless the task says so; rewrite a golden fixture (a Java corpus file, an LSP transcript, a `--json` golden); add `#[allow]`/`#[expect]` without an `architecture.toml` exception citing an ADR, `#[ignore]`, or a sleep in a test
- Add a dependency other than with `cargo add` (name it and why in the commit message)
- Commit without polish, without a green gate on the exact tree, or without a sign-off
- End the turn with the gate, a build, a `jfl` daemon, a JVM worker or a Gradle run still running in the background, or waiting on one
- Kill a process by name pattern (`pkill -f`, `killall`): only PIDs you started
- Run Gradle, `javac` or any build of a target Java repository outside a scratchpad copy; delete anything you did not create in a scratchpad tempdir
- Force-push, bypass hooks (`--no-verify`), push tags, or merge a PR
- Edit `CLAUDE.md` or `.claude/` (report a needed change instead), or flip a Proposed decision to Accepted
- Review your own diff inline: the review station dispatches `/review` subagents
- Put a model id, secret, token, internal hostname, personal path, content of a private Java repository, or the name of a private project in any repo file (public repo, ADR 0004)

Always:

- Read `CLAUDE.md` first, and pass its rules to every subagent
- Read `stations/{name}.md` before entering that station
- Write `.work/<id>/<station>.json` after every station and update `.work/<id>/work-order.json` before the next (see `work-order.md`)
- Commit with the subject `<ID>: <summary>`, a body with why and an `Impact:` line, `--author "Patryk Żmigrodzki <pzmi@users.noreply.github.com>"`, and a trailer block last, in the order `Signed-off-by`, `Co-Authored-By`, `Claude-Session` (ADR 0003). `git commit -s` appends `Signed-off-by` after an existing trailer block, so write every line by hand and drop `-s`
- Use the `Co-Authored-By` and `Claude-Session` lines the caller's (or session's) attribution instructions give; add none of your own
- Stage only files you changed (never `git add -A` or `git add .`)
- Record decisions where they belong (ADR 0038): an ADR only for an important decision whose context is worth recalling later, with `adrs new "<title>"` (a number the caller reserved: copy `doc/adr/template.md` to `NNNN-title.md`; Nygard format: Status, Context, Decision, Consequences; add the row to `doc/adr/README.md` and run `just adr-index`); a detail in plan, foundation, verification or the card; the reason in the commit message and code comments (non-obvious constraints). ADRs are records, not rules. Never edit an Accepted ADR's text and never add an `## Amended` section: a change that reverses a recorded decision is a new ADR that supersedes it (the old one's Status line becomes `Superseded by NNNN`, its only edit), in the same commit
- Reuse before building (ADR 0019): before adding a subsystem, read the card's `Reuse considered:` line and plan §16; a hand-built subsystem with a maintained alternative says why in the card and the commit message
- Measure speed (ADR 0031): a product task runs its benchmarks on the bench corpus (small at least, `just bench`) and reports the metrics it changes; never loosen a budget in plan §1.7
- Stop at every checkpoint task (⛳ in `doc/tasks.md`: FND-05, FORMAL-05, DEC-02, IDX-15, LSP-06, SEM-07, REL-06) after the commit: report that a `/review` range is due, and do not start the next task
- Deliver everything up to an `(owner)` criterion and report it as pending

## Input

`$ARGUMENTS`: a task id, or a GitHub issue number or URL, plus optional
`--small`/`--medium`/`--large`. Absent: take the first task in `doc/tasks.md` whose dependencies
all have a commit (`git log --oneline | grep " <dep>:"`; a `DEC-` dependency only once it is
recorded as decided: its ADR reads `Accepted (owner, …)`, or its card is ticked and marked
`Decided:` with the date when no ADR was needed), which has none of its own yet and is neither an `(owner)` task nor
a `DEC-` task waiting on the owner.

## Startup

1. Read `processes.md` and `work-order.md`.
2. `.work/` must be ignored (`git check-ignore -q .work/x`); if it is not, add `/.work/` to `.gitignore` in this task's commit and say so. `mkdir -p .work/<id>`. If `.work/<id>/work-order.json` exists, resume after the last station in `stationOutputs`, after checking `git log` and `git status` so finished work is never redone.
3. Working tree must be clean of other people's changes: `git status --short`. Someone else's uncommitted edits are left untouched and unstaged; mention them in the report.

## Routing

```
run_inline(intake); run_inline(plan)
for station in process.sequence_after(plan):
    def = read(".claude/skills/work/stations/{station}.md")
    out = inline ? execute(def) : subagent(def, inputs_from(work_order))
    write(".work/<id>/{station}.json", out); work_order.stationOutputs[station] = true
    if not sensors(def.sensors, out): retry once with the failure attached, else stop and report
    next = gates(def.gates, out)   # first passing gate wins; none -> stop and report
    if next != following(station): jump(next); work_order.loopCount += 1
```

## Dispatch

Inline: intake, plan, qa, ship, babysit. Subagent (fresh context, one station, structured
output): implement, test, polish, review.

**When the Agent tool is not available** (a task agent started by `/orchestrate` usually cannot
spawn subagents): run implement, test and polish inline; for review, do a separate, deliberate
pass that re-reads the diff against `.claude/skills/review/SKILL.md` as if it were someone
else's, and write `review.json` with `"mode": "inline-no-agent"`. Say so in the report: the
orchestrator then runs an independent `/review` over the commit before landing it. Each
subagent's prompt tells it to read `CLAUDE.md` and its station file and to follow the station's
Prompt section. A subagent crash counts as a failed sensor. Subagents work in the same directory
(the worktree, if one was given) with the same `CARGO_TARGET_DIR`.

## Errors

| Scenario | Action |
|:--|:--|
| Sensor fails twice | Stop. Report station, sensor, output. |
| No gate passes | Stop. Report station, output, gates. |
| Gate red for a reason outside the diff | Stop. Report; do not "fix" other people's WIP. |
| A required tool is missing (just, nextest, deny, quint, Apalache, the pinned JDK) | Install it with `bash scripts/bootstrap-tools.sh` (`just doctor` says what is missing) or stop and report; never treat a skipped check as green. |
| The task needs the target Java repository, a Claude Code run or the owner's machine | Build and test everything against the synthetic fixtures; leave the owner-run step documented and runnable; report the `(owner)` part as pending. |
| CI red 3 times | Stop at babysit. Report the failing job and step. |
| Force-push needed | Stop at ship. Ask the owner. |
| Plan or task text is wrong or contradictory | Fix it minimally in `doc/plan.md` or `doc/tasks.md` in the same commit and say so (an ADR only if it is an important decision; otherwise the commit message says why). |

## Files

`processes.md`, `work-order.md`, and `stations/{intake,plan,implement,test,polish,review,qa,ship,babysit}.md`.

## Completion

Report in a few lines: what was built, the gate that ran and its result with the test count, the
commit sha, push and CI status (or "commit ready, not pushed"), any deviation from the plan,
`(owner)` criteria left pending, anything a later task must know, and `issues` from the work
order.
