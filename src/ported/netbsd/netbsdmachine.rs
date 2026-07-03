//! Port of `NetBSDMachine.c` — the NetBSD per-host `Machine`.
//!
//! Ported struct model:
//! - [`CPUData`] (`NetBSDMachine.h:21`) and the [`NetBSDMachine`] struct
//!   (`NetBSDMachine.h:43`), modeled `#[repr(C)]` with `super_: Machine` at
//!   offset 0 so htop's `(NetBSDMachine*)host` downcast round-trips.
//! - [`uvmexp_sysctl`] (`uvm/uvm_extern.h`), transcribed field-for-field
//!   because `libc` does not model it.
//!
//! Ported functions:
//! - [`NetBSDMachine_updateCPUcount`] (`NetBSDMachine.c:51`)
//! - [`NetBSDMachine_scanMemoryInfo`] (`NetBSDMachine.c:147`)
//! - [`getKernelCPUTimes`] (`NetBSDMachine.c:172`)
//! - [`kernelCPUTimesToHtop`] (`NetBSDMachine.c:180`)
//! - [`NetBSDMachine_scanCPUTime`] (`NetBSDMachine.c:205`)
//! - [`NetBSDMachine_scanCPUFrequency`] (`NetBSDMachine.c:230`)
//! - [`Machine_scan`] (`NetBSDMachine.c:276`)
//! - [`Machine_delete`] (`NetBSDMachine.c:135`)
//! - `Machine_isCPUonline` / `Machine_getCPUPhysicalCoreID` /
//!   `Machine_getCPUThreadIndex` (`NetBSDMachine.c:287`/`295`/`301`)
//!
//! Still `todo!()`:
//! - `Machine_new` (`NetBSDMachine.c:105`) calls `Machine_init`, which is
//!   `#[cfg(target_os = "macos")]` in `machine.rs` and therefore absent on the
//!   NetBSD target; the base `Machine_init` cannot be reached from here and
//!   `machine.rs` is out of scope for this module.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::ffi::CString;
use std::mem::size_of;
use std::os::raw::{c_int, c_long, c_uint, c_void};
use std::ptr;

use crate::ported::crt::CRT_fatalError;
use crate::ported::machine::{Machine, Machine_done};
use crate::ported::xutils::saturatingSub;

// ── NetBSD sysctl identifiers absent from `libc` for this target.

/// `HW_NCPU` (`sys/sysctl.h`) — number of existing CPUs (`CTL_HW`).
const HW_NCPU: c_int = 3;
/// `HW_NCPUONLINE` (`sys/sysctl.h`) — number of online CPUs (`CTL_HW`).
const HW_NCPUONLINE: c_int = 16;
/// `VM_UVMEXP2` (`uvm/uvm_param.h`) — the binary `uvmexp_sysctl` snapshot
/// (`CTL_VM`).
const VM_UVMEXP2: c_int = 5;

/// `CPUSTATES` (`sys/sched.h`) — number of per-CPU time buckets.
const CPUSTATES: usize = 5;
/// `CP_USER` (`sys/sched.h`).
const CP_USER: usize = 0;
/// `CP_NICE` (`sys/sched.h`).
const CP_NICE: usize = 1;
/// `CP_SYS` (`sys/sched.h`).
const CP_SYS: usize = 2;
/// `CP_INTR` (`sys/sched.h`).
const CP_INTR: usize = 3;
/// `CP_IDLE` (`sys/sched.h`).
const CP_IDLE: usize = 4;

/// `ONE_K` (`Macros.h`) — the KiB divisor.
const ONE_K: usize = 1024;

extern "C" {
    /// `int kvm_close(kvm_t*)` (`kvm.h`). Not exposed by `libc`.
    fn kvm_close(kd: *mut c_void) -> c_int;
}

/// Port of `static const struct { const char* name; long int scale; }
/// freqSysctls[]` (`NetBSDMachine.c:38`) — the legacy single-core CPU
/// frequency sysctl nodes, tried in order until one resolves.
const FREQ_SYSCTLS: &[(&str, c_long)] = &[
    ("machdep.est.frequency.current", 1),
    ("machdep.powernow.frequency.current", 1),
    ("machdep.intrepid.frequency.current", 1),
    ("machdep.loongson.frequency.current", 1),
    ("machdep.cpu.frequency.current", 1),
    ("machdep.frequency.current", 1),
    ("machdep.tsc_freq", 1_000_000),
];

