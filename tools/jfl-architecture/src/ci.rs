//! `ARCH-ci-lockstep` (fnd §7.2 rule 3): `ci.yml`, the `justfile`, xtask's stage catalog and
//! `required-checks.md` say the same thing, and no job of `ci.yml` can fail without failing
//! `Required`.
//!
//! What fails, each a finding of [`RULE`]:
//!
//! - a stage of `check` (xtask's catalog, `bash scripts/xtask.sh list --json`, read by
//!   [`xtask_catalog`] since XT-01) run by no job, or by two (or twice in one), counting
//!   `just check-only <stage>...` and `just <stage>`; a `check-only` argument that is not a stage;
//!   a catalog that names no stage or does not parse;
//! - the aggregate job [`AGGREGATE`] missing, not `if: always()`, not needing every other job, or
//!   not reading `needs.*.result`;
//! - a job other than the aggregate with an `if:`, any job or step with `continue-on-error`, a step
//!   with an `if:`, a job that calls a reusable workflow or has no steps;
//! - a job whose first `just` step comes before a `rustup toolchain install` step: the justfile
//!   builds xtask with the pinned toolchain while it parses, so every recipe needs it (FND-13:
//!   the `security` and `dco` jobs died at parse time from XT-02 on);
//! - a `run:` that is neither one of [`SETUP_RUNS`] nor exactly one line `just <recipe> [args]`, each argument a plain
//!   word or `"$VAR"` of the step's own `env:` (an allowed shape, so `||`, `&&`, `&`, `;`, `set`,
//!   redirections and writes to `$GITHUB_ENV` or `$GITHUB_PATH` cannot appear); the aggregate's
//!   one step differing from [`AGGREGATE_ENV`] and [`AGGREGATE_RUN`];
//! - a `uses:` outside [`ALLOWED_ACTIONS`], or an input of it outside its list (mise-action's
//!   `mise_toml` or `env: true` could export variables the `env:` check never sees); a
//!   mise-action `install_args` that is not `--locked` followed by distinct tools of
//!   [`CI_TOOLS`] (FORMAL-01: each job installs what it runs, and the JVM tools only in
//!   `formal.yml`);
//! - a `working-directory` on a step, a job's or the workflow's defaults;
//! - a shell other than `bash`, or a workflow whose `defaults.run.shell` is not `bash` (only an
//!   explicit `bash` runs with `-o pipefail`);
//! - `platform-tests`, which the native jobs run, depending on anything but [`PLATFORM_TESTS`];
//! - an `env:` entry, at any level, outside [`ALLOWED_ENV`] (name, exact value and level): an
//!   allowlist, so `BASH_ENV`, `PATH`, `CI`, `JUST_*` and the build's variables cannot appear;
//! - a `just` command whose first word is not a recipe of the justfile (a variable override such
//!   as `cargo=true` or a flag); a job with `container:` or `services:`; an allowed action that
//!   omits an input of [`REQUIRED_INPUTS`];
//! - `required-checks.md`'s table under "## The checks" and `ci.yml` disagreeing on a job's id,
//!   display name, runner or the `just` invocations it runs, in order;
//! - `on:` other than exactly [`TRIGGERS`]: `push` to `main` with no other filter, and bare
//!   `pull_request` and `workflow_dispatch` (a `paths-ignore`, a `branches-ignore` or a dropped
//!   `push` would let a landed commit skip CI; FND-08).
//!
//! A stage hidden in a recipe that is not a stage (`platform-tests` excepted, checked above) is not
//! counted, and the doc comparison fails because the doc names each recipe a job runs.
//!
//! Pinned by: tests/ci_lockstep.rs (the repository clean, and one negative case per item above).

use std::collections::{BTreeMap, BTreeSet};

use serde_norway::{Mapping, Value};

use crate::Finding;
use crate::tree::Tree;

/// The rule id.
pub const RULE: &str = "ARCH-ci-lockstep";
/// The workflow whose jobs are the gate in CI.
pub const WORKFLOW: &str = ".github/workflows/ci.yml";
/// The recipes CI runs.
pub const JUSTFILE: &str = "justfile";
/// Where the stage catalog is defined (`list --json` prints it); findings about it point here.
pub const CATALOG: &str = "tools/xtask/src/catalog.rs";
/// The page that documents the jobs.
pub const REQUIRED_CHECKS: &str = "doc/required-checks.md";
/// The aggregate job's id.
pub const AGGREGATE: &str = "required";

