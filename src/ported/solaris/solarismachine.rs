//! Partial port of `SolarisMachine.c` — the Solaris/illumos per-host
//! `Machine`.
//!
//! Ported here (operate on the base [`Machine`], so no unported substrate
//! is needed):
//! - `Machine_getCPUPhysicalCoreID` (`SolarisMachine.c:333`)
//! - `Machine_getCPUThreadIndex` (`SolarisMachine.c:339`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `Machine_isCPUonline` reads `SolarisMachine.cpus[id + 1].online`, so it
//!   needs the `SolarisMachine` struct modeled.
//! - the `SolarisMachine` struct plus `SolarisMachine_updateCPUcount` /
//!   `_scanCPUTime` / `_scanMemoryInfo` / `_scanZfsArcstats` need `libkstat`
//!   (`kstat_*`) FFI and the ZFS ARC kstats modeled.
//! - `Machine_scan` / `Machine_new` / `Machine_delete` additionally need
//!   `Machine_init` / `Machine_done` (still stubs in `machine.rs`) and
//!   `kstat_open`/`kstat_close` FFI.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;

/// TODO: port of `static void SolarisMachine_updateCPUcount(SolarisMachine*
/// this)` from `SolarisMachine.c:29`. Blocked: needs the `SolarisMachine`
/// struct and `sysconf(_SC_NPROCESSORS_*)`.
pub fn SolarisMachine_updateCPUcount() {
    todo!("port of SolarisMachine.c:29")
}

/// TODO: port of `static void SolarisMachine_scanCPUTime(Machine* super)` from
/// `SolarisMachine.c:71`. Blocked: needs the `SolarisMachine` struct and the
/// `libkstat` per-CPU `cpu_stat` scan.
pub fn SolarisMachine_scanCPUTime() {
    todo!("port of SolarisMachine.c:71")
}

/// TODO: port of `static void SolarisMachine_scanMemoryInfo(Machine* super)`
/// from `SolarisMachine.c:165`. Blocked: needs the `SolarisMachine` struct and
/// the `libkstat` `unix:0:system_pages` scan.
pub fn SolarisMachine_scanMemoryInfo() {
    todo!("port of SolarisMachine.c:165")
}

/// TODO: port of `static void SolarisMachine_scanZfsArcstats(Machine* super)`
/// from `SolarisMachine.c:234`. Blocked: needs the `SolarisMachine` struct and
/// the `libkstat` `zfs:0:arcstats` scan.
pub fn SolarisMachine_scanZfsArcstats() {
    todo!("port of SolarisMachine.c:234")
}

/// TODO: port of `void Machine_scan(Machine* super)` from
/// `SolarisMachine.c:283`. Blocked: needs the `SolarisMachine` scan helpers
/// above.
pub fn Machine_scan() {
    todo!("port of SolarisMachine.c:283")
}

/// TODO: port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// from `SolarisMachine.c:292`. Blocked: needs `Machine_init` (stub in
/// `machine.rs`), the `SolarisMachine` struct and `kstat_open` FFI.
pub fn Machine_new() {
    todo!("port of SolarisMachine.c:292")
}

/// TODO: port of `void Machine_delete(Machine* super)` from
/// `SolarisMachine.c:313`. Blocked: needs `Machine_done` (stub in
/// `machine.rs`), the `SolarisMachine` struct and `kstat_close` FFI.
pub fn Machine_delete() {
    todo!("port of SolarisMachine.c:313")
}

/// TODO: port of `bool Machine_isCPUonline(const Machine* super, unsigned int
/// id)` from `SolarisMachine.c:325`. Blocked: reads
/// `SolarisMachine.cpus[id + 1].online`, so it needs the `SolarisMachine`
/// struct modeled.
pub fn Machine_isCPUonline() {
    todo!("port of SolarisMachine.c:325")
}

/// Port of `int Machine_getCPUPhysicalCoreID(const Machine* host, unsigned int
/// id)` (`SolarisMachine.c:333`).
pub fn Machine_getCPUPhysicalCoreID(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    id as i32
}

/// Port of `int Machine_getCPUThreadIndex(const Machine* host, unsigned int
/// id)` (`SolarisMachine.c:339`).
pub fn Machine_getCPUThreadIndex(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    0
}
