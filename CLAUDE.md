# CLAUDE.md

## Project

jfastlsp is **agent-centric Java code intelligence in Rust**: one binary, `jfl`, whose `jfl lsp`
is a stdio language server (built first, M1) and whose other subcommands are a JSON CLI (second,
M2; ADR 0034). Both are thin proxies over
one daemon per user (Unix socket, single owner by lock) that indexes the Java sources of many Git
worktrees of a large Gradle repository at once. Every navigation answer is an envelope with
`exact`, `possible` and `complete`, and **`complete=true` is never wrong** (ADR 0014). No JVM and no
build on the request path (ADR 0015). **Speed is the primary value**: every change is measured on
a pinned real-repository corpus (plan §1.7, ADR 0031). Reuse before building (ADR 0019). Status:
**pre-alpha, planning**; phase 0 measurements decide go/no-go (DEC-02). Public, open source
(`MIT`, ADR 0033).

## Docs

| File | What it holds |
|:--|:--|
| `doc/plan.md` | Product and architecture: goal, kill criterion and speed budget (§1, §1.7), the agent contract and envelope (§2), diagnostics (§3), worktrees, views and freshness (§4), runtime (§5), crates and dependency law (§6), Quint models (§7), phase 0 (§9), milestones (§10), security (§11), open decisions (§14), reuse and dependencies (§16) |
| `doc/bench-corpus.md` | The pinned benchmark repositories and the E1 spike's numbers |
| `doc/foundation.md` | Engineering rules: dependency law and authority (§2), enforcement (§3), language and lints (§4), logging (§5), persistent state (§6), CI (§7), tool pins (§8), commands (§9), commits (§10), agent contract (§11) |
| `doc/verification.md` | Test levels (§2), determinism (§3), goldens (§4), contract suites (§5), formal models (§6), oracles (§7), fuzz (§8), benchmarks (§10), agent evaluation (§11) |
| `doc/sdlc.md` | Ready, procedure, Done, the gate, integration, checkpoints, owner decisions |
| `doc/required-checks.md` | The CI jobs and the `Required` aggregate |
| `doc/tasks.md` | Decisions (ADR numbers), rules for every task, the live gate, lanes and task cards, ⛳ checkpoints |
| `doc/adr/` | Decisions, Nygard style; the ADR number is the decision id; index in `doc/adr/README.md` |
| `spikes/e1-parse/` | The archived E1 parse spike: evidence for plan §1.7 and §5.5, the seed of BENCH-02; outside the workspace |

Citations read "plan §4.3", "fnd §3", "ver §6"; read the sections a task cites, not whole
documents. Decisions in `doc/tasks.md` and Accepted ADRs override earlier plan text. The owner
decided every record at the FND checkpoint (DEC-07, 2026-11-17): all are Accepted (owner) except
0005, 0011–0013, 0027 and 0030, which are superseded; a new record is Proposed until the owner's
next checkpoint.

## Workflow

- `/orchestrate [ids|lane]`: run ready tasks through background agents in worktrees, review each
  commit, land it on `main`, watch CI, stop at ⛳ checkpoints. The orchestrator never implements.
- `/work <ID>`: one task through intake, plan, implement, test, polish, review, qa, ship,
  babysit. State in `.work/<ID>/` (gitignored). An agent that cannot load the skill by name reads
  `.claude/skills/work/SKILL.md` and its station files and follows them.
- `/review [range]`: read-only review; default range since the newest `checkpoint-*` tag.
- `CLAUDE.md` and `.claude/` belong to the orchestrator: a task agent reports a needed change.
- `DEC-` tasks and `(owner)` items are the owner's: agents prepare, never decide or run them.

## The gate

The gate is `just check` (fnd §9; ADR 0035 moves it to `cargo xtask` piece by piece, lane XT);
CI runs the same stages (`doc/required-checks.md`):

```bash
bash scripts/bootstrap-tools.sh      # pinned mise and just, checksum-verified, then `mise install --locked`
mise install --locked                # every pinned tool from mise.lock; `just doctor` checks them
just quick                           # fmt check, architecture, clippy, fast tests: while working
just check                           # the full blocking gate: before every commit
just check-only <stage>...           # named stages through the same recipes (what each CI job runs)
just bench [suite] [corpus=small]    # benchmarks (BENCH-02); reports, never gates; `just bench-corpus` fetches the corpus
just ci-parity                       # the cargo stages with CI's environment, before landing tests that start cargo
adrs new "<title>"                   # a new ADR; `just adr-index` checks the index
scripts/land.sh <sha>                # orchestrator only (ADR 0002)
```

`quick` is not `check`; never report one as the other. A missing tool fails, never a
skipped-but-green check. Run the gate in the foreground with `set -o pipefail` semantics (no
`| tail` in an `&&` chain); commit only on green. A repeatable job becomes a `just` recipe.
**Never prefix a command with inline `JFL_*`, `QUINT_SEED` or `PROPTEST_*` variables**; a
scenario that needs one becomes a recipe parameter. The gate's own recipes, scripts and
configuration are pinned (`.config/gate.sha256`, `ARCH-gate-config`, fnd §3.6): a change to one
moves its pin in the same commit, and review reads it as a change of the gate. Every check is an
allowlist or a pinned shape, never a list of bad patterns (fnd §3).

