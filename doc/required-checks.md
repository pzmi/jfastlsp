# Required checks

> **Status: planned.** Nothing here runs yet: `.github/workflows/ci.yml` does not exist until
> FND-05 lands. Until then CI is absent and the orchestrator runs the live gate locally on the
> merged tree before every push ([`sdlc.md`](sdlc.md) §2). This page lists the jobs FND-05
> creates; FND-05 rewrites it from the real workflow, and the architecture test's lockstep rule
> (`ARCH-ci-lockstep`) keeps the two in step.

The CI result that must be green on `main` will be the `required` job of
`.github/workflows/ci.yml`. It is an aggregate: it needs every other job of the workflow and fails
when any of them failed, was cancelled or was skipped, so a job cannot drop out of the gate by not
running (fnd §7.2). Each job runs `mise install --locked`, then `just check-only <stage>…`, so the
same recipes run on a laptop with `just check`.

## Branch protection

Integration is direct on `main` (ADR 0002), so "required" means "must be green, and red is fixed
immediately on `main`", not "GitHub refuses the merge". No task creates a ruleset. **(owner,
pending)** If the owner wants a branch ruleset on `main`, it requires the single check
`Required` and nothing else: requiring the aggregate means a job added later is required without
editing the ruleset.

## The planned checks

| Job id | Display name | Runs (`just ...`) | Arrives with |
|:--|:--|:--|:--|
| `architecture` | Architecture and policy | `check-only architecture deny` | FND-05 |
| `static` | Static quality | `doctor`, then `check-only fmt-check clippy nostd docs` | FND-05 |
| `tests` | Tests (Linux x86-64) | `check-only unit-tests integration-tests doctests script-tests` | FND-05 |
| `linux-arm` | Linux (arm64) | `check-only platform-tests` on `ubuntu-24.04-arm` | FND-05 |
| `macos` | macOS (arm64) | `check-only platform-tests` on `macos-15` | FND-05 |
| `security` | Security | `check-only secrets workflows private-names` | FND-05 |
| `dco` | DCO | every pushed commit signed off by its author | FND-05 |
| `coverage` | Coverage | `check-only coverage changed-lines`, then `just mutants-diff` (full history; diff base `git merge-base HEAD origin/main`, 20-minute cap) | FND-06 |
| `formal` | Formal models (Quint) | `check-only formal-typecheck formal-test formal-freshness conformance-replay` | FORMAL-01, FORMAL-02 |
| `fixtures` | Java fixtures | `check-only java-fixtures` (pinned JDK) | EVAL-07 |
| `e2e` | End to end | `check-only e2e-tests lsp-conformance` | IDX-13, LSP-01 |
| `required` | Required | the aggregate; needs all of the above | FND-05 |

Every stage of `check` will be in exactly one job; a test fails when a stage is in none or in two,
and when a gating step has `continue-on-error` or an `if:`. Runs on `main` are never cancelled or
replaced by a newer push: each commit there has its own concurrency group.

## Other workflows

Not in `required`: `security.yml` (weekly: new advisories can fail an unchanged tree; zizmor's
online audits), `bench.yml` (the corpus suites on one fixed runner class, history upload, and its
`bench-gate` job, which blocks `main` from IDX-15: red there is fixed like red CI; it is kept out
of `ci.yml` because wall times are comparable only on one runner class, fnd §9),
`codeql.yml`, `scorecard.yml`, `formal.yml` (Apalache, weekly and by hand), `fuzz.yml` (nightly),
`mutation.yml` (weekly), `release.yml` (tags). A red run of any of them is reported to the owner
and fixed as a task (fnd §7.1).

## When a check goes red

Read the failed job's log, name the failing line, and fix it on `main`. Locally,
`just check-only <stage>` reruns one stage through the same recipe, and `just doctor` comes first
when local and CI disagree.

- **Re-run at most once**, and only when the job died before any test body ran or to prove the
  failure is not the commit's (red on the previous commit too). Before the re-run, name the job,
  what failed and why it is not this change. A second failure is real.
- **A failure that passes on a rerun is a race to find**, not a flake (ver §1).
- **Before cancelling a run, read every job's state.**
- **Every job failing within seconds with no log** is an Actions quota or spending limit: the
  owner is told once; nothing more lands until it is resolved.
