<!-- For outside contributors: planned work lands as one signed-off commit per task on main
(doc/sdlc.md); the maintainer integrates an accepted pull request the same way. -->

## What and why

<!-- What changes, and why. Link the issue for anything larger than a small fix. -->

## How it was checked

- [ ] `just check` is green (or say which stages ran: `quick` is not `check`)
- [ ] tests named `should_<outcome>_when_<condition>`, with `// given`, `// when`, `// then`
- [ ] no test, lint, budget or policy loosened to get green
- [ ] every commit signed off (`git commit -s`, the DCO)

## Dependencies and decisions

<!-- A new dependency: added with `cargo add`, and why this crate. A new or reversed
architectural decision: the ADR it adds or amends. "None" otherwise. -->

## Speed

<!-- For a change to indexing or queries: what it does to the benchmarks (`just bench`). -->
