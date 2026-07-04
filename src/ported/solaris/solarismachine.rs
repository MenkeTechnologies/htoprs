//! Port of `SolarisMachine.c` — the Solaris/illumos per-host `Machine`.
//!
//! Ported struct model:
//! - [`CPUData`] (`SolarisMachine.h:32`) and the [`SolarisMachine`] struct
//!   (`SolarisMachine.h:47`), modeled `#[repr(C)]` with `super_` at offset 0
//!   so htop's `(SolarisMachine*)host` downcast — a `*const Machine` obtained
//!   from a `SolarisMachine`, cast back — is sound (used by the meter setters
//!   in `platform.rs` and by `SolarisProcessTable_walkproc`). `ZfsArcStats`
//!   is reused from the (platform-independent) zfs model in `linux/`.
//! - the `libkstat` FFI ([`kstat_ctl_t`]/[`kstat_t`]/[`kstat_named_t`] plus
//!   `kstat_open`/`kstat_close`/`kstat_lookup`/`kstat_data_lookup`/
//!   `kstat_read`/`kstat_chain_update`), transcribed from `<kstat.h>` since
//!   `libc` does not model it, and the two `kstat_*_wrapper` helpers
//!   (`Platform.h:130`/`:136`).
//!
//! Ported functions:
//! - [`SolarisMachine_updateCPUcount`] (`SolarisMachine.c:29`)
//! - [`SolarisMachine_scanCPUTime`] (`SolarisMachine.c:71`)
//! - [`SolarisMachine_scanMemoryInfo`] (`SolarisMachine.c:165`)
//! - [`SolarisMachine_scanZfsArcstats`] (`SolarisMachine.c:234`)
//! - [`Machine_scan`] (`SolarisMachine.c:283`)
//! - [`Machine_new`] (`SolarisMachine.c:292`)
//! - [`Machine_delete`] (`SolarisMachine.c:313`)
//! - [`Machine_isCPUonline`] (`SolarisMachine.c:325`)
//! - [`Machine_getCPUPhysicalCoreID`] (`SolarisMachine.c:333`)
//! - [`Machine_getCPUThreadIndex`] (`SolarisMachine.c:339`)
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::ffi::CString;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_long, c_uchar, c_uint, c_void};
use std::ptr;

use crate::ported::crt::CRT_fatalError;
use crate::ported::linux::linuxmachine::ZfsArcStats;
use crate::ported::machine::{Machine, Machine_done, Machine_init};

/// `#define ONE_K 1024L` (`Macros.h`).
const ONE_K: usize = 1024;

// ── `libkstat` FFI (`<kstat.h>`). `libc` does not model kstat, so the
// handle/entry/named-value types are transcribed field-for-field with
// `#[repr(C)]` for an exact ABI match — `kstat_read` fills these by the
// library, and `ks_data`/`ks_name` are read directly.

/// `typedef int kid_t` (`<sys/types.h>`).
pub type kid_t = c_int;
/// `typedef longlong_t hrtime_t` (`<sys/time.h>`).
pub type hrtime_t = i64;

/// `#define KSTAT_STRLEN 31` (`<sys/kstat.h>`).
const KSTAT_STRLEN: usize = 31;

/// Port of `typedef struct kstat_ctl { … } kstat_ctl_t` (`<kstat.h>`).
#[repr(C)]
pub struct kstat_ctl_t {
    pub kc_chain_id: kid_t,
    pub kc_chain: *mut kstat_t,
    pub kc_kd: c_int,
}

