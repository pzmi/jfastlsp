# Required checks

The CI result that must be green on `main` is the `Required` job (id `required`) of
[`.github/workflows/ci.yml`](../.github/workflows/ci.yml). It is an aggregate: it runs
`if: always()`, needs every other job of the workflow, and fails unless every one of them ended in
`success`, so a job cannot drop out of the gate by failing, being cancelled or being skipped
(fnd §7.2). Each job installs the pinned tools it runs (mise at the version
`scripts/bootstrap-tools.sh` pins, checksum-verified, then `mise install --locked` of the tools
mise-action's `install_args` names: the gate's tools, plus Node and Quint in the `formal` job,
which also runs `doctor` since it holds every tool `doctor` requires; the toolchain of
`rust-toolchain.toml`) and runs `just` recipes, so the same recipes run on a laptop with `just
check`. The JDK and Apalache are installed only by `formal.yml` (FORMAL-01). The `formal` job's
`conformance-replay` stage draws a fresh seed on every run and prints it with the command that
repeats the run (`just conformance <model> <seed> <traces> <steps>`), and builds the replay
(`jfl-conformance`) with the job's Rust toolchain (FORMAL-02).

`ci.yml` runs on every push to `main`, on pull requests (outside contributors and Dependabot) and
by hand (`workflow_dispatch`). Integration is direct on `main` (ADR 0002), so the run that matters
is the push of the landed commit.

## Branch protection

"Required" means "must be green, and red is fixed immediately on `main`", not "GitHub refuses the
push". No task creates a ruleset. **(owner, pending)** If the owner wants a branch ruleset on
`main`, it requires the single status check `Required` and nothing else: requiring the aggregate
means a job added later is required without editing the ruleset. **But** a ruleset that requires a
status check rejects a direct push of any commit that has no passing `Required` yet, which is every
commit `scripts/land.sh` pushes (ADR 0002). Landing then works only if the identity that pushes is
on the ruleset's bypass list, and for that identity the rule is advisory: it is what land.sh's own
gate on the merged tree already guarantees. Without the bypass, direct integration stops.

## The checks

The table is read by the architecture test (`ARCH-ci-lockstep`, `tools/jfl-architecture/src/ci.rs`):
each row is one job of `ci.yml`, with its id, display name and runner, and the `just` invocations
it runs in order, each in backticks (a recipe's arguments are not part of the contract). A job
without a row, a row without a job, or any difference fails the `architecture` stage.

| Job id | Display name | Runner | Runs (`just ...`) |
|:--|:--|:--|:--|
| `architecture` | Architecture and policy | `ubuntu-24.04` | `check-only architecture deny` |
| `static` | Static quality | `ubuntu-24.04` | `check-only fmt-check clippy nostd docs` |
| `tests` | Tests (Linux x86-64) | `ubuntu-24.04` | `check-only unit-tests integration-tests doctests script-tests lsp-conformance` |
| `linux-arm` | Linux (arm64) | `ubuntu-24.04-arm` | `platform-tests` |
| `formal` | Formal models (Quint) | `ubuntu-24.04` | `doctor`, then `check-only formal-typecheck formal-test formal-freshness conformance-replay` |
| `security` | Security | `ubuntu-24.04` | `check-only secrets workflows`, then `workflows-verify` |
| `dco` | DCO | `ubuntu-24.04` | `dco-event` over the commits the event brought (a push's before..after; a pull request's base..head; the commit itself on a manual run; a push from the all-zero commit or a force-push fails) |
| `required` | Required | `ubuntu-24.04` | the aggregate of every other job's result |

Every stage of `just check` is in exactly one job. `platform-tests` is not a stage of `check`: it
runs the unit, integration, doctest and LSP conformance stages natively on Linux arm64 here and on
macOS arm64 in `macos.yml` (ADR 0006, fnd §7.2 rule 4; see "Reported checks"), which the `tests`
job already runs on Linux x86-64. `workflows-verify` (pinact's
version-comment check) and `dco-event` need the GitHub API or the pushed range, so they are
recipes outside `check`; the lockstep test also requires `platform-tests` to depend on exactly
`unit-tests`, `integration-tests`, `doctests` and `lsp-conformance` (LSP-02). It checks shapes it allows rather than bypasses
it knows: every `run:` of a job is `rustup toolchain install` or exactly one line `just <recipe> [args]`, the first `just` of a job
after its `rustup toolchain install` (the justfile builds xtask with the pinned toolchain while it
parses, so every recipe needs it; FND-13), each argument a plain
word or `"$VAR"` of the step's own `env:` (so no `||`, `&&`, `&`, `;`, `set`, redirection or write
to `$GITHUB_ENV` or `$GITHUB_PATH` can appear), except `required`'s one step, which must equal the
text in `ci.rs` byte for byte; every `uses:` is actions/checkout, jdx/mise-action or
Swatinem/rust-cache with only the inputs `ci.rs` lists (mise-action with `env: false`, so mise's
`[env]` never reaches the gate, and an `install_args` of `--locked` followed by distinct tools of
`CI_TOOLS`, never a flag, the JDK or Apalache, nor a bare `--locked`, which installs every tool);
and it fails on an `if:` anywhere but `required`, on
`continue-on-error`, on a `working-directory`, on a shell other than `bash`, on a job with
`container:` or `services:`, on a `just` command whose first word is not a recipe of the justfile
(a variable override such as `cargo=true`, or a flag), on an allowed action that omits a required
input (mise-action's `env`, checkout's `persist-credentials`), and on any `env:` entry outside
`ALLOWED_ENV` in `ci.rs`, which fixes each name, its value and its level: `CARGO_TERM_COLOR=always`
and `NEXTEST_PROFILE=ci` for the workflow, and the job token and event fields the `security`,
`dco` and `required` steps pass on (so `BASH_ENV`, `PATH`, `CI`, `JUST_*` and the build's own
variables are refused by construction).