/// Port of `typedef struct CPUData_` (`NetBSDMachine.h:21`) — the per-CPU
/// accumulator (`cpuData[0]` holds the average).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CPUData {
    pub totalTime: u64,
    pub userTime: u64,
    pub niceTime: u64,
    pub sysTime: u64,
    pub sysAllTime: u64,
    pub spinTime: u64,
    pub intrTime: u64,
    pub idleTime: u64,

    pub totalPeriod: u64,
    pub userPeriod: u64,
    pub nicePeriod: u64,
    pub sysPeriod: u64,
    pub sysAllPeriod: u64,
    pub spinPeriod: u64,
    pub intrPeriod: u64,
    pub idlePeriod: u64,

    pub frequency: f64,
}

/// Port of `typedef struct NetBSDMachine_` (`NetBSDMachine.h:43`). "Extends"
/// the base [`Machine`] via `super_` (first member); `#[repr(C)]` keeps
/// `super_` at offset 0 so htop's `(NetBSDMachine*)host` downcast — a
/// `*const Machine` obtained from a `NetBSDMachine`, cast back — is sound
/// (used by `NetBSDProcessTable`/`Platform` to reach `cpuData`, `fscale`,
/// `kd`, and the per-class memory totals).
///
/// The C `CPUData* cpuData` heap array of `existingCPUs + 1` entries is
/// modeled as an owned `Vec<CPUData>` (the `xReallocArray`/`free` map to the
/// `Vec`'s grow/drop).
#[repr(C)]
pub struct NetBSDMachine {
    /// C `Machine super`.
    pub super_: Machine,
    /// C `kvm_t* kd`.
    pub kd: *mut c_void,

    /// C `long fscale`.
    pub fscale: c_long,
    /// C `size_t pageSize`.
    pub pageSize: usize,
    /// C `size_t pageSizeKB`.
    pub pageSizeKB: usize,

    /// C `memory_t wiredMem`.
    pub wiredMem: u64,
    /// C `memory_t activeMem`.
    pub activeMem: u64,
    /// C `memory_t pagedMem`.
    pub pagedMem: u64,
    /// C `memory_t inactiveMem`.
    pub inactiveMem: u64,

    /// C `CPUData* cpuData`.
    pub cpuData: Vec<CPUData>,
}

/// Port of `struct uvmexp_sysctl` (`uvm/uvm_extern.h`) — the binary
/// UVM statistics snapshot filled by `sysctl(VM_UVMEXP2)`. `libc` does not
/// model it; transcribed field-for-field (`int64_t` → `i64`) so the offsets
/// of the members htop reads (`npages`/`active`/`inactive`/`paging`/`wired`/
/// `swpages`/`swpginuse`) are exact.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct uvmexp_sysctl {
    pub pagesize: i64,
    pub pagemask: i64,
    pub pageshift: i64,
    pub npages: i64,
    pub free: i64,
    pub active: i64,
    pub inactive: i64,
    pub paging: i64,
    pub wired: i64,
    pub zeropages: i64,
    pub reserve_pagedaemon: i64,
    pub reserve_kernel: i64,
    pub freemin: i64,
    pub freetarg: i64,
    pub inactarg: i64,
    pub wiredmax: i64,
    pub nswapdev: i64,
    pub swpages: i64,
    pub swpginuse: i64,
    pub swpgonly: i64,
    pub nswget: i64,
    pub unused1: i64,
    pub cpuhit: i64,
    pub cpumiss: i64,
    pub faults: i64,
    pub traps: i64,
    pub intrs: i64,
    pub swtch: i64,
    pub softs: i64,
    pub syscalls: i64,
    pub pageins: i64,
    pub swapins: i64,
    pub swapouts: i64,
    pub pgswapin: i64,
    pub pgswapout: i64,
    pub forks: i64,
    pub forks_ppwait: i64,
    pub forks_sharevm: i64,
    pub pga_zerohit: i64,
    pub pga_zeromiss: i64,
    pub zeroaborts: i64,
    pub fltnoram: i64,
    pub fltnoanon: i64,
    pub fltpgwait: i64,
    pub fltpgrele: i64,
    pub fltrelck: i64,
    pub fltrelckok: i64,
    pub fltanget: i64,
    pub fltanretry: i64,
    pub fltamcopy: i64,
    pub fltnamap: i64,
    pub fltnomap: i64,
    pub fltlget: i64,
    pub fltget: i64,
    pub flt_anon: i64,
    pub flt_acow: i64,
    pub flt_obj: i64,
    pub flt_prcopy: i64,
    pub flt_przero: i64,
    pub pdwoke: i64,
    pub pdrevs: i64,
    pub unused4: i64,
    pub pdfreed: i64,
    pub pdscans: i64,
    pub pdanscan: i64,
    pub pdobscan: i64,
    pub pdreact: i64,
    pub pdbusy: i64,
    pub pdpageouts: i64,
    pub pdpending: i64,
    pub pddeact: i64,
    pub anonpages: i64,
    pub filepages: i64,
    pub execpages: i64,
    pub colorhit: i64,
    pub colormiss: i64,
    pub ncolors: i64,
    pub bootpages: i64,
    pub poolpages: i64,
    pub countsyncone: i64,
    pub countsyncall: i64,
    pub anonunknown: i64,
    pub anonclean: i64,
    pub anondirty: i64,
    pub fileunknown: i64,
    pub fileclean: i64,
    pub filedirty: i64,
    pub fltup: i64,
    pub fltnoup: i64,
}

