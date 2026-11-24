//! Innermost-root resolution on real repositories (plan §4.1, §4.2, ADR 0017), and what each Git
//! operation does to a worktree's stat set and changed-file set (plan §4.3).

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use jfl_adapter_git::{GitConfig, GitRepoProbe};
use jfl_testkit::exec::InlinePool;

use jfl_application::{
    ChangedFiles, GitSnapshot, ObjectId, RepoProbeError, Worktree, WorktreeKind,
};
use jfl_testkit::fixtures::git::Repo;

#[cfg(test)]
mod support;
use support::Scratch;

#[test]
fn should_resolve_each_agent_worktree_to_itself_when_they_nest_in_the_main_checkout() {
    // given
    let scratch = Scratch::new("agents");
    let main = Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    for name in ["a", "b", "c"] {
        main.add_agent_worktree(name).unwrap();
    }
    main.add_worktree(&scratch.path().join("sibling"), "sibling")
        .unwrap();
    let probe = scratch.probe();

    // when
    let resolved: Vec<(String, WorktreeKind)> = [
        "main/src/A.java",
        "main/.claude/worktrees/a/src/A.java",
        "main/.claude/worktrees/b/src",
        "main/.claude/worktrees/c/src/new/Missing.java",
        "main/.claude/worktrees",
        "sibling/src/A.java",
    ]
    .iter()
    .map(|path| {
        let worktree = probe.resolve_blocking(&scratch.uri(path)).unwrap().unwrap();
        (worktree.root.path().to_owned(), worktree.kind)
    })
    .collect();

    // then
    let at = |path: &str| scratch.path().join(path).to_str().unwrap().to_owned();
    assert_eq!(
        resolved,
        [
            (at("main"), WorktreeKind::Main),
            (at("main/.claude/worktrees/a"), WorktreeKind::Linked),
            (at("main/.claude/worktrees/b"), WorktreeKind::Linked),
            (at("main/.claude/worktrees/c"), WorktreeKind::Linked),
            (at("main"), WorktreeKind::Main),
            (at("sibling"), WorktreeKind::Linked),
        ]
    );
}

#[test]
fn should_resolve_to_a_new_nested_repository_when_it_appears_after_a_resolve() {
    // given
    let scratch = Scratch::new("late");
    let main = Repo::init(
        &scratch.path().join("main"),
        &[("lib/L.java", b"class L {}\n")],
    )
    .unwrap();
    let probe = scratch.probe();
    let before = probe
        .resolve_blocking(&scratch.uri("main/lib/L.java"))
        .unwrap()
        .unwrap();

    // when
    main.git(&["-C", "lib", "init", "--quiet"]).unwrap();
    let after = probe
        .resolve_blocking(&scratch.uri("main/lib/L.java"))
        .unwrap()
        .unwrap();

    // then
    assert_eq!(before.root, scratch.uri("main"));
    assert_eq!(after.root, scratch.uri("main/lib"));
    assert_ne!(after.repo(), before.repo());
}

#[test]
fn should_resolve_to_the_canonical_root_when_the_path_goes_through_a_link() {
    // given
    let scratch = Scratch::new("link");
    Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    symlink(
        scratch.path().join("main/src"),
        scratch.path().join("alias"),
    )
    .unwrap();

    // when
    let worktree = scratch
        .probe()
        .resolve_blocking(&scratch.uri("alias/A.java"))
        .unwrap()
        .unwrap();

    // then
    assert_eq!(worktree.root, scratch.uri("main"));
}

#[test]
fn should_resolve_the_directory_and_keep_the_rest_when_a_path_goes_through_a_link() {
    // given: a worktree `main`, `via` a link to it
    let scratch = Scratch::new("canonical");
    Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    symlink(scratch.path().join("main"), scratch.path().join("via")).unwrap();
    let canonical =
        |path: &str| jfl_adapter_git::resolve::canonical_path(&scratch.path().join(path)).unwrap();

    // when
    let file = canonical("via/src/A.java");
    let missing = canonical("via/src/new/B.java");
    let dir = canonical("via/src");
    let root = scratch
        .probe()
        .resolve_blocking(&scratch.uri("via/src/new/B.java"))
        .unwrap()
        .unwrap()
        .root;

    // then: the directory's links resolved and the rest as written, under the root the probe
    // resolves the same path to (the daemon binds a request by this path, FND-17)
    assert_eq!(file, scratch.path().join("main/src/A.java"));
    assert_eq!(missing, scratch.path().join("main/src/new/B.java"));
    assert_eq!(dir, scratch.path().join("main/src"));
    assert_eq!(root, scratch.uri("main"));
    assert!(missing.starts_with(root.path()));
}

