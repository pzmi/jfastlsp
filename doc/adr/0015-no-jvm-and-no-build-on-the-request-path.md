# 0015. No JVM and no build on the request path

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-17).

## Context

A JVM start, a Gradle configuration or a compile takes seconds to minutes; an agent's navigation
request has a budget of hundreds of milliseconds (plan §2.7), and Claude Code may start the
server mid-task.

## Decision

No query waits on a JVM, a Gradle run or any build. JVMs run only as background workers: `javac`
workers per `EnvironmentKey` that push diagnostics (plan §3) and the opt-in Gradle model computed
in the background and cached by build fingerprint (plan §5.7). A query uses whatever the current
generation holds and says what is missing in `incomplete_because`. `jfl diag` waits for the
current version's diagnostics with a deadline and returns what it has.

## Consequences

Semantic answers before the project model exists are syntax-only and say so. Reviews treat a
request that can block on a JVM as HIGH. The javac worker and Gradle model live in their own
adapters behind `jfl-process` (plan §6.5).
