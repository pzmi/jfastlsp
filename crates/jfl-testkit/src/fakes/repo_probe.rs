//! A scripted repository behind the `RepoProbe` port: commits, branches, worktrees, an index and
//! a working tree per worktree, all in memory, changed by the same operations a test runs with
//! `git` against the real adapter (`contracts::repo_probe::Fixture`).
//!
//! It is not looser than the adapter (ver §5): its stat set has the same files (`HEAD`, the branch
//! `HEAD` names, `packed-refs`, the index) and each moves only when Git would move it (a commit or
//! `reset --soft` moves the branch alone); it refuses a base tree it never made, counts a path
//! that is not UTF-8 instead of dropping it, and keeps a nested worktree's files out of its
//! parent's status. Its `.gitignore` support is none: the contract suite puts no untracked file
//! where an ignore rule would apply. [`FakeRepoProbe::rewrite_index`] does what a `status` that
//! may take optional locks does, for the suite's broken implementation.
//!
//! Pinned by the `repo_probe` contract suite (`contracts::repo_probe::tests`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jfl_application::source_tree::MTime;
use jfl_application::{
    ChangedFiles, GitFile, GitSnapshot, GitStat, GitStep, HeadState, ObjectId, RepoProbe,
    RepoProbeError, StatRecord, TrackedFiles, Worktree, WorktreeKind,
};
use jfl_domain::{FileUri, RelPath};

/// Paths and contents: a tree, an index or a working tree. Paths are bytes, as Git's are.
type Files = BTreeMap<Vec<u8>, Vec<u8>>;

#[derive(Debug, Clone, Copy)]
struct Commit {
    tree: usize,
    parent: Option<usize>,
}

#[derive(Debug, Default)]
struct Repo {
    commits: Vec<Commit>,
    /// Branch name to commit.
    branches: BTreeMap<String, usize>,
    /// Branch name to the logical time its loose ref was last written.
    ref_written: BTreeMap<String, u64>,
}

#[derive(Debug)]
struct Tree {
    worktree: Worktree,
    repo: usize,
    /// The branch `HEAD` names (the fake never detaches).
    branch: String,
    head_written: u64,
    index: Files,
    index_written: u64,
    disk: Files,
}

#[derive(Debug, Default)]
struct Model {
    /// The error every call returns while set ([`FakeRepoProbe::fail_with`]).
    failure: Option<RepoProbeError>,
    repos: Vec<Repo>,
    trees: Vec<Tree>,
    /// Every distinct tree content, by id.
    contents: Vec<Files>,
    clock: u64,
}

/// The scripted repository. Clones share one model.
#[derive(Debug, Clone, Default)]
pub struct FakeRepoProbe {
    model: Arc<Mutex<Model>>,
}

fn uri_join(base: &FileUri, path: &str) -> Result<FileUri, String> {
    let rel = RelPath::new(path).map_err(|e| e.to_string())?;
    base.join(&rel).map_err(|e| e.to_string())
}

/// A tree's id: its index in the model, as 40 hexadecimal digits.
fn tree_id(index: usize) -> Option<ObjectId> {
    ObjectId::parse(&format!("{:040x}", index.saturating_add(1)))
}

fn stat_at(time: u64) -> StatRecord {
    let at = MTime {
        secs: i64::try_from(time).unwrap_or(i64::MAX),
        nanos: 0,
    };
    StatRecord {
        dev: 1,
        inode: time,
        size: time,
        mtime: at,
        ctime: at,
    }
}

/// Whether `path` is `prefix` or below it (`.` is everything).
fn under(path: &[u8], prefix: &str) -> bool {
    prefix == "."
        || path == prefix.as_bytes()
        || path
            .strip_prefix(prefix.as_bytes())
            .is_some_and(|rest| rest.first() == Some(&b'/'))
}

/// Sorted representable paths, and how many were not.
fn convert<'a>(paths: impl Iterator<Item = &'a Vec<u8>>) -> (Vec<RelPath>, u64) {
    let mut out = Vec::new();
    let mut unrepresentable = 0_u64;
    for path in paths {
        match core::str::from_utf8(path)
            .ok()
            .and_then(|text| RelPath::new(text).ok())
        {
            Some(rel) => out.push(rel),
            None => unrepresentable = unrepresentable.saturating_add(1),
        }
    }
    out.sort_unstable();
    out.dedup();
    (out, unrepresentable)
}

/// Paths whose content differs between `a` and `b`, either way.
fn differ<'a>(a: &'a Files, b: &'a Files) -> impl Iterator<Item = &'a Vec<u8>> {
    a.iter()
        .filter(|(path, bytes)| b.get(*path) != Some(*bytes))
        .map(|(path, _)| path)
        .chain(b.keys().filter(|path| !a.contains_key(*path)))
}

