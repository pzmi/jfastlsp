//! The `jfl-bench` binary: see `doc/bench/README.md`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    match jfl_bench::cli::run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("jfl-bench: {e}");
            ExitCode::FAILURE
        }
    }
}
