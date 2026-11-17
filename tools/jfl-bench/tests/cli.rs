//! The command line through the real binary: help, an unknown command, `parse` and `compare`.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_jfl-bench");

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("jfl-bench-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Result<(bool, String, String), Box<dyn Error>> {
    let out = Command::new(BIN).args(args).output()?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

#[test]
fn should_print_the_usage_when_called_without_arguments() {
    // given
    // when
    let (ok, stdout, _) = run(&[]).unwrap();
    // then
    assert!(ok);
    assert!(stdout.contains("usage: jfl-bench"));
}

#[test]
fn should_fail_and_say_why_when_the_command_is_unknown() {
    // given
    // when
    let (ok, _, stderr) = run(&["frobnicate"]).unwrap();
    // then
    assert!(!ok);
    assert!(stderr.contains("unknown command"), "{stderr}");
}

#[test]
fn should_print_one_json_line_when_parse_runs_on_a_tree() {
    // given
    let dir = Dir::new("cli-parse").unwrap();
    std::fs::write(dir.path().join("A.java"), "class A { int x; }\n").unwrap();
    // when
    let (ok, stdout, stderr) =
        run(&["parse", dir.path().to_str().unwrap(), "--threads", "2"]).unwrap();
    // then
    assert!(ok, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["files"], 1);
    assert_eq!(v["threads"], 2);
    assert_eq!(v["upto"], "extract");
    assert_eq!(v["grammar"], "orchard");
}

#[test]
fn should_refuse_an_unknown_option_when_parse_is_given_one() {
    // given
    // when
    let (ok, _, stderr) = run(&["parse", ".", "--thread", "2"]).unwrap();
    // then
    assert!(!ok);
    assert!(stderr.contains("unknown option --thread"), "{stderr}");
}

#[test]
fn should_name_the_fetch_command_when_the_corpus_is_not_checked_out() {
    // given
    let dir = Dir::new("cli-nocorpus").unwrap();
    let missing = dir.path().join("none");
    // when
    let (ok, _, stderr) = run(&[
        "run",
        "e1",
        "small",
        "--allow-debug",
        "--corpus-path",
        missing.to_str().unwrap(),
        "--out",
        dir.path().to_str().unwrap(),
    ])
    .unwrap();
    // then
    assert!(!ok);
    assert!(stderr.contains("bench-corpus.sh fetch small"), "{stderr}");
}

#[test]
fn should_fail_in_strict_mode_only_when_a_metric_is_outside_the_band() {
    // given: two reports differing by 50 % in one metric
    let dir = Dir::new("cli-compare").unwrap();
    let report = |value: f64| {
        serde_json::json!({
            "schema": 1, "suite": "s",
            "corpus": {"size": "small", "repo": "r", "commit": "c", "java_files": 1},
            "machine": {"cpu_model": "x", "logical_cpus": 1, "ram_mib": null, "os": "o", "kernel": "k",
                        "arch": "a", "disk": null, "tools": {}, "commit": "c", "dirty": false},
            "build": {"profile": "bench-tool", "debug_assertions": false, "toolchain": "1.97.0"}, "queries": [],
            "config": {"runs": 1, "warmup": 0, "seed": 1},
            "metrics": [{"name": "s/small/m", "unit": "ms", "better": "smaller", "value": value,
                         "stat": "1 run", "runs": [value], "spread_pct": null}],
            "notes": []
        })
        .to_string()
    };
    let (a, b) = (dir.path().join("a.json"), dir.path().join("b.json"));
    std::fs::write(&a, report(100.0)).unwrap();
    std::fs::write(&b, report(150.0)).unwrap();
    let (a, b) = (a.to_str().unwrap(), b.to_str().unwrap());
    // when
    let (lenient_ok, stdout, _) = run(&["compare", a, b]).unwrap();
    let (strict_ok, _, stderr) = run(&["compare", a, b, "--strict"]).unwrap();
    let (wide_ok, _, _) = run(&["compare", a, b, "--band", "60", "--strict"]).unwrap();
    // then
    assert!(lenient_ok && stdout.contains("OUTSIDE"), "{stdout}");
    assert!(
        !strict_ok && stderr.contains("outside the band"),
        "{stderr}"
    );
    assert!(wide_ok);
}

#[cfg(debug_assertions)]
#[test]
fn should_refuse_a_debug_build_unless_told_otherwise_when_run_is_called() {
    // given
    let dir = Dir::new("cli-debug").unwrap();
    // when
    let (ok, _, stderr) = run(&[
        "run",
        "e1",
        "small",
        "--corpus-path",
        dir.path().to_str().unwrap(),
    ])
    .unwrap();
    // then
    assert!(!ok);
    assert!(stderr.contains("debug build"), "{stderr}");
}

#[test]
fn should_ignore_a_global_gitignore_and_parent_ignore_files_when_listing() {
    // given: a checkout (it has a .git) under a parent that ignores *.java, with a global ignore of *Tests.java
    let dir = Dir::new("cli-ignores").unwrap();
    let outer = dir.path().join("outer");
    let tree = outer.join("checkout");
    std::fs::create_dir_all(tree.join(".git")).unwrap();
    std::fs::create_dir_all(outer.join(".git")).unwrap();
    std::fs::write(outer.join(".gitignore"), "*.java\n").unwrap();
    std::fs::create_dir_all(tree.join(".git/info")).unwrap();
    std::fs::write(tree.join(".git/info/exclude"), "Excluded.java\n").unwrap();
    for f in ["A.java", "BTests.java", "Excluded.java", "Local.java"] {
        std::fs::write(tree.join(f), "class X {}\n").unwrap();
    }
    std::fs::write(tree.join(".gitignore"), "Local.java\n").unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("git")).unwrap();
    std::fs::write(home.join("git/ignore"), "*Tests.java\n").unwrap();
    // when
    let out = Command::new(BIN)
        .args(["parse", tree.to_str().unwrap(), "--upto", "read"])
        .env("XDG_CONFIG_HOME", &home)
        .env("HOME", &home)
        .output()
        .unwrap();
    // then: only the checkout's own .gitignore applies (Local.java); the other three are listed
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["files"], 3, "{}", String::from_utf8_lossy(&out.stderr));
}
