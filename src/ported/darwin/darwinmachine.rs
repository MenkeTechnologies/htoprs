//! Partial port of `DarwinMachine.c` — the Darwin per-host `Machine`.
//!
//! Ported here (operate on the base [`Machine`], so no unported substrate
//! is needed):
//! - `Machine_isCPUonline` (`DarwinMachine.c:132`)
//! - `Machine_getCPUPhysicalCoreID` (`DarwinMachine.c:141`)
//! - `Machine_getCPUThreadIndex` (`DarwinMachine.c:147`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - the `DarwinMachine` struct and its mach helpers (`DarwinMachine_getHostInfo`,
//!   `_freeCPULoadInfo`, `_allocateCPULoadInfo`, `_getVMStats`) need the
//!   `host_basic_info_data_t` / `vm_statistics64_data_t` /
//!   `processor_cpu_load_info_t` mach types modeled plus the embedded
//!   `ZfsArcStats`.
//! - `Machine_scan` / `Machine_new` / `Machine_delete` additionally need
//!   `Machine_init` / `Machine_done` (still stubs in `machine.rs`),
//!   `openzfs_sysctl_*` (`generic/openzfs_sysctl.c`, unported) and IOKit
//!   (`IOServiceGetMatchingService`) FFI bindings.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;

/// TODO: port of `static void DarwinMachine_getHostInfo(host_basic_info_data_t*
/// p)` from `DarwinMachine.c:33`. Blocked: needs the `host_basic_info_data_t`
/// mach type and a `DarwinMachine` to store it in.
pub fn DarwinMachine_getHostInfo() {
    todo!("port of DarwinMachine.c:33")
}

/// TODO: port of `static void
/// DarwinMachine_freeCPULoadInfo(processor_cpu_load_info_t* p)` from
/// `DarwinMachine.c:41`. Blocked: needs the `processor_cpu_load_info_t` mach
/// type and a `DarwinMachine`.
pub fn DarwinMachine_freeCPULoadInfo() {
    todo!("port of DarwinMachine.c:41")
}

/// TODO: port of `static unsigned int
/// DarwinMachine_allocateCPULoadInfo(processor_cpu_load_info_t* p)` from
/// `DarwinMachine.c:55`. Blocked: needs `host_processor_info` FFI and a
/// `DarwinMachine`.
pub fn DarwinMachine_allocateCPULoadInfo() {
    todo!("port of DarwinMachine.c:55")
}

/// TODO: port of `static void DarwinMachine_getVMStats(DarwinMachine* this)`
/// from `DarwinMachine.c:67`. Blocked: needs `host_statistics64` FFI, the
/// `vm_statistics64_data_t` mach type and a `DarwinMachine`.
pub fn DarwinMachine_getVMStats() {
    todo!("port of DarwinMachine.c:67")
}

/// TODO: port of `void Machine_scan(Machine* super)` from
/// `DarwinMachine.c:83`. Blocked: needs the `DarwinMachine` mach helpers
/// above and `openzfs_sysctl_updateArcStats` (`generic/openzfs_sysctl.c`,
/// unported).
pub fn Machine_scan() {
    todo!("port of DarwinMachine.c:83")
}

/// TODO: port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// from `DarwinMachine.c:94`. Blocked: needs `Machine_init` (stub in
/// `machine.rs`), the `DarwinMachine` struct, `openzfs_sysctl_*` and IOKit
/// (`IOServiceGetMatchingService`) FFI.
pub fn Machine_new() {
    todo!("port of DarwinMachine.c:94")
}

/// TODO: port of `void Machine_delete(Machine* super)` from
/// `DarwinMachine.c:121`. Blocked: needs `Machine_done` (stub in
/// `machine.rs`), the `DarwinMachine` struct and IOKit (`IOObjectRelease`)
/// FFI.
pub fn Machine_delete() {
    todo!("port of DarwinMachine.c:121")
}

/// Port of `bool Machine_isCPUonline(const Machine* host, unsigned int id)`
/// (`DarwinMachine.c:132`). Darwin does not yet support offline CPUs or hot
/// swapping, so every existing CPU reports online.
pub fn Machine_isCPUonline(host: &Machine, id: u32) -> bool {
    debug_assert!(id < host.existingCPUs);

    // TODO: support offline CPUs and hot swapping
    true
}

/// Port of `int Machine_getCPUPhysicalCoreID(const Machine* host, unsigned
/// int id)` (`DarwinMachine.c:141`).
pub fn Machine_getCPUPhysicalCoreID(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    id as i32
}

/// Port of `int Machine_getCPUThreadIndex(const Machine* host, unsigned int
/// id)` (`DarwinMachine.c:147`).
pub fn Machine_getCPUThreadIndex(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    0
}
