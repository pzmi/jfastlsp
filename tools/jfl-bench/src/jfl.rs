//! The `jfl-lsp` suite (LSP-02, ADR 0031, plan §1.7): the built `jfl lsp` over a corpus checkout,
//! end to end as a client meets it, in a fresh state directory so the first session finds no
//! daemon and no stored summary.
//!
//! - **The cold session**: the process started, `initialize` answered (never waiting on the
//!   daemon, plan §2.5), then the first `textDocument/definition` at a usage site, which starts
//!   the daemon, opens the view (its own directory read first, plan §2.7) and answers: plan §1.7's
//!   "time to first useful answer on a fresh worktree" (< 2 s on medium), from the process's start.
//!   Then `warmup + runs` rounds of a definition at every site, each request's round trip timed.
//! - **The warm session**: a second `jfl lsp` while the first one's daemon still runs: its
//!   `initialize` (plan §1.7's "`initialize` against a warm daemon", < 200 ms, reported and never
//!   asserted) and its first definition, the daemon already holding the view.
//!
//! Each session runs under coreutils `timeout`, so a server that hangs cannot hang the harness.
//! The daemon is started with a one-second idle period and exits on its own once the warm
//! session closes (it holds no connection then); the harness has no grant to wait for it.
//! Metrics: `jfl-lsp/<size>/initialize_ms`, `first_definition_ms`, `initialize_warm_ms`,
//! `first_definition_warm_ms`, `definition/p50`, `definition/p95`, `definition/found`.
//!
//! Pinned by: this module's tests (the command line).

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use serde_json::{Value, json};

use crate::arms::{Site, usage_sites};
use crate::error::{BenchError, Context, Result};
use crate::lsp::{self, Client};
use crate::queries;
use crate::report::{Better, Metric, QuerySlot};
use crate::suite::JflLsp;

/// What the arm needs besides the suite.
#[derive(Debug, Clone)]
pub struct JflRun<'a> {
    /// The corpus size name, the metrics' middle segment.
    pub size: &'a str,
    /// The corpus checkout.
    pub root: &'a Path,
    /// The `jfl` binary (a release build).
    pub jfl: &'a Path,
    /// Scratch space: the state directory and the servers' standard error.
    pub scratch: &'a Path,
    /// Untimed rounds.
    pub warmup: u32,
    /// Timed rounds.
    pub runs: u32,
    /// Seed for the picked types.
    pub seed: u64,
}

/// The variables a session keeps (the daemon runs `git`); every other is cleared.
const ENV_ALLOWLIST: [&str; 3] = ["PATH", "HOME", "LANG"];

/// The `jfl lsp` command line for a session in `state`, under `timeout` (`deadline_secs`).
#[must_use]
pub fn argv(jfl: &Path, state: &Path, deadline_secs: u64) -> Vec<String> {
    vec![
        "timeout".to_string(),
        "--kill-after=10s".to_string(),
        format!("{deadline_secs}s"),
        jfl.display().to_string(),
        "lsp".to_string(),
        "--state-dir".to_string(),
        state.display().to_string(),
        "--idle-timeout".to_string(),
        "1".to_string(),
    ]
}

/// One session: the child, ended by `exit` and waited for when dropped.
struct Session {
    child: Child,
    client: Client<BufReader<std::process::ChildStdout>, std::process::ChildStdin>,
}

