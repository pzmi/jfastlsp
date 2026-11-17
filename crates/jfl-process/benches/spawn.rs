//! Spawn overhead of the one-shot mode against `std::process::Command::output` on the same
//! child: what the git adapter and the javac pool pay per child (`just bench-process`). Reports
//! the median and p95 wall time of each arm; never gates. Given a repository checkout as its
//! argument (`just bench-process small`: the small bench corpus), it also times the two `git`
//! calls of the git adapter's reconcile path (ADR 0024) there, through both runners.

use std::error::Error;
use std::process::Stdio;
use std::time::{Duration, Instant};

use jfl_process::{Command, Ending, Exit, Runner};

const CHILD: &str = env!("CARGO_BIN_EXE_jfl-process-test-child");
const ROUNDS: usize = 300;
const WARMUP: usize = 20;

fn main() -> Result<(), Box<dyn Error>> {
    let runner = Runner::new();
    let deadline = Duration::from_secs(10);
    let exit0 = Command::new(CHILD).args(["exit", "0"]);
    let write64k = Command::new(CHILD).args(["write", "65536", "0"]);

    report("jfl-process run (exit 0)", || {
        let outcome = runner.run(&exit0, deadline)?;
        expect_success(outcome.ending)
    })?;
    report("std Command::output (exit 0)", || {
        let output = std::process::Command::new(CHILD)
            .args(["exit", "0"])
            .env_clear()
            .stdin(Stdio::null())
            .output()?;
        expect(output.status.success())
    })?;
    report("jfl-process run (64 KiB stdout)", || {
        let outcome = runner.run(&write64k, deadline)?;
        expect(outcome.stdout.bytes().len() == 65536)?;
        expect_success(outcome.ending)
    })?;
    report("std Command::output (64 KiB stdout)", || {
        let output = std::process::Command::new(CHILD)
            .args(["write", "65536", "0"])
            .env_clear()
            .stdin(Stdio::null())
            .output()?;
        expect(output.stdout.len() == 65536)
    })?;
    if let Some(repo) = std::env::args_os()
        .skip(1)
        .find(|arg| !arg.as_encoded_bytes().starts_with(b"--"))
    {
        for args in [
            &["rev-parse", "HEAD"][..],
            &["status", "-z", "--porcelain=v2", "--untracked-files=no"][..],
        ] {
            let git = Command::new("git")
                .inherit_env("PATH")
                .inherit_env("HOME")
                .args(args)
                .current_dir(&repo);
            report(&format!("jfl-process run (git {})", args.join(" ")), || {
                let outcome = runner.run(&git, deadline)?;
                expect_success(outcome.ending)
            })?;
            report(
                &format!("std Command::output (git {})", args.join(" ")),
                || {
                    let mut command = std::process::Command::new("git");
                    command
                        .env_clear()
                        .args(args)
                        .current_dir(&repo)
                        .stdin(Stdio::null());
                    for name in ["PATH", "HOME"] {
                        if let Some(value) = std::env::var_os(name) {
                            command.env(name, value);
                        }
                    }
                    expect(command.output()?.status.success())
                },
            )?;
        }
    }
    Ok(())
}

fn expect_success(ending: Ending) -> Result<(), Box<dyn Error>> {
    expect(ending == Ending::Exited(Exit::Code(0)))
}

fn expect(holds: bool) -> Result<(), Box<dyn Error>> {
    if holds {
        Ok(())
    } else {
        Err("the child did not do what the bench expects".into())
    }
}

fn report(
    name: &str,
    mut once: impl FnMut() -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    for _ in 0..WARMUP {
        once()?;
    }
    let mut samples = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        once()?;
        samples.push(start.elapsed());
    }
    samples.sort();
    let at = |q: usize| {
        samples
            .get(samples.len().saturating_mul(q) / 100)
            .copied()
            .unwrap_or_default()
    };
    println!(
        "{name:<40} median {:>8.1} µs   p95 {:>8.1} µs   ({ROUNDS} rounds)",
        at(50).as_secs_f64() * 1e6,
        at(95).as_secs_f64() * 1e6,
    );
    Ok(())
}
