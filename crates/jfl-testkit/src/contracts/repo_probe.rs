//! The `repo_probe` contract (ver §5, plan §6.4): the innermost root of a path, a nested linked
//! worktree's and a nested repository's included (plan §4.1, §4.2); the common directory (§4.6);
//! a stat set that moves on every Git operation that moves `HEAD`, the branch it names or the
//! index, that stays put when nothing does and that the probe's own calls never move (§4.3); the
//! files changed since a tree after a commit, checkout, stash and reset, soft and hard; and the
//! tracked `.java` files. Checked the same way against
//! [`crate::fakes::repo_probe::FakeRepoProbe`] (`contracts::repo_probe::tests` here) and against
//! `jfl-adapter-git` in `crates/jfl-adapter-git/tests/contract.rs`.
//!
//! Each case asks the [`Factory`] for a fresh, empty base directory ([`Fixture`]) and builds what
//! it needs through the fixture's own vocabulary: paths are `/`-separated and relative to the
//! base, a worktree is named by its root's path, and every Git operation is the one `git` command
//! its name says. The expectations are literal paths.
//!
//! The suite's own tests run it against deliberately broken implementations and assert each fails
//! the cases it should (`repo_probe::tests`).

use jfl_application::{
    ChangedFiles, GitSnapshot, ObjectId, RepoProbe, RepoProbeError, Worktree, WorktreeKind,
};
use jfl_domain::{FileUri, RelPath};

use crate::contracts::{NonUtf8Name, non_utf8_names};
use crate::exec::block_on;

/// A fresh base directory under test, with the means to change it.
pub trait Fixture {
    /// The implementation under test.
    type Probe: RepoProbe;

    /// The implementation, answering for paths under the base.
    fn probe(&self) -> &Self::Probe;

    /// `base/path`.
    fn uri(&self, path: &str) -> FileUri;

    /// `git init` in `dir` (made if missing) and one commit of `files` on `main`.
    fn init(&self, dir: &str, files: &[(&str, &[u8])]) -> Result<(), String>;

    /// `git worktree add -b branch path` in the repository whose worktree is at `main`.
    fn add_worktree(&self, main: &str, path: &str, branch: &str) -> Result<(), String>;

    /// Writes a file, making its parents.
    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String>;

    /// Writes a `.java` file whose name is not UTF-8 in the directory `dir`, or reports that the
    /// platform's filesystem refused the name ([`NonUtf8Name`]).
    fn write_non_utf8_java(&self, dir: &str) -> Result<NonUtf8Name, String>;

    /// Removes a file.
    fn remove(&self, path: &str) -> Result<(), String>;

    /// `git add -f -A -- path` in `worktree` (`path` relative to it; `.` for everything).
    fn stage(&self, worktree: &str, path: &str) -> Result<(), String>;

    /// `git commit` in `worktree`.
    fn commit(&self, worktree: &str) -> Result<(), String>;

    /// `git branch name` in `worktree`.
    fn branch(&self, worktree: &str, name: &str) -> Result<(), String>;

    /// `git checkout name` in `worktree`.
    fn checkout(&self, worktree: &str, name: &str) -> Result<(), String>;

    /// `git stash` in `worktree`.
    fn stash(&self, worktree: &str) -> Result<(), String>;

    /// `git reset --soft HEAD~1` in `worktree`, or `--hard` with `hard`.
    fn reset(&self, worktree: &str, hard: bool) -> Result<(), String>;
}

/// Makes fresh fixtures.
pub trait Factory {
    /// The fixture it makes.
    type Fixture: Fixture;

    /// A fresh, empty base directory.
    fn fixture(&self) -> Result<Self::Fixture, String>;
}

/// A case the implementation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The case's name.
    pub case: &'static str,
    /// What was wrong.
    pub detail: String,
}

type Case<F> = fn(&F) -> Result<(), String>;

