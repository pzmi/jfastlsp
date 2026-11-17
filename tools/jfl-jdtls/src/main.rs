//! The `jfl-jdtls` binary: jdtls behind a multiplexing proxy (see `doc/eval/jfl-jdtls.md`).
//!
//! ```text
//! jfl-jdtls lsp    [--state-dir DIR] [--root DIR] [DAEMON OPTIONS]
//! jfl-jdtls daemon [--state-dir DIR] [DAEMON OPTIONS]
//! jfl-jdtls status [--state-dir DIR] [--short-socket-base DIR]
//! jfl-jdtls stop   [--state-dir DIR] [--short-socket-base DIR]
//!
//! DAEMON OPTIONS (used by the proxy that starts the daemon; a running daemon keeps its own):
//!   --jdtls-home DIR        the extracted jdtls (default: where scripts/jdtls-fetch.sh puts it)
//!   --java PATH             the java executable (default: java on PATH)
//!   --heap-mib N            jdtls's -Xmx (default 2048)
//!   --syntax-server         run jdtls in syntax-server mode
//!   --no-gradle-import      java.import.gradle.enabled=false
//!   --idle-secs N           stop a jdtls with no client after N seconds (default 1800)
//!   --daemon-idle-secs N    exit the daemon after N seconds with no jdtls (default 600)
//!   --short-socket-base DIR where the socket goes when <state>/sock is too long for a socket
//!                           address (default /tmp; the socket is <DIR>/jfl-jdtls-<uid>/<key>.sock)
//!   --backend-command PROG [ARGS...]   front another language server instead (must be last)
//! ```

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use jfl_jdtls::daemon::{self, DaemonConfig};
use jfl_jdtls::json::json;
use jfl_jdtls::launch::{BackendSpec, JdtlsLaunch, StateLayout};
use jfl_jdtls::proxy::{self, LspOptions};

const USAGE: &str = "usage: jfl-jdtls <lsp|daemon|status|stop> [--state-dir DIR] [options]; see doc/eval/jfl-jdtls.md";

#[derive(Default)]
struct Parsed {
    state_dir: Option<PathBuf>,
    root: Option<PathBuf>,
    jdtls_home: Option<PathBuf>,
    java: Option<PathBuf>,
    heap_mib: Option<u32>,
    syntax_server: bool,
    gradle_import: bool,
    idle_secs: Option<u64>,
    daemon_idle_secs: Option<u64>,
    short_socket_base: Option<PathBuf>,
    backend_command: Option<Vec<OsString>>,
    /// The daemon options as given, for a proxy to pass on.
    daemon_args: Vec<OsString>,
}

fn parse(args: Vec<OsString>) -> Result<Parsed, String> {
    let mut parsed = Parsed {
        gradle_import: true,
        ..Parsed::default()
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let name = arg.to_string_lossy().into_owned();
        let value = |it: &mut std::vec::IntoIter<OsString>| {
            it.next().ok_or_else(|| format!("{name} needs a value"))
        };
        let number = |v: OsString| {
            v.to_string_lossy()
                .parse::<u64>()
                .map_err(|_| format!("{name} needs a number"))
        };
        match name.as_str() {
            "--state-dir" => parsed.state_dir = Some(value(&mut it)?.into()),
            "--root" => parsed.root = Some(value(&mut it)?.into()),
            "--short-socket-base" => {
                let v = value(&mut it)?;
                parsed.daemon_args.push(arg.clone());
                parsed.daemon_args.push(v.clone());
                parsed.short_socket_base = Some(v.into());
            }
            "--backend-command" => {
                let rest: Vec<OsString> = it.by_ref().collect();
                if rest.is_empty() {
                    return Err("--backend-command needs a program".into());
                }
                parsed.daemon_args.push(arg);
                parsed.daemon_args.extend(rest.iter().cloned());
                parsed.backend_command = Some(rest);
            }
            "--syntax-server" | "--no-gradle-import" => {
                if name == "--syntax-server" {
                    parsed.syntax_server = true;
                } else {
                    parsed.gradle_import = false;
                }
                parsed.daemon_args.push(arg);
            }
            "--jdtls-home" | "--java" | "--heap-mib" | "--idle-secs" | "--daemon-idle-secs" => {
                let v = value(&mut it)?;
                parsed.daemon_args.push(arg.clone());
                parsed.daemon_args.push(v.clone());
                match name.as_str() {
                    "--jdtls-home" => parsed.jdtls_home = Some(v.into()),
                    "--java" => parsed.java = Some(v.into()),
                    "--heap-mib" => {
                        let n = u32::try_from(number(v)?)
                            .map_err(|_| "--heap-mib is too large".to_owned())?;
                        parsed.heap_mib = Some(n);
                    }
                    "--idle-secs" => parsed.idle_secs = Some(number(v)?),
                    _ => parsed.daemon_idle_secs = Some(number(v)?),
                }
            }
            _ => return Err(format!("unknown option {name}")),
        }
    }
    Ok(parsed)
}

