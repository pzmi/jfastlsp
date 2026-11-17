---
name: review
description: Read-only review of a git range (default since the newest checkpoint-* tag) against jfastlsp's enforced architecture and authority rules, the cited plan/foundation/verification sections and ADRs, the result envelope's honesty (complete=true never wrong), views, generations and freshness, LSP spec fidelity (UTF-16 positions, cancellation, JSON-RPC framing limits), daemon lifecycle and socket permissions, worktree isolation, untrusted Java input, processes, model-first Quint conformance, contract suites, performance claims, test quality and supply chain. Produces findings by severity with file:line and a verdict. Use at review checkpoints (⛳ tasks in doc/tasks.md), before landing a task commit, or when asked to /review.
---

# /review

Review a range of commits (or an uncommitted diff against `HEAD`) and report. **This skill never
edits a file, never commits, never tags, never pushes and never merges.** Fixes are the
implementer's job, after the report. A subagent that cannot load it by name reads this file and
follows it.

## Input

`/review [range]` where `range` is any git range (`a..b`, a single commit, `HEAD~N`), or `HEAD`
plus "uncommitted" for the working tree.

Default range: `<newest checkpoint tag>..HEAD`, found with
`git tag --list 'checkpoint-*' --sort=-creatordate | head -1`. If there is no such tag, use the
root commit to `HEAD` and say so. If the range is empty, report a clean verdict for an empty
range and stop.

## Procedure

1. `git log --oneline <range>` and `git diff --stat <range>`. Note the task ids in the subjects
   (`<ID>: summary`).
2. For each task id, read its card in `doc/tasks.md`: the sections of `doc/plan.md`,
   `doc/foundation.md` (fnd) and `doc/verification.md` (ver) it cites, and its `Accept:` items.
   Read those sections, and the ADRs in `doc/adr/` that touch the changed crates. Decisions in
   `doc/tasks.md` and later ADRs override earlier plan text.
3. Read every changed file in full (not just the hunks), plus the tests beside it.
4. Run the live gate (`just check`) in the
   foreground and record pass/fail and the test count. If it rewrites tracked files, report
   which as a finding; do not revert or keep them yourself.
5. Walk the checklist below. Only report what you verified in the code; cite `file:line`.

## Checklist

**Enforced architecture** (`architecture.toml`, `deny.toml`, the `clippy.toml` lists, `just check-only architecture deny`; plan §6, fnd §2,
§3): cli → composition → adapters → application → pure crates; adapters never reference sibling
adapters or composition; `jfl-process` reaches no adapter; production crates never depend on
`jfl-testkit` or `jfl-conformance`; tool crates (`jfl-oracle`, `jfl-eval`, `jfl-jdtls`, `jfl-bench`) import no production
crate; a new crate has its `members` entry, `architecture.toml` entry and external allowlist in
the same commit, and each new rule has a negative fixture. A change that loosens
`architecture.toml`, `[workspace.lints]`, `deny.toml` or a test without a stated reason (the task
asked for it, the commit says why, or an ADR exception where the policy requires one) is HIGH.

**Authority** (plan §6.5): pure crates use no `std::{fs,net,process,env}`, clock, randomness,
threads or Tokio; a child process is started only through `jfl-process` (argv only, never
`sh -c` or a string built from input; cleared environment plus an allowlist; a deadline; its own
process group, killed as a group; bounded captured output), the daemon launcher and the JVM
helpers included;
sockets only in `jfl-adapter-ipc`, Unix domain only, never TCP. Tool and test crates hold the
grants their `architecture.toml` row gives them (fnd §2.4: `jfl-jdtls`'s socket and daemon,
`jfl-testkit`'s sleep and clock inside `poll` only). A spawn, a socket, a filesystem write, a
clock, a sleep or an environment read outside the crates fnd §2.4 grants it to is HIGH. Rules no tool enforces
(fnd §3.3) are checked here: application command code naming query modules or the reverse, a
`tracing` value reaching a branch, a hidden seam into composition, a non-literal event name,
arithmetic on LSP `Position` fields outside `jfl-domain`.

**No JVM on the request path** (ADR 0015, plan §3): a query handler never awaits a `javac`
worker, a Gradle run or any build; diagnostics are pushed, not requested; `jfl diag` waits with a
deadline and returns what it has. A request that can block on a JVM is HIGH.

**The result envelope** (ADR 0014, plan §2.1, §2.2): every navigation answer carries `exact`,
`possible`, `complete` and `incomplete_because`; `exact` only with evidence (resolver or
compiler); `possible` a sound superset (simple name and arity in every file that can see the
declaring module, comments and strings excluded, each with its `why`); `complete=true` only when
every module that can see the target's module is indexed and fresh at the pinned generation;
definition's bounded ambiguity never returns a same-name member of another type; an empty
answer without a completeness statement is HIGH. **A path by which `complete=true` can be wrong
is HIGH**, including one through a deadline (a timed-out query returns `complete=false`), an
unindexed or generated source root, a nested worktree, an overlay older than disk, or a module
whose visibility is unknown.

**Views, generations and freshness** (plan §4, `formal/views.qnt`): each request resolves its
file to the innermost worktree root (per URI, never per connection); nested worktrees
(`.git` file or directory, `.claude/worktrees/`) are excluded from their parent's scan; every
answer is computed at one pinned generation and never mixes two; a generation is published
atomically after every summary it names is stored; the per-request stat of the file, `HEAD` and
the index triggers a reconcile before the answer; disk wins over an overlay older than the disk
mtime seen. A cross-view result, a mixed-generation answer or a stale answer after a shell edit
is HIGH.

**Daemon lifecycle and socket** (plan §5.2, §5.3, `formal/daemon.qnt`): one daemon per state
directory, enforced by a lock held for the daemon's life; the state directory created `0700`
and an existing one refused unless owned by the daemon's uid with mode `0700` (the directory is
the access control, plan §5.3); the socket and lock bound inside it, then `0600`; a peer's uid
checked against
the daemon's; proxies spawn the daemon on demand and reconnect after a restart; a restarted
daemon holds no buffer from a dead connection (no ghost buffers); idle exit and SIGTERM release
the socket and the lock; every spawned task owned, cancellable and joined on shutdown.

