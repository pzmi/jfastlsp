# plan

Dispatch: inline

## Inputs

intake.{taskId, criteria, ownerCriteria, cites, affectedCrates, authority, invariants, modelImpact, protocolImpact, contractSuites, process}

## Steps

1. **Existing plan?** If `.work/<id>/plan.md` exists, reuse it and honour ticked items (resume).
2. **Explore** only what the task names: the cited sections, the files it modifies, and the nearest existing pattern (a sibling crate or module, an existing test of the same level, an existing contract suite, golden fixture or LSP transcript).
3. **Write the checklist** to `.work/<id>/plan.md`: one line per step, `- [ ] path: what`, each traceable to an `Accept:` clause or a `CLAUDE.md` rule. It must cover, where relevant:
   - **the crate each item belongs in** per plan §6 and fnd §2: pure crates (`jfl-domain`, `jfl-protocol`) hold types, positions, keys, the envelope and policies with no I/O, Tokio, clock, randomness or env; `jfl-application` holds use cases, the view registry and port traits; adapters translate one technology and never name a sibling adapter or composition; `jfl-process` is the only crate (with composition) that spawns a child; composition is the only wiring; the CLI only parses, prints and delegates
   - **crate-scoped steps**: each step inside one crate; a port another crate consumes, or composition wiring, is its own step
   - **a new crate or dependency**: workspace `members` entry, `architecture.toml` entry and external allowlist in the same commit, `[lints] workspace = true`
   - **model first** (ver §6) when `modelImpact` is not `none`: the Quint model and invariants, named scenarios, `MODELS` registration, the conformance driver's observation, then the Rust code; plus a negative control
   - **protocol first** when `protocolImpact` is not `none`: the golden JSON (protocol message, `--json` output, LSP transcript) written and reviewed before the code that produces it; additive change only (plan §6.6)
   - **untrusted input**: every parser of Java source, class files, JARs, JSON-RPC frames, protocol frames or Gradle output is bounded (size, depth, count), never panics, and gets a fuzz target or a property test (ver §8)
   - **the envelope**: where `exact` is claimed, the evidence; where `possible` is filled, why it is a sound superset; where `complete=true` is returned, which check proves every visible module is indexed and fresh (plan §2.1). `complete=true` is never wrong
   - **the test levels** touched (ver §2): unit, property, golden, component, contract, LSP conformance, integration, e2e, architecture, conformance, differential; one test per `Accept:` clause at least, and each contract suite in `contractSuites` run against every implementation the task names, with a deliberately broken one
   - **performance**: a claim about latency, memory or throughput names the benchmark that measures it (ver §10); no budget is set from a guess
   - **new dependencies**: `cargo add`, why this crate, its licence, and whether `deny.toml` accepts it
   - **ADRs** (ADR 0001, amended): only an important decision whose context is worth recalling later gets a step that runs `adrs new "<title>"` (or copies the template to the reserved number) and `just adr-index`; a step that reverses a recorded decision amends its ADR or marks it `Superseded by NNNN`. Any other decision is explained in the commit message and, where the constraint is not obvious, a code comment; list it so ship writes it
   - **`(owner)` criteria**: what is delivered up to them (runbook, harness, gated test, docs) and left pending
4. **Owner question.** LARGE with a genuine design fork: one short question with a recommended default, then continue on the answer. No fork: no question.
5. Write `plan.json`.

## Output

```
{ planPath, checklist[], testLevels[], newDependencies[], adrs[], issues[] }
```

## Sensors

`plan.md` exists and has at least one item per `Accept:` clause; `newDependencies` empty or each justified; a model-first item precedes the code when `modelImpact` is not `none`; a golden item precedes the code when `protocolImpact` is not `none`.

## Gates

To implement: always.
