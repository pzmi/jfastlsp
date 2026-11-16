# Processes

The size decides how many stations run. The unit is a task in `doc/tasks.md`; a task's size is read from its text, not guessed from the diff.

## Classification

Priority: user override (`--small`/`--medium`/`--large`), then auto, then MEDIUM.

Auto:

- **SMALL**: docs, ADR files, config, CI, mechanical, or one module plus its test; `light` tasks.
- **MEDIUM**: one crate, up to about five `Accept:` clauses; the usual `standard` task.
- **LARGE**: the view registry, generations and freshness (plan §4); the daemon lifecycle, the single-owner lock, the socket and its permissions (plan §5.2, §5.3); the wire protocol or the `jfl --json` schema (plan §2.4, §6); the LSP adapter, JSON-RPC framing, cancellation or position encodings (plan §2.3, §2.5); the result envelope and the completeness computation (plan §2.1); the resolver; the `javac` workers and the diagnostics coordinator (plan §3); the Gradle model (trusted code, plan §5.7); `jfl-process` or any spawn of a child; the storage tier and cache keys (plan §5.4, §5.5); a Quint model or conformance driver; a contract suite; `architecture.toml` or the architecture test, `deny.toml` or the clippy lists; anything marked ⛳; `deep` tasks; or production code in more than one crate.

## Sequences

**SMALL**, no review station:

```
intake -> plan -> implement -> test -> polish -> qa -> ship -> babysit
```

**MEDIUM and LARGE**, with the review loop:

```
intake -> plan -> implement -> test -> polish -> review -> qa -> ship -> babysit
              ^                                  |
              +------ structural changes --------+
```

`qa` runs the binary only when the task changed something a person or agent can observe (`jfl` subcommands, their output, `--json`, exit codes, `jfl lsp` over stdio, the daemon's socket, its start, stop and restart, the plugin and skill files); otherwise it records `skipped: no runnable surface` with the reason and passes.

## Gates

| From | To | Kind | Condition |
|:--|:--|:--|:--|
| intake | plan | SCRIPT | Definition of Ready met |
| plan | implement | SCRIPT | checklist written; LARGE with an open design question also asks the owner one short question |
| implement | test | SCRIPT | diff non-empty |
| test | polish | SCRIPT | the gate green |
| polish | review | SCRIPT | MEDIUM or LARGE |
| polish | qa | SCRIPT | SMALL |
| review | qa | AGENT | no finding needs a structural change |
| review | implement | AGENT | structural change needed and `loopCount < 2` |
| review | qa | SCRIPT | `loopCount >= 2`: force on; remainder goes in the ship report |
| qa | ship | SCRIPT | no regression |
| ship | babysit | SCRIPT | pushed |
| ship | end | SCRIPT | not the push owner: report "commit ready" |
| babysit | end | SCRIPT | CI green, or escalated after 3 fixes |

## Limits

| Loop | Max | On limit |
|:--|:--|:--|
| review to implement | 2 | remaining findings reported |
| CI fix attempts | 3 | escalate |
