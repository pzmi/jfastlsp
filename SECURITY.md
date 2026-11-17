# Security Policy

## Supported versions

No version has been released yet. Only the latest commit on `main` is considered supported.

## Reporting a vulnerability

Please report privately using GitHub private vulnerability reporting: open the repository's
**Security** tab and choose **Report a vulnerability**
(<https://github.com/pzmi/jfastlsp/security/advisories/new>). Do not open a public issue for
security problems.

## Scope

In scope, as the design develops (see [doc/plan.md](doc/plan.md) §11):

- Another local user reaching the daemon's socket or state directory
- A crafted Java source file, class file, JAR, LSP message or protocol frame that crashes the
  daemon, exhausts its memory or makes it read or write outside the worktree
- Command injection through file names, paths or configuration into the processes `jfl` starts
- The project's Gradle build code running without the user's opt-in
- Source text leaking into logs or anywhere off the machine

## Non-goals

- `jfl` does not sandbox a project's Gradle build code once the user has opted in to running it.
- Compromise of the user's account, operating system, JDK or Git installation.

## Supply chain and private names

`just check` runs three stages for this (foundation §7.3, §7.4): `secrets` (gitleaks over the full
history, default rules, nothing allowlisted), `workflows` (actionlint, zizmor, pinact and shellcheck)
and `private-names`.

This repository never names or alludes to a private project (ADR 0004). The names to keep out are
not written anywhere: `.private-names.sha256` holds only salted SHA-256 hashes, and
`just private-names` hashes the words of every line a change adds, of every untracked file and of
every commit message since the merge base with `origin/main`, and fails on a match, printing the
path and line but never the name. Maintainers add a name to the list with
`scripts/private-name-add.sh` (it reads the name from standard input when given no argument, which
keeps it out of the shell history) and commit the changed file, which holds hashes only. Matching is on whole words after folding accents to ASCII, so it catches `Foo Bar`, `foo-bar`,
`FooBar` and accented spellings, and also file names, commit authors and the branch name; it does
not match substrings, homoglyphs or misspellings. The salt is
public, so the list keeps names out of plain sight and out of every search, not out of reach of a
determined guesser. `scripts/private-names-check.sh --all` checks every file and commit message,
which is the way to audit the tree after adding a name.
