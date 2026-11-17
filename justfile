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

# The cargo of the recipes outside the gate (bench, jdtls-it), with the compiler flags set here,
# not taken from the environment or a cargo config file: RUSTFLAGS and an empty RUSTC_WRAPPER beat
# `build.rustflags` and `build.rustc-wrapper` in any `.cargo/config.toml` (fnd §3.6). The gate's
# stages set the same in xtask (tools/xtask/src/catalog.rs, CARGO_ENV).
cargo := "env RUSTFLAGS='-D warnings' RUSTDOCFLAGS= RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo"

# The stages of `check`, in order, from xtask's catalog: what scripts/land.sh reads with
# `just --evaluate check_stages` (its private-names hook) until XT-05 ports it.
check_stages := shell('bash "$1/scripts/xtask.sh" list | paste -s -d " " -', justfile_directory())

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

# The hash file is required under CI=true; add a name with scripts/private-name-add.sh (owner).
private-names:
    bash {{xtask}} check-only private-names

# Fetch the pinned benchmark corpus of doc/bench-corpus.md into its cache (BENCH-01).
bench-corpus size="small" cache="":
    bash scripts/bench-corpus.sh fetch {{quote(size)}} {{ if cache != "" { "--cache " + quote(cache) } else { "" } }}

# The benchmark suites over the pinned corpus (BENCH-02, ADR 0031): JSON results with the machine's
# facts in `out`. Reports, never gates: `check` and `land.sh` do not run it (fnd §9).
# `just bench rg-floor small`, `just bench e1 medium`, `just bench path/to/suite.toml small`.
bench suite="rg-floor" corpus="small" out="target/bench": _preflight (_require "hyperfine") (_require "rg") (bench-corpus corpus)
    {{cargo}} build --locked --profile bench-tool -p jfl-bench
    # The real binaries first on PATH: a mise shim in front of rg or hyperfine adds its own launcher's
    # start-up time and memory to every timed run.
    PATH="$(dirname "$(mise which hyperfine)"):$(dirname "$(mise which rg)"):$PATH" \
      "${CARGO_TARGET_DIR:-target}/bench-tool/jfl-bench" run {{quote(suite)}} {{quote(corpus)}} --out {{quote(out)}}

# The pinned jdtls behind jfl-jdtls on the synthetic fixture (EVAL-11); needs Java 21+ and the download.
jdtls-it:
    bash scripts/jdtls-fetch.sh
    {{cargo}} nextest run -p jfl-jdtls --locked --retries 0 --features real-jdtls --test real_jdtls

# just must be the version mise.toml and scripts/bootstrap-tools.sh pin.
# Fail when a pinned tool is missing or wrong, or just is not its pinned version. It lists the
# security tools of FND-04 and fails when one is not on PATH.
doctor:
    # Non-interactive shells never activate mise, which `mise doctor` reports as a problem:
    # the shims directory on PATH is the documented alternative for such setups.
    PATH="${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims:$PATH" mise doctor
    missing="$(mise ls --missing)"; test -z "$missing"
    pinned="$(sed -n 's/^just = "\(.*\)"$/\1/p' mise.toml)"; \
      actual="$({{just_executable()}} --version | cut -d' ' -f2)"; \
      if [ -z "$pinned" ] || [ "$pinned" != "$actual" ]; then \
        echo "doctor: just is $actual but mise.toml pins '$pinned'" >&2; exit 1; fi
    for tool in gitleaks actionlint shellcheck zizmor pinact; do \
      command -v "$tool" >/dev/null 2>&1 || { echo "doctor: $tool is not on PATH: run 'mise install --locked'" >&2; exit 1; }; \
    done
    mise ls --current | grep -E 'gitleaks|actionlint|shellcheck|zizmor|pinact'

# Fail closed with mise's install hint when a pinned tool is not on PATH (fnd §8 rule 4).
_require tool:
    @command -v {{tool}} >/dev/null 2>&1 || { echo "{{tool}} is missing: run 'mise install --locked' (or bash scripts/bootstrap-tools.sh); just doctor says what else is" >&2; exit 1; }