/// Runs every case; an empty list is a pass.
pub fn check<F: Factory>(factory: &F) -> Vec<Violation> {
    let cases: [(&'static str, Case<F::Fixture>); 21] = [
        ("resolves_the_main_worktree", resolves_the_main_worktree),
        (
            "resolves_a_nested_linked_worktree_to_itself",
            resolves_a_nested_linked_worktree_to_itself,
        ),
        (
            "resolves_a_nested_repository_to_itself",
            resolves_a_nested_repository_to_itself,
        ),
        (
            "resolves_nothing_outside_a_repository",
            resolves_nothing_outside_a_repository,
        ),
        ("snapshot_is_stable", snapshot_is_stable),
        ("snapshot_sees_staging", snapshot_sees_staging),
        ("snapshot_sees_a_commit", snapshot_sees_a_commit),
        ("snapshot_sees_a_checkout", snapshot_sees_a_checkout),
        ("snapshot_sees_a_soft_reset", snapshot_sees_a_soft_reset),
        ("snapshot_sees_a_hard_reset", snapshot_sees_a_hard_reset),
        ("snapshot_sees_a_stash", snapshot_sees_a_stash),
        (
            "snapshot_of_each_worktree_is_its_own",
            snapshot_of_each_worktree_is_its_own,
        ),
        ("changed_after_a_commit", changed_after_a_commit),
        ("changed_after_a_checkout", changed_after_a_checkout),
        ("changed_after_a_soft_reset", changed_after_a_soft_reset),
        ("changed_after_a_hard_reset", changed_after_a_hard_reset),
        ("changed_after_a_stash", changed_after_a_stash),
        (
            "changed_reports_working_states",
            changed_reports_working_states,
        ),
        (
            "changed_refuses_an_unknown_base",
            changed_refuses_an_unknown_base,
        ),
        (
            "tracked_lists_tracked_java_only",
            tracked_lists_tracked_java_only,
        ),
        (
            "tracked_lists_other_jvm_sources",
            tracked_lists_other_jvm_sources,
        ),
    ];
    cases
        .into_iter()
        .filter_map(|(case, run)| {
            let outcome = factory.fixture().and_then(|fixture| run(&fixture));
            outcome.err().map(|detail| Violation { case, detail })
        })
        .collect()
}

const A: &[u8] = b"class A {}\n";
const B: &[u8] = b"class B {}\n";

fn ensure(condition: bool, what: impl FnOnce() -> String) -> Result<(), String> {
    if condition { Ok(()) } else { Err(what()) }
}

fn rel(paths: &[&str]) -> Vec<RelPath> {
    paths.iter().filter_map(|p| RelPath::new(p).ok()).collect()
}

fn names(paths: &[RelPath]) -> Vec<&str> {
    paths.iter().map(RelPath::as_str).collect()
}

fn resolve<X: Fixture>(fx: &X, path: &str) -> Result<Option<Worktree>, String> {
    block_on(fx.probe().resolve(&fx.uri(path))).map_err(|e| format!("resolve {path}: {e}"))
}

fn worktree<X: Fixture>(fx: &X, root: &str) -> Result<Worktree, String> {
    resolve(fx, root)?.ok_or_else(|| format!("{root} resolved to no worktree"))
}

fn snap<X: Fixture>(fx: &X, worktree: &Worktree) -> Result<GitSnapshot, String> {
    block_on(fx.probe().snapshot(worktree)).map_err(|e| format!("snapshot: {e}"))
}

fn head<X: Fixture>(fx: &X, worktree: &Worktree) -> Result<ObjectId, String> {
    block_on(fx.probe().head_tree(worktree))
        .map_err(|e| format!("head_tree: {e}"))?
        .ok_or_else(|| String::from("head_tree: unborn"))
}

fn changed<X: Fixture>(
    fx: &X,
    worktree: &Worktree,
    base: &ObjectId,
) -> Result<ChangedFiles, String> {
    block_on(fx.probe().changed_since(worktree, Some(base)))
        .map_err(|e| format!("changed_since: {e}"))
}

/// A main checkout `main` with `src/A.java` and `src/B.java`.
fn main_repo<X: Fixture>(fx: &X) -> Result<Worktree, String> {
    fx.init("main", &[("src/A.java", A), ("src/B.java", B)])?;
    worktree(fx, "main")
}

/// Whether `operation` moves `main`'s snapshot.
fn moves<X: Fixture>(
    fx: &X,
    main: &Worktree,
    operation: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let before = snap(fx, main)?;
    operation()?;
    let after = snap(fx, main)?;
    ensure(before != after, || {
        format!("the snapshot did not change: {after:?}")
    })
}

/// The committed, working and untracked parts, and the count of unrepresentable paths.
fn parts(changed: &ChangedFiles) -> (Vec<&str>, Vec<&str>, Vec<&str>, u64) {
    (
        names(&changed.committed),
        names(&changed.working),
        names(&changed.untracked),
        changed.unrepresentable,
    )
}

fn resolves_the_main_worktree<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    ensure(main.root == fx.uri("main"), || {
        format!("root {}", main.root)
    })?;
    ensure(main.kind == WorktreeKind::Main, || format!("{main:?}"))?;
    ensure(main.git_dir == main.common_dir, || format!("{main:?}"))?;
    for path in ["main/src/A.java", "main/src", "main/src/pkg/New.java"] {
        let found = resolve(fx, path)?;
        ensure(found.as_ref() == Some(&main), || {
            format!("{path}: {found:?}")
        })?;
    }
    Ok(())
}

