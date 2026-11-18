//! The stat set of plan §4.3 on real repositories: exactly which files, for a main and a linked
//! worktree, and which of them each Git operation moves.

use std::fs;

use jfl_application::{GitFile, GitSnapshot, HeadState, Worktree, WorktreeKind};
use jfl_testkit::fixtures::git::Repo;

#[cfg(test)]
mod support;
use support::Scratch;

#[cfg(test)]
fn files(snapshot: &GitSnapshot) -> Vec<(GitFile, String)> {
    snapshot
        .files
        .iter()
        .map(|f| (f.file, f.path.path().to_owned()))
        .collect()
}

#[cfg(test)]
/// Which files differ between two snapshots of the same set.
fn moved(before: &GitSnapshot, after: &GitSnapshot) -> Vec<GitFile> {
    assert_eq!(files(before), files(after), "the set itself changed");
    before
        .files
        .iter()
        .zip(&after.files)
        .filter(|(a, b)| a.stat != b.stat)
        .map(|(a, _)| a.file)
        .collect()
}

#[cfg(test)]
fn setup(scratch: &Scratch) -> (Repo, Worktree, Worktree) {
    let main = Repo::init(
        &scratch.path().join("main"),
        &[("src/A.java", b"class A {}\n")],
    )
    .unwrap();
    main.add_agent_worktree("agent").unwrap();
    let probe = scratch.probe();
    let main_tree = probe
        .resolve_blocking(&scratch.uri("main"))
        .unwrap()
        .unwrap();
    let agent_tree = probe
        .resolve_blocking(&scratch.uri("main/.claude/worktrees/agent/src/A.java"))
        .unwrap()
        .unwrap();
    (main, main_tree, agent_tree)
}

#[test]
fn should_stat_head_branch_packed_refs_and_index_when_a_worktree_is_on_a_branch() {
    // given
    let scratch = Scratch::new("set");
    let (_main, main, agent) = setup(&scratch);
    let base = scratch.path().join("main/.git");
    let base = base.to_str().unwrap();

    // when
    let main_set = scratch.probe().snapshot_blocking(&main).unwrap();
    let agent_set = scratch.probe().snapshot_blocking(&agent).unwrap();

    // then
    assert_eq!(main.kind, WorktreeKind::Main);
    assert_eq!(
        main_set.head,
        HeadState::Symbolic("refs/heads/main".to_owned())
    );
    assert_eq!(
        files(&main_set),
        [
            (GitFile::Head, format!("{base}/HEAD")),
            (GitFile::Ref, format!("{base}/refs/heads/main")),
            (GitFile::PackedRefs, format!("{base}/packed-refs")),
            (GitFile::Index, format!("{base}/index")),
        ]
    );
    assert_eq!(agent.kind, WorktreeKind::Linked);
    assert_eq!(agent.git_dir.path(), format!("{base}/worktrees/agent"));
    assert_eq!(
        files(&agent_set),
        [
            (GitFile::Head, format!("{base}/worktrees/agent/HEAD")),
            (GitFile::Ref, format!("{base}/refs/heads/agent")),
            (GitFile::PackedRefs, format!("{base}/packed-refs")),
            (GitFile::Index, format!("{base}/worktrees/agent/index")),
        ]
    );
}

#[test]
fn should_move_only_the_branch_ref_when_a_soft_reset_or_a_commit_leaves_the_index_alone() {
    // given
    let scratch = Scratch::new("soft");
    let (main_repo, main, _) = setup(&scratch);
    let probe = scratch.probe();
    main_repo
        .write("src/A.java", b"class A { int x; }\n")
        .unwrap();
    main_repo.commit_all("second").unwrap();
    let before = probe.snapshot_blocking(&main).unwrap();

    // when
    main_repo
        .git(&["reset", "--quiet", "--soft", "HEAD~1"])
        .unwrap();
    let after_reset = probe.snapshot_blocking(&main).unwrap();
    main_repo
        .git(&["commit", "--quiet", "--allow-empty", "--message", "empty"])
        .unwrap();
    let after_commit = probe.snapshot_blocking(&main).unwrap();

    // then
    assert_eq!(moved(&before, &after_reset), [GitFile::Ref]);
    assert!(moved(&after_reset, &after_commit).contains(&GitFile::Ref));
}

