# ship

Dispatch: inline

## Inputs

intake.{taskId, title, branch, checkpoint, gate, pushOwner, ownerCriteria}, implement.{filesChanged, newDependencies, adrsWritten}

## Steps

1. **Final gate.** The gate green once more on the exact tree being committed (`just check` once FND-02 has landed, the fallback before). `git status --short`: every changed file is yours or listed; someone else's uncommitted edits stay unstaged.
2. **Docs.** A decision that adds to or reverses `doc/plan.md`, `doc/foundation.md`, `doc/verification.md` or an Accepted ADR has its ADR in this commit (`adrs new` or the reserved number, index updated and `just adr-index` clean; a reversed ADR marked `Superseded by NNNN`). Affected docs updated in the same commit. `CLAUDE.md` and `.claude/` are not edited: a needed change goes in the report.
3. **Commit.** Only on the green gate of step 1. `git add <the files>` (never `-A`, never `.`). `git commit --author "Patryk Żmigrodzki <pzmi@users.noreply.github.com>"` (the committer stays the environment's identity; ADR 0003) with:
   - subject `<ID>: <summary>` (imperative, under 72 chars)
   - a body saying what and why; each new dependency named with its reason; the gate that ran; an `Impact:` line
   - the trailer block last, written by hand, in the order `Signed-off-by: Patryk Żmigrodzki <pzmi@users.noreply.github.com>`, then the `Co-Authored-By` and `Claude-Session` lines exactly as the caller's (or session's) attribution instructions give them; never `git commit -s` (it would append `Signed-off-by` after the block)
   - no model ids (outside the trailer block), secrets, tokens, internal hostnames, personal paths, private-repo content or the name of a private project (ADR 0004)
   Never `--no-verify`. If a hook fails, fix the cause; do not bypass.
4. **Push.** Only when the caller owns the push (`pushOwner`; under `/orchestrate` it never does, ADR 0002). Then integration is `scripts/land.sh <sha>` (before FND-03: cherry-pick onto a clean `main`, the gate on the merged tree, then `git push origin main`), confirmed with `git ls-remote origin main`. Rejected: `git fetch`, land again. Never force. Not the push owner: stop here and report "commit ready" with the sha.
5. Checkpoint task: say that a `/review` plus the adversarial second review is due; do not start the next task.
6. Write `ship.json`.

## Output

```
{ commitSha, pushed: bool, ownerPending[], issues[] }
```

## Sensors

Commit exists; its author is the owner's identity; its trailer block is last and ordered `Signed-off-by`, `Co-Authored-By`, `Claude-Session` (`git log -1 --format=%B | tail -4`); its subject starts with `<ID>: ` and the body has an `Impact:` line; the gate was green on it; if pushed, the remote head equals the sha.

## Gates

- To babysit: pushed.
- To end: not the push owner (report "commit ready").
- Stop and ask: a force-push would be needed.
