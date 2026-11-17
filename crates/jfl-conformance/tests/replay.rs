//! The conformance harness (FORMAL-02, ver §6.2) on `formal/smoke.qnt`'s driver, with no Quint:
//! the committed regression trace conforms; the negative control (a counter that skips 3) fails
//! at step 3 with expected and actual state in its report; a malformed ITF file is a harness
//! error, never a pass; and the binary's command (`cli::run`, in process) exits 0, 1 or 3 for a
//! conforming, diverging or unreadable set of generated traces. The real Quint generates traces in
//! the `conformance-replay` stage (xtask) and in `formal-freshness`.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use jfl_conformance::HarnessError;
use jfl_conformance::cli;
use jfl_conformance::manifest;
use jfl_conformance::registry;
use jfl_conformance::replay::{Context, Failure};
use jfl_conformance::smoke;
use jfl_conformance::trace;

type TestResult = Result<(), Box<dyn Error>>;

/// The committed trace the tests start from: the counter from 0 to 6.
const COMMITTED: &str = "formal/traces/regression/smoke-counts-up.itf.json";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn committed() -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(root().join(COMMITTED))?)
}

/// A scratch directory of generated traces (`<dir>/smoke/*.itf.json`), removed when dropped.
struct Generated {
    dir: PathBuf,
}

impl Generated {
    fn new(case: &str) -> Result<Self, Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("jfl-conformance-{case}-{}", std::process::id()));
        if dir.try_exists()? {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(dir.join("smoke"))?;
        Ok(Self { dir })
    }

    fn trace(&self, name: &str, text: &str) -> TestResult {
        Ok(fs::write(self.dir.join("smoke").join(name), text)?)
    }

    /// `jfl-conformance replay --seed 0x7 --generated <dir>` on the repository: (code, out, err).
    fn replay(&self) -> (u8, String, String) {
        let args = [
            "replay".to_owned(),
            "--seed".to_owned(),
            "0x7".to_owned(),
            "--generated".to_owned(),
            self.dir.display().to_string(),
        ];
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli::run(&args, &root(), &mut out, &mut err);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }
}

impl Drop for Generated {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn should_conform_when_every_committed_regression_trace_is_replayed() {
    // given
    let args = ["replay".to_owned()];
    let (mut out, mut err) = (Vec::new(), Vec::new());

    // when
    let code = cli::run(&args, &root(), &mut out, &mut err);

