//! `ARCH-ci-lockstep`'s negative cases (fnd §7.2 rule 3, §3.4): each case is the repository's own
//! `ci.yml`, justfile and `doc/required-checks.md` with one small change that must produce the
//! rule at its path with its reason; the unchanged files must produce nothing.

use std::error::Error;
use std::path::Path;

use jfl_architecture::ci::{self, REQUIRED_CHECKS, RULE, WORKFLOW};
use jfl_architecture::{Finding, tree::MemoryTree};

type TestResult = Result<(), Box<dyn Error>>;

/// The three files the rule reads, as the repository has them.
struct Files {
    workflow: String,
    justfile: String,
    doc: String,
}

fn repo_file(path: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path),
    )?)
}

fn baseline() -> Result<Files, Box<dyn Error>> {
    Ok(Files {
        workflow: repo_file(WORKFLOW)?,
        justfile: repo_file(ci::JUSTFILE)?,
        doc: repo_file(REQUIRED_CHECKS)?,
    })
}

/// Replaces the first `from` in `text` by `to`; an absent `from` is an error, so a case can never
/// pass by changing nothing.
fn edit(text: &mut String, from: &str, to: &str) -> TestResult {
    if !text.contains(from) {
        return Err(format!("the file has no `{from}`").into());
    }
    *text = text.replacen(from, to, 1);
    Ok(())
}

fn findings(files: &Files) -> Vec<Finding> {
    ci::check_files(&files.workflow, &files.justfile, &files.doc)
}

/// Fails unless a finding of the rule is at `at` and its message contains `reason`.
fn assert_reported(findings: &[Finding], at: &str, reason: &str) {
    let hit = findings
        .iter()
        .any(|f| f.rule == RULE && f.at == at && f.message.contains(reason));
    let got: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        hit,
        "expected {RULE} at {at} with `{reason}`; got:\n  {}",
        got.join("\n  ")
    );
}

