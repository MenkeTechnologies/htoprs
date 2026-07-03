//! Partial port of `NetBSDMachine.c` — the NetBSD per-host `Machine`.
//!
//! Ported here (operate on the base [`Machine`], so no unported substrate
//! is needed):
//! - `Machine_isCPUonline` (`NetBSDMachine.c:287`)
//! - `Machine_getCPUPhysicalCoreID` (`NetBSDMachine.c:295`)
//! - `Machine_getCPUThreadIndex` (`NetBSDMachine.c:301`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - the `NetBSDMachine` struct plus `NetBSDMachine_updateCPUcount` /
//!   `_scanMemoryInfo` / `_scanCPUTime` / `_scanCPUFrequency` /
//!   `getKernelCPUTimes` / `kernelCPUTimesToHtop` need the per-CPU
//!   `kern.cp_time` sysctl scan and `uvmexp` modeled.
//! - `Machine_scan` / `Machine_new` / `Machine_delete` additionally need
//!   `Machine_init` / `Machine_done` (still stubs in `machine.rs`) and
//!   `kvm_openfiles`/`kvm_close` FFI.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;

/// TODO: port of `static void NetBSDMachine_updateCPUcount(NetBSDMachine*
/// this)` from `NetBSDMachine.c:51`. Blocked: needs the `NetBSDMachine` struct
/// and the `hw.ncpu`/`hw.ncpuonline` sysctls.
pub fn NetBSDMachine_updateCPUcount() {
    todo!("port of NetBSDMachine.c:51")
}

/// TODO: port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// from `NetBSDMachine.c:105`. Blocked: needs `Machine_init` (stub in
/// `machine.rs`), the `NetBSDMachine` struct and `kvm_openfiles` FFI.
pub fn Machine_new() {
    todo!("port of NetBSDMachine.c:105")
}

/// TODO: port of `void Machine_delete(Machine* super)` from
/// `NetBSDMachine.c:135`. Blocked: needs `Machine_done` (stub in
/// `machine.rs`), the `NetBSDMachine` struct and `kvm_close` FFI.
pub fn Machine_delete() {
    todo!("port of NetBSDMachine.c:135")
}

/// TODO: port of `static void NetBSDMachine_scanMemoryInfo(Machine* super)`
/// from `NetBSDMachine.c:147`. Blocked: needs the `NetBSDMachine` struct and
/// the `uvmexp`/`VM_UVMEXP2` sysctl scan.
pub fn NetBSDMachine_scanMemoryInfo() {
    todo!("port of NetBSDMachine.c:147")
}

/// TODO: port of `static void getKernelCPUTimes(int cpuId, uint64_t* times)`
/// from `NetBSDMachine.c:172`. Blocked: needs the per-CPU `kern.cp_time`
/// sysctl scan.
pub fn getKernelCPUTimes() {
    todo!("port of NetBSDMachine.c:172")
}

/// TODO: port of `static void kernelCPUTimesToHtop(const uint64_t* times,
/// CPUData* cpu)` from `NetBSDMachine.c:180`. Blocked: needs the
/// `NetBSDMachine` `CPUData` struct.
pub fn kernelCPUTimesToHtop() {
    todo!("port of NetBSDMachine.c:180")
}

/// TODO: port of `static void NetBSDMachine_scanCPUTime(NetBSDMachine* this)`
/// from `NetBSDMachine.c:205`. Blocked: needs the `NetBSDMachine` scan helpers
/// above.
pub fn NetBSDMachine_scanCPUTime() {
    todo!("port of NetBSDMachine.c:205")
}

/// TODO: port of `static void NetBSDMachine_scanCPUFrequency(NetBSDMachine*
/// this)` from `NetBSDMachine.c:230`. Blocked: needs the `NetBSDMachine`
/// struct and the `machdep.*.frequency.current` sysctl.
pub fn NetBSDMachine_scanCPUFrequency() {
    todo!("port of NetBSDMachine.c:230")
}

/// TODO: port of `void Machine_scan(Machine* super)` from
/// `NetBSDMachine.c:276`. Blocked: needs the `NetBSDMachine` scan helpers
/// above.
pub fn Machine_scan() {
    todo!("port of NetBSDMachine.c:276")
}

/// Port of `bool Machine_isCPUonline(const Machine* host, unsigned int id)`
/// (`NetBSDMachine.c:287`). NetBSD detection of online/offline CPUs is not yet
/// supported, so every existing CPU reports online.
pub fn Machine_isCPUonline(host: &Machine, id: u32) -> bool {
    debug_assert!(id < host.existingCPUs);

    // TODO: Support detecting online / offline CPUs.
    true
}

/// Port of `int Machine_getCPUPhysicalCoreID(const Machine* host, unsigned int
/// id)` (`NetBSDMachine.c:295`).
pub fn Machine_getCPUPhysicalCoreID(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    id as i32
}

/// Port of `int Machine_getCPUThreadIndex(const Machine* host, unsigned int
/// id)` (`NetBSDMachine.c:301`).
pub fn Machine_getCPUThreadIndex(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    0
}
