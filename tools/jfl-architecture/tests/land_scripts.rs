//! The integration scripts of FND-03, driven against scratch repositories (a bare "origin", a
//! clone and sometimes a second clone) in a fresh directory per case: `scripts/land.sh`,
//! `scripts/dco-check.sh`, `scripts/adr-index.sh` and `adrs new` with the repository's
//! `adrs.toml`. Nothing here touches the real remote or the repository's own git configuration:
//! every child gets its own global git config (no signing, a fixed author) and a stub `just`
//! whose `check` runs the scratch repository's `gate.sh`, so a case decides what the gate says.
//!
//! The cases run `git`, `bash`, `cargo` (the lockfile case, with no dependencies, so offline, and
//! land.sh's build of this checkout's xtask for its tree check) and the pinned `adrs` from PATH and
//! mise; a missing tool fails a case, never skips it (fnd §8).

use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn script(name: &str) -> PathBuf {
    repo_root().join("scripts").join(name)
}

/// What a child printed, both streams, for assertions and failure messages.
fn printed(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn succeeded(output: &Output) -> Result<String, Box<dyn Error>> {
    let text = printed(output);
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!("command failed:\n{text}").into())
    }
}

/// A scratch universe: a directory per case holding a global git config, a `bin/` with the stub
/// `just`, a bare `origin.git`, the clone `work` (what land.sh runs in) and, on demand, `other`.
struct Lab {
    base: PathBuf,
}

/// The stub gate: `just check` runs `./gate.sh`; `--summary` and `--evaluate check_stages` read
/// `./recipes.txt` and `./stages.txt` when the scratch repository has them (the private-names
/// hook), and print nothing otherwise.
const STUB_JUST: &str = r#"#!/usr/bin/env bash
case "$1" in
  --summary) cat ./recipes.txt 2>/dev/null || true ;;
  --evaluate) cat ./stages.txt 2>/dev/null || true ;;
  check) exec bash ./gate.sh ;;
  *) echo "stub just: unexpected $*" >&2; exit 2 ;;
esac
"#;

impl Lab {
    fn new(case: &str) -> Result<Self, Box<dyn Error>> {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("land-scripts")
            .join(case);
        if base.try_exists()? {
            fs::remove_dir_all(&base)?;
        }
        fs::create_dir_all(base.join("bin"))?;
        fs::create_dir_all(base.join("tmp"))?;
        fs::write(base.join("bin/just"), STUB_JUST)?;
        fs::write(
            base.join("gitconfig"),
            "[user]\n\tname = Ann Author\n\temail = ann@example.test\n\
             [commit]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n\
             [protocol \"file\"]\n\tallow = always\n",
        )?;
        let lab = Self { base };
        succeeded(&lab.run("bash", &["-c", "chmod +x bin/just"], &lab.base)?)?;
        succeeded(&lab.run("git", &["init", "--bare", "-q", "origin.git"], &lab.base)?)?;
        Ok(lab)
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.base.join(name)
    }

