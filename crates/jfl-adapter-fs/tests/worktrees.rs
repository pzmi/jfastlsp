//! The nested-worktree fixture (plan §4.2, ADR 0017): a main checkout with three linked worktrees
//! under `.claude/worktrees/`, made by real `git worktree add`, lists each file once, under its
//! own root.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_application::source_tree::{Exclusion, ListMode, Listing, SourceTree};
use jfl_testkit::exec::{InlinePool, block_on};
use jfl_testkit::fixtures::git::{add_worktree, init_and_commit};

#[cfg(test)]
mod support;
use support::{Scratch, uri, write};

const AGENTS: [&str; 3] = ["agent-a", "agent-b", "agent-c"];

fn list(root: &Path) -> Result<Listing, String> {
    let tree = FsSourceTree::new(InlinePool, FsConfig::default());
    block_on(tree.list(&uri(root), ListMode::Paths)).map_err(|e| e.to_string())
}

/// A main checkout with `src/Main.java` and `lib/Lib.java` committed (plus `extra`, written into
/// the main tree before the commit), and one worktree per agent, each with its own extra file.
fn checkout(scratch: &Scratch, extra: &[(&str, &[u8])]) -> Result<PathBuf, String> {
    let main = scratch.path().join("main");
    write(&main, "src/Main.java", b"class Main {}")?;
    write(&main, "lib/Lib.java", b"class Lib {}")?;
    for (path, bytes) in extra {
        write(&main, path, bytes)?;
    }
    init_and_commit(&main)?;
    for agent in AGENTS {
        add_worktree(&main, &format!(".claude/worktrees/{agent}"), agent)?;
        let only = format!(
            ".claude/worktrees/{agent}/src/Only_{}.java",
            agent.replace('-', "_")
        );
        write(&main, &only, b"class Only {}")?;
    }
    Ok(main)
}

#[test]
fn should_list_each_file_once_under_its_own_root_when_agent_worktrees_are_nested() {
    // given
    let scratch = Scratch::new("worktrees");
    let main = checkout(&scratch, &[]).unwrap();
    let roots: Vec<_> = std::iter::once(main.clone())
        .chain(
            AGENTS
                .iter()
                .map(|a| main.join(".claude/worktrees").join(a)),
        )
        .collect();

    // when
    let listings: Vec<Listing> = roots.iter().map(|root| list(root).unwrap()).collect();

    // then
    let mut absolute = Vec::new();
    for (root, listing) in roots.iter().zip(&listings) {
        assert!(listing.is_complete(), "{:?}", listing.unlisted);
        absolute.extend(listing.files.iter().map(|f| root.join(f.path.as_str())));
    }
    let distinct: BTreeSet<_> = absolute.iter().collect();
    assert_eq!(
        distinct.len(),
        absolute.len(),
        "a file listed twice: {absolute:?}"
    );
    let main_paths: Vec<&str> = listings[0].files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(main_paths, ["lib/Lib.java", "src/Main.java"]);
    assert_eq!(
        listings[0]
            .skipped
            .iter()
            .map(|s| (s.path.as_str(), s.why))
            .collect::<Vec<_>>(),
        [(".claude/worktrees", Exclusion::AgentWorktrees)]
    );
    for (agent, listing) in AGENTS.iter().zip(&listings[1..]) {
        let paths: Vec<&str> = listing.files.iter().map(|f| f.path.as_str()).collect();
        let only = format!("src/Only_{}.java", agent.replace('-', "_"));
        assert_eq!(paths, ["lib/Lib.java", "src/Main.java", only.as_str()]);
    }
    assert_eq!(absolute.len(), 2 + 3 * 3);
}

#[test]
fn should_list_a_worktree_whole_when_the_main_checkout_ignores_claude() {
    // given: the main checkout's `.gitignore` names `.claude/`; a walk of an agent's worktree must
    // not read ignore files above its root.
    let scratch = Scratch::new("worktrees-ignored");
    let main = checkout(&scratch, &[(".gitignore", b".claude/\n")]).unwrap();

    // when
    let agent = list(&main.join(".claude/worktrees/agent-a")).unwrap();
    let parent = list(&main).unwrap();

    // then
    let paths: Vec<&str> = agent.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        ["lib/Lib.java", "src/Main.java", "src/Only_agent_a.java"]
    );
    let parent_paths: Vec<&str> = parent.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(parent_paths, ["lib/Lib.java", "src/Main.java"]);
}

#[test]
fn should_skip_a_linked_worktree_when_it_sits_outside_claude_worktrees() {
    // given
    let scratch = Scratch::new("worktrees-elsewhere");
    let main = scratch.path().join("main");
    write(&main, "src/Main.java", b"class Main {}").unwrap();
    init_and_commit(&main).unwrap();
    add_worktree(&main, "nested/wt", "wt").unwrap();

    // when
    let listing = list(&main).unwrap();

    // then
    let paths: Vec<&str> = listing.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["src/Main.java"]);
    assert_eq!(
        listing
            .skipped
            .iter()
            .map(|s| (s.path.as_str(), s.why))
            .collect::<Vec<_>>(),
        [("nested/wt", Exclusion::NestedWorktree)]
    );
}

#[test]
fn should_skip_a_directory_when_its_git_entry_is_a_dangling_link_or_a_fifo() {
    // given
    let scratch = Scratch::new("oddgit");
    let main = checkout(&scratch, &[]).unwrap();
    write(&main, "dangling/D.java", b"class D {}").unwrap();
    write(&main, "fifo/F.java", b"class F {}").unwrap();
    std::os::unix::fs::symlink("/nonexistent/gitdir", main.join("dangling/.git")).unwrap();
    jfl_testkit::fixtures::fifo(&main.join("fifo/.git")).unwrap();

    // when
    let listing = list(&main).unwrap();

    // then
    let listed: Vec<&str> = listing.files.iter().map(|f| f.path.as_str()).collect();
    assert!(
        !listed
            .iter()
            .any(|p| p.starts_with("dangling/") || p.starts_with("fifo/")),
        "{listed:?}"
    );
    let skipped: Vec<(&str, Exclusion)> = listing
        .skipped
        .iter()
        .map(|s| (s.path.as_str(), s.why))
        .collect();
    assert!(
        skipped.contains(&("dangling", Exclusion::NestedWorktree)),
        "{skipped:?}"
    );
    assert!(
        skipped.contains(&("fifo", Exclusion::NestedWorktree)),
        "{skipped:?}"
    );
}