/// Port of `typedef struct kstat { … } kstat_t` (`<sys/kstat.h>`). Only the
/// user-visible prefix is read (`ks_data`/`ks_name`); the kernel-only tail
/// (`ks_update`/`ks_private`/`ks_snapshot`/`ks_lock`) is transcribed for an
/// exact size, its function-pointer slots modeled as opaque optionals.
#[repr(C)]
pub struct kstat_t {
    pub ks_crtime: hrtime_t,
    pub ks_next: *mut kstat_t,
    pub ks_kid: kid_t,
    pub ks_module: [c_char; KSTAT_STRLEN],
    pub ks_resv: c_uchar,
    pub ks_instance: c_int,
    pub ks_name: [c_char; KSTAT_STRLEN],
    pub ks_type: c_uchar,
    pub ks_class: [c_char; KSTAT_STRLEN],
    pub ks_flags: c_uchar,
    pub ks_data: *mut c_void,
    pub ks_ndata: c_uint,
    pub ks_data_size: usize,
    pub ks_snaptime: hrtime_t,
    pub ks_update: Option<unsafe extern "C" fn(*mut kstat_t, c_int) -> c_int>,
    pub ks_private: *mut c_void,
    pub ks_snapshot: Option<unsafe extern "C" fn(*mut kstat_t, *mut c_void, c_int) -> c_int>,
    pub ks_lock: *mut c_void,
}

/// Port of the `value` union in `kstat_named_t` (`<sys/kstat.h>`). htop reads
/// only `value.ui64`; the leading `char c[16]` fixes the union's 16-byte size.
#[repr(C)]
pub union kstat_named_value {
    pub c: [c_char; 16],
    pub ui64: u64,
}

/// Port of `typedef struct kstat_named { … } kstat_named_t` (`<sys/kstat.h>`)
/// — one named counter in a `KSTAT_TYPE_NAMED` data section.
#[repr(C)]
pub struct kstat_named_t {
    pub name: [c_char; KSTAT_STRLEN],
    pub data_type: c_uchar,
    pub value: kstat_named_value,
}

#[link(name = "kstat")]
extern "C" {
    pub fn kstat_open() -> *mut kstat_ctl_t;
    pub fn kstat_close(kc: *mut kstat_ctl_t) -> c_int;
    pub fn kstat_lookup(
        kc: *mut kstat_ctl_t,
        ks_module: *mut c_char,
        ks_instance: c_int,
        ks_name: *mut c_char,
    ) -> *mut kstat_t;
    pub fn kstat_data_lookup(ksp: *mut kstat_t, name: *mut c_char) -> *mut c_void;
    pub fn kstat_read(kc: *mut kstat_ctl_t, ksp: *mut kstat_t, buf: *mut c_void) -> kid_t;
    pub fn kstat_chain_update(kc: *mut kstat_ctl_t) -> kid_t;
}

/// Port of `static inline kstat_t* kstat_lookup_wrapper(kstat_ctl_t* kc, const
/// char* ks_module, int ks_instance, const char* ks_name)` (`Platform.h:136`)
/// — the const-stripping shim around `kstat_lookup`. `ks_name == None` is the
/// C `NULL` (match any).
///
/// # Safety
/// `kc` must be a live `kstat_ctl_t` handle from [`kstat_open`].
pub unsafe fn kstat_lookup_wrapper(
    kc: *mut kstat_ctl_t,
    ks_module: &str,
    ks_instance: c_int,
    ks_name: Option<&str>,
) -> *mut kstat_t {
    let module = CString::new(ks_module).unwrap();
    let name = ks_name.map(|s| CString::new(s).unwrap());
    kstat_lookup(
        kc,
        module.as_ptr() as *mut c_char,
        ks_instance,
        name.as_ref()
            .map_or(ptr::null_mut(), |c| c.as_ptr() as *mut c_char),
    )
}

/// Port of `static inline void* kstat_data_lookup_wrapper(kstat_t* ksp, const
/// char* name)` (`Platform.h:130`) — the const-stripping shim around
/// `kstat_data_lookup`, typed to the `kstat_named_t` htop always reads back.
///
/// # Safety
/// `ksp` must be a live `kstat_t` that has been `kstat_read`.
pub unsafe fn kstat_data_lookup_wrapper(ksp: *mut kstat_t, name: &str) -> *mut kstat_named_t {
    let n = CString::new(name).unwrap();
    kstat_data_lookup(ksp, n.as_ptr() as *mut c_char) as *mut kstat_named_t
}

