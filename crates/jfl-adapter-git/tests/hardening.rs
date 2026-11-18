//! The hardening of `cli::Git` against what configures `git` (plan §11): nothing the repository's
//! or the user's configuration names is run, the index is never written, and output that is too
//! large or not in the asked-for form is refused whole. Each "is not run" test first shows, with a
//! plain `git`, that the fixture does run when nothing stops it.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use jfl_adapter_git::{GitConfig, GitRepoProbe};
use jfl_application::{GitStep, RepoProbeError, Worktree};
use jfl_process::{Command, Runner};
use jfl_testkit::exec::InlinePool;
use jfl_testkit::fixtures::git::Repo;

#[cfg(test)]
mod support;
use support::Scratch;

/// A search path that holds `git` on the platforms the tests run on (the tests hold no `env`
/// grant to read the real one).
const SEARCH_PATH: &str = "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin";

#[cfg(test)]
/// An executable script at `path` that creates `marker` when run.
fn marker_script(path: &Path, marker: &Path) {
    fs::write(
        path,
        format!("#!/bin/sh\n: > '{}'\nexit 0\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(test)]
fn setup(scratch: &Scratch) -> (Repo, Worktree) {
    let repo = Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    let worktree = scratch
        .probe()
        .resolve_blocking(&scratch.uri("main"))
        .unwrap()
        .unwrap();
    (repo, worktree)
}

#[test]
fn should_not_run_the_repositorys_fsmonitor_when_reading_the_status() {
    // given
    let scratch = Scratch::new("fsmonitor");
    let (repo, worktree) = setup(&scratch);
    let (script, marker) = (
        scratch.path().join("monitor.sh"),
        scratch.path().join("ran"),
    );
    marker_script(&script, &marker);
    repo.git(&["config", "core.fsmonitor", script.to_str().unwrap()])
        .unwrap();
    repo.git(&["status", "--porcelain"]).unwrap();
    assert!(marker.exists(), "control: a plain status runs the monitor");
    fs::remove_file(&marker).unwrap();

    // when
    scratch
        .probe()
        .changed_since_blocking(&worktree, None)
        .unwrap();
    scratch.probe().tracked_java_blocking(&worktree).unwrap();

    // then
    assert!(!marker.exists());
}

#[test]
fn should_not_run_an_external_diff_when_comparing_trees() {
    // given
    let scratch = Scratch::new("extdiff");
    let (repo, worktree) = setup(&scratch);
    let probe = scratch.probe();
    let base = probe.head_tree_blocking(&worktree).unwrap().unwrap();
    repo.write("src/A.java", b"class A { int x; }\n").unwrap();
    repo.commit_all("second").unwrap();
    let (script, marker) = (scratch.path().join("diff.sh"), scratch.path().join("ran"));
    marker_script(&script, &marker);
    repo.git(&["config", "diff.external", script.to_str().unwrap()])
        .unwrap();
    repo.git(&["diff", "HEAD~1", "HEAD"]).unwrap();
    assert!(
        marker.exists(),
        "control: a plain diff runs the external diff"
    );
    fs::remove_file(&marker).unwrap();

    // when
    let changed = probe
        .changed_since_blocking(&worktree, Some(&base))
        .unwrap();

    // then
    assert_eq!(changed.committed.len(), 1);
    assert!(!marker.exists());
}

#[test]
fn should_not_read_the_users_global_configuration_when_running_git() {
    // given
    let scratch = Scratch::new("global");
    let (_repo, worktree) = setup(&scratch);
    let home = scratch.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let (script, marker) = (
        scratch.path().join("monitor.sh"),
        scratch.path().join("ran"),
    );
    marker_script(&script, &marker);
    fs::write(
        home.join(".gitconfig"),
        format!("[core]\n\tfsmonitor = {}\n", script.display()),
    )
    .unwrap();
    let env: BTreeMap<OsString, OsString> = [
        ("PATH".into(), SEARCH_PATH.into()),
        ("HOME".into(), home.clone().into_os_string()),
    ]
    .into_iter()
    .collect();
    let runner = Runner::with_env(env);
    let plain = Command::new("git")
        .inherit_env("PATH")
        .inherit_env("HOME")
        .args(["-C", worktree.root.path(), "status", "--porcelain"]);
    runner.run(&plain, Duration::from_secs(30)).unwrap();
    assert!(
        marker.exists(),
        "control: git reads ~/.gitconfig when not told otherwise"
    );
    fs::remove_file(&marker).unwrap();
    let probe = GitRepoProbe::with_runner(
        InlinePool,
        runner,
        GitConfig {
            ceiling: Some(scratch.path().to_path_buf()),
            ..GitConfig::default()
        },
    );

    // when
    probe.changed_since_blocking(&worktree, None).unwrap();

    // then
    assert!(!marker.exists());
}

#[test]
fn should_leave_the_index_unwritten_when_a_status_would_refresh_it() {
    // given
    let scratch = Scratch::new("index");
    let (repo, worktree) = setup(&scratch);
    let probe = scratch.probe();
    let touch = || {
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        File::options()
            .write(true)
            .open(repo.root().join("src/A.java"))
            .unwrap()
            .set_modified(later)
            .unwrap();
    };
    touch();
    let before = probe.snapshot_blocking(&worktree).unwrap();

    // when
    probe.changed_since_blocking(&worktree, None).unwrap();
    let after = probe.snapshot_blocking(&worktree).unwrap();

    // then
    assert_eq!(before, after);
    repo.git(&["status", "--porcelain"]).unwrap();
    let refreshed = probe.snapshot_blocking(&worktree).unwrap();
    assert_ne!(
        after, refreshed,
        "control: a plain status rewrites the index"
    );
}

#[test]
fn should_refuse_the_output_whole_when_it_passes_the_limit() {
    // given
    let scratch = Scratch::new("limit");
    let (_repo, worktree) = setup(&scratch);
    let probe = GitRepoProbe::new(
        InlinePool,
        GitConfig {
            ceiling: Some(scratch.path().to_path_buf()),
            max_output: 4,
            ..GitConfig::default()
        },
    );

    // when
    let outcome = probe.tracked_java_blocking(&worktree);

    // then
    assert_eq!(
        outcome,
        Err(RepoProbeError::OutputTooLarge(GitStep::LsFiles))
    );
}

#[cfg(test)]
/// An adapter whose `git` is `program`.
fn with_program(scratch: &Scratch, program: &str) -> GitRepoProbe<InlinePool> {
    GitRepoProbe::new(
        InlinePool,
        GitConfig {
            program: program.into(),
            ceiling: Some(PathBuf::from(scratch.path())),
            ..GitConfig::default()
        },
    )
}

#[test]
fn should_refuse_the_output_when_it_is_not_in_the_asked_for_form() {
    // given
    let scratch = Scratch::new("malformed");
    let (_repo, worktree) = setup(&scratch);
    let echo = with_program(&scratch, "/bin/echo");

    // when
    let head = echo.head_tree_blocking(&worktree);
    let tracked = echo.tracked_java_blocking(&worktree);

    // then
    assert_eq!(head, Err(RepoProbeError::Malformed(GitStep::HeadTree)));
    assert_eq!(tracked, Err(RepoProbeError::Malformed(GitStep::LsFiles)));
}

#[test]
fn should_say_git_is_missing_when_the_program_cannot_start() {
    // given
    let scratch = Scratch::new("missing");
    let (_repo, worktree) = setup(&scratch);
    let missing = with_program(&scratch, "/nonexistent/git");

    // when
    let outcome = missing.head_tree_blocking(&worktree);
    let resolved = missing.resolve_blocking(&scratch.uri("main/src/A.java"));

    // then
    assert_eq!(outcome, Err(RepoProbeError::GitMissing));
    assert_eq!(resolved, Err(RepoProbeError::GitMissing));
}

/// Rewrites `src/A.java` with the same bytes and a new mtime, so a status must rehash it.
#[cfg(test)]
fn retouch(repo: &Repo, secs: u64) {
    let path = repo.root().join("src/A.java");
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, bytes).unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(
            SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_secs(secs))
                .unwrap(),
        )
        .unwrap();
}

#[test]
fn should_run_no_filter_when_the_repository_configures_one() {
    for variable in ["clean", "process"] {
        // given
        let scratch = Scratch::new("filter");
        let (repo, worktree) = setup(&scratch);
        repo.write(".gitattributes", b"*.java filter=evil\n")
            .unwrap();
        repo.commit_all("attributes").unwrap();
        let (script, marker) = (scratch.path().join("filter.sh"), scratch.path().join("ran"));
        marker_script(&script, &marker);
        repo.git(&[
            "config",
            &format!("filter.evil.{variable}"),
            script.to_str().unwrap(),
        ])
        .unwrap();
        retouch(&repo, 2_000_000_000);
        let _ = repo.git(&["status", "--porcelain"]);
        assert!(
            marker.exists(),
            "control: a plain status runs the {variable} filter"
        );
        fs::remove_file(&marker).unwrap();
        retouch(&repo, 2_000_000_100);
        let base = scratch
            .probe()
            .head_tree_blocking(&worktree)
            .unwrap()
            .unwrap();

        // when
        let first = scratch.probe().changed_since_blocking(&worktree, None);
        let second = scratch
            .probe()
            .changed_since_blocking(&worktree, Some(&base));

        // then
        assert!(
            first.is_ok() && second.is_ok(),
            "{variable}: {first:?} {second:?}"
        );
        assert!(!marker.exists(), "the {variable} filter ran");
    }
}

#[test]
fn should_ignore_the_users_global_ignore_and_attributes_when_reading_the_status() {
    // given
    let scratch = Scratch::new("excludes");
    let (repo, worktree) = setup(&scratch);
    let home = scratch.path().join("home");
    fs::create_dir_all(home.join(".config/git")).unwrap();
    fs::write(home.join(".config/git/ignore"), b"Secret.java\n").unwrap();
    fs::write(home.join(".config/git/attributes"), b"*.java filter=evil\n").unwrap();
    repo.write("src/Secret.java", b"class Secret {}\n").unwrap();
    let env: BTreeMap<OsString, OsString> = [
        ("PATH".into(), SEARCH_PATH.into()),
        ("HOME".into(), home.clone().into_os_string()),
    ]
    .into_iter()
    .collect();
    let runner = Runner::with_env(env);
    let plain = Command::new("git")
        .inherit_env("PATH")
        .inherit_env("HOME")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-C",
            worktree.root.path(),
            "status",
            "--porcelain",
            "--untracked-files=all",
        ]);
    let control = runner.run(&plain, Duration::from_secs(30)).unwrap();
    assert!(
        !String::from_utf8_lossy(control.stdout.bytes()).contains("Secret.java"),
        "control: git reads ~/.config/git/ignore even with GIT_CONFIG_GLOBAL=/dev/null"
    );
    let probe = GitRepoProbe::with_runner(
        InlinePool,
        runner,
        GitConfig {
            ceiling: Some(scratch.path().to_path_buf()),
            ..GitConfig::default()
        },
    );

    // when
    let changed = probe.changed_since_blocking(&worktree, None).unwrap();

    // then
    let untracked: Vec<&str> = changed.untracked.iter().map(|p| p.as_str()).collect();
    assert_eq!(untracked, ["src/Secret.java"]);
}

#[test]
fn should_read_the_commits_own_tree_when_a_replace_ref_exists() {
    // given
    let scratch = Scratch::new("replace");
    let (repo, worktree) = setup(&scratch);
    let own = repo
        .git(&["rev-parse", "HEAD^{tree}"])
        .unwrap()
        .trim()
        .to_owned();
    let first = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_owned();
    repo.write("src/B.java", b"class B {}\n").unwrap();
    repo.commit_all("other").unwrap();
    let other = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_owned();
    repo.git(&["reset", "--quiet", "--hard", &first]).unwrap();
    repo.git(&["replace", &first, &other]).unwrap();
    assert_ne!(
        repo.git(&["rev-parse", "HEAD^{tree}"]).unwrap().trim(),
        own,
        "control: a plain git follows the replace ref"
    );

    // when
    let tree = scratch
        .probe()
        .head_tree_blocking(&worktree)
        .unwrap()
        .unwrap();

    // then
    assert_eq!(tree.as_str(), own);
}
