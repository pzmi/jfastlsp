# jfastlsp: product and architecture plan

**Status:** plan v3.1, 2026-11-16 (v3 with the reuse survey and early benchmarks applied, BOOT-01),
pre-alpha (no code yet). Owner: Patryk Żmigrodzki.
**What:** agent-centric Java code intelligence in Rust. One binary, `jfl`: `jfl lsp` is a stdio
language server, every other subcommand is a JSON CLI. Both talk to one shared daemon that
indexes the Java sources of many Git worktrees of a large Gradle repository at once.
**Consumer:** Claude Code agents, many at once, each in its own Git worktree.

This document owns *what* is built and *how it is structured*. [`foundation.md`](foundation.md)
owns the engineering rules (workspace enforcement, gate, commands, commits, agent contract),
[`verification.md`](verification.md) owns how it is shown to be right (test levels, oracles,
formal models), [`sdlc.md`](sdlc.md) owns how a change reaches `main`, and
[`tasks.md`](tasks.md) owns order and size. Decisions are ADRs in [`adr/`](adr/README.md); an
Accepted ADR overrides this text. A citation "plan §4.3" points here.

---

## 1. Goal, success and limits

### 1.1 Goal

An agent working in any of N concurrent worktrees finishes Java tasks with fewer tool calls,
fewer tokens and fewer broken builds than with grep alone or with a tuned `jdtls`. The marginal
cost of the Nth worktree is small.

**Speed is the product's primary value** (owner, 2026-11-16). An answer an agent waits for is an
answer it replaces with grep; a server that takes minutes to import a Gradle build per worktree is
one an agent never gets to use. Every design choice is judged first by what it costs in latency,
time to first answer and memory at 10 worktrees, measured on real repositories from the first
weeks (§1.7, ADR 0031), not at the end.

### 1.2 Success metrics

Success is measured on agent tasks first and machine economics second.