/// Port of `static void NetBSDMachine_updateCPUcount(NetBSDMachine* this)`
/// from `NetBSDMachine.c:51`. Queries `hw.ncpuonline` and `hw.ncpu`, growing
/// the [`CPUData`] array (`Vec`) to `existingCPUs + 1` on change and resetting
/// every bucket (`totalTime = totalPeriod = 1`) when the online/existing count
/// changed.
pub fn NetBSDMachine_updateCPUcount(this: &mut NetBSDMachine) {
    // cf. https://nxr.netbsd.org/xref/src/sys/sys/sysctl.h
    let mib_ncpu_existing: [c_int; 2] = [libc::CTL_HW, HW_NCPU];
    let mib_ncpu_online: [c_int; 2] = [libc::CTL_HW, HW_NCPUONLINE];

    let mut value: c_uint = 0;
    let mut change = false;

    // Query the number of active/online CPUs.
    let mut size = size_of::<c_uint>();
    let r = unsafe {
        libc::sysctl(
            mib_ncpu_online.as_ptr(),
            2,
            &mut value as *mut c_uint as *mut c_void,
            &mut size,
            ptr::null(),
            0,
        )
    };
    if r < 0 || value < 1 {
        value = 1;
    }

    if value != this.super_.activeCPUs {
        this.super_.activeCPUs = value;
        change = true;
    }

    // Query the total number of CPUs.
    size = size_of::<c_uint>();
    let r = unsafe {
        libc::sysctl(
            mib_ncpu_existing.as_ptr(),
            2,
            &mut value as *mut c_uint as *mut c_void,
            &mut size,
            ptr::null(),
            0,
        )
    };
    if r < 0 || value < 1 {
        value = this.super_.activeCPUs;
    }

    if value != this.super_.existingCPUs {
        // xReallocArray(cpuData, value + 1) → a Vec of value + 1 CPUData.
        this.cpuData.resize(value as usize + 1, CPUData::default());
        this.super_.existingCPUs = value;
        change = true;
    }

    // Reset CPU stats when number of online/existing CPU cores changed.
    if change {
        for i in 0..=this.super_.existingCPUs as usize {
            let d = &mut this.cpuData[i];
            *d = CPUData::default();
            d.totalTime = 1;
            d.totalPeriod = 1;
        }
    }
}

/// Port of `void Machine_delete(Machine* super)` from `NetBSDMachine.c:135`.
/// Runs the base [`Machine_done`], closes the `kvm` handle if open, and drops
/// the machine (`free(this->cpuData)` / `free(this)` → the owned `Vec` and
/// `Box` reclaim). Consumes the owning `Box<NetBSDMachine>`.
pub fn Machine_delete(mut this: Box<NetBSDMachine>) {
    Machine_done(&mut this.super_);

    if !this.kd.is_null() {
        unsafe {
            kvm_close(this.kd);
        }
    }
    // free(this->cpuData); free(this); — Vec / Box Drop reclaims.
}

