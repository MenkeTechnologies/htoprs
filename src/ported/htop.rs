//! Port of `htop.c` — the program entry point.
//!
//! The C file is tiny: a single `program` global and a `main` that
//! delegates to `CommandLine_run`. `main` stays stubbed until
//! `CommandLine_run` lands in `commandline.rs` (see below).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

/// Port of `htop.c:14` — `const char* program = PACKAGE;`.
///
/// `PACKAGE` is the autoconf package name, `"htop"` (config.h).
pub static program: &str = "htop";

/// Port of `htop.c:16` — `int main(int argc, char** argv)`.
///
/// The C body is `return CommandLine_run(argc, argv);`. That function is
/// not yet ported (no `CommandLine_run` exists anywhere in the crate, only
/// `printVersionFlag`/`printHelpFlag` in `commandline.rs`), so emitting the
/// call would reference a nonexistent item and break the shared build.
/// Kept stubbed per the consumer-file rule until the dependency lands.
pub fn main() {
    todo!("port of htop.c:16: needs commandline::CommandLine_run (not yet ported)")
}
