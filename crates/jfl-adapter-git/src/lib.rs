//! `RepoProbe` over the `git` CLI: worktree roots, common directory, `HEAD` and index stat,
//! changed files. Role: adapter.
//!
//! Two paths (ADR 0024):
//!
//! - **The hot path reads files, never spawns:** [`resolve`] walks up from a request's directory
//!   with one `stat` per level and confirms each new root with Git once (cached per root);
//!   [`snapshot`] stats `HEAD`, the refs it leads to, `packed-refs` and the index, reading only
//!   `HEAD` and loose refs.
//! - **The reconcile path runs `git`** through `jfl-process`, hardened as [`cli`] describes:
//!   `rev-parse HEAD^{tree}`, `diff --name-only -z` between two trees, `status -z
//!   --porcelain=v2`, `ls-files -z`; each parsed by [`parse`], bounded, and refused whole when it
//!   is not in the form asked for, when it passes its output limit or when its deadline passes.
//!
//! Every blocking job runs on the [`BlockingPool`] composition gives the adapter (fnd §4.4); the
//! `*_blocking` methods are the same jobs for callers already off the runtime (the benchmark).
//!
//! What it does is the `repo_probe` contract suite, run against it in `tests/contract.rs`.

use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use jfl_application::{
    BlockingPool, ChangedFiles, GitSnapshot, GitStep, ObjectId, RepoProbe, RepoProbeError,
    TrackedFiles, Worktree,
};
use jfl_domain::FileUri;
use jfl_process::{EnvSource, ProcessEnv, Runner};

pub mod cli;
pub mod parse;
pub mod resolve;
pub mod snapshot;

use cli::Git;

/// The pathspecs of [`RepoProbe::tracked_java`]'s `ls-files`: each indexed language's extension
/// (`jfl_application::INDEXED_LANGUAGES`, `.java` today) as the listing matches it, case and all,
/// and each extension of `jfl_application::source_tree::OTHER_JVM_EXTENSIONS` and
/// `BUILD_SCRIPT_SUFFIXES` (`.gradle.kts` is under `.kts`) without regard to case, as
/// `source_kind` matches them (IDX-16). `*` matches across `/` in a pathspec without `glob`
/// magic, so each names the extension anywhere in the tree.
///
/// Pinned by: `tests::should_name_every_extension_of_the_registry_when_tracked_files_are_asked_for`.
pub const TRACKED_PATHSPECS: [&str; 23] = [
    "*.java",
    ":(icase)*.kt",
    ":(icase)*.kts",
    ":(icase)*.groovy",
    ":(icase)*.gvy",
    ":(icase)*.gy",
    ":(icase)*.gsh",
    ":(icase)*.scala",
    ":(icase)*.sc",
    ":(icase)*.clj",
    ":(icase)*.cljc",
    ":(icase)*.xtend",
    ":(icase)*.jsp",
    ":(icase)*.jspf",
    ":(icase)*.jspx",
    ":(icase)*.tag",
    ":(icase)*.tagx",
    ":(icase)*.aj",
    ":(icase)*.bsh",
    ":(icase)*.gsp",
    ":(icase)*.jte",
    ":(icase)*.kte",
    ":(icase)*.gradle",
];

/// The output a reconcile command may print: 64 MiB, room for `ls-files` of a worktree two
/// hundred times the largest corpus repository (elasticsearch, 33.9k files); more is refused.
pub const DEFAULT_MAX_OUTPUT: usize = 64 << 20;

/// The deadline of a reconcile command.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);

/// The deadline of a root's confirmation, on the request path.
pub const DEFAULT_CONFIRM_DEADLINE: Duration = Duration::from_secs(5);