impl Model {
    fn tick(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }

    fn intern(&mut self, files: &Files) -> usize {
        if let Some(at) = self.contents.iter().position(|c| c == files) {
            return at;
        }
        self.contents.push(files.clone());
        self.contents.len().saturating_sub(1)
    }

    fn content(&self, tree: usize) -> Files {
        self.contents.get(tree).cloned().unwrap_or_default()
    }

    /// The innermost worktree holding `path` (itself included), and the path below its root.
    fn locate(&self, path: &FileUri) -> Option<(usize, Option<RelPath>)> {
        self.trees
            .iter()
            .enumerate()
            .filter_map(|(at, tree)| {
                if *path == tree.worktree.root {
                    Some((at, None, tree.worktree.root.path().len()))
                } else {
                    path.relative_to(&tree.worktree.root)
                        .map(|rel| (at, Some(rel), tree.worktree.root.path().len()))
                }
            })
            .max_by_key(|(_, _, depth)| *depth)
            .map(|(at, rel, _)| (at, rel))
    }

    fn tree_at(&self, root: &FileUri) -> Result<usize, String> {
        self.trees
            .iter()
            .position(|tree| tree.worktree.root == *root)
            .ok_or_else(|| String::from("no such worktree"))
    }

    fn head_commit(&self, at: usize) -> Option<usize> {
        let tree = self.trees.get(at)?;
        self.repos
            .get(tree.repo)?
            .branches
            .get(&tree.branch)
            .copied()
    }

    fn head_files(&self, at: usize) -> Files {
        self.head_commit(at)
            .and_then(|commit| {
                let tree = self.trees.get(at)?;
                self.repos.get(tree.repo)?.commits.get(commit)
            })
            .map(|commit| self.content(commit.tree))
            .unwrap_or_default()
    }

    fn set_branch(&mut self, repo: usize, branch: &str, commit: usize) {
        let now = self.tick();
        if let Some(repo) = self.repos.get_mut(repo) {
            repo.branches.insert(String::from(branch), commit);
            repo.ref_written.insert(String::from(branch), now);
        }
    }

    /// Makes index and working tree match `to`, removing what `from` and the index tracked.
    fn switch_files(&mut self, at: usize, to: &Files) {
        let now = self.tick();
        let head = self.head_files(at);
        let Some(tree) = self.trees.get_mut(at) else {
            return;
        };
        let tracked: Vec<Vec<u8>> = tree.index.keys().chain(head.keys()).cloned().collect();
        for path in tracked {
            if !to.contains_key(&path) {
                tree.disk.remove(&path);
            }
        }
        for (path, bytes) in to {
            tree.disk.insert(path.clone(), bytes.clone());
        }
        tree.index.clone_from(to);
        tree.index_written = now;
    }
}

impl FakeRepoProbe {
    /// An empty model.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn model(&self) -> MutexGuard<'_, Model> {
        self.model.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes every call fail with `error` (an I/O error, a deadline, a failed `git`) until it is
    /// called again with `None`, as ver §5 asks of a fake: so a caller's handling of each failure
    /// can be tested.
    ///
    /// Pinned by: `fakes::repo_probe::tests::should_fail_every_call_when_told_to`.
    pub fn fail_with(&self, error: Option<RepoProbeError>) {
        self.model().failure = error;
    }

    fn failure(&self) -> Result<(), RepoProbeError> {
        self.model().failure.map_or(Ok(()), Err)
    }

    /// `git init` at `root` and one commit of `files` on `main`.
    pub fn init(&self, root: &FileUri, files: &[(&str, &[u8])]) -> Result<(), String> {
        let mut model = self.model();
        let git_dir = uri_join(root, ".git")?;
        let files: Files = files
            .iter()
            .map(|(path, bytes)| (path.as_bytes().to_vec(), bytes.to_vec()))
            .collect();
        let tree = model.intern(&files);
        let repo = model.repos.len();
        model.repos.push(Repo {
            commits: vec![Commit { tree, parent: None }],
            ..Repo::default()
        });
        model.set_branch(repo, "main", 0);
        let now = model.tick();
        model.trees.push(Tree {
            worktree: Worktree {
                root: root.clone(),
                git_dir: git_dir.clone(),
                common_dir: git_dir,
                kind: WorktreeKind::Main,
            },
            repo,
            branch: String::from("main"),
            head_written: now,
            index: files.clone(),
            index_written: now,
            disk: files,
        });
        Ok(())
    }

