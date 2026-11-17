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
quick_stages := "fmt-check clippy unit-tests script-tests"
check_stages := "fmt-check clippy nostd unit-tests integration-tests doctests script-tests deny"

# The inner loop: fmt-check, clippy, unit tests and the scripts' self-tests. Not the gate.
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
          case "$RUSTUP_TOOLCHAIN" in "$pinned" | "$pinned"-*) ;; *) refused+=("$name") ;; esac ;;
        RUSTFLAGS)
          case "$RUSTFLAGS" in "" | "-D warnings") ;; *) refused+=("$name") ;; esac ;;
        NEXTEST_PROFILE)
          case "$NEXTEST_PROFILE" in default | ci | serial) ;; *) refused+=("$name") ;; esac ;;
        CARGO | CARGO_* | RUSTC | RUSTC_* | RUSTDOC | RUSTDOC_* | RUSTDOCFLAGS | RUSTUP_* | CLIPPY_* | NEXTEST_*)
          refused+=("$name") ;;
      esac
    done < <(compgen -e)
    # Cargo reads .cargo/config{,.toml} in every ancestor of the working directory and in
    # CARGO_HOME; a steering key in any of them is refused.
    steering='^[[:space:]]*(\[(alias|env|profile|target|unstable)([].]|$)|(build\.|target\.[^=]*\.)?(rustflags|rustdocflags|rustc-wrapper|rustc-workspace-wrapper|rustc|rustdoc|target|runner|linker)[[:space:]]*=|(alias|env|profile|unstable)\.)'
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

# Advisories, licences, sources and bans (fnd §2.5, §7.4).
deny: _preflight (_require "cargo-deny")
    {{cargo}} deny --locked check

# Fetch the pinned benchmark corpus of doc/bench-corpus.md into its cache (BENCH-01).
bench-corpus size="small" cache="":
    bash scripts/bench-corpus.sh fetch {{quote(size)}} {{ if cache != "" { "--cache " + quote(cache) } else { "" } }}

# just must be the version mise.toml and scripts/bootstrap-tools.sh pin.
# Fail when a pinned tool is missing or wrong, or just is not its pinned version.
doctor:
    # Non-interactive shells never activate mise, which `mise doctor` reports as a problem:
    # the shims directory on PATH is the documented alternative for such setups.
    PATH="${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims:$PATH" mise doctor
    missing="$(mise ls --missing)"; test -z "$missing"
    pinned="$(sed -n 's/^just = "\(.*\)"$/\1/p' mise.toml)"; \
      actual="$({{just_executable()}} --version | cut -d' ' -f2)"; \
      if [ -z "$pinned" ] || [ "$pinned" != "$actual" ]; then \
        echo "doctor: just is $actual but mise.toml pins '$pinned'" >&2; exit 1; fi

# Fail closed with mise's install hint when a pinned tool is not on PATH (fnd §8 rule 4).
_require tool:
    @command -v {{tool}} >/dev/null 2>&1 || { echo "{{tool}} is missing: run 'mise install --locked' (or bash scripts/bootstrap-tools.sh); just doctor says what else is" >&2; exit 1; }
