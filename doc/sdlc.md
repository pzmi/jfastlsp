# Software development life cycle

How a change to jfastlsp goes from a task to green CI on `main`. The rules come from
[`foundation.md`](foundation.md): fnd §11 (the agent contract), fnd §9 (commands) and fnd §10
(commits and integration); the integration decision is [ADR 0002](adr/0002-integrate-on-main-one-signed-off-commit-per-task.md)
and the commit identity [ADR 0003](adr/0003-commit-identity-subject-and-trailer-order.md). The
checks CI requires are in [`required-checks.md`](required-checks.md). Outside contributors start
at [`../CONTRIBUTING.md`](../CONTRIBUTING.md).

## 1. Definition of Ready

A task in [`tasks.md`](tasks.md) is ready to start when:

- every task it depends on has a commit on `main` (subjects are `<ID>: <summary>`, so
  `git log --oneline main | grep " <dep>:"` finds it), a `DEC-` dependency has its ADR in the
  tree, and the task has no commit of its own;
- its card states what is built, the sections to read (`Cites:`), the acceptance items and
  which of them are `(owner)`;
- the observable behaviour, the invariants, the crates it touches, the threat model (when
  untrusted input, processes or files change), the test levels, the model impact, the protocol
  impact and any compatibility effect are stated or follow from the cited sections; a large task
  has an ADR or a design note first;
- a new dependency, edge or authority has its architecture policy and ADR planned before code.

A card whose text contradicts a decision in the Decisions table is not ready until the
contradiction is fixed; the Decision wins.

## 2. The agent procedure

Work is run by the `/orchestrate` skill ([`.claude/skills/orchestrate/SKILL.md`](../.claude/skills/orchestrate/SKILL.md))
and the `/work` skill ([`.claude/skills/work/SKILL.md`](../.claude/skills/work/SKILL.md)). They
are the real procedure; this section summarises it.

1. **Orchestrate.** The orchestrator never implements. It picks the ready tasks, creates one git
   worktree per lane under the session scratchpad from a fetched `origin/main`, and starts one
   background agent per task, at most two at a time. Tasks that change the same shared file
   (`Cargo.toml`, `Cargo.lock`, `architecture.toml`, `tools/xtask/`, `doc/tasks.md`, the model
   registry, protocol goldens) run one after another.
2. **Work.** The agent runs the stations `intake`, `plan`, `implement`, `test`, `polish`,
   `review`, `qa`, `ship`, writing its state to `.work/<ID>/` (ignored by git). It updates the
   architecture policy or an ADR first when a new dependency or authority is needed; the Quint
   model first for a change to views, the daemon lifecycle or diagnostics publication; the golden
   first for a protocol, `--json` or LSP change; writes failing tests before the smallest
   implementation; runs `cargo xtask quick` while working and the full gate before the commit;
   and reports exactly which gates ran.
3. **Commit.** Exactly one commit on its lane branch: subject `<ID>: <summary>`, a body that says
   why, an `Impact:` line, then the trailer block (`Signed-off-by`, `Co-Authored-By`,
   `Claude-Session`, in that order, written by hand). Only its own files are staged. The agent
   does not push, tag or touch `main`.
4. **Review.** The orchestrator runs a read-only `/review` over every task commit before landing
   it; HIGH and MEDIUM findings go back to the task's agent.
5. **Land.** `cargo xtask land <sha> --push origin main`: one commit at a time, the next only
   after the previous commit's CI is green; `check` on the merged tree; push only on green.
   **Until FND-03 lands `land`**, the orchestrator does it by hand: `git fetch`, a clean `main`
   equal to `origin/main`, `git cherry-pick <sha>`, the live gate on the merged tree in the
   foreground (the fallback until FND-02), `git push origin main` only on green.
6. **Watch CI.** The orchestrator waits for the run of the pushed commit and reads the
   `Required` result. Without `gh` it uses the GitHub MCP tools.

Agents never suppress a failing test, widen a lint exemption, remove a modelled invariant,
loosen a budget or rewrite a golden fixture to obtain green. A missing pinned tool is an
actionable failure, never a skipped check.

## 3. Definition of Done

A task is done when:

- code, tests and Quint models agree, and the affected documentation and ADRs are updated in the
  same commit;
- the live gate exits 0 on the merged tree (`quick` is never reported as `check`);
- the architecture rules and their negative fixtures pass;
- the commit is signed off, has the task's subject, and states its impact;
- it is on `main` and `Required` is green for that commit (once FND-07 has landed);
- the report names the gates that ran, their versions and seeds, the `(owner)` items left
  pending, and anything that did not run.

A manual exception needs the owner's explicit approval and a follow-up task, and is never
reported as green. A ⛳ task is done only after the checkpoint review below.

## 4. The gate

| Stage of the repository | Gate |
|:--|:--|
| Before FND-02 | the fallback: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` |
| From FND-02 | `cargo xtask quick` while working; `cargo xtask check` before every commit and in `land` |

The stages of `check` are listed in fnd §9 and printed by `cargo xtask list-checks`; each CI job
runs `cargo xtask check --only <stages>` ([`required-checks.md`](required-checks.md)).

## 5. Integration procedure

- **Direct on `main`** (ADR 0002). No pull-request flow and no branch protection for the plan;
  a pull-request flow may follow only through a new ADR.
- **One signed-off commit per task**, subject `<ID>: <summary>` (ADR 0003).
- **Only the orchestrator pushes.** Implementing agents never push, tag or change remote
  settings. Checkpoint tags (`checkpoint-<ID>`) are annotated, local and never pushed.
- **No force-push.** A rejected push is fetched and landed again. History on `main` is never
  rewritten.
- **Red CI on `main` is fixed immediately, on `main`,** by a new commit carrying the task id
  (`<ID>: fix <what>`). A test is never skipped, ignored or retried to green; a job is re-run at
  most once, and only when it died before any test ran or the failure is shown not to be the
  commit's. After three red attempts the work stops and the owner hears the failing job and step.
- **Outside contributions** arrive as pull requests, which run the same CI; the maintainer
  integrates an accepted one as a signed-off commit on `main`.

## 6. Checkpoints and owner decisions

- ⛳ tasks (listed in `tasks.md`) stop their lane once landed and green. The orchestrator runs
  `/review` over the range since the newest `checkpoint-*` tag, then an adversarial second review
  aimed at the checkpoint's named risks, has HIGH and MEDIUM findings fixed as a follow-up commit,
  tags `checkpoint-<ID>` locally, reports to the owner and waits for a go.
- Owner decisions are batched into each checkpoint report: Proposed ADRs to accept or override,
  `(owner)` acceptance items pending, `DEC-` tasks ready for a decision.
- **`DEC-` tasks** record an owner decision as an ADR. An agent may prepare the inputs (a
  criteria sheet, a memo with the numbers, a draft ADR marked Proposed); only the owner decides,
  and the ADR's status says so.
- **`(owner)` items** are steps only the owner can take: choosing the target repository, running
  Claude Code evaluation sessions, running a harness against a private repository, publishing a
  release, setting a repository setting. Agents deliver everything up to them (harness, runbook,
  synthetic dry run) and report them pending.

## 7. Phase 0 with a private target repository

If the target Java repository chosen at DEC-01 is private, the public-repository rule
(ADR 0004) decides how phase 0 runs: the harnesses (`jfl-eval`, `jfl-oracle`, the spikes) and
runbooks are committed here; the owner runs them on a machine with the repository; only the
aggregated reports (numbers per arm, per construct, per module count; no paths, class names or
task text) are committed under `doc/eval/`. The same holds for the owner-run milestone gates.
