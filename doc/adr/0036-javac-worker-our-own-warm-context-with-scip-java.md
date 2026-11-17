# 0036. javac worker: our own small worker with one warm javac context, scip-java's plugin

Date: 2026-11-17

## Status

Accepted (owner, 2026-11-17). Supersedes
[0027](0027-javac-worker-forked-from-java-language-server-with-scip-java.md). How the worker is
built and launched stays SEM-04's ADR.

## Context

ADR 0027 proposed forking the compiler core of
[java-language-server](https://github.com/georgewfraser/java-language-server) into the javac
worker. A closer read: that core is about 600 lines; its `ReusableCompiler.java` is copied from
the JDK under GPL-2.0 with the Classpath Exception, which does not suit an MIT-only project (ADR
0033); and its method-body erasure serves only autocomplete, which jfastlsp does not offer. At
the FND checkpoint (2026-11-17) the owner chose to write our own small worker instead.

## Decision

- **Our own worker** in `java/javac-worker/`, about 1–2 weeks of work. It keeps **one javac
  context warm across compiles** per `EnvironmentKey`: the technique java-language-server and the
  JDK use, read as reference only; no code is copied from either.
- **JDK 17, 21 and 25** are tested in CI. When context reuse fails on a JDK (an internal API
  differs, a reuse throws), the worker falls back to a fresh compiler for that compile and says
  so in its reply.
- **Body blanking in dependency sources**: files pulled in through `-sourcepath` have their
  method bodies blanked, so only the edited file is attributed in full. EVAL-10 confirms first
  that javac checks those bodies at all under `-implicit:none`; if it does not, blanking is
  dropped as unneeded.
- **Class folders from earlier builds** (Gradle's `build/classes`) are read when present, ahead
  of sources, to cut what javac must parse.
- **scip-java's javac plugin 0.14.2** (`-Xplugin`) is loaded for compiler-exact references (E2's
  javac-attributed arm, plan §5.6). What it emits for files with errors, and its latency, are
  EVAL-10's to measure.
- **Annotation processors are off by default** (`-proc:none`, plan §3.3) and opt-in per
  repository, with their cost measured.
- **ECJ** stays only as E3's comparison arm (`-proceedOnError`).
- The protocol to the daemon is unchanged: length-prefixed JSON over stdin and stdout through
  `jfl-process`'s long-lived mode (plan §3.1).

## Consequences

EVAL-10 builds the spike from nothing rather than from a fork and answers the two open questions
(body checking under `-implicit:none`; the plugin on files with errors and its latency); SEM-04
hardens it. No GPL code enters the repository. The worker is ours to maintain, small, and has no
upstream to track.

## Alternatives considered

- Fork java-language-server's core (ADR 0027): GPL-2.0 code in `ReusableCompiler.java`, and an
  erasure tuned for autocomplete.
- Run java-language-server or jdtls as the worker: one workspace per process and no project
  model; jdtls compiles with ECJ.