**LSP fidelity** (plan §2.5, the LSP 3.17 specification): positions in the negotiated encoding
(`utf-16` unless the client offers another), converted by the pure functions of `jfl-domain`,
never by byte offsets; `Content-Length` framing with a maximum frame size enforced before
allocation, header parsing that refuses junk, and a bounded queue; every request gets exactly
one response (result or error), notifications get none; `$/cancelRequest` answered with
`RequestCancelled` (or the result if it already finished); `initialize` answers without waiting
for indexing; `shutdown` then `exit` ends cleanly, `exit` without `shutdown` exits 1; unknown
requests get `MethodNotFound`, unknown notifications are ignored; capabilities advertise only
what is implemented; `.lsp.json` uses only documented keys. A deviation from the specification
is MEDIUM, HIGH when a client could hang or misplace an edit.

**Diagnostics** (plan §3, `formal/diagnostics.qnt`): every published set is tagged with the
document version it was computed for; a result for an older version is never published over a
newer one; semantic diagnostics only from `javac` with a known classpath, never from the
resolver; with annotation processors detected, errors on generated members are suppressed and
marked, never shown as authoritative. A false error published as authoritative is HIGH.

**Untrusted input** (plan §11): Java sources, class files, JARs, JSON-RPC and protocol frames,
Gradle output and git output are input. Bounded size, depth and count; no `unwrap`, `expect`,
unchecked indexing or arithmetic overflow on input-derived values; no panic; a parser without a
fuzz target or property test is MEDIUM; a reachable panic is HIGH. The Gradle model runs the
project's build code: only after the opt-in plan §5.7 describes (HIGH otherwise).

**Formal conformance** (ver §6, ADR 0007): a change to views and generations, the daemon
lifecycle or diagnostics publication updates the Quint model, named scenarios and the
conformance driver in the same change (model first) and registers new models in `MODELS`; the
driver drives real code and compares after every step; a negative control exists and still
fails; no invariant removed or weakened (HIGH).

**Contract suites** (ver §5): a port with a fake and a real adapter runs one shared suite against
both; the suite's own tests prove a deliberately broken implementation fails it. A fake that
passes by sharing production code is MEDIUM.

**Claims against evidence**: a performance claim (latency, memory, "fast enough", a budget) names
the benchmark or measurement that supports it (ver §10), with the machine and the numbers; an
"exact" or "complete" claim in a doc or report names the test or oracle run; "seen failing",
"refused" or "never" in a commit body or report is checked against a test. An unsupported claim
is MEDIUM; a budget set or loosened without a measurement is HIGH. **Speed** (plan §1.7,
ADR 0031): a product task that changes indexing, queries, storage or the daemon reports its
metrics on the bench corpus (small at least); a missing report is MEDIUM, a regression over 10 %
accepted without the owner is HIGH.

**Logging** (fnd §5): `tracing`, fixed `snake_case` event names, data in fields, no INFO on the
healthy path, no source text, file contents or absolute home paths at INFO or above.