/// Where an `env:` entry sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// The workflow's top-level `env:`.
    Workflow,
    /// A job's `env:`.
    Job,
    /// A step's `env:`.
    Step,
}

/// Every environment variable `ci.yml` may set, where, and its only value: an allowlist, so
/// `BASH_ENV`, `PATH`, `CI`, `JUST_*` and the build's own variables are refused by construction.
/// The colour changes no result; `ci` is a nextest profile `_preflight` accepts; the step
/// variables carry the event's fields and the job token to the one recipe that reads them.
pub const ALLOWED_ENV: [(&str, &str, Level); 8] = [
    ("CARGO_TERM_COLOR", "always", Level::Workflow),
    ("NEXTEST_PROFILE", "ci", Level::Workflow),
    ("GITHUB_TOKEN", "${{ github.token }}", Level::Step),
    ("EVENT", "${{ github.event_name }}", Level::Step),
    ("BEFORE", "${{ github.event.before }}", Level::Step),
    (
        "BASE",
        "${{ github.event.pull_request.base.sha }}",
        Level::Step,
    ),
    (
        "HEAD_SHA",
        "${{ github.event.pull_request.head.sha || github.sha }}",
        Level::Step,
    ),
    ("RESULTS", "${{ join(needs.*.result, ' ') }}", Level::Step),
];

/// What `platform-tests` runs on Linux arm64 and macOS (fnd §7.2 rule 4).
pub const PLATFORM_TESTS: [&str; 3] = ["unit-tests", "integration-tests", "doctests"];

/// The `run:` lines a job may have besides `just` commands, exactly: setup that runs no stage.
/// `rustup toolchain install` installs the toolchain rust-toolchain.toml pins.
pub const SETUP_RUNS: [&str; 1] = [TOOLCHAIN_INSTALL];

/// The setup step every job runs before its first `just` command: the justfile's `mise_bins`
/// builds xtask through `scripts/xtask.sh` while `just` parses, and the launcher requires the
/// pinned toolchain installed (XT-02).
pub const TOOLCHAIN_INSTALL: &str = "rustup toolchain install";

/// The aggregate job's one step, exactly: every result must be `success`, so a failed,
/// cancelled or skipped job (or none) fails it.
pub const AGGREGATE_ENV: &str = "RESULTS: ${{ join(needs.*.result, ' ') }}";
/// The aggregate step's `run:`, byte for byte.
pub const AGGREGATE_RUN: &str = r#"[ -n "$RESULTS" ] || { echo "required: no job results" >&2; exit 1; }
for result in $RESULTS; do
  [ "$result" = success ] || { echo "required: a job ended '$result': $RESULTS" >&2; exit 1; }
done
echo "required: every job succeeded ($RESULTS)"
"#;

/// An action input a job may set, with the only value allowed (`None`: any).
pub type Input = (&'static str, Option<&'static str>);

/// The actions a job of `ci.yml` may use, with the `with:` inputs each may set and the one value
/// allowed (`None`: any). Anything else (another action, another input) is refused: an input such
/// as mise-action's `mise_toml` or `env: true` could put a variable or a tool in front of the
/// gate where the `env:` check never looks.
pub const ALLOWED_ACTIONS: [(&str, &[Input]); 3] = [
    (
        "actions/checkout@",
        &[
            ("persist-credentials", Some("false")),
            ("fetch-depth", Some("0")),
        ],
    ),
    (
        "jdx/mise-action@",
        &[
            ("version", None),
            ("sha256", None),
            ("install_args", None),
            ("cache_save", Some("${{ github.ref == 'refs/heads/main' }}")),
            ("env", Some("false")),
        ],
    ),
    (
        "Swatinem/rust-cache@",
        &[
            ("key", None),
            ("save-if", Some("${{ github.ref == 'refs/heads/main' }}")),
        ],
    ),
];

