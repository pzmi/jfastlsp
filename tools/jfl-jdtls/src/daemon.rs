//! The per-user daemon: one state directory, one owner (by lock), one Unix socket, one session
//! (one backend) per worktree root.
//!
//! A connection starts with a *hello* frame, `{"op": "lsp" | "status" | "stop", ...}`, answered
//! by one reply frame; an `lsp` connection then carries the client's LSP stream, bound for its
//! lifetime to the worktree of the hello's `root` (the proxy's working directory), not per URI.
//! The state directory is the access control: created `0700`, and an existing one is refused
//! unless it is a real directory (not a symlink) of mode `0700` owned by the effective uid. The
//! socket is `0600`, the locks `0600`; on Linux a peer whose uid is not ours is refused (macOS
//! relies on the directory check alone). Connections are capped; a hello must arrive within
//! [`HELLO_TIMEOUT`].
//!
//! Pinned by `tests/daemon.rs` and the tests below.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{DirBuilder, File, OpenOptions, TryLockError};
use std::io::{self, BufReader, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crate::framing::{read_frame, write_frame};
use crate::json::{Value, json};
use crate::launch::{BackendSpec, ENV_ALLOWLIST, MAX_SOCKET_PATH, StateLayout};
use crate::mux::MuxConfig;
use crate::session::{self, Backend, CLIENT_CAPACITY, Event, Owner, SessionConfig, SessionHandle};
use crate::worktree::{data_key, worktree_root};

/// The largest hello or reply frame.
pub const MAX_HELLO_BYTES: usize = 64 * 1024;

/// How long a new connection may take to send its hello.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// The most connections served at once (each is a thread and, for `lsp`, a client).
pub const MAX_CONNECTIONS: usize = 256;

/// The most bytes of a backend's stderr kept in its log; the rest is read and dropped.
pub const MAX_BACKEND_LOG_BYTES: u64 = 8 * 1024 * 1024;

/// jdtls's default `java.import.exclusions`, plus the nested agent worktrees, which are their own
/// roots with their own jdtls: the outer checkout's jdtls must not import them again.
pub const IMPORT_EXCLUSIONS: &[&str] = &[
    "**/node_modules/**",
    "**/.metadata/**",
    "**/archetype-resources/**",
    "**/META-INF/maven/**",
    "**/.claude/worktrees/**",
];

/// The daemon's settings.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Its state directory.
    pub layout: StateLayout,
    /// How backends start.
    pub backend: BackendSpec,
    /// How long a backend with no client lives.
    pub idle_timeout: Duration,
    /// How long the daemon lives with no backend and no connection.
    pub daemon_idle: Duration,
    /// How long a stopping backend gets before it is killed.
    pub stop_timeout: Duration,
    /// Whether jdtls may import Gradle projects (`java.import.gradle.enabled`).
    pub gradle_import: bool,
}

/// What is wrong with an existing state directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateDirFault {
    /// It is a symlink.
    Symlink,
    /// It is not a directory.
    NotADirectory,
    /// Its mode is not exactly `0700` (the mode found).
    Mode(u32),
    /// Another uid owns it (the owner found).
    Owner(u32),
}

/// Checks an existing state directory's metadata against the effective uid. Pinned by
/// `tests/daemon.rs::daemon_or_jvm::should_refuse_a_state_directory_when_it_is_not_private` and
/// `::should_refuse_a_state_directory_when_another_user_owns_it`.
#[must_use]
pub fn check_state_dir(meta: &std::fs::Metadata, euid: u32) -> Option<StateDirFault> {
    let kind = meta.file_type();
    if kind.is_symlink() {
        return Some(StateDirFault::Symlink);
    }
    if !kind.is_dir() {
        return Some(StateDirFault::NotADirectory);
    }
    if meta.uid() != euid {
        return Some(StateDirFault::Owner(meta.uid()));
    }
    let mode = meta.mode() & 0o7777;
    if mode != 0o700 {
        return Some(StateDirFault::Mode(mode));
    }
    None
}

