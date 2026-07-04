//! Styled `-h` / `--help` screen — an htoprs-original presentation of the
//! command-line options in the MenkeTechnologies `tp -h` house style
//! (figlet banner, status box, `── SECTION ──` dividers, `//` comments).
//!
//! The flag names and their descriptions are htop's, verbatim from
//! `CommandLine.c`'s `printHelpFlag` (kept faithful in
//! [`crate::ported::commandline::printHelpFlag`]); only the layout differs.
//! This lives under [`crate::extensions`] because it is original chrome, not
//! a 1:1 port, so it is exempt from the `build.rs` port-purity gate.
//!
//! Color matches the `tp`/`temprs` house palette — a cyan→magenta→red banner
//! gradient, cyan status box / section dividers / rules, a magenta tagline,
//! a yellow `USAGE:` label, bold flag specs, and green `//` comment markers.
//! Like `tp`, the ANSI is emitted only when stdout is a terminal (and
//! `NO_COLOR` is unset); piped/redirected output stays plain.

use std::io::IsTerminal;

/// htop's `VERSION` — the crate version, matching the ported flag printers.
const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── house-style SGR colors (`tp`/`temprs` palette) ──────────────────────────
/// Cyan — banner top, status box, section dividers, rules.
const CYAN: &str = "36";
/// Magenta — banner middle, the `>> … <<` tagline, the version line.
const MAGENTA: &str = "35";
/// Red — banner bottom.
const RED: &str = "31";
/// Yellow — the `USAGE:` label and the copyright line.
const YELLOW: &str = "33";

