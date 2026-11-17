# jfl-jdtls: jdtls behind our multiplexing proxy

`tools/jfl-jdtls` (EVAL-11, ADR 0028) is the kill-criterion baseline of plan §1.3 and an arm of
E0 and E4 (plan §9.2): Eclipse JDT LS run the way a careful user could run it, so the comparison
with jfastlsp is like with like. It is a tool crate and imports no production crate (fnd §2.7). If
DEC-02 stops the project, it is the deliverable.

The stock `jdtls-lsp` plugin is not that baseline: it starts a bare `jdtls` with no `-data`, so
the launcher keys the workspace on the working directory's *basename* and worktrees named alike
collide ([claude-code#67182](https://github.com/anthropics/claude-code/issues/67182)), and every
subagent starts its own jdtls of 2–3 GB
([claude-code#64536](https://github.com/anthropics/claude-code/issues/64536)).

## What it does

```
Claude Code ──stdio── jfl-jdtls lsp ─┐
subagent 1  ──stdio── jfl-jdtls lsp ─┼─ Unix socket ── daemon ── one jdtls per worktree root
subagent 2  ──stdio── jfl-jdtls lsp ─┘   (0600)       (one per state directory, by lock)
```

- **One jdtls per worktree root**, shared by every client in it. The root is the innermost
  directory holding `.git` (a directory or a linked worktree's file) above the proxy's working
  directory, so a nested worktree under `.claude/worktrees/` binds to itself. Binding is per
  connection, not per URI: a client started in the outer checkout talks to the outer jdtls for
  every file. The outer jdtls excludes `**/.claude/worktrees/**` (with jdtls's default
  `java.import.exclusions`) so it does not import the nested worktrees a second time.
- **`-data`** is `<state>/workspaces/<sha1 of the full canonical root path>`: two worktrees named
  `repo` get two workspaces. The library index is shared by every jdtls of the daemon
  (`-Djdt.core.sharedIndexLocation=<state>/shared-index`); the heap is capped (`-Xmx`, default
  2048 MiB); vscode-java's GC settings are used; `--syntax-server` runs jdtls in syntax-server mode.
- **Multiplexing** (`src/mux.rs`): client request ids are remapped to backend ids and back, so
  each client sees only its own answers under its own ids. The first `initialize` is forwarded
  with the worktree as root and the daemon as `processId` (jdtls exits with the daemon); later
  clients get the cached result, and a client is served only after its own `initialize`.
  `initialized` goes once; `shutdown` is answered by the proxy;
  `exit` only detaches the client. `didOpen`/`didClose` are reference-counted per URI, the
  advertised sync kind is rewritten to *full* so the last buffer wins, and versions are rewritten
  to one counter per URI. `$/cancelRequest` is forwarded. Client progress tokens are stripped
  (two clients may pick the same one); backend progress goes to the client that accepted the
  token; backend requests go to the oldest client and are answered by the proxy if it leaves,
  except `workspace/applyEdit`, answered `applied: false` (the proxy cannot tell which client
  caused it; agents write files themselves). Only the JSON-RPC envelope is decoded: payloads the
  proxy does not rewrite pass byte for byte, and a message that does not decode is answered with
  an error rather than left hanging.
- **Last buffer wins.** With one jdtls per worktree, an open document has one text: the last one
  any client sent. Client A can briefly be answered against B's unsaved text, and diagnostics
  follow the last text. Agents write files and send buffers equal to the disk, so for this
  baseline the difference is accepted (ADR 0028).
- **A client's exit never stops a jdtls others use**: its requests are cancelled and their late
  answers dropped, documents only it held are closed. A jdtls with no client for `--idle-secs`
  (default 1800) gets `shutdown` and `exit` and is killed if it outlives 10 s; the daemon exits
  after `--daemon-idle-secs` (default 600) with no jdtls. jdtls runs in its own process group,
  and the whole group (Gradle and other helpers included) is killed when its session ends;
  its stderr goes to `<state>/logs/<key>.log`, capped at 8 MiB.
- **Limits**: frames over 16 MiB are refused before allocation; payloads are skipped, never
  recursed into, and decoded (where a rule needs it) only up to a nesting of 128; queues are bounded (a client that stops reading is dropped, not waited on);
  at most 256 connections; a connection must say hello within 10 s.

## Install and run

```bash
bash scripts/jdtls-fetch.sh                  # the pinned jdtls, checksum-verified
cargo install --locked --path tools/jfl-jdtls  # puts jfl-jdtls on PATH
jfl-jdtls lsp                                # what an LSP client runs (from the worktree)
jfl-jdtls status                             # sessions: root, dataDir, clients, backendPid
jfl-jdtls stop                               # stops every jdtls and the daemon
```

- **jdtls 1.61.0** is pinned with its SHA-256 in `scripts/jdtls-fetch.sh` (and the version in
  `src/launch.rs`; the script's self-test checks they agree). It is the newest release that runs
  on **Java 21**: from 1.62.0 the Eclipse platform bundles require JavaSE-25. Moving the pin
  needs a pinned JDK 25 first. jdtls is EPL-2.0 and is only ever run as a separate process.
- The state directory defaults to `$XDG_STATE_HOME/jfl-jdtls` (else `~/.local/state/jfl-jdtls`);
  `--state-dir` overrides it. It is created `0700`, and an existing one is refused, naming the
  path, unless it is a real directory (not a symlink) of mode `0700` owned by you. The socket and
  locks are `0600`; on Linux a peer of another uid is refused. Logs are in `<state>/logs/`.
- Daemon options (`--jdtls-home`, `--java`, `--heap-mib`, `--syntax-server`,
  `--no-gradle-import`, `--idle-secs`, `--daemon-idle-secs`, `--backend-command`) are given to
  `lsp` and passed on by the proxy that starts the daemon; a running daemon keeps its own. Stop it
  to change them.
- `--backend-command PROG ARGS...` fronts any stdio language server the same way (it gets
  `-data <dir>` appended), which keeps the proxy's backend pluggable (ADR 0028).

### The Claude Code plugin

`tools/jfl-jdtls/plugin/` is the plugin variant for E0 and E4: `.lsp.json` (documented keys only,
plan §2.6) runs `jfl-jdtls lsp` for `.java` files, with jfl-jdtls on PATH. Install it in place of
the stock `jdtls-lsp` plugin for the "jdtls behind our proxy" arm.

## Tests

| Level | Where | What |
|:--|:--|:--|
| unit | `src/*.rs` | framing limits and truncation, the envelope's byte-for-byte forwarding and the id scan (2000 generated messages against the decoder), worktree roots and keys, launch arguments, every mux rule |
| component | `tests/session.rs` | the session over the in-process scripted fake: shared backend with colliding ids, a client dying mid-request, `shutdown`/`exit` of one client, idle eviction, the backend dying, undecodable messages from either side |
| integration | `tests/daemon.rs` (`daemon_or_jvm`) | the built proxy, daemon and fake backend as processes: one backend for two clients, two `-data` for same-named worktrees, a killed client, `0700`/`0600`, refused state directories (`0755`, a symlink, another owner; also run as a non-root user under `setpriv`), `stop` leaves no backend |
| integration, real jdtls | `tests/real_jdtls.rs` (feature `real-jdtls`) | the pinned jdtls on the synthetic `tests/fixtures/single-module` with the Gradle import off: a cross-file definition for two clients from one jdtls, no JVM left after `stop` |
| script | `scripts/test-jdtls-fetch.sh` | checksum refusal, launcher check, cached no-op, the version agreeing with the crate |

The real-jdtls test is outside the gate (it needs the 50 MB download and a JVM). Run it with:

```bash
bash scripts/jdtls-fetch.sh
cargo nextest run -p jfl-jdtls --locked --features real-jdtls --test real_jdtls
```

## Owner runs

- **One jdtls for a session and its subagents** (EVAL-11's owner criterion): with the plugin
  installed, start Claude Code in a worktree of a Java repository, have it start three subagents
  that each use LSP operations on `.java` files, then run `jfl-jdtls status`: one session for the
  worktree, `clients` 4, one `backendPid`. `ps -o pid,args --ppid <daemonPid>` lists one java.
- **E0 and E4** use this plugin and `jfl-jdtls` with the Gradle import on and a warm Gradle (the
  tuned baseline); they run on the owner's machine or `bench.yml`, never in the sandbox.

## Known limits

- `--syntax-server` switches the whole session to syntax-server mode; the card's "syntax server
  until the import finishes" (two jdtls per root, switching over on `ServiceReady`) is not built.
- On macOS the socket's peer uid is not checked (`rustix` has no `getpeereid`); the `0700` state
  directory is the access control there.
- Queues are bounded in messages (1024 per channel), not bytes.