#[test]
fn should_see_the_branch_move_into_packed_refs_when_refs_are_packed() {
    // given
    let scratch = Scratch::new("packed");
    let (main_repo, main, agent) = setup(&scratch);
    let probe = scratch.probe();
    let before = probe.snapshot_blocking(&agent).unwrap();

    // when
    main_repo.git(&["pack-refs", "--all"]).unwrap();
    let packed = probe.snapshot_blocking(&agent).unwrap();
    let main_packed = probe.snapshot_blocking(&main).unwrap();

    // then
    let moved = moved(&before, &packed);
    assert!(
        moved.contains(&GitFile::Ref) && moved.contains(&GitFile::PackedRefs),
        "{moved:?}"
    );
    let stat = |s: &GitSnapshot, file| s.files.iter().find(|f| f.file == file).unwrap().stat;
    assert_eq!(stat(&packed, GitFile::Ref), None);
    assert!(stat(&packed, GitFile::PackedRefs).is_some());
    assert_eq!(stat(&main_packed, GitFile::Ref), None);
}

#[test]
fn should_stat_head_and_index_only_when_head_is_detached() {
    // given
    let scratch = Scratch::new("detached");
    let (main_repo, main, _) = setup(&scratch);
    main_repo.git(&["checkout", "--quiet", "--detach"]).unwrap();

    // when
    let snapshot = scratch.probe().snapshot_blocking(&main).unwrap();

    // then
    assert!(matches!(snapshot.head, HeadState::Detached(_)));
    let kinds: Vec<GitFile> = snapshot.files.iter().map(|f| f.file).collect();
    assert_eq!(kinds, [GitFile::Head, GitFile::Index]);
}

#[test]
fn should_record_absent_files_when_the_branch_is_unborn() {
    // given
    let scratch = Scratch::new("unborn");
    let root = scratch.path().join("fresh");
    fs::create_dir_all(&root).unwrap();
    Repo::open(&root).git(&["init", "--quiet"]).unwrap();
    let probe = scratch.probe();
    let worktree = probe
        .resolve_blocking(&scratch.uri("fresh"))
        .unwrap()
        .unwrap();

    // when
    let empty = probe.snapshot_blocking(&worktree).unwrap();
    let head_tree = probe.head_tree_blocking(&worktree).unwrap();
    Repo::open(&root).write("A.java", b"class A {}\n").unwrap();
    Repo::open(&root).commit_all("first").unwrap();
    let born = probe.snapshot_blocking(&worktree).unwrap();

    // then
    assert_eq!(head_tree, None);
    assert!(
        empty
            .files
            .iter()
            .filter(|f| f.file != GitFile::Head)
            .all(|f| f.stat.is_none())
    );
    let moved = moved(&empty, &born);
    assert!(
        moved.contains(&GitFile::Ref) && moved.contains(&GitFile::Index),
        "{moved:?}"
    );
}

#[test]
fn should_answer_without_blocking_when_head_is_a_fifo() {
    // given
    let scratch = Scratch::new("fifo");
    let (_main, main, _) = setup(&scratch);
    let head = scratch.path().join("main/.git/HEAD");
    fs::remove_file(&head).unwrap();
    jfl_testkit::fixtures::fifo(&head).unwrap();

    // when
    let snapshot = scratch.probe().snapshot_blocking(&main).unwrap();

    // then
    assert_eq!(snapshot.head, HeadState::Unreadable);
    assert_eq!(snapshot.files.len(), 2);
}

#[test]
fn should_stay_inside_the_git_dir_when_head_names_a_ref_that_climbs_out() {
    // given
    let scratch = Scratch::new("climb");
    let (_main, main, _) = setup(&scratch);
    fs::write(
        scratch.path().join("main/.git/HEAD"),
        b"ref: refs/../../../outside\n",
    )
    .unwrap();

    // when
    let snapshot = scratch.probe().snapshot_blocking(&main).unwrap();

    // then
    assert_eq!(snapshot.head, HeadState::Unreadable);
    assert!(
        snapshot
            .files
            .iter()
            .all(|f| f.path.path().starts_with(main.git_dir.path()))
    );
}
