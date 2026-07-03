//! CLI parity for the flags htoprs has ported so far, checked against the
//! reference htop 3.5.x. htoprs's `CommandLine.c` version printer must
//! reproduce htop's output exactly (modulo the rebrand), so `version` compares
//! the two binaries byte-for-byte. `-h`/`--help` is deliberately NOT a parity
//! surface: `main.rs` routes it to the branded `extensions::help` printer, an
//! intentional divergence from htop; the `help` module pins that divergence
//! (branded output present, and it differs from htop) rather than checking
//! parity.

use super::harness::{assert_stdout_parity, canon, htoprs_bin, ref_available, run};

mod help {
    use super::*;

    // htoprs `-h`/`--help` is intentionally routed (main.rs) to the branded
    // `extensions::help` printer — an ANSI-Shadow banner + styled sections that
    // diverge from htop's plain help by design (and must, since htop's help
    // carries an upstream-specific copyright line htoprs rebrands). Byte-parity
    // with htop therefore does NOT apply here. Rather than ignore that fact,
    // these two tests pin the divergence: the branded help is present and, when
    // the reference htop is available, deliberately differs from it. The
    // faithful plain port lives on in `commandline::printHelpFlag`.

    /// The branded help must carry the banner/usage and the monitoring hotkeys,
    /// and must NOT be htop's plain help.
    fn assert_branded_help(args: &[&str]) {
        let out = run(&htoprs_bin(), args).stdout;
        assert!(out.contains("USAGE:"), "branded help shows a usage line: {out:?}");
        assert!(out.contains("MONITOR"), "branded help lists the monitoring hotkeys");
        if let Some(refbin) = ref_available() {
            let h = run(&refbin, args).stdout;
            assert_ne!(
                canon(&h),
                canon(&out),
                "htoprs `{}` is branded and must diverge from htop's plain help",
                args.join(" "),
            );
        }
    }

    #[test]
    fn help_long() {
        assert_branded_help(&["--help"]);
    }

    #[test]
    fn help_short() {
        assert_branded_help(&["-h"]);
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

// Behaviors driven by the ported `CommandLine.c` `parseArguments` getopt_long
// switch: an unknown flag exits 1 with empty stdout, and `--sort-key=help`
// lists the sort columns — both byte-parity-checked against the reference htop.
mod gaps {
    use super::*;

    // htop treats an unknown flag as a getopt error (message to stderr, exit 1,
    // empty stdout); the ported `parseArguments` getopt_long switch reproduces
    // the empty stdout and the exit code.
    #[test]
    fn unknown_flag_getopt_error() {
        assert_stdout_parity(&["--definitely-not-a-flag"]);
    }

    // `--sort-key=help` prints the available sort columns (from the ported
    // darwin `Process_fields[]` table) and exits 0.
    #[test]
    fn sort_key_help_listing() {
        assert_stdout_parity(&["--sort-key=help"]);
    }
}
