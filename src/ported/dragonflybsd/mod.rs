//! Ported htop DragonFly BSD platform modules.
//!
//! Mirrors the `linux/` and `darwin/` platform sub-trees: one Rust module per
//! DragonFly BSD C file. `DragonFlyBSDProcess` is pure (no `libkvm`/`sysctl`)
//! and reuses the shared `Process`/`Row` object model and the `ProcessClass`/
//! `RowClass` vtables; the `Machine`/`ProcessTable`/`Platform` modules (which
//! call `kvm_*`/`sysctl`) are added as they are ported and gate their
//! BSD-specific syscalls behind `#[cfg(target_os = "dragonfly")]`.

pub mod dragonflybsdmachine;
pub mod dragonflybsdprocess;
pub mod dragonflybsdprocesstable;
