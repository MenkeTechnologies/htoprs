//! Partial port of `DarwinMachine.c` — the Darwin per-host `Machine`.
//!
//! Ported here (operate on the base [`Machine`], so no unported substrate
//! is needed):
//! - `Machine_isCPUonline` (`DarwinMachine.c:132`)
//! - `Machine_getCPUPhysicalCoreID` (`DarwinMachine.c:141`)
//! - `Machine_getCPUThreadIndex` (`DarwinMachine.c:147`)
//!
//! Ported struct model:
//! - [`host_basic_info_data_t`] (`mach/host_info.h`) and the
//!   [`DarwinMachine`] struct (`DarwinMachine.h:18`), modeled `#[repr(C)]`.
//!   `vm_statistics64`/`processor_cpu_load_info_t`/`mach_port_t` come from
//!   `libc`; `ZfsArcStats` is reused from the (platform-independent) zfs
//!   model in `linux/`.
//!
//! The mach helpers (`DarwinMachine_getHostInfo`, `_freeCPULoadInfo`,
//! `_allocateCPULoadInfo`, `_getVMStats`), `Machine_new` / `Machine_scan` /
//! `Machine_delete` are all ported. `Machine_new` resolves `GPUService` via
//! `IOServiceGetMatchingService(IOGPU)`; `Machine_delete` releases it with
//! `IOObjectRelease`, `munmap`s `prev_load`, and runs `Machine_done`. The ZFS
//! ARC init/teardown is the only documented deviation (its substrate is
//! unported).
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]
// `mach_host_self` is deprecated in `libc` in favor of `mach2`; the C
// original uses it directly, so keep the libc path (as `darwin/platform.rs`).
#![allow(deprecated)]

use std::mem::size_of;
use std::os::raw::{c_int, c_void};
use std::ptr;

use crate::ported::crt::CRT_fatalError;
use crate::ported::generic::openzfs_sysctl::{openzfs_sysctl_init, openzfs_sysctl_updateArcStats};
use crate::ported::linux::linuxmachine::ZfsArcStats;
use crate::ported::machine::{Machine, Machine_done, Machine_init};

// `#define HOST_BASIC_INFO 1` (`mach/host_info.h`).
const HOST_BASIC_INFO: c_int = 1;

// IOKit FFI — resolving the GPU service handle (`IOGPU`) that
// `Platform_setGPUValues` reads. `IOServiceMatching` returns a matching dict
// (`CFMutableDictionaryRef`, an opaque `*mut c_void`) that
// `IOServiceGetMatchingService` consumes; the returned `io_service_t` is a
// `mach_port_t` (`MACH_PORT_NULL`/0 on no match).
#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const libc::c_char) -> *const c_void;
    fn IOServiceGetMatchingService(
        main_port: libc::mach_port_t,
        matching: *const c_void,
    ) -> libc::mach_port_t;
    // Releases an io_object_t (here the GPUService handle) at teardown.
    fn IOObjectRelease(object: libc::mach_port_t) -> libc::kern_return_t;
}

extern "C" {
    // `kern_return_t host_info(host_t, host_flavor_t, host_info_t, mach_msg_type_number_t*)`
    // — not exposed by `libc` (unlike `host_statistics64`/`host_processor_info`).
    fn host_info(
        host: libc::host_t,
        flavor: c_int,
        host_info_out: libc::host_info_t,
        host_info_outCnt: *mut libc::mach_msg_type_number_t,
    ) -> libc::kern_return_t;
}

/// Port of `struct host_basic_info` / `host_basic_info_data_t`
/// (`mach/host_info.h`) — the host summary filled by `host_info()`. `libc`
/// does not model it; the only field htop reads is `max_mem` (total
/// physical memory), but the full layout is transcribed so the offset is
/// exact. `integer_t` → `i32`, `natural_t` → `u32`, `cpu_*_t` → `i32`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct host_basic_info_data_t {
    pub max_cpus: i32,
    pub avail_cpus: i32,
    pub memory_size: u32,
    pub cpu_type: i32,
    pub cpu_subtype: i32,
    pub cpu_threadtype: i32,
    pub physical_cpu: i32,
    pub physical_cpu_max: i32,
    pub logical_cpu: i32,
    pub logical_cpu_max: i32,
    pub max_mem: u64,
}

