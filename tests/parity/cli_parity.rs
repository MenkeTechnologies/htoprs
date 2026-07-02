//! CLI parity for the flags htoprs has ported so far, checked against the
//! reference htop 3.5.x. htoprs's `CommandLine.c` help/version printers must
//! reproduce htop's output exactly (modulo the rebrand), so these compare the
//! two binaries' output byte-for-byte.

use super::harness::{assert_stdout_parity, canon, htoprs_bin, ref_available, run};

mod help {
    use super::*;

    #[test]
    fn help_long() {
        assert_stdout_parity(&["--help"]);
    }

    #[test]
    fn help_short() {
        assert_stdout_parity(&["-h"]);
    }

    // Both spellings must produce identical output on the htoprs side.
    #[test]
    fn help_short_equals_long() {
        let long = run(&htoprs_bin(), &["--help"]).stdout;
        let short = run(&htoprs_bin(), &["-h"]).stdout;
        assert_eq!(long, short, "-h and --help differ on the htoprs side");
    }
}

mod version {
    use super::*;

    #[test]
    fn version_long() {
        assert_stdout_parity(&["--version"]);
    }

    #[test]
    fn version_short() {
        assert_stdout_parity(&["-V"]);
    }

    #[test]
    fn version_short_equals_long() {
        let long = run(&htoprs_bin(), &["--version"]).stdout;
        let short = run(&htoprs_bin(), &["-V"]).stdout;
        assert_eq!(long, short, "-V and --version differ on the htoprs side");
    }

    // The version banner must be a single `<name> <semver>` line, matching
    // htop's format even though the number itself differs (0.1.0 vs 3.5.1).
    #[test]
    fn version_banner_format() {
        if ref_available().is_none() {
            return;
        }
        let r = run(&htoprs_bin(), &["--version"]);
        let canon = canon(&r.stdout);
        assert_eq!(
            canon.trim_end(),
            "htop VERSION",
            "unexpected version banner: {:?}",
            r.stdout
        );
    }
}

// Not-yet-ported surfaces, pinned as documented gaps (run with `--ignored`).
// These will go green as the `CommandLine_parseArgs` getopt switch is ported;
// each is the exact htop behavior htoprs must eventually reproduce.
mod gaps {
    use super::*;

    // htop treats an unknown flag as a getopt error (usage to stderr, exit 1);
    // htoprs currently prints a placeholder "being ported" line instead.
    #[test]
    #[ignore = "CommandLine_parseArgs getopt_long switch not ported yet"]
    fn unknown_flag_getopt_error() {
        assert_stdout_parity(&["--definitely-not-a-flag"]);
    }

    // `--sort-key=help` prints the available sort columns and exits 0; needs the
    // ProcessTable/columns port.
    #[test]
    #[ignore = "--sort-key=help column listing not ported yet"]
    fn sort_key_help_listing() {
        assert_stdout_parity(&["--sort-key=help"]);
    }
}
