//! The `jfl` binary: prints its version; the subcommands arrive with IDX-13.

fn main() {
    println!("jfl {}", env!("CARGO_PKG_VERSION"));
}
