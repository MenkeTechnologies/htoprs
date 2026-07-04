//! Styled `-h` / `--help` screen — an htoprs-original presentation of the
//! command-line options in the MenkeTechnologies `tp -h` house style
//! (figlet banner, status box, `── SECTION ──` dividers, `//` comments).
//!
//! The flag names and their descriptions are htop's, verbatim from
//! `CommandLine.c`'s `printHelpFlag` (kept faithful in
//! [`crate::ported::commandline::printHelpFlag`]); only the layout differs.
//! This lives under [`crate::extensions`] because it is original chrome, not
//! a 1:1 port, so it is exempt from the `build.rs` port-purity gate.

/// htop's `VERSION` — the crate version, matching the ported flag printers.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Authorship/description line for the styled help footer. htoprs is written
/// by MenkeTechnologies; the upstream htop copyright is preserved in the
/// faithful ported [`crate::ported::commandline::printHelpFlag`].
const COPYRIGHT: &str = "htoprs by MenkeTechnologies — a Rust port of htop.";

/// The `HTOPRS` wordmark in figlet's "ANSI Shadow" font.
const BANNER: &str = "\
 ██╗  ██╗████████╗ ██████╗ ██████╗ ██████╗ ███████╗\n\
 ██║  ██║╚══██╔══╝██╔═══██╗██╔══██╗██╔══██╗██╔════╝\n\
 ███████║   ██║   ██║   ██║██████╔╝██████╔╝███████╗\n\
 ██╔══██║   ██║   ██║   ██║██╔═══╝ ██╔══██╗╚════██║\n\
 ██║  ██║   ██║   ╚██████╔╝██║     ██║  ██║███████║\n\
 ╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝     ╚═╝  ╚═╝╚══════╝";

/// Visible width of the status box interior, in terminal cells.
const BOX_INNER: usize = 54;
/// Visible width of a `── SECTION ──` divider / the bottom rule.
const RULE_WIDTH: usize = 57;

/// `  ── LABEL ───────…` padded to [`RULE_WIDTH`] cells.
fn section(label: &str) -> String {
    // "  ── " (5) + label + " " (1), then fill with `─` to RULE_WIDTH.
    let used = 5 + label.chars().count() + 1;
    format!(
        "  ── {label} {}",
        "─".repeat(RULE_WIDTH.saturating_sub(used))
    )
}

/// One option row: `  -x, --long=ARG                 // description`,
/// with the `//` comment aligned to a fixed column.
fn opt(spec: &str, desc: &str) -> String {
    format!("  {spec:<48}// {desc}")
}

/// Render the styled help screen for `htoprs -h` / `--help`.
pub fn print_help(name: &str) {
    let status = format!(" STATUS: ONLINE  // SIGNAL: ████████░░ // v{VERSION}");

    println!("{BANNER}");
    println!(" ┌{}┐", "─".repeat(BOX_INNER));
    println!(" │{status:<BOX_INNER$}│");
    println!(" └{}┘", "─".repeat(BOX_INNER));
    println!("  >> INTERACTIVE PROCESS VIEWER // FULL SPECTRUM <<");
    println!();
    println!("An interactive process viewer\n");
    println!("  USAGE: {name} [OPTIONS]\n");

    println!("{}", section("DISPLAY"));
    println!(
        "{}",
        opt("-t, --tree", "Show the tree view (can be combined with -s)")
    );
    println!(
        "{}",
        opt(
            "-s, --sort-key=COLUMN",
            "Sort by COLUMN in list view (try --sort-key=help for a list)"
        )
    );
    println!(
        "{}",
        opt(
            "-H, --highlight-changes[=DELAY]",
            "Highlight new and old processes"
        )
    );
    println!("{}", opt("-C, --no-color", "Use a monochrome color scheme"));
    println!(
        "{}",
        opt("-U, --no-unicode", "Do not use unicode but plain ASCII")
    );
    println!("{}", opt("    --no-function-bar", "Hide the function bar"));
    println!("{}", opt("    --no-meters", "Hide meters"));
    println!();

    println!("{}", section("FILTERING"));
    println!(
        "{}",
        opt(
            "-F, --filter=FILTER",
            "Show only the commands matching the given filter"
        )
    );
    println!(
        "{}",
        opt("-p, --pid=PID[,PID,PID...]", "Show only the given PIDs")
    );
    println!(
        "{}",
        opt(
            "-u, --user[=USERNAME]",
            "Show only processes for a given user (or $USER)"
        )
    );
    println!();

    println!("{}", section("BEHAVIOR"));
    println!(
        "{}",
        opt(
            "-d, --delay=DELAY",
            "Set the delay between updates, in tenths of seconds"
        )
    );
    println!(
        "{}",
        opt(
            "-n, --max-iterations=NUMBER",
            "Exit htoprs after NUMBER iterations/frame updates"
        )
    );
    println!("{}", opt("-M, --no-mouse", "Disable the mouse"));
    println!(
        "{}",
        opt(
            "    --readonly",
            "Disable all system and process changing features"
        )
    );
    println!();

    println!("{}", section("MONITOR"));
    println!("  htoprs-original capabilities — press inside the running TUI:");
    println!("{}", opt("f", "Fuzzy process finder"));
    println!("{}", opt("r", "Regex / saved-named filters"));
    println!("{}", opt("d", "Snapshot + diff the process table"));
    println!("{}", opt("o", "Export the table to JSON / CSV"));
    println!("{}", opt("A", "Threshold alerts (recolor firing rows)"));
    println!("{}", opt("G", "Braille CPU history graph"));
    println!(
        "{}",
        opt("y", "Aggregate/pivot: totals by user / command / parent (Tab cycles)")
    );
    println!(
        "{}",
        opt(":", "Command palette — fuzzy-search and run any action")
    );
    println!(
        "{}",
        opt("v", "Cycle per-PID CPU sparkline: off / column / inline graph (taller = busier)")
    );
    println!("{}", opt("b", "Cycle bar fill style (classic/gradient/solid/thin/ascii)"));
    println!();

    println!("{}", section("INFO"));
    println!("{}", opt("-h, --help", "Print this help screen"));
    println!("{}", opt("-V, --version", "Print version info"));
    println!();

    println!("{}", section("SYSTEM"));
    println!("  v{VERSION} // {COPYRIGHT}");
    println!("  Released under the GNU GPLv2+.");
    println!("  Press F1 inside {name} for online help. See 'man {name}' for more.");
    println!(" {}", "░".repeat(RULE_WIDTH - 1));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_reaches_rule_width() {
        // `─` is one cell; count chars, not bytes.
        assert_eq!(section("DISPLAY").chars().count(), RULE_WIDTH);
        assert_eq!(section("FILTERING").chars().count(), RULE_WIDTH);
    }

    #[test]
    fn opt_aligns_comment_column() {
        // "  " + 48-wide spec + "// ..." → `//` starts at column 51 (0-based 50).
        let row = opt("-t, --tree", "x");
        assert_eq!(row.find("//"), Some(50));
    }

    #[test]
    fn opt_never_swallows_long_spec() {
        // The widest real spec must survive intact and still leave a space
        // before `//` (the `:<48` pad is wider than the spec).
        let row = opt("-H, --highlight-changes[=DELAY]", "y");
        assert!(
            row.contains("-H, --highlight-changes[=DELAY]"),
            "spec preserved"
        );
        let slashes = row.find("//").expect("comment marker present");
        assert_eq!(&row[slashes - 1..slashes], " ", "space before //");
    }
}