/// Wrap `text` in an SGR color (`\e[{code}m … \e[0m`) when `color` is on;
/// otherwise return it unchanged. The reset restores the terminal default so
/// each colored span is self-contained.
fn paint(color: bool, code: &str, text: &str) -> String {
    if color {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

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
/// with the `//` comment aligned to a fixed column. When `color` is on the
/// spec is bold and the `//` marker green (the `tp` house style); the visible
/// layout is identical either way, since the padding is applied to the spec
/// before the (zero-width) SGR codes are wrapped around it.
fn opt(color: bool, spec: &str, desc: &str) -> String {
    if color {
        format!("  \x1b[1m{spec:<48}\x1b[0m\x1b[32m//\x1b[0m {desc}")
    } else {
        format!("  {spec:<48}// {desc}")
    }
}

/// Render the styled help screen for `htoprs -h` / `--help`. Colors the
/// house-style chrome when stdout is a terminal and `NO_COLOR` is unset;
/// otherwise emits the same layout in plain text (so pipes/files stay clean).
pub fn print_help(name: &str) {
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    print_help_to(name, color);
}

/// The body of [`print_help`], parameterized on whether to emit color, so the
/// colored and plain renderings are both exercisable in tests.
fn print_help_to(name: &str, color: bool) {
    // Cyan → magenta → red banner gradient (two lines each), matching `tp`.
    let banner_codes = [CYAN, CYAN, MAGENTA, MAGENTA, RED, RED];
    for (line, code) in BANNER.lines().zip(banner_codes) {
        println!("{}", paint(color, code, line));
    }

    // Status box (cyan borders + text) and the magenta tagline.
    let status = format!(" STATUS: ONLINE  // SIGNAL: ████████░░ // v{VERSION}");
    println!(
        "{}",
        paint(color, CYAN, &format!(" ┌{}┐", "─".repeat(BOX_INNER)))
    );
    println!(
        "{}",
        paint(color, CYAN, &format!(" │{status:<BOX_INNER$}│"))
    );
    println!(
        "{}",
        paint(color, CYAN, &format!(" └{}┘", "─".repeat(BOX_INNER)))
    );
    println!(
        "{}",
        paint(
            color,
            MAGENTA,
            "  >> INTERACTIVE PROCESS VIEWER // FULL SPECTRUM <<"
        )
    );
    println!();
    println!("An interactive process viewer\n");
    // Yellow `USAGE:` label + bold program name (the `tp` usage style).
    let usage_label = paint(color, YELLOW, "  USAGE:");
    let usage_name = if color {
        format!("\x1b[1m{name}\x1b[0m")
    } else {
        name.to_string()
    };
    println!("{usage_label} {usage_name} [OPTIONS]\n");

    let sect = |label: &str| paint(color, CYAN, &section(label));

    println!("{}", sect("DISPLAY"));
    println!(
        "{}",
        opt(
            color,
            "-t, --tree",
            "Show the tree view (can be combined with -s)"
        )
    );
    println!(
        "{}",
        opt(
            color,
            "-s, --sort-key=COLUMN",
            "Sort by COLUMN in list view (try --sort-key=help for a list)"
        )
    );
    println!(
        "{}",
        opt(
            color,
            "-H, --highlight-changes[=DELAY]",
            "Highlight new and old processes"
        )
    );
    println!(
        "{}",
        opt(color, "-C, --no-color", "Use a monochrome color scheme")
    );
    println!(
        "{}",
        opt(
            color,
            "-U, --no-unicode",
            "Do not use unicode but plain ASCII"
        )
    );
    println!(
        "{}",
        opt(color, "    --no-function-bar", "Hide the function bar")
    );
    println!("{}", opt(color, "    --no-meters", "Hide meters"));
    println!();

    println!("{}", sect("FILTERING"));
    println!(
        "{}",
        opt(
            color,
            "-F, --filter=FILTER",
            "Show only the commands matching the given filter"
        )
    );
    println!(
        "{}",
        opt(
            color,
            "-p, --pid=PID[,PID,PID...]",
            "Show only the given PIDs"
        )
    );
    println!(
        "{}",
        opt(
            color,
            "-u, --user[=USERNAME]",
            "Show only processes for a given user (or $USER)"
        )
    );
    println!();

    println!("{}", sect("BEHAVIOR"));
    println!(
        "{}",
        opt(
            color,
            "-d, --delay=DELAY",
            "Set the delay between updates, in tenths of seconds"
        )
    );
    println!(
        "{}",
        opt(
            color,
            "-n, --max-iterations=NUMBER",
            "Exit htoprs after NUMBER iterations/frame updates"
        )
    );
    println!("{}", opt(color, "-M, --no-mouse", "Disable the mouse"));
    println!(
        "{}",
        opt(
            color,
            "    --readonly",
            "Disable all system and process changing features"
        )
    );
    println!();

    println!("{}", sect("MONITOR"));
    println!("  htoprs-original capabilities — press inside the running TUI:");
    println!("{}", opt(color, "f", "Fuzzy process finder"));
    println!("{}", opt(color, "r", "Regex / saved-named filters"));
    println!("{}", opt(color, "d", "Snapshot + diff the process table"));
    println!("{}", opt(color, "o", "Export the table to JSON / CSV"));
    println!(
        "{}",
        opt(color, "A", "Threshold alerts (recolor firing rows)")
    );
    println!("{}", opt(color, "G", "Braille CPU history graph"));
    println!(
        "{}",
        opt(
            color,
            "y",
            "Aggregate/pivot: totals by user / command / parent (Tab cycles)"
        )
    );
    println!(
        "{}",
        opt(
            color,
            ":",
            "Command palette — fuzzy-search and run any action"
        )
    );
    println!(
        "{}",
        opt(
            color,
            "v",
            "Cycle per-PID CPU sparkline: off / column / inline graph (taller = busier)"
        )
    );
    println!(
        "{}",
        opt(
            color,
            "b",
            "Cycle bar fill style (classic/gradient/solid/thin/ascii)"
        )
    );
    println!();

    println!("{}", sect("INFO"));
    println!("{}", opt(color, "-h, --help", "Print this help screen"));
    println!("{}", opt(color, "-V, --version", "Print version info"));
    println!();

    println!("{}", sect("SYSTEM"));
    println!(
        "{} // {}",
        paint(color, MAGENTA, &format!("  v{VERSION}")),
        paint(color, YELLOW, COPYRIGHT)
    );
    println!("  Released under the GNU GPLv2+.");
    println!("  Press F1 inside {name} for online help. See 'man {name}' for more.");
    println!(
        "{}",
        paint(color, CYAN, &format!(" {}", "░".repeat(RULE_WIDTH - 1)))
    );
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
        let row = opt(false, "-t, --tree", "x");
        assert_eq!(row.find("//"), Some(50));
    }

    #[test]
    fn opt_never_swallows_long_spec() {
        // The widest real spec must survive intact and still leave a space
        // before `//` (the `:<48` pad is wider than the spec).
        let row = opt(false, "-H, --highlight-changes[=DELAY]", "y");
        assert!(
            row.contains("-H, --highlight-changes[=DELAY]"),
            "spec preserved"
        );
        let slashes = row.find("//").expect("comment marker present");
        assert_eq!(&row[slashes - 1..slashes], " ", "space before //");
    }

    #[test]
    fn paint_wraps_only_when_color_on() {
        assert_eq!(paint(true, CYAN, "x"), "\x1b[36mx\x1b[0m");
        assert_eq!(paint(false, CYAN, "x"), "x");
    }

    #[test]
    fn colored_opt_bolds_spec_and_greens_marker_without_shifting_layout() {
        let plain = opt(false, "-t, --tree", "desc");
        let colored = opt(true, "-t, --tree", "desc");
        // The colored row carries the bold spec and green `//`.
        assert!(colored.contains("\x1b[1m-t, --tree"), "spec bolded");
        assert!(colored.contains("\x1b[32m//\x1b[0m"), "marker greened");
        // Stripping every SGR from the colored row yields the plain row, so the
        // visible layout (and the `//` column) is identical either way.
        let re = |s: &str| {
            let mut out = String::new();
            let mut bytes = s.bytes().peekable();
            while let Some(b) = bytes.next() {
                if b == 0x1b {
                    // Skip `[` … final byte of the CSI.
                    while let Some(&n) = bytes.peek() {
                        bytes.next();
                        if n.is_ascii_alphabetic() {
                            break;
                        }
                    }
                } else {
                    out.push(b as char);
                }
            }
            out
        };
        assert_eq!(re(&colored), plain);
    }
}
