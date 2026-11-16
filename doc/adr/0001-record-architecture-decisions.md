# 0001. Record architecture decisions

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-16).

## Context

Several lanes and agents change this repository in parallel, and the reasons behind its structure
would otherwise live in chat history. Parallel lanes also need decision ids that cannot collide,
and a decision must be changeable on purpose without being edited away.

## Decision

Architectural decisions are recorded as ADRs in `doc/adr/`, in Michael Nygard's format: a title,
a date, then Status, Context, Decision and Consequences, and optionally Alternatives considered.
`doc/adr/README.md` is the index.

- **The ADR number is the decision id.** There are no separate D-ids; `doc/tasks.md`'s Decisions
  table lists ADR numbers. The orchestrator reserves numbers when it dispatches a task; a lane
  that finds its number taken renumbers its record and every citation in the same commit.
- **An ADR is a decision record, not a rule.** It records a decision in its context at the time.
  When the context changes the decision may change, deliberately and on the record; an ADR is
  never cited as a reason not to do the better thing, and code never drifts from one silently.
  The rules, fixtures and tests that pin an ADR change in the same commit as the ADR.
- **Statuses** carry the decider and date: `Proposed` (a planner default that tasks follow until
  the owner decides), `Accepted (owner, YYYY-MM-DD)`, `Rejected (owner, YYYY-MM-DD)`,
  `Superseded by NNNN`, with annotations such as `amended YYYY-MM-DD` or `amended by NNNN`. Only
  the owner moves a record to Accepted or Rejected.
- **Changing a decision.** A refinement that keeps the decision is appended as
  `## Amended YYYY-MM-DD (owner|planner): <what>` with its reason; the original Context and
  Decision stay. A reversal is a new ADR that supersedes the old one. An amendment of an Accepted
  record by anyone but the owner is marked `amendment proposed` until the owner accepts it.

## Consequences

The reasons behind the architecture survive the session that produced them. Until FND-03 lands
`cargo xtask adr new` and `adr index`, records are created by copying `template.md` and the index
row is added by hand; from FND-03 on the `docs` stage checks the index against the files. The
cost is one small file per decision and the discipline to write it in the commit that makes it.
