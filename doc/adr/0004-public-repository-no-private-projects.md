# 0004. Public repository: no private projects named or alluded to

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-16).

## Context

jfastlsp is public. The owner has private projects, and the phase 0 target Java repository may be
private too. Its design borrows from the owner's public repositories site-preview and mayfly.

## Decision

- **Never name or allude to a private project** of the owner's anywhere: code, docs, ADRs,
  fixtures, evaluation reports, commit messages. No name, no path, and no hint that a design came
  from an unnamed elsewhere ("an earlier project", "the same owner", lessons "we learned before").
  Every design is described on its own merits.
- The public repositories **site-preview and mayfly may be cited** where a design or tool is
  ported from them (ADR 0012).
- **Nothing from a private target repository** enters this repository: no code, paths, class or
  package names, task text or transcripts. Harnesses and runbooks are committed; runs happen on
  the owner's machine; only aggregated numbers come back (plan §9.1, `doc/sdlc.md` §7).
- No secrets, tokens, internal hostnames, personal paths or model ids in repository content (the
  commit attribution trailers are the only model mention). Fixtures are synthetic.

## Consequences

Reviews flag any such mention as HIGH (review skill, "Public repository"). Fixtures for
Lombok, multi-module builds and nested worktrees are written from scratch. Some evaluation detail
stays outside the repository by design.
