# The command surface (fnd §9, ADR 0035): `quick`, `check`, `check-only <stage>...`, one recipe
# per gate stage, and the tools around them; CI jobs call the same recipes (fnd §7.2). Since XT-01
# the gate's composition (the stage catalog, its order, each stage's command and the preflight) is
# tools/xtask, and the gate recipes are one-line delegations to it through scripts/xtask.sh, which
# builds xtask from outside the tree so the tree's cargo configuration cannot steer that build.
# `bash scripts/xtask.sh list --json` is the catalog. A recipe that has not landed does not exist:
# just exits non-zero naming it.
set shell := ["bash", "-euo", "pipefail", "-c"]

# xtask, built from this checkout and run with the caller's environment (scripts/xtask.sh).
xtask := quote(justfile_directory() / "scripts/xtask.sh")

# The pinned tools (cargo-nextest, cargo-deny, gitleaks, ...) by their install directories
# (`mise bin-paths`), so a stage runs the real binary and never a mise shim, which would apply a
# mise `[env]` after the preflight has looked (fnd §3.6, FND-08); then mise's shims, for a shell
# without mise's tools installed (`doctor` says what is missing). CI's mise-action installs the
# same tools. mise runs code from a trusted configuration as it loads it, so the tree's tool
# configuration passes xtask's `tool-config` (no mise) first; a refusal fails every recipe before
# mise has run (FND-09, XT-01).
mise_bins := shell('cd "$1" && bash scripts/xtask.sh tool-config "$1" && { mise bin-paths 2>/dev/null || true; } | paste -s -d : -', justfile_directory())
mise_shims := env("MISE_DATA_DIR", env("XDG_DATA_HOME", env("HOME") / ".local/share") / "mise") / "shims"
export PATH := if mise_bins == "" { mise_shims + ":" + env("PATH") } else { mise_bins + ":" + mise_shims + ":" + env("PATH") }

# The cargo of the recipes outside the gate (bench), with the compiler flags set here,
# not taken from the environment or a cargo config file: RUSTFLAGS and an empty RUSTC_WRAPPER beat
# `build.rustflags` and `build.rustc-wrapper` in any `.cargo/config.toml` (fnd §3.6). The gate's
# stages set the same in xtask (tools/xtask/src/catalog.rs, CARGO_ENV).
cargo := "env RUSTFLAGS='-D warnings' RUSTDOCFLAGS= RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo"

# The inner loop: fmt-check, architecture, clippy, unit tests and the scripts' self-tests. Not the
# gate.
quick:
    bash {{xtask}} quick

# The full blocking gate: every stage, in order; the first failure stops it with its exit code.
check:
    bash {{xtask}} check

# Run the named stages, in the order given (what each CI job runs). The names reach xtask as
# arguments, never as interpolated text, and are checked before use.
[positional-arguments]
check-only +stages:
    bash {{xtask}} check-only "$@"

# Refuse an environment that steers the build (fnd §3.6): xtask's preflight, which every xtask
# gate command runs first (tools/xtask/src/preflight.rs).
_preflight:
    bash {{xtask}} preflight

# One recipe per stage of the catalog, each the stage alone through xtask.
fmt-check:
    bash {{xtask}} check-only fmt-check

architecture:
    bash {{xtask}} check-only architecture

clippy:
    bash {{xtask}} check-only clippy

# Rewrite every crate's clippy.toml from the root list and architecture.toml's grants.
clippy-lists:
    bash scripts/clippy-lists.sh

nostd:
    bash {{xtask}} check-only nostd

unit-tests:
    bash {{xtask}} check-only unit-tests

# The tests that can differ by platform, natively on each target of ADR 0006: what the
# `linux-arm` and `macos` jobs of ci.yml run (fnd §7.2 rule 4). Not a stage of `check`: on Linux
# x86-64 the `tests` job runs the same stages, so each stage of `check` stays in one job.
platform-tests: unit-tests integration-tests doctests

integration-tests:
    bash {{xtask}} check-only integration-tests

doctests:
    bash {{xtask}} check-only doctests

script-tests:
    bash {{xtask}} check-only script-tests

# The LSP conformance level (ver §2, LSP-01): the transcripts of fixtures/lsp/ against a server
# binary over stdio (the adapter's test-only jfl-lsp-stub until `jfl lsp` exists, LSP-02).
lsp-conformance:
    bash {{xtask}} check-only lsp-conformance

# Every Quint model of formal/models.toml typechecks, and an unregistered formal/**/*.qnt fails;
# then every model's Quint tests, on the TypeScript backend (FORMAL-01, ver §6).
formal-typecheck:
    bash {{xtask}} check-only formal-typecheck

formal-test:
    bash {{xtask}} check-only formal-test