## Enforced rules

- **Dependency law** (plan §6.3, fnd §2): cli → composition → adapters → application → pure
  crates. Adapters never name a sibling adapter, composition or the CLI. Production crates never
  depend on test-only crates; `jfl-oracle`, `jfl-eval` and `jfl-bench` import no
  production crate. A new crate gets its `members`, `architecture.toml`, `deny.toml` and
  `clippy.toml` entries in the same commit. Fix an architecture failure in the structure, never by
  loosening the policy.
- **Authority** (plan §6.5, fnd §2.3–§2.4): pure crates (`jfl-domain`, `jfl-protocol`) are
  `#![no_std]` + `alloc` and build for `x86_64-unknown-none` (the `nostd` stage). Production
  crates: children only through `jfl-process` (argv only, never `sh -c`; cleared environment plus
  an allowlist; deadline; own process group; bounded output), the daemon launcher and the JVM
  helpers included; sockets only in `jfl-adapter-ipc`, Unix domain, in a `0700` state dir that is
  the access control (socket and lock `0600`), peer uid checked; clock and environment only in
  composition and `jfl-process` (the CLI reads the environment for defaults); no sleeps. Tool and
  test crates hold exactly the grants of their `architecture.toml` row (process, socket, fs,
  spawn, sleep, clock, env; fnd §2.4). Never TCP.
- **The envelope** (plan §2.1): `exact` only with evidence; `possible` a sound superset with
  reasons; `complete=true` only when every module that can see the target is indexed and fresh at
  the pinned generation; never an empty answer without a completeness statement.
- **Views and freshness** (plan §4): bind per request URI to the innermost worktree; nested
  worktrees (`.git` file or dir, `.claude/worktrees/`) never scanned by their parent; one pinned
  generation per answer; atomic publish; a per-request stat of file, `HEAD` and index; disk wins
  over an older overlay.
- **LSP fidelity** (plan §2.3, §2.5): positions in the negotiated encoding (UTF-16 by default)
  through `jfl-domain`'s conversions; framing limits checked before allocation; every request
  answered or cancelled; `initialize` never waits on indexing; `.lsp.json` uses documented keys only.
- **Untrusted input** (plan §11): Java sources, class files, JARs, frames and tool output are
  bounded and never panic; no `unwrap`, `expect`, indexing or unchecked arithmetic on input.
- **Language**: `unsafe_code` forbidden; no `todo!`, `unimplemented!`, `dbg!`, `#[ignore]`; no
  `#[allow]`/`#[expect]` without an `architecture.toml` exception citing an ADR; zero clippy
  warnings; `#[cfg]` items, never a `cfg!(target_os…)` branch.
- **Tests** (ver §2): `should_<outcome>_when_<condition>` with `// given`, `// when`, `// then`,
  at the lowest level that can fail for their reason; no sleeps (fake clock, channels, the testkit
  `poll` helper); no mocking frameworks; tempdirs. A port with a fake and a real adapter runs one
  contract suite against both. Never delete a failing test, weaken an invariant, loosen a budget
  or rewrite a golden to get green.
- **Model first** (ADR 0007): a change to views and generations, the daemon lifecycle or
  diagnostics publication updates the Quint model, its scenarios and the conformance driver
  before the Rust code. **Golden first** for protocol, `--json` and LSP changes.
- **Claims need evidence**: a doc comment's behavioural claim names its test
  (`/// Pinned by: <path>::<name>`); a performance claim names its benchmark and machine.
- **Logging** (fnd §5): `tracing`, fixed `snake_case` event names, data in fields; no INFO per
  request; never source text.
- **Dependencies**: reuse first; a card's `Reuse considered:` line says what was weighed (ADR
  0019, plan §16). `cargo add`, reason in the commit; `Cargo.lock` committed, builds `--locked`;
  licences per `deny.toml`; tools pinned in `mise.toml`; GitHub Actions pinned by commit SHA.
- **Speed** (ADR 0031): a product task runs on the bench corpus (small at least) and reports the
  metrics it changes; from M1 a >10 % regression fails `bench.yml`'s `bench-gate` on its fixed
  runner class (local runs report, never gate). Never loosen a budget.
- **Decisions** (`doc/adr/README.md`, ADR 0001 as amended): an ADR only for an important
  decision whose context is worth recalling later; otherwise the commit message says why and a
  code comment explains a non-obvious constraint. An ADR is a record, not a rule: a change that
  reverses one amends or supersedes it in the same commit, with the rules and tests that pin it.
  Only the owner flips Proposed to Accepted or decides a `DEC-` task.

## Commits

- Direct on `main` (ADR 0002): one DCO-signed commit per task, reviewed and landed by the
  orchestrator; implementing agents commit on their lane and never push or tag.
