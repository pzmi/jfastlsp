//! The stdio side: `jfl-jdtls lsp` connects to the daemon (starting it when none answers) and
//! copies bytes between its stdio and the daemon's socket; `status` and `stop` ask the daemon.
//!
//! "Connect, or start the daemon" runs under the state directory's startup lock, so two proxies
//! never start two daemons, and the daemon's idle exit takes the same lock so it never exits
//! under a proxy that has just connected. The proxy writes nothing to the socket after its hello
//! until the daemon has replied, which is what lets the daemon read the hello through a buffer.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::sync_channel;
use std::thread;
use std::time::Duration;

use crate::daemon::{MAX_HELLO_BYTES, allowed_env, open_lock, prepare_state_dir};
use crate::framing::{read_frame, write_frame};
use crate::json::{Value, json};
use crate::launch::StateLayout;

/// How long a freshly started daemon may take to print `ready`.
pub const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the daemon may take to answer a hello (a `stop` waits for every jdtls to stop).
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// What `lsp` needs.
#[derive(Debug, Clone)]
pub struct LspOptions {
    /// The state directory.
    pub layout: StateLayout,
    /// The directory whose worktree this client works in.
    pub start: PathBuf,
    /// This executable, to start the daemon with.
    pub exe: PathBuf,
    /// Daemon options, passed on when this proxy starts the daemon (ignored by a running one).
    pub daemon_args: Vec<OsString>,
}

/// Runs the stdio proxy until the daemon closes the connection.
///
/// # Errors
///
/// When the daemon cannot be reached or started, or refuses the client.
pub fn run_lsp(options: &LspOptions) -> io::Result<()> {
    let root = std::path::absolute(&options.start)?;
    let hello = json!({ "op": "lsp", "root": root.to_string_lossy() });
    let stream = {
        let layout = prepare_state_dir(&options.layout)?;
        let startup = open_lock(&layout.startup_lock())?;
        startup.lock()?;
        let mut stream = match UnixStream::connect(layout.socket()) {
            Ok(stream) => stream,
            Err(_) => {
                start_daemon(options, &layout)?;
                UnixStream::connect(layout.socket())?
            }
        };
        let reply = exchange(&mut stream, &hello)?;
        if reply.get("ok") != Some(&Value::Bool(true)) {
            let why = reply
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("refused");
            return Err(io::Error::other(format!(
                "daemon refused the client: {why}"
            )));
        }
        stream
    };
    let mut to_daemon = stream.try_clone()?;
    thread::Builder::new()
        .name("jdtls-stdin".into())
        .spawn(move || {
            let _ = copy_flushing(&mut io::stdin().lock(), &mut to_daemon);
            let _ = to_daemon.shutdown(std::net::Shutdown::Write);
        })?;
    let mut from_daemon = stream;
    copy_flushing(&mut from_daemon, &mut io::stdout().lock())
}

/// Copies until end of input, flushing after every read (stdout is line-buffered and LSP frames
/// do not end in a newline).
fn copy_flushing<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> io::Result<()> {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(buf.get(..n).unwrap_or_default())?;
        writer.flush()?;
    }
}

fn exchange(stream: &mut UnixStream, hello: &Value) -> io::Result<Value> {
    let body = serde_json::to_vec(hello).map_err(io::Error::other)?;
    write_frame(stream, &body)?;
    stream.set_read_timeout(Some(REPLY_TIMEOUT))?;
    // Unbuffered: the bytes after the reply belong to the LSP stream.
    let mut reader = BufReader::with_capacity(1, &*stream);
    let reply = read_frame(&mut reader, MAX_HELLO_BYTES)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "daemon closed the connection")
    })?;
    stream.set_read_timeout(None)?;
    serde_json::from_slice(&reply).map_err(io::Error::other)
}

fn start_daemon(options: &LspOptions, layout: &StateLayout) -> io::Result<()> {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(layout.logs().join("daemon.log"))?;
    let mut child = Command::new(&options.exe)
        .arg("daemon")
        .arg("--state-dir")
        .arg(layout.root())
        .args(&options.daemon_args)
        .env_clear()
        .envs(allowed_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log))
        .process_group(0)
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("daemon has no stdout"))?;
    let (tx, rx) = sync_channel(1);
    thread::Builder::new()
        .name("jdtls-daemon-ready".into())
        .spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).take(64).read_line(&mut line);
            let _ = tx.send(line);
        })?;
    // Reap the daemon whenever it exits, so it never lingers as a zombie of this proxy.
    thread::Builder::new()
        .name("jdtls-daemon-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    let line = rx.recv_timeout(READY_TIMEOUT).map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "the daemon did not get ready; see logs/daemon.log",
        )
    })?;
    match line.trim() {
        "ready" => Ok(()),
        "busy" => Err(io::Error::other(
            "another daemon holds the lock but does not answer",
        )),
        _ => Err(io::Error::other(
            "the daemon failed to start; see logs/daemon.log",
        )),
    }
}

/// Sends `{"op": op}` to a running daemon and returns its reply; `None` when no daemon answers.
///
/// # Errors
///
/// When the state or short socket directory is refused, or the exchange fails after
/// connecting.
pub fn ask(layout: &StateLayout, op: &str) -> io::Result<Option<Value>> {
    if layout.root().symlink_metadata().is_err() {
        return Ok(None);
    }
    // The same checks as `lsp`: a socket in a directory someone else controls is not asked.
    let layout = prepare_state_dir(layout)?;
    let Ok(mut stream) = UnixStream::connect(layout.socket()) else {
        return Ok(None);
    };
    exchange(&mut stream, &json!({ "op": op })).map(Some)
}

/// The default state directory: `$XDG_STATE_HOME/jfl-jdtls`, else `~/.local/state/jfl-jdtls`.
#[must_use]
pub fn default_state_dir() -> Option<PathBuf> {
    match std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        Some(state) => Some(PathBuf::from(state).join("jfl-jdtls")),
        None => std::env::home_dir().map(|h| h.join(".local/state/jfl-jdtls")),
    }
}

/// The default jdtls install: where `scripts/jdtls-fetch.sh` puts the pinned release.
#[must_use]
pub fn default_jdtls_home() -> Option<PathBuf> {
    let cache = match std::env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        Some(cache) => PathBuf::from(cache),
        None => std::env::home_dir()?.join(".cache"),
    };
    Some(
        cache
            .join("jfl-jdtls")
            .join(format!("jdtls-{}", crate::launch::JDTLS_VERSION)),
    )
}
