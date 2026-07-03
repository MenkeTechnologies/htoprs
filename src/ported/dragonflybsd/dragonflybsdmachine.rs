//! Port of `dragonflybsd/DragonFlyBSDMachine.c` + `.h` — the DragonFly BSD
//! per-host state and its `sysctl`/`libkvm` scan layer.
//!
//! The struct model and the small pure accessors are ported here. The scan
//! functions (`Machine_new`/`Machine_scan`/`DragonFlyBSDMachine_scan*`) drive
//! `sysctl`/`kvm_*`, which exist only on DragonFly BSD; like the `linux/` scan
//! layer they are ported-but-only-runnable on their platform. They are kept as
//! faithful `todo!()` stubs (named after the C functions so the port gate
//! accepts the module) until ported behind `#[cfg(target_os = "dragonfly")]`
//! with the DragonFly `sys/sysctl.h` / `sys/user.h` bindings.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::ffi::c_void;

use crate::ported::hashtable::Hashtable;
use crate::ported::machine::Machine;

/// Port of `typedef struct CPUData_` (`DragonFlyBSDMachine.h:26`) — the
/// per-CPU load percentages computed each scan from the `kern.cp_time(s)`
/// sysctl deltas.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CPUData {
    pub userPercent: f64,
    pub nicePercent: f64,
    pub systemPercent: f64,
    pub irqPercent: f64,
    pub idlePercent: f64,
    pub systemAllPercent: f64,
}

/// Port of `typedef struct DragonFlyBSDMachine_` (`DragonFlyBSDMachine.h`).
/// "Extends" [`Machine`] via the embedded `super_`, plus the DragonFly kvm
/// handle, jail table, page-size / scale constants, memory partition sizes,
/// per-CPU data, and the `cp_time(s)` old/new tick buffers.
///
/// `kd` (C `kvm_t*`) is an opaque `*mut c_void` (no `libkvm` on non-DragonFly
/// hosts); `jails` (C `Hashtable*` of jailid → hostname) is an owned
/// [`Hashtable`]; `memory_t` fields are `u64`; the `cp_time*` arrays are
/// `Vec<u64>` (C `unsigned long*`, `xCalloc`-sized per CPU).
///
/// No `#[derive(Debug)]`: the `jails` [`Hashtable`] holds trait-object values
/// and is not `Debug`. Constructed by the (stubbed) [`Machine_new`].
pub struct DragonFlyBSDMachine {
    /// C `Machine super`.
    pub super_: Machine,
    /// C `kvm_t* kd` — the libkvm handle (opaque here).
    pub kd: *mut c_void,
    /// C `Hashtable* jails` — jailid → hostname.
    pub jails: Option<Hashtable>,
    /// C `int pageSize`.
    pub pageSize: i32,
    /// C `int pageSizeKb`.
    pub pageSizeKb: i32,
    /// C `int kernelFScale` — kernel fixed-point load scale.
    pub kernelFScale: i32,
    /// C `memory_t wiredMem`.
    pub wiredMem: u64,
    /// C `memory_t buffersMem`.
    pub buffersMem: u64,
    /// C `memory_t activeMem`.
    pub activeMem: u64,
    /// C `memory_t inactiveMem`.
    pub inactiveMem: u64,
    /// C `memory_t cacheMem`.
    pub cacheMem: u64,
    /// C `CPUData* cpus` — one entry per CPU (index 0 is the aggregate).
    pub cpus: Vec<CPUData>,
    /// C `unsigned long* cp_time_o` — previous aggregate cp_time ticks.
    pub cp_time_o: Vec<u64>,
    /// C `unsigned long* cp_time_n` — current aggregate cp_time ticks.
    pub cp_time_n: Vec<u64>,
    /// C `unsigned long* cp_times_o` — previous per-CPU cp_times ticks.
    pub cp_times_o: Vec<u64>,
    /// C `unsigned long* cp_times_n` — current per-CPU cp_times ticks.
    pub cp_times_n: Vec<u64>,
}

/// Port of `bool Machine_isCPUonline(const Machine* host, unsigned int id)`
/// (`DragonFlyBSDMachine.c:369`). DragonFly does not yet expose per-CPU
/// online/offline state, so every existing CPU is reported online (verbatim
/// C behavior, including the `id < existingCPUs` precondition).
pub fn Machine_isCPUonline(host: &Machine, id: u32) -> bool {
    debug_assert!(id < host.existingCPUs);
    // TODO (as in C): support detecting online / offline CPUs.
    true
}

