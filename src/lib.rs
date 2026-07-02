//! htoprs — a faithful Rust port of htop.
//!
//! The C source at `~/forkedRepos/htop` (v3.5.1) is the spec. Every
//! function under [`ported`] ports a specific htop C function, cited
//! by `<File>.c:<line>` in its doc comment. The port-purity gate in
//! `build.rs` rejects any free `fn` under `src/ported/` whose name
//! has no counterpart in the htop C source (snapshotted at
//! `tests/data/htop_c_fn_names.txt`).

pub mod ported;
