# Contributing

Thanks for your interest. The project is pre-alpha and still in planning; see
[doc/tasks.md](doc/tasks.md) for the current work.

Planned work is done by the maintainer and coding agents as one signed-off commit per task,
directly on `main` ([doc/sdlc.md](doc/sdlc.md)). Outside contributions are welcome as pull
requests; for anything larger than a small fix, open an issue first so the design can be agreed
before you write code. The maintainer integrates an accepted pull request as a signed-off commit.

## Sign off your commits (DCO)

Sign off every commit under the [Developer Certificate of Origin](https://developercertificate.org/):

```text
git commit -s
```

That adds a `Signed-off-by:` line with your name and email. Use short imperative subjects, with a
body that explains why when it is not obvious.

## Build and test

Once the workspace exists, the Rust toolchain is pinned in `rust-toolchain.toml` (`rustup`
installs it on first use) and the other tools the gate needs are pinned in `mise.toml` and
`mise.lock` ([mise](https://github.com/jdx/mise)):

```text
mise install --locked       # installs every pinned tool
just doctor                 # checks them; run it first when local and CI disagree
just quick                  # fmt check, architecture, clippy, fast tests: while working
just check                  # the full blocking gate: before every commit
just bench                  # benchmarks on the pinned corpus (`just bench-corpus` fetches it)
```

`quick` is not `check`; a pull request says which stages it ran. `just check-only <stage>...`
runs named stages through the same recipes CI runs. Speed is the product's main value: a change
to indexing or queries reports what it does to the benchmarks.

## Architecture and decisions

Read [doc/plan.md](doc/plan.md) (crates and the dependency law are §6). Crate boundaries, the
purity of the domain crates and which crates may spawn processes or open sockets are enforced by
the compiler (`#![no_std]` domain crates), `cargo-deny`, clippy and an architecture test
(`just check-only architecture`); fix the structure instead of loosening a lint or a test. A change
that adds or reverses an architectural decision adds an ADR or amends or supersedes one, in the
same change, together with the tests that pin it.

## Pull requests

- Do not suppress, skip or delete tests or lints to get green; fix the cause.
- Add tests for behaviour changes, named `should_<outcome>_when_<condition>` with `// given`,
  `// when` and `// then` sections.
- Keep a pull request to one change, and do not include secrets, personal paths or code from
  other projects: Java fixtures are synthetic.
- New dependencies are added with `cargo add` and need a reason in the description.

## Reporting problems

Security issues must be reported privately; see [SECURITY.md](SECURITY.md).

## Licensing

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as
`MIT OR Apache-2.0`, without any additional terms or conditions.

By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
