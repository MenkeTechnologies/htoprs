//! Port of `FreeBSDMachine.c` — the FreeBSD per-host `Machine`.
//!
//! Ported struct model:
//! - the [`CPUData`] struct (`FreeBSDMachine.h:20`) and the [`FreeBSDMachine`]
//!   struct (`FreeBSDMachine.h:31`), modeled `#[repr(C)]` (`super_` at offset
//!   0). The `kern.cp_time{,s}` clicks arrays and the `cpus` array — C
//!   `xCalloc`/`xRealloc` heap — are owned `Vec`s; the cached sysctl MIBs
//!   (C file-scope statics, one machine instance) live on the struct.
//!   `ZfsArcStats` is reused from the (platform-independent) zfs model.
//!
//! Ported functions:
//! - [`Machine_new`] (`FreeBSDMachine.c:53`),
//!   [`Machine_delete`] (`FreeBSDMachine.c:147`).
//! - [`FreeBSDMachine_scanCPU`] (`FreeBSDMachine.c:165`),
//!   [`FreeBSDMachine_scanMemoryInfo`] (`FreeBSDMachine.c:305`),
//!   [`Machine_scan`] (`FreeBSDMachine.c:389`).
//! - [`Machine_isCPUonline`] (`FreeBSDMachine.c:397`),
//!   [`Machine_getCPUPhysicalCoreID`] (`FreeBSDMachine.c:406`),
//!   [`Machine_getCPUThreadIndex`] (`FreeBSDMachine.c:412`).
//!
//! Deviation (documented, as the darwin port): `openzfs_sysctl_init` /
//! `openzfs_sysctl_updateArcStats` (`generic/openzfs_sysctl.c`) are unported,
//! so `zfs` stays zeroed (`enabled == 0`) — the ARC portion of the memory /
//! ZFS meters reads as empty until that substrate lands.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::mem::size_of;
use std::os::raw::{c_int, c_ulong, c_void};
use std::ptr;

use crate::ported::crt::CRT_fatalError;
use crate::ported::linux::linuxmachine::{memory_t, ZfsArcStats};
use crate::ported::machine::{Machine, Machine_done, Machine_init};

/// Port of `typedef struct CPUData_` (`FreeBSDMachine.h:20`) — the per-CPU
/// percentages and frequency/temperature for one core (plus, at index 0 on
/// SMP, the aggregate).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CPUData {
    pub userPercent: f64,
    pub nicePercent: f64,
    pub systemPercent: f64,
    pub irqPercent: f64,
    pub systemAllPercent: f64,
    pub frequency: f64,
    pub temperature: f64,
}

impl Default for CPUData {
    fn default() -> Self {
        // C xCalloc zeroes; the scan overwrites every field before use.
        CPUData {
            userPercent: 0.0,
            nicePercent: 0.0,
            systemPercent: 0.0,
            irqPercent: 0.0,
            systemAllPercent: 0.0,
            frequency: 0.0,
            temperature: 0.0,
        }
    }
}

/// Port of htop's `struct FreeBSDMachine_` (`FreeBSDMachine.h:31`). "Extends"
/// the base [`Machine`] via `super_` (first member); `#[repr(C)]` keeps
/// `super_` at offset 0 so htop's `(const FreeBSDMachine*)host` downcast — a
/// `*const Machine` obtained from a `FreeBSDMachine`, cast back — is sound
/// (used by `freebsd/Platform.c` and `FreeBSDProcessTable.c`).
///
/// The kernel clicks arrays (`cp_time_*` / `cp_times_*`) and the `cpus` array
/// are owned `Vec`s (C's `xCalloc`/`xRealloc` heap); the cached sysctl MIBs
/// (C file-scope statics) are struct fields since there is a single machine
/// instance.
#[repr(C)]
pub struct FreeBSDMachine {
    /// C `Machine super` — the embedded base machine.
    pub super_: Machine,
    /// C `kvm_t* kd` — the libkvm descriptor (swap info).
    pub kd: *mut libc::kvm_t,

    pub pageSize: c_int,
    pub pageSizeKb: c_int,
    pub kernelFScale: c_int,

