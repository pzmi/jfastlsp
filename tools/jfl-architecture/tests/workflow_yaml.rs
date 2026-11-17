//! `ARCH-workflow-yaml`'s negative cases (fnd §7.3, FND-09): each review round's payload against
//! the line checks of `scripts/workflows-check.sh`, decoded here. The YAML is built in the test,
//! never committed as a `.yml` file, and every case names the round it reproduces. These cases
//! are also the acceptance list of the xtask port.

use jfl_architecture::Finding;
use jfl_architecture::workflow_yaml::{self, Allowed, RULE, check_file};

const PATH: &str = ".github/workflows/w.yml";

/// A workflow whose single job runs `steps` (each line indented for a step list).
fn workflow(steps: &str) -> String {
    format!(
        "name: w\non: pull_request\npermissions:\n  contents: read\njobs:\n  job:\n    runs-on: ubuntu-24.04\n    steps:\n{steps}"
    )
}

/// The allowlist with one entry for [`PATH`].
fn allowed() -> Vec<Allowed> {
    vec![Allowed {
        path: PATH.to_owned(),
        line: "shellcheck disable=SC2016 # literal on purpose".to_owned(),
    }]
}

/// Fails unless a finding of the rule contains `reason`.
fn assert_reported(findings: &[Finding], reason: &str) {
    let got: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        findings
            .iter()
            .any(|f| f.rule == RULE && f.message.contains(reason)),
        "expected {RULE} with `{reason}`; got:\n  {}",
        got.join("\n  ")
    );
}

#[test]
fn should_find_nothing_when_the_workflow_uses_plain_and_block_run_values() {
    // given: the control, including the one allowlisted directive
    let text = workflow(
        "      - run: echo hi\n      - run: |\n          # shellcheck disable=SC2016 # literal on purpose\n          echo '$HOME'\n        shell: bash\n",
    );

    // when
    let found = check_file(PATH, &text, &allowed());

    // then
    let report: Vec<String> = found.iter().map(ToString::to_string).collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
}

#[test]
fn should_fail_workflow_yaml_when_a_double_quoted_run_hides_a_directive_behind_x23() {
    // given: round 4's I1, `\x23` decodes to `#` before shellcheck reads the step
    let text = workflow(
        "      - run: \"if true; then \\x23 shellcheck disable=all\\n  rm -rf $HOME/$X/*\\nfi\"\n",
    );

    // when
    let found = check_file(PATH, &text, &allowed());

    // then
    assert_reported(&found, "disables every check");
}

#[test]
fn should_fail_workflow_yaml_when_the_run_key_is_spelled_with_an_escape() {
    // given: round 5's J1, `"\x72un"` decodes to `run`
    let text = workflow("      - \"\\x72un\": \"echo \\x23 shellcheck disable=SC2086\"\n");

    // when
    let found = check_file(PATH, &text, &allowed());

    // then
    assert_reported(&found, "outside the allowlist");
}

#[test]
fn should_fail_workflow_yaml_when_the_run_key_is_a_complex_key() {
    // given: round 5's J1, `? run` then `: value`
    let text = workflow("      - ? run\n        : \"echo \\x23 shellcheck disable=SC2086\"\n");

    // when
    let found = check_file(PATH, &text, &allowed());

    // then
    assert_reported(&found, "outside the allowlist");
}

#[test]
fn should_fail_workflow_yaml_when_a_comment_looks_like_a_block_start() {
    // given: round 6's K1, `steps: # x: |` made the line check skip what follows
    let text = format!(
        "name: w\non: pull_request\njobs:\n  job:\n    runs-on: ubuntu-24.04\n    steps: # x: |\n{}",
        "      - \"\\x72un\": \"if true; then \\x23 shellcheck disable=all\\n  rm -rf $HOME/$X/*\\nfi\"\n"
    );

    // when
    let found = check_file(PATH, &text, &allowed());

    // then
    assert_reported(&found, "disables every check");
}

#[test]
fn should_fail_workflow_yaml_when_an_allowlisted_directive_repeats() {
    // given: round 2's H2, the allowed line twice in one file
    let line = "          # shellcheck disable=SC2016 # literal on purpose\n";
    let text = workflow(&format!(
        "      - run: |\n{line}          echo '$HOME'\n      - run: |\n{line}          echo '$PATH'\n"
    ));

    // when
    let found = check_file(PATH, &text, &allowed());

    // then
    assert_reported(&found, "allowlisted once");
}

#[test]
fn should_fail_workflow_yaml_when_a_step_switches_its_shell() {
    // given: a step that leaves shellcheck by running another interpreter
    let text = workflow("      - run: print('hi')\n        shell: python\n");

    // when
    let found = check_file(PATH, &text, &allowed());

    // then
    assert_reported(&found, "only bash or sh");
}

#[test]
fn should_fail_workflow_yaml_when_anchors_aliases_tags_or_merge_keys_appear() {
    // given
    let cases = [
        (
            workflow("      - run: &cmd echo hi\n      - run: *cmd\n"),
            "an anchor (&)",
        ),
        (
            workflow("      - run: &cmd echo hi\n      - run: *cmd\n"),
            "an alias (*)",
        ),
        (workflow("      - run: !!str echo hi\n"), "a tag (!)"),
        (
            workflow("      - <<: {name: x}\n        run: echo hi\n"),
            "a merge key",
        ),
    ];

    // when
    let found: Vec<(Vec<Finding>, &str)> = cases
        .iter()
        .map(|(text, reason)| (check_file(PATH, text, &allowed()), *reason))
        .collect();

    // then
    for (findings, reason) in found {
        assert_reported(&findings, reason);
    }
}

#[test]
fn should_fail_workflow_yaml_when_the_file_does_not_parse_or_holds_two_documents() {
    // given
    let broken = "jobs: [unclosed\n";
    let two = format!("{}---\nname: second\n", workflow("      - run: echo hi\n"));

    // when
    let broken_found = check_file(PATH, broken, &allowed());
    let two_found = check_file(PATH, &two, &allowed());

    // then
    assert_reported(&broken_found, "does not parse");
    assert_reported(&two_found, "more than one YAML document");
}

#[test]
fn should_read_the_allowlist_from_workflows_check_sh_when_the_list_is_well_formed() {
    // given: the one list both layers read, and a malformed copy
    let script =
        "x\nallowed_suppressions=\"$(cat <<'LIST'\na.sh|shellcheck disable=SC2016|why\nLIST\n)\"\n";
    let malformed = script.replace("|why", "");

    // when
    let parsed = workflow_yaml::allowlist(script);
    let refused = workflow_yaml::allowlist(&malformed);

    // then
    assert_eq!(
        parsed,
        Ok(vec![Allowed {
            path: "a.sh".to_owned(),
            line: "shellcheck disable=SC2016".to_owned(),
        }])
    );
    assert!(refused.is_err(), "{refused:?}");
}