/// Port of `static void NetBSDMachine_scanMemoryInfo(NetBSDMachine* this)`
/// from `NetBSDMachine.c:147`. Reads the UVM snapshot via `sysctl(VM_UVMEXP2)`
/// and converts the page counts to KiB. A sysctl failure is fatal, as in the
/// C.
pub fn NetBSDMachine_scanMemoryInfo(this: &mut NetBSDMachine) {
    let uvmexp_mib: [c_int; 2] = [libc::CTL_VM, VM_UVMEXP2];
    let mut uvmexp: uvmexp_sysctl = unsafe { std::mem::zeroed() };
    let mut size_uvmexp = size_of::<uvmexp_sysctl>();

    if unsafe {
        libc::sysctl(
            uvmexp_mib.as_ptr(),
            2,
            &mut uvmexp as *mut uvmexp_sysctl as *mut c_void,
            &mut size_uvmexp,
            ptr::null(),
            0,
        )
    } < 0
    {
        CRT_fatalError("uvmexp sysctl call failed");
    }

    let page_kb = this.pageSizeKB as u64;
    this.wiredMem = page_kb * uvmexp.wired as u64;
    this.activeMem = page_kb * uvmexp.active as u64;
    this.pagedMem = page_kb * uvmexp.paging as u64;
    this.inactiveMem = page_kb * uvmexp.inactive as u64;

    this.super_.totalMem = page_kb * uvmexp.npages as u64;
    this.super_.totalSwap = uvmexp.swpages as u64 * page_kb;
    this.super_.usedSwap = uvmexp.swpginuse as u64 * page_kb;
}

/// Port of `static void getKernelCPUTimes(int cpuId, u_int64_t* times)` from
/// `NetBSDMachine.c:172`. Fills `times[0..CPUSTATES]` from
/// `sysctl(kern.cp_time, cpuId)`; any short read is fatal, as in the C.
pub fn getKernelCPUTimes(cpuId: c_int, times: &mut [u64; CPUSTATES]) {
    let mib: [c_int; 3] = [libc::CTL_KERN, libc::KERN_CP_TIME, cpuId];
    let mut length = size_of::<u64>() * CPUSTATES;
    if unsafe {
        libc::sysctl(
            mib.as_ptr(),
            3,
            times.as_mut_ptr() as *mut c_void,
            &mut length,
            ptr::null(),
            0,
        )
    } == -1
        || length != size_of::<u64>() * CPUSTATES
    {
        CRT_fatalError("sysctl kern.cp_time2 failed");
    }
}

/// Port of `static void kernelCPUTimesToHtop(const u_int64_t* times, CPUData*
/// cpu)` from `NetBSDMachine.c:180`. Computes the per-bucket periods
/// (saturating deltas from the previous sample) and rolls the running totals.
pub fn kernelCPUTimesToHtop(times: &[u64; CPUSTATES], cpu: &mut CPUData) {
    let mut totalTime: u64 = 0;
    for i in 0..CPUSTATES {
        totalTime += times[i];
    }

    let sysAllTime = times[CP_INTR] + times[CP_SYS];

    cpu.totalPeriod = saturatingSub(totalTime, cpu.totalTime);
    cpu.userPeriod = saturatingSub(times[CP_USER], cpu.userTime);
    cpu.nicePeriod = saturatingSub(times[CP_NICE], cpu.niceTime);
    cpu.sysPeriod = saturatingSub(times[CP_SYS], cpu.sysTime);
    cpu.sysAllPeriod = saturatingSub(sysAllTime, cpu.sysAllTime);
    cpu.intrPeriod = saturatingSub(times[CP_INTR], cpu.intrTime);
    cpu.idlePeriod = saturatingSub(times[CP_IDLE], cpu.idleTime);

    cpu.totalTime = totalTime;
    cpu.userTime = times[CP_USER];
    cpu.niceTime = times[CP_NICE];
    cpu.sysTime = times[CP_SYS];
    cpu.sysAllTime = sysAllTime;
    cpu.intrTime = times[CP_INTR];
    cpu.idleTime = times[CP_IDLE];
}

