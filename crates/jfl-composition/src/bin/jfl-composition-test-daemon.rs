//! `jfl daemon` for this crate's own tests (`tests/daemon.rs`): the same entry the `jfl` binary
//! calls, `daemon [--state-dir DIR] [--notify PATH] [--idle-timeout SECONDS]`. A bin of this crate
//! because only a crate's own tests get a bin's path (`CARGO_BIN_EXE_*`), and composition's tests
//! cannot depend on `jfl-cli` (ARCH-forbidden-dev-edge); no release artefact ships it.
//!
//! One test hook, here only, never in `jfl`: while `<state>/test-hold-settle` exists, the views'
//! background work (the settle's slices, the write-back) waits, so a test can put a request
//! between a view's open and its settle every time (`tests/daemon.rs::should_leave_the_deferred_files_to_the_slices_when_a_commit_in_a_linked_worktree_comes_during_the_settle`).
//! The run loop asks for background work again after every frame, so a request after the marker
//! is removed starts it.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use jfl_adapter_ipc::Call;
use jfl_composition::daemon::service::{Context, Service, Working};
use jfl_composition::daemon::views::Views;
use jfl_composition::daemon::{DaemonConfig, ports, run};

/// The marker that holds the background work, in the state directory.
const HOLD_SETTLE: &str = "test-hold-settle";

/// [`Views`], its background work held while the marker exists.
struct Held {
    views: Views,
    marker: PathBuf,
}

impl Service for Held {
    fn answer<'a>(&'a mut self, call: &'a Call, context: &'a Context<'a>) -> Working<'a, Vec<u8>> {
        self.views.answer(call, context)
    }

    fn background(&mut self) -> Option<Working<'_, ()>> {
        if self.marker.exists() {
            return None;
        }
        self.views.background()
    }
}

/// `daemon::serve` with the hook.
fn serve(config: &DaemonConfig) -> Result<run::Stopped, String> {
    run::run(config, |state| {
        let pool = ports::TokioPool::current()
            .ok_or_else(|| "the views need the daemon's runtime".to_owned())?;
        let views = Views::open(state.path(), &pool, &format!("jfl{}", std::process::id()))?;
        Ok(Held {
            views,
            marker: state.path().join(HOLD_SETTLE),
        })
    })
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.split_first() {
        Some((command, rest)) if command == "daemon" => match DaemonConfig::from_args(rest) {
            Ok(config) => match serve(&config) {
                Ok(stopped) => {
                    // The daemon's log is its stderr; a WARN event, by its fixed name (fnd §5).
                    if let Some(warning) = stopped.warning {
                        eprintln!("WARN {}", warning.event());
                    }
                    ExitCode::SUCCESS
                }
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
