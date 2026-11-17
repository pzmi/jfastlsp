//! The incremental suite on a synthetic two-commit repository in a tempdir (real `git`, real
//! files; nothing from the corpus): the changed set of a pair, the edit's diff, the widely used
//! types and their dependants, and the metric names every case writes.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use jfl_bench::e1::{self, Stage};
use jfl_bench::error::{BenchError, Result as BenchResult};
use jfl_bench::incremental::{self, Options, Pair};
use jfl_bench::runner::CacheDrop;

type Res<T> = Result<T, Box<dyn Error>>;

struct Repo(PathBuf);

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(root: &Path, args: &[&str]) -> Res<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=bench",
            "-c",
            "user.email=bench@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()?;
    if !out.status.success() {
        return Err(format!("git {args:?}: {}", String::from_utf8_lossy(&out.stderr)).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn write(root: &Path, rel: &str, text: &str) -> Res<()> {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().ok_or("no parent")?)?;
    Ok(std::fs::write(path, text)?)
}

const MONEY: &str = "package app.core;\n\npublic record Money(long minor) {\n    public Money plus(Money o) { return new Money(minor + o.minor); }\n}\n";
const LEDGER: &str =
    "package app.core;\n\npublic final class Ledger {\n    Money total = new Money(0);\n}\n";

/// Commit `old`: four files; then the pin: one changed, one deleted, one added.
fn repo(name: &str) -> Res<(Repo, String)> {
    let dir = std::env::temp_dir().join(format!("jfl-bench-inc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let r = Repo(dir.clone());
    git(&dir, &["init", "-q"])?;
    write(&dir, "core/src/main/java/app/core/Money.java", MONEY)?;
    write(&dir, "core/src/main/java/app/core/Ledger.java", LEDGER)?;
    write(
        &dir,
        "api/src/main/java/app/api/Service.java",
        "package app.api;\n\nimport app.core.Money;\n\npublic interface Service {\n    Money balance();\n}\n",
    )?;
    write(
        &dir,
        "api/src/main/java/app/api/Old.java",
        "package app.api;\n\nclass Old {}\n",
    )?;
    write(&dir, "README.md", "not java\n")?;
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-q", "-m", "old"])?;
    let old = git(&dir, &["rev-parse", "HEAD"])?;
    write(
        &dir,
        "api/src/main/java/app/api/Service.java",
        "package app.api;\n\nimport app.core.Money;\n\npublic interface Service {\n    Money balance();\n    Money limit();\n}\n",
    )?;
    std::fs::remove_file(dir.join("api/src/main/java/app/api/Old.java"))?;
    write(
        &dir,
        "app/src/main/java/app/main/Main.java",
        "package app.main;\n\nimport app.core.*;\nimport app.api.Service;\n\nclass Main {\n    Money m;\n    Service s;\n}\n",
    )?;
    write(
        &dir,
        "app/src/main/java/app/main/Other.java",
        "package app.main;\n\nimport app.core.*;\n\nclass Other {\n    Ledger l;\n}\n",
    )?;
    git(&dir, &["add", "-A"])?;
    git(&dir, &["commit", "-q", "-m", "pin"])?;
    Ok((r, old))
}

struct NoDrop;

impl CacheDrop for NoDrop {
    fn drop_caches(&self) -> BenchResult<()> {
        Err(BenchError::new("not permitted in a test"))
    }
}

fn names(paths: &[PathBuf], root: &Path) -> Vec<String> {
    let mut out: Vec<String> = paths
        .iter()
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|p| p.display().to_string())
        .collect();
    out.sort();
    out
}

#[test]
fn should_list_changed_files_present_at_the_pin_and_count_deletions_when_diffing_a_pair() -> Res<()>
{
    // given
    let (repo, old) = repo("pair")?;
    // when
    let changed = incremental::changed_java(&repo.0, &old)?;
    let batch = e1::index_files(&changed.present, 2, Stage::Extract)?;
    // then
    assert_eq!(
        names(&changed.present, &repo.0),
        [
            "api/src/main/java/app/api/Service.java",
            "app/src/main/java/app/main/Main.java",
            "app/src/main/java/app/main/Other.java",
        ]
    );
    assert_eq!(changed.deleted, 1);
    assert_eq!(batch.files, 3);
    assert_eq!(batch.failed, 0);
    assert!(batch.decls > 0);
    Ok(())
}

#[test]
fn should_fail_naming_the_fetch_when_the_pair_commit_is_absent_or_not_a_commit_id() -> Res<()> {
    // given
    let (repo, _) = repo("absent")?;
    // when
    let err = incremental::changed_java(&repo.0, "0123456789012345678901234567890123456789")
        .expect_err("an absent commit must fail");
    let option = incremental::changed_java(&repo.0, "--output=/tmp/x")
        .expect_err("an option-like commit must be refused");
    // then
    assert!(err.to_string().contains("fetch-pairs"), "{err}");
    assert!(
        option.to_string().contains("not a full commit id"),
        "{option}"
    );
    Ok(())
}

#[test]
fn should_rank_by_importers_and_find_wildcard_and_same_package_dependants_when_picking_widely_used_types()
-> Res<()> {
    // given
    let (repo, _) = repo("fanout")?;
    // when
    let picked = incremental::widely_used(&repo.0, 3)?;
    // then: Money is imported by name once (Service) and named by Main through `app.core.*` and
    // by Ledger in its own package; Service is imported by Main. JDK types are never picked.
    let fqns: Vec<&str> = picked.iter().map(|w| w.fqn.as_str()).collect();
    assert_eq!(fqns, ["app.api.Service", "app.core.Money"]);
    let money = picked.get(1).ok_or("no Money")?;
    assert_eq!(money.importers, 1);
    assert_eq!(
        names(&money.dependants, &repo.0),
        [
            "api/src/main/java/app/api/Service.java",
            "app/src/main/java/app/main/Main.java",
            "core/src/main/java/app/core/Ledger.java",
        ]
    );
    assert_eq!(money.mentions, 4);
    Ok(())
}

#[test]
fn should_diff_every_edit_as_one_added_declaration_when_editing_files() -> Res<()> {
    // given
    let (repo, _) = repo("edit")?;
    // when
    let e = incremental::edits(&repo.0, 3, 7, 1, 2)?;
    // then
    assert_eq!(e.files, 3);
    assert_eq!(e.samples.len(), 6);
    assert_eq!(e.unexpected, 0);
    assert!(e.samples.iter().all(|s| s.reparse_ms > 0.0));
    let status = git(&repo.0, &["status", "--porcelain"])?;
    assert_eq!(status, "", "the edit must stay in memory");
    Ok(())
}

#[test]
fn should_write_proxy_series_for_every_case_and_note_a_missing_pair_when_measuring() -> Res<()> {
    // given
    let (repo, old) = repo("measure")?;
    let options = Options {
        threads: 2,
        warmup: 0,
        runs: 2,
        cold_runs: 1,
        seed: 1,
        edits: 2,
        fanout: 2,
        roles: vec!["rebase".to_string(), "switch".to_string()],
    };
    let pairs = [Pair {
        role: "rebase".to_string(),
        commit: old,
    }];
    let mut notes = Vec::new();
    // when
    let (metrics, slots) =
        incremental::measure(&repo.0, "tiny", &options, &pairs, &NoDrop, &mut notes)?;
    // then
    let has = |name: &str| metrics.iter().any(|m| m.name == name);
    for name in [
        "incremental/tiny/edit/proxy/total_ms/p50",
        "incremental/tiny/edit/proxy/reparse_ms/p95",
        "incremental/tiny/rebase/proxy/diff_ms",
        "incremental/tiny/rebase/proxy/t2/reparse_ms",
        "incremental/tiny/rebase/proxy/changed_files",
        "incremental/tiny/fanout/k00/proxy/dependants",
        "incremental/tiny/fanout/k01/proxy/t2/reparse_ms",
        "incremental/tiny/reopen/proxy/t2/rehash_ms",
        "incremental/tiny/reopen/proxy/stat_ms",
    ] {
        assert!(has(name), "missing {name}");
    }
    assert!(metrics.iter().all(|m| m.name.contains("/proxy/")));
    assert!(!metrics.iter().any(|m| m.name.contains("/switch/")));
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("switch: no pinned commit pair"))
    );
    assert!(notes.iter().any(|n| n.contains("reopen cold runs skipped")));
    assert_eq!(slots.len(), 2);
    assert!(slots.iter().all(|s| !s.name_blake3.contains("Money")));
    Ok(())
}