    pub wiredMem: memory_t,
    pub buffersMem: memory_t,
    pub activeMem: memory_t,
    pub laundryMem: memory_t,
    pub inactiveMem: memory_t,
    pub arcMem: memory_t,

    /// C `ZfsArcStats zfs`.
    pub zfs: ZfsArcStats,

    /// C `CPUData* cpus` — `existingCPUs` cores (+1 aggregate slot on SMP).
    pub cpus: Vec<CPUData>,

    /// C `unsigned long* cp_time_o/_n` — single-CPU (or average) clicks.
    pub cp_time_o: Vec<c_ulong>,
    pub cp_time_n: Vec<c_ulong>,
    /// C `unsigned long* cp_times_o/_n` — per-core clicks (SMP only).
    pub cp_times_o: Vec<c_ulong>,
    pub cp_times_n: Vec<c_ulong>,

    // Cached sysctl MIBs (C file-scope `MIB_*`).
    pub MIB_hw_physmem: [c_int; 2],
    pub MIB_vm_stats_vm_v_wire_count: [c_int; 4],
    pub MIB_vm_stats_vm_v_active_count: [c_int; 4],
    pub MIB_vm_stats_vm_v_laundry_count: [c_int; 4],
    pub MIB_vm_stats_vm_v_inactive_count: [c_int; 4],
    pub MIB_vfs_bufspace: [c_int; 2],
    pub MIB_kern_cp_time: [c_int; 2],
    pub MIB_kern_cp_times: [c_int; 2],
}