fn layout(parsed: &Parsed) -> Result<StateLayout, String> {
    let dir = match &parsed.state_dir {
        Some(dir) => std::path::absolute(dir).map_err(|e| e.to_string())?,
        None => proxy::default_state_dir().ok_or("no state directory: pass --state-dir")?,
    };
    let layout = StateLayout::new(dir);
    Ok(match &parsed.short_socket_base {
        Some(base) => layout.with_short_base(std::path::absolute(base).map_err(|e| e.to_string())?),
        None => layout,
    })
}

fn daemon_config(parsed: Parsed) -> Result<DaemonConfig, String> {
    let layout = layout(&parsed)?;
    let backend = match parsed.backend_command {
        Some(mut command) => {
            let program = command.remove(0);
            BackendSpec::Command {
                program,
                args: command,
            }
        }
        None => BackendSpec::Jdtls(JdtlsLaunch {
            java: parsed.java.unwrap_or_else(|| "java".into()),
            home: match parsed.jdtls_home {
                Some(home) => home,
                None => proxy::default_jdtls_home().ok_or("no jdtls home: pass --jdtls-home")?,
            },
            heap_mib: parsed.heap_mib.unwrap_or(2048),
            syntax_server: parsed.syntax_server,
        }),
    };
    Ok(DaemonConfig {
        layout,
        backend,
        idle_timeout: Duration::from_secs(parsed.idle_secs.unwrap_or(1800)),
        daemon_idle: Duration::from_secs(parsed.daemon_idle_secs.unwrap_or(600)),
        stop_timeout: Duration::from_secs(10),
        gradle_import: parsed.gradle_import,
    })
}

fn run(args: Vec<OsString>) -> Result<(), String> {
    let mut args = args.into_iter();
    let command = args
        .next()
        .map(|c| c.to_string_lossy().into_owned())
        .ok_or(USAGE)?;
    let parsed = parse(args.collect())?;
    match command.as_str() {
        "lsp" => {
            let options = LspOptions {
                layout: layout(&parsed)?,
                start: match &parsed.root {
                    Some(root) => root.clone(),
                    None => std::env::current_dir().map_err(|e| e.to_string())?,
                },
                exe: std::env::current_exe().map_err(|e| e.to_string())?,
                daemon_args: parsed.daemon_args,
            };
            proxy::run_lsp(&options).map_err(|e| e.to_string())
        }
        "daemon" => daemon::run(daemon_config(parsed)?).map_err(|e| e.to_string()),
        "status" | "stop" => {
            let reply = proxy::ask(&layout(&parsed)?, &command).map_err(|e| e.to_string())?;
            let reply = reply.unwrap_or_else(|| json!({ "ok": true, "running": false }));
            let text = serde_json::to_vec(&reply).map_err(|e| e.to_string())?;
            println!("{}", String::from_utf8_lossy(&text));
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("jfl-jdtls: {message}");
            ExitCode::FAILURE
        }
    }
}
