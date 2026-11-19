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
use jfl_conformance::daemon;
use jfl_conformance::manifest;
use jfl_conformance::registry;
use jfl_conformance::replay::{self, Context, Failure};
use jfl_conformance::smoke;
use jfl_conformance::trace;
use jfl_conformance::views;

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

    /// `jfl-conformance replay --model smoke --seed 0x7 --generated <dir>` on the repository:
    /// (code, out, err).
    fn replay(&self) -> (u8, String, String) {
        let args = [
            "replay".to_owned(),
            "--model".to_owned(),
            "smoke".to_owned(),
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
    let replayed = replay::replay(smoke::control(), &context, steps);

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
        out.contains("the negative control diverged as it must, at step 3 of")
            && out.contains("trace_0.itf.json"),
        "a control that diverges on a generated trace is reported there: {out}"
    );
    Ok(())
}

#[test]
fn should_pass_the_control_when_it_diverges_only_on_a_regression_trace() -> TestResult {
    // given: a generated run too short for the control (it counts to 2; the control skips 3),
    // so only the committed regression trace (counting to 6) can show its fault
    let short = Generated::new("short")?;
    let three_states = committed()?
        .split(r##",{"#meta":{"index":3}"##)
        .next()
        .map(|head| format!("{head}]}}"))
        .unwrap_or_default();
    short.trace("trace_0.itf.json", &three_states)?;

    // when
    let (code, out, err) = short.replay();

    // then: the divergence is required in the union, and found on the committed trace
    assert_eq!(code, cli::OK, "{out}{err}");
    assert!(
        out.contains(&format!(
            "smoke: the negative control diverged as it must, at step 3 of {COMMITTED}"
        )),
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
        let replayed = trace::parse("case", &text).and_then(|steps| {
            match replay::replay(smoke::driver(), &context, steps) {
                Ok(n) => Ok(n),
                Err(Failure::Harness(error)) => Err(error),
                Err(Failure::Diverged(d)) => Err(HarnessError::new(format!("diverged: {d}"))),
            }
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
    // given: no trace; a file that is not a trace (a control that never diverges is
    // cli::tests::should_name_every_trace_tried_when_the_control_never_diverges)
    let empty = Generated::new("empty")?;
    let stranger = Generated::new("stranger")?;
    stranger.trace("trace_0.itf.json", &committed()?)?;
    stranger.trace("notes.txt", "x")?;

    // when
    let results = [empty.replay(), stranger.replay()];

    // then
    let whys = [
        "no generated trace (an empty run never passes)",
        "notes.txt: not a trace file",
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
    let present: Vec<String> = manifest::read(&root())?
        .iter()
        .map(|r| format!("{}{}", r.name, manifest::SUFFIX))
        .collect();
    let with_stray: Vec<String> = present
        .iter()
        .cloned()
        .chain(["stray.itf.json".to_owned()])
        .collect();
    let cases = [
        (text.clone(), vec![], "is missing"),
        (text.clone(), with_stray, "stray.itf.json: not listed"),
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
    assert_eq!(manifest::parse(&text, &present)?, read);
    assert!(read.iter().any(|r| r.model == "smoke"));
    assert!(read.iter().any(|r| r.model == "views"));
    assert!(read.iter().any(|r| r.model == "daemon"));
    assert!(read.iter().any(|r| r.model == "diagnostics"));
    Ok(())
}

#[test]
fn should_refuse_the_registry_when_a_model_has_no_driver() -> TestResult {
    // given
    let registered = registry::models(&root())?;
    let one_more = vec![
        "smoke".to_owned(),
        "views".to_owned(),
        "daemon".to_owned(),
        "diagnostics".to_owned(),
        "unmodelled".to_owned(),
    ];
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
    assert!(
        error(1).contains("unmodelled is registered"),
        "{}",
        error(1)
    );
    assert!(error(2).contains("other is registered"), "{}", error(2));
    assert!(error(2).contains("no driver"), "{}", error(2));
    assert!(error(3).contains("smoke has a driver"), "{}", error(3));
    assert!(
        registry::check(&["smoke".to_owned()]).is_err(),
        "views has a driver, so registered"
    );
    assert!(
        registry::check(&["smoke".to_owned(), "views".to_owned()]).is_err(),
        "daemon has a driver, so registered"
    );
    assert!(
        registry::check(&["smoke".to_owned(), "views".to_owned(), "daemon".to_owned()]).is_err(),
        "diagnostics is pending, so registered"
    );
    Ok(())
}

#[test]
fn should_report_a_harness_error_when_a_model_has_no_regression_trace() -> TestResult {
    // given: a repository whose manifest lists no trace
    let scratch = Generated::new("bare-root")?;
    let bare = scratch.dir.join("root");
    fs::create_dir_all(bare.join(manifest::DIR))?;
    fs::copy(root().join(registry::MODELS), bare.join(registry::MODELS))?;
    fs::create_dir_all(bare.join("doc"))?;
    fs::copy(root().join(registry::TASKS), bare.join(registry::TASKS))?;
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
    // the first registered model in the harness's order is named, whichever that is
    assert!(
        registry::models(&root())?
            .iter()
            .any(|m| err.contains(&format!("{m} has no committed regression trace"))),
        "{err}"
    );
    Ok(())
}

#[test]
fn should_refuse_a_pending_entry_when_its_task_is_unknown_or_done() -> TestResult {
    // given
    let tasks = "\
#### IDX-09: View registry (views driver)
- **Accept:**
  - [x] one
  - [ ] two

#### IDX-12: Daemon host
- [ ] an unticked note outside the Accept list does not keep the task open
- **Accept:**
  - [x] one
  - [X] two
- **Impact:** none
  - [ ] nor does a box under another item

#### LSP-04: Diagnostics
- no box here
";
    let cases = [
        (("views", "IDX-09"), None),
        (
            ("daemon", "IDX-12"),
            Some("IDX-12 is done (every `Accept:` box is ticked)"),
        ),
        (
            ("diagnostics", "LSP-04"),
            Some("LSP-04's card has no `Accept:` box"),
        ),
        (
            ("views", "IDX-99"),
            Some("IDX-99 has no card (`#### IDX-99:`)"),
        ),
        (("views", "IDX-0"), Some("IDX-0 has no card")),
    ];

    // when
    let checked: Vec<_> = cases
        .iter()
        .map(|(entry, _)| registry::check_pending_in(&[*entry], tasks))
        .collect();

    // then
    for ((entry, why), result) in cases.iter().zip(checked) {
        match (why, result) {
            (None, Ok(())) => {}
            (Some(why), Err(error)) => {
                let error = error.to_string();
                assert!(error.contains(why), "{entry:?}: {error}");
                assert!(
                    error.contains(&format!("{} is pending on {}", entry.0, entry.1)),
                    "{error}"
                );
            }
            (want, got) => return Err(format!("{entry:?}: wanted {want:?}, got {got:?}").into()),
        }
    }
    assert_eq!(registry::check_pending(&registry::tasks(&root())?), Ok(()));
    Ok(())
}

#[test]
fn should_warn_of_each_pending_model_when_the_traces_are_replayed() {
    // given
    let args = ["replay".to_owned()];
    let (mut out, mut err) = (Vec::new(), Vec::new());

    // when
    let code = cli::run(&args, &root(), &mut out, &mut err);

    // then
    let (out, err) = (String::from_utf8_lossy(&out), String::from_utf8_lossy(&err));
    assert_eq!(code, cli::OK, "{out}{err}");
    for (model, task) in registry::PENDING {
        assert!(
            err.contains(&format!(
                "warning: {model} is pending: its driver lands with {task}"
            )),
            "{err}"
        );
        assert!(out.contains(&format!("conformance: {model}: ")), "{out}");
    }
    assert!(!err.contains("warning: smoke"), "{err}");
}

#[test]
fn should_read_a_pending_model_s_generated_traces_when_its_driver_has_not_landed() -> TestResult {
    // given: a well-formed and a malformed generated run of the pending diagnostics model
    let scratch = Generated::new("pending")?;
    let daemon = scratch.dir.join("diagnostics");
    fs::create_dir_all(&daemon)?;
    let committed_daemon = fs::read_to_string(
        root().join("formal/traces/regression/diagnostics-close-during-compile.itf.json"),
    )?;
    let run = |text: &str| -> Result<(u8, String, String), Box<dyn Error>> {
        fs::write(daemon.join("trace_0.itf.json"), text)?;
        let args: Vec<String> = [
            "replay",
            "--model",
            "diagnostics",
            "--seed",
            "0x7",
            "--generated",
        ]
        .iter()
        .map(|w| (*w).to_owned())
        .chain([scratch.dir.display().to_string()])
        .collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli::run(&args, &root(), &mut out, &mut err);
        Ok((
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        ))
    };

    // when
    let (good, bad) = (run(&committed_daemon)?, run("{")?);

    // then
    assert_eq!(good.0, cli::OK, "{}{}", good.1, good.2);
    assert!(
        good.1.contains(
            "diagnostics: 1 generated traces read, not replayed (its driver lands with LSP-04)"
        ),
        "{}",
        good.1
    );
    assert_eq!(bad.0, cli::HARNESS, "{}{}", bad.1, bad.2);
    assert!(bad.2.contains("not an ITF trace"), "{}", bad.2);
    Ok(())
}

/// The views regression trace whose build starts at step 3 (`write`, `deliver`, `reconcileStart`).
const VIEWS_RECONCILE: &str = "formal/traces/regression/views-checkout-during-reconcile.itf.json";

#[test]
fn should_diverge_at_the_reconcile_when_the_views_control_publishes_early() -> TestResult {
    // given
    let text = fs::read_to_string(root().join(VIEWS_RECONCILE))?;
    let context = Context {
        model: "views",
        seed: "0x3",
        trace: VIEWS_RECONCILE,
    };

    // when
    let conformed = replay::replay(
        views::driver(),
        &context,
        trace::parse(VIEWS_RECONCILE, &text)?,
    );
    let control = replay::replay(
        views::control_publishing_at_start(),
        &context,
        trace::parse(VIEWS_RECONCILE, &text)?,
    );

    // then
    assert_eq!(conformed, Ok(21));
    let Err(Failure::Diverged(divergence)) = control else {
        return Err(format!("the views control did not diverge: {control:?}").into());
    };
    assert_eq!(divergence.step, 3);
    assert_eq!(divergence.action, "reconcileStart");
    assert!(
        divergence
            .to_string()
            .contains("the build still waits for summaries"),
        "{divergence}"
    );
    Ok(())
}

/// The daemon regression traces with a shutdown, and the step of the first shutdown step in each.
const DAEMON_SHUTDOWNS: [(&str, usize, &str); 3] = [
    (
        "formal/traces/regression/daemon-sigterm-after-stale-start.itf.json",
        10,
        "sigterm",
    ),
    (
        "formal/traces/regression/daemon-idle-exit-races-connect.itf.json",
        15,
        "idleExit",
    ),
    (
        "formal/traces/regression/daemon-swept-socket-stops.itf.json",
        16,
        "noticeSwept",
    ),
];

#[test]
fn should_diverge_at_the_shutdown_when_the_daemon_control_releases_first() -> TestResult {
    for (file, step, action) in DAEMON_SHUTDOWNS {
        // given
        let text = fs::read_to_string(root().join(file))?;
        let context = Context {
            model: "daemon",
            seed: "0x3",
            trace: file,
        };

        // when
        let conformed = replay::replay(daemon::driver(), &context, trace::parse(file, &text)?);
        let control = replay::replay(daemon::control(), &context, trace::parse(file, &text)?);

        // then: the model's daemon still holds its lock; the faulty host has released it
        assert!(conformed.is_ok(), "{file}: {conformed:?}");
        let Err(Failure::Diverged(divergence)) = control else {
            return Err(format!("{file}: the daemon control did not diverge: {control:?}").into());
        };
        assert_eq!(
            (divergence.step, divergence.action.as_str()),
            (step, action),
            "{divergence}"
        );
        assert!(
            divergence.diff().contains("-            lock: 1,"),
            "{divergence}"
        );
        assert!(
            divergence.diff().contains("+            lock: 0,"),
            "{divergence}"
        );
    }
    Ok(())
}

#[test]
fn should_diverge_at_the_publish_when_the_views_control_store_lags() -> TestResult {
    // given
    let text = fs::read_to_string(root().join(VIEWS_RECONCILE))?;
    let context = Context {
        model: "views",
        seed: "0x3",
        trace: VIEWS_RECONCILE,
    };

    // when
    let control = replay::replay(
        views::control(),
        &context,
        trace::parse(VIEWS_RECONCILE, &text)?,
    );

    // then: write, deliver, reconcileStart, checkout, summarise, then publish (step 6)
    let Err(Failure::Diverged(divergence)) = control else {
        return Err(format!("the views control did not diverge: {control:?}").into());
    };
    assert_eq!(divergence.step, 6);
    assert_eq!(divergence.action, "publish");
    assert!(
        divergence
            .to_string()
            .contains("a summary the build names is not stored"),
        "{divergence}"
    );
    Ok(())
}

#[test]
fn should_conform_and_see_the_views_control_diverge_when_views_traces_are_generated() -> TestResult
{
    // given: two committed views traces as one generated run
    let scratch = Generated::new("views")?;
    let views = scratch.dir.join("views");
    fs::create_dir_all(&views)?;
    for (i, name) in [
        "views-two-views-share-a-generation",
        "views-checkout-during-reconcile",
    ]
    .iter()
    .enumerate()
    {
        let text =
            fs::read_to_string(root().join(format!("formal/traces/regression/{name}.itf.json")))?;
        fs::write(views.join(format!("trace_{i}.itf.json")), text)?;
    }
    let args: Vec<String> = ["replay", "--model", "views", "--seed", "0x7", "--generated"]
        .iter()
        .map(|w| (*w).to_owned())
        .chain([scratch.dir.display().to_string()])
        .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());

    // when
    let code = cli::run(&args, &root(), &mut out, &mut err);

    // then
    let (out, err) = (String::from_utf8_lossy(&out), String::from_utf8_lossy(&err));
    assert_eq!(code, cli::OK, "{out}{err}");
    assert!(
        out.contains("views: seed 0x7: 2 generated traces (30 states) conform"),
        "{out}"
    );
    assert!(
        out.contains("views: the negative control diverged as it must, at step 6 of"),
        "{out}"
    );
    assert!(!err.contains("warning: views"), "{err}");
    Ok(())
}
