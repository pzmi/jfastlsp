//! The RSS sampler and the command line, against this crate's own binary as the child.

use std::error::Error;
use std::path::{Path, PathBuf};

use jfl_bench::sampler;

const BIN: &str = env!("CARGO_BIN_EXE_jfl-bench");

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("jfl-bench-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A tree whose single-threaded parse takes long enough (hundreds of files) that the sampler's
/// first read finds the child alive.
fn big_tree(dir: &Dir) -> Result<PathBuf, Box<dyn Error>> {
    let root = dir.path().join("tree");
    std::fs::create_dir_all(&root)?;
    for i in 0..600 {
        let body = format!("package s;\nclass C{i} {{ int a; void m() {{ int b = {i}; }} }}\n");
        std::fs::write(root.join(format!("C{i}.java")), body)?;
    }
    Ok(root)
}

#[test]
fn should_capture_stdout_and_the_exit_code_when_the_child_succeeds() {
    // given
    let dir = Dir::new("sampler-ok").unwrap();
    let root = big_tree(&dir).unwrap();
    let argv = [
        BIN.into(),
        "parse".into(),
        root.into_os_string(),
        "--upto".into(),
        "read".into(),
    ];
    // when
    let s = sampler::run(&argv, &dir.path().join("out.txt")).unwrap();
    // then
    assert_eq!(s.exit_code, Some(0));
    assert!(s.stdout.contains("\"files\":600"), "{}", s.stdout);
    assert!(s.wall_ms > 0.0);
}

#[cfg(target_os = "linux")]
#[test]
fn should_report_peak_at_least_steady_when_the_child_runs_long_enough_to_sample() {
    // given
    let dir = Dir::new("sampler-rss").unwrap();
    let root = big_tree(&dir).unwrap();
    let argv = [
        BIN.into(),
        "parse".into(),
        root.into_os_string(),
        "--threads".into(),
        "1".into(),
    ];
    // when
    let s = sampler::run(&argv, &dir.path().join("out.txt")).unwrap();
    // then
    assert!(s.samples > 0, "no sample of a child that parsed 600 files");
    let (peak, steady) = (s.peak_kib.unwrap(), s.steady_kib.unwrap());
    assert!(peak >= steady && steady > 0, "peak {peak} steady {steady}");
}

#[test]
fn should_fail_when_the_command_does_not_exist() {
    // given
    let dir = Dir::new("sampler-missing").unwrap();
    let argv = ["/nonexistent/jfl-bench-no-such-program".into()];
    // when
    let err = sampler::run(&argv, &dir.path().join("out.txt")).unwrap_err();
    // then
    assert!(err.to_string().contains("spawn"), "{err}");
}

#[test]
fn should_fail_when_the_command_is_empty() {
    // given
    let dir = Dir::new("sampler-empty").unwrap();
    // when
    let err = sampler::run(&[], &dir.path().join("out.txt")).unwrap_err();
    // then
    assert!(err.to_string().contains("empty command"), "{err}");
}
