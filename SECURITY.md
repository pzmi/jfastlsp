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
