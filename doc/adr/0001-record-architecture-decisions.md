# 0001. Record architecture decisions

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-16); amended 2026-11-16 (accepted by the owner on 2026-11-17) and
2026-11-17 (owner).

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

## Amended 2026-11-16 (planner; accepted by the owner on 2026-11-17): ADR tooling is `adrs`

The Consequences named `cargo xtask adr new` and `adr index`. ADR 0030 replaces the xtask port
with maintained tools: records are created with `adrs new` (the `adrs` crate, 0.13.0) and the
index is checked by `just adr-index` (FND-03). The format stays Nygard as decided above; `adrs`
also offers MADR 4, which the owner may adopt by amending this record (listed in DEC-07). The owner
accepted this amendment on 2026-11-17 and kept the Nygard format. ADR 0035 later moves the ADR
commands into `cargo xtask` with lane XT; until that card lands, `adrs` and `just adr-index`
are the tools.

## Amended 2026-11-17 (owner): what deserves an ADR, and what an ADR is

The owner, at the FND checkpoint: "ADRs are important decision that we feel like we might want
to know the context of in the future. We don't need to write ADR for everything. We still have
commit messages and comments and code. ADRs are decision records and they can change in time.
They're not absolute, there are not rules."

- **An ADR only for an important decision whose context is worth recalling later**: a direction
  the project could plausibly reverse, where the reasons and the alternatives weighed would be
  lost otherwise (the licence, the carrier order, the command surface, the javac worker). Not for
  every crate boundary, dependency, rule or tool choice.
- **Everything else** is recorded where it is made: the commit message says why, a code comment
  explains a non-obvious constraint, the plan, foundation and task documents hold the design.
- **An ADR is a record, not a rule.** It can change in time, deliberately (an amendment or a new
  record that supersedes it); it is never cited as a reason not to do the better thing.
- ADRs 0010 (the crate map) and 0018 (the agent sandbox) are accepted and kept, but they are the
  kind that did not need an ADR; no more records of that kind are written. Their content lives in
  plan §6, fnd §2 and §3, and `.claude/settings.json`.
