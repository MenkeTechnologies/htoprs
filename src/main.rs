//! htoprs entry point.
//!
//! The interactive TUI, process table, meters, and platform data
//! collection are not yet ported, so the `-V` / `-h` flags are handled
//! here directly (via the ported `CommandLine.c` printers) and any
//! other invocation reports that the interface is not yet available.
//! The full `CommandLine_parseArgs` getopt_long switch is a later port.

use htoprs::ported::commandline;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // htop's `printVersionFlag`/`printHelpFlag` take the program name
    // (argv[0] basename); default to "htoprs" when unavailable.
    let name = args
        .first()
        .and_then(|p| p.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("htoprs");

    for arg in &args[1..] {
        match arg.as_str() {
            "-V" | "--version" => {
                commandline::printVersionFlag(name);
                return;
            }
            "-h" | "--help" => {
                commandline::printHelpFlag(name);
                return;
            }
            _ => {}
        }
    }

    eprintln!("htoprs: port in progress — the interactive interface is not yet available");
    eprintln!("htoprs: run 'htoprs --help' for the command-line options being ported");
    std::process::exit(1);
}
