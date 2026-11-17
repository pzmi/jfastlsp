# E0 target repository: criteria and the choice (DEC-01)

Status: input to DEC-01, written by an agent on 2026-11-17. Nothing here is decided. The owner
chooses the target and records the choice in an ADR (DEC-01, plan §9.1). This page does not name
or select a repository beyond the public corpus entries it cites.

## 1. What the experiments need from the target

Phase 0 measures jfastlsp on one real, large Gradle Java repository with a history of
agent-sized tasks (plan §9.1). Each experiment uses a different part of it:

| Experiment (plan §9.2) | Needs from the target | Criteria below |
|:--|:--|:--|
| E0 agent eval on Harbor (ADR 0029) | 15 to 20 tasks from history, each with one test command, run in several arms, three times each | C1, C4, C5, C8 |
| E1 parse and index time | a full worktree with a realistic file count and size | C2 |
| E2 resolver bake-off | a SCIP index built by `scip-java` as the oracle | C3, C6 |
| E3 `javac` worker spike | per-module compile with `-sourcepath`; annotation processors | C2, S1, S2 |
| E4 jdtls at 1, 3 and 10 worktrees | worktrees at nearby commits of one history | C7 |

## 2. Criteria

**Must** (a candidate without one of these is not used):

- **C1 Builds on the owner's machine.** The Gradle wrapper builds the repository at a pinned
  commit with a pinned JDK on Linux (x86-64 or arm64) or macOS arm64 (ADR 0006). After one warm-up that
  fills the Gradle cache, a task's build and test run need no network (ADR 0029 bakes the cache
  into the arm image).
- **C2 Size and shape.** At least 5 Gradle subprojects, with dependencies between them so that
  cross-module navigation matters. Proposed: at least 20 subprojects and at least 5,000 `.java`
  files, so E1 and E4 numbers can be read against the corpus (`doc/bench-corpus.md`). The owner
  declares the size class (small, medium or large, as the corpus table uses them) once the file
  count is known.
- **C3 Java dominates the code the tasks touch.** Any Kotlin or Scala in the same modules is
  counted at the pin (`git ls-files`) and listed before selection: jdtls and the `javac` worker
  see only Java, so mixed-language modules skew E0 and E2 coverage.
- **C4 History of agent-sized changes.** At least 20 commits, so that 15 to 20 tasks and spares
  can be chosen. Each is a bug fix, a signature change, a cross-module feature or a rename, small
  enough for one agent session (proposed: about five production files or fewer), and comes with
  a test change that fails before the fix and passes after it. The task set is taken from
  history, not written from scratch (EVAL-02).
- **C5 One deterministic test command per task.** One command exits with a result, gives the
  same result when run twice on the pinned commit, and finishes in a time the owner accepts on
  the owner's machine. The time limit is set from the Harbor smoke task (EVAL-01, ADR 0029), not guessed here.
- **C6 `scip-java` indexes the build** at the pinned commit. E2 uses its index as the oracle
  (plan §9.2).
- **C7 Worktrees can sit at nearby commits.** The pin and the task commits share one history on
  the default branch, so E4 can create 1, 3 and 10 worktrees at nearby commits. The bench corpus
  measured a worktree 150 commits from the pin (`doc/bench-corpus.md`, E1 spike).
- **C8 Licence and permission.** Either a public repository under a permissive licence (Apache-2.0,
  MIT or BSD; `doc/bench-corpus.md` gives each corpus licence), or the owner's own right, or
  written permission from whoever holds the repository, to run Claude Code, language servers and
  Docker over it.

**Should** (a candidate without these is weaker, not excluded):

- **S1 Lombok or MapStruct present.** Plan §9.2 asks for them "if possible", and E3 needs zero
  false errors with them (plan §10.4). None of the five public corpus repositories uses
  either, so without a candidate that does, they are covered only by synthetic fixtures
  (`doc/bench-corpus.md`; ver §4). *Update 2026-11-17:* apache/fineract is now the corpus's
  `lombok` repository and a second target (ADR 0032, amended); the five above stay Lombok-free.
- **S2 Annotation processors that generate sources.** Plan §3.3 needs generated sources on the
  diagnostics path.
- **S3 A clean default branch** with no history rewrites, so pins stay reachable.

**Excluded from the public option.** elasticsearch is not permissively licensed (SSPL, Elastic
License 2.0 or AGPL-3.0). `doc/bench-corpus.md` keeps it out of the repository, and the public
options (§4, B and C) commit E0 task definitions, which would then derive from it. junit-framework
is EPL-2.0 and is a small-class repository, not a candidate for E0's size.

## 3. What stays private, and how results come back

This section applies only if the chosen target is private (ADR 0004, `doc/sdlc.md` §7, plan §9.1).

- **Committed:** the harnesses (`jfl-eval`, the EVAL lane), the runbooks, this page, and the ADR
  that records the choice. The ADR names the target as "a private repository meeting the criteria"
  with its size class. Whether the commit pin goes into the ADR is the owner's call, since DEC-01
  asks for it and the ADR is public.
- **Never committed:** code, file paths, class or package names, task text, prompts, agent
  transcripts, language-server answers that carry paths or source lines, and screenshots.