/// Creates the state directory `0700` (its parents as usual), or checks an existing one, then
/// its `logs/` and, when the socket path would be too long, the short socket directory. Returns
/// the layout rooted at the **canonical** state directory, so every alias of it computes the
/// same socket.
///
/// # Errors
///
/// The filesystem's error, or a refusal naming the path when an existing directory is a symlink,
/// not a directory, not `0700` or not ours.
pub fn prepare_state_dir(layout: &StateLayout) -> io::Result<StateLayout> {
    let root = layout.root();
    if let Some(parent) = root.parent() {
        std::fs::create_dir_all(parent)?;
    }
    private_dir(root, "state directory")?;
    let layout = layout.clone().with_root(root.canonicalize()?);
    let root = layout.root();
    match DirBuilder::new().mode(0o700).create(layout.logs()) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        _ => {}
    }
    let socket = layout.socket();
    if let Some(dir) = socket.parent().filter(|d| *d != root) {
        // The state path is too long for a socket address: the socket lives in the short
        // directory, which must be as private as the state directory.
        let made = match dir.parent() {
            Some(base) => std::fs::create_dir_all(base),
            None => Ok(()),
        };
        made.and_then(|()| private_dir(dir, "short socket directory")).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!(
                    "{}/sock would exceed the {MAX_SOCKET_PATH}-byte socket path limit, and {e}",
                    root.display()
                ),
            )
        })?;
    }
    Ok(layout)
}

/// Creates `dir` with mode `0700`, or checks that an existing one passes [`check_state_dir`].
fn private_dir(dir: &Path, what: &str) -> io::Result<()> {
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let meta = dir.symlink_metadata()?;
            match check_state_dir(&meta, rustix::process::geteuid().as_raw()) {
                None => Ok(()),
                Some(fault) => Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "refusing {what} {}: {fault:?}; it must be a directory of mode 0700 owned by this user",
                        dir.display()
                    ),
                )),
            }
        }
        Err(e) => Err(e),
    }
}

