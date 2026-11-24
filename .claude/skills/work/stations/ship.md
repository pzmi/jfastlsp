# ship

Dispatch: inline

## Inputs

intake.{taskId, title, branch, checkpoint, gate, pushOwner, ownerCriteria}, implement.{filesChanged, newDependencies, adrsWritten}

## Steps

1. **Final gate.** The gate green once more on the exact tree being committed (`just check`), run in the foreground in the worktree you commit from, with its exit code recorded in `ship.json` (`gateExit`). In a lane worktree this station is the gate: the Stop hook guards only the checkout the session started in. `git status --short`: every changed file is yours or listed; someone else's uncommitted edits stay unstaged.
   The gate must also be green on the committed tree: after step 3's `git commit` (or `--amend`), run `just check` again with the commit in place and record that exit code as `gateExit`, because some stages (secrets) scan committed objects. Read the gate's own exit code, never a wrapper's `echo` or the last command of a pipeline.
2. **Docs.** A decision that changes `doc/plan.md`, `doc/foundation.md` or `doc/verification.md` updates them in this commit and says why in the commit message. A change that reverses an ADR supersedes it (a new record, the old one's Status line marked `Superseded by NNNN`, its only edit; no `## Amended` section), with the index updated and `just adr-index` clean. A new ADR only for an important decision whose context is worth recalling later (ADR 0038); otherwise the detail goes in plan, foundation or the card and the commit message and code comments carry the reason. `CLAUDE.md` and `.claude/` are not edited: a needed change goes in the report.
3. **Commit.** Only on the green gate of step 1. `git add <the files>` (never `-A`, never `.`). `git commit --author "Patryk Żmigrodzki <pzmi@users.noreply.github.com>"` (the committer stays the environment's identity; ADR 0003) with:
   - subject `<ID>: <summary>` (imperative, under 72 chars)
   - a body saying what and why; each new dependency named with its reason; the gate that ran; an `Impact:` line
   - the trailer block last, written by hand, in the order `Signed-off-by: Patryk Żmigrodzki <pzmi@users.noreply.github.com>`, then the `Co-Authored-By` and `Claude-Session` lines exactly as the caller's (or session's) attribution instructions give them; never `git commit -s` (it would append `Signed-off-by` after the block)
   - no model ids (outside the trailer block), secrets, tokens, internal hostnames, personal paths, private-repo content or the name of a private project (ADR 0004)
   Never `--no-verify`. If a hook fails, fix the cause; do not bypass.
4. **Push.** Only when the caller owns the push (`pushOwner`; under `/orchestrate` it never does, ADR 0002). Then integration is `scripts/land.sh <sha>`, the only pusher (agents never push by any means; the common `git push` forms are denied, fnd §11.6), confirmed with `git ls-remote origin main`. Rejected: `git fetch`, land again. Never force. Not the push owner: stop here and report "commit ready" with the sha.
5. Checkpoint task: say that a `/review` plus the adversarial second review is due; do not start the next task.
6. Write `ship.json`.

## Output

```
{ commitSha, gateCommand, gateExit: 0, pushed: bool, ownerPending[], issues[] }
```

## Sensors

Commit exists; `gateExit` is 0 for the exact tree committed; its author is the owner's identity; its trailer block is last and ordered `Signed-off-by`, `Co-Authored-By`, `Claude-Session` (`git log -1 --format=%B | tail -4`); its subject starts with `<ID>: ` and the body has an `Impact:` line; the gate was green on it; if pushed, the remote head equals the sha.

## Gates

- To babysit: pushed.
- To end: not the push owner (report "commit ready").
- Stop and ask: a force-push would be needed.