    /// A child with the lab's environment: its own git config, the stub `just` first on PATH and
    /// the temporary directory inside the lab (land.sh's worktrees); CARGO_TARGET_DIR passes
    /// through, so land.sh's tree check (scripts/xtask.sh) finds the xtask the gate built.
    fn command(&self, program: impl AsRef<std::ffi::OsStr>, dir: &Path) -> Command {
        let mut path = OsString::from(self.base.join("bin"));
        if let Some(rest) = std::env::var_os("PATH") {
            path.push(":");
            path.push(rest);
        }
        let mut command = Command::new(program);
        command
            .current_dir(dir)
            .env_clear()
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", self.base.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("TMPDIR", self.base.join("tmp"));
        for name in ["HOME", "RUSTUP_HOME", "CARGO_HOME", "CARGO_TARGET_DIR"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
    }

    fn run(&self, program: &str, args: &[&str], dir: &Path) -> Result<Output, Box<dyn Error>> {
        Ok(self.command(program, dir).args(args).output()?)
    }

    fn git(&self, repo: &str, args: &[&str]) -> Result<String, Box<dyn Error>> {
        succeeded(&self.run("git", args, &self.dir(repo))?)
    }

    fn git_out(&self, repo: &str, args: &[&str]) -> Result<String, Box<dyn Error>> {
        let output = self.run("git", args, &self.dir(repo))?;
        succeeded(&output)?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Write `files` into the clone `repo`, replacing what is there.
    fn write(&self, repo: &str, files: &[(&str, &str)]) -> TestResult {
        for (path, content) in files {
            let target = self.dir(repo).join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(target, content)?;
        }
        Ok(())
    }

    /// The clone `work`, with one pushed commit holding `files` plus a passing `gate.sh`.
    fn seed(&self, files: &[(&str, &str)]) -> TestResult {
        self.clone_origin("work")?;
        self.write("work", &[("gate.sh", "exit 0\n"), ("a.txt", "a\n")])?;
        self.write("work", files)?;
        self.git("work", &["add", "."])?;
        self.git("work", &["commit", "-q", "-s", "-m", "init"])?;
        self.git("work", &["push", "-q", "origin", "main"])?;
        Ok(())
    }

    fn clone_origin(&self, name: &str) -> TestResult {
        succeeded(&self.run("git", &["clone", "-q", "origin.git", name], &self.base)?)?;
        Ok(())
    }

    /// A commit of `files` on a new branch of `work`, signed off by its author, returned by sha;
    /// the clone is left on `main`.
    fn lane_commit(
        &self,
        lane: &str,
        files: &[(&str, &str)],
        subject: &str,
    ) -> Result<String, Box<dyn Error>> {
        self.git("work", &["checkout", "-q", "-b", lane, "main"])?;
        self.write("work", files)?;
        self.git("work", &["add", "."])?;
        self.git("work", &["commit", "-q", "-s", "-m", subject])?;
        let sha = self.git_out("work", &["rev-parse", "HEAD"])?;
        self.git("work", &["checkout", "-q", "main"])?;
        Ok(sha)
    }

    /// A commit of `files` pushed to origin by the second clone: integration moving on.
    fn origin_commit(&self, files: &[(&str, &str)], subject: &str) -> TestResult {
        if !self.dir("other").try_exists()? {
            self.clone_origin("other")?;
        }
        self.git("other", &["pull", "-q", "origin", "main"])?;
        self.write("other", files)?;
        self.git("other", &["add", "."])?;
        self.git("other", &["commit", "-q", "-s", "-m", subject])?;
        self.git("other", &["push", "-q", "origin", "main"])?;
        Ok(())
    }

    /// land.sh as the lab's author (the scratch repositories are not the owner's).
    fn land(&self, args: &[&str]) -> Result<Output, Box<dyn Error>> {
        let mut all = vec!["--author", "Ann Author <ann@example.test>"];
        all.extend_from_slice(args);
        self.land_raw(&all)
    }

    fn land_raw(&self, args: &[&str]) -> Result<Output, Box<dyn Error>> {
        Ok(self
            .command("bash", &self.dir("work"))
            .arg(script("land.sh"))
            .args(args)
            .output()?)
    }

    fn origin_head(&self) -> Result<String, Box<dyn Error>> {
        self.git_out("work", &["ls-remote", "origin", "refs/heads/main"])
            .map(|line| {
                line.split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
    }

    fn origin_subjects(&self) -> Result<String, Box<dyn Error>> {
        self.git_out("work", &["log", "--format=%s", "origin/main"])
    }
}

#[test]
fn should_land_a_clean_commit_with_its_author_and_trailers_when_the_gate_is_green() -> TestResult {
    // given
    let lab = Lab::new("land-clean")?;
    lab.seed(&[])?;
    let sha = lab.lane_commit("lane", &[("b.txt", "b\n")], "T1: add b\n\nImpact: none")?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    let text = succeeded(&output)?;
    assert!(text.contains("landed"), "{text}");
    lab.git("work", &["fetch", "-q", "origin"])?;
    assert_eq!(lab.origin_subjects()?, "T1: add b\ninit");
    let landed = lab.git_out(
        "work",
        &["log", "-1", "--format=%an <%ae>|%B", "origin/main"],
    )?;
    assert!(
        landed.starts_with("Ann Author <ann@example.test>|T1: add b"),
        "{landed}"
    );
    assert!(
        landed.contains("Signed-off-by: Ann Author <ann@example.test>"),
        "{landed}"
    );
    assert_eq!(
        lab.git_out("work", &["rev-parse", "main"])?,
        lab.origin_head()?
    );
    assert_eq!(
        lab.git_out("work", &["worktree", "list", "--porcelain"])?
            .matches("worktree ")
            .count(),
        1
    );
    Ok(())
}

#[test]
fn should_push_nothing_and_keep_the_result_when_run_with_no_push() -> TestResult {
    // given
    let lab = Lab::new("land-no-push")?;
    lab.seed(&[])?;
    let before = lab.origin_head()?;
    let sha = lab.lane_commit("lane", &[("b.txt", "b\n")], "T1: add b")?;

    // when
    let output = lab.land(&["--no-push", "--allow-unsigned", &sha])?;

    // then
    let text = succeeded(&output)?;
    assert!(text.contains("nothing pushed"), "{text}");
    assert_eq!(lab.origin_head()?, before);
    let kept = lab.git_out("work", &["rev-parse", "refs/land/last"])?;
    assert_eq!(
        lab.git_out("work", &["log", "-1", "--format=%s", &kept])?,
        "T1: add b"
    );
    Ok(())
}

#[test]
fn should_refuse_a_dirty_checkout_when_a_file_is_untracked_or_modified() -> TestResult {
    // given
    let lab = Lab::new("land-dirty")?;
    lab.seed(&[])?;
    let sha = lab.lane_commit("lane", &[("b.txt", "b\n")], "T1: add b")?;
    let before = lab.origin_head()?;

    for (path, content) in [("stray.txt", "x\n"), ("a.txt", "changed\n")] {
        // when
        lab.write("work", &[(path, content)])?;
        let output = lab.land(&["--allow-unsigned", &sha])?;

        // then
        let text = printed(&output);
        assert!(!output.status.success(), "{text}");
        assert!(text.contains("dirty"), "{text}");
        assert_eq!(lab.origin_head()?, before);
        lab.git("work", &["checkout", "-q", "--", "a.txt"])?;
        let _ = fs::remove_file(lab.dir("work").join("stray.txt"));
    }
    Ok(())
}

#[test]
fn should_abort_naming_the_files_when_a_non_lockfile_conflict_arises() -> TestResult {
    // given: integration changed a.txt and c.txt after the lane branched; the lane changed both too
    let lab = Lab::new("land-conflict")?;
    lab.seed(&[("c.txt", "c\n")])?;
    let sha = lab.lane_commit(
        "lane",
        &[("a.txt", "lane\n"), ("c.txt", "lane\n")],
        "T1: edit",
    )?;
    lab.origin_commit(&[("a.txt", "main\n"), ("c.txt", "main\n")], "other: edit")?;
    let before = lab.origin_head()?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("a.txt") && text.contains("c.txt"), "{text}");
    assert_eq!(lab.origin_head()?, before);
    assert_eq!(
        lab.git_out("work", &["worktree", "list", "--porcelain"])?
            .matches("worktree ")
            .count(),
        1
    );
    Ok(())
}

#[test]
fn should_move_nothing_when_just_check_fails_on_the_merged_tree() -> TestResult {
    // given: the lane's commit breaks the gate
    let lab = Lab::new("land-red")?;
    lab.seed(&[])?;
    let local_before = lab.git_out("work", &["rev-parse", "main"])?;
    let before = lab.origin_head()?;
    let sha = lab.lane_commit(
        "lane",
        &[("gate.sh", "echo red >&2\nexit 1\n")],
        "T1: break",
    )?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("just check failed"), "{text}");
    assert_eq!(lab.origin_head()?, before);
    assert_eq!(lab.git_out("work", &["rev-parse", "main"])?, local_before);
    assert_eq!(
        lab.git_out("work", &["worktree", "list", "--porcelain"])?
            .matches("worktree ")
            .count(),
        1
    );
    Ok(())
}

#[test]
fn should_refuse_an_unsigned_commit_when_allow_unsigned_is_not_given() -> TestResult {
    // given: the scratch repository never signs
    let lab = Lab::new("land-unsigned")?;
    lab.seed(&[])?;
    let sha = lab.lane_commit("lane", &[("b.txt", "b\n")], "T1: add b")?;
    let before = lab.origin_head()?;

    // when
    let output = lab.land(&[&sha])?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("not signed"), "{text}");
    assert_eq!(lab.origin_head()?, before);
    Ok(())
}

#[test]
fn should_refuse_a_commit_without_its_authors_sign_off_when_landing() -> TestResult {
    // given
    let lab = Lab::new("land-dco")?;
    lab.seed(&[])?;
    lab.git("work", &["checkout", "-q", "-b", "lane"])?;
    lab.write("work", &[("b.txt", "b\n")])?;
    lab.git("work", &["add", "."])?;
    lab.git("work", &["commit", "-q", "-m", "T1: no sign-off"])?;
    let sha = lab.git_out("work", &["rev-parse", "HEAD"])?;
    lab.git("work", &["checkout", "-q", "main"])?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("Signed-off-by"), "{text}");
    Ok(())
}

