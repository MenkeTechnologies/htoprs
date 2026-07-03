//! Partial port of `OpenBSDMachine.c` — the OpenBSD per-host `Machine`.
//!
//! Ported here (operate on the base [`Machine`], so no unported substrate
//! is needed):
//! - `Machine_getCPUPhysicalCoreID` (`OpenBSDMachine.c:296`)
//! - `Machine_getCPUThreadIndex` (`OpenBSDMachine.c:302`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `Machine_isCPUonline` reads `OpenBSDMachine.cpuData[id + 1].online`, so
//!   it needs the `OpenBSDMachine` struct modeled.
//! - the `OpenBSDMachine` struct plus `OpenBSDMachine_updateCPUcount` /
//!   `_scanMemoryInfo` / `_scanCPUTime` / `getKernelCPUTimes` /
//!   `kernelCPUTimesToHtop` need the `hw.ncpu`/`kern.cp_time2` sysctl scan.
//! - `Machine_scan` / `Machine_new` / `Machine_delete` additionally need
//!   `Machine_init` / `Machine_done` (still stubs in `machine.rs`) and
//!   `kvm_openfiles`/`kvm_close` FFI.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;

/// TODO: port of `static void OpenBSDMachine_updateCPUcount(OpenBSDMachine*
/// this)` from `OpenBSDMachine.c:34`. Blocked: needs the `OpenBSDMachine`
/// struct and the `hw.ncpu`/`hw.ncpuonline` sysctls.
pub fn OpenBSDMachine_updateCPUcount() {
    todo!("port of OpenBSDMachine.c:34")
}

/// TODO: port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// from `OpenBSDMachine.c:91`. Blocked: needs `Machine_init` (stub in
/// `machine.rs`), the `OpenBSDMachine` struct and `kvm_openfiles` FFI.
pub fn Machine_new() {
    todo!("port of OpenBSDMachine.c:91")
}

/// TODO: port of `void Machine_delete(Machine* super)` from
/// `OpenBSDMachine.c:124`. Blocked: needs `Machine_done` (stub in
/// `machine.rs`), the `OpenBSDMachine` struct and `kvm_close` FFI.
pub fn Machine_delete() {
    todo!("port of OpenBSDMachine.c:124")
}

/// TODO: port of `static void OpenBSDMachine_scanMemoryInfo(Machine* super)`
/// from `OpenBSDMachine.c:135`. Blocked: needs the `OpenBSDMachine` struct and
/// the `vm.uvmexp`/`bcstats` sysctl scan.
pub fn OpenBSDMachine_scanMemoryInfo() {
    todo!("port of OpenBSDMachine.c:135")
}

/// TODO: port of `static void getKernelCPUTimes(int cpuId, u_int64_t* times)`
/// from `OpenBSDMachine.c:193`. Blocked: needs the per-CPU `kern.cp_time2`
/// sysctl scan.
pub fn getKernelCPUTimes() {
    todo!("port of OpenBSDMachine.c:193")
}

/// TODO: port of `static void kernelCPUTimesToHtop(const u_int64_t* times,
/// CPUData* cpu)` from `OpenBSDMachine.c:201`. Blocked: needs the
/// `OpenBSDMachine` `CPUData` struct.
pub fn kernelCPUTimesToHtop() {
    todo!("port of OpenBSDMachine.c:201")
}

/// TODO: port of `static void OpenBSDMachine_scanCPUTime(Machine* super)` from
/// `OpenBSDMachine.c:238`. Blocked: needs the `OpenBSDMachine` scan helpers
/// above.
pub fn OpenBSDMachine_scanCPUTime() {
    todo!("port of OpenBSDMachine.c:238")
}

/// TODO: port of `void Machine_scan(Machine* super)` from
/// `OpenBSDMachine.c:281`. Blocked: needs the `OpenBSDMachine` scan helpers
/// above.
pub fn Machine_scan() {
    todo!("port of OpenBSDMachine.c:281")
}

/// TODO: port of `bool Machine_isCPUonline(const Machine* super, unsigned int
/// id)` from `OpenBSDMachine.c:289`. Blocked: reads
/// `OpenBSDMachine.cpuData[id + 1].online`, so it needs the `OpenBSDMachine`
/// struct modeled.
pub fn Machine_isCPUonline() {
    todo!("port of OpenBSDMachine.c:289")
}

/// Port of `int Machine_getCPUPhysicalCoreID(const Machine* host, unsigned int
/// id)` (`OpenBSDMachine.c:296`).
pub fn Machine_getCPUPhysicalCoreID(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    id as i32
}

/// Port of `int Machine_getCPUThreadIndex(const Machine* host, unsigned int
/// id)` (`OpenBSDMachine.c:302`).
pub fn Machine_getCPUThreadIndex(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    0
}
