//! The conformance stages (FORMAL-02, ver §6.2) through `xtask check-only` and `xtask conformance`,
//! over a scratch repository holding the repository's gate files, its `formal/` registry, model
//! and regression traces, and a scratch bin directory whose fake `node`, `quint` and `cargo` stand
//! in for the real ones (so no Node and no nested build run in the test stages, ver §2): the fake
//! `quint run` writes a given trace wherever `--out-itf` says, with a `#meta` timestamp that
//! differs on every call, and the fake `cargo` logs the replay it was asked for. Each case breaks
//! one thing and the stage must fail for that reason; the controls pass. The real Quint and the
//! real replay run in the gate's own `formal-freshness` and `conformance-replay` stages.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

/// The committed regression trace of the repository.
const COMMITTED: &str = "formal/traces/regression/smoke-counts-up.itf.json";

/// `text` (formal/models.toml or the regression manifest) without the tables of other models
/// than smoke: a table is kept when its header names smoke (`[models.smoke]`, `[traces.smoke-…]`).
fn smoke_only(text: &str) -> String {
    let mut keep = true;
    let mut kept = String::new();
    for line in text.lines() {
        if line.starts_with('[') {
            keep = line.starts_with("[models.smoke]") || line.starts_with("[traces.smoke-");
        }
        if keep {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    kept
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn printed(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A fake `quint` that logs its arguments and, for `run`, writes `$FAKE_DIR/trace` (its `STAMP`
/// replaced by the call's number, so `#meta` differs per call) to each `--out-itf` file
/// (`{seq}` from 0 to `--n-traces` - 1); in the `drift` mode a second call writes another state;
/// it exits with `$FAKE_DIR/quint-exit` (0 when absent).
const QUINT: &str = r##"#!/bin/sh
echo "quint $*" >> "$FAKE_DIR/log"
[ "$1" = run ] || exit 0
out=""; n=1; prev=""
for a in "$@"; do
  case "$prev" in
    --out-itf) out="$a" ;;
    --n-traces) n="$a" ;;
  esac
  prev="$a"
done
[ -n "$out" ] || exit 9
calls=$(cat "$FAKE_DIR/calls" 2>/dev/null || echo 0); calls=$((calls + 1)); echo "$calls" > "$FAKE_DIR/calls"
i=0
while [ "$i" -lt "$n" ]; do
  file=$(printf '%s' "$out" | sed "s/{seq}/$i/")
  sed "s/STAMP/$calls/" "$FAKE_DIR/trace" > "$file"
  if [ "$(cat "$FAKE_DIR/mode" 2>/dev/null)" = drift ] && [ "$calls" -gt 1 ]; then
    sed -i 's/"#bigint":"6"/"#bigint":"7"/' "$file"
  fi
  i=$((i + 1))
done
exit "$(cat "$FAKE_DIR/quint-exit" 2>/dev/null || echo 0)"
"##;

/// A fake `cargo` that logs its arguments and the generated traces it was pointed at, and exits
/// with `$FAKE_DIR/cargo-exit` (0 when absent).
const CARGO: &str = r#"#!/bin/sh
echo "cargo $*" >> "$FAKE_DIR/log"
prev=""
for a in "$@"; do
  [ "$prev" = --generated ] && ls -R "$a" >> "$FAKE_DIR/log"
  prev="$a"
done
exit "$(cat "$FAKE_DIR/cargo-exit" 2>/dev/null || echo 0)"
"#;

/// The scratch world of one case, removed when dropped.
struct World {
    dir: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
}

impl World {
    fn new(case: &str) -> Result<Self, Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("xtask-conformance-{}-{case}", std::process::id()));
        if dir.try_exists()? {
            fs::remove_dir_all(&dir)?;
        }
        let world = Self {
            repo: dir.join("repo"),
            bin: dir.join("bin"),
            dir,
        };
        fs::create_dir_all(world.repo.join("tools"))?;
        fs::create_dir_all(world.repo.join("formal/traces/regression"))?;
        for file in [
            "rust-toolchain.toml",
            "mise.toml",
            "mise.lock",
            "tools/versions.toml",
            "formal/smoke.qnt",
            COMMITTED,
        ] {
            fs::copy(repo_root().join(file), world.repo.join(file))?;
        }
        // The world holds the smoke model alone, whatever else the repository registers: the fake
        // `quint run` writes the smoke trace for every run.
        for file in [
            "formal/models.toml",
            "formal/traces/regression/manifest.toml",
        ] {
            let text = fs::read_to_string(repo_root().join(file))?;
            fs::write(world.repo.join(file), smoke_only(&text))?;
        }
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&world.repo)
            .output()?;
        assert!(init.status.success(), "{}", printed(&init));
        fs::create_dir_all(&world.bin)?;
        world.tool("node", "#!/bin/sh\necho v0\n")?;
        world.tool("quint", QUINT)?;
        world.tool("cargo", CARGO)?;
        world.quint_writes(&stamped(&fs::read_to_string(repo_root().join(COMMITTED))?))?;
        Ok(world)
    }

    /// Writes an executable `name` into the scratch bin directory.
    fn tool(&self, name: &str, body: &str) -> TestResult {
        let file = self.bin.join(name);
        fs::write(&file, body)?;
        let chmod = Command::new("chmod").arg("+x").arg(&file).output()?;
        assert!(chmod.status.success(), "{}", printed(&chmod));
        Ok(())
    }

    /// The trace the fake `quint run` writes.
    fn quint_writes(&self, trace: &str) -> TestResult {
        Ok(fs::write(self.dir.join("trace"), trace)?)
    }

    /// A control file of the fakes (`mode`, `quint-exit`, `cargo-exit`).
    fn set(&self, name: &str, value: &str) -> TestResult {
        Ok(fs::write(self.dir.join(name), value)?)
    }

    /// The fakes' calls so far.
    fn calls(&self) -> String {
        fs::read_to_string(self.dir.join("log")).unwrap_or_default()
    }

    /// `xtask --root <repo> <args>` in the repository, with the scratch bin directory and the
    /// system's PATH, a scratch HOME and tool cache, and nothing else.
    fn xtask(&self, args: &[&str]) -> Result<Output, Box<dyn Error>> {
        let mut path = std::ffi::OsString::from(&self.bin);
        path.push(":/usr/bin:/bin");
        Ok(Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("--root")
            .arg(&self.repo)
            .args(args)
            .current_dir(&self.repo)
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.dir)
            .env("TMPDIR", &self.dir)
            .env("XDG_CACHE_HOME", self.dir.join("cache"))
            .env("FAKE_DIR", &self.dir)
            .output()?)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// `trace` with its `#meta` timestamp replaced by `STAMP`, which the fake quint fills in.
fn stamped(trace: &str) -> String {
    let Some(at) = trace.find("\"timestamp\":") else {
        return trace.to_owned();
    };
    let (head, tail) = trace.split_at(at);
    let digits = tail
        .trim_start_matches("\"timestamp\":")
        .trim_start_matches(|c: char| c.is_ascii_digit());
    format!("{head}\"timestamp\":STAMP{digits}")
}

#[test]
fn should_pass_freshness_when_quint_regenerates_each_trace_outside_its_meta() -> TestResult {
    // given: the control of the cases below; the regenerated #meta differs from the committed one
    let world = World::new("fresh")?;

    // when
    let output = world.xtask(&["check-only", "formal-freshness"])?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    let calls = world.calls();
    assert!(
        calls.contains("quint run --backend typescript --mbt --verbosity 0 --main smoke --init init --step step --invariant allInvariants --max-steps 6 --seed 0x2a --out-itf "),
        "{calls}"
    );
    assert!(
        calls
            .trim_end()
            .ends_with("smoke-counts-up.itf.json formal/smoke.qnt"),
        "{calls}"
    );
    assert!(printed(&output).contains("1 regression traces regenerate byte for byte"));
    Ok(())
}

#[test]
fn should_fail_freshness_when_a_regression_trace_is_stale() -> TestResult {
    // given: Quint now generates another last state than the committed trace holds
    let world = World::new("stale")?;
    let committed = fs::read_to_string(repo_root().join(COMMITTED))?;
    world
        .quint_writes(&stamped(&committed).replace(r##""#bigint":"6""##, r##""#bigint":"60""##))?;

    // when
    let output = world.xtask(&["check-only", "formal-freshness"])?;

    // then
    assert_eq!(output.status.code(), Some(1), "{}", printed(&output));
    let text = printed(&output);
    assert!(
        text.contains(&format!("{COMMITTED} is stale: Quint now generates")),
        "{text}"
    );
    assert!(
        text.contains("regenerate it (and review the diff) with: quint run"),
        "{text}"
    );
    Ok(())
}

#[test]
fn should_fail_freshness_when_a_committed_trace_is_not_one_quint_wrote() -> TestResult {
    // given
    let world = World::new("malformed")?;
    fs::write(world.repo.join(COMMITTED), "{\"states\": []}")?;

    // when
    let output = world.xtask(&["check-only", "formal-freshness"])?;

    // then
    assert_eq!(output.status.code(), Some(1), "{}", printed(&output));
    assert!(
        printed(&output)
            .contains("cannot be compared: the committed trace: it does not start with"),
        "{}",
        printed(&output)
    );
    Ok(())
}

#[test]
fn should_fail_both_stages_before_quint_when_a_trace_is_not_listed() -> TestResult {
    // given
    let world = World::new("unlisted")?;
    fs::write(
        world.repo.join("formal/traces/regression/stray.itf.json"),
        "{}",
    )?;

    // when
    let freshness = world.xtask(&["check-only", "formal-freshness"])?;

    // then
    assert_eq!(freshness.status.code(), Some(1), "{}", printed(&freshness));
    assert!(
        printed(&freshness)
            .contains("stray.itf.json: not listed in formal/traces/regression/manifest.toml"),
        "{}",
        printed(&freshness)
    );
    assert_eq!(world.calls(), "");
    Ok(())
}

#[test]
fn should_fail_with_the_install_command_when_quint_is_missing() -> TestResult {
    // given
    let world = World::new("no-quint")?;
    fs::remove_file(world.bin.join("quint"))?;

    // when
    let outputs = [
        world.xtask(&["check-only", "formal-freshness"])?,
        world.xtask(&["conformance", "smoke", "0x2a"])?,
    ];

    // then
    for output in outputs {
        assert_eq!(output.status.code(), Some(1), "{}", printed(&output));
        assert!(
            printed(&output)
                .contains("quint missing from the stages' PATH: run 'mise install --locked'"),
            "{}",
            printed(&output)
        );
    }
    Ok(())
}

#[test]
fn should_generate_twice_and_replay_at_the_printed_seed_when_conformance_runs() -> TestResult {
    // given
    let world = World::new("run")?;

    // when
    let output = world.xtask(&["conformance", "smoke", "0x2a", "2", "3"])?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    let text = printed(&output);
    assert!(text.contains("seed 0x2a: 2 traces per model"), "{text}");
    assert!(
        text.contains("repeat smoke (2 traces of 3 steps) with: just conformance smoke 0x2a 2 3"),
        "{text}"
    );
    let calls = world.calls();
    let runs = calls
        .lines()
        .filter(|l| {
            l.starts_with("quint run --backend typescript --mbt --verbosity 0 --main smoke --init init --step step --invariant allInvariants --max-steps 3 --max-samples 2 --n-traces 2 --seed 0x2a --out-itf ")
                && l.ends_with("trace_{seq}.itf.json formal/smoke.qnt")
        })
        .count();
    assert_eq!(runs, 2, "{calls}");
    assert!(
        calls.contains(
            "cargo run --locked -q -p jfl-conformance -- replay --seed 0x2a --generated "
        ),
        "{calls}"
    );
    assert!(calls.contains("/run --model smoke\n"), "{calls}");
    assert!(
        calls.contains("trace_0.itf.json\ntrace_1.itf.json"),
        "{calls}"
    );
    Ok(())
}

#[test]
fn should_print_a_fresh_seed_and_use_it_when_the_stage_runs_without_one() -> TestResult {
    // given
    let world = World::new("fresh-seed")?;

    // when
    let output = world.xtask(&["check-only", "conformance-replay"])?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    let text = printed(&output);
    let seed = text
        .split("conformance-replay: seed ")
        .nth(1)
        .and_then(|rest| rest.split(':').next())
        .unwrap_or_default()
        .to_owned();
    assert!(
        seed.len() == 18 && seed.starts_with("0x"),
        "no seed printed: {text}"
    );
    assert!(
        text.contains(&format!("just conformance smoke {seed} 10 25")),
        "{text}"
    );
    let calls = world.calls();
    assert_eq!(
        calls
            .matches(&format!("--n-traces 10 --seed {seed} "))
            .count(),
        2,
        "{calls}"
    );
    assert!(
        calls.contains(&format!("replay --seed {seed} --generated ")),
        "{calls}"
    );
    Ok(())
}

/// Removes the directory a failed run kept (`kept <dir>` in its output).
fn remove_kept(text: &str) -> Option<PathBuf> {
    let kept = text
        .split("kept ")
        .nth(1)?
        .split([' ', '\n'])
        .next()
        .map(PathBuf::from)?;
    let exists = kept.is_dir();
    let _ = fs::remove_dir_all(&kept);
    exists.then_some(kept)
}

#[test]
fn should_fail_before_the_replay_when_the_seed_does_not_reproduce_the_run() -> TestResult {
    // given: the second generation at the same seed differs
    let world = World::new("drift")?;
    world.set("mode", "drift")?;

    // when
    let output = world.xtask(&["conformance", "smoke", "0x2a", "1", "6"])?;

    // then
    assert_eq!(output.status.code(), Some(1), "{}", printed(&output));
    let text = printed(&output);
    assert!(
        text.contains("smoke: the seed 0x2a does not reproduce the run: trace_0.itf.json differs"),
        "{text}"
    );
    assert!(!world.calls().contains("cargo "), "{}", world.calls());
    assert!(remove_kept(&text).is_some(), "{text}");
    Ok(())
}

#[test]
fn should_fail_with_the_replay_s_code_and_keep_the_traces_when_the_replay_fails() -> TestResult {
    // given: the replay reports a harness error (exit 3)
    let world = World::new("replay-fails")?;
    world.set("cargo-exit", "3")?;

    // when
    let output = world.xtask(&["conformance", "all", "0x2a", "1", "6"])?;

    // then
    assert_eq!(output.status.code(), Some(3), "{}", printed(&output));
    let text = printed(&output);
    assert!(text.contains("FAILED at seed 0x2a; kept "), "{text}");
    let kept = remove_kept(&text);
    assert!(kept.is_some(), "{text}");
    assert!(
        !world.calls().contains("--model"),
        "`all` replays every model: {}",
        world.calls()
    );
    Ok(())
}

#[test]
fn should_fail_when_quint_fails_while_generating() -> TestResult {
    // given: an invariant violated while generating
    let world = World::new("quint-fails")?;
    world.set("quint-exit", "1")?;

    // when
    let output = world.xtask(&["conformance", "smoke", "0x2a", "1", "6"])?;

    // then
    assert_eq!(output.status.code(), Some(1), "{}", printed(&output));
    let text = printed(&output);
    assert!(text.contains("smoke: quint run failed (exit 1)"), "{text}");
    let _ = remove_kept(&text);
    Ok(())
}

#[test]
fn should_refuse_usage_when_the_run_is_malformed() -> TestResult {
    // given
    let world = World::new("usage")?;

    // when
    let outputs = [
        world.xtask(&["conformance", "smoke", "0xZZ"])?,
        world.xtask(&["conformance", "smoke", "0x2a", "0"])?,
        world.xtask(&["conformance", "nope", "0x2a"])?,
    ];

    // then
    for output in &outputs {
        assert_eq!(output.status.code(), Some(2), "{}", printed(output));
    }
    assert!(
        printed(&outputs[2]).contains("no model nope; registered: smoke (or all)"),
        "{}",
        printed(&outputs[2])
    );
    assert_eq!(world.calls(), "");
    Ok(())
}