const LOCK_CRATE: &str = "[package]\nname = \"NAME\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";

fn crate_files(name: &str) -> Vec<(String, String)> {
    vec![
        (
            format!("crates/{name}/Cargo.toml"),
            LOCK_CRATE.replace("NAME", name),
        ),
        (format!("crates/{name}/src/lib.rs"), String::new()),
    ]
}

#[test]
fn should_resolve_cargo_lock_by_taking_integration_and_updating_when_both_sides_add_a_crate()
-> TestResult {
    // given: a glob workspace, so only Cargo.lock conflicts
    let lab = Lab::new("land-lock")?;
    let mut seed = crate_files("a");
    seed.push((
        "Cargo.toml".to_owned(),
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n".to_owned(),
    ));
    lab.seed(&as_refs(&seed))?;
    succeeded(&lab.run(
        "cargo",
        &["generate-lockfile", "--offline"],
        &lab.dir("work"),
    )?)?;
    lab.git("work", &["add", "Cargo.lock"])?;
    lab.git("work", &["commit", "-q", "-s", "-m", "lock"])?;
    lab.git("work", &["push", "-q", "origin", "main"])?;
    let add = |lab: &Lab, repo: &str, name: &str| -> TestResult {
        lab.write(repo, &as_refs(&crate_files(name)))?;
        succeeded(&lab.run("cargo", &["generate-lockfile", "--offline"], &lab.dir(repo))?)?;
        Ok(())
    };
    lab.git("work", &["checkout", "-q", "-b", "lane", "main"])?;
    add(&lab, "work", "b")?;
    lab.git("work", &["add", "."])?;
    lab.git("work", &["commit", "-q", "-s", "-m", "T1: add crate b"])?;
    let sha = lab.git_out("work", &["rev-parse", "HEAD"])?;
    lab.git("work", &["checkout", "-q", "main"])?;
    lab.clone_origin("other")?;
    add(&lab, "other", "c")?;
    lab.git("other", &["add", "."])?;
    lab.git("other", &["commit", "-q", "-s", "-m", "other: add crate c"])?;
    lab.git("other", &["push", "-q", "origin", "main"])?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    let text = succeeded(&output)?;
    assert!(text.contains("landed"), "{text}");
    assert!(
        text.contains("+name = \"c\""),
        "the lock diff is not in the log:\n{text}"
    );
    lab.git("work", &["fetch", "-q", "origin"])?;
    let lock = lab.git_out("work", &["show", "origin/main:Cargo.lock"])?;
    for name in ["a", "b", "c"] {
        assert!(lock.contains(&format!("name = \"{name}\"")), "{lock}");
    }
    assert!(!lock.contains("<<<<<<<"), "{lock}");
    Ok(())
}

fn adr_file(number: u32, title: &str, status: &str) -> String {
    format!(
        "# {number:04}. {title}\n\nDate: 2026-11-17\n\n## Status\n\n{status}\n\n## Context\n\nWhy.\n\n\
         ## Decision\n\nWhat.\n\n## Consequences\n\nSo.\n"
    )
}

fn adr_row(number: u32, file_slug: &str, title: &str, status: &str) -> String {
    format!("| [{number:04}]({number:04}-{file_slug}.md) | {title} | {status} |\n")
}

const INDEX_HEAD: &str =
    "# Architecture decision records\n\nProse.\n\n| ADR | Title | Status |\n|---|---|---|\n";
const INDEX_TAIL: &str = "\nMore prose.\n";