fn resolves_a_nested_linked_worktree_to_itself<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    fx.add_worktree("main", "main/.claude/worktrees/agent", "agent")?;
    let agent = worktree(fx, "main/.claude/worktrees/agent/src/A.java")?;
    ensure(agent.root == fx.uri("main/.claude/worktrees/agent"), || {
        format!("a path in the nested worktree resolved to {}", agent.root)
    })?;
    ensure(agent.kind == WorktreeKind::Linked, || format!("{agent:?}"))?;
    ensure(agent.common_dir == main.common_dir, || {
        format!("common directories differ: {agent:?} {main:?}")
    })?;
    ensure(agent.repo() == main.repo(), || {
        String::from("repo ids differ")
    })?;
    ensure(agent.git_dir != main.git_dir, || {
        String::from("the linked worktree shares the main one's GIT_DIR")
    })?;
    let beside = resolve(fx, "main/.claude/Notes.java")?;
    ensure(beside.as_ref() == Some(&main), || format!("{beside:?}"))
}

fn resolves_a_nested_repository_to_itself<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    fx.init("main/vendored/lib", &[("L.java", b"class L {}\n")])?;
    let lib = worktree(fx, "main/vendored/lib/L.java")?;
    ensure(lib.root == fx.uri("main/vendored/lib"), || {
        format!("a path in the nested repository resolved to {}", lib.root)
    })?;
    ensure(lib.kind == WorktreeKind::Main, || format!("{lib:?}"))?;
    ensure(lib.repo() != main.repo(), || {
        String::from("the nested repository shares an id")
    })
}

fn resolves_nothing_outside_a_repository<X: Fixture>(fx: &X) -> Result<(), String> {
    main_repo(fx)?;
    fx.write("plain/X.java", A)?;
    let found = resolve(fx, "plain/X.java")?;
    ensure(found.is_none(), || format!("{found:?}"))
}