/// Port of `typedef struct CPUData_ { … } CPUData` (`SolarisMachine.h:32`) —
/// per-CPU tick accumulators and derived percentages.
#[repr(C)]
#[derive(Clone, Default)]
pub struct CPUData {
    pub userPercent: f64,
    pub nicePercent: f64,
    pub systemPercent: f64,
    pub irqPercent: f64,
    pub idlePercent: f64,
    pub systemAllPercent: f64,
    pub frequency: f64,
    pub luser: u64,
    pub lkrnl: u64,
    pub lintr: u64,
    pub lidle: u64,
    pub online: bool,
}

/// Port of `typedef struct SolarisMachine_ { … } SolarisMachine`
/// (`SolarisMachine.h:47`). "Extends" the base [`Machine`] via `super_`
/// (first member); `#[repr(C)]` keeps `super_` at offset 0 so the
/// `(SolarisMachine*)host` downcast is sound. The C `CPUData* cpus`
/// heap array is modeled as an owned `Vec<CPUData>` (index `0` is the
/// average, `1..=existingCPUs` the per-CPU rows when there is >1 CPU).
#[repr(C)]
pub struct SolarisMachine {
    /// C `Machine super` — the embedded base machine.
    pub super_: Machine,
    /// C `kstat_ctl_t* kd`.
    pub kd: *mut kstat_ctl_t,
    /// C `CPUData* cpus`.
    pub cpus: Vec<CPUData>,
    /// C `size_t pageSize`.
    pub pageSize: usize,
    /// C `size_t pageSizeKB`.
    pub pageSizeKB: usize,
    /// C `memory_t usedMem`.
    pub usedMem: u64,
    /// C `memory_t lockedMem`.
    pub lockedMem: u64,
    /// C `ZfsArcStats zfs`.
    pub zfs: ZfsArcStats,
}

/// Port of `static void SolarisMachine_updateCPUcount(SolarisMachine* this)`
/// from `SolarisMachine.c:29`. Re-sizes the `cpus` array when the configured
/// CPU count changes (index `0` reserved for the average), refreshes the
/// active count, and re-syncs the kstat chain on any change.
pub fn SolarisMachine_updateCPUcount(this: &mut SolarisMachine) {
    let mut change = false;

    let mut s = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_CONF) };
    if s < 1 {
        CRT_fatalError("Cannot get existing CPU count by sysconf(_SC_NPROCESSORS_CONF)");
    }

    if s as u32 != this.super_.existingCPUs {
        if s == 1 {
            this.cpus.resize(1, CPUData::default());
            this.cpus[0].online = true;
        } else {
            this.cpus.resize((s + 1) as usize, CPUData::default());
            this.cpus[0].online = true; /* average is always "online" */
            for i in 1..(s + 1) as usize {
                this.cpus[i].online = false;
            }
        }

        change = true;
        this.super_.existingCPUs = s as u32;
    }

    s = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if s < 1 {
        CRT_fatalError("Cannot get active CPU count by sysconf(_SC_NPROCESSORS_ONLN)");
    }

    if s as u32 != this.super_.activeCPUs {
        change = true;
        this.super_.activeCPUs = s as u32;
    }

    if change {
        let update_kid = unsafe { kstat_chain_update(this.kd) };
        if update_kid < 0 {
            CRT_fatalError("Cannot update kstat chain");
        }
    }
}

