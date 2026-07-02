//! Aggregated parity-test binary for htoprs — the whole parity suite as ONE
//! target, mirroring the zshrs parity harness. It runs the reference `htop`
//! (the C original, 3.5.x) and `htoprs` on the same inputs and diffs the
//! output byte-for-byte, modulo the deliberate rebrand (see `harness.rs`).
//!
//!     cargo test --test parity                 # run the whole suite
//!     cargo test --test parity cli             # filter to CLI cases
//!     cargo test --test parity -- --ignored    # documented not-yet-ported gaps
//!
//! Cargo does NOT auto-discover files in this subdirectory; the `[[test]]`
//! stanza in `Cargo.toml` makes this file the single discovered target. Add a
//! parity area by dropping a `*_parity.rs` file here and adding one `mod` line
//! below.
//!
//! Per the MenkeTechnologies endgame rule (as in zshrs/ztmux), this suite is
//! part of the immune system once green: every newly-ported CLI surface adds a
//! case here first — confirm it fails, then fix the port until it passes.

#![allow(dead_code)]

mod harness;

mod cli_parity;
mod xutils_parity;