/// Inputs an allowed action must set: mise-action's `env` defaults to true, which would export
/// mise.toml's `[env]`; checkout's `persist-credentials` defaults to true; without
/// `install_args`, mise-action installs every tool of mise.toml without `--locked`.
pub const REQUIRED_INPUTS: [(&str, &str); 3] = [
    ("jdx/mise-action@", "env"),
    ("jdx/mise-action@", "install_args"),
    ("actions/checkout@", "persist-credentials"),
];

/// The mise.toml tools a job of `ci.yml` may install, by name after `--locked` in mise-action's
/// `install_args` (FORMAL-01): the gate's tools, and Node and Quint for the `formal` job. The
/// JDK and Apalache are absent on purpose: only `formal.yml` installs them (about 400 MB a job),
/// so a bare `--locked`, which would install every tool, is refused too.
pub const CI_TOOLS: [&str; 13] = [
    "just",
    "aqua:nextest-rs/nextest/cargo-nextest",
    "aqua:EmbarkStudios/cargo-deny",
    "github:joshrotenberg/adrs",
    "aqua:gitleaks/gitleaks",
    "aqua:rhysd/actionlint",
    "aqua:zizmorcore/zizmor",
    "aqua:suzuki-shunsuke/pinact",
    "aqua:koalaman/shellcheck",
    "aqua:sharkdp/hyperfine",
    "aqua:BurntSushi/ripgrep",
    "node",
    "npm:@informalsystems/quint",
];