/// How the adapter runs `git`.
#[derive(Debug, Clone)]
pub struct GitConfig {
    /// The `git` program: a bare name is looked up on the inherited `PATH`.
    pub program: OsString,
    /// A canonical directory at and above which no root is looked for (as Git's
    /// `GIT_CEILING_DIRECTORIES`): tests set their scratch directory, so a checkout around it is
    /// never found.
    pub ceiling: Option<PathBuf>,
    /// Each reconcile command's deadline.
    pub deadline: Duration,
    /// Each root confirmation's deadline.
    pub confirm_deadline: Duration,
    /// Each reconcile command's output limit.
    pub max_output: usize,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            program: OsString::from("git"),
            ceiling: None,
            deadline: DEFAULT_DEADLINE,
            confirm_deadline: DEFAULT_CONFIRM_DEADLINE,
            max_output: DEFAULT_MAX_OUTPUT,
        }
    }
}

#[derive(Debug)]
struct Inner<E> {
    git: Git<E>,
    config: GitConfig,
    roots: resolve::Roots,
}

/// The `git` CLI `RepoProbe`.
#[derive(Debug)]
pub struct GitRepoProbe<P, E = ProcessEnv> {
    pool: P,
    inner: Arc<Inner<E>>,
}

impl<P: Clone, E> Clone for GitRepoProbe<P, E> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<P: BlockingPool> GitRepoProbe<P, ProcessEnv> {
    /// An adapter over the daemon's own environment (for `PATH` and `HOME`), running its blocking
    /// jobs on `pool`.
    pub fn new(pool: P, config: GitConfig) -> Self {
        Self::with_runner(pool, Runner::new(), config)
    }
}

impl<P: BlockingPool, E: EnvSource> GitRepoProbe<P, E> {
    /// An adapter starting `git` through `runner`.
    pub fn with_runner(pool: P, runner: Runner<E>, config: GitConfig) -> Self {
        Self {
            pool,
            inner: Arc::new(Inner {
                git: Git::new(runner, config.program.clone()),
                config,
                roots: resolve::Roots::default(),
            }),
        }
    }

    /// The hardened `git` this adapter runs (for the benchmark and the hardening tests).
    pub fn git(&self) -> &Git<E> {
        &self.inner.git
    }

    /// [`RepoProbe::resolve`], on the calling thread.
    pub fn resolve_blocking(&self, path: &FileUri) -> Result<Option<Worktree>, RepoProbeError> {
        self.inner.resolve(path)
    }

    /// [`RepoProbe::snapshot`], on the calling thread.
    pub fn snapshot_blocking(&self, worktree: &Worktree) -> Result<GitSnapshot, RepoProbeError> {
        snapshot::snapshot(worktree)
    }

    /// [`RepoProbe::head_tree`], on the calling thread.
    pub fn head_tree_blocking(
        &self,
        worktree: &Worktree,
    ) -> Result<Option<ObjectId>, RepoProbeError> {
        self.inner.head_tree(worktree)
    }

    /// [`RepoProbe::changed_since`], on the calling thread.
    pub fn changed_since_blocking(
        &self,
        worktree: &Worktree,
        base: Option<&ObjectId>,
    ) -> Result<ChangedFiles, RepoProbeError> {
        self.inner.changed_since(worktree, base)
    }

    /// [`RepoProbe::tracked_java`], on the calling thread.
    pub fn tracked_java_blocking(
        &self,
        worktree: &Worktree,
    ) -> Result<TrackedFiles, RepoProbeError> {
        self.inner.tracked_java(worktree)
    }
}

impl<E: EnvSource> Inner<E> {
    fn resolve(&self, path: &FileUri) -> Result<Option<Worktree>, RepoProbeError> {
        self.roots.resolve(&self.git, &self.config, path)
    }

    fn run(
        &self,
        step: GitStep,
        worktree: &Worktree,
        args: &[&str],
        accepted: &[i32],
    ) -> Result<cli::Printed, RepoProbeError> {
        self.run_overriding(step, worktree, args, accepted, &[])
    }

    fn run_overriding(
        &self,
        step: GitStep,
        worktree: &Worktree,
        args: &[&str],
        accepted: &[i32],
        overrides: &[(OsString, OsString)],
    ) -> Result<cli::Printed, RepoProbeError> {
        self.git.call(
            Path::new(worktree.root.path()),
            &cli::Call {
                step,
                args,
                stdout_limit: self.config.max_output,
                deadline: self.config.deadline,
                accepted,
                overrides,
            },
        )
    }

