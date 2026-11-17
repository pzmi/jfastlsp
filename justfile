# The command surface (fnd §9, ADR 0030): one recipe per gate stage, `quick`, `check` and
# `check-only <stage>...`; CI jobs call the same recipes (fnd §7.2). A recipe that has not landed
# does not exist: just exits non-zero naming it. `just --list` and
# `just --dump --dump-format json` are the catalog.
set shell := ["bash", "-euo", "pipefail", "-c"]

# mise's shims, so a recipe finds the pinned tools (cargo-nextest, cargo-deny) in a shell that
# never activated mise; CI's mise-action puts the same tools on PATH.
export PATH := env("MISE_DATA_DIR", env("XDG_DATA_HOME", env("HOME") / ".local/share") / "mise") / "shims" + ":" + env("PATH")

# Every cargo of the gate runs with the compiler flags set here, not taken from the environment or
# from a cargo config file: RUSTFLAGS and an empty RUSTC_WRAPPER beat `build.rustflags` and
# `build.rustc-wrapper` in any `.cargo/config.toml` (fnd §3.6). `_preflight` still refuses such a
# config and such variables, so a steered environment fails loudly rather than being overridden
# silently. Pinned by tools/jfl-architecture/tests/gate_negative_cases.rs.
cargo := "env RUSTFLAGS='-D warnings' RUSTDOCFLAGS= RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo"

# The stages of `quick` and `check`, in fnd §9's order. A stage enters `check` in the commit of
# the task that introduces it (`architecture` with FND-07, `docs` with FND-03, ...).
quick_stages := "fmt-check architecture clippy unit-tests script-tests"
check_stages := "fmt-check architecture clippy nostd docs unit-tests integration-tests doctests script-tests deny secrets workflows private-names"

# The inner loop: fmt-check, architecture, clippy, unit tests and the scripts' self-tests. Not the
# gate.
quick: (check-only quick_stages)

# The full blocking gate: every stage, in order; the first failure stops it with its exit code.
check: (check-only check_stages)

# Run the named stages through their recipes, in the order given (what each CI job runs). The
# names reach the shell as arguments, never as interpolated text, and are checked before use.
[positional-arguments]
check-only +stages:
    #!/usr/bin/env bash
    set -euo pipefail -o noglob
    valid=" {{check_stages}} "
    names=()
    for arg in "$@"; do
      for stage in $arg; do
        case "$valid" in *" $stage "*) names+=("$stage") ;; *)
          echo "check-only: unknown stage '$stage'; valid stages: {{check_stages}}" >&2; exit 2 ;;
        esac
      done
    done
    exec {{quote(just_executable())}} --justfile {{quote(justfile())}} --working-directory "$PWD" "${names[@]}"