| Order | Metric | Source |
|---|---|---|
| 1 | Task success rate (tests and build green after the agent's change) | E0 (§9.2) |
| 2 | Tool calls and tokens per successful task | E0 |
| 3 | Edits that broke compilation and were caught by pushed diagnostics, not by a later build | E0 |
| 4 | Coverage: share of real call sites resolved `exact`, and the precision of those | E2, SEM-06 |
| 5 | Aggregate RSS, CPU-seconds and time to first useful answer for 1, 3 and 10 worktrees; the budgets of §1.7 | E1, E4, BENCH lane |

### 1.3 Kill criterion

The baseline is **jdtls behind our multiplexing proxy** (ADR 0028), built in phase 0 as
`jfl-jdtls` (EVAL-11): one jdtls per worktree root shared by every client in it, `-data` keyed on
the full worktree path, a shared library index (`-Djdt.core.sharedIndexLocation`), a heap cap and
a warm Gradle. The stock `jdtls-lsp` plugin is not a fair baseline: every subagent starts its own
jdtls of 2–3 GB ([claude-code#64536](https://github.com/anthropics/claude-code/issues/64536)) and
worktrees with the same basename share one workspace directory
([claude-code#67182](https://github.com/anthropics/claude-code/issues/67182)). If that baseline
scores within noise of this design on rows 1 to 3 at 10 worktrees, and its memory fits the target
machines, the deliverable becomes `jfl-jdtls` itself instead of a new engine. The owner decides at
DEC-02 (ADR 0009).

### 1.4 Non-goals (v1)

A Java compiler or build tool; refactoring, rename or code actions; completion; formatting;
Kotlin, Groovy or Scala sources; Maven or Bazel project models (Gradle only, §5.7); a GUI;
remote or multi-user daemons; Windows (ADR 0006); telemetry of any kind (§12).

### 1.5 Guardrails

- **No JVM and no build on the request path** (ADR 0015). JVMs run only as background workers
  (`javac`, §3) and the opt-in Gradle model (§5.7).
- **Honest answers.** Every navigation answer says how complete it is (§2.1). `complete=true` is
  never wrong; a wrong `complete=true` is the worst bug this product can have.
- **Untrusted input.** Java sources, class files, JARs, protocol and JSON-RPC frames are parsed
  defensively: bounded, never a panic (§11).
- **Trusted code is opt-in.** Running a project's Gradle build code needs an explicit opt-in per
  repository (§5.7).
- **Public repository** (ADR 0004): no secrets, private code, personal paths or names of private
  projects; fixtures are synthetic; evaluation results from a private target repository enter
  this repository only as aggregates (§9.1).

### 1.6 Platforms

Linux x86-64 and arm64, macOS arm64 (ADR 0006). Windows is out of scope; Unix sockets, `flock`
and process groups are used freely. Every platform-specific path is a `#[cfg]` item, tested on its
native CI runner.

### 1.7 Speed targets and the performance budget

Measured by the BENCH lane (ADR 0031) on the pinned corpus of
[`bench-corpus.md`](bench-corpus.md) (small junit-framework, medium spring-framework and kafka,
large elasticsearch, all public Gradle Java repositories at fixed commits) and on the target
repository (owner-run). The "measured" column is the E1 spike, **measured 2026-11-16 on a 4 vCPU
Xeon 2.3 GHz VM** (15 GB RAM, virtio disk; tree-sitter 0.27.2 with tree-sitter-java-orchard
0.5.25, 4 threads; warm is the median of three runs, cold follows a page-cache drop). Its budgets
leave about 1.5× headroom. Rows marked *v3* come from plan v3's deadlines; rows marked
*proposed* are planner targets that BENCH-03's baseline confirms or corrects. The owner confirms
the table at DEC-02; a target is changed only by the owner, never loosened to make a run pass.

| Metric (corpus) | Budget | Measured 2026-11-16 | Arm to beat |
|:--|:--|:--|:--|
| Full syntax index, cold, 4 cores (large: elasticsearch, 33.9k files, 334 MB) | ≤ 13 s | 8.7 s cold, 7.2 s warm (about 48 MB/s) | kmp-lsp; jdtls import |
| Full syntax index, cold, 4 cores (medium: spring-framework, 9.3k files) | ≤ 2.5 s | 1.42 s cold, 1.04 s warm (kafka 1.76 s / 1.59 s) | kmp-lsp; jdtls import |
| Parse and declaration extract per file | p95 ≤ 5 ms, max ≤ 200 ms | p95 ≤ 3.4 ms everywhere; max 138 ms (kafka's 1.6 MB test file) | — |
| Unchanged-file check: warm re-hash of the large repository | ≤ 0.2 s | about 0.1 s (blake3 with the walk) | — |
| Peak RSS of the syntax tier (large) | ≤ 250 MB | 153 MB with 1.2 M declarations and imports | jdtls behind proxy (2–3 GB per worktree) |
| Marginal extra worktree at a nearby commit (medium) | < 50 MB RSS and < 2 s (*proposed*) | spring-framework, 150 commits apart: blob-id diff 20–30 ms, re-hash 0.05–0.08 s warm, about 270 of 9.3k files to re-parse | jdtls behind proxy (one JVM and import per worktree) |
| `initialize` against a warm daemon | < 200 ms (*v3*) | — | — |
| `jfl def` / `jfl hover`, CLI end to end, warm (medium) | p50 < 20 ms, p95 < 50 ms (*proposed*); internal deadline 300 ms (*v3*) | — | jdtls behind proxy |
| `jfl refs`, warm, a widely used method (medium) | p50 < 100 ms, p95 < 300 ms (*proposed*); deadline 2 s (*v3*) | `rg` floor: 0.04 s warm on spring-framework, 0.18 s on elasticsearch | `rg` floor; jdtls behind proxy |
| `jfl sym` prefix query, warm (large) | p95 < 50 ms (*proposed*) | — | jdtls behind proxy |
| Time to first useful answer on a fresh worktree (medium) | < 2 s (*proposed*; syntax answers that say how complete they are) | — | jdtls behind proxy |
| Warm reopen of an indexed worktree (large) | < 1 s (*proposed*) | walk and hash alone: 0.1 s warm, about 1.5 s cold | — |
| Freshness lag: a disk edit seen by the next request | the request's own stat (§4.3); p95 < 100 ms to a published generation (*proposed*) | — | — |
| Single-file semantic diagnostics (E3) | p95 < 3 s (*v3*, §9.3) | — | ECJ arm |

For scale: a full syntax index costs about 40 times a warm `rg` scan on elasticsearch and about
25 times on spring-framework, so the index pays for itself only when it is built once and shared
across worktrees and agents (§5.1), and kept fresh by re-hashing, not re-parsing.

**From M1 on the table is a blocking gate**: a tracked metric that regresses by more than 10 %
against the history of the same runner class fails the `bench-gate` job of `bench.yml` (medians
of several runs, reproduced once; instruction counts where the suite allows), and each milestone
gate checks the budgets on the recorded reference machine (ver §10). A local or sandbox run
reports and never gates (fnd §9).

---

## 2. The agent contract

### 2.1 The result envelope

Every navigation query returns one envelope (ADR 0014):

```jsonc
{
  "query": { "op": "refs", "uri": "file:///repo/.claude/worktrees/a/src/Billing.java", "line": 41, "col": 17 },
  "view": { "worktree": "/repo/.claude/worktrees/a", "head": "3f2c…", "generation": 118 },
  "target": { "symbol": "com.acme.Billing#charge(Money)", "status": "exact" },
  "exact":    [ { "uri": "...", "range": [41, 17, 41, 23] } ],
  "possible": [ { "uri": "...", "range": [ … ], "why": "receiver type unknown (var in lambda)" } ],
  "complete": false,
  "incomplete_because": ["module :payments not indexed", "generated sources missing in :api"],
  "freshness": { "synced_at": "…", "pending_events": 0 }
}
```

- **`exact`**: bound to the target with evidence (the resolver or the compiler).
- **`possible`**: a **sound superset** of the remaining candidates: syntax-aware matches by
  simple name and arity in every file that can see the declaring module, excluding comments and
  strings, each with the reason it could not be proved. A method reference (`Foo::bar`) matches
  by name whatever the arity.
- **`complete`**: `true` only when every module that can see the target's module is indexed and
  fresh at the pinned generation (§4.5). Then `exact ∪ possible` contains every reference. Before
  the project model exists (M3), "module" means the whole worktree's source set, and `complete`
  requires every source file under it indexed and fresh and no generated-source root missing.
- **`incomplete_because`**: a non-empty list whenever `complete` is `false`. An answer with no
  hits and no completeness statement is never returned.
- **`view`** names the worktree, its `HEAD` and the generation the answer was computed at.

The envelope is a `jfl-domain` type; its JSON form is a `jfl-protocol` golden (§6.6).

### 2.2 Per operation

| Operation | Precision rule | What fills the gap |
|---|---|---|
| definition | `exact` when proved | **Bounded ambiguity**: when the owner type is proved but the overload is not, all overloads of that name in that type. Never a same-name match from another type |
| references | `exact` plus the `possible` superset | Recall matters most: agents use references for impact analysis before changing a signature |
| hover | Signature, Javadoc, declaring type and module, and a status line: `exact`, `ambiguous (3 overloads)` or `unresolved: <reason>`; for a declaration also the reference summary `12 exact, 3 possible, complete` | Replaces a file read and gives the LSP carrier its only completeness channel |
| documentSymbol / workspaceSymbol | Declarations from the syntax index; candidates are fine because this is search | n/a |
| implementation | Proved subtype edges in `exact`; same-name types that extend an unresolved supertype in `possible` | n/a |
| call hierarchy | As references, from call sites; virtual-dispatch targets listed and marked | n/a |
| diagnostics (push) | Syntax always. Semantic only from `javac` with a known classpath (§3), never from the resolver | With no classpath: no semantic diagnostics, and hover and `status` say why |

### 2.3 Positions and encodings

- `jfl-domain` stores byte offsets into the file's UTF-8 bytes plus a line index. Conversions to
  and from (line, UTF-16 code unit), (line, UTF-8 byte) and (line, Unicode scalar) are pure
  functions in `jfl-domain`, property-tested for round trips, surrogate pairs, CRLF and a final
  line without a newline.
- The LSP carrier negotiates `positionEncoding` (LSP 3.17): `utf-16` unless the client offers
  `utf-8`. The CLI takes and prints `file:line:col` with 1-based lines and 1-based UTF-8 byte
  columns, matching `rg --column`; `--json` ranges are 0-based (line, UTF-16 column) like LSP, and
  the JSON says so in a `position_encoding` field.
- A position past the end of a line or file is a refusal with a typed error, never a clamp that
  silently answers for another symbol.

### 2.4 The CLI carrier

`jfl` in the plugin's `bin/`, so the Bash tool calls it bare (ADR 0013):

```
jfl def   <file>:<line>:<col> [--json]
jfl refs  <file>:<line>:<col> [--json] [--exact-only]
jfl hover <file>:<line>:<col> [--json]
jfl sym   <query> [--kind class|method|field] [--json]
jfl impls <file>:<line>:<col> [--json]
jfl calls <file>:<line>:<col> [--incoming|--outgoing] [--json]
jfl diag  <file> [--json]        # waits for the javac worker, with a deadline
jfl status [--json]              # coverage, environment, pending events, why incomplete
jfl sync  [<path>]               # freshness barrier
jfl daemon [--state-dir <dir>]   # the daemon itself (normally spawned on demand)
jfl lsp                          # the stdio language server (§2.5)
```

- Output is compact by default: grouped by file, one line per hit, `exact` first, capped with a
  count; `--json` prints the full envelope. The JSON shape is versioned (`schema_version`) and
  golden-tested; fields are only ever added (§6.6).
- Exit codes: `0` answered (whatever `complete` says), `1` no such symbol at the position, `2`
  usage error, `3` the daemon could not be reached or started, `4` deadline passed with no answer.
- A plugin skill tells the agent when to prefer `jfl refs --json` over grep: before changing a
  public signature, before deleting a member, and whenever completeness matters (IDX-14).

### 2.5 The LSP carrier

Secondary and lossy; it maps the envelope onto plain LSP 3.17:

- `textDocument/references` returns `exact` first, then `possible`, ordered so a truncated read
  still sees the proven ones. The `referencesMode` setting (`exact` | `superset`) has a default
  chosen from E0 (DEC-06).
- `textDocument/hover` carries the status line, the only completeness channel LSP has.
- `textDocument/definition` returns one location, or the bounded overload set.
- `textDocument/documentSymbol`, `workspace/symbol`, `textDocument/implementation`, and the call
  hierarchy (`prepareCallHierarchy`, `incomingCalls`, `outgoingCalls`, M5).
- `textDocument/publishDiagnostics` carries `version` (§3.2).
- Base protocol: `Content-Length` framing with a maximum frame size (default 16 MiB, refused
  before allocation), UTF-8 JSON, `$/cancelRequest` honoured (`RequestCancelled`), `$/progress`
  only when the client offers a token, `initialize` answered in under 200 ms against a warm
  daemon and never blocked on indexing, `shutdown` then `exit` (exit code 0; 1 without
  `shutdown`), unknown requests `MethodNotFound`, unknown notifications ignored.
- Document sync: `didOpen`/`didChange`/`didClose` maintain overlays (§4.4); disk wins over an
  overlay older than the disk mtime seen.

### 2.6 Plugins and distribution

- Claude Code's `.lsp.json` is strict: an unknown key fails validation and the plugin does not
  load. Only documented keys are used:

```json
{
  "jfl": {
    "command": "jfl",
    "args": ["lsp"],
    "extensionToLanguage": { ".java": "java" },
    "startupTimeout": 5000,
    "requestTimeout": 10000,
    "diagnostics": true
  }
}
```

- Two plugin variants (claude.ai and Cowork refuse a plugin with a top-level `bin/`): a **CLI
  plugin** with `bin/jfl` and the skill, for the terminal and IDE; an **LSP-only plugin**. For
  cloud sessions (where Claude Code starts no plugin language server) the binary is installed by
  the environment's setup script and only the skill ships (IDX-14, LSP-05).
- `AGENTS.md` (IDX-14) is the guide for agents that *use* `jfl`; `CLAUDE.md` is for agents that
  build it.
- Releases ship archives for the three targets of §1.6 with checksums, an SBOM and provenance
  (REL-06).

### 2.7 Deadlines and partial answers

Internal deadlines sit well below the client's `requestTimeout`: 300 ms for definition and hover,
2 s for references and symbols. At the deadline the daemon returns what it has, with
`complete=false` and `incomplete_because: ["deadline"]`, never a hang. The server may start
mid-task on the first `.java` touch, so index loading continues in the background after
`initialize`, and early answers are partial and say so.

---

## 3. Semantic diagnostics

### 3.1 javac workers

One long-lived JVM per `EnvironmentKey` (§5.4), shared by every worktree with that environment.
It runs `javac` through the `JavacTask` API with `-proc:none` by default and `-implicit:none`,
compiling the edited file with the worktree's source roots as `-sourcepath` and the module's
classpath. The worker (`java/javac-worker/`) is **forked from the compiler core of
[georgewfraser/java-language-server](https://github.com/georgewfraser/java-language-server)**
(MIT): a long-lived `JavacTask`, file-level incremental recompiles and erasure of irrelevant
method bodies (ADR 0027). It loads **scip-java's javac plugin** (Apache-2.0), so a compiled file
can also yield compiler-exact occurrences for the resolver (§5.6). It speaks length-prefixed JSON
over stdin and stdout; `jfl-adapter-javac` owns the pool and starts workers through `jfl-process`
(§6.5). How the worker is built and shipped (source-file launch or a prebuilt jar) is decided in
SEM-04's ADR.

### 3.2 Publication and staleness

An edit (a `didChange`, or a disk change seen by the freshness stat) triggers a compile. Results
are published as `publishDiagnostics` tagged with the document version they were computed for.
A result for an older version than the newest published or pending one is dropped; a result is
never published over a newer one. The request path never waits on the JVM: diagnostics are push,
not request. `jfl diag` waits for the current version's result with a deadline. Modelled in
`formal/diagnostics.qnt` (§7.3).

### 3.3 Annotation processors

With `-proc:none`, Lombok and MapStruct produce false errors. Processors are detected from the
project model (§5.7). When generated sources exist on disk, their roots join the source path.
Otherwise errors on symbols those processors generate (unknown members of `@Data` and `@Builder`
types and the like) are suppressed and marked in hover. A false compile error is never shown as
authoritative.

### 3.4 Lifecycle and budgets

Workers spawn lazily on the first edit in a known environment, are evicted after an idle period,
are capped in number (LRU), and restart on crash with a backoff. Budgets come from E3 (§9.2):
p50 and p95 single-file latency, worker RSS, and the number of distinct `EnvironmentKey`s 10
real worktrees produce (likely 1 to 3). Whether semantic diagnostics ship in v1 or fall back to
"on save, best effort" is DEC-05. Without a known classpath: syntax diagnostics only, and
`status` says why.

---

## 4. Worktrees, views and freshness

### 4.1 Binding per URI

Each request's file resolves to the innermost enclosing worktree root (the directory holding a
`.git` file or directory, confirmed once with the repository probe and cached per directory
prefix). One connection never implies one view: a subagent's request may name another worktree's
file. A **view** is the daemon's state for one worktree root (ADR 0017).

### 4.2 Nested worktrees

Agent worktrees land inside the main checkout (`.claude/worktrees/<name>/`). When scanning a
worktree, any directory holding a `.git` file (a linked worktree) or a `.git` directory (a nested
repository) is skipped, and so is `.claude/worktrees/` explicitly; otherwise `workspace/symbol`
returns one copy per agent. A request inside a nested worktree binds to that worktree, never to
the parent.

### 4.3 Freshness per request

Before answering, the daemon stats the request's file plus the worktree's `HEAD` and index
(`$GIT_DIR/HEAD` and `$GIT_DIR/index`; for a linked worktree under the common directory's
`worktrees/<name>/`). A change triggers a targeted reconcile: `git diff --name-only` against the
last-seen tree plus `git status -z`, then re-summarise what changed. Filesystem watcher events are
hints that start the same reconcile early; the stat catches checkout, rebase, reset, stash and
commit without a tree walk. `jfl sync` is the explicit barrier: it returns once every pending
change is in a published generation.

### 4.4 Shell edits and overlays

Agents edit through tools that write to disk, so disk wins over any stale buffer. An open-buffer
overlay (from `didOpen`/`didChange`) applies only while its content is newer than the disk mtime
seen when it arrived; a later disk write replaces it. Overlays belong to the connection that
opened them and die with it (§5.2).

### 4.5 Generations and the coherence contract

A view's index advances in **generations**. A reconcile builds generation N+1 beside N and
publishes it atomically once every summary it names is stored; until then requests keep reading
N. A request **pins** one generation at its start and computes its whole answer from it: no
answer mixes two generations. Generations of a view increase strictly. "Latest disk state" is
best effort unless `sync` ran, except that a change the request's own freshness stat observed is
always in the pinned generation (§4.3). Modelled in `formal/views.qnt` (§7.1).

### 4.6 Repository identity

A repository is identified by its Git common directory (`git rev-parse --git-common-dir`,
canonicalised); its worktrees share it. Caches are keyed by content (§5.4), not by path, so
worktrees at different commits share every unchanged file's summary.

---

## 5. Runtime architecture

### 5.1 Processes

```
Claude (LSP tool) ──stdio──▶ jfl lsp  ─┐
Claude (Bash)     ─────────▶ jfl refs ─┴─unix socket─▶ jfl daemon
                                                     ├─ view registry (per worktree root)
                                                     ├─ syntax index (tree-sitter, per file)
                                                     ├─ resolver (choice from E2)
                                                     ├─ JAR/JDK API index (shared, by checksum)
                                                     ├─ project model (Gradle, opt-in, cached) ──▶ gradle-model JVM
                                                     └─ javac workers (per EnvironmentKey) ──▶ javac-worker JVMs
```

The daemon is where the expensive shared things live (JAR and JDK indexes, the Gradle model,
`javac` workers), and it lets several agents share one copy. `jfl lsp` and the CLI subcommands
are thin proxies: they own no index (ADR 0016). This topology is essential, not an optimisation:
Claude Code starts one language-server process per subagent
([claude-code#64536](https://github.com/anthropics/claude-code/issues/64536)), so anything that
owns an index per process pays for it once per agent. Biome's `lsp-proxy` and Plumb's shared
daemon are the closest published shapes (§16).

### 5.2 Daemon lifecycle and the single owner

- **One daemon per state directory** (default: one per user; `--state-dir` overrides). It holds
  an exclusive lock on `<state-dir>/daemon.lock` for its whole life; a second daemon on the same
  state directory exits with a clear error.
- **Spawn on demand.** A proxy that finds no daemon takes a short spawn lock, starts
  `jfl daemon` detached (its own session) through `jfl-process`, and waits for readiness with a
  bounded budget through sccache's notify-socket handshake; two proxies racing start one daemon.
  The lock is std `File::try_lock`; a pid file is never trusted (ADR 0021).
- **Reconnect.** A proxy that loses its daemon (crash, upgrade, idle exit) reconnects, spawning
  if needed, and re-sends its open documents; a request in flight is answered with an error the
  client can retry, never silently dropped.
- **No ghost buffers.** Overlays and per-connection state are keyed by connection; a closed or
  dead connection's state is dropped, and a restarted daemon starts with none.
- **Shutdown.** SIGTERM or an idle period with no connection: stop accepting, cancel running
  queries, stop workers, remove the socket, release the lock, exit 0.
- Modelled in `formal/daemon.qnt` (§7.2).

### 5.3 IPC and socket security

- A Unix domain socket in the state directory; never TCP. **The `0700` state directory is the
  access control.** It is created with `DirBuilderExt::mode(0o700)`; if it already exists, its
  owner must be the daemon's uid and its mode exactly `0700`, or the daemon refuses to start and
  names the path. The socket is bound and the lock file created inside it, then both are set to
  `0600`. That chmod comes after the bind (std and Tokio's `UnixListener::bind` take no mode, and
  `umask` is process-wide and racy in a threaded daemon), but the window is harmless because no
  other user can traverse the directory; macOS ignores socket-file permissions on `connect`
  anyway. Each connection's peer uid (`SO_PEERCRED` on Linux, `getpeereid` on macOS) must equal
  the daemon's: the backstop if the directory check is ever wrong.
- Socket paths are limited to about 104 bytes on macOS and 108 on Linux, and a state directory
  under a long `$XDG_STATE_HOME` or a sandbox's long `$TMPDIR` can exceed that. **Proposed
  (FND-08, for IDX-12), the policy `jfl-jdtls` built and ADR 0028's amendment records:** the
  socket is `<state>/sock` when that path fits; otherwise it moves to a short private directory,
  `/tmp/jfl-<euid>/<key>.sock` (a fixed short base, not `$TMPDIR`), keyed on a digest of the
  canonical state directory so two state directories never share a socket. That directory is
  held to the state directory's rules: created `0700`, and an existing one refused unless it is a
  real directory (not a symlink) of mode `0700` owned by the daemon's uid; the peer-uid check
  applies as above. A path that does not fit even there is refused with the limit named, never
  truncated. The lock stays in the state directory.
- Frames are JSON-RPC 2.0 with the LSP base protocol's `Content-Length` framing, so the CLI, the
  LSP proxy and the daemon share one frame decoder (`jfl-protocol`'s, which checks the length
  before allocating), one message type (`lsp_server::Message`) and one trace format (ADR 0021);
  the adapters feed bytes to that decoder rather than use `lsp-server`'s own reader. `jfl-protocol` defines the methods, payloads and limits; the declared length
  is checked against the maximum size before allocation; a version handshake opens every
  connection (§6.6). The daemon side is Tokio; the CLI client is std only, so it starts fast.

### 5.4 Cache keys

| Key | Derived from | Shares |
|---|---|---|
| `SyntaxKey` | file bytes digest, grammar version, summariser version | a file's syntax summary across every worktree and commit |
| `EnvironmentKey` | the ordered classpath and module path (each entry by content digest), JDK identity (vendor, version, `release` file digest), `--release`, semantic compiler flags, annotation-processor path and options | `javac` workers, library indexes, semantic results |
| `SemanticKey` | the file's `SyntaxKey`, the `EnvironmentKey`, and the digest of every declaration summary its resolution read | a file's resolution results, only within one `EnvironmentKey` |

### 5.5 Storage tier

Infrastructure follows measurements. E1 (§9.2) measures a cold full-worktree parse and summarise
of the target repository; DEC-03 picks the tier:

| E1 result | Storage design |
|---|---|
| under ~3 s | per-view in-memory index, rebuilt on view open from a flat on-disk summary cache keyed by `SyntaxKey`; no snapshots or GC beyond an LRU size cap |
| ~3 to 30 s | add a shared baseline manifest per commit plus per-view deltas |
| over 30 s | full generational store with tombstones and reachability GC |

**E1 outcome (spike, measured 2026-11-16, [`bench-corpus.md`](bench-corpus.md)):** small and
medium repositories rebuild in memory at start (spring-framework 1.4 s cold on 4 cores, ≤ 2 s
everywhere at that size), so the first row fits them. A large repository does not (elasticsearch
8.7 s cold): above about 10–15k files or 50 MB of Java, per-file summaries are persisted, keyed by
blake3 with Git blob ids as a free pre-filter, so a restart costs a walk and a re-hash (0.1 s
warm, about 1.5 s cold) plus re-parsing only changed files. That is the "persisted per-file
summary" store of the first two rows, not the third. Re-hashing is the source of truth: blob ids
describe the index, not a dirty working tree. DEC-03 records the tier; EVAL-05 re-runs E1 on CI
hardware and the target.

The default, until DEC-03 says otherwise, is the first row for small and medium repositories and
persisted per-file summaries for large ones. Whatever the tier, the store uses the
same crates (ADR 0025): `blake3` content keys, `postcard` summary blobs in a content-addressed
directory of our own, `fst` for name lookup and, from the second row, `rusqlite` (WAL) for
manifests and postings. The store is a cache: deleting the state directory loses time, never
correctness (§5.10).

### 5.6 Resolver

Name and type resolution for `exact` answers. Candidates (E2, §9.2): a tree-sitter resolver of
our own, the `ktcore` resolver, the `kmp-lsp` resolver at a pinned commit, and a
**javac-attributed** arm (scip-java's plugin in the warm worker of §3.1, ADR 0027), compared on
200+ golden cases, on the real-corpus golden cases (BENCH-05) and on the target repository
against a SCIP index, by precision, **coverage by construct** (static call, call on a field or
parameter, chain, `var`, lambda, stream, method reference, Lombok member), latency and cost.
Stack Graphs is dropped: [github/stack-graphs](https://github.com/github/stack-graphs) was
archived in September 2025. The likely outcome is a hybrid: javac for `exact` where the classpath
is known and the file compiled, the tree-sitter index for `possible` and for files with no known
classpath. DEC-04 records the choice; a resolver of our own is a pure crate `jfl-resolve`
(SEM-01). Expected weak spots: inferred receivers and generated members, which fall to `possible`
with their reason.

### 5.7 Project model (Gradle)

Opt-in per repository, because it runs the project's build code: `jfl` never runs Gradle until
the repository is allowed (a `jfl` config entry naming the repository's common directory, or a
flag on `jfl sync --project-model`). The model (modules, source sets, dependencies, classpaths,
processor path, JDK, `--release`) is computed by `jfl-adapter-gradle` through `jfl-process`,
cached per **build-file fingerprint** (digest of `settings.gradle*`, every `build.gradle*`,
`gradle.properties`, the wrapper properties and version catalogs) and shared across worktrees
with the same fingerprint. The mechanism wraps
[microsoft/build-server-for-gradle](https://github.com/microsoft/build-server-for-gradle) (MIT,
ADR 0026): a small JVM helper (`java/gradle-model/`) runs its `GetSourceSetsAction`, whose
`GradleSourceSet` already carries source and generated-source roots, the compile classpath, module
dependencies, the Java home and version and `compilerArgs`, and prints JSON. Without the model:
syntax-only answers, the whole worktree as one module, and `status` says so.

### 5.8 Library and JDK index

Class files from JARs and from the JDK (`ct.sym` for `--release`, or `jmods`) are summarised into
API summaries (types, members, signatures, generics, annotations) by `jfl-adapter-classfile`,
shared by content checksum across every view, multi-release aware. The parser is `cafebabe`
(0BSD, forbid-unsafe) over `zip`; the generic-signature parser and multi-release selection are our
own; `$JAVA_HOME/lib/ct.sym` gives any `--release` API without a JVM (ADR 0023). JARs are
untrusted input (§11).

### 5.9 Resource limits

Every queue and cache is bounded: frame size, connections, in-flight requests per connection,
files per reconcile batch, summary cache size (LRU), library index size, javac workers (count and
idle eviction), Gradle runs (one at a time per fingerprint). A limit reached is a counted refusal
or backpressure with a reason in `status`, never an unbounded buffer. Exact numbers are set from
measurements (REL-02).

### 5.10 No event sourcing

The index is a rebuildable cache, not business history (ADR 0008). There is no journal, no event
store and no projections. Domain events are plain values passed in memory; persisted state
(summary cache, library index, project-model cache) is versioned by format and discarded, not
migrated, when its version changes.

---

## 6. Workspace and dependency law

### 6.1 Crate map

| Crate | Role | Holds |
|---|---|---|
| `jfl-domain` | pure | paths and URIs, positions and encodings (§2.3), symbols, file summaries, cache keys (§5.4), the envelope and the completeness computation (§2.1), generation and staleness policies |
| `jfl-protocol` | pure | the daemon wire protocol and the `--json` schema: versioned messages, handshake, frame limits (§6.6) |
| `jfl-application` | application | use cases and queries (def, refs, hover, sym, impls, calls, status, sync), the view registry and generations (§4), the diagnostics coordinator (§3.2), the port traits (§6.4) |
| `jfl-process` | support | the only child-process runner: argv only, cleared environment plus an allowlist, deadline, own process group, bounded output (§6.5) |
| `jfl-adapter-fs` | adapter | `SourceTree`: scanning with nested-worktree exclusion (`ignore`), reading, stat, watcher hints (`notify`) |
| `jfl-adapter-git` | adapter | `RepoProbe`: worktree roots, common directory, `HEAD` and index stat, changed files (the `git` CLI, ADR 0024) |
| `jfl-adapter-java` | adapter | `SyntaxIndexer`: tree-sitter Java parsing (ADR 0022) into file summaries and syntax diagnostics |
| `jfl-adapter-classfile` | adapter | `LibraryIndexer`: class files, JARs, `ct.sym` and `jmods` into API summaries (ADR 0023) |
| `jfl-adapter-store` | adapter | `SummaryStore`, `LibraryStore`, `ProjectModelCache`: the on-disk caches at the DEC-03 tier (ADR 0025) |
| `jfl-adapter-ipc` | adapter | the Unix socket endpoint (server and client), the state-directory lock, peer checks, framing (ADR 0021) |
| `jfl-adapter-lsp` | adapter | LSP 3.17 ↔ `jfl-protocol` translation for the `jfl lsp` proxy (`lsp-server`, `gen-lsp-types`, ADR 0020) |
| `jfl-adapter-javac` | adapter | `Compiler`: the `javac` worker pool per `EnvironmentKey` (§3), workers from `java/javac-worker/` |
| `jfl-adapter-gradle` | adapter | `ProjectModelSource`: the opt-in Gradle model (§5.7) through the `java/gradle-model/` helper |
| `jfl-composition` | root | wiring for the daemon and the proxies, runtime and task supervision, the daemon launcher |
| `jfl-cli` | root | the `jfl` binary: argument parsing, output formatting, exit codes; delegates to composition |
| `jfl-testkit` | test only | fakes for every port, the contract suites, fixture builders, the `poll` helper |
| `jfl-conformance` | test only | the Quint ITF replay harness and the drivers (§7) |
| `jfl-architecture` (`tools/jfl-architecture`) | test only | the architecture test over `cargo_metadata`, the source scan and the CI lockstep (ADR 0030; there is no `xtask`) |
| `jfl-jdtls` (`tools/jfl-jdtls`) | tool only, independent | phase 0: jdtls behind our multiplexing proxy, the kill-criterion baseline and the fallback product (ADR 0028) |
| `jfl-bench` (`tools/jfl-bench`) | tool only, independent | the benchmark harness over the pinned corpus and the comparison arms (ADR 0031) |
| `jfl-oracle` (`tools/jfl-oracle`) | tool only, independent | SCIP reader, differential and completeness oracles (ver §7); imports no production crate |
| `jfl-eval` (`tools/jfl-eval`) | tool only, independent | the agent-evaluation harness and phase 0 measurements (§9) |
| `jfl-fuzz` (`fuzz/`) | fuzz only | fuzz targets on the pinned nightly, its own workspace (REL-03) |

`jfl-resolve` (pure) joins the map if DEC-04 picks our own resolver (SEM-01). Two JVM helpers
are part of the repository but not of the Cargo workspace: `java/javac-worker/` (§3.1, forked)
and `java/gradle-model/` (§5.7, wrapping the build server). Both are started only through
`jfl-process` by their adapter, so the authority table (§6.5) is unchanged. Pure crates are
`#![no_std]` + `alloc` (fnd §2.3).

### 6.2 Why this cut

The cut starts from one crate per technology behind a port, with these changes, each for a
reason:

1. **`jfl-process` (support role).** Three adapters spawn children (git, javac, Gradle). Rather
   than give each process authority, one small crate owns `std::process` and enforces argv-only,
   a cleared environment, deadlines and process groups once; the adapters use its API. It depends
   on no workspace crate, so it creates no path between adapters.
2. **`jfl-adapter-classfile` split from `jfl-adapter-java`.** Class files and JARs are a
   different format, a different fuzz surface and a shared-by-checksum lifecycle; Java source
   parsing (tree-sitter, C code) stays alone in `jfl-adapter-java`.
3. **`jfl-adapter-lsp` depends only on `jfl-protocol` and `jfl-domain`.** The LSP translation
   runs in the proxy and never touches the engine, so the daemon is carrier-neutral and the CLI
   and LSP carriers cannot drift apart in what they compute (ADR 0013).
4. **Tool crates are independent oracles.** `jfl-oracle` and `jfl-eval` judge the product from
   outside (its binary and its JSON), so they import no production crate.
5. **The resolver is not pre-cut.** Its crate depends on E2; a pure crate is the default.

### 6.3 Dependency law

Arrows point from dependent to dependency; this table is the complete list of legal normal edges
between workspace crates (`[dependencies.normal]` in `architecture.toml`, ADR 0010).

| Crate | May depend on |
|---|---|
| `jfl-domain` | — |
| `jfl-protocol` | `jfl-domain` |
| `jfl-application` | `jfl-domain` |
| `jfl-process` | — |
| `jfl-adapter-fs`, `jfl-adapter-java`, `jfl-adapter-classfile`, `jfl-adapter-store` | `jfl-domain`, `jfl-application` |
| `jfl-adapter-git`, `jfl-adapter-javac`, `jfl-adapter-gradle` | `jfl-domain`, `jfl-application`, `jfl-process` |
| `jfl-adapter-ipc` | `jfl-domain`, `jfl-protocol`, `jfl-application` |
| `jfl-adapter-lsp` | `jfl-domain`, `jfl-protocol` |
| `jfl-composition` | every production crate except `jfl-cli` |
| `jfl-cli` | `jfl-composition` |
| `jfl-testkit` | `jfl-domain`, `jfl-protocol`, `jfl-application` |
| `jfl-conformance` | `jfl-testkit`, the pure and application crates, `jfl-adapter-ipc`, `jfl-composition` |
| `jfl-architecture` | — (reads `cargo metadata`; no workspace crate) |
| `jfl-oracle`, `jfl-eval`, `jfl-jdtls`, `jfl-bench` | — (no workspace crate) |

Rules: acyclic; explicit `members`; adapters never name a sibling adapter, composition or the CLI;
production crates never reach a test-only or tool-only crate; a dev edge targets a test-only crate
or a crate already in the dependent's normal list; edges are evaluated for each target of §1.6.
Enforced by the architecture test and `cargo-deny` bans (`just check-only architecture deny`;
fnd §2, §3).

### 6.4 Ports

Owned by `jfl-application`, native `async fn` traits used through generics; each has a testkit
fake and a contract suite run against the fake and the real adapter (ver §5).

| Port | Real adapter | What the contract pins |
|---|---|---|
| `SourceTree` | `jfl-adapter-fs` | listing honours exclusions (nested worktrees, `.claude/worktrees/`), stat reflects writes, reads are bounded |
| `RepoProbe` | `jfl-adapter-git` | innermost root, common directory, `HEAD`/index change detection, changed files after checkout, stash and reset |
| `SyntaxIndexer` | `jfl-adapter-java` | the golden corpus's summaries, syntax errors located, no panic on any input |
| `SummaryStore` | `jfl-adapter-store` | get after put by key, LRU cap honoured, a torn write never returns a corrupt summary |
| `LibraryIndexer`, `LibraryStore` | `jfl-adapter-classfile`, `jfl-adapter-store` | API summaries of the fixture JARs, multi-release selection, malformed input refused |
| `ProjectModelSource` | `jfl-adapter-gradle` | the fixture builds' models, fingerprint stability, refusal without opt-in |
| `Compiler` | `jfl-adapter-javac` | diagnostics of the fixture files, cancellation, crash and restart, version tags |
| `Clock` | composition (system clock) | monotonic deadlines |

Inbound: the daemon's request handler (in `jfl-adapter-ipc`) calls application queries; the
proxies (`jfl lsp`, CLI) speak `jfl-protocol` through the IPC client.

### 6.5 Authority

| Authority | Granted to | Everything else |
|---|---|---|
| Child processes | `jfl-process` (and through it the git, javac and Gradle adapters, and `jfl-composition`'s daemon launcher) | names no `std::process` (`ARCH-process-authority`) |
| Sockets | `jfl-adapter-ipc` (Unix domain only) | names no `std::net` or `std::os::unix::net`; no TCP anywhere |
| Filesystem | `jfl-adapter-fs`, `-git`, `-store`, `-ipc`, `-classfile`, `-javac`, `-gradle`, `jfl-composition` | pure and application crates name no `std::fs` |
| Clock and environment | `jfl-composition` (provides `Clock`, reads configuration), `jfl-process` (deadlines, the child's allowlisted environment), `jfl-cli` (environment only, for defaults) | adapters take time from the `Clock` port; pure and application crates name none |
| Sleeping | no production crate (waits are deadlines on the `Clock` port or channels) | `jfl-testkit`'s `poll` is the only sleep in the workspace |
| Randomness | none in v0.x | pure crates name none; a future need is a port |

**Tool and test crates** are never reached from production (§6.3) and hold the authority their
job needs, each listed in `architecture.toml`'s `[grants]` (fnd §2.4): `jfl-testkit` (processes
and files for fixture builders; sleep and clock inside `poll` only), `jfl-conformance` (files,
environment), `jfl-architecture` (processes and files: `cargo metadata`, `git`, the scripts it
tests), `jfl-bench` and `jfl-eval` (processes, files, clock, environment), `jfl-jdtls`
(processes, files, Unix sockets, spawns, clock, environment: it is a daemon), `jfl-oracle`
(files). TCP stays denied everywhere.

Enforced by `#![no_std]` and the `nostd` stage on the pure crates, per-crate clippy
`disallowed-*` lists and `cargo-deny` wrappers, kept in step by the architecture test (fnd §2.3,
§2.4, §3; FND-02, FND-07).

### 6.6 Protocol and compatibility

- `jfl-protocol` messages carry a protocol major and minor version; the handshake refuses a
  different major with a clear error ("restart the daemon"); minors are additive. A proxy and a
  daemon from different releases therefore fail loudly or work, never misread each other.
- The `--json` schema and the protocol change only by adding optional fields or new messages;
  a rename, removal or retyped field is a new major version and an ADR.
- Golden JSON fixtures pin every message and the `--json` output (ver §4).

---

## 7. Formal models

Quint models are required (ADR 0007). Each lives in `formal/`, is registered in the `MODELS`
registry, has named scenarios, invariants, a broken-model negative control (an action variant
that must violate an invariant within the bound), committed regression traces, and a conformance
driver that replays traces against the real Rust code and compares observations after every step
(ver §6). The model lands first (FORMAL lane); the driver lands with the code it drives. Drivers
implement `quint-connect`'s `Driver` trait and decode traces with `itf`; only the replay of
committed regression traces is ours (ADR 0007, ADR 0030).

### 7.1 `views.qnt`: views, generations and freshness

- **Stimuli:** disk write, delete, `git checkout`/`stash`/`reset` (changes `HEAD` and many files),
  watcher hint (possibly lost, duplicated or reordered), reconcile step, publish, request start
  (stat, pin), request finish, `sync`.
- **Invariants:** `NoMixedGeneration` (every value a request reads belongs to its pinned
  generation); `GenerationsIncrease`; `AtomicPublish` (a published generation's summaries are all
  stored); `FreshAfterStat` (a change the request's stat observed is in the generation it
  answers from); `CompleteIsHonest` (`complete=true` implies every file of every visible module
  is indexed at the pinned generation and its disk version equals the indexed one at pin time);
  `NoCrossView` (a request is answered only from its own worktree's view).
- **Driver:** `jfl-application`'s view registry over the testkit's scripted source tree, repo
  probe and store (IDX-09). **Negative control:** publishing before the last summary is stored.

### 7.2 `daemon.qnt`: daemon lifecycle

- **Stimuli:** proxy connect, proxy spawn (racing), daemon start, lock acquire/fail, daemon crash,
  idle exit, SIGTERM, proxy reconnect, `didOpen`/`didClose`, connection drop.
- **Invariants:** `AtMostOneOwner` (at most one daemon holds the lock); `ServedOnlyByOwner`;
  `NoGhostBuffers` (every overlay belongs to a live connection of the current daemon);
  `ShutdownReleases` (after stop, no socket and no lock remain). **Liveness** (under fairness):
  a proxy that keeps retrying is eventually served.
- **Driver:** `jfl-composition`'s daemon host in-process, with real Unix sockets and lock files in
  a tempdir; a crash is an abort of the host (IDX-12). **Negative control:** a lock released
  before the socket is removed.

### 7.3 `diagnostics.qnt`: diagnostics staleness

- **Stimuli:** document change (version + 1), compile start, compile result (in any order),
  worker crash and restart, document close.
- **Invariants:** `NeverOlderOverNewer` (the published version per document never decreases and
  is never older than a version already published); `PublishedMatchesVersion` (published
  diagnostics were computed for the version they carry); `ClosedPublishesNothing`.
- **Driver:** `jfl-application`'s diagnostics coordinator with the testkit's scripted compiler,
  which completes jobs in the order the trace says (LSP-04). **Negative control:** publishing
  every result as it arrives.

### 7.4 What is not modelled

Parsing, resolution and the completeness computation are pure functions, covered by unit,
property and oracle tests (ver §7), not by Quint. The javac worker pool's eviction and the Gradle
model's caching are covered by component tests unless a defect shows they need a model.

---

## 8. Verification summary

[`verification.md`](verification.md) owns this; in short: unit, property, golden (protocol JSON,
`--json` output, LSP transcripts, the Java corpus), component, contract, **LSP conformance**
(scripted JSON-RPC transcripts against the built `jfl lsp`), integration, e2e, architecture,
Quint conformance, **differential** (SCIP oracle on the golden corpus and, owner-run, on the
target repository), the **completeness oracle** (an inserted caller appears in `exact ∪
possible` or `complete=false`), the **envelope-honesty mutation suite**, fuzzing, mutation
testing, benchmarks, and the **agent evaluation** rerun per milestone. The **golden Java
fixture corpus** (`fixtures/java/`) holds small projects (single module, multi-module Gradle,
nested worktrees, Lombok, generated sources, multi-release JARs) and per-construct resolution
cases with expected answers. Toy fixtures are not enough for a speed product: correctness tests
also run against the **pinned real corpus** (`doc/bench-corpus.md`), whose golden cases are
expectations keyed by repository, commit, file and position with `scip-java` as the oracle, taken
from the Apache-2.0 repositories only; the corpus code is fetched, never committed (BENCH-05,
ADR 0031).

---

## 9. Phase 0: measure before building

### 9.1 Target repository

The experiments need one real, large Gradle Java repository with a history of agent-sized tasks.
The owner chooses it (DEC-01). If it is private, nothing from it (code, paths, class names, task
text) enters this repository: harnesses and runbooks are committed, the runs happen on the
owner's machine, and only aggregated numbers come back (ADR 0004, ADR 0009).

### 9.2 Experiments

| Id | Experiment | Output | Decision it drives |
|---|---|---|---|
| **E0** | **Agent eval** on [Harbor](https://github.com/harbor-framework/harbor) (ADR 0029). 15 to 20 real tasks from the target repository's history (bug fixes, signature changes, cross-module features, a rename) plus 10 to 20 Gradle instances from [Multi-SWE-bench](https://github.com/multi-swe-bench/multi-swe-bench)'s Java set as an external control. Arms: grep only; the stock `jdtls-lsp` plugin; jdtls behind our proxy (§1.3, EVAL-11); `kmp-lsp` at a pinned commit. 3 runs per task per arm | success, tool calls, tokens, broken-build edits, a histogram of the LSP operations Claude actually calls, its reaction to empty references, diagnostics wait time, whether the server starts on the first `.java` touch | kill criterion (§1.3); operation priority; `referencesMode` default (DEC-06) |
| **E1** | Absolute parse + summarise time for a full worktree (cold and warm file cache), RSS of the in-memory summary index, and the parse-error rate of both Java grammars (ADR 0022), on the bench corpus and the target. **Spike done 2026-11-16** (§1.7, §5.5); EVAL-05 re-runs it in the harness | seconds, files per second, MB, error rate | storage tier (DEC-03) |
| **E2** | **Resolver bake-off** (§5.6) on 200+ golden cases, the real-corpus cases and the target repository against a SCIP index from `scip-java` | precision, coverage by construct, latency, cost | resolver (DEC-04) |
| **E3** | `javac` worker spike, starting from the forked compiler core (ADR 0027): single-file compile latency with `-sourcepath`, per module; ECJ batch (`-proceedOnError`) as a comparison arm; the scip-java plugin's attribution cost | p50, p95, RSS, false errors, `EnvironmentKey` count across 10 worktrees | diagnostics in v1 (DEC-05) |
| **E4** | jdtls at 1, 3 and 10 worktrees, stock and behind our proxy, on the bench corpus and the target | RSS, time to first useful answer, import count | kill criterion |

**Already answered** (2026-11-16): subagents do **not** share the parent's language server;
Claude Code starts one per process subtree with no plugin setting to change it
([claude-code#64536](https://github.com/anthropics/claude-code/issues/64536), closed by the stale
bot without a maintainer reply). E0 re-checks it on the current Claude Code release, but the design
assumes one LSP process per subagent. **Still unverified**, settled by E0: how long Claude Code
waits for diagnostics after an edit; whether the server starts on the first `.java` touch or at
session start; whether `claude -p` (and so Harbor and `claude plugin eval`) starts plugin language
servers at all; whether `${CLAUDE_PROJECT_DIR}` is substituted inside `.lsp.json` arguments.

### 9.3 Go/no-go

Proceed (DEC-02, owner) if (a) E0 shows a gap the design can close (an LSP or CLI arm beats grep on
rows 1 to 3 of §1.2, and jdtls behind our proxy does not already win at 10 worktrees on memory and
latency), (b) E2 shows a path to ≥ 70 % `exact` coverage of call sites with ≥ 99.5 % precision, the
rest covered soundly by `possible`, and (c) E3's single-file p95 is under 3 s (otherwise diagnostics
fall back per DEC-05, which does not by itself stop the project), and (d) the BENCH baseline
(BENCH-03) shows the §1.7 targets are reachable on the corpus.

---

## 10. Milestones

Each milestone ends with an E0-style eval rerun on a subset of tasks (owner-run), so progress is
measured in agent terms, and with the §1.7 budget checked on the bench corpus: from M1 on, a
tracked metric that regresses by more than 10 % fails the gate (ADR 0031).

### 10.1 M1: CLI vertical slice (syntax + envelope)

Daemon with spawn on demand, Unix socket, per-URI views, nested-worktree exclusion, freshness
stat; tree-sitter syntax index at DEC-03's tier; `sym`, `def` (lexical, imports, project types),
`refs` with the `possible` superset and `complete`, `hover` (signature, Javadoc, status),
`status`, `sync`; the plugin with `bin/` and the skill. Tasks IDX-01..IDX-15.
**Gate:** works in a cloud session through the setup script; the envelope on 50 golden cases;
`complete=true` never wrong in a mutation test that adds a caller in each module; 10 concurrent
worktrees without cross-view results; freshness under shell edits; the §1.7 rows M1 touches
(index time, RSS, `def`/`hover`/`refs`/`sym` latency, marginal worktree, freshness lag) within
budget on the corpus, and the >10 % regression gate blocking from here on.

### 10.2 M2: LSP carrier

`initialize` < 200 ms warm; definition, references (`referencesMode`), hover with the status
line, documentSymbol, workspaceSymbol, implementation; UTF-16 positions; cancellation; internal
deadlines; syntax diagnostics; two plugin variants. Tasks LSP-01..LSP-06.
**Gate:** `claude plugin validate --strict` passes; a real Claude Code smoke test with shell edits
across 3 worktrees; outranks `jdtls-lsp` for `.java` when both are installed, or documents that
they cannot coexist; the LSP conformance suite green.

### 10.3 M3: project model and library APIs

The opt-in Gradle model (§5.7), by wrapping microsoft/build-server-for-gradle (ADR 0026), cached
per build fingerprint and shared; `EnvironmentKey`; the JAR and JDK class-file index shared by
checksum, multi-release aware. Tasks PROJ-01..PROJ-05.
**Gate:** classpath divergence isolates environments; no test-to-main leaks (a main source set
never sees test-only types); malformed-JAR fuzzing clean; hover and definition resolve into
library types.

### 10.4 M4: resolver and semantic diagnostics

The resolver chosen by DEC-04, classpath-aware; `javac` workers (forked core, scip-java plugin)
with processor handling; the SCIP baseline import formerly planned as M6, if DEC-04 keeps it
(SEM-08), with `scip-java` as the oracle throughout. Tasks SEM-01..SEM-08.
**Gates:**
- **Precision** ≥ 99.5 % of `exact` answers agree with SCIP on the target repository.
- **Coverage** ≥ the DEC-04 share of call sites resolve `exact`, reported per construct; no
  construct silently drops out.
- **Recall of the envelope:** for every SCIP reference, it is in `exact ∪ possible` when
  `complete=true` (zero misses).
- **Diagnostics:** zero false errors on a clean checkout of the target repository (Lombok and
  MapStruct included); p95 under E3's budget.

### 10.5 M5: call hierarchy and release preview

Incoming and outgoing calls with dispatch marking; resource limits, watcher recovery, crash and
restart soak; fuzz and mutation lanes (the benchmark lane runs from FND-01 on); release workflow;
the final agent eval against all baselines. Tasks REL-01..REL-06.
**Gate:** E0 metrics beat both baselines on the full task set at 10 concurrent worktrees.

### 10.6 M6: folded into M4

The SCIP baseline import is a small step once `scip-java` is the oracle and the scip-java plugin
runs in the javac worker, so it is SEM-08 in M4, kept or dropped by DEC-04 (ADR 0009, as amended).

---

## 11. Security and untrusted input

| Threat | Mitigation | Pinned by |
|---|---|---|
| Another local user talks to the daemon | `0700` state dir, `0600` socket and lock, peer uid check | IDX-12 integration tests |
| A crafted Java file, class file, JAR or frame crashes or exhausts the daemon | bounded parsers, no panics, size and depth limits, fuzz targets | IDX-07, PROJ-03, LSP-01, REL-03 |
| Zip bombs and path tricks in JARs | entry count, ratio and size caps; entries never extracted to disk by name | PROJ-03 |
| Running untrusted build code | the Gradle model is opt-in per repository; never on the request path | PROJ-01 |
| Command injection through file names | argv only through `jfl-process`, never a shell | IDX-04 |
| Leaking source text | logs carry no source text or file contents; no telemetry | fnd §5 review |
| Symlinks escaping a worktree | the scanner does not follow symlinked directories out of the root | IDX-06 |

---

## 12. Observability

`tracing` with fixed `snake_case` event names, data in fields; a healthy daemon logs (almost)
nothing at INFO (fnd §5). `jfl status --json` is the operator and agent view: views, generations,
pending events, coverage (indexed and unindexed modules), environments, workers, limits reached,
and the reasons answers are incomplete. Nothing is ever sent off the machine: no telemetry, no
update check, no crash upload.

---

## 13. Risks

| Risk | Mitigation |
|---|---|
| `possible` lists are long and cost tokens | group by file, cap with counts, `--exact-only`; measure tokens in E0 |
| Agents ignore the skill and keep grepping | E0 measures it; tune the skill text; hover status nudges through LSP |
| `javac` false errors from processors erode trust | `-proc:none` + generated-source roots + suppression; zero-false-error gate |
| `javac` workers eat memory with many environments | cap workers, LRU-evict, share by `EnvironmentKey` |
| Subagents start their own LSP proxies (confirmed: claude-code#64536) | proxies are thin and the daemon is shared: the cost is per process, not per index |
| The server starts mid-task and the first answers are slow | fast `initialize`, partial answers with `complete=false`, a SessionStart hook can warm the daemon |
| `jdtls` is good enough | kill criterion §1.3 against jdtls behind our proxy; `jfl-jdtls` is then the deliverable |
| JetBrains ships a headless, agent-licensed IntelliJ LSP ([IDEA-393669](https://youtrack.jetbrains.com/projects/IDEA/issues/IDEA-393669)) | keep the proxy's backend pluggable (ADR 0028), so it could front that server as it fronts jdtls; recheck at each milestone |
| `kmp-lsp` (MIT, active) converges on the same niche | it is an E0 arm and a bench arm; forking its resolver or JAR cache is the mitigation |
| A reused upstream stalls (tree-sitter-java-orchard, java-language-server and build-server-for-gradle have small teams; `quint-connect` and `gen-lsp-types` are 0.x) | pin exact versions or commits, keep forks' diffs small, name a runner-up in each ADR (§16) |
| A speed regression lands unnoticed | the BENCH lane from FND-01 on; the >10 % gate from M1 (§1.7) |
| Facts about Claude Code drift (operations, `bin/` policy, cloud LSP, `.lsp.json` keys, one server per subagent) | re-check at each milestone gate; smoke test on the current release |
| tree-sitter grammar gaps on new Java syntax | the grammar version is part of `SyntaxKey`; parse errors are diagnostics, never crashes; corpus cases per Java release |
| The target repository is private | harnesses public, runs owner-side, aggregates only (§9.1) |

---

## 14. Open decisions

Each open choice is a task that records an ADR; nothing here is decided by an implementer.

| Decision | Task | Input |
|---|---|---|
| Target Java repository | DEC-01 (owner) | the criteria sheet of DEC-01 |
| Go/no-go and the kill criterion | DEC-02 ⛳ (owner) | E0–E4 |
| Storage tier | DEC-03 | E1 |
| Resolver | DEC-04 | E2 |
| Semantic diagnostics in v1 | DEC-05 | E3 |
| `referencesMode` default and operation priority | DEC-06 | E0 |
| Confirm the Proposed defaults (ADRs 0005–0010, 0013–0031) | DEC-07 (owner, at the FND-05 ⛳) | this plan and the survey evidence |
| The §1.7 speed targets | DEC-02 ⛳ (owner) | the BENCH-03 baseline |
| Shared skills plugin or repo-local skills | DEC-08 (owner) | the plugin-marketplace option of §16 |
| Keep the SCIP baseline import (SEM-08) | DEC-04 | E2 and M4's coverage gap |
| javac worker build and launch | SEM-04 (ADR) | startup cost |

Settled by the reuse survey (Proposed, owner confirms at DEC-07): Git access is the `git` CLI
(ADR 0024); the Gradle model wraps the build server (ADR 0026); M6 is folded into M4.

---

## 15. Design history

Plan v3 replaced the design half of v2 (2026-11-11). The changes and their reasons:

| v2 | v3 | Reason |
|---|---|---|
| Return nothing unless exact | Every answer says how complete it is: `exact`, `possible`, `complete` | an empty reference list is the worst answer an agent can get |
| LSP the only carrier | a JSON CLI and LSP over one engine | the CLI carries the full envelope and works in cloud sessions |
| typeDefinition among the features | hover, document and workspace symbols, implementation, call hierarchy | the operations Claude Code's LSP tool calls |
| semantic diagnostics later | semantic diagnostics in v1 from `javac` workers, asynchronous | Claude Code pushes diagnostics after every edit; type errors are what agents make |
| go/no-go on syntax-artifact reuse | an agent eval plus a resolver bake-off plus an absolute parse time | tree-sitter parsing is cheap; the real costs are elsewhere |
| precision gate alone | precision and coverage gates | precision on a self-chosen subset can be gamed |
| a view per connection | a view per request URI; nested worktrees excluded | agent worktrees live inside the main checkout |
| snapshots, tombstones, GC from the start | only if E1 says so | infrastructure follows measurements |

Kept from v2: no builds or JVM on the request path; filesystem watching as hints, not truth;
the three-key cache split; differential tests against SCIP; a single daemon owner; Gradle as
opt-in trusted code; the golden corpus and fault injection.

v3.1 (BOOT-01) applied the reuse survey (§16, ADR 0019) and the owner's direction to benchmark
early (§1.7, ADR 0031): the jdtls baseline is multiplexed, the Gradle model and the javac worker
are reused, E0 runs on Harbor, Stack Graphs and M6 are gone, and the foundation is maintained
tools instead of a ported `xtask`.

---

## 16. Reuse and dependencies

The owner's rule: don't reinvent the wheel (ADR 0019). A survey on 2026-11-16 read crates.io, the
projects' repositories and their release pages; versions and dates below are from that day and
are re-checked by the task that adds each dependency (a version is never invented). Every task
that adds a subsystem says on its card what reuse it considered.

### 16.1 Build or reuse

| Subsystem | Verdict | ADR |
|:--|:--|:--|
| Shared daemon, thin proxies, per-worktree views, freshness, envelope, storage layout | **build**: nothing published shares indexes across worktrees | 0016, 0017 |
| LSP protocol and types | reuse `lsp-server`, `gen-lsp-types`, `line-index` | 0020 |
| Daemon IPC and single instance | reuse LSP framing, std `File::try_lock`, `rustix`; copy sccache's handshake, Biome's proxy | 0021 |
| Java syntax | reuse `tree-sitter` + `tree-sitter-java-orchard`; own queries | 0022 |
| Class files and the JDK API | reuse `cafebabe` + `zip`; `ct.sym`; own signature parser | 0023 |
| Git | reuse the `git` CLI; stat-only hot path | 0024 |
| Watching and walking | reuse `notify` 8.2 + `notify-debouncer-mini`, `ignore` 0.4; turborepo's cookie file for `jfl sync` | 0019 |
| Storage | reuse `rusqlite`, `fst`, `postcard`, `blake3`; own CAS directory | 0025 |
| Gradle model | **wrap** microsoft/build-server-for-gradle | 0026 |
| javac worker | **fork** java-language-server's compiler core; scip-java plugin | 0027 |
| Resolver | E2 decides; javac-attributed arm added, Stack Graphs dropped | DEC-04 |
| SCIP oracle and baseline | reuse `scip-java`, the `scip` crate | 0009 |
| Baseline and fallback product | **wrap** jdtls behind our multiplexing proxy | 0028 |
| Agent evaluation | **wrap** Harbor; Multi-SWE-bench Java control | 0029 |
| Benchmarks | reuse hyperfine, divan or criterion, github-action-benchmark; a pinned real corpus | 0031 |
| Engineering foundation | reuse just, mise, cargo-deny, `no_std`, adrs, zizmor, pinact, actionlint, gitleaks, diff-cover, cargo-mutants, quint-connect, dist, release-plz | 0030 |

### 16.2 Product crates

| Crate | Version (date) | Licence | Used by | Note |
|:--|:--|:--|:--|:--|
| `lsp-server` | 0.10.0 (2026-07-16) | MIT OR Apache-2.0 | `jfl-adapter-lsp`, `-ipc` | rust-analyzer's; `Connection::memory()` for tests |
| `gen-lsp-types` | 0.11.0 (2026-07-28) | MIT | `jfl-adapter-lsp` | LSP 3.18 metamodel; exact pin |
| `line-index` | 0.1.2 | MIT OR Apache-2.0 | `jfl-domain` or its tests | UTF-8 ↔ UTF-16 |
| `tokio`, `tokio-util` | 1.54, 0.7.20 | MIT | daemon side only | `peer_cred()`, `CancellationToken` |
| `rustix` | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `jfl-adapter-ipc`, daemon start | safe `setsid`, uid and mode checks |
| `tree-sitter`, `tree-sitter-java-orchard` | 0.27.2, 0.5.25 (2026-11-15) | MIT | `jfl-adapter-java` | C via `cc` |
| `cafebabe`, `zip` | 0.9.0, 9.6.1 | 0BSD, MIT | `jfl-adapter-classfile` | `zip` without default features, `deflate` on |
| `notify`, `notify-debouncer-mini`, `ignore` | 8.2.2, 0.7.0, 0.4.34 | CC0-1.0, MIT OR Apache-2.0, Unlicense OR MIT | `jfl-adapter-fs` | `notify` 9 when final |
| `rusqlite` (bundled), `fst`, `postcard`, `blake3` | 0.43.0, 0.4.7, 1.1.3, 1.8.7 | MIT, Unlicense OR MIT, MIT OR Apache-2.0, CC0-1.0 OR Apache-2.0 | `jfl-adapter-store` | tier by DEC-03 |
| `scip` | 0.10.0 | Apache-2.0 | `jfl-oracle`, SEM-08 | pins `protobuf` =3.7.2 exactly |
| `clap`, `serde`, `serde_json`, `schemars`, `camino`, `thiserror`, `anyhow` | 4.6, 1, 1, 1.2, 1.2, 2, 1 | MIT OR Apache-2.0 (schemars MIT) | CLI, protocol | `anyhow` only in binaries and tools |
| `tracing`, `tracing-subscriber`, `tracing-appender` | 0.1.44, 0.3.23, 0.2.5 | MIT | composition | the LSP proxy never logs to stdout |
| `insta`, `expect-test`, `assert_cmd`, `trycmd`, `proptest`, `jsonschema` | 1.49, 1.5, 2.2, 1.2, 1.12, 0.61 | Apache-2.0 or MIT | tests only | `insta` JSON redactions for generations and times |

`deny.toml` therefore allows MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, 0BSD, CC0-1.0,
Unlicense, BSD-3-Clause and Unicode-3.0 (transitive); anything else needs an ADR (ADR 0005).
Runtime-optional JVM pieces (not linked): java-language-server's compiler core (MIT), scip-java's
plugin (Apache-2.0), build-server-for-gradle (MIT), and jdtls (EPL-2.0, a separate process only,
in `jfl-jdtls`).

### 16.3 Rejected, and why

| Option | Why not |
|:--|:--|
| `lsp-types` 0.97, `tower-lsp` 0.20 | stalled (2024) and unmaintained (2023); runner-up `tower-lsp-server` 0.24 when final |
| `async-lsp` | pinned to `lsp-types` 0.95; its design is copied |
| `jsonrpsee`, `tarpc` | heavy, or a second protocol no LSP tool can read |
| `fs2`, `fd-lock`, `fs4`, `daemonize`, `single-instance` | std file locks since Rust 1.89; `fork()` in a threaded process |
| upstream `tree-sitter-java` 0.23.5 | dormant since 2024-12; no Java 25 `import module`; runner-up |
| `java-lang` | too new (1.4k downloads), no incremental reparse |
| `ristretto_classfile`, `noak`, `classfile-parser`, `jimage` | heavy and MSRV 1.99; `unsafe`; incomplete; Windows FFI |
| `gix`, `git2` | linked-worktree gap; C build |
| `bincode`, `bitcode`, `tantivy`, `salsa` | unmaintained (RUSTSEC-2025-0141); no format stability; overkill; experimental persistence |
| Stack Graphs | archived 2025-09 |
| Serena's application | GPL-3.0; ideas only |
| the IntelliJ LSP | proprietary, no headless launcher yet (plan §13) |
| mayfly's `xtask` port, `cargo-make`, `cargo-bylaw` | 16k lines to own; stale; too young (ADR 0030) |

### 16.4 Agent workflow

The `/work`, `/orchestrate` and `/review` skills stay in this repository's `.claude/` for now.
The alternative is one personal marketplace (`pzmi/claude-plugins`) with one `sdlc` plugin
holding the three skills, parameterised by each repository's `CLAUDE.md` and `doc/`, added with
`claude plugin marketplace add … --scope project` and pinned by `ref`/`sha`, with
`claude plugin eval` cases for "never pushes without the gate", "review cites file:line" and
"stops at ⛳". The owner decides at DEC-08.