/// Port of `static void SolarisMachine_scanCPUTime(SolarisMachine* this)` from
/// `SolarisMachine.c:71`. Reads each CPU's `cpu:N:sys` nanosecond counters via
/// kstat, computes the idle/intr/kernel/user percentages from the deltas since
/// the last reading, optionally the `cpu_info:N:current_clock_Hz` frequency,
/// and (for >1 CPU) accumulates the per-CPU average into index `0`.
pub fn SolarisMachine_scanCPUTime(this: &mut SolarisMachine) {
    let activeCPUs = this.super_.activeCPUs;
    let existingCPUs = this.super_.existingCPUs;
    let mut idlebuf = 0.0;
    let mut intrbuf = 0.0;
    let mut krnlbuf = 0.0;
    let mut userbuf = 0.0;
    let mut arrskip = 0usize;

    debug_assert!(existingCPUs > 0);
    debug_assert!(!this.kd.is_null());

    if existingCPUs > 1 {
        // Store values for the stats loop one extra element up in the array
        // to leave room for the average to be calculated afterwards
        arrskip += 1;
    }

    let showCPUFrequency = this
        .super_
        .settings
        .as_ref()
        .is_some_and(|s| s.showCPUFrequency);

    // Calculate per-CPU statistics first
    for i in 0..existingCPUs {
        let mut idletime: *mut kstat_named_t = ptr::null_mut();
        let mut intrtime: *mut kstat_named_t = ptr::null_mut();
        let mut krnltime: *mut kstat_named_t = ptr::null_mut();
        let mut usertime: *mut kstat_named_t = ptr::null_mut();
        let mut cpu_freq: *mut kstat_named_t = ptr::null_mut();

        let cpuinfo = unsafe { kstat_lookup_wrapper(this.kd, "cpu", i as c_int, Some("sys")) };
        if !cpuinfo.is_null() {
            this.cpus[i as usize + arrskip].online = true;
            if unsafe { kstat_read(this.kd, cpuinfo, ptr::null_mut()) } != -1 {
                idletime = unsafe { kstat_data_lookup_wrapper(cpuinfo, "cpu_nsec_idle") };
                intrtime = unsafe { kstat_data_lookup_wrapper(cpuinfo, "cpu_nsec_intr") };
                krnltime = unsafe { kstat_data_lookup_wrapper(cpuinfo, "cpu_nsec_kernel") };
                usertime = unsafe { kstat_data_lookup_wrapper(cpuinfo, "cpu_nsec_user") };
            }
        } else {
            this.cpus[i as usize + arrskip].online = false;
            continue;
        }

        debug_assert!(
            !idletime.is_null()
                && !intrtime.is_null()
                && !krnltime.is_null()
                && !usertime.is_null()
        );

        if showCPUFrequency {
            let ci = unsafe { kstat_lookup_wrapper(this.kd, "cpu_info", i as c_int, None) };
            if !ci.is_null() && unsafe { kstat_read(this.kd, ci, ptr::null_mut()) } != -1 {
                cpu_freq = unsafe { kstat_data_lookup_wrapper(ci, "current_clock_Hz") };
            }
            debug_assert!(!cpu_freq.is_null());
        }

        let (idle_v, intr_v, krnl_v, user_v) = unsafe {
            (
                (*idletime).value.ui64,
                (*intrtime).value.ui64,
                (*krnltime).value.ui64,
                (*usertime).value.ui64,
            )
        };

        let cpuData = &mut this.cpus[i as usize + arrskip];
        let totaltime = (idle_v - cpuData.lidle)
            + (intr_v - cpuData.lintr)
            + (krnl_v - cpuData.lkrnl)
            + (user_v - cpuData.luser);

        // Calculate percentages of deltas since last reading
        cpuData.userPercent = ((user_v - cpuData.luser) as f64 / totaltime as f64) * 100.0;
        cpuData.nicePercent = 0.0; // Not implemented on Solaris
        cpuData.systemPercent = ((krnl_v - cpuData.lkrnl) as f64 / totaltime as f64) * 100.0;
        cpuData.irqPercent = ((intr_v - cpuData.lintr) as f64 / totaltime as f64) * 100.0;
        cpuData.systemAllPercent = cpuData.systemPercent + cpuData.irqPercent;
        cpuData.idlePercent = ((idle_v - cpuData.lidle) as f64 / totaltime as f64) * 100.0;
        // Store current values to use for the next round of deltas
        cpuData.luser = user_v;
        cpuData.lkrnl = krnl_v;
        cpuData.lintr = intr_v;
        cpuData.lidle = idle_v;
        // Add frequency in MHz
        cpuData.frequency = if showCPUFrequency {
            unsafe { (*cpu_freq).value.ui64 as f64 / 1e6 }
        } else {
            f64::NAN
        };
        // Accumulate the current percentages into buffers for later average
        if existingCPUs > 1 {
            userbuf += cpuData.userPercent;
            krnlbuf += cpuData.systemPercent;
            intrbuf += cpuData.irqPercent;
            idlebuf += cpuData.idlePercent;
        }
    }

    if existingCPUs > 1 {
        let cpuData = &mut this.cpus[0];
        cpuData.userPercent = userbuf / activeCPUs as f64;
        cpuData.nicePercent = 0.0; // Not implemented on Solaris
        cpuData.systemPercent = krnlbuf / activeCPUs as f64;
        cpuData.irqPercent = intrbuf / activeCPUs as f64;
        cpuData.systemAllPercent = cpuData.systemPercent + cpuData.irqPercent;
        cpuData.idlePercent = idlebuf / activeCPUs as f64;
    }
}