#[test]
fn should_find_nothing_when_the_files_are_the_repository_s_own() -> TestResult {
    // given
    let files = baseline()?;

    // when
    let found = findings(&files);

    // then
    let report: Vec<String> = found.iter().map(ToString::to_string).collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_stage_runs_in_two_jobs() -> TestResult {
    // given: `nostd` runs in `static` already
    let mut files = baseline()?;
    edit(
        &mut files.workflow,
        "just check-only unit-tests integration-tests doctests script-tests",
        "just check-only unit-tests integration-tests doctests script-tests nostd",
    )?;

    // when
    let found = findings(&files);

    // then
    assert_reported(
        &found,
        WORKFLOW,
        "stage `nostd` runs 2 times (jobs static, tests)",
    );
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_stage_runs_twice_in_one_job() -> TestResult {
    // given
    let mut files = baseline()?;
    edit(
        &mut files.workflow,
        "just check-only architecture deny",
        "just check-only architecture deny\n      - run: just deny",
    )?;

    // when
    let found = findings(&files);

    // then
    assert_reported(&found, WORKFLOW, "stage `deny` runs 2 times");
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_stage_runs_in_no_job() -> TestResult {
    // given
    let mut files = baseline()?;
    edit(
        &mut files.workflow,
        "just check-only architecture deny",
        "just check-only architecture",
    )?;

    // when
    let found = findings(&files);

    // then
    assert_reported(
        &found,
        WORKFLOW,
        "stage `deny` of `just check` runs in no job",
    );
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_check_only_argument_is_not_a_stage() -> TestResult {
    // given
    let mut files = baseline()?;
    edit(
        &mut files.workflow,
        "just check-only architecture deny",
        "just check-only architecture deny formal-typecheck",
    )?;

    // when
    let found = findings(&files);

    // then
    assert_reported(&found, WORKFLOW, "`formal-typecheck` to check-only");
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_step_has_continue_on_error() -> TestResult {
    // given
    let mut files = baseline()?;
    edit(
        &mut files.workflow,
        "      - run: just check-only architecture deny",
        "      - run: just check-only architecture deny\n        continue-on-error: true",
    )?;

    // when
    let found = findings(&files);

    // then
    assert_reported(&found, WORKFLOW, "has `continue-on-error`");
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_job_has_continue_on_error() -> TestResult {
    // given
    let mut files = baseline()?;
    edit(
        &mut files.workflow,
        "    name: Static quality\n",
        "    name: Static quality\n    continue-on-error: true\n",
    )?;

    // when
    let found = findings(&files);

    // then
    assert_reported(&found, WORKFLOW, "job `static` has `continue-on-error`");
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_run_ends_in_or_true() -> TestResult {
    // given
    let mut files = baseline()?;
    edit(
        &mut files.workflow,
        "just check-only architecture deny",
        "just check-only architecture deny || true",
    )?;

    // when
    let found = findings(&files);

    // then
    assert_reported(
        &found,
        WORKFLOW,
        "must be exactly one line `just <recipe> [args]`",
    );
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_run_is_not_a_single_just_command() -> TestResult {
    for probe in [
        "set -u +e; just doctor",
        "set +o pipefail; just doctor",
        "just doctor; exit 0",
        "'just doctor ||:'",
        "rustup toolchain install && just doctor",
        "just doctor || printf x",
        "just doctor &",
        "false && just doctor",
        "just doctor > /dev/null",
        "echo /tmp/shim >> \"$GITHUB_PATH\"; just doctor",
        "echo NEXTEST_PROFILE=default >> \"$GITHUB_ENV\"; just doctor",
        "|\n          just doctor\n          just doctor",
        "just doctor \"$UNDECLARED\"",
        "just $(echo doctor)",
        "bash -c 'just doctor'",
    ] {
        // given
        let mut files = baseline()?;
        edit(
            &mut files.workflow,
            "- run: just doctor",
            &format!("- run: {probe}"),
        )?;

        // when
        let found = findings(&files);

        // then
        assert!(
            found.iter().any(|f| f.rule == RULE
                && f.at == WORKFLOW
                && f.message.contains("job `static`")
                && f.message
                    .contains("must be exactly one line `just <recipe> [args]`")),
            "{probe}: {found:?}"
        );
    }
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_working_directory_moves_the_gate() -> TestResult {
    for (from, to, reason) in [
        (
            "      - run: just doctor",
            "      - run: just doctor\n        working-directory: tools",
            "has a `working-directory`",
        ),
        (
            "    name: Static quality\n",
            "    name: Static quality\n    defaults:\n      run:\n        working-directory: tools\n",
            "job `static` sets a `working-directory`",
        ),
        (
            "    shell: bash\n",
            "    shell: bash\n    working-directory: tools\n",
            "`defaults.run.working-directory` is set",
        ),
    ] {
        // given
        let mut files = baseline()?;
        edit(&mut files.workflow, from, to)?;

        // when
        let found = findings(&files);

        // then
        assert_reported(&found, WORKFLOW, reason);
    }
    Ok(())
}

#[test]
fn should_fail_lockstep_when_an_action_or_its_input_is_not_allowed() -> TestResult {
    for (from, to, reason) in [
        (
            "          env: false\n",
            "          env: false\n          mise_toml: |\n            [env]\n            RUSTFLAGS = \"--cap-lints=allow\"\n",
            "input `mise_toml` is not allowed",
        ),
        (
            "          env: false\n",
            "          env: true\n",
            "input `env` must be `false`",
        ),
        (
            "          install_args: --locked\n",
            "          install_args: just\n",
            "input `install_args` must be `--locked`",
        ),
        (
            "          persist-credentials: false\n",
            "          persist-credentials: true\n",
            "input `persist-credentials` must be `false`",
        ),
        (
            "      - run: just doctor",
            "      - uses: some-org/setup-anything@0123456789abcdef0123456789abcdef01234567\n      - run: just doctor",
            "is not an action the gate allows",
        ),
    ] {
        // given
        let mut files = baseline()?;
        edit(&mut files.workflow, from, to)?;

        // when
        let found = findings(&files);

        // then
        assert_reported(&found, WORKFLOW, reason);
    }
    Ok(())
}

#[test]
fn should_fail_lockstep_when_just_is_given_a_variable_override_or_an_unknown_recipe() -> TestResult
{
    for (probe, reason) in [
        (
            "just cargo=true platform-tests",
            "`cargo=true` after `just` is not a recipe",
        ),
        (
            "just --set cargo true platform-tests",
            "`--set` after `just` is not a recipe",
        ),
        (
            "just no-such-recipe",
            "`no-such-recipe` after `just` is not a recipe",
        ),
        ("just platform-tests cargo=true", "must be exactly one line"),
    ] {
        // given: the doc row edited to match, so only the shape can catch it
        let mut files = baseline()?;
        let at = "    runs-on: macos-15";
        let start = files.workflow.find(at).ok_or("no macos job")?;
        let from = files
            .workflow
            .get(start..)
            .and_then(|rest| rest.find("- run: just platform-tests"))
            .and_then(|i| start.checked_add(i))
            .ok_or("no platform-tests step")?;
        files
            .workflow
            .replace_range(from..from.saturating_add(26), &format!("- run: {probe}"));

        // when
        let found = findings(&files);

        // then
        assert_reported(&found, WORKFLOW, reason);
    }
    Ok(())
}

#[test]
fn should_fail_lockstep_when_mise_action_leaves_env_at_its_default() -> TestResult {
    // given
    let mut files = baseline()?;
    edit(&mut files.workflow, "          env: false\n", "")?;

    // when
    let found = findings(&files);

    // then
    assert_reported(&found, WORKFLOW, "must set its input `env`");
    Ok(())
}

#[test]
fn should_fail_lockstep_when_an_env_entry_is_not_on_the_allowlist() -> TestResult {
    for (from, to, reason) in [
        (
            "    name: Static quality\n",
            "    name: Static quality\n    env:\n      BASH_ENV: ./evil.sh\n",
            "job `static` sets `BASH_ENV`",
        ),
        (
            "  CARGO_TERM_COLOR: always\n",
            "  CARGO_TERM_COLOR: always\n  JUST_JUSTFILE: other.just\n",
            "the workflow sets `JUST_JUSTFILE`",
        ),
        (
            "  CARGO_TERM_COLOR: always\n",
            "  CARGO_TERM_COLOR: always\n  CI: \"false\"\n",
            "the workflow sets `CI`",
        ),
        (
            "          GITHUB_TOKEN: ${{ github.token }}\n",
            "          GITHUB_TOKEN: ${{ github.token }}\n          PATH: /tmp/shim\n",
            "sets `PATH`",
        ),
        (
            "          GITHUB_TOKEN: ${{ github.token }}\n",
            "          GITHUB_TOKEN: ${{ secrets.OTHER }}\n",
            "sets `GITHUB_TOKEN`",
        ),
    ] {
        // given
        let mut files = baseline()?;
        edit(&mut files.workflow, from, to)?;

        // when
        let found = findings(&files);

        // then
        assert_reported(&found, WORKFLOW, reason);
    }
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_job_runs_in_a_container_or_with_services() -> TestResult {
    for (to, reason) in [
        (
            "    container: ubuntu:24.04\n",
            "job `static` has `container:`",
        ),
        (
            "    services:\n      db:\n        image: postgres\n",
            "job `static` has `services:`",
        ),
    ] {
        // given
        let mut files = baseline()?;
        edit(
            &mut files.workflow,
            "    name: Static quality\n",
            &format!("    name: Static quality\n{to}"),
        )?;

        // when
        let found = findings(&files);

        // then
        assert_reported(&found, WORKFLOW, reason);
    }
    Ok(())
}

#[test]
fn should_fail_lockstep_when_platform_tests_runs_other_stages() -> TestResult {
    for to in [
        "platform-tests: unit-tests",
        "platform-tests: unit-tests integration-tests doctests script-tests",
        "platform-tests:",
    ] {
        // given
        let mut files = baseline()?;
        edit(
            &mut files.justfile,
            "platform-tests: unit-tests integration-tests doctests",
            to,
        )?;

        // when
        let found = findings(&files);

        // then
        assert_reported(
            &found,
            ci::JUSTFILE,
            "`platform-tests` must depend on exactly",
        );
    }
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_step_or_a_gating_job_has_an_if() -> TestResult {
    // given
    let mut step = baseline()?;
    edit(
        &mut step.workflow,
        "      - run: just check-only architecture deny",
        "      - run: just check-only architecture deny\n        if: github.ref == 'refs/heads/main'",
    )?;
    let mut job = baseline()?;
    edit(
        &mut job.workflow,
        "    name: Static quality\n",
        "    name: Static quality\n    if: github.event_name == 'push'\n",
    )?;

    // when
    let step_found = findings(&step);
    let job_found = findings(&job);

    // then
    assert_reported(
        &step_found,
        WORKFLOW,
        "has an `if:`: a gating step always runs",
    );
    assert_reported(&job_found, WORKFLOW, "job `static` has an `if:`");
    Ok(())
}

#[test]
fn should_fail_lockstep_when_required_does_not_need_every_job() -> TestResult {
    // given
    let mut files = baseline()?;
    edit(&mut files.workflow, "security, dco]", "security]")?;

    // when
    let found = findings(&files);

    // then
    assert_reported(&found, WORKFLOW, "job `required` does not need `dco`");
    Ok(())
}

#[test]
fn should_fail_lockstep_when_required_is_not_always_run_or_reads_no_result() -> TestResult {
    // given
    let mut condition = baseline()?;
    edit(
        &mut condition.workflow,
        "    if: always()\n",
        "    if: success()\n",
    )?;
    let mut blind = baseline()?;
    edit(
        &mut blind.workflow,
        "join(needs.*.result, ' ')",
        "github.sha",
    )?;

    // when
    let condition_found = findings(&condition);
    let blind_found = findings(&blind);

    // then
    assert_reported(&condition_found, WORKFLOW, "must have `if: always()`");
    assert_reported(&blind_found, WORKFLOW, "is not the aggregate step");
    assert_reported(&blind_found, WORKFLOW, "never reads `needs.*.result`");
    Ok(())
}

#[test]
fn should_fail_lockstep_when_an_env_entry_steers_the_build() -> TestResult {
    for (from, to, reason) in [
        (
            "  NEXTEST_PROFILE: ci\n",
            "  NEXTEST_PROFILE: default\n",
            "the workflow sets `NEXTEST_PROFILE`",
        ),
        (
            "  CARGO_TERM_COLOR: always\n",
            "  CARGO_TERM_COLOR: always\n  RUSTFLAGS: --cap-lints=allow\n",
            "the workflow sets `RUSTFLAGS`",
        ),
        (
            "    name: Static quality\n",
            "    name: Static quality\n    env:\n      CARGO_BUILD_RUSTC_WRAPPER: sccache\n",
            "job `static` sets `CARGO_BUILD_RUSTC_WRAPPER`",
        ),
        (
            "          GITHUB_TOKEN: ${{ github.token }}\n",
            "          GITHUB_TOKEN: ${{ github.token }}\n          MISE_LOCKED: 0\n",
            "sets `MISE_LOCKED`",
        ),
    ] {
        // given
        let mut files = baseline()?;
        edit(&mut files.workflow, from, to)?;

        // when
        let found = findings(&files);

        // then
        assert_reported(&found, WORKFLOW, reason);
    }
    Ok(())
}

#[test]
fn should_fail_lockstep_when_the_shell_is_not_bash() -> TestResult {
    // given
    let mut defaults = baseline()?;
    edit(
        &mut defaults.workflow,
        "    shell: bash\n",
        "    shell: sh\n",
    )?;
    let mut step = baseline()?;
    edit(
        &mut step.workflow,
        "      - run: just doctor",
        "      - run: just doctor\n        shell: sh",
    )?;

    // when
    let defaults_found = findings(&defaults);
    let step_found = findings(&step);

    // then
    assert_reported(
        &defaults_found,
        WORKFLOW,
        "`defaults.run.shell` is not `bash`",
    );
    assert_reported(&step_found, WORKFLOW, "runs a shell other than `bash`");
    Ok(())
}

#[test]
fn should_fail_lockstep_when_the_doc_and_the_workflow_disagree() -> TestResult {
    // given: the doc still says `static` runs `docs`, the workflow moved it
    let mut moved = baseline()?;
    edit(
        &mut moved.workflow,
        "just check-only fmt-check clippy nostd docs",
        "just check-only fmt-check clippy nostd",
    )?;
    edit(
        &mut moved.workflow,
        "just check-only architecture deny",
        "just check-only architecture deny docs",
    )?;
    let mut renamed = baseline()?;
    edit(
        &mut renamed.workflow,
        "name: Security\n",
        "name: Supply chain\n",
    )?;
    let mut extra = baseline()?;
    edit(
        &mut extra.doc,
        "| `required` |",
        "| `fuzz` | Fuzz | `ubuntu-24.04` | `fuzz` |\n| `required` |",
    )?;

    // when
    let moved_found = findings(&moved);
    let renamed_found = findings(&renamed);
    let extra_found = findings(&extra);

    // then
    assert_reported(&moved_found, REQUIRED_CHECKS, "job `static`: the row says");
    assert_reported(
        &moved_found,
        REQUIRED_CHECKS,
        "job `architecture`: the row says",
    );
    assert_reported(
        &renamed_found,
        REQUIRED_CHECKS,
        "job `security`: the row says",
    );
    assert_reported(
        &extra_found,
        REQUIRED_CHECKS,
        "row `fuzz` names a job ci.yml does not have",
    );
    Ok(())
}

#[test]
fn should_fail_lockstep_when_a_file_is_missing_or_names_no_stages() -> TestResult {
    // given
    let files = baseline()?;
    let without_doc = MemoryTree::new()
        .with(WORKFLOW, &files.workflow)
        .with(ci::JUSTFILE, &files.justfile);
    let mut no_stages = baseline()?;
    edit(&mut no_stages.justfile, "check_stages :=", "all_stages :=")?;

    // when
    let missing = ci::check(&without_doc);
    let unknown = findings(&no_stages);

    // then
    assert_reported(&missing, REQUIRED_CHECKS, "the file is missing");
    assert_reported(&unknown, ci::JUSTFILE, "no `check_stages");
    Ok(())
}

#[test]
fn should_read_each_just_command_when_a_script_chains_them() {
    // given
    let run = "set -x\nthen just check-only a b && just dco \"$range\" | tee x; just \\\n  doctor # just quick\n";

    // when
    let invocations = ci::just_invocations(run);

    // then
    assert_eq!(invocations, vec!["check-only a b", "dco", "doctor"]);
}
