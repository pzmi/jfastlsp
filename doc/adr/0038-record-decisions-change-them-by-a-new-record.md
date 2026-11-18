# 0038. Record important decisions; change one by a new record that supersedes it

Date: 2026-11-18

## Status

Accepted (owner, 2026-11-18). Supersedes [0001](0001-record-architecture-decisions.md).

## Context

ADR 0001 let a refinement be appended to an Accepted record as `## Amended YYYY-MM-DD`. In two
days 37 records gathered about 34 such sections, some written by agents and accepted later, so a
record no longer read as the decision taken in its context but as a running log of details. At
the FORMAL checkpoint (2026-11-18) the owner said: "you amend by creating a new adr. If you need
to amend a lot it may mean you wrote too many ADRs for small things." This record applies that
rule to 0001 itself, which is Accepted: it is superseded rather than amended once more.

## Decision

Architectural decisions are recorded as ADRs in `doc/adr/`, Nygard format (a title, a date, then
Status, Context, Decision and Consequences, optionally Alternatives considered);
`doc/adr/README.md` is the index, checked by `just adr-index`.

- **An ADR only for an important decision whose context is worth recalling later**: a direction
  the project could plausibly reverse, where the reasons and the alternatives weighed would
  otherwise be lost. Everything else is recorded where it is made: the plan, the foundation, the
  verification document or the task card holds the design, the commit message says why, a code
  comment explains a non-obvious constraint.
- **An Accepted record's text is not edited.** Only its Status line changes, to
  `Superseded by NNNN` (with the date and a pointer when only part of it is replaced).
- **A changed decision is a new ADR that supersedes the old one**, with its own Context and the
  reason for the change, and the rules, fixtures and tests that pin the old decision change in
  the same commit.
- **A detail that is not an important decision** (a measured number, a configuration, a
  technical correction, a narrowed mechanism) goes in plan, foundation, verification or the card
  and in the commit message, never into an ADR. Needing many changes to one record is a sign it
  recorded a small thing.
- **An ADR is a record, not a rule.** It is never cited as a reason not to do the better thing,
  and code never drifts from one silently.
- **The ADR number is the decision id.** The orchestrator reserves numbers at dispatch; a lane
  that finds its number taken renumbers its record and every citation in the same commit.
- **Statuses** carry the decider and date: `Proposed` (a default tasks follow until the owner
  decides), `Accepted (owner, YYYY-MM-DD)`, `Rejected (owner, YYYY-MM-DD)`,
  `Superseded by NNNN`. Only the owner moves a record to Accepted or Rejected. A Proposed record
  may still be edited until the owner decides it.

## Consequences

The `## Amended` sections written before 2026-11-18 stay as they are; the owner's acceptances of
2026-11-18 are the last edits of that kind. No new one is written. The existing records are
audited for the owner (BOOT-04): each is kept, retired into the document that holds its detail,
or consolidated into a new record that supersedes several; the owner decides the audit.
`just adr-index` knows Proposed, Accepted, Rejected and Superseded; a `Retired` status, if the
owner adopts it, needs that check widened in the same commit.