/// Port of htop's `struct DarwinMachine_` (`DarwinMachine.h:18`). "Extends"
/// the base [`Machine`] via `super_` (first member); `#[repr(C)]` keeps
/// `super_` at offset 0 so htop's `(const DarwinMachine*)host` downcast — a
/// `*const Machine` obtained from a `DarwinMachine`, cast back — is sound
/// (used by `DarwinProcess_setFromLibprocPidinfo` to reach `host_info`).
#[repr(C)]
pub struct DarwinMachine {
    /// C `Machine super` — the embedded base machine.
    pub super_: Machine,
    /// C `host_basic_info_data_t host_info`.
    pub host_info: host_basic_info_data_t,
    /// C `vm_statistics64_data_t vm_stats`.
    pub vm_stats: libc::vm_statistics64,
    /// C `processor_cpu_load_info_t prev_load` — kernel-allocated array.
    pub prev_load: libc::processor_cpu_load_info_t,
    /// C `processor_cpu_load_info_t curr_load`.
    pub curr_load: libc::processor_cpu_load_info_t,
    /// C `io_service_t GPUService` (an `io_object_t` == `mach_port_t`).
    pub GPUService: libc::mach_port_t,
    /// C `ZfsArcStats zfs`.
    pub zfs: ZfsArcStats,
}

/// Port of `static void DarwinMachine_getHostInfo(host_basic_info_data_t* p)`
/// from `DarwinMachine.c:33`. Fills `p` via `host_info(HOST_BASIC_INFO)`;
/// a failure is fatal, as in the C.
pub fn DarwinMachine_getHostInfo(p: &mut host_basic_info_data_t) {
    // C `HOST_BASIC_INFO_COUNT` == sizeof(host_basic_info_data_t)/sizeof(integer_t).
    let mut info_size =
        (size_of::<host_basic_info_data_t>() / size_of::<c_int>()) as libc::mach_msg_type_number_t;
    let rc = unsafe {
        host_info(
            libc::mach_host_self(),
            HOST_BASIC_INFO,
            p as *mut host_basic_info_data_t as libc::host_info_t,
            &mut info_size,
        )
    };
    if rc != 0 {
        CRT_fatalError("Unable to retrieve host info");
    }
}

/// Port of `static void DarwinMachine_freeCPULoadInfo(processor_cpu_load_info_t*
/// p)` from `DarwinMachine.c:41`. Releases the kernel-allocated CPU-load
/// array with `munmap(*p, vm_page_size)` and nulls the pointer; a null
/// array (never allocated) is a no-op.
pub fn DarwinMachine_freeCPULoadInfo(p: &mut libc::processor_cpu_load_info_t) {
    // C also guards `if (!p)`, unreachable here (`p` is a reference).
    if p.is_null() {
        return;
    }

    if unsafe { libc::munmap(*p as *mut c_void, libc::vm_page_size) } != 0 {
        CRT_fatalError("Unable to free old CPU load information");
    }

    *p = ptr::null_mut();
}

/// Port of `static unsigned int
/// DarwinMachine_allocateCPULoadInfo(processor_cpu_load_info_t* p)` from
/// `DarwinMachine.c:55`. Fetches the per-CPU load array via
/// `host_processor_info(PROCESSOR_CPU_LOAD_INFO)`, storing the
/// kernel-allocated array in `*p` and returning the CPU count. A failure is
/// fatal.
pub fn DarwinMachine_allocateCPULoadInfo(p: &mut libc::processor_cpu_load_info_t) -> u32 {
    // C passes `sizeof(processor_cpu_load_info_t)`; host_processor_info
    // overwrites it with the real count.
    let mut info_size =
        size_of::<libc::processor_cpu_load_info_t>() as libc::mach_msg_type_number_t;
    let mut cpu_count: libc::natural_t = 0;

    let rc = unsafe {
        libc::host_processor_info(
            libc::mach_host_self(),
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut cpu_count,
            p as *mut libc::processor_cpu_load_info_t as *mut libc::processor_info_array_t,
            &mut info_size,
        )
    };
    if rc != 0 {
        CRT_fatalError("Unable to retrieve CPU info");
    }

    cpu_count
}