/// Port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)` from
/// `FreeBSDMachine.c:53`. Allocates a `FreeBSDMachine` (C `xCalloc`, mirrored
/// by explicit zero-init), runs the base [`Machine_init`], caches the sysctl
/// MIBs, resolves the usable page size via `sysctl(vm.stats.vm.v_page_size)`,
/// detects SMP + CPU count, allocates and fetches the initial `kern.cp_time{,s}`
/// clicks, reads `kern.fscale` (default 2048), and opens the `kvm` descriptor.
/// Returns the owning `Box<FreeBSDMachine>` (C returns `&this->super`); the
/// caller derives `*mut Machine` from `&mut box.super_`.
///
/// Deviations: `openzfs_sysctl_init` / `openzfs_sysctl_updateArcStats`
/// (`generic/openzfs_sysctl.c`) are unported — skipped, as in
/// [`Machine_scan`]. The C caches a `MIB_vm_stats_vm_v_page_count` file-scope
/// static that is never read; the [`FreeBSDMachine`] struct model omits it, so
/// its `sysctlnametomib` is resolved into a throwaway to preserve the side
/// effect.
pub fn Machine_new(usersTable: Option<usize>, userId: u32) -> Box<FreeBSDMachine> {
    // Nested helper for the repeated `len = N; sysctlnametomib(name, MIB, &len)`
    // idiom the C inlines per MIB (`len` is the array element count, in/out).
    fn nametomib(name: &core::ffi::CStr, mib: &mut [c_int]) {
        let mut len: libc::size_t = mib.len();
        unsafe {
            libc::sysctlnametomib(name.as_ptr(), mib.as_mut_ptr(), &mut len);
        }
    }

    // FreeBSDMachine* this = xCalloc(1, sizeof(FreeBSDMachine));
    let mut this = Box::new(FreeBSDMachine {
        super_: Machine::default(),
        kd: ptr::null_mut(),
        pageSize: 0,
        pageSizeKb: 0,
        kernelFScale: 0,
        wiredMem: 0,
        buffersMem: 0,
        activeMem: 0,
        laundryMem: 0,
        inactiveMem: 0,
        arcMem: 0,
        zfs: ZfsArcStats::default(),
        cpus: Vec::new(),
        cp_time_o: Vec::new(),
        cp_time_n: Vec::new(),
        cp_times_o: Vec::new(),
        cp_times_n: Vec::new(),
        MIB_hw_physmem: [0; 2],
        MIB_vm_stats_vm_v_wire_count: [0; 4],
        MIB_vm_stats_vm_v_active_count: [0; 4],
        MIB_vm_stats_vm_v_laundry_count: [0; 4],
        MIB_vm_stats_vm_v_inactive_count: [0; 4],
        MIB_vfs_bufspace: [0; 2],
        MIB_kern_cp_time: [0; 2],
        MIB_kern_cp_times: [0; 2],
    });

    Machine_init(&mut this.super_, usersTable, userId);

    // physical memory in system: hw.physmem
    nametomib(c"hw.physmem", &mut this.MIB_hw_physmem);

    // usable pagesize : vm.stats.vm.v_page_size
    let mut len = size_of::<c_int>();
    if unsafe {
        libc::sysctlbyname(
            c"vm.stats.vm.v_page_size".as_ptr(),
            &mut this.pageSize as *mut c_int as *mut c_void,
            &mut len,
            ptr::null_mut(),
            0,
        )
    } == -1
    {
        CRT_fatalError("Cannot get pagesize by sysctl");
    }
    this.pageSizeKb = this.pageSize / 1024; // ONE_K

    // usable page count vm.stats.vm.v_page_count — the C caches this MIB but
    // never reads it; the struct model omits it, so resolve into a throwaway.
    nametomib(c"vm.stats.vm.v_page_count", &mut [0 as c_int; 4]);

    nametomib(
        c"vm.stats.vm.v_wire_count",
        &mut this.MIB_vm_stats_vm_v_wire_count,
    );
    nametomib(
        c"vm.stats.vm.v_active_count",
        &mut this.MIB_vm_stats_vm_v_active_count,
    );
    nametomib(
        c"vm.stats.vm.v_laundry_count",
        &mut this.MIB_vm_stats_vm_v_laundry_count,
    );
    nametomib(
        c"vm.stats.vm.v_inactive_count",
        &mut this.MIB_vm_stats_vm_v_inactive_count,
    );

    nametomib(c"vfs.bufspace", &mut this.MIB_vfs_bufspace);

    // openzfs_sysctl_init / openzfs_sysctl_updateArcStats — ZFS substrate unported.

    let mut smp: c_int = 0;
    let mut len = size_of::<c_int>();
    if unsafe {
        libc::sysctlbyname(
            c"kern.smp.active".as_ptr(),
            &mut smp as *mut c_int as *mut c_void,
            &mut len,
            ptr::null_mut(),
            0,
        )
    } != 0
        || len != size_of::<c_int>()
    {
        smp = 0;
    }

    let mut cpus: c_int = 1;
    let mut len = size_of::<c_int>();
    if smp != 0 {
        let err = unsafe {
            libc::sysctlbyname(
                c"kern.smp.cpus".as_ptr(),
                &mut cpus as *mut c_int as *mut c_void,
                &mut len,
                ptr::null_mut(),
                0,
            )
        };
        if err != 0 {
            cpus = 1;
        }
    } else {
        cpus = 1;
    }

    let cpustates = libc::CPUSTATES as usize;
    let sizeof_cp_time_array = size_of::<c_ulong>() * cpustates;
    nametomib(c"kern.cp_time", &mut this.MIB_kern_cp_time);
    this.cp_time_o = vec![0 as c_ulong; cpustates];
    this.cp_time_n = vec![0 as c_ulong; cpustates];

    // fetch initial single (or average) CPU clicks from kernel
    let mut len = sizeof_cp_time_array;
    unsafe {
        libc::sysctl(
            this.MIB_kern_cp_time.as_ptr() as *mut c_int,
            2,
            this.cp_time_o.as_mut_ptr() as *mut c_void,
            &mut len,
            ptr::null_mut(),
            0,
        );
    }

    // on smp box, fetch rest of initial CPU's clicks
    if cpus > 1 {
        nametomib(c"kern.cp_times", &mut this.MIB_kern_cp_times);
        this.cp_times_o = vec![0 as c_ulong; cpus as usize * cpustates];
        this.cp_times_n = vec![0 as c_ulong; cpus as usize * cpustates];
        let mut len = cpus as usize * sizeof_cp_time_array;
        unsafe {
            libc::sysctl(
                this.MIB_kern_cp_times.as_ptr() as *mut c_int,
                2,
                this.cp_times_o.as_mut_ptr() as *mut c_void,
                &mut len,
                ptr::null_mut(),
                0,
            );
        }
    }

    this.super_.existingCPUs = std::cmp::max(cpus, 1) as u32;
    // TODO: support offline CPUs and hot swapping
    this.super_.activeCPUs = this.super_.existingCPUs;

    if cpus == 1 {
        this.cpus.resize(1, CPUData::default());
    } else {
        // on smp we need CPUs + 1 to store averages too (as kernel kindly
        // provides that as well)
        this.cpus
            .resize(this.super_.existingCPUs as usize + 1, CPUData::default());
    }

    let mut len = size_of::<c_int>();
    if unsafe {
        libc::sysctlbyname(
            c"kern.fscale".as_ptr(),
            &mut this.kernelFScale as *mut c_int as *mut c_void,
            &mut len,
            ptr::null_mut(),
            0,
        )
    } == -1
        || this.kernelFScale <= 0
    {
        // sane default for kernel-provided CPU percentage scaling, at least on
        // x86 machines, in case this sysctl call failed
        this.kernelFScale = 2048;
    }

    let mut errbuf = [0 as libc::c_char; libc::_POSIX2_LINE_MAX as usize];
    this.kd = unsafe {
        libc::kvm_openfiles(
            ptr::null(),
            c"/dev/null".as_ptr(),
            ptr::null(),
            0,
            errbuf.as_mut_ptr(),
        )
    };
    if this.kd.is_null() {
        CRT_fatalError("kvm_openfiles() failed");
    }

    this
}