// ── swap FFI (`<sys/swap.h>`), absent from `libc`.

/// `#define SC_LIST 2` (`<sys/swap.h>`).
const SC_LIST: c_int = 2;
/// `#define SC_GETNSWP 4` (`<sys/swap.h>`).
const SC_GETNSWP: c_int = 4;
/// `#define MAXPATHLEN 1024` (`<sys/param.h>`).
const MAXPATHLEN: usize = 1024;

/// Port of `typedef struct swapent { … } swapent_t` (`<sys/swap.h>`).
#[repr(C)]
struct swapent_t {
    ste_path: *mut c_char,
    ste_start: libc::off_t,
    ste_length: libc::off_t,
    ste_pages: c_long,
    ste_free: c_long,
    ste_flags: c_int,
}

/// Port of `struct swaptable { int swt_n; struct swapent swt_ent[]; }`
/// (`<sys/swap.h>`) — the variable-length swap listing header.
#[repr(C)]
struct swaptable_t {
    swt_n: c_int,
    swt_ent: [swapent_t; 1],
}

extern "C" {
    fn swapctl(cmd: c_int, arg: *mut c_void) -> c_int;
}

/// Port of `static void SolarisMachine_scanMemoryInfo(SolarisMachine* this)`
/// from `SolarisMachine.c:165`. Reads physical-memory pages from the
/// `unix:0:system_pages` kstat (falling back to `sysconf` if kstat is
/// unavailable) and totals swap usage via `swapctl(SC_GETNSWP/SC_LIST)`.
pub fn SolarisMachine_scanMemoryInfo(this: &mut SolarisMachine) {
    let mut totalswap: u64 = 0;
    let mut totalfree: u64 = 0;

    // Part 1 - physical memory
    let mut meminfo: *mut kstat_t = ptr::null_mut();
    if !this.kd.is_null() {
        // The ptr `meminfo` is invalidated when the kstat chain is updated by
        // `kstat_chain_update` (in `SolarisMachine_updateCPUcount`). So it
        // needs to be re-read on every memory update.
        meminfo = unsafe { kstat_lookup_wrapper(this.kd, "unix", 0, Some("system_pages")) };
    }
    let mut ksrphyserr: kid_t = -1;
    if !meminfo.is_null() {
        ksrphyserr = unsafe { kstat_read(this.kd, meminfo, ptr::null_mut()) };
    }
    if ksrphyserr != -1 {
        let physmem = unsafe { kstat_data_lookup_wrapper(meminfo, "physmem") };
        let pagesfree = unsafe { kstat_data_lookup_wrapper(meminfo, "pagesfree") };
        let pagestotal = unsafe { kstat_data_lookup_wrapper(meminfo, "pagestotal") };
        let pageslocked = unsafe { kstat_data_lookup_wrapper(meminfo, "pageslocked") };

        let (physmem, pagesfree, pagestotal, pageslocked) = unsafe {
            (
                (*physmem).value.ui64,
                (*pagesfree).value.ui64,
                (*pagestotal).value.ui64,
                (*pageslocked).value.ui64,
            )
        };

        this.super_.totalMem = physmem * this.pageSizeKB as u64;
        this.usedMem = (pagestotal - pageslocked - pagesfree) * this.pageSizeKB as u64;
        this.lockedMem = pageslocked * this.pageSizeKB as u64;
    } else {
        // Fall back to basic sysconf if kstat isn't working
        this.super_.totalMem =
            unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) } as u64 * this.pageSize as u64;
        this.usedMem = this.super_.totalMem
            - (unsafe { libc::sysconf(libc::_SC_AVPHYS_PAGES) } as u64 * this.pageSize as u64);
        this.lockedMem = 0;
    }

    // Part 2 - swap
    let mut nswap = unsafe { swapctl(SC_GETNSWP, ptr::null_mut()) };
    let mut sl: *mut swaptable_t = ptr::null_mut();
    let mut spathbase: *mut c_char = ptr::null_mut();
    if nswap > 0 {
        let sz = nswap as usize * size_of::<swapent_t>() + size_of::<c_int>();
        sl = unsafe { libc::malloc(sz) } as *mut swaptable_t;
    }
    if !sl.is_null() {
        spathbase = unsafe { libc::malloc(nswap as usize * MAXPATHLEN) } as *mut c_char;
    }
    if !spathbase.is_null() {
        let mut spath = spathbase;
        let ent0 = unsafe { (*sl).swt_ent.as_mut_ptr() };
        for i in 0..nswap as isize {
            unsafe {
                (*ent0.offset(i)).ste_path = spath;
                spath = spath.add(MAXPATHLEN);
            }
        }
        unsafe {
            (*sl).swt_n = nswap;
        }
    }
    nswap = unsafe { swapctl(SC_LIST, sl as *mut c_void) };
    if nswap > 0 {
        let ent0 = unsafe { (*sl).swt_ent.as_mut_ptr() };
        for i in 0..nswap as isize {
            let e = unsafe { &*ent0.offset(i) };
            totalswap += e.ste_pages as u64;
            totalfree += e.ste_free as u64;
        }
    }
    unsafe {
        libc::free(spathbase as *mut c_void);
        libc::free(sl as *mut c_void);
    }
    this.super_.totalSwap = totalswap * this.pageSizeKB as u64;
    this.super_.usedSwap = this.super_.totalSwap - (totalfree * this.pageSizeKB as u64);
}