    /// Every filter driver the repository's configuration defines, turned off: empty `clean`,
    /// `smudge` and `process`, and `required=false` so a missing filter is not an error
    /// (`cli`'s module doc says why). `git config --get-regexp` reads keys and runs nothing; exit
    /// 1 is "no such key". Read again for each reconcile (one spawn), so a driver added since is
    /// covered. Pinned by `tests/hardening.rs::should_run_no_filter_when_the_repository_configures_one`.
    fn filters_off(
        &self,
        worktree: &Worktree,
    ) -> Result<Vec<(OsString, OsString)>, RepoProbeError> {
        let printed = self.run(
            GitStep::Config,
            worktree,
            &[
                "config",
                "--includes",
                "-z",
                "--name-only",
                "--get-regexp",
                "^filter\\.",
            ],
            &[1],
        )?;
        let drivers = parse::filter_drivers(&printed.stdout)
            .map_err(|_| RepoProbeError::Malformed(GitStep::Config))?;
        let mut overrides = Vec::with_capacity(drivers.len().saturating_mul(4));
        for driver in drivers {
            for (variable, value) in [
                ("clean", ""),
                ("smudge", ""),
                ("process", ""),
                ("required", "false"),
            ] {
                let mut key = b"filter.".to_vec();
                key.extend_from_slice(&driver);
                key.push(b'.');
                key.extend_from_slice(variable.as_bytes());
                overrides.push((OsString::from_vec(key), OsString::from(value)));
            }
        }
        Ok(overrides)
    }

    /// `rev-parse --verify --quiet HEAD^{tree}`: exit 1 with nothing printed is an unborn branch.
    fn head_tree(&self, worktree: &Worktree) -> Result<Option<ObjectId>, RepoProbeError> {
        let printed = self.run(
            GitStep::HeadTree,
            worktree,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                "HEAD^{tree}",
            ],
            &[1],
        )?;
        if printed.exit != jfl_process::Exit::Code(0) {
            return if printed.stdout.is_empty() {
                Ok(None)
            } else {
                Err(RepoProbeError::Malformed(GitStep::HeadTree))
            };
        }
        parse::object_line(&printed.stdout)
            .map(Some)
            .map_err(|_| RepoProbeError::Malformed(GitStep::HeadTree))
    }

    fn changed_since(
        &self,
        worktree: &Worktree,
        base: Option<&ObjectId>,
    ) -> Result<ChangedFiles, RepoProbeError> {
        let head = self.head_tree(worktree)?;
        let filters_off = self.filters_off(worktree)?;
        let mut changed = ChangedFiles {
            head_tree: head.clone(),
            ..ChangedFiles::default()
        };
        match (base, &head) {
            (None, _) | (Some(_), None) => changed.everything = true,
            (Some(base), Some(head)) if base == head => {}
            (Some(base), Some(head)) => {
                let printed = self.run_overriding(
                    GitStep::Diff,
                    worktree,
                    &[
                        "diff",
                        "--name-only",
                        "-z",
                        "--no-renames",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--no-color",
                        "--no-relative",
                        "--end-of-options",
                        base.as_str(),
                        head.as_str(),
                    ],
                    &[],
                    &filters_off,
                )?;
                let paths = parse::names(&printed.stdout)
                    .map_err(|_| RepoProbeError::Malformed(GitStep::Diff))?;
                changed.committed = paths.paths;
                changed.unrepresentable = paths.unrepresentable;
            }
        }
        let printed = self.run_overriding(
            GitStep::Status,
            worktree,
            &[
                "status",
                "-z",
                "--porcelain=v2",
                "--untracked-files=all",
                "--no-renames",
                "--ignore-submodules=all",
            ],
            &[],
            &filters_off,
        )?;
        let status = parse::status(&printed.stdout)
            .map_err(|_| RepoProbeError::Malformed(GitStep::Status))?;
        changed.working = status.working.paths;
        changed.untracked = status.untracked.paths;
        changed.unrepresentable = changed
            .unrepresentable
            .saturating_add(status.working.unrepresentable)
            .saturating_add(status.untracked.unrepresentable);
        Ok(changed)
    }

    fn tracked_java(&self, worktree: &Worktree) -> Result<TrackedFiles, RepoProbeError> {
        let mut args = vec!["ls-files", "-z", "--cached", "--full-name", "--"];
        args.extend(TRACKED_PATHSPECS);
        let printed = self.run(GitStep::LsFiles, worktree, &args, &[])?;
        let (java, other) = parse::tracked_names(&printed.stdout)
            .map_err(|_| RepoProbeError::Malformed(GitStep::LsFiles))?;
        Ok(TrackedFiles {
            files: java.paths,
            unrepresentable: java.unrepresentable,
            other: other.paths,
            other_unrepresentable: other.unrepresentable,
        })
    }
}