/// The problem with a mise-action `install_args`, if any: it must be `--locked` and then one or
/// more distinct tools of [`CI_TOOLS`], separated by single spaces (no other flag).
pub fn install_args_problem(value: &str) -> Option<String> {
    let mut words = value.split(' ');
    if words.next() != Some("--locked") {
        return Some(format!(
            "input `install_args` must start with `--locked`, not `{value}`"
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    for word in words {
        if !CI_TOOLS.contains(&word) {
            return Some(format!(
                "input `install_args` names `{word}`, which is not a tool of ci.rs's CI_TOOLS (no flag, no JDK or Apalache in ci.yml)"
            ));
        }
        if seen.contains(&word) {
            return Some(format!("input `install_args` names `{word}` twice"));
        }
        seen.push(word);
    }
    if seen.is_empty() {
        return Some(
            "input `install_args` must name the tools the job installs after `--locked` (a bare `--locked` installs every tool, the JDK and Apalache included)".to_owned(),
        );
    }
    None
}

/// `ci.yml`'s `on:`, exactly (FND-08): every push to `main` runs, with no path or branch filter,
/// and every pull request and manual dispatch, unfiltered.
pub const TRIGGERS: &str = "push:\n  branches: [main]\npull_request:\nworkflow_dispatch:\n";

/// Runs the rule over the three files of `tree` and the stage catalog `catalog` (the JSON of
/// `xtask list --json`).
pub fn check(tree: &dyn Tree, catalog: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut read = |path: &str| {
        let text = tree.read(path);
        if text.is_none() {
            findings.push(Finding::new(RULE, path, "the file is missing"));
        }
        text
    };
    let (Some(workflow), Some(justfile), Some(doc)) =
        (read(WORKFLOW), read(JUSTFILE), read(REQUIRED_CHECKS))
    else {
        return findings;
    };
    findings.extend(check_files(&workflow, &justfile, &doc, catalog));
    findings
}

/// Runs the rule over the texts of `ci.yml`, the `justfile` and `required-checks.md`, and the stage
/// catalog `catalog` (the JSON of `xtask list --json`).
pub fn check_files(workflow: &str, justfile: &str, doc: &str, catalog: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let stages = match check_stages(catalog) {
        Ok(stages) => stages,
        Err(finding) => return vec![finding],
    };
    let root: Value = match serde_norway::from_str(workflow) {
        Ok(value) => value,
        Err(error) => {
            return vec![Finding::new(
                RULE,
                WORKFLOW,
                format!("does not parse: {error}"),
            )];
        }
    };
    let recipes = recipe_names(justfile);
    let Some(jobs) = root.get("jobs").and_then(Value::as_mapping) else {
        return vec![Finding::new(RULE, WORKFLOW, "no `jobs:` mapping")];
    };
    let mut wf = |message: String| findings.push(Finding::new(RULE, WORKFLOW, message));

    let shell = root
        .get("defaults")
        .and_then(|d| d.get("run"))
        .and_then(|r| r.get("shell"))
        .and_then(Value::as_str);
    if shell != Some("bash") {
        wf(
            "`defaults.run.shell` is not `bash`: only an explicit bash shell runs with -o pipefail"
                .to_owned(),
        );
    }
    if root
        .get("defaults")
        .and_then(|d| d.get("run"))
        .and_then(|r| r.get("working-directory"))
        .is_some()
    {
        wf("`defaults.run.working-directory` is set: the gate runs at the root".to_owned());
    }
    for problem in allowed_env(root.get("env"), "the workflow", Level::Workflow) {
        wf(problem);
    }
    let triggers: Option<Value> = serde_norway::from_str(TRIGGERS).ok();
    if root.get("on").is_none() || root.get("on") != triggers.as_ref() {
        wf(format!(
            "`on:` must be exactly ci.rs's TRIGGERS (push to main with no filter, pull_request, workflow_dispatch), not {}",
            root.get("on")
                .map(scalar)
                .unwrap_or_default()
                .replace('\n', " ")
        ));
    }

    let mut ran: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen: BTreeMap<String, Job> = BTreeMap::new();
    for (key, body) in jobs {
        let Some(id) = key.as_str() else {
            wf("a job id that is not a string".to_owned());
            continue;
        };
        let job = read_job(id, body, &recipes, &mut wf);
        for invocation in &job.invocations {
            for stage in invocation_stages(invocation, &stages, &mut wf, id) {
                ran.entry(stage).or_default().push(id.to_owned());
            }
        }
        seen.insert(id.to_owned(), job);
    }
    for stage in &stages {
        match ran.get(stage).map(Vec::as_slice) {
            None | Some([]) => wf(format!("stage `{stage}` of `just check` runs in no job")),
            Some([_]) => {}
            Some(where_) => wf(format!(
                "stage `{stage}` runs {} times (jobs {}); each stage runs in exactly one job",
                where_.len(),
                where_.join(", ")
            )),
        }
    }
    if seen
        .values()
        .any(|job| job.invocations.iter().any(|i| i == "platform-tests"))
    {
        let deps = recipe_dependencies(justfile, "platform-tests");
        if deps
            .as_deref()
            .map(|d| d.iter().map(String::as_str).collect::<Vec<_>>())
            != Some(PLATFORM_TESTS.to_vec())
        {
            findings.push(Finding::new(
                RULE,
                JUSTFILE,
                format!(
                    "`platform-tests` must depend on exactly {PLATFORM_TESTS:?}, not {deps:?}: \
                     it is what the native jobs run"
                ),
            ));
        }
    }
    let mut wf = |message: String| findings.push(Finding::new(RULE, WORKFLOW, message));
    aggregate(&seen, &mut wf);
    findings.extend(compare_doc(doc, &seen));
    findings
}

/// What the lockstep reads of one job.
#[derive(Debug, Default)]
struct Job {
    name: Option<String>,
    runner: Option<String>,
    if_: Option<String>,
    needs: BTreeSet<String>,
    invocations: Vec<String>,
    reads_results: bool,
}

fn read_job(
    id: &str,
    body: &Value,
    recipes: &BTreeSet<String>,
    wf: &mut impl FnMut(String),
) -> Job {
    let mut job = Job {
        name: body.get("name").and_then(Value::as_str).map(str::to_owned),
        runner: body
            .get("runs-on")
            .and_then(Value::as_str)
            .map(str::to_owned),
        if_: body.get("if").map(scalar),
        ..Job::default()
    };
    match body.get("needs") {
        Some(Value::String(one)) => {
            job.needs.insert(one.clone());
        }
        Some(Value::Sequence(many)) => {
            job.needs
                .extend(many.iter().filter_map(Value::as_str).map(str::to_owned));
        }
        _ => {}
    }
    if job.if_.is_some() && id != AGGREGATE {
        wf(format!("job `{id}` has an `if:`: a gating job always runs"));
    }
    for key in ["container", "services"] {
        if body.get(key).is_some() {
            wf(format!(
                "job `{id}` has `{key}:`: the gate runs on the runner itself"
            ));
        }
    }
    if body.get("continue-on-error").is_some() {
        wf(format!("job `{id}` has `continue-on-error`"));
    }
    if body.get("uses").is_some() {
        wf(format!(
            "job `{id}` calls a reusable workflow, which the lockstep cannot read"
        ));
    }
    if let Some(shell) = body
        .get("defaults")
        .and_then(|d| d.get("run"))
        .and_then(|r| r.get("shell"))
        && shell.as_str() != Some("bash")
    {
        wf(format!("job `{id}` sets a shell other than `bash`"));
    }
    if body
        .get("defaults")
        .and_then(|d| d.get("run"))
        .and_then(|r| r.get("working-directory"))
        .is_some()
    {
        wf(format!(
            "job `{id}` sets a `working-directory`: the gate runs at the root"
        ));
    }
    for problem in allowed_env(body.get("env"), &format!("job `{id}`"), Level::Job) {
        wf(problem);
    }
    let steps = body.get("steps").and_then(Value::as_sequence);
    let Some(steps) = steps.filter(|s| !s.is_empty()) else {
        wf(format!("job `{id}` has no steps"));
        return job;
    };
    let mut toolchain = false;
    for (index, step) in steps.iter().enumerate() {
        let label = step.get("name").and_then(Value::as_str).map_or_else(
            || format!("step {}", index.saturating_add(1)),
            |n| format!("step `{n}`"),
        );
        let at = format!("job `{id}`, {label}");
        if step.get("if").is_some() {
            wf(format!("{at} has an `if:`: a gating step always runs"));
        }
        if step.get("continue-on-error").is_some() {
            wf(format!("{at} has `continue-on-error`"));
        }
        if let Some(shell) = step.get("shell")
            && shell.as_str() != Some("bash")
        {
            wf(format!("{at} runs a shell other than `bash`"));
        }
        for problem in allowed_env(step.get("env"), &at, Level::Step) {
            wf(problem);
        }
        if step.get("working-directory").is_some() {
            wf(format!(
                "{at} has a `working-directory`: a gating step runs at the root"
            ));
        }
        let env_keys: BTreeSet<String> = step
            .get("env")
            .and_then(Value::as_mapping)
            .map(|m| entries(m).map(|(k, _)| k).collect())
            .unwrap_or_default();
        match (step.get("run").and_then(Value::as_str), step.get("uses")) {
            (Some(run), None) if id == AGGREGATE => {
                let env = step.get("env").map(scalar).unwrap_or_default();
                if run == AGGREGATE_RUN && env.trim() == AGGREGATE_ENV {
                    job.reads_results = true;
                } else {
                    wf(format!(
                        "{at} is not the aggregate step: its `env:` and `run:` must be exactly \
                         AGGREGATE_ENV and AGGREGATE_RUN of ci.rs"
                    ));
                }
            }
            (Some(run), None) if SETUP_RUNS.contains(&run.trim()) => {
                toolchain |= run.trim() == TOOLCHAIN_INSTALL;
            }
            (Some(run), None) => match gating_command(run, &env_keys, recipes) {
                Ok(invocation) => {
                    if !toolchain && job.invocations.is_empty() {
                        wf(format!(
                            "{at} runs `just` before a `{TOOLCHAIN_INSTALL}` step: the justfile \
                             builds xtask with the pinned toolchain as it parses"
                        ));
                    }
                    job.invocations.push(invocation);
                }
                Err(problem) => wf(format!("{at}: {problem}")),
            },
            (None, Some(uses)) => {
                for problem in check_action(&scalar(uses), step.get("with")) {
                    wf(format!("{at}: {problem}"));
                }
            }
            _ => wf(format!("{at} must have exactly one of `run:` and `uses:`")),
        }
    }
    job
}

/// The stages one invocation runs; reports a `check-only` argument that is not a stage.
fn invocation_stages(
    invocation: &str,
    stages: &[String],
    wf: &mut impl FnMut(String),
    job: &str,
) -> Vec<String> {
    let mut words = invocation.split(' ');
    match words.next() {
        Some("check-only") => words
            .filter(|w| {
                let known = stages.iter().any(|s| s == w);
                if !known {
                    wf(format!("job `{job}` passes `{w}` to check-only, which is not a stage of `just check`"));
                }
                known
            })
            .map(str::to_owned)
            .collect(),
        Some(recipe) if stages.iter().any(|s| s == recipe) => vec![recipe.to_owned()],
        _ => Vec::new(),
    }
}

fn aggregate(jobs: &BTreeMap<String, Job>, wf: &mut impl FnMut(String)) {
    let Some(required) = jobs.get(AGGREGATE) else {
        wf(format!("no aggregate job `{AGGREGATE}`"));
        return;
    };
    let condition = required.if_.as_deref().map(|c| {
        c.trim()
            .trim_start_matches("${{")
            .trim_end_matches("}}")
            .trim()
            .to_owned()
    });
    if condition.as_deref() != Some("always()") {
        wf(format!(
            "job `{AGGREGATE}` must have `if: always()`, so a failed or skipped job fails it"
        ));
    }
    let others: BTreeSet<String> = jobs.keys().filter(|k| *k != AGGREGATE).cloned().collect();
    let missing: Vec<&String> = others.difference(&required.needs).collect();
    if !missing.is_empty() {
        wf(format!(
            "job `{AGGREGATE}` does not need {}",
            missing
                .iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let unknown: Vec<&String> = required.needs.difference(&others).collect();
    if !unknown.is_empty() {
        wf(format!(
            "job `{AGGREGATE}` needs jobs that do not exist: {unknown:?}"
        ));
    }
    if !required.reads_results {
        wf(format!(
            "job `{AGGREGATE}` never reads `needs.*.result`, so it cannot fail when a job did"
        ));
    }
}

/// The problems of one `env:` mapping at `level` (absent: none) against [`ALLOWED_ENV`].
fn allowed_env(env: Option<&Value>, at: &str, level: Level) -> Vec<String> {
    let Some(env) = env else {
        return Vec::new();
    };
    let Some(map) = env.as_mapping() else {
        return vec![format!("{at} has an `env:` that is not a mapping")];
    };
    entries(map)
        .filter_map(|(name, value)| {
            let allowed = ALLOWED_ENV
                .iter()
                .any(|(n, v, l)| *n == name && *v == value && *l == level);
            (!allowed).then(|| {
                format!(
                    "{at} sets `{name}`, which is not in ci.rs's ALLOWED_ENV with that value at that level (fnd §3.6)"
                )
            })
        })
        .collect()
}

fn entries(map: &Mapping) -> impl Iterator<Item = (String, String)> + '_ {
    map.iter().map(|(k, v)| (scalar(k), scalar(v)))
}

/// A scalar as text; a mapping or sequence as its YAML (for substring checks only).
fn scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => serde_norway::to_string(other).unwrap_or_default(),
    }
}

/// A gating `run:` must be exactly one line, `just <recipe> [arg...]`, each argument a plain
/// word or `"$NAME"` for a variable of the step's own `env:`. Anything else (a second command,
/// `||`, `&&`, `&`, `;`, a redirection, `set`, a write to `$GITHUB_ENV` or `$GITHUB_PATH`) is
/// refused: an allowed shape rather than a list of known bypasses. Returns the normalised
/// invocation.
fn gating_command(
    run: &str,
    env_keys: &BTreeSet<String>,
    recipes: &BTreeSet<String>,
) -> Result<String, String> {
    let line = run.trim();
    let refuse = || {
        format!(
            "`run:` must be exactly one line `just <recipe> [args]` (plain words or \"$VAR\" of the step's env), not `{}`",
            line.replace('\n', "\\n")
        )
    };
    if line.contains('\n') {
        return Err(refuse());
    }
    let mut words = line.split(' ');
    if words.next() != Some("just") {
        return Err(refuse());
    }
    match words.next() {
        Some(recipe) if recipes.contains(recipe) => {}
        Some(other) => {
            return Err(format!(
                "{}: `{other}` after `just` is not a recipe of the justfile (no variable overrides, no flags)",
                refuse()
            ));
        }
        None => return Err(refuse()),
    }
    let mut count = 1_usize;
    for word in words {
        count = count.saturating_add(1);
        let plain = word.starts_with(|c: char| c.is_ascii_alphanumeric())
            && word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':'));
        let variable = word
            .strip_prefix("\"$")
            .and_then(|w| w.strip_suffix('"'))
            .is_some_and(|name| env_keys.contains(name));
        if !(plain || variable) {
            return Err(refuse());
        }
    }
    if count == 0 {
        return Err(refuse());
    }
    just_invocations(line).into_iter().next().ok_or_else(refuse)
}

/// The problems of one `uses:` step against [`ALLOWED_ACTIONS`].
fn check_action(uses: &str, with: Option<&Value>) -> Vec<String> {
    let Some((_, inputs)) = ALLOWED_ACTIONS.iter().find(|(a, _)| uses.starts_with(a)) else {
        return vec![format!("`uses: {uses}` is not an action the gate allows")];
    };
    let mut problems: Vec<String> = REQUIRED_INPUTS
        .iter()
        .filter(|(action, key)| {
            uses.starts_with(action) && with.and_then(|w| w.get(*key)).is_none()
        })
        .map(|(_, key)| format!("`{uses}` must set its input `{key}`"))
        .collect();
    let Some(with) = with else {
        return problems;
    };
    let Some(map) = with.as_mapping() else {
        problems.push("`with:` is not a mapping".to_owned());
        return problems;
    };
    problems.extend(entries(map).filter_map(|(key, value)| {
        if uses.starts_with("jdx/mise-action@") && key == "install_args" {
            return install_args_problem(&value).map(|p| format!("`{uses}` {p}"));
        }
        match inputs.iter().find(|(k, _)| *k == key) {
            Some((_, None)) => None,
            Some((_, Some(only))) if *only == value => None,
            Some((_, Some(only))) => Some(format!("`{uses}` input `{key}` must be `{only}`")),
            None => Some(format!("`{uses}` input `{key}` is not allowed")),
        }
    }));
    problems
}

/// Every `just` command of a `run:` script, normalised to `check-only <stage>...` or the bare
/// recipe name (a recipe's arguments are not part of the contract).
pub fn just_invocations(run: &str) -> Vec<String> {
    let joined = run.replace("\\\n", " ");
    let mut found = Vec::new();
    for line in joined.lines() {
        let line = line.split(" #").next().unwrap_or_default();
        for command in line.split(['&', '|', ';']) {
            let mut words = command
                .split_whitespace()
                .skip_while(|w| matches!(*w, "then" | "do" | "else" | "exec" | "{" | "("))
                .map(|w| w.trim_matches(['"', '\'']));
            if words.next() != Some("just") {
                continue;
            }
            let Some(recipe) = words.next() else {
                found.push(String::new());
                continue;
            };
            if recipe == "check-only" {
                let stages: Vec<&str> = words.collect();
                found.push(
                    format!("check-only {}", stages.join(" "))
                        .trim_end()
                        .to_owned(),
                );
            } else {
                found.push(recipe.to_owned());
            }
        }
    }
    found
}

/// The names of the justfile's recipes: a line at column 0 that is `name [params]:` (not `:=`).
pub fn recipe_names(justfile: &str) -> BTreeSet<String> {
    justfile
        .lines()
        .filter_map(|line| {
            let head = line.strip_prefix('@').unwrap_or(line);
            let (before, after) = head.split_once(':')?;
            if after.starts_with('=') || head.starts_with(|c: char| !c.is_ascii_alphanumeric()) {
                return None;
            }
            let name = before.split_whitespace().next()?;
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .then(|| name.to_owned())
        })
        .collect()
}

/// The dependencies of a recipe without parameters (`name: dep dep`), from its header line.
pub fn recipe_dependencies(justfile: &str, recipe: &str) -> Option<Vec<String>> {
    justfile.lines().find_map(|line| {
        let rest = line.strip_prefix(recipe)?.strip_prefix(':')?;
        if rest.starts_with('=') {
            return None;
        }
        let deps = rest.split('#').next().unwrap_or_default();
        Some(deps.split_whitespace().map(str::to_owned).collect())
    })
}

/// One row of xtask's stage catalog, as `list --json` prints it; any other field fails the parse.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogRow {
    id: String,
    command: String,
    quick: bool,
    check: bool,
}

