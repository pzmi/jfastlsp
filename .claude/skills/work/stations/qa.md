# qa

Dispatch: inline

## Inputs

implement.{affectedCrates, filesChanged}, intake.{criteria, taskId, authority, protocolImpact}

## Steps

1. **Is there a surface?** `jfl` subcommands, their output, `--json`, exit codes; `jfl lsp` over stdio; the daemon's start, stop, restart and socket; the plugin and skill files: yes. Pure crates alone, tooling, CI, models or docs: record `{skipped: "no runnable surface: <reason>"}` and pass.
2. **Build** what the surface needs: `cargo build --locked --bin jfl`, with the lane's `CARGO_TARGET_DIR`. Everything runs against a **fresh copy of a fixture Java project** (`fixtures/java/projects/*`) and a fresh state directory (the `--state-dir` flag, never an inline environment variable) **under the session scratchpad**; never the repository itself, never a target Java repository, never the network. Make the copy a git repository with at least one linked worktree under `.claude/worktrees/` when the task touches views or scanning.
3. **Exercise the surface.**
   - Start the daemon through the CLI (spawn on demand) or explicitly, note its PID, and wait for readiness with a bounded loop on `jfl status --json` or the socket (no blind sleep).
   - Golden path: the commands the task touched (`jfl sym`, `def`, `refs`, `hover`, `impls`, `status`, `sync`, `diag`) return what the task promises, with the envelope fields; for `jfl lsp`, a scripted `initialize` → `initialized` → `textDocument/definition` (or the method changed) → `shutdown` → `exit` exchange over stdio with Content-Length framing.
   - At least one refusal per changed area: a path outside any worktree, a position past the end of the file, a malformed or oversized JSON-RPC frame, an unknown method (`MethodNotFound`), a request cancelled with `$/cancelRequest`, a second daemon on the same state dir (refused by the lock), a socket and state dir that are not `0600`/`0700`, a query after `git checkout` or `sed -i` without `sync` (must see the change).
   - Check that an answer with `complete=true` is not contradicted by a `grep -rn` over the fixture copy (every textual occurrence is in `exact ∪ possible`, or is in a comment or string).
   - Stop the daemon with SIGTERM (`kill -TERM <pid>`, the PID you started; never by pattern), confirm a clean exit, that the socket file is gone, and that no JVM worker or child it started is still alive.
4. **Regressions**: a neighbouring command that used to work. Any regression goes back to implement as a finding.
5. `git status` shows no change from QA itself (no state written into the repo); remove the scratchpad state.
6. Write `qa.json`.

## Output

```
{ skipped: string?, exercised[], refusalsTested[], completenessCheck: "consistent"|"contradicted"|null, regressions[], issues[] }
```

## Sensors

Skipped with a reason, or `exercised` non-empty with at least one refusal tested, every process you started stopped, `completenessCheck` not `contradicted`, and `regressions` empty.

## Gates

To ship: no regression.
