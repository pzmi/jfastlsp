//! The daemon's IPC (IDX-12, ADR 0031): `just bench-daemon`.
//!
//! Real processes, as a client meets them: the test daemon binary (the `jfl daemon` entry) spawned
//! detached through `jfl-process` by a real proxy, in a scratch state directory.
//!
//! - **spawn to ready**: from a proxy finding no daemon to the answer of its first request (the
//!   spawn lock, the second check, the spawn, the notify handshake, connect, the version
//!   handshake, one `jfl/status`), [`SPAWNS`] times, each daemon stopped with `jfl/stop` after;
//! - **connect and handshake** against a running daemon: a new connection, its handshake request
//!   and the daemon's answer, [`CONNECTS`] times;
//! - **round trip** of a trivial request (`jfl/status`, whose answer holds no view) on one open
//!   connection, [`ROUND_TRIPS`] times.
//!
//! p50 and p95 of each. No corpus: none of these touches a Java file (the queries' costs are the
//! views' and the resolver's suites). Reports, never gates.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use jfl_adapter_ipc::{SocketPath, StateDir};
use jfl_composition::daemon::ProcessLauncher;
use jfl_composition::daemon::proxy::{Budget, Proxy};
use jfl_protocol::messages::{ClientKind, Status, StatusParams, Stop, StopParams};

const SPAWNS: usize = 20;
const CONNECTS: usize = 500;
const ROUND_TRIPS: usize = 5_000;
const BUDGET: Duration = Duration::from_secs(30);
const PROGRAM: &str = env!("CARGO_BIN_EXE_jfl-composition-test-daemon");

fn percentile(samples: &mut [Duration], pct: usize) -> Duration {
    samples.sort_unstable();
    let at = samples
        .len()
        .saturating_sub(1)
        .saturating_mul(pct)
        .checked_div(100)
        .unwrap_or(0);
    samples.get(at).copied().unwrap_or_default()
}

fn report(name: &str, samples: &mut [Duration]) {
    let p50 = percentile(samples, 50);
    let p95 = percentile(samples, 95);
    println!(
        "daemon/{name}: p50 {:.1} µs, p95 {:.1} µs ({} samples)",
        p50.as_secs_f64() * 1e6,
        p95.as_secs_f64() * 1e6,
        samples.len()
    );
}

fn proxy(state: &StateDir, socket: &SocketPath, name: &str) -> Proxy<ProcessLauncher> {
    Proxy::new(
        name,
        ClientKind::Cli,
        state.clone(),
        socket.clone(),
        ProcessLauncher::new(Path::new(PROGRAM), state.clone()),
        Budget::default(),
    )
}

fn main() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("jfl-bench-daemon-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let state = StateDir::open(&root.join("s")).map_err(|e| e.to_string())?;
    let socket = SocketPath::of_state(&state).map_err(|e| e.to_string())?;
    let measured = measure(&state, &socket);
    // Whatever happened, no daemon is left.
    let mut last = proxy(&state, &socket, "stop");
    if socket.path().exists() {
        let _ = last.call::<Stop>(StopParams {}, BUDGET);
    }
    let _ = fs::remove_dir_all(&root);
    measured
}

fn measure(state: &StateDir, socket: &SocketPath) -> Result<(), String> {
    let mut spawns = Vec::with_capacity(SPAWNS);
    for i in 0..SPAWNS {
        let mut client = proxy(state, socket, &format!("s{i}"));
        let start = Instant::now();
        client
            .call::<Status>(StatusParams { worktree: None }, BUDGET)
            .map_err(|e| e.to_string())?;
        spawns.push(start.elapsed());
        client
            .call::<Stop>(StopParams {}, BUDGET)
            .map_err(|e| e.to_string())?;
        let stopped = Instant::now();
        while socket.path().exists() {
            if stopped.elapsed() > BUDGET {
                return Err("the daemon did not stop".to_owned());
            }
            std::thread::yield_now();
        }
    }
    report("spawn-to-ready", &mut spawns);

    let mut keeper = proxy(state, socket, "keeper");
    keeper
        .call::<Status>(StatusParams { worktree: None }, BUDGET)
        .map_err(|e| e.to_string())?;
    let mut connects = Vec::with_capacity(CONNECTS);
    for i in 0..CONNECTS {
        let mut client = proxy(state, socket, &format!("c{i}"));
        let start = Instant::now();
        client.connect().map_err(|e| e.to_string())?;
        client.on_accepted().map_err(|e| e.to_string())?;
        connects.push(start.elapsed());
    }
    report("connect-and-handshake", &mut connects);

    let mut trips = Vec::with_capacity(ROUND_TRIPS);
    for _ in 0..ROUND_TRIPS {
        let start = Instant::now();
        keeper
            .call::<Status>(StatusParams { worktree: None }, BUDGET)
            .map_err(|e| e.to_string())?;
        trips.push(start.elapsed());
    }
    report("status-round-trip", &mut trips);
    keeper
        .call::<Stop>(StopParams {}, BUDGET)
        .map_err(|e| e.to_string())?;
    Ok(())
}