- **Returned as aggregates under `doc/eval/`:** success rate per arm and task, tool calls and
  tokens (means and spreads), counts of LSP operations and of `exact`/`possible` answers per
  construct, module and file counts, timings, and resident memory. Tasks appear by anonymous
  number only.
- **Enforced by a test:** the `jfl-eval` report never contains a prompt, path or source line from
  its input (EVAL-01 acceptance; a test plants one and checks it is absent). The hashed
  private-name check (fnd §7.4, `.private-names.sha256`) guards names in committed text.

If the target is public, task definitions (commit ids, prompts, test commands) may be committed,
and golden cases taken from it are positions and symbol names only, from Apache-2.0 repositories
(`doc/bench-corpus.md`).

## 4. The choice

Three options. The owner picks one. The ranking is B, A, C; it reflects this page's recommendation (B, below).

| Rank | Option | Summary |
|:--|:--|:--|
| 1 (recommended) | **B: spring-framework** at the pinned commit `4cb62d893e09e23791d661e8d9b35aedee7006d0` (public, Apache-2.0) | reproducible, pure Java, and already measured in the corpus |
| 2 | **A: a private repository** on the owner's machine meeting C1 to C8 | the most representative of the owner's work; results return as aggregates only |
| 3 | **C: kafka** at the pinned commit `7e0ebdf74f30cab1ded2a462cc9d45cefdf86c2a` (public, Apache-2.0) | more modules and a heavier build; Scala beside Java |

Pros and cons for each experiment:

**A, private repository.**
- E0: the most realistic tasks, with the owner's own Lombok or MapStruct code if present. Tasks
  and results cannot be published, so nobody else can rerun the eval; only aggregates are kept.
- E1: no baseline yet. The EVAL-05 harness measures it on the owner's machine.
- E2: the `scip-java` index and the coverage counts stay local; only aggregates return.
- E3: the only option that can test Lombok and MapStruct on the real build, which the zero-false-
  errors criterion needs (plan §10.4).
- E4: numbers reflect the owner's size and machine; nothing to compare against the corpus.

**B, spring-framework (recommended).**
- E0: the task set can be published and rerun by anyone (EVAL-02). Public history may already be
  in model training data, so success rates could be inflated. The E0 control set is also public
  (ADR 0029), so the risk is shared; the owner should read the success rates with it in mind.
  Pure Java per the corpus table, so the Java arms are not confounded by other languages.
- E1: measured on 2026-11-16 (9.3k files, 1.04 s warm, 1.42 s cold), so the target's numbers
  compare directly with the corpus.
- E2: `scip-java` indexes a pure Java build; coverage is not diluted by unindexed Scala.
- E3: no Lombok or MapStruct in spring-framework, so those need the synthetic fixtures (and,
  since 2026-11-17, fineract; ADR 0032 amendment).
- E4: a worktree 150 commits apart is already measured (C7).

**C, kafka.**
- E0: the same public-history caveat as B. Build is heavier (about 68 projects from one root
  build, Scala compiled beside Java), so three arms times three runs times 15 to 20 tasks costs
  more owner machine time.
- E1: measured on 2026-11-16 (6.3k files, 74 MB, 1.59 s warm). Its 1.6 MB test file is the
  per-file worst case, a useful stress case for E1.
- E2: `scip-java` indexes the Java part only; Scala call sites are not counted, so the coverage
  figure covers less of the code the owner would see.
- E3: many projects and the per-module compile multiply the `javac` worker cost across 10
  worktrees. Good stress for the EnvironmentKey count.
- E4: about 68 projects against spring's 22 modules (`doc/bench-corpus.md`), a heavier stress
  for the jdtls baseline (plan §1.3), with the build cost noted above.

**Recommendation: B (spring-framework).** It is public, so its task set and results can be
checked and cited in the ADR, which matters for the DEC-02 go/no-go record. It is pure Java, so
the Java arms and the coverage measure are not muddied by Scala. Its numbers for E1 and E4 are
already on record in the corpus, so E0 to E4 read together. Its build is smaller than kafka's,
which keeps the E0 run count on the owner's machine practical. Its weak points are public-history
contamination (shared with the control) and no Lombok or MapStruct (fineract was added as a second target on 2026-11-17 for
that; ADR 0032). If the owner values
representativeness over reproducibility, option A is the alternative, with the aggregate-only
rules of §3 and the loss of published task sets.

## 5. What the owner does next

1. Pick A, B or C. Under B, the bench-pinned commit is the default pin; any other commit goes into
   the ADR.
2. For A: confirm C8, and declare the size class. Record "a private repository meeting the
   criteria" in the ADR, with the pin if the owner agrees to publish it.
3. Under any option: the Harbor smoke task (EVAL-01) measures C5 (test command time and determinism) on the
   chosen repository, on the owner's machine. The result sets the per-task time limit.
4. Write the ADR (DEC-01 Accept) with the choice, the pin, the size class and the reason. It is
   the owner's decision; the agent does not write it as Accepted.

## 6. Open points for the owner

- Whether public history contamination (option B) is acceptable for E0, or whether E0 should use
  a task set taken only after a cut-off date.
- Whether a private repository should be a later second target (a new DEC), once the public one has
  shown the harness works.
