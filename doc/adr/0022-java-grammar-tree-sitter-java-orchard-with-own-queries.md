# 0022. Java grammar: tree-sitter-java-orchard with our own queries

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-17); amended 2026-11-17 (owner).

## Context

The syntax index parses every Java file with tree-sitter (plan §5.1, IDX-07). Upstream
[`tree-sitter-java`](https://github.com/tree-sitter/tree-sitter-java) 0.23.5 has had no grammar
commit since December 2024 and lacks JEP 511 `import module java.base;` (final in Java 25), so
every such line becomes an ERROR node. It ships only a thin `tags.scm` (no record, enum, field,
constructor, annotation type or package) and no `locals.scm`. Survey of 2026-11-16, read from
the published crate sources.

## Decision

- **`tree-sitter` 0.27** (MIT) with
  **[`tree-sitter-java-orchard`](https://codeberg.org/grammar-orchard/tree-sitter-java-orchard)
  0.5** (0.5.25 on 2026-11-15, MIT): an active fork (27 releases) that adds `import module`,
  reworks string and text-block tokens and fixes numeric-literal edge cases.
- **Our own `tags.scm` and `locals.scm`**, run with `tree_sitter::Query`/`QueryCursor` into our
  own types (not `tree-sitter-tags`, which hides ranges and scopes). `locals.scm` starts from
  nvim-treesitter's Java query (Apache-2.0, copied with attribution); Helix's (MPL-2.0) is
  reference only.
- The grammar crate's name and version are part of `SyntaxKey` (plan §5.4).
  `template_expression` (string templates, preview in Java 21–22, withdrawn in 23) is treated as
  a syntax error.
- **Measured** (E1 spike, 2026-11-16, 4 vCPU Xeon 2.3 GHz VM, plan §1.7): orchard parsed every
  file of junit-framework, spring-framework and elasticsearch; its 14 error files in kafka and
  gradle are real non-Java (templates with `${...}` placeholders, a deliberately broken file).
  Upstream 0.23.5 marked 0.33 % of junit-framework (Java 25 `import module`, compact source
  files) and 0.62 % of spring-framework (type annotations on varargs) as broken. Both cost the
  same to within 3 %. EVAL-05 re-measures the parse-error rate on CI hardware and the target.

## Consequences

The grammar compiles C through `cc` (a build script allowed by name in `deny.toml`). Node names
mostly match upstream, but literal nodes differ, so queries are written against orchard. The
fork has one main committer: if it stalls, switching back to upstream is a grammar-version change
with a re-run of the corpus goldens. IDX-07 implements this; EVAL-05 measures it.

## Alternatives considered

- Upstream `tree-sitter-java` 0.23.5: larger user base, but dormant and missing Java 25 syntax;
  the runner-up.
- `java-lang` 0.6 (a typed Java AST): 1.4k downloads, weekly churn, no incremental reparse.

## Amended 2026-11-17 (owner): pinned exactly, with the switch-back boundary built from the start

The owner kept tree-sitter-java-orchard, pinned to an exact version (`=0.5.x` in the manifest,
the version IDX-07 finds current), and asked for the way back to upstream to be built in from the
first commit rather than left to "a grammar-version change" (Consequences above):

- **Maintainers.** The fork has about three maintainers, not one; the stall risk is smaller than
  stated above, but still a risk.
- **One crate knows node names.** Only `jfl-adapter-java` names grammar nodes and fields; every
  other crate sees our own summary types.
- **Per-grammar queries.** `tags.scm` and `locals.scm` exist per grammar (orchard and upstream),
  so a switch changes which set is loaded, not the queries in place.
- **Names resolved at start-up.** Every node kind and field name the adapter uses is resolved
  against the loaded grammar into a table when the indexer starts (`Language::id_for_node_kind`,
  `field_id_for_name`); a missing name fails loudly, naming it, before any file is parsed, never
  a silent empty match.
- **Both grammars in tests and CI.** A synthetic per-feature Java corpus (one small file per
  language feature: records, sealed types, switch patterns, text blocks, `import module`,
  flexible constructor bodies, annotations in every position) is parsed by both grammars, and
  each grammar's summaries are compared with its golden summaries. Where upstream cannot parse a
  feature, the gap is listed in a committed known-gaps file, and a gap that closes or opens fails
  the test until the list changes.
- **The grammar in the cache key.** The grammar crate's name and exact version stay part of
  `SyntaxKey` (plan §5.4), so a switch re-parses everything.
- **Switch triggers.** Move to upstream `tree-sitter-java` when it releases 0.24 or later with
  `import module` and flexible constructor bodies and no more false errors than orchard on the
  bench corpus; or when orchard has no release for 6 months while a Java feature we need goes
  final.

IDX-07 builds this (about 1–2 days on top of its card); EVAL-05 keeps measuring both grammars.

## Amended 2026-11-18 (IDX-07; proposed, for the owner's next checkpoint): the queries as a table

IDX-07 built the boundary above. Run with `QueryCursor`, our queries made indexing 2.3 times as
slow as the E1 pipeline: tree-sitter's query engine cost half a parse again on the corpus
(callgrind over 300 junit-framework files: the parse 4.09 G instructions, the query engine 2.26 G,
our extraction 0.65 G), and nested patterns kept match states open across whole method bodies.
spring-framework's full index at 4 threads took 3.1 s on a loaded machine, past plan §1.7's 2.5 s.

- **Single-node patterns only.** `tags.scm` and `locals.scm` hold `(kind) @capture` patterns and
  nothing else; the adapter reads names and fields of a captured node through the start-up table.
- **Compiled, then read as a table.** `Query::new` still compiles both against the loaded grammar,
  so a node name the grammar lacks fails start-up naming it; then the adapter reads the patterns
  as a table from node kind to captures and applies it in one pre-order cursor walk, the cost of
  E1's own walk. A pattern of any other shape fails start-up, naming it.
- **Equivalence** is pinned by the goldens: every summary of both grammars stayed byte for byte
  the same when the engine was replaced (`crates/jfl-adapter-java/tests/goldens.rs`).
- **Result:** about 1.5 times the parse's instructions, down from 2.0. On a loaded machine, in
  single runs beside E1's own extract stage, spring-framework took 1.26 times E1's time at 1
  thread and 1.18 times at 4 (`doc/bench/README.md`).

What stays: our own queries, our own types, per-grammar query files, and `tree-sitter-tags`
rejected. What changes: "run with `tree_sitter::Query`/`QueryCursor`" above now reads "compiled
with `tree_sitter::Query`, applied by the adapter's walk". A future need for a structural pattern
(a child under a parent) is a field read in Rust, or an amendment here.