#[test]
fn should_refuse_a_root_when_its_git_file_points_nowhere() {
    // given
    let scratch = Scratch::new("broken");
    let dir = scratch.path().join("broken");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(".git"), b"gitdir: /nonexistent/worktrees/x\n").unwrap();

    // when
    let outcome = scratch
        .probe()
        .resolve_blocking(&scratch.uri("broken/A.java"));

    // then
    assert_eq!(outcome, Err(jfl_application::RepoProbeError::NotAWorktree));
}

/// What one operation did: whether the stat set moved, and the changed set before and after.
struct Observed {
    stat_moved: bool,
    before: ChangedFiles,
    after: ChangedFiles,
}

#[cfg(test)]
fn observe(
    scratch: &Scratch,
    worktree: &Worktree,
    base: &ObjectId,
    operation: impl FnOnce(),
) -> Observed {
    let probe = scratch.probe();
    let snap = || -> GitSnapshot { probe.snapshot_blocking(worktree).unwrap() };
    let changed = || probe.changed_since_blocking(worktree, Some(base)).unwrap();
    let (stat, before) = (snap(), changed());
    operation();
    let (stat_after, after) = (snap(), changed());
    Observed {
        stat_moved: stat != stat_after,
        before,
        after,
    }
}

#[cfg(test)]
fn paths(changed: &ChangedFiles) -> Vec<&str> {
    changed.paths().into_iter().map(|p| p.as_str()).collect()
}

#[cfg(test)]
/// A main checkout with `src/A.java` and `src/B.java`, its first tree, and its worktree.
fn repo(scratch: &Scratch) -> (Repo, Worktree, ObjectId) {
    let main = Repo::init(
        &scratch.path().join("main"),
        &[
            ("src/A.java", b"class A {}\n"),
            ("src/B.java", b"class B {}\n"),
        ],
    )
    .unwrap();
    let probe = scratch.probe();
    let worktree = probe
        .resolve_blocking(&scratch.uri("main"))
        .unwrap()
        .unwrap();
    let base = probe.head_tree_blocking(&worktree).unwrap().unwrap();
    (main, worktree, base)
}

#[test]
fn should_move_the_stat_and_the_changed_set_when_a_commit_lands() {
    // given
    let scratch = Scratch::new("commit");
    let (main, worktree, base) = repo(&scratch);
    main.write("src/A.java", b"class A { int c; }\n").unwrap();
    main.stage(".").unwrap();

    // when
    let seen = observe(&scratch, &worktree, &base, || main.commit("c").unwrap());

    // then
    assert!(seen.stat_moved);
    assert_eq!(
        (seen.before.committed.len(), seen.before.working.len()),
        (0, 1)
    );
    assert_eq!(
        (seen.after.committed.len(), seen.after.working.len()),
        (1, 0)
    );
}

#[test]
fn should_move_the_stat_and_the_changed_set_when_a_branch_is_checked_out() {
    // given
    let scratch = Scratch::new("checkout");
    let (main, worktree, base) = repo(&scratch);
    main.git(&["checkout", "--quiet", "-b", "other"]).unwrap();
    main.write("src/C.java", b"class C {}\n").unwrap();
    main.commit_all("other").unwrap();
    main.git(&["checkout", "--quiet", "main"]).unwrap();

    // when
    let seen = observe(&scratch, &worktree, &base, || {
        main.git(&["checkout", "--quiet", "other"]).unwrap();
    });

    // then
    assert!(seen.stat_moved);
    assert_eq!(paths(&seen.before), Vec::<&str>::new());
    assert_eq!(paths(&seen.after), ["src/C.java"]);
}

#[test]
fn should_move_the_stat_and_the_changed_set_when_changes_are_stashed() {
    // given
    let scratch = Scratch::new("stash");
    let (main, worktree, base) = repo(&scratch);
    main.write("src/A.java", b"class A { int s; }\n").unwrap();

    // when
    let seen = observe(&scratch, &worktree, &base, || {
        main.git(&["stash", "push", "--quiet"]).unwrap();
    });

    // then
    assert!(seen.stat_moved);
    assert_eq!(paths(&seen.before), ["src/A.java"]);
    assert_eq!(paths(&seen.after), Vec::<&str>::new());
}