/// Opens (creating, `0600`) a lock file in the state directory.
///
/// # Errors
///
/// The filesystem's error.
pub fn open_lock(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

/// Whether a connected peer runs as our effective uid. Linux reads `SO_PEERCRED`; elsewhere the
/// `0700` state directory is the only check (rustix has no `getpeereid`), so this is true.
#[cfg(target_os = "linux")]
fn peer_is_us(stream: &UnixStream) -> bool {
    rustix::net::sockopt::socket_peercred(stream)
        .is_ok_and(|cred| cred.uid == rustix::process::geteuid())
}

#[cfg(not(target_os = "linux"))]
fn peer_is_us(_stream: &UnixStream) -> bool {
    true
}

/// A `file://` URI for an absolute path (bytes outside the unreserved set percent-encoded).
#[must_use]
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'.' | b'_' | b'~') {
            uri.push(char::from(b));
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

struct Entry {
    id: u64,
    handle: SessionHandle,
    data_dir: PathBuf,
}

struct Daemon {
    config: DaemonConfig,
    registry: Mutex<HashMap<PathBuf, Entry>>,
    next_id: AtomicU64,
    connections: AtomicUsize,
    /// Poked when a connection or a session ends, so the idle monitor re-checks.
    activity: SyncSender<()>,
}

impl Daemon {
    fn registry(&self) -> MutexGuard<'_, HashMap<PathBuf, Entry>> {
        // A panic while holding the lock leaves the map itself consistent (every mutation is a
        // single insert or remove), so a poisoned lock is still usable.
        self.registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn fresh_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn poke(&self) {
        let _ = self.activity.try_send(());
    }

    fn busy(&self) -> bool {
        !self.registry().is_empty() || self.connections.load(Ordering::Acquire) > 0
    }
}

struct RegistryOwner {
    daemon: Arc<Daemon>,
    root: PathBuf,
    id: u64,
}

impl Owner for RegistryOwner {
    fn evict(&self, still_idle: &mut dyn FnMut() -> bool) -> bool {
        let mut registry = self.daemon.registry();
        if !still_idle() {
            return false;
        }
        if registry.get(&self.root).is_some_and(|e| e.id == self.id) {
            registry.remove(&self.root);
        }
        true
    }

    fn ended(&self) {
        {
            let mut registry = self.daemon.registry();
            if registry.get(&self.root).is_some_and(|e| e.id == self.id) {
                registry.remove(&self.root);
            }
        }
        self.daemon.poke();
    }
}

/// Runs the daemon: takes the lock (printing `busy` and returning when another daemon holds it),
/// binds the socket, prints `ready`, and serves until stopped or idle. Never returns once ready:
/// stop and idle exit end the process.
///
/// # Errors
///
/// When the state directory, the lock or the socket cannot be set up.
pub fn run(mut config: DaemonConfig) -> io::Result<()> {
    config.layout = prepare_state_dir(&config.layout)?;
    let mut lock = open_lock(&config.layout.daemon_lock())?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            println!("busy");
            return Ok(());
        }
        Err(TryLockError::Error(e)) => return Err(e),
    }
    lock.set_len(0)?;
    writeln!(lock, "{}", std::process::id())?;
    let socket = config.layout.socket();
    match std::fs::remove_file(&socket) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let (activity, woken) = sync_channel(1);
    let daemon = Arc::new(Daemon {
        config,
        registry: Mutex::new(HashMap::new()),
        next_id: AtomicU64::new(1),
        connections: AtomicUsize::new(0),
        activity,
    });
    let monitor = Arc::clone(&daemon);
    thread::Builder::new()
        .name("jdtls-idle".into())
        .spawn(move || idle_monitor(&monitor, &woken))?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "ready")?;
    stdout.flush()?;
    drop(stdout);
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if daemon.connections.load(Ordering::Acquire) >= MAX_CONNECTIONS {
            continue;
        }
        daemon.connections.fetch_add(1, Ordering::AcqRel);
        let served = Arc::clone(&daemon);
        let spawned = thread::Builder::new()
            .name("jdtls-connection".into())
            .spawn(move || {
                let _ = serve_connection(&served, stream);
                served.connections.fetch_sub(1, Ordering::AcqRel);
                served.poke();
            });
        if spawned.is_err() {
            daemon.connections.fetch_sub(1, Ordering::AcqRel);
        }
    }
    Ok(())
}

