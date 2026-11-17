# test

Dispatch: subagent (general-purpose)

## Inputs

implement.{filesChanged, affectedCrates}, intake.{criteria, invariants, modelImpact, protocolImpact, contractSuites, gate}

## Prompt

Verify test coverage and run the gate for the working-tree diff. Read `CLAUDE.md` first.

1. Map every `Accept:` clause to at least one test that would fail if the behaviour broke. A clause with none: write the test.
2. Per changed source file, check the level (ver §2); a test lives at the lowest level that can fail for the reason it exists:
   - **unit**: pure logic beside the module (`#[cfg(test)] mod tests`), no I/O; position conversions, key derivation, completeness computation, envelope ordering, transition tables
   - **property** (`proptest`): laws over generated inputs (UTF-8 ↔ UTF-16 ↔ byte offset round trips, generations strictly increasing, `possible` ⊇ every name-and-arity match, framing never allocates past the limit); a found counterexample is committed as a regression seed
   - **golden**: a byte-level promise (protocol JSON, `jfl --json` output, LSP transcripts, the Java corpus's expected summaries); never rewritten to get green
   - **component**: real application code over testkit fakes (fake source tree, fake clock, fake compiler), e.g. a request pinned to generation N while N+1 is published, a stale diagnostics result dropped
   - **contract** (ver §5): a port with a fake and a real adapter (source tree, summary store, endpoint, compiler, project model, process runner) runs one shared suite against both, and the suite's own tests prove a deliberately broken implementation fails it
   - **LSP conformance** (ver §2): scripted JSON-RPC transcripts against the built `jfl lsp`: framing, lifecycle, UTF-16 positions, cancellation, unknown methods, malformed frames
   - **integration**: real files in tempdirs, real `git` worktrees, real Unix sockets in a tempdir, real child processes through `jfl-process`
   - **e2e** (`assert_cmd`): the built `jfl` against fixture Java projects copied to a tempdir; stdout, `--json` goldens, stderr and exit codes
   - **architecture**: `just check-only architecture deny clippy` and their negative cases; a new rule has a negative case
   - **conformance** (ver §6): Quint traces replayed against the real code, plus the negative control
   - **differential and oracle** (ver §7): the completeness oracle (an inserted caller appears in `exact ∪ possible` or `complete=false`) and the envelope-honesty mutation (a flipped resolver rule makes a wrong `exact` fail the suite)
3. Conventions: `should_<outcome>_when_<condition>`, `// given / when / then`, real objects over fakes, no mocking frameworks, no `#[ignore]`, no sleeps (fake clock, `tokio::time::pause`; real-OS waits only via the testkit `poll` helper), tempdir sockets, `tempfile` dirs. Never assert on log lines. Never the network or a target Java repository.
4. Run the gate (`just check`) in the foreground with the lane's `CARGO_TARGET_DIR`, never in the background. Iterate to green. A missing tool is installed at its pinned version or reported; a skipped check is never green. Kill any daemon, JVM worker or child you started, by its PID.
5. A product task: `just bench <suite>` on the corpus (small at least) and the real-corpus cases (BENCH-05) where they apply; record the metrics it changes, with corpus and machine, for the report.
6. Failures outside your diff (someone else's WIP): report them, do not fix or hide them.

## Output

```
{ testsAdded[], criteriaCoverage[{criterion, tests[]}], gate, checkPassed: bool, testCount: int, gaps[], issues[] }
```

## Sensors

Gate exit 0; every criterion has a test; `gaps` empty.

## Gates

To polish: always.
