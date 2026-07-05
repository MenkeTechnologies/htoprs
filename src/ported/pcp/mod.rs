//! Ported htop Performance Co-Pilot (PCP) platform modules.
//!
//! Mirrors the `linux/` / `darwin/` / `dragonflybsd/` platform sub-trees: one
//! Rust module per PCP C file (`pcp/*.c`). PCP is htop's `--enable-pcp`
//! (`HAVE_PCP`) build variant, so the whole tree is behind the `pcp` cargo
//! feature (off by default). The libpcp/pmapi surface is confined to `Metric`
//! (the C `pcp/Metric.c` FFI wrapper layer) and hand-declared in `extern`
//! blocks — the DragonFly-kvm precedent; the FFI-free leaf modules (starting
//! with `PCPProcess`) reuse the shared `Process`/`Row`/`Object` object model
//! and compile under `--features pcp`.

pub mod metric;
pub mod pcpdynamiccolumn;
pub mod pcpmachine;
pub mod pcpprocess;
pub mod pcpprocesstable;
pub mod platform;
pub mod pmapi;