/// The stages of `check`, in order, from the JSON of `xtask list --json`: the ids of the rows
/// whose `check` is true. A catalog that does not parse, names no stage, has a row without a
/// command or a `quick` stage that `check` does not run, is a finding at [`CATALOG`]. Pinned by
/// tests/ci_lockstep.rs (`should_fail_lockstep_when_a_file_is_missing_or_names_no_stages`).
pub fn check_stages(catalog: &str) -> Result<Vec<String>, Finding> {
    let rows: Vec<CatalogRow> = serde_json::from_str(catalog).map_err(|error| {
        Finding::new(
            RULE,
            CATALOG,
            format!("`xtask list --json` does not parse: {error}"),
        )
    })?;
    let mut stages = Vec::new();
    for CatalogRow {
        id,
        command,
        quick,
        check,
    } in rows
    {
        if command.trim().is_empty() || (quick && !check) {
            return Err(Finding::new(
                RULE,
                CATALOG,
                format!(
                    "stage `{id}`: every row names its command, and `quick` runs only stages of `check`"
                ),
            ));
        }
        if check {
            stages.push(id);
        }
    }
    if stages.is_empty() {
        return Err(Finding::new(
            RULE,
            CATALOG,
            "`xtask list --json` names no stage of `check`: the gate's stages are unknown",
        ));
    }
    Ok(stages)
}

