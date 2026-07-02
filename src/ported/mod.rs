//! Faithful ports of htop C source files.
//!
//! One Rust module per C file (module name = C file stem, lowercased).
//! Each `fn` here ports a specific htop C function and cites its
//! origin (`<File>.c:<line>`) in the doc comment. See `build.rs` for
//! the port-purity gate that enforces this.

pub mod commandline;
pub mod hashtable;
pub mod meter;
pub mod vector;
pub mod xutils;
