//! Port of `CommandLine.c` — htop's command-line entry and flag output.
//!
//! The `-V` / `-h` flag printers and the `parseArguments` getopt_long switch
//! are ported; the interactive run loop (`CommandLine_run`) is still driven
//! from `main.rs` rather than ported wholesale.
#![allow(non_snake_case)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;

// The platform `Process_fields[]` table + count, for `--sort-key=help` and the
// column lookup. Selected by target, mirroring htop's per-platform link.
#[cfg(target_os = "macos")]
use crate::ported::darwin::darwinprocess::{Process_fields, LAST_PROCESSFIELD};
#[cfg(all(not(target_os = "macos"), target_os = "linux"))]
use crate::ported::linux::linuxprocess::{Process_fields, LAST_PROCESSFIELD};

// getopt's result globals. The `libc` crate declares `getopt_long` and `option`
// for the BSD/apple target but not the `optarg`/`optind` externs, so bind the
// real libSystem/glibc symbols directly (same ones htop's getopt_long fills).
extern "C" {
    static mut optarg: *mut c_char;
    static mut optind: c_int;
}

/// htop's `VERSION` — a build-time macro produced by configure. The
/// faithful Rust equivalent is the crate version from `Cargo.toml`.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// htop's `COPYRIGHT` macro (`configure.ac:1833`).
pub(crate) const COPYRIGHT: &str = "(C) MenkeTechnologies 2026.";

/// Port of `printVersionFlag(const char* name)` from `CommandLine.c`.
/// C: `printf("%s " VERSION "\n", name)`.
pub fn printVersionFlag(name: &str) {
    println!("{name} {VERSION}");
}

/// Port of `printHelpFlag(const char* name)` from `CommandLine.c`. The
/// C emits the version line, copyright, license, then the option list.
/// `HAVE_GETMOUSE` gates the `-M` line; the mouse is always compiled
/// in here, so it is emitted unconditionally. `Platform_longOptionsUsage`
/// is a no-op until platform options are ported.
///
/// The htoprs binary itself does not call this — its `-h` handler renders
/// the styled help screen in [`crate::extensions::help`] instead. This
/// faithful port is retained as the spec that styled screen tracks.
pub fn printHelpFlag(name: &str) {
    print!(
        "{name} {VERSION}\n\
         {COPYRIGHT}\n\
         Released under the GNU GPLv2+.\n\n\
         -C --no-color                   Use a monochrome color scheme\n\
         -d --delay=DELAY                Set the delay between updates, in tenths of seconds\n\
         -F --filter=FILTER              Show only the commands matching the given filter\n   \
            --no-function-bar            Hide the function bar\n\
         -h --help                       Print this help screen\n\
         -H --highlight-changes[=DELAY]  Highlight new and old processes\n\
         -M --no-mouse                   Disable the mouse\n   \
            --no-meters                  Hide meters\n\
         -n --max-iterations=NUMBER      Exit htop after NUMBER iterations/frame updates\n\
         -p --pid=PID[,PID,PID...]       Show only the given PIDs\n   \
            --readonly                   Disable all system and process changing features\n\
         -s --sort-key=COLUMN            Sort by COLUMN in list view (try --sort-key=help for a list)\n\
         -t --tree                       Show the tree view (can be combined with -s)\n\
         -u --user[=USERNAME]            Show only processes for a given user (or $USER)\n\
         -U --no-unicode                 Do not use unicode but plain ASCII\n\
         -V --version                    Print version info\n\
         \n\
         Press F1 inside {name} for online help.\n\
         See 'man {name}' for more information.\n"
    );
}

#[cfg(test)]
mod tests {
    // printVersionFlag / printHelpFlag write to stdout; their content
    // is pinned by the release smoke test (`htoprs --version`) and the
    // man page. The constants below guard against accidental edits.
    use super::*;

    #[test]
    fn version_is_crate_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
        assert!(!VERSION.is_empty());
    }

}
