//! Port of `htop.c` — the program entry point.
//!
//! The C file is tiny: a single `program` global and a `main` that delegates to
//! `CommandLine_run` (now ported in [`crate::ported::commandline`]).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::commandline::CommandLine_run;

/// Port of `htop.c:14` — `const char* program = PACKAGE;`.
///
/// `PACKAGE` is the autoconf package name, `"htop"` (config.h).
pub static program: &str = "htop";

/// Port of `htop.c:16` — `int main(int argc, char** argv)`: the C body is
/// `return CommandLine_run(argc, argv);`. Reads the process arguments (the Rust
/// analog of `argc`/`argv`), derives the invoked program name, and returns
/// [`CommandLine_run`]'s exit code. This is the faithful htop entry; the htoprs
/// binary (`src/main.rs`) adds its branded `-h`/`-V` screens on top before
/// delegating to the same `CommandLine_run`.
pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let name = args
        .first()
        .and_then(|p| p.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .unwrap_or(program);
    CommandLine_run(name, &args)
}