fn adr_files(rows: &[(u32, &str, &str, &str)]) -> Vec<(String, String)> {
    let mut files = vec![(
        "doc/adr/README.md".to_owned(),
        format!(
            "{INDEX_HEAD}{}{INDEX_TAIL}",
            rows.iter()
                .map(|(n, slug, title, status)| adr_row(*n, slug, title, status))
                .collect::<String>()
        ),
    )];
    for (n, slug, title, status) in rows {
        files.push((
            format!("doc/adr/{n:04}-{slug}.md"),
            adr_file(*n, title, status),
        ));
    }
    files
}

fn as_refs(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect()
}

#[test]
fn should_keep_both_index_rows_in_order_when_two_lanes_append_an_adr() -> TestResult {
    // given: both sides append a row after row 2, and their own record file
    let lab = Lab::new("land-index")?;
    let base = adr_files(&[(1, "one", "One", "Proposed"), (2, "two", "Two", "Proposed")]);
    lab.seed(&as_refs(&base))?;
    let lane = adr_files(&[
        (1, "one", "One", "Proposed"),
        (2, "two", "Two", "Proposed"),
        (4, "four", "Four", "Proposed"),
    ]);
    let sha = lab.lane_commit("lane", &as_refs(&lane), "T1: adr four")?;
    let main = adr_files(&[
        (1, "one", "One", "Proposed"),
        (2, "two", "Two", "Proposed"),
        (3, "three", "Three", "Proposed"),
    ]);
    lab.origin_commit(&as_refs(&main), "other: adr three")?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    let text = succeeded(&output)?;
    assert!(text.contains("landed"), "{text}");
    lab.git("work", &["fetch", "-q", "origin"])?;
    let index = lab.git_out("work", &["show", "origin/main:doc/adr/README.md"])?;
    let rows: Vec<&str> = index.lines().filter(|l| l.starts_with("| [")).collect();
    assert_eq!(rows.len(), 4, "{index}");
    for (row, number) in rows.iter().zip(["0001", "0002", "0003", "0004"]) {
        assert!(row.starts_with(&format!("| [{number}]")), "{index}");
    }
    assert!(index.contains("More prose."), "{index}");
    Ok(())
}

#[test]
fn should_abort_when_private_names_is_in_check_but_its_hash_file_is_missing() -> TestResult {
    // given: the tree has the private-names stage in `check` but no hash file
    let lab = Lab::new("land-private-missing")?;
    lab.seed(&[
        ("recipes.txt", "check private-names\n"),
        ("stages.txt", "fmt private-names\n"),
    ])?;
    let sha = lab.lane_commit("lane", &[("b.txt", "b\n")], "T1: add b")?;
    let before = lab.origin_head()?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains(".private-names.sha256"), "{text}");
    assert_eq!(lab.origin_head()?, before);
    Ok(())
}

#[test]
fn should_land_when_the_private_names_hash_file_has_an_entry() -> TestResult {
    // given
    let lab = Lab::new("land-private-present")?;
    lab.seed(&[
        ("recipes.txt", "check private-names\n"),
        ("stages.txt", "fmt private-names\n"),
        (".private-names.sha256", "# salt: 00\n0123456789abcdef\n"),
    ])?;
    let sha = lab.lane_commit("lane", &[("b.txt", "b\n")], "T1: add b")?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    let text = succeeded(&output)?;
    assert!(text.contains("landed"), "{text}");
    Ok(())
}

#[test]
fn should_abort_when_private_names_is_a_recipe_but_not_a_stage_of_check() -> TestResult {
    // given
    let lab = Lab::new("land-private-stage")?;
    lab.seed(&[
        ("recipes.txt", "check private-names\n"),
        ("stages.txt", "fmt\n"),
        (".private-names.sha256", "# salt: 00\n0123456789abcdef\n"),
    ])?;
    let sha = lab.lane_commit("lane", &[("b.txt", "b\n")], "T1: add b")?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("does not run it"), "{text}");
    Ok(())
}

#[test]
fn should_redo_the_cherry_pick_and_the_gate_once_when_the_push_loses_a_race() -> TestResult {
    // given: the first gate run pushes another commit to origin, so the push is not a fast-forward
    let lab = Lab::new("land-race")?;
    lab.seed(&[])?;
    lab.clone_origin("other")?;
    lab.write("other", &[("raced.txt", "r\n")])?;
    lab.git("other", &["add", "."])?;
    lab.git("other", &["commit", "-q", "-s", "-m", "other: raced"])?;
    let mark = lab.base.join("raced.mark");
    let gate = format!(
        "if [ ! -e '{mark}' ]; then touch '{mark}'; git -C '{other}' push -q origin HEAD:main; fi\nexit 0\n",
        mark = mark.display(),
        other = lab.dir("other").display()
    );
    let sha = lab.lane_commit("lane", &[("gate.sh", &gate)], "T1: gate")?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    let text = succeeded(&output)?;
    assert!(text.contains("redoing"), "{text}");
    lab.git("work", &["fetch", "-q", "origin"])?;
    assert_eq!(lab.origin_subjects()?, "T1: gate\nother: raced\ninit");
    Ok(())
}

fn commit_as(lab: &Lab, author: &str, message: &str, file: &str) -> TestResult {
    lab.write("work", &[(file, "x\n")])?;
    lab.git("work", &["add", "."])?;
    lab.git("work", &["commit", "-q", "--author", author, "-m", message])?;
    Ok(())
}

fn dco(lab: &Lab, range: &str) -> Result<Output, Box<dyn Error>> {
    Ok(lab
        .command("bash", &lab.dir("work"))
        .arg(script("dco-check.sh"))
        .arg(range)
        .output()?)
}