    /// `git worktree add -b branch path` in the repository whose worktree is at `main`.
    pub fn add_worktree(&self, main: &FileUri, path: &FileUri, branch: &str) -> Result<(), String> {
        let mut model = self.model();
        let at = model.tree_at(main)?;
        let commit = model.head_commit(at).ok_or("unborn")?;
        let files = model.head_files(at);
        let (repo, common) = {
            let tree = model.trees.get(at).ok_or("no such worktree")?;
            (tree.repo, tree.worktree.common_dir.clone())
        };
        let name = path.path().rsplit('/').next().unwrap_or("worktree");
        let git_dir = uri_join(&common, &format!("worktrees/{name}"))?;
        model.set_branch(repo, branch, commit);
        let now = model.tick();
        model.trees.push(Tree {
            worktree: Worktree {
                root: path.clone(),
                git_dir,
                common_dir: common,
                kind: WorktreeKind::Linked,
            },
            repo,
            branch: String::from(branch),
            head_written: now,
            index: files.clone(),
            index_written: now,
            disk: files,
        });
        Ok(())
    }

    /// Writes a file in the innermost worktree holding `path`; outside every worktree, nothing.
    pub fn write(&self, path: &FileUri, bytes: &[u8]) -> Result<(), String> {
        let mut model = self.model();
        if let Some((at, Some(rel))) = model.locate(path)
            && let Some(tree) = model.trees.get_mut(at)
        {
            tree.disk
                .insert(rel.as_str().as_bytes().to_vec(), bytes.to_vec());
        }
        Ok(())
    }

    /// Writes a `.java` file whose name is not UTF-8 in the directory `dir`.
    pub fn write_non_utf8_java(&self, dir: &FileUri) -> Result<(), String> {
        let mut model = self.model();
        let (at, rel) = model.locate(dir).ok_or("outside every worktree")?;
        let mut name = rel
            .map(|rel| format!("{rel}/").into_bytes())
            .unwrap_or_default();
        name.extend_from_slice(b"\xff-not-utf-8.java");
        if let Some(tree) = model.trees.get_mut(at) {
            tree.disk.insert(name, b"class X {}".to_vec());
        }
        Ok(())
    }

    /// Removes a file, or a directory's files, from the working tree.
    pub fn remove(&self, path: &FileUri) -> Result<(), String> {
        let mut model = self.model();
        let (at, rel) = model.locate(path).ok_or("outside every worktree")?;
        let prefix = rel.map_or_else(|| String::from("."), |rel| rel.as_str().to_owned());
        if let Some(tree) = model.trees.get_mut(at) {
            tree.disk.retain(|path, _| !under(path, &prefix));
        }
        Ok(())
    }

    /// `git add -f -A -- path` in the worktree at `root`.
    pub fn stage(&self, root: &FileUri, path: &str) -> Result<(), String> {
        let mut model = self.model();
        let at = model.tree_at(root)?;
        let now = model.tick();
        let tree = model.trees.get_mut(at).ok_or("no such worktree")?;
        tree.index.retain(|p, _| !under(p, path));
        for (p, bytes) in &tree.disk {
            if under(p, path) {
                tree.index.insert(p.clone(), bytes.clone());
            }
        }
        tree.index_written = now;
        Ok(())
    }

    /// `git commit` of the index, on the branch `HEAD` names. The index is not rewritten: only
    /// the branch's ref moves.
    pub fn commit(&self, root: &FileUri) -> Result<(), String> {
        let mut model = self.model();
        let at = model.tree_at(root)?;
        let parent = model.head_commit(at);
        let (index, repo, branch) = {
            let tree = model.trees.get(at).ok_or("no such worktree")?;
            (tree.index.clone(), tree.repo, tree.branch.clone())
        };
        let tree = model.intern(&index);
        let commits = &mut model.repos.get_mut(repo).ok_or("no repo")?.commits;
        commits.push(Commit { tree, parent });
        let commit = commits.len().saturating_sub(1);
        model.set_branch(repo, &branch, commit);
        Ok(())
    }

    /// `git branch name` at `HEAD`.
    pub fn branch(&self, root: &FileUri, name: &str) -> Result<(), String> {
        let mut model = self.model();
        let at = model.tree_at(root)?;
        let commit = model.head_commit(at).ok_or("unborn")?;
        let repo = model.trees.get(at).ok_or("no such worktree")?.repo;
        model.set_branch(repo, name, commit);
        Ok(())
    }