# Refuse an environment that steers the build (fnd §3.6); every stage depends on it. Variables
# of cargo, rustc, rustup, clippy and nextest are refused unless allowlisted here, and so is a
# cargo config file, in the working directory's ancestors or in CARGO_HOME, that sets compiler
# flags, a wrapper, a runner, a linker, a profile, an alias, `[env]` or unstable features.
_preflight:
    #!/usr/bin/env bash
    set -euo pipefail
    refused=()
    pinned="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\(.*\)"$/\1/p' {{quote(justfile_directory() / "rust-toolchain.toml")}})"
    [ -n "$pinned" ] || { echo "preflight: no channel in rust-toolchain.toml" >&2; exit 1; }
    while IFS= read -r name; do
      case "$name" in
        CARGO_TARGET_DIR | CARGO_BUILD_JOBS | CARGO_INCREMENTAL | CARGO_HOME | CARGO_TERM_* | \
          CARGO_NET_* | CARGO_HTTP_* | CARGO_REGISTRIES_CRATES_IO_PROTOCOL | RUSTUP_HOME | \
          RUSTUP_DIST_SERVER | RUSTUP_UPDATE_ROOT | RUSTUP_MAX_RETRIES | RUSTUP_IO_THREADS) ;;
        RUSTUP_TOOLCHAIN)
          case "$RUSTUP_TOOLCHAIN" in
            "$pinned" | "$pinned-x86_64-unknown-linux-gnu" | "$pinned-aarch64-unknown-linux-gnu" | \
              "$pinned-aarch64-apple-darwin") ;;
            *) refused+=("$name") ;;
          esac ;;
        RUSTFLAGS)
          case "$RUSTFLAGS" in "" | "-D warnings") ;; *) refused+=("$name") ;; esac ;;
        NEXTEST_PROFILE)
          case "$NEXTEST_PROFILE" in default | ci | serial) ;; *) refused+=("$name") ;; esac ;;
        CARGO | CARGO_* | __CARGO_* | RUSTC | RUSTC_* | RUSTDOC | RUSTDOC_* | RUSTDOCFLAGS | RUSTUP_* | \
          CLIPPY_* | NEXTEST_* | PROPTEST_*)
          refused+=("$name") ;;
      esac
    done < <(compgen -e)
    # Cargo reads .cargo/config{,.toml} in every ancestor of the working directory and in
    # CARGO_HOME; a steering key in any of them is refused: a steering table header, a steering
    # top-level key in dotted or inline-table form (`profile = { dev = ... }`), quoted (basic or
    # literal) or not,
    # and a steering key anywhere on a line (`build = { rustflags = [...] }`).
    table='(alias|env|profile|target|unstable)'
    key='(rustflags|rustdocflags|rustc-wrapper|rustc-workspace-wrapper|rustc|rustdoc|target|runner|linker)'
    q="[\"']?"
    steering="^[[:space:]]*\\[[[:space:]]*$q$table$q[[:space:]]*([].]|\$)|^[[:space:]]*$q$table$q[[:space:]]*[.=]|(^|[[:space:]{,.\"'])$key$q[[:space:]]*="
    configs=()
    dir="$PWD"
    while :; do
      configs+=("$dir/.cargo/config" "$dir/.cargo/config.toml")
      [ "$dir" = / ] && break
      dir="$(dirname "$dir")"
    done
    home="${CARGO_HOME:-$HOME/.cargo}"
    configs+=("$home/config" "$home/config.toml")
    for file in "${configs[@]}"; do
      if [ -f "$file" ] && grep -Eq "$steering" "$file"; then refused+=("$file"); fi
    done
    if [ "${#refused[@]}" -gt 0 ]; then
      echo "preflight: refusing to run the gate: ${refused[*]} steers the build (fnd §3.6); unset or remove it" >&2
      exit 1
    fi

# Formatting, checked, never applied.
fmt-check: _preflight
    {{cargo}} fmt --all --check

# The architecture test (fnd §3.2, FND-07): the dependency law over `cargo metadata` for each
# target of ADR 0006, the clippy lists against `[grants]` and the source scan, over the tree
# (tests/architecture.rs, with ARCH-ci-lockstep over ci.yml); plus the checker's own table-driven
# negative cases and the meta-test (tests/rules.rs) and the lockstep's (tests/ci_lockstep.rs). The
# fixture workspaces run in `integration-tests`.
architecture: _preflight (_require "cargo-nextest")
    {{cargo}} nextest run --locked --retries 0 --no-tests fail -p jfl-architecture --test architecture --test rules --test ci_lockstep

# Clippy with the authority lists (fnd §2.4): the per-crate lists must match the root one first,
# and a list entry naming a path that does not exist fails the stage (clippy only warns, and
# `-D warnings` does not reach configuration warnings).
clippy: _preflight
    #!/usr/bin/env bash
    set -euo pipefail
    bash {{quote(justfile_directory() / "scripts/clippy-lists.sh")}} --check
    log="${CARGO_TARGET_DIR:-target}/clippy-stage.log"
    mkdir -p "$(dirname "$log")"
    {{cargo}} clippy --workspace --all-targets --locked -- -D warnings 2>&1 | tee "$log"
    if grep -q "does not refer to a" "$log"; then
      echo "clippy: a clippy.toml entry names a path that does not exist (above); fix it or, for a crate absent from the graph, add allow-invalid" >&2
      exit 1
    fi

# Rewrite every crate's clippy.toml from the root list and architecture.toml's grants.
clippy-lists:
    bash scripts/clippy-lists.sh

# Build every pure crate of architecture.toml for a target with no std (fnd §2.3, ARCH-domain-purity).
nostd: _preflight
    #!/usr/bin/env bash
    set -euo pipefail
    line="$(sed -n 's/^pure[[:space:]]*=[[:space:]]*\[\(.*\)\][[:space:]]*$/\1/p' architecture.toml)"
    crates="$(tr -d '" ' <<<"$line" | tr ',' '\n' | sed '/^$/d')"
    if [ -z "$crates" ]; then echo "nostd: architecture.toml names no pure crate" >&2; exit 1; fi
    args=()
    for crate in $crates; do args+=(-p "$crate"); done
    {{cargo}} build --locked "${args[@]}" --target x86_64-unknown-none