#[test]
fn should_fail_dco_check_when_a_commit_lacks_a_matching_sign_off() -> TestResult {
    // given: no sign-off; a sign-off by someone else; a sign-off quoted above the real trailer block
    let lab = Lab::new("dco-fail")?;
    lab.seed(&[])?;
    commit_as(&lab, "Ann Author <ann@example.test>", "none", "1.txt")?;
    commit_as(
        &lab,
        "Ann Author <ann@example.test>",
        "other\n\nSigned-off-by: Bob <bob@example.test>",
        "2.txt",
    )?;
    commit_as(
        &lab,
        "Ann Author <ann@example.test>",
        "quoted\n\nSigned-off-by: Ann Author <ann@example.test>\n\nnot a trailer block any more",
        "3.txt",
    )?;
    commit_as(
        &lab,
        "Ann Author <ann@example.test>",
        "fine\n\nSigned-off-by: Ann Author <ann@example.test>",
        "4.txt",
    )?;

    // when
    let output = dco(&lab, "origin/main..HEAD")?;

    // then
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    for subject in ["none", "other", "quoted"] {
        assert!(text.contains(&format!(" {subject} has no")), "{text}");
    }
    assert!(!text.contains(" fine "), "{text}");
    Ok(())
}

#[test]
fn should_pass_dco_check_when_every_commit_is_signed_off_by_its_author() -> TestResult {
    // given
    let lab = Lab::new("dco-pass")?;
    lab.seed(&[])?;
    commit_as(
        &lab,
        "Bob <bob@example.test>",
        "T1: by bob\n\nBody.\n\nSigned-off-by: Bob <bob@example.test>",
        "1.txt",
    )?;
    commit_as(
        &lab,
        "Ann Author <ann@example.test>",
        "T2: by ann\n\nSigned-off-by: Ann Author <ann@example.test>\nCo-Authored-By: C <c@example.test>",
        "2.txt",
    )?;

    // when
    let output = dco(&lab, "origin/main..HEAD")?;

    // then
    let text = succeeded(&output)?;
    assert!(text.contains("2 commit(s)"), "{text}");
    Ok(())
}

fn dco_event(lab: &Lab, args: [&str; 4]) -> Result<Output, Box<dyn Error>> {
    Ok(lab
        .command("bash", &lab.dir("work"))
        .arg(script("dco-check.sh"))
        .arg("--event")
        .args(args)
        .output()?)
}

#[test]
fn should_check_the_event_s_range_when_dco_check_runs_for_a_github_event() -> TestResult {
    // given: an unsigned commit, then a signed one on top
    let lab = Lab::new("dco-event")?;
    lab.seed(&[])?;
    let base = lab.git_out("work", &["rev-parse", "HEAD"])?;
    commit_as(&lab, "Ann Author <ann@example.test>", "unsigned", "1.txt")?;
    let middle = lab.git_out("work", &["rev-parse", "HEAD"])?;
    commit_as(
        &lab,
        "Ann Author <ann@example.test>",
        "signed\n\nSigned-off-by: Ann Author <ann@example.test>",
        "2.txt",
    )?;
    let head = lab.git_out("work", &["rev-parse", "HEAD"])?;
    let zero = "0000000000000000000000000000000000000000";

    // when
    let push_last = dco_event(&lab, ["push", &middle, "", &head])?;
    let push_all = dco_event(&lab, ["push", &base, "", &head])?;
    let first_push = dco_event(&lab, ["push", zero, "", &head])?;
    let pull_request = dco_event(&lab, ["pull_request", "", &base, &head])?;
    let dispatch = dco_event(&lab, ["workflow_dispatch", "", "", &head])?;

    // then
    assert!(succeeded(&push_last)?.contains("1 commit(s)"));
    assert!(!push_all.status.success(), "{}", printed(&push_all));
    assert!(!first_push.status.success(), "{}", printed(&first_push));
    assert!(printed(&first_push).contains("is not a commit here"));
    assert!(!pull_request.status.success(), "{}", printed(&pull_request));
    assert!(succeeded(&dispatch)?.contains("1 commit(s)"));
    Ok(())
}

fn adr_index(lab: &Lab, mode: &str, dir: &Path) -> Result<Output, Box<dyn Error>> {
    Ok(lab
        .command("bash", &lab.base)
        .arg(script("adr-index.sh"))
        .arg(mode)
        .arg(dir)
        .output()?)
}

#[test]
fn should_pass_the_adr_index_check_when_run_over_the_committed_records() -> TestResult {
    // given
    let lab = Lab::new("adr-index-real")?;

    // when
    let output = adr_index(&lab, "check", &repo_root().join("doc/adr"))?;

    // then
    succeeded(&output)?;
    Ok(())
}