Runs on `main` are never cancelled or replaced by a newer push: each commit there has its own
concurrency group. A pull request's older run is cancelled by its newer one. Caches (mise's and
`Swatinem/rust-cache`'s, keyed by the toolchain, `Cargo.lock` and `mise.lock`) are saved only
from `main`.

CI differs from a local gate in three ways that a test can depend on: its target directory is
`./target` inside the checkout (a scratch directory under it is inside this workspace), every
child process inherits the workflow's `env:` (`NEXTEST_PROFILE=ci`, `CARGO_TERM_COLOR=always`), and the `linux-arm` job (and
`macos.yml`'s `macos` job) run on other platforms. `just ci-parity` reproduces the first two
locally; the platforms are seen only after the push, by design of ADR 0002, so a red `linux-arm`
job on `main` is fixed at once, and a red nightly `macos` run as soon as it is reported.

## Reported checks

Not in `Required`: the owner's decision at the FORMAL checkpoint (2026-11-18, option B; BOOT-04)
took the macOS job out of `ci.yml`. [`.github/workflows/macos.yml`](../.github/workflows/macos.yml)
runs it nightly (`schedule`) and by hand (`workflow_dispatch`), never on a push or pull request; a
red run is reported to the owner and fixed as a task (fnd §7.1), and does not block landing. The
orchestrator may also dispatch it on a landed commit that touches macOS-specific code
(`#[cfg(target_os = "macos")]` items, file watching, sockets, processes). Linux arm64 stays in
`Required` (`linux-arm` above): nothing argued for moving it, it is a supported target (ADR 0006)
on a Linux runner like the other jobs, and it is the only native arm64 check on pushes.

`ARCH-ci-lockstep` reads `macos.yml` too and holds its jobs to `ci.yml`'s job, step, action,
`env:`, shell and `run:` rules; its `on:` must be exactly `schedule` (`cron` entries only) and a
bare `workflow_dispatch`; no job may be named `required`, have `needs:` or run a stage of
`check`; and the table below must match its jobs as the table above matches `ci.yml`'s.

| Job id | Display name | Runner | Runs (`just ...`) |
|:--|:--|:--|:--|
| `macos` | macOS (arm64) | `macos-15` | `platform-tests` |

## Planned checks

Not in `ci.yml` yet; each arrives with its task, which adds the job, its row above and its name
to `required`'s `needs` in one commit.

| Job id | Display name | Runs | Arrives with |
|:--|:--|:--|:--|
| `coverage` | Coverage | `check-only coverage changed-lines`, then `just mutants-diff` (full history; diff base `git merge-base HEAD origin/main`, 20-minute cap) | FND-06 |
| `fixtures` | Java fixtures | `check-only java-fixtures` (pinned JDK) | EVAL-07 |
| `e2e` | End to end | `check-only e2e-tests lsp-conformance` (until this job exists, `lsp-conformance` runs in `tests`; IDX-13 moves it here) | IDX-13 |

## Other workflows

Not in `required`:

| Workflow | Trigger | Runs |
|:--|:--|:--|
| `security.yml` | weekly, dispatch | `just check-only deny` (new advisories can fail an unchanged tree), `just check-only secrets` (gitleaks over the full history), `just workflows-online` (zizmor's online audits) |
| `codeql.yml` | push to `main`, pull request, weekly, dispatch | CodeQL for Rust and Actions (`build-mode: none`), results in code scanning |
| `scorecard.yml` | push to `main`, weekly, dispatch | OpenSSF Scorecard, SARIF to code scanning, results published |
| `.github/dependabot.yml` | weekly | cargo and github-actions, minor and patch grouped, a 7-day cooldown |
| `macos.yml` | nightly, dispatch | `just platform-tests` on `macos-15` (see "Reported checks") |
| `formal.yml` | weekly, dispatch | `just formal-verify <model> <part>`, one job per model of `formal/models.toml` and part: `apalache` (Apalache's bounded verification; a JVM per model, so outside `check`) and `simulation` (the Quint simulations), each with the job's 60 minutes to itself (IDX-09e); `crates/jfl-conformance/tests/models.rs` keeps the matrix equal to the registry and the parts to xtask's (FORMAL-01) |

Planned: `bench.yml` (the corpus suites on one fixed runner class, history upload, and its
`bench-gate` job, which blocks `main` from LSP-06, the M1 gate (ADR 0034): red there is fixed like red CI; it is kept out
of `ci.yml` because wall times are comparable only on one runner class, fnd §9), `fuzz.yml` (nightly), `mutation.yml` (weekly), `release.yml`
(tags). A red run of any of them is reported to the owner and fixed as a task (fnd §7.1).

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
