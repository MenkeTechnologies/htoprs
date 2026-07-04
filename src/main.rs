//! htoprs entry point.
//!
//! Thin binary wrapper: htoprs routes `-h`/`-V` to its branded help/version
//! screens (an intentional divergence from htop's plain printers), then delegates
//! to the ported [`htoprs::ported::commandline::CommandLine_run`] — the single
//! startup path shared with the faithful [`htoprs::ported::htop::main`], which
//! assembles the runtime object graph and drives [`ScreenManager_run`].
//!
//! [`ScreenManager_run`]: htoprs::ported::screenmanager::ScreenManager_run

use htoprs::ported::commandline::CommandLine_run;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let name = args
        .first()
        .and_then(|p| p.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("htoprs");

    // htoprs routes -h/--help and -V/--version to the branded help/version
    // screens (an intentional divergence from htop's plain printers), so
    // short-circuit those before delegating to the shared CommandLine_run.
    for arg in &args[1..] {
        match arg.as_str() {
            "-V" | "--version" => {
                htoprs::ported::commandline::printVersionFlag(name);
                return;
            }
            "-h" | "--help" => {
                htoprs::extensions::help::print_help(name);
                return;
            }
            _ => {}
        }
    }

    // Everything else: the ported entry (getopt parse + assemble + run loop).
    std::process::exit(CommandLine_run(name, &args));
}