#[test]
fn should_fail_the_adr_index_check_when_a_row_and_a_file_disagree() -> TestResult {
    // given: a consistent pair of records, then one disagreement at a time
    let lab = Lab::new("adr-index-cases")?;
    let rows = [
        (1, "one", "One", "Accepted (owner, 2026-11-16); amended"),
        (2, "two", "Two", "Superseded by 0003"),
        (3, "three", "Three", "Proposed"),
    ];
    let good = adr_files(&rows);
    let dir = lab.base.join("adr");
    let write = |files: &[(String, String)]| -> TestResult {
        if dir.try_exists()? {
            fs::remove_dir_all(&dir)?;
        }
        for (path, content) in files {
            let target = dir.join(path.trim_start_matches("doc/adr/"));
            fs::create_dir_all(dir.as_path())?;
            fs::write(target, content)?;
        }
        Ok(())
    };
    write(&good)?;
    succeeded(&adr_index(&lab, "check", &dir)?)?;

    let cases: [(&str, &str, &str, &str); 7] = [
        (
            "README.md",
            "| Proposed |\n",
            "| Accepted (owner, 2026-11-16) |\n",
            "status",
        ),
        ("README.md", "| Three |", "| Tres |", "title"),
        ("README.md", "(0003-three.md)", "(0003-trois.md)", "links"),
        (
            "README.md",
            "(owner, 2026-11-16); amended",
            "(owner, 2026-11-17); amended",
            "owner decision",
        ),
        (
            "README.md",
            "Superseded by 0003",
            "Superseded by 0009",
            "but the file is superseded by 0003",
        ),
        (
            "README.md",
            "| [0003](0003-three.md) | Three | Proposed |\n",
            "",
            "no index row for 0003-three.md",
        ),
        ("0004-extra.md", "", "", "no index row for 0004-extra.md"),
    ];

    for (file, from, to, expected) in cases {
        // when
        write(&good)?;
        if file == "README.md" {
            let path = dir.join(file);
            let text = fs::read_to_string(&path)?;
            assert!(text.contains(from), "case fixture: {from} not in {text}");
            fs::write(&path, text.replacen(from, to, 1))?;
        } else {
            fs::write(dir.join(file), adr_file(4, "Extra", "Proposed"))?;
        }
        let output = adr_index(&lab, "check", &dir)?;

        // then
        let text = printed(&output);
        assert!(!output.status.success(), "{expected}: passed:\n{text}");
        assert!(text.contains(expected), "{expected}: not in:\n{text}");
    }
    Ok(())
}

#[test]
fn should_sort_the_rows_and_drop_exact_duplicates_when_adr_index_sort_runs() -> TestResult {
    // given
    let lab = Lab::new("adr-index-sort")?;
    let dir = lab.base.join("adr");
    fs::create_dir_all(&dir)?;
    let row = |n: u32| adr_row(n, "x", "X", "Proposed");
    fs::write(
        dir.join("README.md"),
        format!(
            "{INDEX_HEAD}{}{}{}{}{INDEX_TAIL}",
            row(3),
            row(1),
            row(3),
            row(2)
        ),
    )?;

    // when
    succeeded(&adr_index(&lab, "sort", &dir)?)?;

    // then
    let sorted = fs::read_to_string(dir.join("README.md"))?;
    assert_eq!(
        sorted,
        format!("{INDEX_HEAD}{}{}{}{INDEX_TAIL}", row(1), row(2), row(3))
    );
    Ok(())
}

#[test]
fn should_create_the_next_record_from_the_template_when_adrs_new_runs_in_a_scratch_copy()
-> TestResult {
    // given: a scratch copy of adrs.toml and doc/adr, the committed records included
    let lab = Lab::new("adrs-new")?;
    let copy = lab.base.join("repo");
    fs::create_dir_all(copy.join("doc/adr"))?;
    fs::copy(repo_root().join("adrs.toml"), copy.join("adrs.toml"))?;
    let mut highest = 0u32;
    for entry in fs::read_dir(repo_root().join("doc/adr"))? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        fs::copy(&path, copy.join("doc/adr").join(name))?;
        if let Some(number) = name.get(..4).and_then(|n| n.parse::<u32>().ok()) {
            highest = highest.max(number);
        }
    }
    let next = highest.saturating_add(1);
    let which = Command::new("mise")
        .arg("which")
        .arg("adrs")
        .current_dir(repo_root())
        .output()?;
    let adrs = String::from_utf8_lossy(&succeeded_stdout(&which)?)
        .trim()
        .to_owned();

    // when
    let created = lab
        .command(&adrs, &copy)
        .args(["new", "Scratch decision"])
        .output()?;

    // then
    succeeded(&created)?;
    let file = copy.join(format!("doc/adr/{next:04}-scratch-decision.md"));
    let text = fs::read_to_string(&file)?;
    assert!(
        text.starts_with(&format!("# {next:04}. Scratch decision\n\nDate: 20")),
        "{text}"
    );
    assert!(
        text.contains("## Status\n\nProposed\n\n## Context"),
        "{text}"
    );
    let missing_row = adr_index(&lab, "check", &copy.join("doc/adr"))?;
    assert!(!missing_row.status.success(), "{}", printed(&missing_row));
    assert!(
        printed(&missing_row).contains("no index row for"),
        "{}",
        printed(&missing_row)
    );
    let readme = copy.join("doc/adr/README.md");
    let index = fs::read_to_string(&readme)?;
    let row = adr_row(next, "scratch-decision", "Scratch decision", "Proposed");
    let mut lines: Vec<&str> = index.lines().collect();
    let last = lines
        .iter()
        .rposition(|l| l.starts_with("| ["))
        .ok_or("no index row")?;
    lines.insert(last.saturating_add(1), row.trim_end());
    fs::write(&readme, format!("{}\n", lines.join("\n")))?;
    succeeded(&adr_index(&lab, "check", &copy.join("doc/adr"))?)?;
    Ok(())
}

fn succeeded_stdout(output: &Output) -> Result<Vec<u8>, Box<dyn Error>> {
    succeeded(output)?;
    Ok(output.stdout.clone())
}

fn assert_refused_and_clean(lab: &Lab, output: &Output, before: &str, needle: &str) -> TestResult {
    let text = printed(output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains(needle), "{needle} not in:\n{text}");
    assert_eq!(lab.origin_head()?, before);
    assert_eq!(
        lab.git_out("work", &["worktree", "list", "--porcelain"])?
            .matches("worktree ")
            .count(),
        1
    );
    Ok(())
}

#[test]
fn should_refuse_to_push_when_the_gate_changes_a_tracked_file() -> TestResult {
    // given: the lane's gate rewrites a.txt and still exits 0
    let lab = Lab::new("land-gate-mutates")?;
    lab.seed(&[])?;
    let before = lab.origin_head()?;
    let sha = lab.lane_commit(
        "lane",
        &[("gate.sh", "echo broken > a.txt\nexit 0\n")],
        "T1: gate",
    )?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    assert_refused_and_clean(&lab, &output, &before, "the gate changed the tree")?;
    assert!(printed(&output).contains("a.txt"), "{}", printed(&output));
    Ok(())
}