# Every committed regression trace of formal/traces/regression regenerates byte for byte at its
# seed outside its `#meta`; then seeded traces of every model (a fresh seed, printed) and the
# committed ones are replayed through the conformance drivers against the real code, with each
# driver's negative control (FORMAL-02, ver §6.2).
formal-freshness:
    bash {{xtask}} check-only formal-freshness

conformance-replay:
    bash {{xtask}} check-only conformance-replay

# Repeat a conformance-replay run: one model (or `all`), a printed seed, a budget of traces and
# steps (`just conformance smoke 0x2a 10 25`); not a stage of `check`.
[positional-arguments]
conformance *args:
    bash {{xtask}} conformance "$@"

# Apalache's bounded verification of each registered model's `allInvariants` (or the one named),
# then a Quint simulation; not a stage of `check` (a JVM per model): `formal.yml` runs it weekly
# and by hand, one job per model (FORMAL-01, ver §6.3). `just formal-verify smoke`. The JDK and
# Apalache come from `bash scripts/bootstrap-tools.sh --formal-verify`.
[positional-arguments]
formal-verify *model:
    bash {{xtask}} formal-verify "$@"

docs:
    bash {{xtask}} check-only docs

alias adr-index := docs

# Every commit of a range carries a Signed-off-by matching its author (FND-03, fnd §7.4; what
# ci.yml and scripts/land.sh run).
dco range="origin/main..HEAD":
    bash scripts/dco-check.sh {{quote(range)}}

# The DCO check over the commits a GitHub event brought (ci.yml's `dco` job): push before..head,
# pull_request base..head, anything else head~1..head (scripts/dco-check.sh --event).
dco-event event before base head:
    bash scripts/dco-check.sh --event {{quote(event)}} {{quote(before)}} {{quote(base)}} {{quote(head)}}

deny:
    bash {{xtask}} check-only deny

# A shallow clone fails the secrets stage: fetch the history in full.
secrets:
    bash {{xtask}} check-only secrets

workflows:
    bash {{xtask}} check-only workflows

# pinact's comment check: each `# vX.Y.Z` names the SHA it sits on. Needs the GitHub API (set
# GITHUB_TOKEN in CI), so it is outside `check`; ci.yml's security job runs it (FND-05).
workflows-verify:
    bash scripts/workflows-check.sh verify

# The cargo stages as CI runs them (fnd §7.2): the default target directory inside the checkout
# (so `cargo metadata` from a test's scratch directory finds this workspace) and ci.yml's
# workflow `env:` (CARGO_TERM_COLOR=always, NEXTEST_PROFILE=ci), which child processes inherit. A laptop or sandbox gate keeps its target outside the
# repository and no profile, so a test that depends on either passes here and fails in CI only;
# run this before landing a change to tests that start cargo, nextest or a scratch workspace. It
# builds a second copy into ./target. Not a stage of `check` (it doubles the build).
ci-parity:
    env -u CARGO_TARGET_DIR CARGO_TERM_COLOR=always NEXTEST_PROFILE=ci {{quote(just_executable())}} --justfile {{quote(justfile())}} --working-directory {{quote(justfile_directory())}} check-only architecture unit-tests integration-tests doctests

# zizmor's online audits (impostor commits, known-vulnerable actions, ref confusion) over the
# workflows, the local actions and dependabot.yml. Needs the GitHub API (GH_TOKEN), so it is
# outside `check`; security.yml runs it weekly (fnd §7.1).
workflows-online:
    bash scripts/workflows-check.sh online

# Fetch the pinned benchmark corpus of doc/bench-corpus.md into its cache (BENCH-01).
bench-corpus size="small" cache="":
    bash scripts/bench-corpus.sh fetch {{quote(size)}} {{ if cache != "" { "--cache " + quote(cache) } else { "" } }}