impl Session {
    fn start(r: &JflRun<'_>, j: &JflLsp, state: &Path, name: &str) -> Result<Self> {
        let argv = argv(r.jfl, state, j.deadline_secs);
        let (program, rest) = argv.split_first().context("empty jfl command")?;
        let stderr = std::fs::File::create(r.scratch.join(format!("{name}.stderr")))
            .context("create the server's stderr file")?;
        let mut cmd = Command::new(program);
        cmd.args(rest).env_clear();
        for var in ENV_ALLOWLIST {
            if let Some(value) = std::env::var_os(var) {
                cmd.env(var, value);
            }
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .context("start jfl lsp under timeout (coreutils `timeout` must be on PATH)")?;
        let stdin = child.stdin.take().context("jfl lsp's stdin")?;
        let stdout = child.stdout.take().context("jfl lsp's stdout")?;
        Ok(Self {
            child,
            client: Client::new(BufReader::new(stdout), stdin),
        })
    }

    fn end(mut self) -> Result<()> {
        self.client.request("shutdown", Value::Null)?;
        self.client.notify("exit", Value::Null)?;
        let status = self.child.wait().context("wait for jfl lsp")?;
        if status.code() != Some(0) {
            return Err(BenchError::new(format!(
                "jfl lsp exited with {status} after shutdown and exit"
            )));
        }
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn definition(site: &Site) -> Value {
    json!({
        "textDocument": {"uri": lsp::file_uri(&site.file)},
        "position": {"line": site.line, "character": site.character}
    })
}

fn initialize(client: &mut Session) -> Result<()> {
    let (result, _) = client.client.request(
        "initialize",
        json!({"processId": std::process::id(), "capabilities": {}}),
    )?;
    if result.get("capabilities").is_none() {
        return Err(BenchError::new("initialize answered without capabilities"));
    }
    client.client.notify("initialized", json!({}))
}

/// The arm: a cold session, then a warm one.
pub fn jfl_lsp_suite(
    r: &JflRun<'_>,
    j: &JflLsp,
    notes: &mut Vec<String>,
) -> Result<(Vec<Metric>, Vec<QuerySlot>)> {
    if !r.jfl.is_file() {
        return Err(BenchError::new(format!(
            "no jfl at {}: build it (`cargo build --release --bin jfl`) or pass --jfl PATH",
            r.jfl.display()
        )));
    }
    let picked = queries::pick(r.root, j.sites, r.seed)?;
    let sites = usage_sites(r.root, &picked);
    let first = sites.first().context("no usage site picked")?;
    let slots: Vec<QuerySlot> = sites
        .iter()
        .enumerate()
        .map(|(i, s)| QuerySlot {
            slot: format!("s{i:02}"),
            name_blake3: blake3::hash(s.name.as_bytes()).to_hex().to_string(),
        })
        .collect();
    // `jfl` makes its state directory itself (0700, or it refuses it): a fresh parent here.
    let parent = r.scratch.join("jfl");
    crate::jdtls::fresh_dir(&parent)?;
    let state: PathBuf = parent.join("state");
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1e3;
    let size = r.size;

    let t0 = Instant::now();
    let mut cold = Session::start(r, j, &state, "cold")?;
    initialize(&mut cold)?;
    let initialize_ms = ms(t0);
    let (answer, _) = cold
        .client
        .request("textDocument/definition", definition(first))?;
    let first_definition_ms = ms(t0);
    if !lsp::is_found(&answer) {
        notes.push("jfl-lsp: the first definition found nothing (a partial first answer)".into());
    }
    let mut times = Vec::new();
    let mut found = 0_u64;
    for round in 0..r.warmup.saturating_add(r.runs) {
        for site in &sites {
            let (result, elapsed) = cold
                .client
                .request("textDocument/definition", definition(site))?;
            if round == r.warmup && lsp::is_found(&result) {
                found = found.saturating_add(1);
            }
            if round >= r.warmup {
                times.push(elapsed);
            }
        }
    }
    // The warm session starts before the cold one ends, so the daemon is certainly still up.
    let t1 = Instant::now();
    let mut warm = Session::start(r, j, &state, "warm")?;
    initialize(&mut warm)?;
    let initialize_warm_ms = ms(t1);
    let _ = warm
        .client
        .request("textDocument/definition", definition(first))?;
    let first_definition_warm_ms = ms(t1);
    cold.end()?;
    warm.end()?;

    let what = format!("requests ({} sites x {} rounds)", sites.len(), r.runs);
    let mut metrics = vec![
        Metric::single(
            format!("jfl-lsp/{size}/initialize_ms"),
            "ms",
            Better::Smaller,
            initialize_ms,
        ),
        Metric::single(
            format!("jfl-lsp/{size}/first_definition_ms"),
            "ms",
            Better::Smaller,
            first_definition_ms,
        ),
        Metric::single(
            format!("jfl-lsp/{size}/initialize_warm_ms"),
            "ms",
            Better::Smaller,
            initialize_warm_ms,
        ),
        Metric::single(
            format!("jfl-lsp/{size}/first_definition_warm_ms"),
            "ms",
            Better::Smaller,
            first_definition_warm_ms,
        ),
    ];
    for (label, p) in [("p50", 0.5), ("p95", 0.95)] {
        metrics.push(Metric::percentile_over(
            format!("jfl-lsp/{size}/definition/{label}"),
            "ms",
            Better::Smaller,
            times.clone(),
            p,
            label,
            &what,
        ));
    }
    metrics.push(Metric::single(
        format!("jfl-lsp/{size}/definition/found"),
        "count",
        Better::Info,
        found as f64,
    ));
    notes.push(
        "jfl-lsp: one cold session from a fresh state directory (no daemon, no stored summary), \
         then a warm one against its daemon; definition timed per request over LSP"
            .to_string(),
    );
    Ok((metrics, slots))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_run_jfl_lsp_under_timeout_in_its_state_directory_when_a_session_starts() {
        // given
        let jfl = Path::new("/opt/jfl");
        let state = Path::new("/s/jfl-state");

        // when
        let argv = argv(jfl, state, 300);

        // then
        assert_eq!(
            argv,
            [
                "timeout",
                "--kill-after=10s",
                "300s",
                "/opt/jfl",
                "lsp",
                "--state-dir",
                "/s/jfl-state",
                "--idle-timeout",
                "1"
            ]
        );
    }
}