**Language rules**: `unsafe_code` forbidden; no `todo!`, `unimplemented!`, `dbg!`; no
`#[ignore]`; no `#[allow]`/`#[expect]` or coverage exclusion without an `architecture.toml`
exception citing an ADR; `#[cfg]` not `cfg!`; no `unwrap`/`expect` on input-derived values in
production paths; no unbounded channel; no lock held across `.await`.

**Plan fidelity and ADRs**: behaviour matches the cited sections and every `Accept:` item; a
deviation is recorded in `doc/plan.md`/`doc/tasks.md` in the same commit or is a finding. ADRs
are records, not rules (ADR 0001, amended): only an important decision whose context is worth
recalling later needs one, and everything else needs its reason in the commit message (and a code
comment for a non-obvious constraint). An important decision with no ADR, or any decision whose
reason is recorded nowhere, is MEDIUM; do not flag a missing ADR for a decision the commit message
explains. Code contradicting an Accepted ADR without amending or superseding it is MEDIUM, HIGH
if it loosens a security or architecture rule. A Proposed decision flipped to Accepted by an
agent is MEDIUM. A `DEC-` outcome written by anyone but the owner is HIGH.

**Tests** (ver §2): each `Accept:` item has a test that would fail without the change, at the
lowest level that can fail for its reason (a higher one is MEDIUM naming the level); names
`should_<outcome>_when_<condition>` with `// given / when / then`; no sleeps (fake clock,
`tokio::time::pause`; real-OS waits only via the testkit `poll` helper); tempdirs, sockets in
tempdirs; real objects or testkit fakes, no mocking frameworks; property regressions committed
with their seed; goldens (Java corpus, LSP transcripts, `--json`) never rewritten to get green.
Missing tests for a new branch is MEDIUM.

**Supply chain** (fnd §7): a new dependency added with `cargo add`, justified in the commit
message, in the crate's external allowlist, licence accepted by `deny.toml`, no git or foreign
registry source without its recorded reason and per-crate exception; a dependency with C code (tree-sitter grammars) named as such;
`Cargo.lock` consistent (`--locked` passes); Actions pinned by full commit SHA with minimal
`permissions`; no `pull_request_target` with checkout of untrusted code. **Reuse** (ADR 0019,
plan §16): a new subsystem built by hand where a maintained crate or tool fits, without the card's
`Reuse considered:` line or the commit saying why, is MEDIUM; GPL code linked into the binary, EPL code outside a separate process, or
code, fixtures or positions taken from a non-permissive corpus repository (elasticsearch) is
HIGH; a tool not pinned in `mise.toml` is MEDIUM.

**Public repository** (ADR 0004): flag any name or path of a private project of the owner's, any
wording that hints a design came from an unnamed elsewhere ("an earlier project", "the same
owner", lessons "we learned before"), content of a private Java repository (its code, paths,
class names or task text in fixtures, eval reports or commit bodies), personal paths, internal
hostnames, tokens or model ids, in files and in commit messages (HIGH). Naming the public
repositories site-preview and mayfly where a design is ported is allowed.

**Housekeeping**: a doc comment's behavioural claim names the test that pins it
(`/// Pinned by: <test path>::<name>`), and that test exists; a behavioural claim in the README,
`AGENTS.md`, the skill shipped to agents, a commit body or a task report is checked against the
code and either names its test or is written as planned or a limitation (MEDIUM); an ADR changed
without the rules, fixtures and tests that pin it, or code drifting from an ADR without an
amendment or a superseding record, is MEDIUM; stale docs are findings; no unrelated files;
commit subject `<ID>: summary`, an `Impact:` line, the trailer block last (`Signed-off-by`,
`Co-Authored-By`, `Claude-Session`), author `Patryk Żmigrodzki <pzmi@users.noreply.github.com>`.

## Output

Findings only, most severe first. Severity: **HIGH** (wrong behaviour, security, broken
invariant, rule loosened or suppressed, `complete=true` wrong), **MEDIUM** (missing test, missing
ADR, doc drift, unsupported claim, likely bug), **LOW** (style, naming, nit).

```
## Review <range> (<n> commits, tasks <ids>)
gate: <command> pass|fail (<test count>)

### HIGH
- crates/x/src/file.rs:42: what is wrong; why it matters; suggested fix direction
### MEDIUM
- ...
### LOW
- ...

Verdict: CLEAN | CHANGES REQUESTED (<n> HIGH, <m> MEDIUM)
```

Omit empty severities. A clean range gets an explicit `Verdict: CLEAN` with one line on what was
checked. Any HIGH means CHANGES REQUESTED.

## After the review

A checkpoint task ends with a local annotated tag `checkpoint-<ID>` (e.g. `checkpoint-IDX-15`).
The orchestrator creates it, only once the review and the adversarial second review have passed
(clean, or every HIGH and MEDIUM fixed), and never pushes it. This skill does not tag.
