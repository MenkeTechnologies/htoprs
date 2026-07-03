//! Partial port of `UnsupportedMachine.c` — the fallback per-host `Machine`.
//!
//! Ported here (operate on the base [`Machine`], so no unported substrate
//! is needed):
//! - `Machine_isCPUonline` (`UnsupportedMachine.c:36`)
//! - `Machine_getCPUPhysicalCoreID` (`UnsupportedMachine.c:56`)
//! - `Machine_getCPUThreadIndex` (`UnsupportedMachine.c:62`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `Machine_new` / `Machine_delete` need `Machine_init` / `Machine_done`
//!   (still stubs in `machine.rs`) and the `UnsupportedMachine` struct.
//! - `Machine_scan` writes `UnsupportedMachine.usedMem` / `.cachedMem`, so it
//!   needs that struct modeled.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;

/// TODO: port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// from `UnsupportedMachine.c:18`. Blocked: needs `Machine_init` (stub in
/// `machine.rs`) and the `UnsupportedMachine` struct.
pub fn Machine_new() {
    todo!("port of UnsupportedMachine.c:18")
}

/// TODO: port of `void Machine_delete(Machine* super)` from
/// `UnsupportedMachine.c:30`. Blocked: needs `Machine_done` (stub in
/// `machine.rs`) and the `UnsupportedMachine` struct.
pub fn Machine_delete() {
    todo!("port of UnsupportedMachine.c:30")
}

/// Port of `bool Machine_isCPUonline(const Machine* host, unsigned int id)`
/// (`UnsupportedMachine.c:36`). The fallback platform always reports online.
pub fn Machine_isCPUonline(host: &Machine, id: u32) -> bool {
    debug_assert!(id < host.existingCPUs);
    true
}

/// TODO: port of `void Machine_scan(Machine* super)` from
/// `UnsupportedMachine.c:44`. Blocked: writes `UnsupportedMachine.usedMem` /
/// `.cachedMem`, so it needs that struct modeled.
pub fn Machine_scan() {
    todo!("port of UnsupportedMachine.c:44")
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