#[test]
fn should_move_the_stat_and_the_changed_set_when_reset_hard_or_soft() {
    for mode in ["--hard", "--soft"] {
        // given
        let scratch = Scratch::new("reset");
        let (main, worktree, _) = repo(&scratch);
        main.write("src/B.java", b"class B { int r; }\n").unwrap();
        main.commit_all("second").unwrap();
        let second = scratch
            .probe()
            .head_tree_blocking(&worktree)
            .unwrap()
            .unwrap();

        // when
        let seen = observe(&scratch, &worktree, &second, || {
            main.git(&["reset", "--quiet", mode, "HEAD~1"]).unwrap();
        });

        // then
        assert!(seen.stat_moved, "{mode}");
        assert_eq!(paths(&seen.before), Vec::<&str>::new(), "{mode}");
        assert_eq!(seen.after.committed.len(), 1, "{mode}");
        assert_eq!(
            seen.after.working.len(),
            usize::from(mode == "--soft"),
            "{mode}"
        );
    }
}

#[test]
fn should_move_the_stat_but_not_the_changed_set_when_a_change_is_staged() {
    // given
    let scratch = Scratch::new("stage");
    let (main, worktree, base) = repo(&scratch);
    main.write("src/A.java", b"class A { int g; }\n").unwrap();

    // when
    let seen = observe(&scratch, &worktree, &base, || {
        main.stage("src/A.java").unwrap()
    });

    // then
    assert!(seen.stat_moved);
    assert_eq!(paths(&seen.before), ["src/A.java"]);
    assert_eq!(paths(&seen.after), ["src/A.java"]);
}

/// An adapter whose `git` is a wrapper that logs each start to the returned file, then runs git.
#[cfg(test)]
fn counting(scratch: &Scratch) -> (GitRepoProbe<InlinePool>, PathBuf) {
    counting_with(scratch, "")
}

/// [`counting`], with shell lines run after the log line and before git.
#[cfg(test)]
fn counting_with(scratch: &Scratch, prelude: &str) -> (GitRepoProbe<InlinePool>, PathBuf) {
    let (wrapper, log) = (scratch.path().join("git.sh"), scratch.path().join("spawns"));
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho x >> '{}'\n{prelude}\nexec git \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let probe = GitRepoProbe::new(
        InlinePool,
        GitConfig {
            program: wrapper.into_os_string(),
            ceiling: Some(scratch.path().to_path_buf()),
            ..GitConfig::default()
        },
    );
    (probe, log)
}