/// Port of `static void DarwinMachine_getVMStats(DarwinMachine* this)` from
/// `DarwinMachine.c:67`. Fills `this.vm_stats` via
/// `host_statistics64(HOST_VM_INFO64)` (the `HAVE_STRUCT_VM_STATISTICS64`
/// branch — the type modern macOS provides); a failure is fatal.
pub fn DarwinMachine_getVMStats(this: &mut DarwinMachine) {
    let mut info_size = libc::HOST_VM_INFO64_COUNT;
    let rc = unsafe {
        libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            &mut this.vm_stats as *mut libc::vm_statistics64 as *mut c_int,
            &mut info_size,
        )
    };
    if rc != 0 {
        CRT_fatalError("Unable to retrieve VM statistics64");
    }
}

/// Port of `void Machine_scan(Machine* super)` from `DarwinMachine.c:83`.
/// Rotates the CPU-load snapshot (`prev_load = curr_load`, re-fetch
/// `curr_load`) and refreshes the VM statistics.
///
/// Rotates the load snapshot, refreshes the VM statistics, and refreshes the
/// ZFS ARC stats (`openzfs_sysctl_updateArcStats`, `DarwinMachine.c:91`).
pub fn Machine_scan(host: &mut DarwinMachine) {
    /* Update the global data (CPU times and VM stats) */
    DarwinMachine_freeCPULoadInfo(&mut host.prev_load);
    host.prev_load = host.curr_load;
    DarwinMachine_allocateCPULoadInfo(&mut host.curr_load);
    DarwinMachine_getVMStats(host);
    openzfs_sysctl_updateArcStats(&mut host.zfs);
}

/// Port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)` from
/// `DarwinMachine.c:94`. Allocates a `DarwinMachine`, runs the base
/// [`Machine_init`], samples the initial CPU load (both `prev_load` and
/// `curr_load`), host info, and VM stats. Returns the owning
/// `Box<DarwinMachine>` (C returns `&this->super`); the caller derives the
/// `*mut Machine` graph pointer from `&mut box.super_`.
///
/// `openzfs_sysctl_init` + `_updateArcStats` seed the ZFS ARC stats (via the
/// `kstat.zfs.misc.arcstats.*` sysctls), matching `DarwinMachine.c:110-111`.
/// `GPUService` is resolved via `IOServiceGetMatchingService(IOGPU)`
/// (the C's own call); the failure branch's `CRT_debug` log is a DEBUG-only
/// no-op, so it is omitted.
pub fn Machine_new(usersTable: Option<usize>, userId: u32) -> Box<DarwinMachine> {
    let mut this = Box::new(DarwinMachine {
        super_: Machine::default(),
        host_info: host_basic_info_data_t::default(),
        vm_stats: unsafe { std::mem::zeroed() },
        prev_load: ptr::null_mut(),
        curr_load: ptr::null_mut(),
        GPUService: 0,
        zfs: ZfsArcStats::default(),
    });

    Machine_init(&mut this.super_, usersTable, userId);

    /* Initialize the CPU information */
    this.super_.activeCPUs = DarwinMachine_allocateCPULoadInfo(&mut this.prev_load);
    this.super_.existingCPUs = this.super_.activeCPUs;
    DarwinMachine_getHostInfo(&mut this.host_info);
    DarwinMachine_allocateCPULoadInfo(&mut this.curr_load);

    /* Initialize the VM statistics */
    DarwinMachine_getVMStats(&mut this);

    openzfs_sysctl_init(&mut this.zfs);
    openzfs_sysctl_updateArcStats(&mut this.zfs);

    // this->GPUService = IOServiceGetMatchingService(kIOMainPortDefault,
    //     IOServiceMatching("IOGPU"));
    // kIOMainPortDefault == MACH_PORT_NULL (0). On no match GPUService stays 0
    // (the C's CRT_debug log is a DEBUG-only no-op).
    this.GPUService =
        unsafe { IOServiceGetMatchingService(0, IOServiceMatching(c"IOGPU".as_ptr())) };

    this
}