    /// `git checkout name` of an existing branch, from a clean state.
    pub fn checkout(&self, root: &FileUri, name: &str) -> Result<(), String> {
        let mut model = self.model();
        let at = model.tree_at(root)?;
        let repo = model.trees.get(at).ok_or("no such worktree")?.repo;
        let commit = *model
            .repos
            .get(repo)
            .and_then(|r| r.branches.get(name))
            .ok_or("no such branch")?;
        let tree = model
            .repos
            .get(repo)
            .and_then(|r| r.commits.get(commit))
            .ok_or("no such commit")?
            .tree;
        let files = model.content(tree);
        model.switch_files(at, &files);
        let now = model.tick();
        let tree = model.trees.get_mut(at).ok_or("no such worktree")?;
        tree.branch = String::from(name);
        tree.head_written = now;
        Ok(())
    }

    /// `git stash`: index and tracked files back to `HEAD`; untracked files stay.
    pub fn stash(&self, root: &FileUri) -> Result<(), String> {
        let mut model = self.model();
        let at = model.tree_at(root)?;
        let head = model.head_files(at);
        model.switch_files(at, &head);
        Ok(())
    }

    /// `git reset --soft HEAD~1`, or `--hard` with `hard`.
    pub fn reset(&self, root: &FileUri, hard: bool) -> Result<(), String> {
        let mut model = self.model();
        let at = model.tree_at(root)?;
        let commit = model.head_commit(at).ok_or("unborn")?;
        let (repo, branch) = {
            let tree = model.trees.get(at).ok_or("no such worktree")?;
            (tree.repo, tree.branch.clone())
        };
        let parent = model
            .repos
            .get(repo)
            .and_then(|r| r.commits.get(commit))
            .and_then(|c| c.parent)
            .ok_or("no parent")?;
        if hard {
            let tree = model
                .repos
                .get(repo)
                .and_then(|r| r.commits.get(parent))
                .ok_or("no such commit")?
                .tree;
            let files = model.content(tree);
            model.switch_files(at, &files);
        }
        model.set_branch(repo, &branch, parent);
        Ok(())
    }

    /// Rewrites the index of the worktree at `root` without changing it, as a `status` that may
    /// take optional locks does.
    pub fn rewrite_index(&self, root: &FileUri) {
        let mut model = self.model();
        let now = model.tick();
        if let Ok(at) = model.tree_at(root)
            && let Some(tree) = model.trees.get_mut(at)
        {
            tree.index_written = now;
        }
    }

    fn resolve_now(&self, path: &FileUri) -> Option<Worktree> {
        let model = self.model();
        let (at, _) = model.locate(path)?;
        model.trees.get(at).map(|tree| tree.worktree.clone())
    }

    fn with_tree<T>(
        &self,
        worktree: &Worktree,
        job: impl FnOnce(&Model, usize) -> Result<T, RepoProbeError>,
    ) -> Result<T, RepoProbeError> {
        let model = self.model();
        let at = model
            .tree_at(&worktree.root)
            .map_err(|_| RepoProbeError::NotAWorktree)?;
        job(&model, at)
    }

    fn snapshot_now(&self, worktree: &Worktree) -> Result<GitSnapshot, RepoProbeError> {
        self.with_tree(worktree, |model, at| {
            let tree = model.trees.get(at).ok_or(RepoProbeError::NotAWorktree)?;
            let repo = model
                .repos
                .get(tree.repo)
                .ok_or(RepoProbeError::NotAWorktree)?;
            let path = |base: &FileUri, name: &str| {
                uri_join(base, name).map_err(|_| RepoProbeError::Unrepresentable)
            };
            let branch_ref = format!("refs/heads/{}", tree.branch);
            Ok(GitSnapshot {
                head: HeadState::Symbolic(branch_ref.clone()),
                files: vec![
                    GitStat {
                        file: GitFile::Head,
                        path: path(&tree.worktree.git_dir, "HEAD")?,
                        stat: Some(stat_at(tree.head_written)),
                    },
                    GitStat {
                        file: GitFile::Ref,
                        path: path(&tree.worktree.common_dir, &branch_ref)?,
                        stat: repo.ref_written.get(&tree.branch).copied().map(stat_at),
                    },
                    GitStat {
                        file: GitFile::PackedRefs,
                        path: path(&tree.worktree.common_dir, "packed-refs")?,
                        stat: None,
                    },
                    GitStat {
                        file: GitFile::Index,
                        path: path(&tree.worktree.git_dir, "index")?,
                        stat: Some(stat_at(tree.index_written)),
                    },
                ],
            })
        })
    }