/// Port of `static void SolarisMachine_scanZfsArcstats(SolarisMachine* this)`
/// from `SolarisMachine.c:234`. Reads the ZFS ARC sizes from the
/// `zfs:0:arcstats` kstat into `this.zfs` (in kB).
pub fn SolarisMachine_scanZfsArcstats(this: &mut SolarisMachine) {
    if this.kd.is_null() {
        return;
    }

    let arcstats = unsafe { kstat_lookup_wrapper(this.kd, "zfs", 0, Some("arcstats")) };
    if arcstats.is_null() {
        return;
    }

    let ksrphyserr = unsafe { kstat_read(this.kd, arcstats, ptr::null_mut()) };
    if ksrphyserr == -1 {
        return;
    }

    // Reads a named ui64 counter (kB), returning `None` when the counter is
    // absent (the C `cur_kstat != NULL ? … : 0` guard).
    let read_kb = |name: &str| -> Option<u64> {
        let k = unsafe { kstat_data_lookup_wrapper(arcstats, name) };
        if k.is_null() {
            None
        } else {
            Some(unsafe { (*k).value.ui64 } / 1024)
        }
    };

    this.zfs.size = read_kb("size").unwrap_or(0);
    this.zfs.enabled = if this.zfs.size > 0 { 1 } else { 0 };

    this.zfs.max = read_kb("c_max").unwrap_or(0);
    this.zfs.MFU = read_kb("mfu_size").unwrap_or(0);
    this.zfs.MRU = read_kb("mru_size").unwrap_or(0);
    this.zfs.anon = read_kb("anon_size").unwrap_or(0);
    this.zfs.header = read_kb("hdr_size").unwrap_or(0);
    this.zfs.other = read_kb("other_size").unwrap_or(0);

    if let Some(compressed) = read_kb("compressed_size") {
        this.zfs.compressed = compressed;
        this.zfs.isCompressed = 1;
        this.zfs.uncompressed = read_kb("uncompressed_size").unwrap_or(0);
    } else {
        this.zfs.isCompressed = 0;
    }
}

