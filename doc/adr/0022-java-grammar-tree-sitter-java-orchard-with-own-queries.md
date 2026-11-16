# 0022. Java grammar: tree-sitter-java-orchard with our own queries

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07).

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
