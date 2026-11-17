# 0027. javac worker: fork java-language-server's compiler core, load scip-java's plugin

Date: 2026-11-16

## Status

Superseded by [0036](0036-javac-worker-our-own-warm-context-with-scip-java.md) (2026-11-17, the owner's decision at the FND checkpoint). How the worker is built and launched
stays SEM-04's ADR.

## Context

Semantic diagnostics come from long-lived `javac` workers per `EnvironmentKey` (plan §3.1);
BOOT-00 planned to write the worker from nothing. Survey of 2026-11-16:
[georgewfraser/java-language-server](https://github.com/georgewfraser/java-language-server)
(MIT, v0.2.49, 2026-07-06, one maintainer) is a whole LSP built on `JavacTask` that keeps a
long-lived compiler and erases irrelevant method bodies so it compiles only the region of
interest. [scip-java](https://github.com/scip-code/scip-java) (Apache-2.0, v0.14.2) is a javac
plugin (`-Xplugin`) that writes SCIP occurrences for every compiled file. jdtls cannot serve: it
compiles with ECJ, and its javac backend is incubating.

## Decision

- **Fork the compiler core** of java-language-server into `java/javac-worker/`: the long-lived
  `JavacTask`, file-level incremental recompiles and body erasure. Drop its LSP server and its
  classpath inference (the classpath comes from the project model, ADR 0026). Pin the fork's base
  commit, keep its licence and notices, and keep the diff small so upstream fixes can be merged.
- **Load scip-java's plugin in the worker**, so each attributed file can also yield
  compiler-exact occurrences. This is E2's **javac-attributed arm** and, if DEC-04 picks it, the
  source of `exact` answers wherever the classpath is known and the file compiled.
- **ECJ batch** (`-proceedOnError`) is an E3 comparison arm for latency and false-error rate.
- The protocol to the daemon stays length-prefixed JSON over stdin and stdout through
  `jfl-process`'s long-lived mode (plan §3.1); `-proc:none` by default (plan §3.3).

## Consequences

EVAL-10 starts from the fork, not from a blank spike; SEM-04 hardens it. Java source under
`java/` is dev-time and runtime-optional (ADR 0015). Upstream is one maintainer: the fork is ours
to maintain once taken.

## Alternatives considered

- Write the worker from scratch: the body-erasure and incremental tricks would be rediscovered.
- Run java-language-server or jdtls as the worker: one workspace per process and no project
  model; jdtls is ECJ. NetBeans' javac services (Apache-2.0): ideas only, platform weight.
