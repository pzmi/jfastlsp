//! `jfl-jdtls-fake-lsp`: the scripted fake language server of the tests, over stdio
//! (see `jfl_jdtls::fake`).

use std::io::{self, BufReader};
use std::process::ExitCode;

use jfl_jdtls::fake::{FakeInfo, serve};

fn main() -> ExitCode {
    let info = FakeInfo {
        argv: std::env::args().skip(1).collect(),
        cwd: std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default(),
        pid: std::process::id(),
    };
    let mut reader = BufReader::new(io::stdin().lock());
    match serve(&mut reader, &mut io::stdout().lock(), &info) {
        Ok(0) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
