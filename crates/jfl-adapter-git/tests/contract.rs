//! The `repo_probe` contract suite (`jfl_testkit::contracts::repo_probe`, ver §5) against the git
//! adapter, on real repositories and linked worktrees made with the `git` CLI.

use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use jfl_adapter_git::GitRepoProbe;
use jfl_domain::FileUri;
use jfl_testkit::contracts::repo_probe::{Factory, Fixture, check};
use jfl_testkit::contracts::{NonUtf8Name, PLATFORM_NON_UTF8_NAME};
use jfl_testkit::exec::InlinePool;
use jfl_testkit::fixtures::git::Repo;

#[cfg(test)]
mod support;
use support::Scratch;

struct Real {
    scratch: Scratch,
    probe: GitRepoProbe<InlinePool>,
}

impl Real {
    fn repo(&self, worktree: &str) -> Repo {
        Repo::open(&self.scratch.path().join(worktree))
    }
}

impl Fixture for Real {
    type Probe = GitRepoProbe<InlinePool>;

    fn probe(&self) -> &Self::Probe {
        &self.probe
    }

    fn uri(&self, path: &str) -> FileUri {
        self.scratch.uri(path)
    }

    fn init(&self, dir: &str, files: &[(&str, &[u8])]) -> Result<(), String> {
        Repo::init(&self.scratch.path().join(dir), files).map(drop)
    }

    fn add_worktree(&self, main: &str, path: &str, branch: &str) -> Result<(), String> {
        self.repo(main)
            .add_worktree(&self.scratch.path().join(path), branch)
            .map(drop)
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        Repo::open(self.scratch.path()).write(path, bytes)
    }

    fn write_non_utf8_java(&self, dir: &str) -> Result<NonUtf8Name, String> {
        let name = OsStr::from_bytes(b"\xff-not-utf-8.java");
        match fs::write(
            self.scratch.path().join(dir).join(Path::new(name)),
            b"class X {}",
        ) {
            Ok(()) => Ok(NonUtf8Name::Written),
            Err(e) if e.raw_os_error() == Some(libc::EILSEQ) => Ok(NonUtf8Name::Refused),
            Err(e) => Err(e.to_string()),
        }
    }

    fn remove(&self, path: &str) -> Result<(), String> {
        fs::remove_file(self.scratch.path().join(path)).map_err(|e| e.to_string())
    }

    fn stage(&self, worktree: &str, path: &str) -> Result<(), String> {
        self.repo(worktree).stage(path)
    }

    fn commit(&self, worktree: &str) -> Result<(), String> {
        self.repo(worktree).commit("contract")
    }

    fn branch(&self, worktree: &str, name: &str) -> Result<(), String> {
        self.repo(worktree).git(&["branch", name]).map(drop)
    }

    fn checkout(&self, worktree: &str, name: &str) -> Result<(), String> {
        self.repo(worktree)
            .git(&["checkout", "--quiet", name])
            .map(drop)
    }

    fn stash(&self, worktree: &str) -> Result<(), String> {
        self.repo(worktree)
            .git(&["stash", "push", "--quiet"])
            .map(drop)
    }

    fn reset(&self, worktree: &str, hard: bool) -> Result<(), String> {
        let mode = if hard { "--hard" } else { "--soft" };
        self.repo(worktree)
            .git(&["reset", "--quiet", mode, "HEAD~1"])
            .map(drop)
    }
}

struct RealGit;

impl Factory for RealGit {
    type Fixture = Real;

    fn fixture(&self) -> Result<Real, String> {
        let scratch = Scratch::new("contract");
        Ok(Real {
            probe: scratch.probe(),
            scratch,
        })
    }
}

#[test]
fn should_pass_the_repo_probe_contract_when_run_against_real_git() {
    // given
    let factory = RealGit;

    // when
    let violations = check(&factory);

    // then
    assert_eq!(violations, []);
}

#[test]
fn should_write_or_refuse_a_non_utf8_name_as_the_platform_does_when_the_fixture_makes_one() {
    // given: Linux holds the name; APFS refuses it with EILSEQ (PLATFORM_NON_UTF8_NAME)
    let fixture = RealGit.fixture().unwrap();
    fixture
        .init("main", &[("src/A.java", b"class A {}\n")])
        .unwrap();

    // when
    let outcome = fixture.write_non_utf8_java("main/src");

    // then
    assert_eq!(outcome, Ok(PLATFORM_NON_UTF8_NAME));
}