# The benchmark suites over the pinned corpus (BENCH-02, BENCH-03, ADR 0031): JSON results with the
# machine's facts in `out`. Reports, never gates: `check` and `land.sh` do not run it (fnd §9).
# `just bench rg-floor small`, `just bench e1 medium`, `just bench incremental lombok`,
# `just bench kmp-lsp medium`, `just bench jdtls fixture`, `just bench path/to/suite.toml small`.
# Fetches what the suite needs first: the corpus (and its commit pairs for `incremental`), the
# pinned kmp-lsp build or the pinned jdtls; `fixture` is the synthetic project in tools/jfl-bench.
bench suite="rg-floor" corpus="small" out="target/bench": _preflight (_require "hyperfine") (_require "rg")
    #!/usr/bin/env bash
    set -euo pipefail
    suite={{quote(suite)}}
    corpus={{quote(corpus)}}
    if [ "$corpus" != fixture ]; then
      bash scripts/bench-corpus.sh fetch "$corpus"
      if [ "$suite" = incremental ]; then bash scripts/bench-corpus.sh fetch-pairs "$corpus"; fi
    fi
    packages=(-p jfl-bench)
    case "$suite" in
      kmp-lsp) bash scripts/kmp-lsp-fetch.sh ;;
      jdtls) bash scripts/jdtls-fetch.sh ;;
    esac
    {{cargo}} build --locked --profile bench-tool "${packages[@]}"
    # The real binaries first on PATH: a mise shim in front of rg or hyperfine adds its own launcher's
    # start-up time and memory to every timed run.
    PATH="$(dirname "$(mise which hyperfine)"):$(dirname "$(mise which rg)"):$PATH" \
      "${CARGO_TARGET_DIR:-target}/bench-tool/jfl-bench" run "$suite" "$corpus" --out {{quote(out)}}

# jfl-domain's in-process position benchmark (IDX-01, divan): synthetic text, then every tracked
# `.java` file of the pinned corpus, streamed on stdin with a NUL after each (jfl-domain holds no
# filesystem grant), after checking that every char boundary of every file round-trips and agrees
# with line-index. Reports, never gates. `just bench-domain small`.
bench-domain corpus="small": _preflight
    #!/usr/bin/env bash
    set -euo pipefail
    bash scripts/bench-corpus.sh fetch {{quote(corpus)}}
    root="$(bash scripts/bench-corpus.sh path {{quote(corpus)}})"
    git -C "$root" ls-files -z -- '*.java' \
      | while IFS= read -r -d '' file; do cat -- "$root/$file"; printf '\0'; done \
      | {{cargo}} bench --locked -p jfl-domain --bench positions

# jfl-process's spawn overhead against std's Command on the same child (IDX-04): median and p95
# per arm, printed; with a corpus size, also the git adapter's two reconcile calls in that
# corpus's checkout. `just bench-process`, `just bench-process small`. Reports, never gates.
bench-process corpus="": _preflight
    #!/usr/bin/env bash
    set -euo pipefail
    repo=()
    if [ -n {{quote(corpus)}} ]; then
      bash scripts/bench-corpus.sh fetch {{quote(corpus)}}
      repo=("$(bash scripts/bench-corpus.sh path {{quote(corpus)}})")
    fi
    # ${repo[@]+...}: an empty array under `set -u` is an error in bash 3.2 (macOS).
    {{cargo}} bench --locked -p jfl-process --bench spawn -- ${repo[@]+"${repo[@]}"}

# jfl-domain's in-process envelope, completeness and summary benchmark (IDX-02, divan): synthetic,
# deterministic inputs sized from the corpus (doc/bench/README.md); prints a typical summary's bytes
# first. Reports, never gates. `just bench-domain-envelope`.
bench-domain-envelope: _preflight
    {{cargo}} bench --locked -p jfl-domain --bench envelope

# jfl-protocol's in-process benchmark (IDX-03, divan): a `jfl/references` response of 1,000 and
# 10,000 locations encoded from the domain envelope, its frame decoded from 64 KiB reads and its
# body decoded; synthetic, deterministic inputs (doc/bench/README.md); prints each response's
# bytes first. Reports, never gates. `just bench-protocol`.
bench-protocol: _preflight
    {{cargo}} bench --locked -p jfl-protocol --bench protocol

# jfl-adapter-lsp's in-process benchmark (LSP-01, divan): one request's round trip through a session
# over in-memory pipes (framing, dispatch and the writer, on the main loop and on the pool), and a
# 1,000- and 10,000-location `jfl/references` answer turned into the LSP result, on the typed path and
# through a `serde_json::Value`; synthetic, deterministic inputs (doc/bench/README.md). Reports, never
# gates. `just bench-lsp`.
bench-lsp: _preflight
    {{cargo}} bench --locked -p jfl-adapter-lsp --bench lsp

# jfl-adapter-fs's walk in process (IDX-06, divan): `list` with paths and with a stat per file on 1
# and 4 walker threads, warm, and a watch's start, on the pinned corpus's checkout (its path and
# `git ls-files` on stdin, so the benchmark first checks that the listing is Git's file set). With
# `cold`, also cold runs, which drop the whole machine's page cache before each sample (Linux,
# root): off by default, since they disturb everything else running. Reports, never gates.
# `just bench-fs small`, `just bench-fs medium cold`.
bench-fs corpus="small" cold="warm": _preflight
    #!/usr/bin/env bash
    set -euo pipefail
    case {{quote(cold)}} in cold|warm) ;; *) echo "bench-fs: cold must be cold or warm" >&2; exit 2;; esac
    bash scripts/bench-corpus.sh fetch {{quote(corpus)}}
    root="$(bash scripts/bench-corpus.sh path {{quote(corpus)}})"
    { printf '%s\n%s\n' {{quote(cold)}} "$root"; git -C "$root" ls-files -z -- '*.java'; } \
      | {{cargo}} bench --locked -p jfl-adapter-fs --bench walk