/// Port of `void Machine_delete(Machine* super)` from `DarwinMachine.c:121`:
/// `IOObjectRelease(GPUService); DarwinMachine_freeCPULoadInfo(&prev_load);
/// Machine_done(super); free(this);`.
///
/// Takes the owning `Box<DarwinMachine>` (the [`Machine_new`] return) by value
/// — the faithful analog of `free(this)`: the struct and its owned fields drop
/// at end of scope. Note the C frees only `prev_load`, **not** `curr_load` (its
/// mmap is leaked there); the port matches that — `curr_load`'s raw pointer
/// drops without `munmap`, so no behavior is invented.
pub fn Machine_delete(mut this: Box<DarwinMachine>) {
    // IOObjectRelease(this->GPUService); — no-op on MACH_PORT_NULL.
    unsafe {
        IOObjectRelease(this.GPUService);
    }

    // DarwinMachine_freeCPULoadInfo(&this->prev_load); — munmaps the array.
    DarwinMachine_freeCPULoadInfo(&mut this.prev_load);

    // Machine_done(super); — Object_delete(processTable) + free(tables).
    Machine_done(&mut this.super_);

    // free(this) — `this` (Box) drops here.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn darwin_machine() -> Box<DarwinMachine> {
        Box::new(DarwinMachine {
            super_: Machine::default(),
            host_info: host_basic_info_data_t::default(),
            vm_stats: unsafe { std::mem::zeroed() },
            prev_load: ptr::null_mut(),
            curr_load: ptr::null_mut(),
            GPUService: 0,
            zfs: ZfsArcStats::default(),
        })
    }

    #[test]
    fn getHostInfo_reports_physical_memory_and_cpus() {
        let mut hi = host_basic_info_data_t::default();
        DarwinMachine_getHostInfo(&mut hi);
        // Every real host has physical memory and at least one CPU.
        assert!(hi.max_mem > 0);
        assert!(hi.max_cpus > 0);
    }

    #[test]
    fn machine_scan_allocates_cpu_load_and_fills_vm() {
        let mut dm = darwin_machine();

        Machine_scan(&mut dm);

        // curr_load now points at the kernel-allocated per-CPU array.
        assert!(!dm.curr_load.is_null());
        // VM stats are populated (the host always has resident pages).
        assert!(dm.vm_stats.free_count > 0 || dm.vm_stats.active_count > 0);

        // Release the mmap'd array; the pointer is nulled.
        DarwinMachine_freeCPULoadInfo(&mut dm.curr_load);
        assert!(dm.curr_load.is_null());
    }

    #[test]
    fn machine_new_builds_a_populated_host() {
        let mut m = Machine_new(None, 0);

        // CPU counts from host_processor_info.
        assert!(m.super_.activeCPUs > 0);
        assert_eq!(m.super_.existingCPUs, m.super_.activeCPUs);
        // Physical memory from host_info().
        assert!(m.host_info.max_mem > 0);
        // Both CPU-load snapshots were allocated.
        assert!(!m.prev_load.is_null());
        assert!(!m.curr_load.is_null());
        // Machine_init recorded the real uid and a realtime sample.
        assert_eq!(m.super_.htopUserId, unsafe { libc::getuid() });
        assert!(m.super_.realtimeMs > 0);
        // GPUService is resolved via IOServiceGetMatchingService(IOGPU). Each
        // call returns a fresh handle for the same service, so compare presence
        // (non-zero) rather than the handle value: on a Mac with a GPU both are
        // non-zero, on a headless/VM host both are 0.
        let direct =
            unsafe { IOServiceGetMatchingService(0, IOServiceMatching(c"IOGPU".as_ptr())) };
        assert_eq!(m.GPUService != 0, direct != 0);

        DarwinMachine_freeCPULoadInfo(&mut m.prev_load);
        DarwinMachine_freeCPULoadInfo(&mut m.curr_load);
    }

    #[test]
    fn machine_delete_tears_down_without_fault() {
        // Full new → delete teardown exercises the real IOKit/mach frees:
        // IOObjectRelease(GPUService), munmap(prev_load), Machine_done. A
        // double-free or a bad handle would fault here. The C leaks curr_load
        // (Machine_delete frees only prev_load), so free it first to keep the
        // test itself leak-clean while the port stays faithful.
        let mut dm = Machine_new(None, 0);
        assert!(!dm.prev_load.is_null());
        DarwinMachine_freeCPULoadInfo(&mut dm.curr_load);
        Machine_delete(dm);
    }
}
