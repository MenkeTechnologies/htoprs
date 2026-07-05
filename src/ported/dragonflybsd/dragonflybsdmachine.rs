//! Port of `dragonflybsd/DragonFlyBSDMachine.c` + `.h` — the DragonFly BSD
//! per-host state and its `sysctl`/`libkvm` scan layer.
//!
//! The struct model, the small pure accessors, and the pure-`sysctl` scans are
//! ported here. Compiled only under `#[cfg(target_os = "dragonfly")]` and, like
//! the other BSD layers, verified by primary-source reading + the port-purity
//! gate (not a cross-compile).
//!
//! Ported: the pure CPU accessors, [`DragonFlyBSDMachine_scanJails`]
//! (`jail.list` sysctlbyname → the `jails` hashtable) /
//! [`DragonFlyBSDMachine_readJailName`], plus the full `sysctl`/`libkvm` scan
//! layer — [`Machine_new`] (`kvm_openfiles` + `sysctlnametomib` MIB init),
//! [`Machine_delete`] (`kvm_close`), [`DragonFlyBSDMachine_scanCPUTime`]
//! (`kern.cp_time(s)` deltas), [`DragonFlyBSDMachine_scanMemoryInfo`]
//! (`vm.stats.vm.*` + `kvm_getswapinfo`), and [`Machine_scan`].
//!
//! `kvm_openfiles`/`kvm_close`/`sysctlnametomib` are in the shared `freebsdlike`
//! `libc`; only `kvm_getswapinfo` (+ its `kvm_swap` record) is hand-declared in
//! an `extern` block — the NetBSD hand-rolled-kvm precedent.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::ffi::c_void;
use core::ptr;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_uint};

use crate::ported::crt::CRT_fatalError;
use crate::ported::hashtable::{Hashtable, Hashtable_get, Hashtable_new, Hashtable_put};
use crate::ported::machine::{Machine, Machine_init};
use crate::ported::object::{Object, ObjectClass};

// `sys/resource.h`: CPUSTATES = 5 buckets, indexed by CP_USER/NICE/SYS/INTR/IDLE.
const CPUSTATES: usize = 5;
const CP_USER: usize = 0;
const CP_NICE: usize = 1;
const CP_SYS: usize = 2;
const CP_INTR: usize = 3;
const CP_IDLE: usize = 4;
// `Macros.h`: ONE_K = 1024.
const ONE_K: i32 = 1024;

/// Port of `struct kvm_swap` (DragonFly `sys/kvm.h`) — the swap-usage record
/// `kvm_getswapinfo` fills. Not modeled by `libc` for the DragonFly target;
/// declared here matching FreeBSD's `kvm_swap` (DragonFly forked from FreeBSD).
#[repr(C)]
#[derive(Clone, Copy)]
struct kvm_swap {
    ksw_devname: [c_char; 32],
    ksw_used: c_uint,
    ksw_total: c_uint,
    ksw_flags: c_int,
    ksw_reserved1: c_int,
    ksw_reserved2: c_int,
}

extern "C" {
    /// `int kvm_getswapinfo(kvm_t*, struct kvm_swap*, int maxswap, int flags)`
    /// (DragonFly `kvm.h`). Not exposed by `libc` for the DragonFly target — the
    /// NetBSD hand-rolled-kvm precedent. (`kvm_openfiles`/`sysctlnametomib` ARE
    /// in shared `freebsdlike` `libc`, so those use `libc::`.)
    fn kvm_getswapinfo(kd: *mut c_void, swap: *mut kvm_swap, maxswap: c_int, flags: c_int)
        -> c_int;
}

/// The `char*` hostname value stored in the `jails` [`Hashtable`] (jailid →
/// hostname). The C stores a raw `xStrdup`'d `char*`; the ported `Hashtable`
/// stores `Object`s, so the string is wrapped (no C struct — the value type is
/// a bare `char*` in htop).
struct JailName(String);

/// Class identity for [`JailName`] (`extends: None`, as the file-local
/// `LibraryData` accumulator elsewhere).
static JailName_class: ObjectClass = ObjectClass { extends: None };