- Author and `Signed-off-by`: `Patryk Żmigrodzki <pzmi@users.noreply.github.com>` (pass
  `--author`); the committer stays the environment's identity (`Claude <noreply@anthropic.com>`),
  which signs (ADR 0003).
- Subject `<ID>: summary`, imperative, under 72 chars; a body that says why, names each new
  dependency, and carries an `Impact:` line.
- Trailer block last, written by hand: `Signed-off-by`, `Co-Authored-By`, `Claude-Session`, the
  last two exactly as the session's attribution instructions give them. Never `git commit -s`
  with other trailers.
- Stage only your own files (never `git add -A` or `git add .`). Never `--no-verify`, never
  force-push, never push tags; `checkpoint-*` tags are local.
- **Agents never push, by any means.** `.claude/settings.json` denies the common forms of
  `git push` (`git push …`, `git -C <dir> push …`, `git -c <k=v> push …`, `git --no-pager push …`,
  `git --git-dir=… push …`, `git --work-tree=… push …`; ADR 0018, amended);
  `scripts/land.sh`, run as a script by the orchestrator, is the only pusher.

## Sandbox

- Linux, 4 CPUs and about 15 GB RAM: **at most two lanes**, each with `CARGO_BUILD_JOBS=2`,
  `CARGO_INCREMENTAL=0` and its own `CARGO_TARGET_DIR` under the session scratchpad, on disk (not
  `/dev/shm`), deleted when the lane is done. JVM-backed tests take a lane's whole budget.
- **The sandbox runs as root; CI does not.** A test about file modes or ownership also passes
  under `setpriv --reuid=<uid> --regid=<gid> --clear-groups` before committing.
- Node (Quint) and the JDK (Apalache, the javac worker, Java fixtures) are dev-time only; `jfl`
  never needs them to start. Never run Gradle or a build on a target Java repository; use the
  synthetic fixtures. The bench corpus is read from its cache, never committed; building it runs
  only in `bench.yml` or owner-side. `gh` may be absent: use the GitHub MCP tools.
- Hooks: SessionStart installs the pinned toolchain
  and runs `scripts/bootstrap-tools.sh` (mise and just, then `mise install --locked`), reporting
  and never blocking; every edited `*.rs` is rustfmt'ed, so re-read a Rust file before the next
  Edit of it rather than fight a stale-file refusal; Stop runs `just quick` with
  `CARGO_BUILD_JOBS=2` when code, manifests, the policy, tool pins, the gate's configuration,
  scripts, workflows, a model or a fixture changed, and fails when `just` is missing. Stop guards only
  the checkout the session started in: in a lane worktree the ship station's gate is the guard.
- Network (verified 2026-11-16): GitHub release downloads, crates.io files, npm and PyPI pass the
  proxy; `api.github.com` and GitHub pages do not, so tools install from `mise.lock` with
  `--locked` (fnd §8).
- Temporary files go in the session scratchpad, never `/tmp` or the repo. Kill only PIDs you
  started, never by pattern. Never end a turn with the gate, a daemon or a JVM still running.

## Standing rules

They bind every session. They are the orchestrator's operating rules from the start of the
project, not incidents of this repository; an incident here adds a dated line.

1. **Review every task commit before landing it**, not only at ⛳; for the socket, the lock,
   `jfl-process`, the Gradle model and parsers, the review finishes before `land` starts.
2. **Gate commands keep their exit codes**: `set -o pipefail`, no `| tail` inside an `&&` chain.
3. **After an agent ends, find what it left running** (`readlink /proc/<pid>/cwd` under its
   worktree) and stop it by PID.
4. **A background command is cut at the harness limit (1 to 2 hours)**: split long runs, each
   with its own explicit timeout.
5. **Resume a stopped agent by `SendMessage`** ("continue WITHOUT resetting"); never discard its
   uncommitted change unread; never start a second agent on the same task.
6. **An agent that changes git config, signing, hooks or harness settings beyond its brief** is
   stopped and the change reverted.
7. **Every CI job failing within seconds with no log** is an Actions quota or spending limit, not
   red code: tell the owner once.
8. **Worktrees start from a fetched `origin/main`**, never a stale local `main`.
9. **Before cancelling a CI run, read every job's state**; re-run a red job at most once, with
   the reason written down; a failure that passes on a rerun is a race to find.
10. **Never edit a worktree while its gate or `land` runs in it**; stop the run and start again.
11. **Check a commit's signature with `git cat-file -p <sha> | grep -c ^gpgsig`**, not `%G?`
    (with no allowed-signers file it prints `N`).

## Public repository

Never commit secrets, tokens, credentials, internal hostnames, personal paths, model ids or
content of a private repository. **Never name or allude to a private project** of the owner's
anywhere (code, docs, ADRs, fixtures, evaluation reports, commit messages), and never hint that a
design came from an unnamed elsewhere (ADR 0004). The public repositories site-preview and mayfly
may be cited where a design is borrowed. Nothing from a private target Java repository enters this
repository: harnesses and runbooks here, runs on the owner's machine, aggregates only. Fixtures are
synthetic.
