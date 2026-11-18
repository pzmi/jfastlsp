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
  `git log --oneline main | grep " <dep>:"` finds it), a `DEC-` dependency counts when it is
  **recorded as decided by the owner**: its ADR's status reads `Accepted (owner, …)`, or, where
  no ADR was needed (ADR 0001, amended), its card is ticked and marked `Decided:` with the date
  (an agent's draft ADR, committed as `Proposed` under the `DEC-` subject, does not count:
  `grep -A2 '^## Status' doc/adr/NNNN-*.md`), and the task has no commit of its own;
- its card states what is built, the sections to read (`Cites:`), the acceptance items and
  which of them are `(owner)`;
- the observable behaviour, the invariants, the crates it touches, the threat model (when
  untrusted input, processes or files change), the test levels, the model impact, the protocol
  impact and any compatibility effect are stated or follow from the cited sections; a large task
  has an ADR or a design note first;
- a new dependency, edge or authority has its architecture policy planned before code, and an
  ADR only if it is an important decision whose context is worth recalling (ADR 0001, amended).

A card whose text contradicts a decision in the Decisions table is not ready until the
contradiction is fixed; the Decision wins.

## 2. The agent procedure

Work is run by the `/orchestrate` skill ([`.claude/skills/orchestrate/SKILL.md`](../.claude/skills/orchestrate/SKILL.md))
and the `/work` skill ([`.claude/skills/work/SKILL.md`](../.claude/skills/work/SKILL.md)). They
are the real procedure; this section summarises it.

1. **Orchestrate.** The orchestrator never implements. It picks the ready tasks, creates one git
   worktree per lane under the session scratchpad from a fetched `origin/main`, and starts one
   background agent per task, at most two at a time. Tasks that change the same shared file
   (`Cargo.toml`, `Cargo.lock`, `architecture.toml`, `deny.toml`, the `clippy.toml` lists, the
   `justfile`, `mise.toml` and `mise.lock`, `tools/jfl-architecture/`, `doc/tasks.md`, the model
   registry, protocol goldens, `doc/bench-corpus.md`) run one after another.
2. **Work.** The agent runs the stations `intake`, `plan`, `implement`, `test`, `polish`,
   `review`, `qa`, `ship`, writing its state to `.work/<ID>/` (ignored by git). It updates the
   architecture policy or an ADR first when a new dependency or authority is needed; the Quint
   model first for a change to views, the daemon lifecycle or diagnostics publication; the golden
   first for a protocol, `--json` or LSP change; writes failing tests before the smallest
   implementation; runs `just quick` while working and the full gate (`just check`) before the
   commit; runs a product task's benchmarks on the corpus (small at least) and reports the metrics
   it changed (ADR 0031); and reports exactly which gates ran.
3. **Commit.** Exactly one commit on its lane branch: subject `<ID>: <summary>`, a body that says
   why, an `Impact:` line, then the trailer block (`Signed-off-by`, `Co-Authored-By`,
   `Claude-Session`, in that order, written by hand). Only its own files are staged. The agent
   does not push, tag or touch `main`.
4. **Review.** The orchestrator runs a read-only `/review` over every task commit before landing
   it; HIGH and MEDIUM findings go back to the task's agent. One review round per commit: LOWs go
   onto the cards they belong to and never block; a focused re-verify of only the blocking
   findings, by the same reviewer, is allowed for a security-relevant crate, a parser or a formal
   model (standing rule 1).
5. **Land.** `scripts/land.sh <sha>`: one commit at a time, the next only after the previous
   commit's CI is green; `just check` on the merged tree; push only on green (ADR 0002, ADR 0030).
   `land.sh` refuses a dirty checkout, resolves only `Cargo.lock` and the ADR index, checks each
   commit's sign-off and signature, and takes `--no-push` for a dry run; any other conflict aborts
   and names its files. Before anything in the merged tree runs, the landing checkout's own xtask
   (`scripts/xtask.sh tool-config`) checks its tool configuration (FND-09, XT-01); a commit that widens
   that check's allowlists is refused by `main`'s `land.sh`, and no agent can push by hand. After
   review, run `scripts/land.sh <sha>` from the lane worktree that holds the reviewed commit: its
   own, reviewed tool-config check is then the trusted one, while the cherry-pick onto a clean
   `origin/main`, the gate on the merged tree and push only on green stay `land.sh`'s. The land
   target dir (`$XDG_CACHE_HOME/jfastlsp/land-xtask`, else `~/.cache/jfastlsp/land-xtask`) is a
   cache that grows with every land: below about 10 GB free, delete it before landing.
6. **Watch CI.** The orchestrator waits for the run of the pushed commit and reads the
   `Required` result. Without `gh` it uses the GitHub MCP tools. The `linux-arm` and `macos`
   jobs run only after the push (ADR 0002), so a platform failure is found there.

Agents never suppress a failing test, widen a lint exemption, remove a modelled invariant,
loosen a budget or rewrite a golden fixture to obtain green. A missing pinned tool is an
actionable failure, never a skipped check.

## 3. Definition of Done

A task is done when:

- code, tests and Quint models agree, and the affected documentation and ADRs are updated in the
  same commit;
- the live gate exits 0 on the merged tree (`quick` is never reported as `check`), and the
  author's own `just check` exited 0 with the commit in place (after `git commit` or `--amend`),
  since some stages (secrets) scan committed objects; the exit code is the gate's own, never a
  wrapper's `echo` or a pipeline's last command;
- the architecture rules and their negative fixtures pass;
- the commit is signed off, has the task's subject, and states its impact;
- it is on `main` and `Required` is green for that commit;
- the report names the gates that ran, their versions and seeds, the `(owner)` items left
  pending, and anything that did not run.

A manual exception needs the owner's explicit approval and a follow-up task, and is never
reported as green. A ⛳ task is done only after the checkpoint review below.

## 4. The gate

`just quick` while working; `just check` before every commit and in `scripts/land.sh`; `just
ci-parity` before landing a change to tests that start cargo, nextest or a scratch workspace.
The gate's own recipes, scripts and configuration are pinned (`.config/gate.sha256`,
`ARCH-gate-config`, fnd §3.6): a diff that moves a pin is a change of the gate and is reviewed
line by line as one. Every check of the gate is an allowlist or a pinned shape, never a list of
bad patterns (fnd §3).

Tools come from `bash scripts/bootstrap-tools.sh` (mise and just) and `mise install --locked`
(the rest). The stages of `check` are listed in fnd §9 and by `just --list`; each CI job runs
`just check-only <stage>…` ([`required-checks.md`](required-checks.md)). `bench-gate` is not a
stage of `check`: from M1 on `bench.yml`'s `bench-gate` job, on one fixed runner class, fails a
tracked metric that regresses by more than 10 % (plan §1.7, fnd §9).

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
  tags `checkpoint-<ID>` locally, reports to the owner and waits for a go. At every checkpoint
  the orchestrator also checks pinned git dependencies for a release (today: `cafebabe`, DEP-01)
  and dispatches the switch card when one has the fix.
- Owner decisions are batched into each checkpoint report: Proposed ADRs to accept or override,
  `(owner)` acceptance items pending, `DEC-` tasks ready for a decision. The owner decided every
  record at the FND checkpoint on 2026-11-17 (DEC-07): all are Accepted (owner) except the
  superseded 0005, 0011–0013, 0027 and 0030; a record written since is Proposed until the next
  checkpoint.
- **Skills stay repo-local** (owner, 2026-11-17, DEC-08): `/work`, `/orchestrate` and `/review`
  live in this repository's `.claude/skills/`; a shared plugin is deferred.
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