#[test]
fn should_abort_when_both_sides_change_prose_in_the_adr_index() -> TestResult {
    // given: the same prose line of the README edited differently on each side
    let lab = Lab::new("land-index-prose")?;
    let base = adr_files(&[(1, "one", "One", "Proposed")]);
    lab.seed(&as_refs(&base))?;
    let edit = |text: &str| -> Vec<(String, String)> {
        base.iter()
            .map(|(p, c)| (p.clone(), c.replace("Prose.", text)))
            .collect()
    };
    let sha = lab.lane_commit("lane", &as_refs(&edit("Lane prose.")), "T1: prose")?;
    lab.origin_commit(&as_refs(&edit("Main prose.")), "other: prose")?;
    let before = lab.origin_head()?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    assert_refused_and_clean(
        &lab,
        &output,
        &before,
        "doc/adr/README.md conflicts outside the index rows",
    )
}

#[test]
fn should_abort_cleanly_when_a_resolvable_file_is_deleted_on_one_side() -> TestResult {
    // given: integration deleted the README that the lane edits (a modify/delete conflict)
    let lab = Lab::new("land-modify-delete")?;
    let base = adr_files(&[(1, "one", "One", "Proposed")]);
    lab.seed(&as_refs(&base))?;
    let edited: Vec<(String, String)> = base
        .iter()
        .map(|(p, c)| (p.clone(), c.replace("Prose.", "Lane.")))
        .collect();
    let sha = lab.lane_commit("lane", &as_refs(&edited), "T1: edit")?;
    lab.origin_commit(&[], "other: nothing")
        .or_else(|_| -> TestResult { Ok(()) })?;
    lab.git("other", &["rm", "-q", "doc/adr/README.md"])?;
    lab.git(
        "other",
        &["commit", "-q", "-s", "-m", "other: delete index"],
    )?;
    lab.git("other", &["push", "-q", "origin", "main"])?;
    let before = lab.origin_head()?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    assert_refused_and_clean(&lab, &output, &before, "deleted on one side")
}

#[test]
fn should_refuse_a_commit_when_its_only_signature_is_a_line_of_the_message_body() -> TestResult {
    // given: an unsigned commit whose body has a line starting with gpgsig
    let lab = Lab::new("land-gpgsig-body")?;
    lab.seed(&[])?;
    let before = lab.origin_head()?;
    let sha = lab.lane_commit(
        "lane",
        &[("b.txt", "b\n")],
        "T1: add b\n\ngpgsig -----BEGIN fake",
    )?;

    // when
    let output = lab.land(&[&sha])?;

    // then
    assert_refused_and_clean(&lab, &output, &before, "not signed")
}

#[test]
fn should_refuse_a_commit_when_it_is_not_authored_by_the_owner_identity() -> TestResult {
    // given: Ann authored and signed off the commit, but the default owner is another identity
    let lab = Lab::new("land-author")?;
    lab.seed(&[])?;
    let before = lab.origin_head()?;
    let sha = lab.lane_commit("lane", &[("b.txt", "b\n")], "T1: add b")?;

    // when
    let output = lab.land_raw(&["--allow-unsigned", &sha])?;

    // then
    assert_refused_and_clean(&lab, &output, &before, "not the owner identity")
}

#[test]
fn should_report_git_s_reason_when_a_cherry_pick_fails_without_a_conflict() -> TestResult {
    // given: a merge commit, which cherry-pick refuses without -m
    let lab = Lab::new("land-merge-commit")?;
    lab.seed(&[])?;
    let before = lab.origin_head()?;
    lab.lane_commit("one", &[("b.txt", "b\n")], "T1: b")?;
    lab.git("work", &["checkout", "-q", "-b", "two", "main"])?;
    lab.git("work", &["merge", "-q", "--no-ff", "-m", "merge", "one"])?;
    let sha = lab.git_out("work", &["rev-parse", "HEAD"])?;
    lab.git("work", &["checkout", "-q", "main"])?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then
    assert_refused_and_clean(&lab, &output, &before, "is a merge")
}

#[test]
fn should_sort_every_row_when_rows_are_split_by_a_line_of_prose() -> TestResult {
    // given
    let lab = Lab::new("adr-index-sort-split")?;
    let dir = lab.base.join("adr");
    fs::create_dir_all(&dir)?;
    let row = |n: u32| adr_row(n, "x", "X", "Proposed");
    fs::write(
        dir.join("README.md"),
        format!(
            "{INDEX_HEAD}{}{}\nprose between\n\n{}{INDEX_TAIL}",
            row(3),
            row(1),
            row(2)
        ),
    )?;

    // when
    succeeded(&adr_index(&lab, "sort", &dir)?)?;

    // then: nothing dropped, all rows at the first row's place
    let sorted = fs::read_to_string(dir.join("README.md"))?;
    assert_eq!(
        sorted,
        format!(
            "{INDEX_HEAD}{}{}{}\nprose between\n\n{INDEX_TAIL}",
            row(1),
            row(2),
            row(3)
        )
    );
    Ok(())
}