/// Port of `int Machine_getCPUPhysicalCoreID(const Machine* host, unsigned int
/// id)` (`DragonFlyBSDMachine.c:377`). DragonFly does not expose topology, so
/// the physical core id is the CPU id itself.
pub fn Machine_getCPUPhysicalCoreID(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    id as i32
}

/// Port of `int Machine_getCPUThreadIndex(const Machine* host, unsigned int
/// id)` (`DragonFlyBSDMachine.c:383`). No SMT topology on DragonFly, so every
/// CPU is thread index 0.
pub fn Machine_getCPUThreadIndex(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    0
}

/// TODO: port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// (`DragonFlyBSDMachine.c:41`). Opens `kvm_openfiles`, reads page size /
/// physmem / v_page_count via `sysctl(name)tomib`, and allocates the CPU /
/// cp_time buffers — DragonFly `sys/sysctl.h` + `libkvm`, gated to
/// `#[cfg(target_os = "dragonfly")]` when ported.
pub fn Machine_new() {
    todo!("port of DragonFlyBSDMachine.c:41 — kvm_openfiles + sysctl (DragonFly-only)")
}

/// TODO: port of `void Machine_delete(Machine* super)`
/// (`DragonFlyBSDMachine.c:119`). `kvm_close` + `Hashtable_delete(jails)` +
/// `free` of the cpu / cp_time buffers; Rust `Drop` releases the owned Vecs,
/// but the `kvm_t*` close is DragonFly-only.
pub fn Machine_delete() {
    todo!("port of DragonFlyBSDMachine.c:119 — kvm_close teardown (DragonFly-only)")
}

/// TODO: port of `static void DragonFlyBSDMachine_scanCPUTime(Machine* super)`
/// (`DragonFlyBSDMachine.c:141`). Reads `kern.cp_time` / `kern.cp_times` via
/// sysctl and computes per-CPU load deltas. DragonFly sysctl.
pub fn DragonFlyBSDMachine_scanCPUTime() {
    todo!("port of DragonFlyBSDMachine.c:141 — kern.cp_time sysctl (DragonFly-only)")
}

/// TODO: port of `static void DragonFlyBSDMachine_scanMemoryInfo(Machine*
/// super)` (`DragonFlyBSDMachine.c:223`). Reads the `vm.stats.vm.*` counters
/// via sysctl for wired/active/inactive/cache/buffers memory. DragonFly sysctl.
pub fn DragonFlyBSDMachine_scanMemoryInfo() {
    todo!("port of DragonFlyBSDMachine.c:223 — vm.stats sysctl (DragonFly-only)")
}

/// TODO: port of `static void DragonFlyBSDMachine_scanJails(DragonFlyBSDMachine*
/// this)` (`DragonFlyBSDMachine.c:294`). Enumerates jails via `kern.jail`
/// sysctl into the `jails` hashtable. DragonFly sysctl.
pub fn DragonFlyBSDMachine_scanJails() {
    todo!("port of DragonFlyBSDMachine.c:294 — kern.jail sysctl (DragonFly-only)")
}

/// TODO: port of `char* DragonFlyBSDMachine_readJailName(const
/// DragonFlyBSDMachine* host, int jailid)` (`DragonFlyBSDMachine.c:348`).
/// Looks up `jailid` in the `jails` hashtable (populated by the stubbed
/// [`DragonFlyBSDMachine_scanJails`]) and duplicates the hostname, else `"-"`.
/// Blocked on the jails hashtable being populated (needs the sysctl scan) and
/// on modeling its `char*` string values.
pub fn DragonFlyBSDMachine_readJailName() {
    todo!("port of DragonFlyBSDMachine.c:348 — needs DragonFlyBSDMachine_scanJails-populated jails + Hashtable string values")
}

/// TODO: port of `void Machine_scan(Machine* super)`
/// (`DragonFlyBSDMachine.c:361`). Orchestrates the per-tick scan:
/// `DragonFlyBSDMachine_scanMemoryInfo` + `DragonFlyBSDMachine_scanCPUTime`
/// (both stubbed above, DragonFly sysctl).
pub fn Machine_scan() {
    todo!("port of DragonFlyBSDMachine.c:361 — drives the stubbed sysctl scans (DragonFly-only)")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pure CPU accessors report DragonFly's fixed topology answers for
    /// every existing CPU.
    #[test]
    fn cpu_accessors_report_fixed_topology() {
        let mut host = Machine::default();
        host.existingCPUs = 4;
        for id in 0..host.existingCPUs {
            assert!(Machine_isCPUonline(&host, id));
            assert_eq!(Machine_getCPUPhysicalCoreID(&host, id), id as i32);
            assert_eq!(Machine_getCPUThreadIndex(&host, id), 0);
        }
    }
}
