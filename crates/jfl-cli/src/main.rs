//! The `jfl` binary: `jfl daemon` runs the daemon (normally spawned on demand by a proxy, plan
//! §5.2); without arguments, `jfl` prints its version. The other subcommands arrive with IDX-13.

use std::ffi::OsString;
use std::process::ExitCode;

use jfl_composition::daemon::{self, DaemonConfig};

const USAGE: &str =
    "usage: jfl [daemon [--state-dir DIR] [--notify PATH] [--idle-timeout SECONDS]]";

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.split_first() {
        None => {
            println!("jfl {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some((command, rest)) if command == "daemon" => match DaemonConfig::from_args(rest) {
            Ok(config) => match daemon::serve(&config) {
                Ok(_) => ExitCode::SUCCESS,
                Err(why) => {
                    eprintln!("jfl daemon: {why}");
                    ExitCode::from(1)
                }
            },
            Err(why) => {
                eprintln!("jfl daemon: {why}\n{USAGE}");
                ExitCode::from(2)
            }
        },
        Some(_) => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