#[test]
fn should_fail_the_adr_index_check_when_a_status_claims_more_than_the_files_support() -> TestResult
{
    let lab = Lab::new("adr-index-status")?;
    let dir = lab.base.join("adr");
    let cases: [(&str, &str, &str); 3] = [
        ("Accepted", "Accepted", "without \"(owner, DATE)\""),
        (
            "Superseded by 0003",
            "Superseded",
            "but the file is superseded by 0003",
        ),
        (
            "Superseded by 0099",
            "Superseded by 0099",
            "no such ADR exists",
        ),
    ];
    for (file_status, row_status, expected) in cases {
        // given: file and row agree except where the case says otherwise
        if dir.try_exists()? {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)?;
        let rows = [
            (1, "one", "One", file_status),
            (3, "three", "Three", "Proposed"),
        ];
        for (path, content) in adr_files(&rows) {
            fs::write(dir.join(path.trim_start_matches("doc/adr/")), content)?;
        }
        let readme = dir.join("README.md");
        let text = fs::read_to_string(&readme)?;
        fs::write(
            &readme,
            text.replacen(
                &format!("| {file_status} |"),
                &format!("| {row_status} |"),
                1,
            ),
        )?;

        // when
        let output = adr_index(&lab, "check", &dir)?;

        // then
        let text = printed(&output);
        assert!(!output.status.success(), "{file_status}: passed:\n{text}");
        assert!(text.contains(expected), "{expected} not in:\n{text}");
    }
    Ok(())
}

#[test]
fn should_refuse_before_any_just_when_the_merged_tree_s_mise_toml_has_a_payload() -> TestResult {
    // given: the FND-08 re-verification's F3, a lane commit whose mise.toml sources a script
    // through [env]; land.sh used to run `just` there with the tree trusted for mise, so the
    // justfile's `mise bin-paths` (or a mise shim of just) ran it before the gate could refuse
    let lab = Lab::new("land-mise-payload")?;
    lab.seed(&[])?;
    let before = lab.origin_head()?;
    let ran = lab.dir("ran-gate");
    let sha = lab.lane_commit(
        "lane",
        &[
            (
                "mise.toml",
                "[tools]\njust = \"1.59.0\"\n\n[env]\n_.source = \"{{config_root}}/payload.sh\"\n",
            ),
            ("payload.sh", "touch ran-payload\n"),
            ("gate.sh", &format!("touch '{}'\nexit 0\n", ran.display())),
        ],
        "T1: tools",
    )?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;

    // then: refused by the landing checkout's own check, before any `just` (the stub's gate.sh)
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("tool configuration"), "{text}");
    assert!(text.contains("mise.toml"), "{text}");
    assert!(
        !ran.try_exists()?,
        "the gate ran on the merged tree:\n{text}"
    );
    assert_eq!(lab.origin_head()?, before);
    Ok(())
}

#[test]
fn should_refuse_before_cargo_runs_when_a_cherry_pick_brings_a_cargo_config_and_a_lock_conflict()
-> TestResult {
    // given: the FND-09 review's G2, a lane commit adding `.cargo/config.toml` with a
    // rustc-wrapper while both sides change Cargo.lock, so land.sh would resolve the conflict
    // with `cargo update`, which runs the wrapper, before any check of the tree
    let lab = Lab::new("land-cargo-config")?;
    let mut seed = crate_files("a");
    seed.push((
        "Cargo.toml".to_owned(),
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n".to_owned(),
    ));
    lab.seed(&as_refs(&seed))?;
    succeeded(&lab.run(
        "cargo",
        &["generate-lockfile", "--offline"],
        &lab.dir("work"),
    )?)?;
    lab.git("work", &["add", "Cargo.lock"])?;
    lab.git("work", &["commit", "-q", "-s", "-m", "lock"])?;
    lab.git("work", &["push", "-q", "origin", "main"])?;
    let add = |lab: &Lab, repo: &str, name: &str| -> TestResult {
        lab.write(repo, &as_refs(&crate_files(name)))?;
        succeeded(&lab.run("cargo", &["generate-lockfile", "--offline"], &lab.dir(repo))?)?;
        Ok(())
    };
    let marker = lab.dir("wrapper-ran");
    let evil = lab.dir("evil.sh");
    fs::write(
        &evil,
        format!(
            "#!/usr/bin/env bash\ntouch '{}'\nexec \"$@\"\n",
            marker.display()
        ),
    )?;
    succeeded(&lab.run("chmod", &["+x", "evil.sh"], &lab.base)?)?;
    lab.git("work", &["checkout", "-q", "-b", "lane", "main"])?;
    add(&lab, "work", "b")?;
    let config = format!("[build]\nrustc-wrapper = \"{}\"\n", evil.display());
    lab.write("work", &[(".cargo/config.toml", &config)])?;
    lab.git("work", &["add", "."])?;
    lab.git("work", &["commit", "-q", "-s", "-m", "T1: add crate b"])?;
    let sha = lab.git_out("work", &["rev-parse", "HEAD"])?;
    lab.git("work", &["checkout", "-q", "main"])?;
    lab.clone_origin("other")?;
    add(&lab, "other", "c")?;
    lab.git("other", &["add", "."])?;
    lab.git("other", &["commit", "-q", "-s", "-m", "other: add crate c"])?;
    lab.git("other", &["push", "-q", "origin", "main"])?;
    let before = lab.origin_head()?;

    // when
    let output = lab.land(&["--allow-unsigned", &sha])?;
    let ran_through_land = marker.try_exists()?;
    lab.git("work", &["checkout", "-q", "lane"])?;
    let control = lab.run(
        "cargo",
        &["update", "--workspace", "--offline"],
        &lab.dir("work"),
    )?;

    // then: refused before cargo ran; cargo itself, the control, runs the wrapper
    let text = printed(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains(".cargo/config.toml"), "{text}");
    assert!(!ran_through_land, "the wrapper ran during land.sh:\n{text}");
    assert_eq!(lab.origin_head()?, before);
    assert!(
        marker.try_exists()?,
        "the control did not run the wrapper, so the case proves nothing:\n{}",
        printed(&control)
    );
    Ok(())
}