    // then
    let out = String::from_utf8_lossy(&out);
    assert_eq!(code, cli::OK, "{out}{}", String::from_utf8_lossy(&err));
    assert!(
        out.contains(&format!("smoke: {COMMITTED}: 7 states conform")),
        "{out}"
    );
}

#[test]
fn should_fail_at_the_skipped_value_when_the_negative_control_replays_a_trace() -> TestResult {
    // given
    let steps = trace::parse(COMMITTED, &committed()?)?;
    let context = Context {
        model: "smoke",
        seed: "0x2a",
        trace: COMMITTED,
    };

    // when
    let replayed = smoke::control(&context, steps);

    // then
    let Err(Failure::Diverged(divergence)) = replayed else {
        return Err(format!("the negative control did not diverge: {replayed:?}").into());
    };
    assert_eq!(divergence.step, 3);
    assert_eq!(divergence.action, "step");
    assert!(divergence.expected.contains("count: 3"), "{divergence}");
    assert!(divergence.actual.contains("count: 4"), "{divergence}");
    let report = divergence.to_string();
    for part in [
        "DIVERGENCE: model smoke, seed 0x2a, trace formal/traces/regression/smoke-counts-up.itf.json, step 3, action step",
        "expected (model):\nSmokeState {\n    count: 3,\n}",
        "actual (implementation):\nSmokeState {\n    count: 4,\n}",
        "--- model\n+++ implementation\n",
        "-    count: 3,\n+    count: 4,\n",
    ] {
        assert!(report.contains(part), "no {part:?} in:\n{report}");
    }
    Ok(())
}

#[test]
fn should_conform_and_see_the_control_diverge_when_generated_traces_are_replayed() -> TestResult {
    // given
    let generated = Generated::new("conform")?;
    generated.trace("trace_0.itf.json", &committed()?)?;
    generated.trace("trace_1.itf.json", &committed()?)?;

    // when
    let (code, out, err) = generated.replay();

    // then
    assert_eq!(code, cli::OK, "{out}{err}");
    assert!(
        out.contains("smoke: seed 0x7: 2 generated traces (14 states) conform"),
        "{out}"
    );
    assert!(
        out.contains("the negative control diverged as it must, at step 3"),
        "{out}"
    );
    Ok(())
}

#[test]
fn should_exit_with_a_divergence_when_a_generated_trace_disagrees_with_the_implementation()
-> TestResult {
    // given: the model counts 0, 1, 2, 3, 9: the implementation cannot
    let text = committed()?.replace(
        r##""#meta":{"index":4},"count":{"#bigint":"4"}"##,
        r##""#meta":{"index":4},"count":{"#bigint":"9"}"##,
    );
    assert_ne!(text, committed()?);
    let generated = Generated::new("diverge")?;
    generated.trace("trace_0.itf.json", &text)?;

    // when
    let (code, out, err) = generated.replay();

    // then
    assert_eq!(code, cli::DIVERGED, "{out}{err}");
    assert!(
        err.starts_with("DIVERGENCE: model smoke, seed 0x7"),
        "{err}"
    );
    assert!(err.contains("step 4, action step"), "{err}");
    assert!(err.contains("-    count: 9,\n+    count: 4,"), "{err}");
    Ok(())
}

/// A malformed trace: its case, its text and what the harness error must name.
type Malformed = (&'static str, String, &'static str);

/// Each malformed trace: an edit of the committed one, or a text of its own.
fn malformed() -> Result<Vec<Malformed>, Box<dyn Error>> {
    let good = committed()?;
    let edit = |from: &str, to: &str| good.replacen(from, to, 1);
    Ok(vec![
        ("empty file", String::new(), "not an ITF trace"),
        ("not JSON", "states: none".to_owned(), "not an ITF trace"),
        (
            "truncated",
            good.chars().take(200).collect(),
            "not an ITF trace",
        ),
        (
            "no states key",
            r##"{"#meta":{}}"##.to_owned(),
            "not an ITF trace",
        ),
        (
            "no state",
            r##"{"#meta":{},"states":[]}"##.to_owned(),
            "no state",
        ),
        (
            "a state not a record",
            r##"{"#meta":{},"states":[7]}"##.to_owned(),
            "not an ITF trace",
        ),
        (
            "no action variable",
            edit(r#","mbt::actionTaken":"init""#, ""),
            "has no `mbt::actionTaken`",
        ),
        (
            "an anonymous action",
            edit(r#""mbt::actionTaken":"init""#, r#""mbt::actionTaken":"""#),
            "not an action's name",
        ),
        (
            "an action not a string",
            edit(r#""mbt::actionTaken":"init""#, r#""mbt::actionTaken":5"#),
            "not an action's name",
        ),
        (
            "no picks variable",
            edit(r#","mbt::nondetPicks":{}"#, ""),
            "has no `mbt::nondetPicks`",
        ),
        (
            "picks not a record",
            edit(r#""mbt::nondetPicks":{}"#, r#""mbt::nondetPicks":[]"#),
            "not a record",
        ),
        (
            "a state index out of place",
            edit(r##""#meta":{"index":1}"##, r##""#meta":{"index":2}"##),
            "says it is state 2",
        ),
        (
            "a variable of the wrong type",
            edit(r##""count":{"#bigint":"0"}"##, r#""count":"zero""#),
            "does not decode as the driver's observation",
        ),
        (
            "a variable the observation lacks",
            edit(
                r##""count":{"#bigint":"0"}"##,
                r##""count":{"#bigint":"0"},"extra":true"##,
            ),
            "does not decode as the driver's observation",
        ),
        (
            "a missing variable",
            edit(r##""count":{"#bigint":"0"},"##, ""),
            "does not decode as the driver's observation",
        ),
        (
            "an action the driver does not know",
            edit(
                r#""mbt::actionTaken":"init""#,
                r#""mbt::actionTaken":"jump""#,
            ),
            "no case for the action `jump`",
        ),
    ])
}

#[test]
fn should_report_a_harness_error_when_the_itf_file_is_malformed() -> TestResult {
    // given
    let cases = malformed()?;
    let context = Context {
        model: "smoke",
        seed: "0x7",
        trace: "case",
    };

    for (case, text, why) in cases {
        // when
        let replayed =
            trace::parse("case", &text).and_then(|steps| match smoke::replay(&context, steps) {
                Ok(n) => Ok(n),
                Err(Failure::Harness(error)) => Err(error),
                Err(Failure::Diverged(d)) => Err(HarnessError::new(format!("diverged: {d}"))),
            });
        let generated = Generated::new(&format!("malformed-{}", case.replace(' ', "-")))?;
        generated.trace("trace_0.itf.json", &text)?;
        let (code, out, err) = generated.replay();

        // then
        let error = replayed.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            error.starts_with("HARNESS_ERROR: case: "),
            "{case}: {error}"
        );
        assert!(error.contains(why), "{case}: {error}");
        assert_eq!(code, cli::HARNESS, "{case}: {out}{err}");
        assert!(err.starts_with("HARNESS_ERROR: "), "{case}: {err}");
        assert!(err.contains(why), "{case}: {err}");
    }
    Ok(())
}

#[test]
fn should_report_a_harness_error_when_a_run_is_empty_or_holds_a_stranger() -> TestResult {
    // given: no trace; a file that is not a trace; a trace too short for the negative control
    let empty = Generated::new("empty")?;
    let stranger = Generated::new("stranger")?;
    stranger.trace("trace_0.itf.json", &committed()?)?;
    stranger.trace("notes.txt", "x")?;
    let short = Generated::new("short")?;
    let three_states = committed()?
        .split(r##",{"#meta":{"index":3}"##)
        .next()
        .map(|head| format!("{head}]}}"))
        .unwrap_or_default();
    short.trace("trace_0.itf.json", &three_states)?;

    // when
    let results = [empty.replay(), stranger.replay(), short.replay()];

    // then
    let whys = [
        "no generated trace (an empty run never passes)",
        "notes.txt: not a trace file",
        "the negative control conformed on all 1 generated traces of seed 0x7",
    ];
    for ((code, out, err), why) in results.iter().zip(whys) {
        assert_eq!(*code, cli::HARNESS, "{out}{err}");
        assert!(err.contains(why), "{why}: {err}");
    }
    Ok(())
}

#[test]
fn should_refuse_the_manifest_when_it_and_the_directory_disagree() -> TestResult {
    // given
    let text = fs::read_to_string(root().join(manifest::DIR).join(manifest::MANIFEST))?;
    let present = vec!["smoke-counts-up.itf.json".to_owned()];
    let cases = [
        (text.clone(), vec![], "is missing"),
        (
            text.clone(),
            vec![
                "smoke-counts-up.itf.json".to_owned(),
                "stray.itf.json".to_owned(),
            ],
            "stray.itf.json: not listed",
        ),
        (
            text.replacen("seed = ", "colour = \"red\"\nseed = ", 1),
            present.clone(),
            "unknown field",
        ),
        (
            text.replacen("[traces.smoke-counts-up]", "[traces.Smoke]", 1),
            present.clone(),
            "a name is [a-z][a-z0-9-]*",
        ),
        (
            text.replacen("max_steps = 6", "max_steps = 0", 1),
            present.clone(),
            "max_steps is at least 1",
        ),
    ];

    // when
    let refusals: Vec<String> = cases
        .iter()
        .map(|(text, present, _)| {
            manifest::parse(text, present)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default()
        })
        .collect();

    // then
    for ((_, _, why), refusal) in cases.iter().zip(&refusals) {
        assert!(refusal.contains(why), "{why}: {refusal}");
    }
    let read = manifest::read(&root())?;
    assert_eq!(read.len(), 1);
    assert_eq!(manifest::parse(&text, &present)?, read);
    Ok(())
}

#[test]
fn should_refuse_the_registry_when_a_model_has_no_driver() -> TestResult {
    // given
    let registered = registry::models(&root())?;
    let one_more = vec!["smoke".to_owned(), "views".to_owned()];
    let unregistered = vec!["other".to_owned()];

    // when
    let checks = [
        registry::check(&registered),
        registry::check(&one_more),
        registry::check(&unregistered),
        registry::check(&[]),
    ];

    // then
    assert_eq!(checks.first().cloned(), Some(Ok(())));
    let error = |i: usize| {
        checks
            .get(i)
            .and_then(|c| c.clone().err())
            .map(|e| e.to_string())
            .unwrap_or_default()
    };
    assert!(error(1).contains("views is registered"), "{}", error(1));
    assert!(error(2).contains("other is registered"), "{}", error(2));
    assert!(error(2).contains("no driver"), "{}", error(2));
    assert!(error(3).contains("smoke has a driver"), "{}", error(3));
    Ok(())
}

#[test]
fn should_report_a_harness_error_when_a_model_has_no_regression_trace() -> TestResult {
    // given: a repository whose manifest lists no trace
    let scratch = Generated::new("bare-root")?;
    let bare = scratch.dir.join("root");
    fs::create_dir_all(bare.join(manifest::DIR))?;
    fs::copy(root().join(registry::MODELS), bare.join(registry::MODELS))?;
    fs::write(
        bare.join(manifest::DIR).join(manifest::MANIFEST),
        "# no trace\n",
    )?;
    let args = ["replay".to_owned()];
    let (mut out, mut err) = (Vec::new(), Vec::new());

    // when
    let code = cli::run(&args, &bare, &mut out, &mut err);

    // then
    let err = String::from_utf8_lossy(&err);
    assert_eq!(code, cli::HARNESS, "{err}");
    assert!(
        err.contains("smoke has no committed regression trace"),
        "{err}"
    );
    Ok(())
}