# Unit and property tests: the lib and bin targets, under nextest (CI picks its profile with
# NEXTEST_PROFILE=ci; stages take no parameters, so `just a b` runs both). `--retries 0` and
# `--no-tests fail` on the command line beat any configuration or variable.
unit-tests: _preflight (_require "cargo-nextest")
    {{cargo}} nextest run --workspace --locked --retries 0 --no-tests fail --lib --bins

# The tests that can differ by platform, natively on each target of ADR 0006: what the
# `linux-arm` and `macos` jobs of ci.yml run (fnd §7.2 rule 4). Not a stage of `check`: on Linux
# x86-64 the `tests` job runs the same stages, so each stage of `check` stays in one job.
platform-tests: unit-tests integration-tests doctests

# Integration, component and contract tests: every tests/ target, under nextest.
integration-tests: _preflight (_require "cargo-nextest")
    {{cargo}} nextest run --workspace --locked --retries 0 --no-tests fail --test '*'

# Doctests (nextest does not run them).
doctests: _preflight
    {{cargo}} test --workspace --locked --doc

# The scripts' own offline self-tests (scripts/test-*.sh), e.g. the bench corpus fetch.
script-tests: _preflight
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    tests=(scripts/test-*.sh)
    if [ "${#tests[@]}" -eq 0 ]; then echo "script-tests: no scripts/test-*.sh" >&2; exit 1; fi
    for t in "${tests[@]}"; do echo "script-tests: $t" >&2; bash "$t"; done

# The ADR index of doc/adr/README.md agrees with the records (FND-03; scripts/adr-index.sh, which
# does what `adrs generate toc` cannot: our status annotations). Needs only bash and awk.
docs: _preflight
    bash scripts/adr-index.sh check doc/adr

alias adr-index := docs

# Every commit of a range carries a Signed-off-by matching its author (FND-03, fnd §7.4; what
# ci.yml and scripts/land.sh run).
dco range="origin/main..HEAD":
    bash scripts/dco-check.sh {{quote(range)}}

# The DCO check over the commits a GitHub event brought (ci.yml's `dco` job): push before..head,
# pull_request base..head, anything else head~1..head (scripts/dco-check.sh --event).
dco-event event before base head:
    bash scripts/dco-check.sh --event {{quote(event)}} {{quote(before)}} {{quote(base)}} {{quote(head)}}

# Advisories, licences, sources and bans (fnd §2.5, §7.4).
deny: _preflight (_require "cargo-deny")
    {{cargo}} deny --locked check

# gitleaks over the full history, default rules, nothing allowlisted (FND-04, fnd §7.4;
# scripts/secrets-check.sh). A shallow clone fails: fetch the history in full.
secrets: _preflight
    bash scripts/secrets-check.sh

# The static checks of GitHub Actions files and shell scripts, offline (FND-04, fnd §7.3;
# scripts/workflows-check.sh): actionlint with shellcheck, zizmor's offline audits with the
# hash-pin policy, pinact's syntactic pin check, then shellcheck over scripts/ and the hooks.
# A tree without workflow files passes with a notice.
workflows: _preflight
    bash scripts/workflows-check.sh workflows
    bash scripts/workflows-check.sh shell

# pinact's comment check: each `# vX.Y.Z` names the SHA it sits on. Needs the GitHub API (set
# GITHUB_TOKEN in CI), so it is outside `check`; ci.yml's security job runs it (FND-05).
workflows-verify:
    bash scripts/workflows-check.sh verify

# zizmor's online audits (impostor commits, known-vulnerable actions, ref confusion) over the
# workflows, the local actions and dependabot.yml. Needs the GitHub API (GH_TOKEN), so it is
# outside `check`; security.yml runs it weekly (fnd §7.1).
workflows-online:
    bash scripts/workflows-check.sh online

# No name whose salted hash is in .private-names.sha256 in the lines this change adds, the
# untracked files or the commit messages since `git merge-base HEAD origin/main` (FND-04, fnd
# §7.4; scripts/private-names-check.sh). The hash file is required under CI=true; add a name
# with scripts/private-name-add.sh (owner).
private-names: _preflight
    bash scripts/private-names-check.sh

# Fetch the pinned benchmark corpus of doc/bench-corpus.md into its cache (BENCH-01).
bench-corpus size="small" cache="":
    bash scripts/bench-corpus.sh fetch {{quote(size)}} {{ if cache != "" { "--cache " + quote(cache) } else { "" } }}

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
