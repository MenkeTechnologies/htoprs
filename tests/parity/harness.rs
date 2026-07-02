//! Shared parity harness: run the reference `htop` (the C original htoprs is
//! ported from, version 3.5.x) and `htoprs` on the same arguments and compare
//! their output byte-for-byte, modulo the deliberate rebrand (program name
//! `htoprs`→`htop` and the version banner).
//!
//! This mirrors the zshrs parity harness — run the reference and the port on
//! the same input, diff the results — and, like it, SKIPS (stays green) when a
//! matching reference is unavailable, so CI without htop passes while a dev box
//! with htop 3.5.x runs the real comparison. Override the reference binary with
//! `HTOP_REF=/path/to/htop`.
//!
//! htoprs is an early-stage port: only the `CommandLine.c` `-V`/`--version` and
//! `-h`/`--help` printers are wired today, so those are the deterministic
//! surfaces compared here. As more of htop's non-interactive CLI is ported
//! (the `CommandLine_parseArgs` getopt switch, `--sort-key=help`, etc.), add a
//! `*_parity.rs` file next to this one and register it in `main.rs`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The htop series htoprs is ported from. The reference must match on
/// major.minor: htop's help text and flag set change across minor releases, so
/// comparing against a different series would produce false divergences (the
/// same version-pinning caution the ztmux parity suite documents).
pub const REF_HTOP_SERIES: &str = "3.5";

pub struct Run {
    pub stdout: String,
    pub stderr: String,
    pub exit: i32,
}

/// The htoprs binary under test (Cargo exports this to integration tests).
pub fn htoprs_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_htoprs") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/htoprs")
}

/// Locate a reference htop binary (env override first, then common prefixes).
fn htop_ref_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HTOP_REF") {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    [
        "/opt/homebrew/bin/htop",
        "/usr/local/bin/htop",
        "/usr/bin/htop",
        "/bin/htop",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}

/// Return the reference htop path only if it exists AND is the ported 3.5
/// series; otherwise `None` so callers skip the comparison.
pub fn ref_available() -> Option<PathBuf> {
    let bin = htop_ref_path()?;
    let out = Command::new(&bin).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout);
    let series = v.trim().strip_prefix("htop ")?;
    series.starts_with(REF_HTOP_SERIES).then_some(bin)
}

pub fn run(bin: &Path, args: &[&str]) -> Run {
    let o = Command::new(bin).args(args).output().expect("spawn binary");
    Run {
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        exit: o.status.code().unwrap_or(-1),
    }
}

/// Canonicalize output so the deliberate rebrand is not counted as a diff:
/// the program name `htoprs`→`htop`, and any `htop <semver>` banner line →
/// `htop VERSION` (htoprs is 0.1.0, htop is 3.5.1 — the numbers differ forever
/// and are not a structural-parity concern; the format and everything else is).
pub fn canon(s: &str) -> String {
    let s = s.replace("htoprs", "htop");
    s.lines()
        .map(|l| match l.strip_prefix("htop ") {
            Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => {
                "htop VERSION".to_string()
            }
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Assert htoprs and the reference htop produce identical stdout + exit code
/// for `args`, after rebrand canonicalization. No-op when no matching reference
/// is present.
pub fn assert_stdout_parity(args: &[&str]) {
    let Some(refbin) = ref_available() else {
        return;
    };
    let r = run(&htoprs_bin(), args);
    let h = run(&refbin, args);
    assert_eq!(
        canon(&h.stdout),
        canon(&r.stdout),
        "stdout divergence for `{}`:\n--- htop (ref) ---\n{}\n--- htoprs ---\n{}",
        args.join(" "),
        h.stdout,
        r.stdout,
    );
    assert_eq!(
        h.exit,
        r.exit,
        "exit-code divergence for `{}`: htop={} htoprs={}",
        args.join(" "),
        h.exit,
        r.exit,
    );
}
