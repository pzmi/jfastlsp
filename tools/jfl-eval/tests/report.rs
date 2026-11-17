//! `jfl-eval report` and `inspect` through the real binary, over the synthetic Harbor trials of
//! `tests/fixtures/runs` (written once by a generator, Claude Code's documented JSONL shape and
//! Harbor 0.26.0's `result.json` and ATIF; no real transcript content).

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_jfl-eval");

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn run(args: &[&str]) -> Result<(bool, String, String), Box<dyn Error>> {
    let out = Command::new(BIN).args(args).output()?;
    Ok((
        out.status.success(),
        String::from_utf8(out.stdout)?,
        String::from_utf8(out.stderr)?,
    ))
}

fn runs_dir() -> String {
    fixtures().join("runs").to_string_lossy().into_owned()
}

/// Every file under `dir`, as text.
fn all_text(dir: &std::path::Path) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            out.push_str(&all_text(&path)?);
        } else {
            out.push_str(&String::from_utf8_lossy(&std::fs::read(&path)?));
        }
    }
    Ok(out)
}

/// What the fixtures plant: the prompt, paths, a source line, a diagnostic's message, a model
/// name, an exception message, and a foreign task name that is itself a path.
const PLANTED: [&str; 11] = [
    "PLANTED",
    "planted",
    "Planted",
    "/app",
    "/work",
    ".java",
    "Money",
    "cents",
    "synthetic-model",
    "Traceback",
    "LedgerServiceImpl",
];

#[test]
fn should_produce_the_golden_markdown_report_when_given_the_fixture_runs() {
    // given
    let golden = std::fs::read_to_string(fixtures().join("report.golden.md")).unwrap();
    // when
    let (ok, stdout, stderr) = run(&["report", &runs_dir()]).unwrap();
    // then
    assert!(ok, "{stderr}");
    assert_eq!(stdout, golden);
}

#[test]
fn should_produce_the_golden_json_report_when_given_the_fixture_runs() {
    // given
    let golden = std::fs::read_to_string(fixtures().join("report.golden.json")).unwrap();
    // when
    let (ok, stdout, stderr) = run(&["report", &runs_dir(), "--json"]).unwrap();
    // then
    assert!(ok, "{stderr}");
    assert_eq!(stdout, golden);
}

#[test]
fn should_leave_out_every_planted_string_when_the_input_carries_them() {
    // given: the inputs really carry every planted string
    let input = all_text(&fixtures().join("runs")).unwrap();
    for planted in PLANTED {
        assert!(
            input.contains(planted),
            "the fixtures no longer plant {planted:?}"
        );
    }
    // when
    let (_, markdown, _) = run(&["report", &runs_dir()]).unwrap();
    let (_, json, _) = run(&["report", &runs_dir(), "--json"]).unwrap();
    let trial = fixtures().join("runs/job-jdtls/jdtls-proxy__fix-rounding__bbb1");
    let (_, inspect, _) = run(&["inspect", trial.to_str().unwrap()]).unwrap();
    // then
    for output in [&markdown, &json, &inspect] {
        assert!(!output.is_empty());
        for planted in PLANTED {
            assert!(
                !output.contains(planted),
                "{planted:?} leaked into:\n{output}"
            );
        }
    }
}

#[test]
fn should_print_the_trials_metrics_and_shapes_when_inspecting_one() {
    // given
    let trial = fixtures().join("runs/job-jdtls/jdtls-proxy__fix-rounding__bbb1");
    // when
    let (ok, stdout, stderr) = run(&["inspect", trial.to_str().unwrap()]).unwrap();
    // then
    assert!(ok, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["verdict"], "success");
    assert_eq!(v["metrics"]["lsp_calls"], 5);
    assert_eq!(v["shapes"]["attachment_types"]["diagnostics"], 2);
}

#[test]
fn should_refuse_to_inspect_a_trial_when_jfl_eval_did_not_write_its_task() {
    // given
    let trial = fixtures().join("runs/job-jdtls/hello-world__ddd1");
    // when
    let (ok, _, stderr) = run(&["inspect", trial.to_str().unwrap()]).unwrap();
    // then
    assert!(!ok);
    assert!(stderr.contains("task_name"), "{stderr}");
    assert!(!stderr.contains("PLANTED"), "{stderr}");
}

#[test]
fn should_report_no_runs_when_the_directory_holds_no_trials() {
    // given: a directory of fixtures that holds no trial
    let empty = fixtures().join("runs/job-jdtls/hello-world__ddd1/agent");
    // when
    let (ok, stdout, stderr) = run(&["report", empty.to_str().unwrap(), "--json"]).unwrap();
    // then
    assert!(ok, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["runs"], 0);
    assert_eq!(v["arms"], serde_json::json!([]));
}
