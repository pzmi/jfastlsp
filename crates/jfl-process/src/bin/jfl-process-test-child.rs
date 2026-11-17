//! A child for `jfl-process`'s own tests and benchmark, so they need no shell and no tool whose
//! output differs between Linux and macOS. Not part of the product: nothing ships or starts it
//! but those tests (`CARGO_BIN_EXE_jfl-process-test-child`); why it is a bin target of this crate
//! is in its Cargo.toml.
//!
//! Modes (the first argument):
//!
//! - `args A B ...`: writes each argument followed by a NUL to stdout;
//! - `env`: writes `NAME=VALUE` followed by a NUL to stdout for each variable, sorted;
//! - `write OUT ERR`: writes OUT bytes `o` to stdout and ERR bytes `e` to stderr;
//! - `exit CODE`: exits with CODE;
//! - `hang`: blocks forever;
//! - `grandchild`: starts `hang` as its own child (in its group), writes that pid and a newline to
//!   stdout, then blocks forever;
//! - `orphan`: as `grandchild`, then exits 0, leaving the grandchild behind;
//! - `echo-frames`: echoes length-prefixed frames from stdin to stdout until end of file;
//! - `oversized-frame`: declares a frame of `u32::MAX` bytes on stdout, then blocks forever;
//! - `pgid`: writes its process group id and a newline to stdout;
//! - `self-kill`: sends itself `SIGKILL`;
//! - `escape-and-flood`: starts `flood` as its own child, waits until it has left the group and
//!   started writing, and exits 0;
//! - `flood`: leaves the process group (`setsid`), says so on stderr, then writes `o` to stdout
//!   until a write fails;
//! - `join-parent-group`: moves itself into its parent's process group (out of its own), writes
//!   `moved` and a newline to stderr, then blocks forever;
//! - `setsid`: calls `setsid` and writes `setsid ok` or `setsid failed: <error>` and a newline to
//!   stdout.

use std::io::{self, Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    use std::os::unix::ffi::OsStrExt;

    // args_os: an argument need not be UTF-8 (the contract suite sends one that is not).
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let Some((mode, rest)) = args.split_first() else {
        return ExitCode::from(2);
    };
    let rest: Vec<String> = rest
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let ran = match mode.to_str().unwrap_or_default() {
        "args" => write_terminated(args.iter().skip(1).map(|arg| arg.as_bytes())),
        "env" => print_env(),
        "write" => write_volume(&rest),
        "exit" => return ExitCode::from(rest.first().and_then(|c| c.parse().ok()).unwrap_or(2)),
        "hang" => hang(),
        "grandchild" => start_grandchild().and_then(|()| hang()),
        "orphan" => start_grandchild(),
        "echo-frames" => echo_frames(),
        "oversized-frame" => io::stdout()
            .write_all(&u32::MAX.to_be_bytes())
            .and_then(|()| io::stdout().flush())
            .and_then(|()| hang()),
        "pgid" => print_pgid(),
        "self-kill" => {
            rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::KILL)
                .map_err(io::Error::from)
                .and_then(|()| hang())
        }
        "escape-and-flood" => escape_and_flood(),
        "flood" => flood(),
        "join-parent-group" => join_parent_group().and_then(|()| hang()),
        "setsid" => report_setsid(),
        _ => return ExitCode::from(2),
    };
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}

fn write_terminated<'a>(items: impl Iterator<Item = &'a [u8]>) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    for item in items {
        stdout.write_all(item)?;
        stdout.write_all(b"\0")?;
    }
    stdout.flush()
}

fn print_env() -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;

    let mut vars: Vec<Vec<u8>> = std::env::vars_os()
        .map(|(name, value)| {
            let mut line = name.as_bytes().to_vec();
            line.push(b'=');
            line.extend_from_slice(value.as_bytes());
            line
        })
        .collect();
    vars.sort();
    write_terminated(vars.iter().map(Vec::as_slice))
}

fn write_volume(rest: &[String]) -> io::Result<()> {
    let count = |index: usize| -> usize {
        rest.get(index)
            .and_then(|n| n.parse().ok())
            .unwrap_or_default()
    };
    write_repeated(&mut io::stdout().lock(), b'o', count(0))?;
    write_repeated(&mut io::stderr().lock(), b'e', count(1))
}

fn write_repeated(sink: &mut impl Write, byte: u8, mut count: usize) -> io::Result<()> {
    let chunk = [byte; 8192];
    while count > 0 {
        let n = count.min(chunk.len());
        sink.write_all(chunk.get(..n).unwrap_or_default())?;
        count = count.saturating_sub(n);
    }
    sink.flush()
}

fn hang() -> io::Result<()> {
    loop {
        std::thread::park();
    }
}

fn start_grandchild() -> io::Result<()> {
    let me = std::env::current_exe()?;
    let grandchild = std::process::Command::new(me).arg("hang").spawn()?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", grandchild.id())?;
    stdout.flush()
}

fn echo_frames() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    loop {
        let mut header = [0_u8; 4];
        match stdin.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }
        let len = usize::try_from(u32::from_be_bytes(header))
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let mut payload = vec![0_u8; len];
        stdin.read_exact(&mut payload)?;
        stdout.write_all(&header)?;
        stdout.write_all(&payload)?;
        stdout.flush()?;
    }
}

fn print_pgid() -> io::Result<()> {
    let pgid = rustix::process::getpgrp();
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", pgid.as_raw_nonzero())?;
    stdout.flush()
}

fn escape_and_flood() -> io::Result<()> {
    let me = std::env::current_exe()?;
    let mut flood = std::process::Command::new(me)
        .arg("flood")
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let mut ready = [0_u8; 1];
    flood
        .stderr
        .take()
        .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?
        .read_exact(&mut ready)
}

fn flood() -> io::Result<()> {
    rustix::process::setsid().map_err(io::Error::from)?;
    let chunk = [b'o'; 8192];
    let mut stdout = io::stdout().lock();
    stdout.write_all(&chunk)?;
    stdout.flush()?;
    io::stderr().write_all(b"r")?;
    loop {
        stdout.write_all(&chunk)?;
    }
}

fn join_parent_group() -> io::Result<()> {
    use rustix::process::{getpgid, getppid, setpgid};

    let parent = getppid().ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    let group = getpgid(Some(parent)).map_err(io::Error::from)?;
    setpgid(None, Some(group)).map_err(io::Error::from)?;
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "moved")?;
    stderr.flush()
}

fn report_setsid() -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    match rustix::process::setsid() {
        Ok(_) => writeln!(stdout, "setsid ok")?,
        Err(error) => writeln!(stdout, "setsid failed: {error}")?,
    }
    stdout.flush()
}
