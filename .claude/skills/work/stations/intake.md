# intake

Dispatch: inline

## Inputs

`$ARGUMENTS`: task id, issue number or URL, optional size override; the caller's prompt (worktree, branch, gate, target dir, reserved ADR numbers, "do not push").

## Steps

1. **Find the task.** Task id: read its card in `doc/tasks.md` and its row in the overview (dependencies, model, size, ⛳ mark, "Parallel with"), plus the Decisions table and "Rules for every task". Issue number: read it with MCP `issue_read` (or `gh issue view` when `gh` exists); if its title starts with a task id, use that task's card too. Absent: pick per SKILL.md "Input".
2. **Dependencies.** Every dependency must already be committed (`git log --oneline | grep " <dep>:"`). A missing one: stop and report; do not start work that builds on nothing. A `DEC-` dependency counts only when it is recorded as decided by the owner: its ADR's status reads `Accepted (owner, …)` (`grep -A2 '^## Status' doc/adr/NNNN-*.md`), or, where no ADR was needed (ADR 0001, amended), its card in `doc/tasks.md` is ticked and marked `Decided:` with the date; an agent's draft committed as `Proposed` under the `DEC-` subject does not count.
3. **Read the citations.** Read every `doc/plan.md`, `doc/foundation.md` (fnd) and `doc/verification.md` (ver) section the card cites, plan §6 (crates and dependency law) and ver §2 (test levels), the ADRs in `doc/adr/` the card names or that touch the affected crates, and every existing file the task modifies. Not the documents whole.
4. **Crates and authority.** Name each affected crate's role (pure, application, support, adapter, root, test-only, tool-only) from plan §6.1 and `architecture.toml`, and the authority it holds (process, filesystem, socket; plan §6.5). A task that seems to need authority in a crate that has none, or an edge plan §6.3 does not list, is a design question, not an edge to add.
5. **Definition of Ready** (fnd §11, `doc/sdlc.md`): observable behaviour, invariants (the model invariants of plan §7 and ver §6.1 it touches), affected crates, threat model when untrusted input (Java sources, JARs, JSON-RPC, socket peers), processes or files change, test levels, model impact (views and generations, the daemon lifecycle, diagnostics publication: then the Quint model changes first), protocol impact (a protocol message, the `jfl --json` schema, an LSP method or capability), contract suites it must pass, testable `Accept:`, `(owner)` criteria. Not met: ask the owner one short question; if the gap is only wording, fix it minimally in `doc/tasks.md` and say so.
6. **Classify** per `processes.md`.
7. **Branch and gate.** Stay on the branch you were given (`main`, or the worktree lane the caller named). Record the gate `doc/tasks.md` names as live, and whether you own the push (the caller said "do not push": you do not).
8. Write `work-order.json` and `intake.json`.

## Output

```
{ taskId, title, criteria[], ownerCriteria[], cites[], dependsOn[], affectedCrates[], authority: "none"|"process"|"fs"|"socket"|"mixed", invariants[], modelImpact: "none"|"<model>.qnt", protocolImpact: "none"|"protocol"|"cli-json"|"lsp"|"mixed", contractSuites[], size, process, checkpoint: bool, branch, gate, pushOwner: bool, issues[] }
```

## Sensors

Task card found; every dependency committed; `criteria` non-empty (each `Accept:` clause one entry, `(owner)` ones in `ownerCriteria`).

## Gates

To plan: always.