# jfl-adapter-git's cost per call in process (IDX-05, divan): `resolve` cold and cached, the stat
# set alone, and each `git` command of the reconcile path (`rev-parse`, `diff --name-only` between
# two pinned trees 100 files apart, `status -z`, `ls-files -z`) on the pinned corpus's checkout
# (its path and `git ls-files` on stdin, so the benchmark first checks the adapter's tracked files
# against Git's). The corpus is only read: the second tree is made in a shared clone under cargo's
# scratch directory. Reports, never gates. `just bench-git small`, `just bench-git medium`.
bench-git corpus="small": _preflight
    #!/usr/bin/env bash
    set -euo pipefail
    bash scripts/bench-corpus.sh fetch {{quote(corpus)}}
    root="$(bash scripts/bench-corpus.sh path {{quote(corpus)}})"
    { printf '%s\n' "$root"; git -C "$root" ls-files -z -- '*.java'; } \
      | {{cargo}} bench --locked -p jfl-adapter-git --bench probe

# The syntax index in process (IDX-07): every tracked `.java` file of the pinned corpus read into
# memory, then indexed by jfl-adapter-java on 1 and 4 threads (files/s, MB/s, per-file p50, p95 and
# max, wall time, summary bytes per file). In jfl-composition, which holds the clock and spawn
# grants the adapter does not. Reports, never gates. `just bench-syntax medium`.
bench-syntax corpus="small": _preflight
    #!/usr/bin/env bash
    set -euo pipefail
    bash scripts/bench-corpus.sh fetch {{quote(corpus)}}
    root="$(bash scripts/bench-corpus.sh path {{quote(corpus)}})"
    { printf '%s\n' "$root"; git -C "$root" ls-files -z -- '*.java'; } \
      | {{cargo}} bench --locked -p jfl-composition --bench syntax

# The summary store in process (IDX-08): every tracked `.java` file of the pinned corpus summarised
# by jfl-adapter-java, then jfl-adapter-store measured in a scratch directory: encoded bytes per
# file, a full write on 1 and 4 threads, the open and the warm load of the whole view (whole
# summaries, and declarations plus the name postings), the postings' size and lookup time, a
# refs-shaped read, and both tiers' memory projected to elasticsearch. With `cold`, also cold
# loads, which drop the whole machine's page cache before each (Linux, root): off by default, since
# they disturb everything else running. Reports, never gates. `just bench-store medium [cold]`.
bench-store corpus="small" cold="warm": _preflight
    #!/usr/bin/env bash
    set -euo pipefail
    case {{quote(cold)}} in cold|warm) ;; *) echo "bench-store: cold must be cold or warm" >&2; exit 2;; esac
    bash scripts/bench-corpus.sh fetch {{quote(corpus)}}
    root="$(bash scripts/bench-corpus.sh path {{quote(corpus)}})"
    { printf '%s\n%s\n' {{quote(cold)}} "$root"; git -C "$root" ls-files -z -- '*.java'; } \
      | {{cargo}} bench --locked -p jfl-composition --bench store

# Rewrite fixtures.lock from the tree after reviewing the changed goldens (ver §4,
# ARCH-fixtures-lock): the SHA-256 of every file under fixtures/ and each crate's tests/fixtures/.
fixtures-lock:
    bash scripts/fixtures-lock.sh

# Every tool of tools/versions.toml on the stages' PATH at its pinned version (xtask's cache first,
# then mise's install directories), and versions.toml agreeing with mise.toml and mise.lock;
# fails naming each missing or wrong tool and the fix (XT-02, fnd §8).
doctor:
    bash {{xtask}} doctor

# The pinned tools of tools/versions.toml (all, or the ones named), each release asset checked
# against its SHA-256, into the tool cache outside the repository that the stages' PATH puts
# first (XT-02, fnd §8). mise.lock's `mise install --locked` installs the same assets until XT-08.
[positional-arguments]
install-tools *tools:
    bash {{xtask}} install-tools "$@"

# Fail closed with mise's install hint when a pinned tool is not on PATH (fnd §8 rule 4).
_require tool:
    @command -v {{tool}} >/dev/null 2>&1 || { echo "{{tool}} is missing: run 'mise install --locked' (or bash scripts/bootstrap-tools.sh); just doctor says what else is" >&2; exit 1; }
