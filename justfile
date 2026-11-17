# The command surface (fnd §9). FND-01 holds `doctor` only; FND-02 adds quick, check and the
# stage recipes. A recipe that has not landed does not exist: just exits non-zero naming it.
set shell := ["bash", "-euo", "pipefail", "-c"]

# Fail when a pinned tool is missing or wrong, or when just is not the version mise.toml pins
# (which scripts/bootstrap-tools.sh also pins).
doctor:
    # Non-interactive shells never activate mise, which `mise doctor` reports as a problem:
    # the shims directory on PATH is the documented alternative for such setups.
    PATH="${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims:$PATH" mise doctor
    test -z "$(mise ls --missing)"
    pinned="$(sed -n 's/^just = "\(.*\)"$/\1/p' mise.toml)"; \
      actual="$({{just_executable()}} --version | cut -d' ' -f2)"; \
      if [ -z "$pinned" ] || [ "$pinned" != "$actual" ]; then \
        echo "doctor: just is $actual but mise.toml pins '$pinned'" >&2; exit 1; fi