/// Port of `void Machine_scan(Machine* super)` from `SolarisMachine.c:283`.
pub fn Machine_scan(this: &mut SolarisMachine) {
    SolarisMachine_updateCPUcount(this);
    SolarisMachine_scanCPUTime(this);
    SolarisMachine_scanMemoryInfo(this);
    SolarisMachine_scanZfsArcstats(this);
}

/// Port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// (`SolarisMachine.c:292`). Allocates the host, runs the base [`Machine_init`],
/// resolves the page size via `sysconf`, opens the kstat handle, and counts the
/// CPUs. Returns `Box<SolarisMachine>`; the caller derives `*mut Machine` from
/// `&mut box.super_` (the C returns `&this->super`).
pub fn Machine_new(usersTable: Option<usize>, userId: u32) -> Box<SolarisMachine> {
    let mut this = Box::new(SolarisMachine {
        super_: Machine::default(),
        kd: std::ptr::null_mut(),
        cpus: Vec::new(),
        pageSize: 0,
        pageSizeKB: 0,
        usedMem: 0,
        lockedMem: 0,
        zfs: ZfsArcStats::default(),
    });

    Machine_init(&mut this.super_, usersTable, userId);

    let pageSize = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if pageSize <= 0 {
        CRT_fatalError("Cannot get pagesize by sysconf(_SC_PAGESIZE)");
    }
    this.pageSize = pageSize as usize;
    this.pageSizeKB = this.pageSize / ONE_K;

    this.kd = unsafe { kstat_open() };
    if this.kd.is_null() {
        CRT_fatalError("Cannot open kstat handle");
    }

    SolarisMachine_updateCPUcount(&mut this);

    this
}

/// Port of `void Machine_delete(Machine* super)` from `SolarisMachine.c:313`.
/// Runs the base [`Machine_done`] teardown and closes the kstat handle; the
/// `cpus` `Vec` and the `Box` allocation are reclaimed by `Drop` (the C
/// `free(this->cpus)`/`free(this)`).
pub fn Machine_delete(mut this: Box<SolarisMachine>) {
    Machine_done(&mut this.super_);

    if !this.kd.is_null() {
        unsafe { kstat_close(this.kd) };
    }
    // free(this->cpus); free(this) — Drop reclaims the Vec and the Box.
}

/// Port of `bool Machine_isCPUonline(const Machine* super, unsigned int id)`
/// (`SolarisMachine.c:325`). A single-CPU host is always online; otherwise
/// reads `cpus[id + 1].online` (index `0` is the average).
pub fn Machine_isCPUonline(super_: &Machine, id: u32) -> bool {
    debug_assert!(id < super_.existingCPUs);

    // SAFETY: the base `Machine` is embedded at offset 0 of a live
    // `SolarisMachine` (guaranteed by `#[repr(C)]`), so the C downcast holds.
    let this = unsafe { &*(super_ as *const Machine as *const SolarisMachine) };

    if super_.existingCPUs == 1 {
        true
    } else {
        this.cpus[(id + 1) as usize].online
    }
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
