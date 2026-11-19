//! `jfl daemon` for this crate's own tests (`tests/daemon.rs`): the same entry the `jfl` binary
//! calls, `daemon [--state-dir DIR] [--notify PATH] [--idle-timeout SECONDS]`. A bin of this crate
//! because only a crate's own tests get a bin's path (`CARGO_BIN_EXE_*`), and composition's tests
//! cannot depend on `jfl-cli` (ARCH-forbidden-dev-edge); no release artefact ships it.

use std::ffi::OsString;
use std::process::ExitCode;

use jfl_composition::daemon::{self, DaemonConfig};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.split_first() {
        Some((command, rest)) if command == "daemon" => match DaemonConfig::from_args(rest) {
            Ok(config) => match daemon::serve(&config) {
                Ok(_) => ExitCode::SUCCESS,
                Err(why) => {
                    eprintln!("{why}");
                    ExitCode::from(1)
                }
            },
            Err(why) => {
                eprintln!("{why}");
                ExitCode::from(2)
            }
        },
        _ => ExitCode::from(2),
    }
}
