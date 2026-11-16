# 0003. Commit identity, subject and trailer order

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-16).

## Context

Commits are written by agents on the owner's behalf in a public repository. Authorship must name
the human responsible, with a public address; the signing identity belongs to the environment;
the history must be searchable by task; and the trailers must parse as one block.

## Decision

- **Author and `Signed-off-by`:** `Patryk Żmigrodzki <pzmi@users.noreply.github.com>` (passed with
  `--author`). The **committer** is the environment's identity, which signs the commit.
- **Subject:** `<ID>: <summary>`, imperative, under 72 characters, where `<ID>` is the task id
  (`IDX-04`) or `BOOT-00` for the bootstrap. A fix to a landed task repeats its id
  (`<ID>: fix <what>`).
- **Body:** what and why; each new dependency named with its reason; the gate that ran; an
  `Impact:` line (none, dependency graph, public API, protocol, `--json` schema, CLI, LSP
  capabilities, cache format, config).
- **Trailer block last, written by hand, in this order:** `Signed-off-by`, `Co-Authored-By`,
  `Claude-Session`, the last two exactly as the session's attribution instructions give them; no
  other trailers. `git commit -s` is not used with other trailers, because it appends
  `Signed-off-by` after an existing block.

## Consequences

`git log --oneline main | grep " <ID>:"` finds a task's commit, which is how dependencies are
checked. The ship station's sensors check author, subject, `Impact:` and trailer order. Model
names appear only in the `Co-Authored-By` trailer, never in repository content.
