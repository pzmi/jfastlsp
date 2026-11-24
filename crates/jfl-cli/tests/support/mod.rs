//! What the end-to-end tests of `jfl lsp` share: a scratch world with a git copy of
//! `fixtures/lsp/worktree/` and a state directory whose daemon is stopped when the world is
//! dropped.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jfl_testkit::fixtures::git::Repo;
use jfl_testkit::fixtures::{ScratchDir, files_under, symlink};
use jfl_testkit::lsp_transcript::substitute;

/// A session's whole run, or one answer, takes well under a few seconds, a daemon's start and its
/// first index included; the bound only stops a server that does not answer or exit.
pub(crate) const DEADLINE: Duration = Duration::from_secs(30);

/// How long the daemon a test started has to answer `jfl/stop`.
const STOP: Duration = Duration::from_secs(10);

/// `fixtures/lsp/`.
pub(crate) fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/lsp")
}

/// One test's world: a worktree under `a+b/c@d/` (so every answer naming a file the client never
/// sent shows the canonical, percent-encoded form), a state directory, and its scratch directory,
/// all removed (the daemon stopped first) when dropped. [`World::through_link`]'s client spells
/// the worktree through a link (`via`, to the directory `real` the worktree is in), as macOS's
/// temporary directory is spelled (`/var` is a link to `/private/var`), so every platform asks the
/// daemon about a file whose canonical path is not the one sent (FND-17).
pub(crate) struct World {
    scratch: ScratchDir,
    pub(crate) root: PathBuf,
}

impl World {
    /// The worktree spelled as it is.
    pub(crate) fn new() -> Self {
        Self::build(false)
    }

    /// The worktree spelled through a link above it.
    pub(crate) fn through_link() -> Self {
        Self::build(true)
    }

    fn build(link: bool) -> Self {
        let scratch = ScratchDir::new(&std::env::temp_dir(), "jfl-lsp").unwrap();
        let worktree = Path::new("a+b").join("c@d").join("wt");
        let files = files_under(&fixtures().join("worktree")).unwrap();
        let borrowed: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .collect();
        Repo::init(&scratch.path().join("real").join(&worktree), &borrowed).unwrap();
        let root = if link {
            symlink(Path::new("real"), &scratch.path().join("via")).unwrap();
            scratch.path().join("via").join(&worktree)
        } else {
            scratch.path().join("real").join(&worktree)
        };
        Self { scratch, root }
    }

    pub(crate) fn state(&self) -> PathBuf {
        self.scratch.path().join("state")
    }

    /// `text` with `{{root}}` as the client spells the worktree's path and `{{root_uri}}` as the
    /// daemon writes it.
    pub(crate) fn substitute(&self, text: &str) -> String {
        let canonical = self
            .root
            .to_string_lossy()
            .replace('+', "%2B")
            .replace('@', "%40");
        substitute(text, &self.root).replace("{{root_uri}}", &canonical)
    }

    /// `jfl lsp`'s arguments for this world's state directory, and the `PATH` it runs with
    /// (the daemon runs `git`; every other variable is cleared).
    pub(crate) fn command(&self) -> (Vec<String>, OsString) {
        let state = self.state().to_string_lossy().into_owned();
        let args = ["lsp", "--state-dir", &state, "--idle-timeout", "60"].map(str::to_owned);
        let path = std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"));
        (args.to_vec(), path)
    }

    /// The scratch directory, for the runner's files.
    pub(crate) fn scratch(&self) -> &Path {
        self.scratch.path()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _stopped = jfl_composition::daemon::stop_running(&self.state(), STOP);
    }
}
