//! Partial port of `UnsupportedMachine.c` — the fallback per-host `Machine`.
//!
//! Ported here:
//! - [`Machine_delete`] (`UnsupportedMachine.c:30`)
//! - `Machine_isCPUonline` (`UnsupportedMachine.c:36`)
//! - [`Machine_scan`] (`UnsupportedMachine.c:44`)
//! - `Machine_getCPUPhysicalCoreID` (`UnsupportedMachine.c:56`)
//! - `Machine_getCPUThreadIndex` (`UnsupportedMachine.c:62`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `Machine_new` calls `Machine_init`, which `machine.rs` defines only under
//!   `#[cfg(target_os = "macos")]`. This module is compiled on *all* targets
//!   (the fallback platform is not cfg-gated), so calling the macos-only
//!   `Machine_init` here would break every non-macos build. The native darwin
//!   `Machine_new` port works because the darwin module is itself macos-gated.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::{Machine, Machine_done, Machine_init};

/// Port of `typedef struct UnsupportedMachine_` (`UnsupportedMachine.h`). The
/// fallback host embeds the base [`Machine`] and adds the two scalar memory
/// fields the fallback `Machine_scan` zeroes (`memory_t` == `u64`).
/// `#[repr(C)]` keeps `super_` at offset 0 so the C `(UnsupportedMachine*)super`
/// downcast is sound.
#[repr(C)]
pub struct UnsupportedMachine {
    /// C `Machine super` — the embedded base machine.
    pub super_: Machine,
    /// C `memory_t usedMem`.
    pub usedMem: u64,
    /// C `memory_t cachedMem`.
    pub cachedMem: u64,
}

/// Port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// (`UnsupportedMachine.c:18`). Allocates the fallback host, runs the base
/// [`Machine_init`], and reports a single always-online CPU. Returns
/// `Box<UnsupportedMachine>`; the caller derives `*mut Machine` from
/// `&mut box.super_` (the C returns `&this->super`).
pub fn Machine_new(usersTable: Option<usize>, userId: u32) -> Box<UnsupportedMachine> {
    let mut this = Box::new(UnsupportedMachine {
        super_: Machine::default(),
        usedMem: 0,
        cachedMem: 0,
    });

    Machine_init(&mut this.super_, usersTable, userId);

    this.super_.existingCPUs = 1;
    this.super_.activeCPUs = 1;

    this
}

/// Port of `void Machine_delete(Machine* super)` (`UnsupportedMachine.c:30`).
/// C casts back to `UnsupportedMachine*`, runs [`Machine_done`] on the base,
/// then `free(this)`. The owning `Box<UnsupportedMachine>` is consumed:
/// `Machine_done` tears the base down and the `Box` drop reclaims the
/// allocation (the C `free`).
pub fn Machine_delete(mut this: Box<UnsupportedMachine>) {
    Machine_done(&mut this.super_);
    // free(this) — the Box drop reclaims the allocation.
}

/// Port of `bool Machine_isCPUonline(const Machine* host, unsigned int id)`
/// (`UnsupportedMachine.c:36`). The fallback platform always reports online.
pub fn Machine_isCPUonline(host: &Machine, id: u32) -> bool {
    debug_assert!(id < host.existingCPUs);
    true
}

/// Port of `void Machine_scan(Machine* super)` (`UnsupportedMachine.c:44`). The
/// fallback platform reports one CPU and no memory/swap — every figure is
/// hard-zeroed each scan.
pub fn Machine_scan(this: &mut UnsupportedMachine) {
    this.super_.existingCPUs = 1;
    this.super_.activeCPUs = 1;
    this.super_.totalSwap = 0;
    this.super_.usedSwap = 0;
    this.super_.cachedSwap = 0;
    this.super_.totalMem = 0;
    this.usedMem = 0;
    this.cachedMem = 0;
}

/// Port of `int Machine_getCPUPhysicalCoreID(const Machine* host, unsigned int
/// id)` (`UnsupportedMachine.c:56`).
pub fn Machine_getCPUPhysicalCoreID(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    id as i32
}

/// Port of `int Machine_getCPUThreadIndex(const Machine* host, unsigned int
/// id)` (`UnsupportedMachine.c:62`).
pub fn Machine_getCPUThreadIndex(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    0
}
