# 0026. Gradle model: wrap microsoft/build-server-for-gradle

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-17). Replaces the
mechanism ADR plan §5.7 left to PROJ-01; PROJ-01 may amend it with the fixture results.

## Context

The opt-in project model (plan §5.7) needs, per module and source set: source and
generated-source roots, the compile classpath, module dependencies, the JDK, `--release` and the
annotation-processor path, which is plan §5.4's `EnvironmentKey`. BOOT-00 planned a Gradle Tooling
API helper of our own. The standard `EclipseProject`/`IdeaProject` models miss per-source-set
detail and some dependencies
([kotlin-language-server #439](https://www.github.com/fwcd/kotlin-language-server/pull/439)).

## Decision

Wrap **[microsoft/build-server-for-gradle](https://github.com/microsoft/build-server-for-gradle)**
(MIT, 0.4.0, last commit 2026-11-11, used by vscode-java's Gradle BSP mode). Its plugin is
injected by an init script, and its `GetSourceSetsAction` (a Tooling API `BuildAction`) returns a
`GradleSourceSet` per source set with `sourceDirs`, `generatedSourceDirs`, `compileClasspath`,
`moduleDependencies`, `buildTargetDependencies`, `javaHome`, `javaVersion`,
`sourceCompatibility`, `targetCompatibility` and `compilerArgs`; it handles composite builds.

- A small JVM helper, **`java/gradle-model/`** (a Tooling API client of about 200 lines),
  runs that action and prints JSON; `jfl-adapter-gradle` launches it through `jfl-process`,
  parses the JSON defensively and caches it by build-file fingerprint. Opt-in, background-only
  and one run per fingerprint, as plan §5.7 says.
- PROJ-01 first checks on the Lombok and MapStruct fixtures that `compilerArgs` carries
  `-processorpath` and `-s`; if not, it sends a small patch upstream and, meanwhile, reads them
  as IntelliJ's Gradle model builders (Apache-2.0) do.
- With no model yet: kmp-lsp's and ktlsp's Gradle-cache heuristics are the reference for a
  "classpath unknown" fallback that only widens `possible[]` and never enables semantic
  diagnostics.

## Consequences

PROJ-01 shrinks to the wrapper, the cache and the opt-in. The helper's dependency on the build
server is pinned to a release and its licence noted in third-party notices. The JDK is needed only
when a user opts in (ADR 0015 unchanged).

## Alternatives considered

- Speak BSP to its `server` module (`workspace/buildTargets`, `buildTarget/javacOptions`): a
  long-lived server process and a second protocol; the fallback if the action API changes.
- Our own Tooling API helper over the standard models: misses per-source-set detail.