/// Port of `void Machine_delete(Machine* super)` from `FreeBSDMachine.c:147`.
/// Runs the base [`Machine_done`], closes the libkvm descriptor, and drops
/// the machine. The C `free`s of the clicks / `cpus` arrays and `free(this)`
/// are Rust `Drop` (consuming the owning `Box`).
pub fn Machine_delete(mut this: Box<FreeBSDMachine>) {
    Machine_done(&mut this.super_);

    if !this.kd.is_null() {
        unsafe { libc::kvm_close(this.kd) };
    }

    // free(cp_time_o/n), free(cp_times_o/n), free(cpus), free(this) → Drop.
}

/// Port of `static inline void FreeBSDMachine_scanCPU(Machine* super)` from
/// `FreeBSDMachine.c:165`. Re-fetches the `kern.cp_time{,s}` clicks, diffs
/// them against the stored previous snapshot to compute per-core
/// user/nice/system/irq percentages, rotates the snapshot, and — when the
/// settings request it — reads per-core `dev.cpu.N.temperature`/`.freq`,
/// then derives the aggregate (slot 0) max temperature and average frequency.
pub fn FreeBSDMachine_scanCPU(this: &mut FreeBSDMachine) {
    let cpus = this.super_.existingCPUs; // actual CPU count
    let mut maxcpu = cpus; // max iteration (average + smp)
    debug_assert!(cpus > 0);

    let cpustates = libc::CPUSTATES as usize;

    // get averages or single CPU clicks
    let mut len = size_of::<c_ulong>() * cpustates;
    unsafe {
        libc::sysctl(
            this.MIB_kern_cp_time.as_ptr() as *mut c_int,
            2,
            this.cp_time_n.as_mut_ptr() as *mut c_void,
            &mut len,
            ptr::null_mut(),
            0,
        );
    }

    // get rest of CPUs
    if cpus > 1 {
        maxcpu = cpus + 1;
        let mut len = cpus as usize * size_of::<c_ulong>() * cpustates;
        unsafe {
            libc::sysctl(
                this.MIB_kern_cp_times.as_ptr() as *mut c_int,
                2,
                this.cp_times_n.as_mut_ptr() as *mut c_void,
                &mut len,
                ptr::null_mut(),
                0,
            );
        }
    }

    let show_temp = this
        .super_
        .settings
        .as_ref()
        .is_some_and(|s| s.showCPUTemperature);
    let show_freq = this
        .super_
        .settings
        .as_ref()
        .is_some_and(|s| s.showCPUFrequency);

    for i in 0..maxcpu {
        let mut cp_time_p = [0.0f64; 8]; // CPUSTATES is 5; oversize is harmless

        {
            // Select old/new clicks (single-CPU/average vs. per-core offset).
            let (n, o): (&[c_ulong], &mut [c_ulong]) = if cpus == 1 || i == 0 {
                (&this.cp_time_n, &mut this.cp_time_o)
            } else {
                let off = (i as usize - 1) * cpustates;
                (
                    &this.cp_times_n[off..off + cpustates],
                    &mut this.cp_times_o[off..off + cpustates],
                )
            };

            // diff old vs new
            let mut total_o: u64 = 0;
            let mut total_n: u64 = 0;
            let mut cp_time_d = [0 as c_ulong; 8];
            for s in 0..cpustates {
                cp_time_d[s] = n[s].wrapping_sub(o[s]);
                total_o += o[s] as u64;
                total_n += n[s] as u64;
            }

            let mut total_d = total_n.wrapping_sub(total_o);
            if total_d < 1 {
                total_d = 1;
            }

            // save current state as old and calc percentages
            for s in 0..cpustates {
                o[s] = n[s];
                cp_time_p[s] = (cp_time_d[s] as f64) / (total_d as f64) * 100.0;
            }
        }

        let cpuData = &mut this.cpus[i as usize];
        cpuData.userPercent = cp_time_p[libc::CP_USER as usize];
        cpuData.nicePercent = cp_time_p[libc::CP_NICE as usize];
        cpuData.systemPercent = cp_time_p[libc::CP_SYS as usize];
        cpuData.irqPercent = cp_time_p[libc::CP_INTR as usize];
        cpuData.systemAllPercent =
            cp_time_p[libc::CP_SYS as usize] + cp_time_p[libc::CP_INTR as usize];

        cpuData.temperature = f64::NAN;
        cpuData.frequency = f64::NAN;

        let coreId = if cpus == 1 { 0 } else { i as i32 - 1 };
        if coreId < 0 {
            continue;
        }

        // TODO: test with hyperthreading and multi-cpu systems
        if show_temp {
            let mut temperature: c_int = 0;
            let mut len = size_of::<c_int>();
            let mib = format!("dev.cpu.{}.temperature\0", coreId);
            let r = unsafe {
                libc::sysctlbyname(
                    mib.as_ptr() as *const libc::c_char,
                    &mut temperature as *mut c_int as *mut c_void,
                    &mut len,
                    ptr::null_mut(),
                    0,
                )
            };
            if r == 0 {
                // convert from deci-Kelvin to Celsius
                cpuData.temperature = (temperature - 2732) as f64 / 10.0;
            }
        }

        // TODO: test with hyperthreading and multi-cpu systems
        if show_freq {
            let mut frequency: c_int = 0;
            let mut len = size_of::<c_int>();
            let mib = format!("dev.cpu.{}.freq\0", coreId);
            let r = unsafe {
                libc::sysctlbyname(
                    mib.as_ptr() as *const libc::c_char,
                    &mut frequency as *mut c_int as *mut c_void,
                    &mut len,
                    ptr::null_mut(),
                    0,
                )
            };
            if r == 0 {
                cpuData.frequency = frequency as f64; // keep in MHz
            }
        }
    }

    // calculate max temperature and avg frequency for the aggregate meter and
    // propagate frequency to all cores if only supplied for CPU 0
    if cpus > 1 {
        if show_temp {
            let mut maxTemp = f64::NEG_INFINITY;
            for i in 1..maxcpu as usize {
                if this.cpus[i].temperature > maxTemp {
                    maxTemp = this.cpus[i].temperature;
                    this.cpus[0].temperature = maxTemp;
                }
            }
        }

        if show_freq {
            let coreZeroFreq = this.cpus[1].frequency;
            let mut freqSum = coreZeroFreq;
            if coreZeroFreq >= 0.0 {
                for i in 2..maxcpu as usize {
                    if !(this.cpus[i].frequency >= 0.0) {
                        this.cpus[i].frequency = coreZeroFreq;
                    }
                    freqSum += this.cpus[i].frequency;
                }
                this.cpus[0].frequency = freqSum / (maxcpu - 1) as f64;
            }
        }
    }
}

