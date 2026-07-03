//! Partial port of `FreeBSDMachine.c` — the FreeBSD per-host `Machine`.
//!
//! Ported here (operate on the base [`Machine`], so no unported substrate
//! is needed):
//! - `Machine_isCPUonline` (`FreeBSDMachine.c:397`)
//! - `Machine_getCPUPhysicalCoreID` (`FreeBSDMachine.c:406`)
//! - `Machine_getCPUThreadIndex` (`FreeBSDMachine.c:412`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - the `FreeBSDMachine` struct plus `FreeBSDMachine_scanCPU` /
//!   `_scanMemoryInfo` need the per-CPU / VM `sysctl` scan modeled and the
//!   embedded `ZfsArcStats` / `kvm_t` handle.
//! - `Machine_scan` / `Machine_new` / `Machine_delete` additionally need
//!   `Machine_init` / `Machine_done` (still stubs in `machine.rs`),
//!   `kvm_open`/`kvm_close` and `openzfs_sysctl_*` FFI.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;

/// TODO: port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// from `FreeBSDMachine.c:53`. Blocked: needs `Machine_init` (stub in
/// `machine.rs`), the `FreeBSDMachine` struct, `kvm_open` and
/// `openzfs_sysctl_init` FFI.
pub fn Machine_new() {
    todo!("port of FreeBSDMachine.c:53")
}

/// TODO: port of `void Machine_delete(Machine* super)` from
/// `FreeBSDMachine.c:147`. Blocked: needs `Machine_done` (stub in
/// `machine.rs`), the `FreeBSDMachine` struct and `kvm_close` FFI.
pub fn Machine_delete() {
    todo!("port of FreeBSDMachine.c:147")
}

/// TODO: port of `static void FreeBSDMachine_scanCPU(Machine* super)` from
/// `FreeBSDMachine.c:165`. Blocked: needs the `FreeBSDMachine` struct and the
/// per-CPU `kern.cp_times` sysctl scan.
pub fn FreeBSDMachine_scanCPU() {
    todo!("port of FreeBSDMachine.c:165")
}

/// TODO: port of `static void FreeBSDMachine_scanMemoryInfo(Machine* super)`
/// from `FreeBSDMachine.c:305`. Blocked: needs the `FreeBSDMachine` struct and
/// the `vm.stats` / `vfs.bufspace` sysctl scan.
pub fn FreeBSDMachine_scanMemoryInfo() {
    todo!("port of FreeBSDMachine.c:305")
}

/// TODO: port of `void Machine_scan(Machine* super)` from
/// `FreeBSDMachine.c:389`. Blocked: needs the `FreeBSDMachine` scan helpers
/// above and `openzfs_sysctl_updateArcStats` (`generic/openzfs_sysctl.c`,
/// unported).
pub fn Machine_scan() {
    todo!("port of FreeBSDMachine.c:389")
}

/// Port of `bool Machine_isCPUonline(const Machine* host, unsigned int id)`
/// (`FreeBSDMachine.c:397`). FreeBSD does not yet support offline CPUs or hot
/// swapping, so every existing CPU reports online.
pub fn Machine_isCPUonline(host: &Machine, id: u32) -> bool {
    debug_assert!(id < host.existingCPUs);

    // TODO: support offline CPUs and hot swapping
    true
}

/// Port of `int Machine_getCPUPhysicalCoreID(const Machine* host, unsigned int
/// id)` (`FreeBSDMachine.c:406`).
pub fn Machine_getCPUPhysicalCoreID(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    id as i32
}

/// Port of `int Machine_getCPUThreadIndex(const Machine* host, unsigned int
/// id)` (`FreeBSDMachine.c:412`).
pub fn Machine_getCPUThreadIndex(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    0
}