/// Port of `static void NetBSDMachine_scanCPUTime(NetBSDMachine* this)` from
/// `NetBSDMachine.c:205`. Scans each existing CPU's kernel times, accumulates
/// the average across active CPUs, and stores the average in `cpuData[0]`.
pub fn NetBSDMachine_scanCPUTime(this: &mut NetBSDMachine) {
    let mut kernelTimes: [u64; CPUSTATES] = [0; CPUSTATES];
    let mut avg: [u64; CPUSTATES] = [0; CPUSTATES];

    for i in 0..this.super_.existingCPUs {
        getKernelCPUTimes(i as c_int, &mut kernelTimes);
        let cpu = &mut this.cpuData[i as usize + 1];
        kernelCPUTimesToHtop(&kernelTimes, cpu);

        avg[CP_USER] += cpu.userTime;
        avg[CP_NICE] += cpu.niceTime;
        avg[CP_SYS] += cpu.sysTime;
        avg[CP_INTR] += cpu.intrTime;
        avg[CP_IDLE] += cpu.idleTime;
    }

    for i in 0..CPUSTATES {
        avg[i] /= this.super_.activeCPUs as u64;
    }

    kernelCPUTimesToHtop(&avg, &mut this.cpuData[0]);
}

/// Port of `static void NetBSDMachine_scanCPUFrequency(NetBSDMachine* this)`
/// from `NetBSDMachine.c:230`. Prefers the per-core
/// `machdep.cpufreq.cpuN.current` nodes (ARM big.LITTLE), then falls back to
/// the legacy single-core [`FREQ_SYSCTLS`] nodes, scaling each to MHz.
pub fn NetBSDMachine_scanCPUFrequency(this: &mut NetBSDMachine) {
    let cpus = this.super_.existingCPUs;
    let mut matched = false;
    let mut freq: c_long = 0;

    for i in 0..cpus {
        this.cpuData[i as usize + 1].frequency = f64::NAN;
    }

    /* newer hardware supports per-core frequency, for e.g. ARM big.LITTLE */
    for i in 0..cpus {
        let name = CString::new(format!("machdep.cpufreq.cpu{}.current", i)).unwrap();
        let mut freqSize = size_of::<c_long>();
        if unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                &mut freq as *mut c_long as *mut c_void,
                &mut freqSize,
                ptr::null(),
                0,
            )
        } != -1
        {
            this.cpuData[i as usize + 1].frequency = freq as f64; /* already in MHz */
            matched = true;
        }
    }

    if matched {
        return;
    }

    /*
     * Iterate through legacy sysctl nodes for single-core frequency until
     * we find a match...
     */
    for &(name, scale) in FREQ_SYSCTLS {
        let cname = CString::new(name).unwrap();
        let mut freqSize = size_of::<c_long>();
        if unsafe {
            libc::sysctlbyname(
                cname.as_ptr(),
                &mut freq as *mut c_long as *mut c_void,
                &mut freqSize,
                ptr::null(),
                0,
            )
        } != -1
        {
            freq /= scale; /* scale to MHz */
            matched = true;
            break;
        }
    }

    if matched {
        for i in 0..cpus {
            this.cpuData[i as usize + 1].frequency = freq as f64;
        }
    }
}

/// Port of `void Machine_scan(Machine* super)` from `NetBSDMachine.c:276`.
/// Refreshes the memory and CPU-time snapshots, and (when the setting is on)
/// the CPU frequencies.
pub fn Machine_scan(this: &mut NetBSDMachine) {
    NetBSDMachine_scanMemoryInfo(this);
    NetBSDMachine_scanCPUTime(this);

    let show_freq = this
        .super_
        .settings
        .as_ref()
        .map(|s| s.showCPUFrequency)
        .unwrap_or(false);
    if show_freq {
        NetBSDMachine_scanCPUFrequency(this);
    }
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

/// TODO: port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// from `NetBSDMachine.c:105`. Blocked: the C runs `Machine_init(super, ...)`,
/// but the ported `Machine_init` is `#[cfg(target_os = "macos")]` in
/// `machine.rs` and is not compiled for the NetBSD target; it cannot be
/// reached from here and `machine.rs` is out of scope for this module.
pub fn Machine_new() {
    todo!("port of NetBSDMachine.c:105 — blocked: base Machine_init is macos-gated")
}