#[cfg(test)]
fn spawns(log: &Path) -> usize {
    fs::read_to_string(log)
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

#[test]
fn should_spawn_no_confirmation_when_the_user_runs_git_after_a_resolve() {
    // given
    let scratch = Scratch::new("nospawn");
    let main = Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    let (probe, log) = counting(&scratch);
    probe
        .resolve_blocking(&scratch.uri("main/src/A.java"))
        .unwrap()
        .unwrap();
    let after_first = spawns(&log);

    // when
    main.write("src/A.java", b"class A { int x; }\n").unwrap();
    main.stage(".").unwrap();
    probe
        .resolve_blocking(&scratch.uri("main/src/A.java"))
        .unwrap()
        .unwrap();
    main.git(&["status", "--porcelain"]).unwrap();
    main.commit("user").unwrap();
    let again = probe
        .resolve_blocking(&scratch.uri("main/src/A.java"))
        .unwrap()
        .unwrap();

    // then
    assert_eq!(after_first, 1);
    assert_eq!(
        spawns(&log),
        1,
        "a user's git add, status or commit made resolve spawn again"
    );
    assert_eq!(again.root, scratch.uri("main"));
}

#[test]
fn should_refuse_without_asking_git_when_a_git_directory_has_no_head() {
    // given
    let scratch = Scratch::new("negative");
    let dir = scratch.path().join("broken");
    fs::create_dir_all(dir.join(".git")).unwrap();
    let (probe, log) = counting(&scratch);

    // when
    let outcomes: Vec<_> = (0..3)
        .map(|_| probe.resolve_blocking(&scratch.uri("broken/A.java")))
        .collect();

    // then
    assert!(
        outcomes
            .iter()
            .all(|o| *o == Err(RepoProbeError::NotAWorktree)),
        "{outcomes:?}"
    );
    assert_eq!(spawns(&log), 0);
}

#[test]
fn should_refuse_the_directory_when_its_git_entry_is_a_dangling_link_or_a_fifo() {
    // given
    let scratch = Scratch::new("oddgit");
    Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    for name in ["dangling", "fifo"] {
        fs::create_dir_all(scratch.path().join("main").join(name)).unwrap();
    }
    symlink(
        "/nonexistent/gitdir",
        scratch.path().join("main/dangling/.git"),
    )
    .unwrap();
    jfl_testkit::fixtures::fifo(&scratch.path().join("main/fifo/.git")).unwrap();
    let (probe, log) = counting(&scratch);

    // when
    let dangling = probe.resolve_blocking(&scratch.uri("main/dangling/A.java"));
    let fifo = probe.resolve_blocking(&scratch.uri("main/fifo/A.java"));

    // then
    assert_eq!(dangling, Err(RepoProbeError::NotAWorktree));
    assert_eq!(fifo, Err(RepoProbeError::NotAWorktree));
    assert_eq!(
        spawns(&log),
        0,
        "a dangling or FIFO .git is refused without asking git"
    );
}

#[test]
fn should_refuse_a_repository_when_its_refs_are_in_the_reftable_format() {
    // given
    let scratch = Scratch::new("reftable");
    Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    fs::create_dir_all(scratch.path().join("main/.git/reftable")).unwrap();

    // when
    let outcome = scratch
        .probe()
        .resolve_blocking(&scratch.uri("main/src/A.java"));

    // then
    assert_eq!(outcome, Err(RepoProbeError::ReftableUnsupported));
}

#[test]
fn should_resolve_again_when_a_confirmation_failed_once() {
    // given
    let scratch = Scratch::new("transient");
    Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    let once = scratch.path().join("killed-once");
    let (probe, log) = counting_with(
        &scratch,
        &format!(
            "if [ ! -e '{0}' ]; then : > '{0}'; kill -KILL $$; fi",
            once.display()
        ),
    );

    // when
    let first = probe.resolve_blocking(&scratch.uri("main/src/A.java"));
    let second = probe.resolve_blocking(&scratch.uri("main/src/A.java"));

    // then
    assert_eq!(
        first,
        Err(RepoProbeError::GitFailed {
            step: jfl_application::GitStep::Confirm,
            code: None
        })
    );
    assert_eq!(second.unwrap().unwrap().root, scratch.uri("main"));
    assert_eq!(spawns(&log), 2);
}

#[test]
fn should_refuse_without_asking_git_when_head_is_a_fifo() {
    // given
    let scratch = Scratch::new("fifohead");
    Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    let head = scratch.path().join("main/.git/HEAD");
    fs::remove_file(&head).unwrap();
    jfl_testkit::fixtures::fifo(&head).unwrap();
    let (probe, log) = counting(&scratch);

    // when
    let outcome = probe.resolve_blocking(&scratch.uri("main/src/A.java"));

    // then
    assert_eq!(outcome, Err(RepoProbeError::NotAWorktree));
    assert_eq!(spawns(&log), 0);
}

#[test]
fn should_refuse_a_repository_when_its_reftable_directory_is_a_link() {
    // given
    let scratch = Scratch::new("reftable-link");
    Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    fs::create_dir_all(scratch.path().join("stack")).unwrap();
    symlink(
        scratch.path().join("stack"),
        scratch.path().join("main/.git/reftable"),
    )
    .unwrap();

    // when
    let outcome = scratch
        .probe()
        .resolve_blocking(&scratch.uri("main/src/A.java"));

    // then
    assert_eq!(outcome, Err(RepoProbeError::ReftableUnsupported));
}

#[test]
fn should_confirm_again_when_the_config_behind_a_linked_git_directory_changes() {
    // given
    let scratch = Scratch::new("gitlink");
    let main = Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    fs::rename(
        scratch.path().join("main/.git"),
        scratch.path().join("real.git"),
    )
    .unwrap();
    symlink(
        scratch.path().join("real.git"),
        scratch.path().join("main/.git"),
    )
    .unwrap();
    let (probe, log) = counting(&scratch);
    probe
        .resolve_blocking(&scratch.uri("main/src/A.java"))
        .unwrap()
        .unwrap();
    main.git(&["add", "--all"]).unwrap();
    probe
        .resolve_blocking(&scratch.uri("main/src/A.java"))
        .unwrap()
        .unwrap();
    let before = spawns(&log);

    // when
    main.git(&["config", "jfl.test", "changed"]).unwrap();
    probe
        .resolve_blocking(&scratch.uri("main/src/A.java"))
        .unwrap()
        .unwrap();

    // then
    assert_eq!(before, 1, "a git add behind the link made resolve spawn");
    assert_eq!(
        spawns(&log),
        2,
        "a config change behind the link was not re-confirmed"
    );
}