impl<P, E> RepoProbe for GitRepoProbe<P, E>
where
    P: BlockingPool,
    E: EnvSource + Send + Sync + 'static,
{
    async fn resolve(&self, path: &FileUri) -> Result<Option<Worktree>, RepoProbeError> {
        let (inner, path) = (Arc::clone(&self.inner), path.clone());
        self.pool.run(move || inner.resolve(&path)).await?
    }

    async fn snapshot(&self, worktree: &Worktree) -> Result<GitSnapshot, RepoProbeError> {
        let worktree = worktree.clone();
        self.pool.run(move || snapshot::snapshot(&worktree)).await?
    }

    async fn head_tree(&self, worktree: &Worktree) -> Result<Option<ObjectId>, RepoProbeError> {
        let (inner, worktree) = (Arc::clone(&self.inner), worktree.clone());
        self.pool.run(move || inner.head_tree(&worktree)).await?
    }

    async fn changed_since(
        &self,
        worktree: &Worktree,
        base: Option<&ObjectId>,
    ) -> Result<ChangedFiles, RepoProbeError> {
        let (inner, worktree, base) = (Arc::clone(&self.inner), worktree.clone(), base.cloned());
        self.pool
            .run(move || inner.changed_since(&worktree, base.as_ref()))
            .await?
    }

    async fn tracked_java(&self, worktree: &Worktree) -> Result<TrackedFiles, RepoProbeError> {
        let (inner, worktree) = (Arc::clone(&self.inner), worktree.clone());
        self.pool.run(move || inner.tracked_java(&worktree)).await?
    }
}

/// An I/O error as the port's error, carrying no path.
pub(crate) fn map_io(error: &io::Error) -> RepoProbeError {
    match error.kind() {
        io::ErrorKind::NotFound => RepoProbeError::NotFound,
        io::ErrorKind::PermissionDenied => RepoProbeError::PermissionDenied,
        kind => RepoProbeError::Io(kind),
    }
}

#[cfg(test)]
mod tests {
    use jfl_application::INDEXED_LANGUAGES;
    use jfl_application::source_tree::{BUILD_SCRIPT_SUFFIXES, OTHER_JVM_EXTENSIONS};

    use super::TRACKED_PATHSPECS;

    #[test]
    fn should_name_every_extension_of_the_registry_when_tracked_files_are_asked_for() {
        // given
        let wanted: Vec<String> = OTHER_JVM_EXTENSIONS
            .iter()
            .map(|(extension, _)| *extension)
            .chain(BUILD_SCRIPT_SUFFIXES.iter().copied())
            .filter(|suffix| *suffix != ".gradle.kts")
            .map(|suffix| format!(":(icase)*{suffix}"))
            .chain(
                INDEXED_LANGUAGES
                    .iter()
                    .flat_map(|indexed| indexed.extensions)
                    .map(|extension| format!("*{extension}")),
            )
            .collect();
        // when
        let mut asked: Vec<&str> = TRACKED_PATHSPECS.to_vec();
        asked.sort_unstable();
        let mut wanted: Vec<&str> = wanted.iter().map(String::as_str).collect();
        wanted.sort_unstable();
        // then
        assert_eq!(asked, wanted);
    }
}