fn snapshot_is_stable<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    fx.add_worktree("main", "main/.claude/worktrees/agent", "agent")?;
    let before = snap(fx, &main)?;
    fx.write("main/src/A.java", b"class A { int edited; }\n")?;
    fx.write("main/src/U.java", b"class U {}\n")?;
    let base = head(fx, &main)?;
    changed(fx, &main, &base)?;
    block_on(fx.probe().tracked_java(&main)).map_err(|e| e.to_string())?;
    resolve(fx, "main/src/A.java")?;
    let again = snap(fx, &main)?;
    let after = snap(fx, &main)?;
    ensure(before == again && again == after, || {
        format!("the snapshot moved with no Git operation: {before:?} then {after:?}")
    })
}

fn snapshot_sees_staging<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    fx.write("main/src/A.java", b"class A { int staged; }\n")?;
    moves(fx, &main, || fx.stage("main", "src/A.java"))
}

fn snapshot_sees_a_commit<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    fx.write("main/src/A.java", b"class A { int committed; }\n")?;
    fx.stage("main", "src/A.java")?;
    moves(fx, &main, || fx.commit("main"))
}

fn snapshot_sees_a_checkout<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    fx.branch("main", "other")?;
    moves(fx, &main, || fx.checkout("main", "other"))
}

/// Two commits on `main`, the second changing `src/A.java`; returns the first one's tree.
fn two_commits<X: Fixture>(fx: &X, main: &Worktree) -> Result<ObjectId, String> {
    let first = head(fx, main)?;
    fx.write("main/src/A.java", b"class A { int second; }\n")?;
    fx.stage("main", "src/A.java")?;
    fx.commit("main")?;
    Ok(first)
}

fn snapshot_sees_a_soft_reset<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    two_commits(fx, &main)?;
    moves(fx, &main, || fx.reset("main", false))
}

fn snapshot_sees_a_hard_reset<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    two_commits(fx, &main)?;
    moves(fx, &main, || fx.reset("main", true))
}

fn snapshot_sees_a_stash<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    fx.write("main/src/A.java", b"class A { int stashed; }\n")?;
    fx.stage("main", "src/A.java")?;
    moves(fx, &main, || fx.stash("main"))
}

fn snapshot_of_each_worktree_is_its_own<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    fx.add_worktree("main", "main/.claude/worktrees/agent", "agent")?;
    let agent = worktree(fx, "main/.claude/worktrees/agent")?;
    let main_before = snap(fx, &main)?;
    let agent_before = snap(fx, &agent)?;
    fx.write(
        "main/.claude/worktrees/agent/src/A.java",
        b"class A { int agent; }\n",
    )?;
    fx.stage("main/.claude/worktrees/agent", "src/A.java")?;
    fx.commit("main/.claude/worktrees/agent")?;
    let main_after = snap(fx, &main)?;
    let agent_after = snap(fx, &agent)?;
    ensure(main_before == main_after, || {
        String::from("a commit in the linked worktree moved the main one's snapshot")
    })?;
    ensure(agent_before != agent_after, || {
        String::from("a commit in the linked worktree did not move its own snapshot")
    })
}

fn changed_after_a_commit<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    let base = two_commits(fx, &main)?;
    let changed = changed(fx, &main, &base)?;
    ensure(changed.head_tree.as_ref() != Some(&base), || {
        String::from("HEAD's tree did not move with the commit")
    })?;
    ensure(
        parts(&changed) == (vec!["src/A.java"], vec![], vec![], 0) && !changed.everything,
        || format!("{changed:?}"),
    )
}

fn changed_after_a_checkout<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    fx.branch("main", "other")?;
    fx.checkout("main", "other")?;
    fx.write("main/src/C.java", b"class C {}\n")?;
    fx.stage("main", "src/C.java")?;
    fx.commit("main")?;
    let other = head(fx, &main)?;
    fx.checkout("main", "main")?;
    let changed = changed(fx, &main, &other)?;
    ensure(
        parts(&changed) == (vec!["src/C.java"], vec![], vec![], 0),
        || format!("{changed:?}"),
    )
}