impl Object for JailName {
    fn klass(&self) -> &'static ObjectClass {
        &JailName_class
    }
}

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
    // The C uses file-scope `static int MIB_*[]` arrays set in `Machine_new`;
    // the port carries them as struct fields (the FreeBSD-machine precedent).
    pub MIB_hw_physmem: [c_int; 2],
    pub MIB_vm_stats_vm_v_page_count: [c_int; 4],
    pub MIB_vm_stats_vm_v_wire_count: [c_int; 4],
    pub MIB_vm_stats_vm_v_active_count: [c_int; 4],
    pub MIB_vm_stats_vm_v_cache_count: [c_int; 4],
    pub MIB_vm_stats_vm_v_inactive_count: [c_int; 4],
    pub MIB_vfs_bufspace: [c_int; 2],
    pub MIB_kern_cp_time: [c_int; 2],
    pub MIB_kern_cp_times: [c_int; 2],
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

/// Port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// (`DragonFlyBSDMachine.c:41`). Opens `kvm_openfiles`, resolves the usable page
/// size / physmem / v_*_count MIBs via `sysctlnametomib`, and allocates the CPU
/// / `cp_time(s)` tick buffers. Returns the owning `Box<DragonFlyBSDMachine>`
/// (C returns `&this->super`); the `cpus`/`cp_time*` heap arrays are `Vec`s
/// (the FreeBSD-machine precedent). `sysctlnametomib`/`kvm_openfiles` are in the
/// shared `freebsdlike` `libc`; only `kvm_getswapinfo` (used by `scanMemoryInfo`)
/// needs the hand-declared `extern`.
pub fn Machine_new(usersTable: Option<usize>, userId: u32) -> Box<DragonFlyBSDMachine> {
    // Nested helper for the repeated `len = N; sysctlnametomib(name, MIB, &len)`
    // idiom the C inlines per MIB (`len` is the array element count, in/out).
    fn nametomib(name: &core::ffi::CStr, mib: &mut [c_int]) {
        let mut len: libc::size_t = mib.len();
        unsafe {
            libc::sysctlnametomib(name.as_ptr(), mib.as_mut_ptr(), &mut len);
        }
    }

    // DragonFlyBSDMachine* this = xCalloc(1, sizeof(DragonFlyBSDMachine));
    let mut this = Box::new(DragonFlyBSDMachine {
        super_: Machine::default(),
        kd: ptr::null_mut(),
        jails: None,
        pageSize: 0,
        pageSizeKb: 0,
        kernelFScale: 0,
        wiredMem: 0,
        buffersMem: 0,
        activeMem: 0,
        inactiveMem: 0,
        cacheMem: 0,
        cpus: Vec::new(),
        cp_time_o: Vec::new(),
        cp_time_n: Vec::new(),
        cp_times_o: Vec::new(),
        cp_times_n: Vec::new(),
        MIB_hw_physmem: [0; 2],
        MIB_vm_stats_vm_v_page_count: [0; 4],
        MIB_vm_stats_vm_v_wire_count: [0; 4],
        MIB_vm_stats_vm_v_active_count: [0; 4],
        MIB_vm_stats_vm_v_cache_count: [0; 4],
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
            &mut this.pageSize as *mut i32 as *mut c_void,
            &mut len,
            ptr::null(),
            0,
        )
    } == -1
    {
        CRT_fatalError("Cannot get pagesize by sysctl");
    }
    this.pageSizeKb = this.pageSize / ONE_K;

    // usable page count vm.stats.vm.v_page_count and the partition counters
    nametomib(
        c"vm.stats.vm.v_page_count",
        &mut this.MIB_vm_stats_vm_v_page_count,
    );
    nametomib(
        c"vm.stats.vm.v_wire_count",
        &mut this.MIB_vm_stats_vm_v_wire_count,
    );
    nametomib(
        c"vm.stats.vm.v_active_count",
        &mut this.MIB_vm_stats_vm_v_active_count,
    );
    nametomib(
        c"vm.stats.vm.v_cache_count",
        &mut this.MIB_vm_stats_vm_v_cache_count,
    );
    nametomib(
        c"vm.stats.vm.v_inactive_count",
        &mut this.MIB_vm_stats_vm_v_inactive_count,
    );
    nametomib(c"vfs.bufspace", &mut this.MIB_vfs_bufspace);

    let mut cpus: c_int = 1;
    let mut len = size_of::<c_int>();
    if unsafe {
        libc::sysctlbyname(
            c"hw.ncpu".as_ptr(),
            &mut cpus as *mut c_int as *mut c_void,
            &mut len,
            ptr::null(),
            0,
        )
    } != 0
    {
        cpus = 1;
    }

    let sizeof_cp_time_array = size_of::<u64>() * CPUSTATES; // unsigned long = u64 (LP64)
    nametomib(c"kern.cp_time", &mut this.MIB_kern_cp_time);
    this.cp_time_o = vec![0u64; CPUSTATES];
    this.cp_time_n = vec![0u64; CPUSTATES];

    // fetch initial single (or average) CPU clicks from kernel
    let mut len = sizeof_cp_time_array;
    unsafe {
        libc::sysctl(
            this.MIB_kern_cp_time.as_ptr(),
            2,
            this.cp_time_o.as_mut_ptr() as *mut c_void,
            &mut len,
            ptr::null(),
            0,
        );
    }

    // on smp box, fetch rest of initial CPU's clicks
    if cpus > 1 {
        nametomib(c"kern.cp_times", &mut this.MIB_kern_cp_times);
        this.cp_times_o = vec![0u64; cpus as usize * CPUSTATES];
        this.cp_times_n = vec![0u64; cpus as usize * CPUSTATES];
        let mut len = cpus as usize * sizeof_cp_time_array;
        unsafe {
            libc::sysctl(
                this.MIB_kern_cp_times.as_ptr(),
                2,
                this.cp_times_o.as_mut_ptr() as *mut c_void,
                &mut len,
                ptr::null(),
                0,
            );
        }
    }

    this.super_.existingCPUs = cpus.max(1) as u32;
    // TODO: support offline CPUs and hot swapping
    this.super_.activeCPUs = this.super_.existingCPUs;

    // cpus==1 → one entry; on smp we need CPUs+1 to store the average too.
    let cpu_data_count = if cpus == 1 {
        1
    } else {
        this.super_.existingCPUs as usize + 1
    };
    this.cpus = vec![CPUData::default(); cpu_data_count];

    let mut len = size_of::<c_int>();
    if unsafe {
        libc::sysctlbyname(
            c"kern.fscale".as_ptr(),
            &mut this.kernelFScale as *mut i32 as *mut c_void,
            &mut len,
            ptr::null(),
            0,
        )
    } == -1
        || this.kernelFScale <= 0
    {
        // sane default for kernel-provided CPU percentage scaling on x86
        this.kernelFScale = 2048;
    }

    // char errbuf[_POSIX2_LINE_MAX]; kd = kvm_openfiles(NULL, "/dev/null", NULL, 0, errbuf);
    let mut errbuf = [0 as c_char; 2048]; // _POSIX2_LINE_MAX
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

/// Port of `void Machine_delete(Machine* super)`
/// (`DragonFlyBSDMachine.c:119`). Runs the base [`Machine_done`], closes the
/// libkvm descriptor, and drops the machine. The C `Hashtable_delete(jails)`,
/// the `free`s of the `cp_time*` / `cpus` arrays, and `free(this)` are Rust
/// `Drop` (consuming the owning `Box`).
pub fn Machine_delete(mut this: Box<DragonFlyBSDMachine>) {
    crate::ported::machine::Machine_done(&mut this.super_);

    if !this.kd.is_null() {
        unsafe {
            libc::kvm_close(this.kd);
        }
    }
}

/// Port of `static void DragonFlyBSDMachine_scanCPUTime(Machine* super)`
/// (`DragonFlyBSDMachine.c:141`). Re-reads `kern.cp_time` (and `kern.cp_times`
/// on smp) into the `_n` tick buffers, diffs against the `_o` buffers, and
/// stores per-CPU load percentages in `this.cpus[i]` (index 0 = the average,
/// then one per core). The C casts away `const` to update the `_o` buffers and
/// `cpus`, so the port takes `&mut`. The `_n`/`_o` pointer selection (average
/// vs. per-core `+ offset*CPUSTATES`) is mirrored by the nested index helpers.
pub fn DragonFlyBSDMachine_scanCPUTime(this: &mut DragonFlyBSDMachine) {
    // Current ("new") tick for CPU `i`, state `s` — the C's `cp_time_n` pointer:
    // the aggregate buffer for the single CPU / the average (i == 0), else the
    // per-core slice at `(i-1) * CPUSTATES`.
    fn n_at(this: &DragonFlyBSDMachine, cpus: u32, i: u32, s: usize) -> u64 {
        if cpus == 1 || i == 0 {
            this.cp_time_n[s]
        } else {
            this.cp_times_n[(i as usize - 1) * CPUSTATES + s]
        }
    }
    fn o_at(this: &DragonFlyBSDMachine, cpus: u32, i: u32, s: usize) -> u64 {
        if cpus == 1 || i == 0 {
            this.cp_time_o[s]
        } else {
            this.cp_times_o[(i as usize - 1) * CPUSTATES + s]
        }
    }
    fn set_o(this: &mut DragonFlyBSDMachine, cpus: u32, i: u32, s: usize, v: u64) {
        if cpus == 1 || i == 0 {
            this.cp_time_o[s] = v;
        } else {
            this.cp_times_o[(i as usize - 1) * CPUSTATES + s] = v;
        }
    }

    let cpus = this.super_.existingCPUs; // actual CPU count
    let mut maxcpu = cpus; // max iteration (average + smp)
    assert!(cpus > 0);

    // get averages or single CPU clicks
    let mut len = size_of::<u64>() * CPUSTATES;
    unsafe {
        libc::sysctl(
            this.MIB_kern_cp_time.as_ptr(),
            2,
            this.cp_time_n.as_mut_ptr() as *mut c_void,
            &mut len,
            ptr::null(),
            0,
        );
    }

    // get rest of CPUs — the kernel concats all CPU states into one array in
    // the kern.cp_times OID; averages live in cpus[0], real cores after.
    if cpus > 1 {
        maxcpu = cpus + 1;
        let mut len = cpus as usize * size_of::<u64>() * CPUSTATES;
        unsafe {
            libc::sysctl(
                this.MIB_kern_cp_times.as_ptr(),
                2,
                this.cp_times_n.as_mut_ptr() as *mut c_void,
                &mut len,
                ptr::null(),
                0,
            );
        }
    }

    for i in 0..maxcpu {
        // diff old vs new
        let mut cp_time_d = [0u64; CPUSTATES];
        let mut total_o: u64 = 0;
        let mut total_n: u64 = 0;
        for s in 0..CPUSTATES {
            let n = n_at(this, cpus, i, s);
            let o = o_at(this, cpus, i, s);
            cp_time_d[s] = n.wrapping_sub(o);
            total_o = total_o.wrapping_add(o);
            total_n = total_n.wrapping_add(n);
        }

        // totals
        let mut total_d = total_n.wrapping_sub(total_o);
        if total_d < 1 {
            total_d = 1;
        }

        // save current state as old and calc percentages
        let mut cp_time_p = [0.0f64; CPUSTATES];
        for s in 0..CPUSTATES {
            let n = n_at(this, cpus, i, s);
            set_o(this, cpus, i, s, n);
            cp_time_p[s] = (cp_time_d[s] as f64) / (total_d as f64) * 100.0;
        }

        let cpuData = &mut this.cpus[i as usize];
        cpuData.userPercent = cp_time_p[CP_USER];
        cpuData.nicePercent = cp_time_p[CP_NICE];
        cpuData.systemPercent = cp_time_p[CP_SYS];
        cpuData.irqPercent = cp_time_p[CP_INTR];
        cpuData.systemAllPercent = cp_time_p[CP_SYS] + cp_time_p[CP_INTR];
        // this one is not really used, but we store it anyway
        cpuData.idlePercent = cp_time_p[CP_IDLE];
    }
}

/// Port of `static void DragonFlyBSDMachine_scanMemoryInfo(Machine* super)`
/// (`DragonFlyBSDMachine.c:223`). Reads the `hw.physmem` / `vm.stats.vm.v_*`
/// counters and `vfs.bufspace` via the cached MIBs, scales the page counts by
/// `pageSizeKb`, subtracts buffers from wired, then sums swap usage via the
/// hand-declared `kvm_getswapinfo`.
pub fn DragonFlyBSDMachine_scanMemoryInfo(this: &mut DragonFlyBSDMachine) {
    // Nested helper for the repeated `len = sizeof(x); sysctl(MIB, n, &x, &len,
    // NULL, 0)` idiom — returns the raw sysctl rc, leaving `out` zero-init on
    // failure (the caller applies the C's `> 0` guards).
    fn read_u64(mib: &[c_int], out: &mut u64) -> c_int {
        let mut len = size_of::<u64>();
        unsafe {
            libc::sysctl(
                mib.as_ptr(),
                mib.len() as c_uint,
                out as *mut u64 as *mut c_void,
                &mut len,
                ptr::null(),
                0,
            )
        }
    }

    // total memory
    let mut total_mem: u64 = 0;
    if read_u64(&this.MIB_hw_physmem, &mut total_mem) == 0 && total_mem > 0 {
        this.super_.totalMem = total_mem / 1024;
    } else {
        this.super_.totalMem = 0;
    }

    // "active" pages
    let mut mem_active: u64 = 0;
    if read_u64(&this.MIB_vm_stats_vm_v_active_count, &mut mem_active) == 0 && mem_active > 0 {
        this.activeMem = mem_active * this.pageSizeKb as u64;
    } else {
        this.activeMem = 0;
    }

    // "wired" pages
    let mut mem_wire: u64 = 0;
    if read_u64(&this.MIB_vm_stats_vm_v_wire_count, &mut mem_wire) == 0 && mem_wire > 0 {
        this.wiredMem = mem_wire * this.pageSizeKb as u64;
    } else {
        this.wiredMem = 0;
    }

    // "inactive" pages
    let mut mem_inactive: u64 = 0;
    if read_u64(&this.MIB_vm_stats_vm_v_inactive_count, &mut mem_inactive) == 0 && mem_inactive > 0
    {
        this.inactiveMem = mem_inactive * this.pageSizeKb as u64;
    } else {
        this.inactiveMem = 0;
    }

    // "cache" pages
    let mut mem_cache: u64 = 0;
    if read_u64(&this.MIB_vm_stats_vm_v_cache_count, &mut mem_cache) == 0 && mem_cache > 0 {
        this.cacheMem = mem_cache * this.pageSizeKb as u64;
    } else {
        this.cacheMem = 0;
    }

    // "buffers" pages (separate read, deducted from 'wired')
    let mut buffers_mem: u64 = 0;
    if read_u64(&this.MIB_vfs_bufspace, &mut buffers_mem) == 0 && buffers_mem > 0 {
        this.buffersMem = buffers_mem / 1024;
    } else {
        this.buffersMem = 0;
    }
    this.wiredMem = this.wiredMem.saturating_sub(this.buffersMem); // "buffers" can't exceed "wired"

    // swap
    let mut swap = [kvm_swap {
        ksw_devname: [0; 32],
        ksw_used: 0,
        ksw_total: 0,
        ksw_flags: 0,
        ksw_reserved1: 0,
        ksw_reserved2: 0,
    }; 16];
    let nswap = unsafe { kvm_getswapinfo(this.kd, swap.as_mut_ptr(), swap.len() as c_int, 0) };
    let mut total_swap: u64 = 0;
    let mut used_swap: u64 = 0;
    for sw in swap.iter().take(nswap.max(0) as usize) {
        total_swap += sw.ksw_total as u64;
        used_swap += sw.ksw_used as u64;
    }
    this.super_.totalSwap = total_swap * this.pageSizeKb as u64;
    this.super_.usedSwap = used_swap * this.pageSizeKb as u64;
}

/// Port of `static void DragonFlyBSDMachine_scanJails(DragonFlyBSDMachine*
/// this)` (`DragonFlyBSDMachine.c:294`). Rebuilds the `jails` hashtable
/// (jailid → hostname) from the `jail.list` sysctlbyname, retrying on `ENOMEM`
/// (the list can grow between sizing and reading). Kvm-free.
pub fn DragonFlyBSDMachine_scanJails(this: &mut DragonFlyBSDMachine) {
    // sysctlbyname("jail.list", NULL, &len, NULL, 0) — get the buffer length.
    let name = c"jail.list";
    let mut len: usize = 0;
    if unsafe { libc::sysctlbyname(name.as_ptr(), ptr::null_mut(), &mut len, ptr::null_mut(), 0) }
        == -1
    {
        CRT_fatalError("initial sysctlbyname / jail.list failed");
    }

    // retry: on ENOMEM the list grew between the sizing and the read.
    loop {
        if len == 0 {
            return;
        }

        let mut jails = vec![0u8; len];
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                jails.as_mut_ptr() as *mut c_void,
                &mut len,
                ptr::null_mut(),
                0,
            )
        };
        if rc == -1 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOMEM) {
                continue; // goto retry
            }
            CRT_fatalError("sysctlbyname / jail.list failed");
        }

        // if (this->jails) Hashtable_delete(this->jails); this->jails = Hashtable_new(20, true);
        this.jails = Some(Hashtable_new(20, true));
        let ht = this.jails.as_mut().unwrap();

        // Walk newline-separated "jailid hostname ..." records; the first two
        // space-delimited tokens are the id and hostname (C strtok on " ").
        let text = String::from_utf8_lossy(&jails[..len.min(jails.len())]);
        for line in text.split('\n') {
            if line.is_empty() {
                continue;
            }
            let mut tok = line.split(' ').filter(|s| !s.is_empty());
            let jailid: i32 = match tok.next() {
                Some(w) => w.parse().unwrap_or(0),
                None => continue,
            };
            let hostname = tok.next().unwrap_or("");
            // if (Hashtable_get(jails, jailid) == NULL) put xStrdup(hostname).
            if Hashtable_get(ht, jailid as u32).is_none() {
                Hashtable_put(ht, jailid as u32, Box::new(JailName(hostname.to_string())));
            }
        }

        return;
    }
}

