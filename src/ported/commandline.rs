//! Port of `CommandLine.c` — htop's command-line entry and flag output.
//!
//! Only the `-V` / `-h` flag printers are ported so far; the full
//! `CommandLine_parseArgs` getopt_long switch and the interactive run
//! loop are not yet ported.
#![allow(non_snake_case)]

/// htop's `VERSION` — a build-time macro produced by configure. The
/// faithful Rust equivalent is the crate version from `Cargo.toml`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// htop's `COPYRIGHT` macro (`configure.ac:1833`).
const COPYRIGHT: &str = "(C) 2004-2019 Hisham Muhammad. (C) 2020-2026 htop dev team.";

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

    #[test]
    fn copyright_credits_htop_authors() {
        assert!(COPYRIGHT.contains("Hisham Muhammad"));
        assert!(COPYRIGHT.contains("htop dev team"));
    }
}