fn changed_after_a_soft_reset<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    two_commits(fx, &main)?;
    let second = head(fx, &main)?;
    fx.reset("main", false)?;
    let changed = changed(fx, &main, &second)?;
    ensure(
        parts(&changed) == (vec!["src/A.java"], vec!["src/A.java"], vec![], 0),
        || format!("a soft reset keeps the change staged: {changed:?}"),
    )
}

fn changed_after_a_hard_reset<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    two_commits(fx, &main)?;
    let second = head(fx, &main)?;
    fx.reset("main", true)?;
    let changed = changed(fx, &main, &second)?;
    ensure(
        parts(&changed) == (vec!["src/A.java"], vec![], vec![], 0),
        || format!("{changed:?}"),
    )
}

fn changed_after_a_stash<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    let base = head(fx, &main)?;
    fx.write("main/src/A.java", b"class A { int stashed; }\n")?;
    fx.write("main/src/U.java", b"class U {}\n")?;
    let before = changed(fx, &main, &base)?;
    fx.stash("main")?;
    let after = changed(fx, &main, &base)?;
    ensure(
        parts(&before) == (vec![], vec!["src/A.java"], vec!["src/U.java"], 0),
        || format!("before the stash: {before:?}"),
    )?;
    ensure(
        parts(&after) == (vec![], vec![], vec!["src/U.java"], 0),
        || format!("after the stash: {after:?}"),
    )
}

fn changed_reports_working_states<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    fx.add_worktree("main", "main/.claude/worktrees/agent", "agent")?;
    let base = head(fx, &main)?;
    fx.write("main/src/A.java", b"class A { int unstaged; }\n")?;
    fx.write("main/src/N.java", b"class N {}\n")?;
    fx.stage("main", "src/N.java")?;
    fx.remove("main/src/B.java")?;
    fx.write("main/src/with space/U.java", b"class U {}\n")?;
    let unrepresentable = non_utf8_names(fx.write_non_utf8_java("main/src")?)?;
    let changed = changed(fx, &main, &base)?;
    ensure(
        parts(&changed)
            == (
                vec![],
                vec!["src/A.java", "src/B.java", "src/N.java"],
                vec!["src/with space/U.java"],
                unrepresentable,
            ),
        || format!("{changed:?}"),
    )
}

fn changed_refuses_an_unknown_base<X: Fixture>(fx: &X) -> Result<(), String> {
    let main = main_repo(fx)?;
    let unknown = ObjectId::parse("0123456789abcdef0123456789abcdef01234567")
        .ok_or("the unknown id does not parse")?;
    let outcome = block_on(fx.probe().changed_since(&main, Some(&unknown)));
    ensure(outcome.is_err(), || {
        format!("an unknown base gave a change set: {outcome:?}")
    })?;
    let everything = block_on(fx.probe().changed_since(&main, None)).map_err(|e| e.to_string())?;
    ensure(
        everything.everything && everything.head_tree.is_some(),
        || format!("no base must mean everything: {everything:?}"),
    )
}

fn tracked_lists_tracked_java_only<X: Fixture>(fx: &X) -> Result<(), String> {
    fx.init(
        "main",
        &[
            ("src/A.java", A),
            (".gitignore", b"gen/\n"),
            ("README", b"r\n"),
        ],
    )?;
    let main = worktree(fx, "main")?;
    fx.write("main/gen/G.java", b"class G {}\n")?;
    fx.stage("main", "gen/G.java")?;
    let unrepresentable = non_utf8_names(fx.write_non_utf8_java("main/src")?)?;
    fx.stage("main", "src")?;
    fx.commit("main")?;
    fx.write("main/src/U.java", b"class U {}\n")?;
    let tracked =
        block_on(fx.probe().tracked_java(&main)).map_err(|e: RepoProbeError| e.to_string())?;
    ensure(
        tracked.files == rel(&["gen/G.java", "src/A.java"])
            && tracked.unrepresentable == unrepresentable,
        || format!("{tracked:?}"),
    )
}