/// The JSON of `bash <root>/scripts/xtask.sh list --json`: the stage catalog of the checkout at
/// `root`, built and listed the way the gate runs it. A failure is a finding at [`CATALOG`].
pub fn xtask_catalog(root: &std::path::Path) -> Result<String, Finding> {
    let output = std::process::Command::new("bash")
        .arg(root.join("scripts/xtask.sh"))
        .args(["list", "--json"])
        .current_dir(root)
        .output()
        .map_err(|error| {
            Finding::new(
                RULE,
                CATALOG,
                format!("cannot run scripts/xtask.sh: {error}"),
            )
        })?;
    if !output.status.success() {
        return Err(Finding::new(
            RULE,
            CATALOG,
            format!(
                "`scripts/xtask.sh list --json` failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| Finding::new(RULE, CATALOG, "`xtask list --json` printed no UTF-8"))
}

/// One row of the doc's table: id, display name, runner, `just` invocations.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    name: String,
    runner: String,
    invocations: Vec<String>,
}

fn compare_doc(doc: &str, jobs: &BTreeMap<String, Job>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut fail = |message: String| findings.push(Finding::new(RULE, REQUIRED_CHECKS, message));
    let Some(section) = doc
        .split("\n## The checks")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
    else {
        fail("no `## The checks` section".to_owned());
        return findings;
    };
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for line in section.lines().map(str::trim) {
        if !line.starts_with("| `") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let [_, id, name, runner, runs, ..] = cells.as_slice() else {
            fail(format!("a row with fewer than four cells: {line}"));
            continue;
        };
        let id = id.trim_matches('`').to_owned();
        let invocations = runs
            .split('`')
            .skip(1)
            .step_by(2)
            .map(|fragment| {
                just_invocations(&format!("just {fragment}"))
                    .into_iter()
                    .next()
                    .unwrap_or_default()
            })
            .collect();
        let row = Row {
            name: (*name).to_owned(),
            runner: runner.trim_matches('`').to_owned(),
            invocations,
        };
        if rows.insert(id.clone(), row).is_some() {
            fail(format!("job `{id}` has two rows"));
        }
    }
    for (id, job) in jobs {
        let Some(row) = rows.remove(id) else {
            fail(format!("job `{id}` of ci.yml has no row"));
            continue;
        };
        let actual = Row {
            name: job.name.clone().unwrap_or_default(),
            runner: job.runner.clone().unwrap_or_default(),
            invocations: job.invocations.clone(),
        };
        if row != actual {
            fail(format!(
                "job `{id}`: the row says {row:?} but ci.yml has {actual:?}"
            ));
        }
    }
    for id in rows.keys() {
        fail(format!("row `{id}` names a job ci.yml does not have"));
    }
    findings
}
