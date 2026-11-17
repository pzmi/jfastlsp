//! `jfl-conformance replay ...`: the conformance replay (ver §6.2), run by xtask's
//! `conformance-replay` stage from the repository root; see [`jfl_conformance::cli`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let code =
        jfl_conformance::cli::run(&args, &root, &mut std::io::stdout(), &mut std::io::stderr());
    ExitCode::from(code)
}
