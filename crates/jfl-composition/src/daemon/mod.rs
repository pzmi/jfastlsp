//! The daemon and its proxies (plan §5.1–§5.3; ADR 0016, ADR 0021): one daemon per state
//! directory, holding its lock for life and serving a Unix socket in it; thin proxies that
//! connect, spawn it on demand and reconnect.
//!
//! - [`host`]: the daemon's state as steps, one per action of `formal/daemon.qnt`;
//!   [`stepped`] runs them one at a time for the conformance driver.
//! - [`run`]: the daemon process: `setsid`, the start order, the serve loop, the shutdown order.
//! - [`proxy`]: the client side: connect, the spawn lock and its second check, the spawn and the
//!   notify handshake, the version handshake, re-sent documents and request.
//! - [`service`]: the request seam the views (IDX-09) and the LSP proxy (LSP-02) plug into.
//!
//! **The state directory** is [`DaemonConfig::state_dir`], `--state-dir` or [`default_state_dir`];
//! [`run::run`] hands its service factory the checked [`StateDir`] once the lock is held, so the
//! summary store (and any other cache) opens under `StateDir::path()` only in the owner.

pub mod host;
pub mod proxy;
pub mod run;
pub mod service;
pub mod stepped;
mod timer;

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use jfl_adapter_ipc::StateDir;
use jfl_process::{Command, Detached, Runner};

use proxy::{Launcher, Spawned};

/// How long a daemon with no connection lives on.
pub const DEFAULT_IDLE: Duration = Duration::from_secs(30 * 60);

/// What `jfl daemon` runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    /// The state directory: the lock, the socket, the caches.
    pub state_dir: PathBuf,
    /// The proxy's notify socket, when a proxy spawned this daemon.
    pub notify: Option<PathBuf>,
    /// The idle period after which a daemon with no connection exits.
    pub idle: Duration,
}

impl DaemonConfig {
    /// The daemon's arguments after `daemon`: `[--state-dir DIR] [--notify PATH]
    /// [--idle-timeout SECONDS]`, the argv [`ProcessLauncher`] passes. Without `--state-dir`,
    /// [`default_state_dir`].
    pub fn from_args(args: &[OsString]) -> Result<Self, String> {
        let mut state_dir = None;
        let mut notify = None;
        let mut idle = DEFAULT_IDLE;
        let mut rest = args;
        while let [flag, value, tail @ ..] = rest {
            match flag.to_str() {
                Some("--state-dir") => state_dir = Some(PathBuf::from(value)),
                Some("--notify") => notify = Some(PathBuf::from(value)),
                Some("--idle-timeout") => {
                    let seconds = value
                        .to_str()
                        .and_then(|v| v.parse::<u64>().ok())
                        .filter(|s| *s > 0)
                        .ok_or_else(|| {
                            format!("--idle-timeout takes seconds, not {}", value.display())
                        })?;
                    idle = Duration::from_secs(seconds);
                }
                _ => return Err(format!("unknown argument {}", flag.display())),
            }
            rest = tail;
        }
        if let [extra, ..] = rest {
            return Err(format!("{} takes a value", extra.display()));
        }
        let state_dir = match state_dir.or_else(default_state_dir) {
            Some(dir) => dir,
            None => {
                return Err("no --state-dir, and neither XDG_STATE_HOME nor HOME is set".to_owned());
            }
        };
        Ok(Self {
            state_dir,
            notify,
            idle,
        })
    }
}

/// Runs the daemon of `config` with today's services (the request seam's [`service::Unwired`]
/// until the views are wired here): why it stopped, or why it failed (the process exits 1).
pub fn serve(config: &DaemonConfig) -> Result<run::Ended, String> {
    run::run(config, |_state| service::Unwired)
}

/// The default state directory: `$XDG_STATE_HOME/jfl`, else `$HOME/.local/state/jfl`; `None`
/// when neither is set (the caller asks for `--state-dir`).
#[must_use]
pub fn default_state_dir() -> Option<PathBuf> {
    let absolute = |v: OsString| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    if let Some(state) = std::env::var_os("XDG_STATE_HOME").and_then(absolute) {
        return Some(state.join("jfl"));
    }
    std::env::var_os("HOME")
        .and_then(absolute)
        .map(|home| home.join(".local").join("state").join("jfl"))
}

/// The variables a spawned daemon inherits (every other is cleared, `jfl-process`).
pub const DAEMON_ENV: [&str; 8] = [
    "PATH",
    "HOME",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
    "JAVA_HOME",
];

/// Starts `<program> daemon --state-dir <dir> --notify <path>` detached through `jfl-process`,
/// its output appended to the state directory's log (`0600`).
#[derive(Debug)]
pub struct ProcessLauncher {
    program: PathBuf,
    state: StateDir,
    idle: Option<Duration>,
    runner: Runner,
}

impl ProcessLauncher {
    /// A launcher of `program` (the `jfl` binary) for the daemon of `state`.
    #[must_use]
    pub fn new(program: &Path, state: StateDir) -> Self {
        Self {
            program: program.to_owned(),
            state,
            idle: None,
            runner: Runner::new(),
        }
    }

    /// The same, its daemons exiting after `idle` with no connection.
    #[must_use]
    pub fn with_idle(mut self, idle: Duration) -> Self {
        self.idle = Some(idle);
        self
    }

    /// Starts the daemon, reporting on `notify`.
    pub fn spawn(&mut self, notify: &Path) -> io::Result<DetachedDaemon> {
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(jfl_adapter_ipc::state::FILE_MODE)
            .open(self.state.log_path())?;
        let mut command = Command::new(self.program.clone())
            .arg("daemon")
            .arg("--state-dir")
            .arg(self.state.path())
            .arg("--notify")
            .arg(notify);
        if let Some(idle) = self.idle {
            command = command
                .arg("--idle-timeout")
                .arg(idle.as_secs().max(1).to_string());
        }
        for name in DAEMON_ENV {
            command = command.inherit_env(name);
        }
        self.runner
            .spawn_detached(&command, OwnedFd::from(log))
            .map(DetachedDaemon)
            .map_err(io::Error::other)
    }
}

/// A daemon started detached.
#[derive(Debug)]
pub struct DetachedDaemon(Detached);

impl DetachedDaemon {
    /// Its pid.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.0.id()
    }

    /// Its exit, if it has exited (reaping it then).
    pub fn try_exit(&mut self) -> io::Result<Option<jfl_process::Exit>> {
        self.0.try_exit()
    }
}

impl Spawned for DetachedDaemon {
    fn exited(&mut self) -> bool {
        self.0.try_exit().map_or(true, |exit| exit.is_some())
    }

    fn exit_fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        self.0.exit_fd()
    }
}

impl Launcher for ProcessLauncher {
    fn launch(&mut self, notify: &Path) -> io::Result<Box<dyn Spawned>> {
        Ok(Box::new(self.spawn(notify)?))
    }
}