/// Port of `char* DragonFlyBSDMachine_readJailName(const DragonFlyBSDMachine*
/// host, int jailid)` (`DragonFlyBSDMachine.c:348`). Looks up `jailid` in the
/// [`DragonFlyBSDMachine_scanJails`]-populated `jails` hashtable and returns a
/// copy of the hostname ([`JailName`]), or `"-"` when absent. The C `char*`
/// return becomes an owned `String`.
pub fn DragonFlyBSDMachine_readJailName(host: &DragonFlyBSDMachine, jailid: i32) -> String {
    // if (jailid != 0 && host->jails && (hostname = Hashtable_get(jails, jailid)))
    //    jname = xStrdup(hostname); else jname = xStrdup("-");
    if jailid != 0 {
        if let Some(ht) = &host.jails {
            if let Some(obj) = Hashtable_get(ht, jailid as u32) {
                if let Some(jn) = (obj as &dyn core::any::Any).downcast_ref::<JailName>() {
                    return jn.0.clone();
                }
            }
        }
    }
    "-".to_string()
}

/// Port of `void Machine_scan(Machine* super)` (`DragonFlyBSDMachine.c:361`).
/// Orchestrates the per-tick scan: memory info, then CPU time, then the jail
/// table (matching the C call order).
pub fn Machine_scan(this: &mut DragonFlyBSDMachine) {
    DragonFlyBSDMachine_scanMemoryInfo(this);
    DragonFlyBSDMachine_scanCPUTime(this);
    DragonFlyBSDMachine_scanJails(this);
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