    fn head_tree_now(&self, worktree: &Worktree) -> Result<Option<ObjectId>, RepoProbeError> {
        self.with_tree(worktree, |model, at| {
            let tree = model.trees.get(at).ok_or(RepoProbeError::NotAWorktree)?;
            Ok(model.head_commit(at).and_then(|commit| {
                model
                    .repos
                    .get(tree.repo)
                    .and_then(|r| r.commits.get(commit))
                    .and_then(|c| tree_id(c.tree))
            }))
        })
    }

    fn changed_now(
        &self,
        worktree: &Worktree,
        base: Option<&ObjectId>,
    ) -> Result<ChangedFiles, RepoProbeError> {
        let head_tree = self.head_tree_now(worktree)?;
        self.with_tree(worktree, |model, at| {
            let tree = model.trees.get(at).ok_or(RepoProbeError::NotAWorktree)?;
            let mut changed = ChangedFiles {
                head_tree: head_tree.clone(),
                ..ChangedFiles::default()
            };
            match (base, &head_tree) {
                (None, _) | (Some(_), None) => changed.everything = true,
                (Some(base), Some(head)) if base == head => {}
                (Some(base), Some(_)) => {
                    let known =
                        (0..model.contents.len()).find(|at| tree_id(*at).as_ref() == Some(base));
                    let base_files =
                        known
                            .map(|at| model.content(at))
                            .ok_or(RepoProbeError::GitFailed {
                                step: GitStep::Diff,
                                code: Some(128),
                            })?;
                    let head_files = model.head_files(at);
                    let (paths, unrepresentable) = convert(differ(&base_files, &head_files));
                    changed.committed = paths;
                    changed.unrepresentable = unrepresentable;
                }
            }
            let head_files = model.head_files(at);
            let staged = differ(&head_files, &tree.index);
            let unstaged = differ(&tree.index, &tree.disk).filter(|p| tree.index.contains_key(*p));
            let (working, w) = convert(staged.chain(unstaged));
            let (untracked, u) = convert(tree.disk.keys().filter(|p| !tree.index.contains_key(*p)));
            changed.working = working;
            changed.untracked = untracked;
            changed.unrepresentable = changed.unrepresentable.saturating_add(w).saturating_add(u);
            Ok(changed)
        })
    }

    fn tracked_now(&self, worktree: &Worktree) -> Result<TrackedFiles, RepoProbeError> {
        self.with_tree(worktree, |model, at| {
            let tree = model.trees.get(at).ok_or(RepoProbeError::NotAWorktree)?;
            let (files, unrepresentable) = convert(tree.index.keys().filter(|path| {
                let name = path.rsplit(|b| *b == b'/').next().unwrap_or(path);
                name.len() > ".java".len() && name.ends_with(b".java")
            }));
            Ok(TrackedFiles {
                files,
                unrepresentable,
            })
        })
    }
}

impl RepoProbe for FakeRepoProbe {
    async fn resolve(&self, path: &FileUri) -> Result<Option<Worktree>, RepoProbeError> {
        self.failure()?;
        Ok(self.resolve_now(path))
    }

    async fn snapshot(&self, worktree: &Worktree) -> Result<GitSnapshot, RepoProbeError> {
        self.failure()?;
        self.snapshot_now(worktree)
    }

    async fn head_tree(&self, worktree: &Worktree) -> Result<Option<ObjectId>, RepoProbeError> {
        self.failure()?;
        self.head_tree_now(worktree)
    }

    async fn changed_since(
        &self,
        worktree: &Worktree,
        base: Option<&ObjectId>,
    ) -> Result<ChangedFiles, RepoProbeError> {
        self.failure()?;
        self.changed_now(worktree, base)
    }

    async fn tracked_java(&self, worktree: &Worktree) -> Result<TrackedFiles, RepoProbeError> {
        self.failure()?;
        self.tracked_now(worktree)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::block_on;

    #[test]
    fn should_fail_every_call_when_told_to() -> Result<(), String> {
        // given
        let probe = FakeRepoProbe::new();
        let root = FileUri::from_path("/r").map_err(|e| e.to_string())?;
        probe.init(&root, &[("A.java", b"class A {}")])?;
        let worktree = block_on(probe.resolve(&root))
            .map_err(|e| e.to_string())?
            .ok_or("no worktree")?;
        let io = RepoProbeError::Io(std::io::ErrorKind::Other);

        // when
        probe.fail_with(Some(io));
        let failed = (
            block_on(probe.resolve(&root)).err(),
            block_on(probe.snapshot(&worktree)).err(),
            block_on(probe.tracked_java(&worktree)).err(),
        );
        probe.fail_with(None);
        let recovered = block_on(probe.snapshot(&worktree)).is_ok();

        // then
        assert_eq!(failed, (Some(io), Some(io), Some(io)));
        assert!(recovered);
        Ok(())
    }
}