/// Port of `static void FreeBSDMachine_scanMemoryInfo(Machine* super)` from
/// `FreeBSDMachine.c:305`. Reads the total / active / wired / inactive /
/// laundry / buffers page counters via the cached MIBs (scaling by the page
/// size), deducts buffers from wired, and totals the swap usage via
/// `kvm_getswapinfo`.
pub fn FreeBSDMachine_scanMemoryInfo(this: &mut FreeBSDMachine) {
    // Local `sysctl(MIB_v_*_count, 4, &memX, ...)` page-count read (the C
    // repeats this inline per class); nested so it stays a faithful translation
    // without a module-level non-C function. Returns the count on success and
    // `> 0`, else 0 (the C `else this->xMem = 0` fallback).
    fn read_page_count(mib: &[c_int; 4]) -> memory_t {
        let mut count: libc::c_uint = 0;
        let mut len = size_of::<libc::c_uint>();
        let r = unsafe {
            libc::sysctl(
                mib.as_ptr() as *mut c_int,
                4,
                &mut count as *mut libc::c_uint as *mut c_void,
                &mut len,
                ptr::null_mut(),
                0,
            )
        };
        if r == 0 && count > 0 {
            count as memory_t
        } else {
            0
        }
    }

    let page_kb = this.pageSizeKb as memory_t;

    // total memory
    let mut totalMem: libc::c_ulong = 0;
    let mut len = size_of::<libc::c_ulong>();
    if unsafe {
        libc::sysctl(
            this.MIB_hw_physmem.as_ptr() as *mut c_int,
            2,
            &mut totalMem as *mut libc::c_ulong as *mut c_void,
            &mut len,
            ptr::null_mut(),
            0,
        )
    } == 0
        && totalMem > 0
    {
        this.super_.totalMem = totalMem as u64 / 1024;
    } else {
        this.super_.totalMem = 0;
    }

    // "active" pages
    this.activeMem = read_page_count(&this.MIB_vm_stats_vm_v_active_count) * page_kb;
    // "wired" pages
    this.wiredMem = read_page_count(&this.MIB_vm_stats_vm_v_wire_count) * page_kb;
    // "inactive" pages
    this.inactiveMem = read_page_count(&this.MIB_vm_stats_vm_v_inactive_count) * page_kb;
    // "laundry" pages
    this.laundryMem = read_page_count(&this.MIB_vm_stats_vm_v_laundry_count) * page_kb;

    // "buffers" pages (separate read, deducted from 'wired')
    let mut buffersMem: libc::c_long = 0;
    let mut len = size_of::<libc::c_long>();
    if unsafe {
        libc::sysctl(
            this.MIB_vfs_bufspace.as_ptr() as *mut c_int,
            2,
            &mut buffersMem as *mut libc::c_long as *mut c_void,
            &mut len,
            ptr::null_mut(),
            0,
        )
    } == 0
        && buffersMem > 0
    {
        this.buffersMem = buffersMem as memory_t / 1024;
    } else {
        this.buffersMem = 0;
    }
    // subtract (NB: "buffers" can't be larger than "wired")
    this.wiredMem = this.wiredMem.saturating_sub(this.buffersMem);

    // swap
    let mut swap: [libc::kvm_swap; 16] = unsafe { std::mem::zeroed() };
    let nswap =
        unsafe { libc::kvm_getswapinfo(this.kd, swap.as_mut_ptr(), swap.len() as c_int, 0) };
    this.super_.totalSwap = 0;
    this.super_.usedSwap = 0;
    for s in swap.iter().take(nswap.max(0) as usize) {
        this.super_.totalSwap += s.ksw_total as u64;
        this.super_.usedSwap += s.ksw_used as u64;
    }
    this.super_.totalSwap *= page_kb;
    this.super_.usedSwap *= page_kb;
}

/// Port of `void Machine_scan(Machine* super)` from `FreeBSDMachine.c:389`.
/// Refreshes the ZFS ARC stats (deviation: unported, see the module docs),
/// then rescans memory and CPU.
pub fn Machine_scan(this: &mut FreeBSDMachine) {
    // openzfs_sysctl_updateArcStats(&this.zfs) — ZFS substrate unported.
    FreeBSDMachine_scanMemoryInfo(this);
    FreeBSDMachine_scanCPU(this);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn super_is_at_offset_zero() {
        assert_eq!(core::mem::offset_of!(FreeBSDMachine, super_), 0);
    }
}