/// Exits the daemon once it has had no session and no connection for `daemon_idle`. It waits on
/// activity (a connection or session ending) while busy, and for the rest of the idle period
/// when not, so it wakes only when something changed or the period is over.
fn idle_monitor(daemon: &Arc<Daemon>, woken: &Receiver<()>) {
    let idle = daemon.config.daemon_idle;
    let mut idle_since = Instant::now();
    loop {
        if daemon.busy() {
            if woken.recv().is_err() {
                return;
            }
            idle_since = Instant::now();
            continue;
        }
        match woken.recv_timeout(idle.saturating_sub(idle_since.elapsed())) {
            Ok(()) => {
                idle_since = Instant::now();
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        // The startup lock keeps a proxy from connecting between this check and the exit.
        let Ok(startup) = open_lock(&daemon.config.layout.startup_lock()) else {
            return;
        };
        if startup.lock().is_err() || daemon.busy() {
            idle_since = Instant::now();
            continue;
        }
        let _ = std::fs::remove_file(daemon.config.layout.socket());
        std::process::exit(0);
    }
}

fn reply(stream: &mut UnixStream, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    write_frame(stream, &body)
}

fn refuse(stream: &mut UnixStream, message: &str) -> io::Result<()> {
    reply(stream, &json!({ "ok": false, "error": message }))
}

fn serve_connection(daemon: &Arc<Daemon>, mut stream: UnixStream) -> io::Result<()> {
    if !peer_is_us(&stream) {
        return refuse(&mut stream, "peer runs as another user");
    }
    stream.set_read_timeout(Some(HELLO_TIMEOUT))?;
    let hello: Value = {
        let mut reader = BufReader::new(&stream);
        let Some(body) = read_frame(&mut reader, MAX_HELLO_BYTES)? else {
            return Ok(());
        };
        // The hello is the only thing written before the reply, so nothing was buffered past it.
        serde_json::from_slice(&body).map_err(io::Error::other)?
    };
    stream.set_read_timeout(None)?;
    match hello.get("op").and_then(Value::as_str) {
        Some("lsp") => serve_lsp(daemon, stream, &hello),
        Some("status") => reply(&mut stream, &status(daemon)),
        Some("stop") => {
            // Under the startup lock and with the socket gone first, no proxy can start a new
            // jdtls while the others stop; the reply then means everything is down.
            let startup = open_lock(&daemon.config.layout.startup_lock())?;
            startup.lock()?;
            let _ = std::fs::remove_file(daemon.config.layout.socket());
            stop_all(daemon);
            reply(&mut stream, &json!({ "ok": true }))?;
            std::process::exit(0);
        }
        _ => refuse(&mut stream, "unknown op"),
    }
}

fn status(daemon: &Daemon) -> Value {
    let registry = daemon.registry();
    let mut sessions: Vec<(&PathBuf, &Entry)> = registry.iter().collect();
    sessions.sort_by(|a, b| a.0.cmp(b.0));
    let sessions: Vec<Value> = sessions
        .into_iter()
        .map(|(root, entry)| {
            json!({
                "root": root.to_string_lossy(),
                "dataDir": entry.data_dir.to_string_lossy(),
                "clients": entry.handle.status.clients(),
                "backendPid": entry.handle.backend_pid,
            })
        })
        .collect();
    json!({ "ok": true, "daemonPid": std::process::id(), "sessions": sessions })
}

fn stop_all(daemon: &Daemon) {
    let entries: Vec<Entry> = daemon.registry().drain().map(|(_, e)| e).collect();
    let mut waits = Vec::new();
    for entry in entries {
        let (done, wait) = sync_channel(1);
        if entry.handle.events.send(Event::Stop { done }).is_ok() {
            waits.push((wait, entry.handle.thread));
        }
    }
    let grace = daemon
        .config
        .stop_timeout
        .saturating_add(Duration::from_secs(5));
    for (wait, thread) in waits {
        if wait.recv_timeout(grace).is_ok() {
            let _ = thread.join();
        }
    }
}

fn serve_lsp(daemon: &Arc<Daemon>, mut stream: UnixStream, hello: &Value) -> io::Result<()> {
    let Some(root) = hello
        .get("root")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|r| r.is_absolute())
    else {
        return refuse(&mut stream, "hello needs an absolute root");
    };
    let root = match worktree_root(&root) {
        Ok(root) => root,
        Err(e) => return refuse(&mut stream, &format!("root: {e}")),
    };
    let client = daemon.fresh_id();
    let reader = stream.try_clone()?;
    let writer = stream.try_clone()?;
    let events = {
        let mut registry = daemon.registry();
        let reusable = registry
            .get(&root)
            .is_some_and(|e| !e.handle.thread.is_finished());
        if !reusable {
            registry.remove(&root);
            match start_session(daemon, &root) {
                Ok(entry) => {
                    registry.insert(root.clone(), entry);
                }
                Err(e) => return refuse(&mut stream, &format!("backend: {e}")),
            }
        }
        let Some(entry) = registry.get(&root) else {
            return refuse(&mut stream, "no session");
        };
        // The reply goes out before the client is attached, so no backend message can precede it.
        let ok = json!({
            "ok": true,
            "root": root.to_string_lossy(),
            "dataDir": entry.data_dir.to_string_lossy(),
        });
        reply(&mut stream, &ok)?;
        let sink = session::spawn_writer(writer, CLIENT_CAPACITY, |s: UnixStream| {
            let _ = s.shutdown(std::net::Shutdown::Both);
        })?;
        // try_send: a blocking send under the registry lock could deadlock with a session
        // that is waiting for the lock to unregister itself.
        match entry.handle.events.try_send(Event::Attach { client, sink }) {
            Ok(()) => entry.handle.events.clone(),
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => return Ok(()),
        }
    };
    session::pump_client(&events, client, reader);
    Ok(())
}

fn start_session(daemon: &Arc<Daemon>, root: &Path) -> io::Result<Entry> {
    let config = &daemon.config;
    let command = config.backend.command(&config.layout, root)?;
    std::fs::create_dir_all(&command.data_dir)?;
    std::fs::create_dir_all(config.layout.shared_index())?;
    let log = File::create(config.layout.logs().join(format!("{}.log", data_key(root))))?;
    // Its own process group, so the stop kills the Gradle and helper processes it starts too.
    let mut child = Command::new(&command.program)
        .args(&command.args)
        .current_dir(&command.cwd)
        .env_clear()
        .envs(allowed_env())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let (Some(stdout), Some(stdin), Some(stderr)) =
        (child.stdout.take(), child.stdin.take(), child.stderr.take())
    else {
        session::kill_group(&mut child);
        return Err(io::Error::other("backend has no stdio"));
    };
    thread::Builder::new()
        .name("jdtls-stderr".into())
        .spawn(move || copy_bounded(stderr, log))?;
    let gradle = if config.gradle_import {
        json!({})
    } else {
        json!({ "enabled": false })
    };
    let java = json!({ "import": { "exclusions": IMPORT_EXCLUSIONS, "gradle": gradle } });
    let settings = Some(json!({ "java": java }));
    let mux = MuxConfig {
        root_uri: file_uri(root),
        root_path: root.to_string_lossy().into_owned(),
        root_name: root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        process_id: Some(std::process::id()),
        settings,
    };
    let session_config = SessionConfig {
        mux,
        idle_timeout: config.idle_timeout,
        stop_timeout: config.stop_timeout,
        tick: quarter(config.idle_timeout).clamp(Duration::from_millis(50), Duration::from_secs(5)),
    };
    let id = daemon.fresh_id();
    let owner = RegistryOwner {
        daemon: Arc::clone(daemon),
        root: root.to_path_buf(),
        id,
    };
    let backend = Backend {
        reader: Box::new(stdout),
        writer: Box::new(stdin),
        child: Some(child),
    };
    let handle = session::start(session_config, backend, owner)?;
    Ok(Entry {
        id,
        handle,
        data_dir: command.data_dir,
    })
}

/// Copies a backend's stderr to its log, keeping the first [`MAX_BACKEND_LOG_BYTES`] and
/// reading the rest away so the backend never blocks on a full pipe.
fn copy_bounded<R: Read>(mut from: R, mut log: File) {
    let _ = io::copy(&mut from.by_ref().take(MAX_BACKEND_LOG_BYTES), &mut log);
    let _ = io::copy(&mut from, &mut io::sink());
}

fn quarter(d: Duration) -> Duration {
    d.checked_div(4).unwrap_or(d)
}

/// The allowlisted part of this process's environment ([`ENV_ALLOWLIST`]).
#[must_use]
pub fn allowed_env() -> Vec<(OsString, OsString)> {
    ENV_ALLOWLIST
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|v| (OsString::from(name), v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_percent_encode_bytes_outside_the_unreserved_set_when_building_a_uri() {
        // given
        let path = Path::new("/w/my repo/ä#x");

        // when
        let uri = file_uri(path);

        // then
        assert_eq!(uri, "file:///w/my%20repo/%C3%A4%23x");
    }
}