/// Tracked other-language sources and build scripts, a `.gitignore`d one and one whose extension
/// is upper case included, listed beside the `.java` files: the listing cannot see the ignored
/// one (IDX-16).
fn tracked_lists_other_jvm_sources<X: Fixture>(fx: &X) -> Result<(), String> {
    fx.init(
        "main",
        &[
            ("src/A.java", A),
            (".gitignore", b"gen/\n"),
            ("build.gradle", b"plugins {}\n"),
            ("web/X.JSP", b"<% %>\n"),
            ("README", b"r\n"),
        ],
    )?;
    let main = worktree(fx, "main")?;
    fx.write("main/gen/L.kt", b"fun f() {}\n")?;
    fx.stage("main", "gen/L.kt")?;
    fx.commit("main")?;
    fx.write("main/src/U.kt", b"fun u() {}\n")?;
    let tracked =
        block_on(fx.probe().tracked_java(&main)).map_err(|e: RepoProbeError| e.to_string())?;
    ensure(
        tracked.files == rel(&["src/A.java"])
            && tracked.other == rel(&["build.gradle", "gen/L.kt", "web/X.JSP"])
            && tracked.other_unrepresentable == 0,
        || format!("{tracked:?}"),
    )
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use jfl_application::{GitFile, TrackedFiles};

    use super::*;
    use crate::fakes::repo_probe::FakeRepoProbe;

    /// How a broken implementation departs from the fake.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Defect {
        None,
        /// Binds a path to the outermost worktree holding it, not the innermost.
        OuterRoot,
        /// Leaves the branch `HEAD` names (and `packed-refs`) out of the stat set.
        NoBranchRef,
        /// Rewrites the index on every query, as a `status` taking optional locks does.
        WritesIndex,
        /// Forgets the untracked files.
        NoUntracked,
        /// Answers an unknown base with an empty change set.
        EmptyForUnknownBase,
        /// Drops the paths that are not UTF-8 without counting them.
        DropsUnrepresentable,
        /// Reports no committed change.
        NoCommitted,
        /// Lists no tracked file of another JVM language.
        NoTrackedOthers,
    }

    trait Kind {
        const DEFECT: Defect;
    }

    struct Probe<K> {
        fake: FakeRepoProbe,
        kind: PhantomData<K>,
    }

    impl<K: Kind + Send + Sync> RepoProbe for Probe<K> {
        async fn resolve(&self, path: &FileUri) -> Result<Option<Worktree>, RepoProbeError> {
            let found = self.fake.resolve(path).await?;
            if K::DEFECT != Defect::OuterRoot {
                return Ok(found);
            }
            let mut outer = found;
            let mut probe = path.clone();
            while let Some(parent) = probe.path().rsplit_once('/').map(|(p, _)| p.to_owned()) {
                if parent.is_empty() {
                    break;
                }
                let Ok(up) = FileUri::from_path(&parent) else {
                    break;
                };
                if let Some(worktree) = self.fake.resolve(&up).await? {
                    outer = Some(worktree);
                }
                probe = up;
            }
            Ok(outer)
        }

        async fn snapshot(&self, worktree: &Worktree) -> Result<GitSnapshot, RepoProbeError> {
            let mut snapshot = self.fake.snapshot(worktree).await?;
            if K::DEFECT == Defect::NoBranchRef {
                snapshot
                    .files
                    .retain(|f| !matches!(f.file, GitFile::Ref | GitFile::PackedRefs));
            }
            Ok(snapshot)
        }

        async fn head_tree(&self, worktree: &Worktree) -> Result<Option<ObjectId>, RepoProbeError> {
            self.fake.head_tree(worktree).await
        }

        async fn changed_since(
            &self,
            worktree: &Worktree,
            base: Option<&ObjectId>,
        ) -> Result<ChangedFiles, RepoProbeError> {
            if K::DEFECT == Defect::WritesIndex {
                self.fake.rewrite_index(&worktree.root);
            }
            let outcome = self.fake.changed_since(worktree, base).await;
            let mut changed = match (outcome, K::DEFECT) {
                (Err(_), Defect::EmptyForUnknownBase) => ChangedFiles::default(),
                (outcome, _) => outcome?,
            };
            match K::DEFECT {
                Defect::NoUntracked => changed.untracked.clear(),
                Defect::DropsUnrepresentable => changed.unrepresentable = 0,
                Defect::NoCommitted => changed.committed.clear(),
                _ => {}
            }
            Ok(changed)
        }

        async fn tracked_java(&self, worktree: &Worktree) -> Result<TrackedFiles, RepoProbeError> {
            if K::DEFECT == Defect::WritesIndex {
                self.fake.rewrite_index(&worktree.root);
            }
            let mut tracked = self.fake.tracked_java(worktree).await?;
            if K::DEFECT == Defect::DropsUnrepresentable {
                tracked.unrepresentable = 0;
            }
            if K::DEFECT == Defect::NoTrackedOthers {
                tracked.other.clear();
            }
            Ok(tracked)
        }
    }

    struct Scripted<K> {
        fake: FakeRepoProbe,
        probe: Probe<K>,
    }

    impl<K> Scripted<K> {
        fn at(&self, path: &str) -> FileUri {
            FileUri::from_path(&format!("/fake/{path}")).unwrap()
        }
    }

    impl<K: Kind + Send + Sync> Fixture for Scripted<K> {
        type Probe = Probe<K>;

        fn probe(&self) -> &Probe<K> {
            &self.probe
        }
        fn uri(&self, path: &str) -> FileUri {
            self.at(path)
        }
        fn init(&self, dir: &str, files: &[(&str, &[u8])]) -> Result<(), String> {
            self.fake.init(&self.at(dir), files)
        }
        fn add_worktree(&self, main: &str, path: &str, branch: &str) -> Result<(), String> {
            self.fake
                .add_worktree(&self.at(main), &self.at(path), branch)
        }
        fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
            self.fake.write(&self.at(path), bytes)
        }
        fn write_non_utf8_java(&self, dir: &str) -> Result<NonUtf8Name, String> {
            self.fake
                .write_non_utf8_java(&self.at(dir))
                .map(|()| NonUtf8Name::Written)
        }
        fn remove(&self, path: &str) -> Result<(), String> {
            self.fake.remove(&self.at(path))
        }
        fn stage(&self, worktree: &str, path: &str) -> Result<(), String> {
            self.fake.stage(&self.at(worktree), path)
        }
        fn commit(&self, worktree: &str) -> Result<(), String> {
            self.fake.commit(&self.at(worktree))
        }
        fn branch(&self, worktree: &str, name: &str) -> Result<(), String> {
            self.fake.branch(&self.at(worktree), name)
        }
        fn checkout(&self, worktree: &str, name: &str) -> Result<(), String> {
            self.fake.checkout(&self.at(worktree), name)
        }
        fn stash(&self, worktree: &str) -> Result<(), String> {
            self.fake.stash(&self.at(worktree))
        }
        fn reset(&self, worktree: &str, hard: bool) -> Result<(), String> {
            self.fake.reset(&self.at(worktree), hard)
        }
    }

    struct Fakes<K>(PhantomData<K>);

    impl<K: Kind + Send + Sync> Factory for Fakes<K> {
        type Fixture = Scripted<K>;

        fn fixture(&self) -> Result<Scripted<K>, String> {
            let fake = FakeRepoProbe::new();
            Ok(Scripted {
                probe: Probe {
                    fake: fake.clone(),
                    kind: PhantomData,
                },
                fake,
            })
        }
    }

    fn failed<K: Kind + Send + Sync>(_: PhantomData<K>) -> Vec<&'static str> {
        check(&Fakes::<K>(PhantomData))
            .into_iter()
            .map(|v| v.case)
            .collect()
    }

    macro_rules! kind {
        ($name:ident, $defect:expr) => {
            struct $name;
            impl Kind for $name {
                const DEFECT: Defect = $defect;
            }
        };
    }

    kind!(Sound, Defect::None);
    kind!(OuterRoot, Defect::OuterRoot);
    kind!(NoBranchRef, Defect::NoBranchRef);
    kind!(WritesIndex, Defect::WritesIndex);
    kind!(NoUntracked, Defect::NoUntracked);
    kind!(EmptyForUnknownBase, Defect::EmptyForUnknownBase);
    kind!(DropsUnrepresentable, Defect::DropsUnrepresentable);
    kind!(NoCommitted, Defect::NoCommitted);
    kind!(NoTrackedOthers, Defect::NoTrackedOthers);

    #[test]
    fn should_fail_the_other_sources_case_when_tracked_other_languages_are_dropped() {
        // given
        let broken = PhantomData::<NoTrackedOthers>;

        // when
        let failed = failed(broken);

        // then
        assert_eq!(failed, ["tracked_lists_other_jvm_sources"]);
    }

    #[test]
    fn should_pass_the_contract_when_run_against_the_fake() {
        // given
        let factory = Fakes::<Sound>(PhantomData);

        // when
        let violations = check(&factory);

        // then
        assert_eq!(violations, []);
    }

    #[test]
    fn should_fail_the_nested_cases_when_a_path_binds_to_the_outer_worktree() {
        // given
        let broken = PhantomData::<OuterRoot>;

        // when
        let failed = failed(broken);

        // then
        assert_eq!(
            failed,
            [
                "resolves_a_nested_linked_worktree_to_itself",
                "resolves_a_nested_repository_to_itself",
                "snapshot_of_each_worktree_is_its_own",
            ]
        );
    }

    #[test]
    fn should_fail_the_ref_moves_when_the_stat_set_leaves_out_the_branch() {
        // given
        let broken = PhantomData::<NoBranchRef>;

        // when
        let failed = failed(broken);

        // then
        assert_eq!(
            failed,
            ["snapshot_sees_a_commit", "snapshot_sees_a_soft_reset"]
        );
    }

    #[test]
    fn should_fail_stability_when_a_query_rewrites_the_index() {
        // given
        let broken = PhantomData::<WritesIndex>;

        // when
        let failed = failed(broken);

        // then
        assert_eq!(failed, ["snapshot_is_stable"]);
    }

    #[test]
    fn should_fail_the_working_cases_when_untracked_files_are_forgotten() {
        // given
        let broken = PhantomData::<NoUntracked>;

        // when
        let failed = failed(broken);

        // then
        assert_eq!(
            failed,
            ["changed_after_a_stash", "changed_reports_working_states"]
        );
    }

    #[test]
    fn should_fail_when_an_unknown_base_gives_an_empty_change_set() {
        // given
        let broken = PhantomData::<EmptyForUnknownBase>;

        // when
        let failed = failed(broken);

        // then
        assert_eq!(failed, ["changed_refuses_an_unknown_base"]);
    }

    #[test]
    fn should_fail_when_a_path_that_is_not_utf8_is_dropped_uncounted() {
        // given
        let broken = PhantomData::<DropsUnrepresentable>;

        // when
        let failed = failed(broken);

        // then
        assert_eq!(
            failed,
            [
                "changed_reports_working_states",
                "tracked_lists_tracked_java_only"
            ]
        );
    }

    #[test]
    fn should_fail_the_tree_moves_when_committed_changes_are_lost() {
        // given
        let broken = PhantomData::<NoCommitted>;

        // when
        let failed = failed(broken);

        // then
        assert_eq!(
            failed,
            [
                "changed_after_a_commit",
                "changed_after_a_checkout",
                "changed_after_a_soft_reset",
                "changed_after_a_hard_reset",
            ]
        );
    }
}
