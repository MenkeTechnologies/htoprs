//! Port of `OpenBSDMachine.c` — the OpenBSD per-host `Machine`.
//!
//! Ported struct model:
//! - [`CPUData`] (`OpenBSDMachine.h:18`) and the [`OpenBSDMachine`] struct
//!   (`OpenBSDMachine.h:40`), modeled `#[repr(C)]` with `super_: Machine` at
//!   offset 0 so htop's `(OpenBSDMachine*)host` downcast is sound.
//! - the kernel `sysctl`/`swapctl` structs [`uvmexp`] (`uvm/uvmexp.h`),
//!   [`bcachestats`] (`sys/mount.h`), [`swapent`] (`sys/swap.h`) and
//!   [`cpustats`] (`sys/sched.h`), transcribed `#[repr(C)]` from the OpenBSD
//!   headers because `libc` does not model them.
//!
//! Ported functions:
//! - `OpenBSDMachine_updateCPUcount` (`OpenBSDMachine.c:34`)
//! - `Machine_new` (`OpenBSDMachine.c:91`)
//! - `Machine_delete` (`OpenBSDMachine.c:124`)
//! - `OpenBSDMachine_scanMemoryInfo` (`OpenBSDMachine.c:135`)
//! - `getKernelCPUTimes` (`OpenBSDMachine.c:193`)
//! - `kernelCPUTimesToHtop` (`OpenBSDMachine.c:201`)
//! - `OpenBSDMachine_scanCPUTime` (`OpenBSDMachine.c:238`)
//! - `Machine_scan` (`OpenBSDMachine.c:281`)
//! - `Machine_isCPUonline` (`OpenBSDMachine.c:289`)
//! - `Machine_getCPUPhysicalCoreID` (`OpenBSDMachine.c:296`)
//! - `Machine_getCPUThreadIndex` (`OpenBSDMachine.c:302`)
//!
//! # Verification note
//!
//! OpenBSD is a tier-3 Rust target with no prebuilt `std`, so this module
//! cannot be cross-compiled on the darwin dev host. The `libc` symbols used
//! (`CTL_KERN`/`CTL_HW`/`CTL_VM`/`CTL_VFS`, `HW_NCPU`, `HW_NCPUONLINE`,
//! `KERN_FSCALE`, `KERN_CPUSTATS`, `KERN_CPTIME2`, `sysctl`, `sysconf`,
//! `struct kinfo_proc`) were verified against `libc`'s
//! `unix/bsd/netbsdlike/openbsd` module; every non-`libc` constant / struct is
//! transcribed from the OpenBSD kernel headers cited inline. The scan model
//! mirrors the compiled darwin `Machine_scan`/`Machine_new` port. It is
//! source-reviewed, not compile-verified.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::ptr;

use crate::ported::crt::CRT_fatalError;
use crate::ported::machine::{Machine, Machine_done, Machine_init};
use crate::ported::xutils::saturatingSub;

// ── Constants absent from `libc`, from the OpenBSD kernel headers. ──────────

/// `#define VM_UVMEXP 4` (`uvm/uvmexp.h`) — the `CTL_VM` uvm-summary node.
const VM_UVMEXP: c_int = 4;
/// `#define HW_CPUSPEED 12` (`sys/sysctl.h`).
const HW_CPUSPEED: c_int = 12;
/// `#define VFS_GENERIC 0` (`sys/mount.h`).
const VFS_GENERIC: c_int = 0;
/// `#define VFS_BCACHESTAT 3` (`sys/mount.h`).
const VFS_BCACHESTAT: c_int = 3;
/// `#define CPUSTATES 6` (`sys/sched.h`).
const CPUSTATES: usize = 6;
/// `sys/sched.h` CPU-time bucket indices.
const CP_USER: usize = 0;
const CP_NICE: usize = 1;
const CP_SYS: usize = 2;
const CP_SPIN: usize = 3;
const CP_INTR: usize = 4;
const CP_IDLE: usize = 5;
/// `#define CPUSTATS_ONLINE 0x0001` (`sys/sched.h`).
const CPUSTATS_ONLINE: u64 = 0x0001;
/// `#define SWAP_NSWAP 3` (`sys/swap.h`).
const SWAP_NSWAP: c_int = 3;
/// `#define SWAP_STATS 4` (`sys/swap.h`).
const SWAP_STATS: c_int = 4;
/// `#define SWF_ENABLE 0x00000002` (`sys/swap.h`).
const SWF_ENABLE: c_int = 0x0000_0002;
/// `#define DEV_BSIZE 512` (`sys/param.h`).
const DEV_BSIZE: i64 = 512;
/// `#define PATH_MAX 1024` (`sys/syslimits.h`).
const PATH_MAX: usize = 1024;
/// `#define ONE_K 1024` (`Macros.h`).
const ONE_K: usize = 1024;
/// `#define KVM_NO_FILES 0x80000000` (`kvm.h`).
pub const KVM_NO_FILES: c_int = 0x8000_0000u32 as c_int;
/// `#define _POSIX2_LINE_MAX 2048` (`limits.h`) — the `kvm_openfiles` errbuf.
pub const _POSIX2_LINE_MAX: usize = 2048;

// ── libkvm / swapctl FFI (absent from `libc`). ──────────────────────────────

/// Opaque `kvm_t` handle (`kvm.h`); only ever held/forwarded, never
/// dereferenced from Rust.
#[repr(C)]
pub struct kvm_t {
    _private: [u8; 0],
}

extern "C" {
    /// `kvm_t* kvm_openfiles(const char*, const char*, const char*, int, char*)`.
    pub fn kvm_openfiles(
        execfile: *const c_char,
        corefile: *const c_char,
        swapfile: *const c_char,
        flags: c_int,
        errbuf: *mut c_char,
    ) -> *mut kvm_t;

    /// `int kvm_close(kvm_t*)`.
    pub fn kvm_close(kd: *mut kvm_t) -> c_int;

    /// `struct kinfo_proc* kvm_getprocs(kvm_t*, int, int, size_t, int*)`.
    pub fn kvm_getprocs(
        kd: *mut kvm_t,
        op: c_int,
        arg: c_int,
        elemsize: usize,
        cnt: *mut c_int,
    ) -> *mut libc::kinfo_proc;

    /// `char** kvm_getargv(kvm_t*, const struct kinfo_proc*, int)`.
    pub fn kvm_getargv(
        kd: *mut kvm_t,
        kp: *const libc::kinfo_proc,
        nchr: c_int,
    ) -> *mut *mut c_char;

    /// `char** kvm_getenvv(kvm_t*, const struct kinfo_proc*, int)`.
    pub fn kvm_getenvv(
        kd: *mut kvm_t,
        kp: *const libc::kinfo_proc,
        nchr: c_int,
    ) -> *mut *mut c_char;

    /// `int swapctl(int cmd, void* arg, int misc)` (`sys/swap.h`).
    fn swapctl(cmd: c_int, arg: *mut c_void, misc: c_int) -> c_int;
}

// ── Kernel sysctl / swapctl structs, transcribed `#[repr(C)]`. ───────────────

/// Port of `struct uvmexp` (`uvm/uvmexp.h`) — the uvm summary filled by
/// `sysctl(CTL_VM, VM_UVMEXP)`. Every field is a C `int`; only the page
/// counters near the front are read, but the whole layout is transcribed so
/// the sysctl length (`sizeof`) matches the kernel.
#[repr(C)]
#[derive(Default)]
pub struct uvmexp {
    pub pagesize: c_int,
    pub pagemask: c_int,
    pub pageshift: c_int,
    pub npages: c_int,
    pub free: c_int,
    pub active: c_int,
    pub inactive: c_int,
    pub paging: c_int,
    pub wired: c_int,
    pub zeropages: c_int,
    pub reserve_pagedaemon: c_int,
    pub reserve_kernel: c_int,
    pub percpucaches: c_int,
    pub vnodepages: c_int,
    pub vtextpages: c_int,
    pub freemin: c_int,
    pub freetarg: c_int,
    pub inactarg: c_int,
    pub wiredmax: c_int,
    pub anonmin: c_int,
    pub vtextmin: c_int,
    pub vnodemin: c_int,
    pub anonminpct: c_int,
    pub vtextminpct: c_int,
    pub vnodeminpct: c_int,
    pub nswapdev: c_int,
    pub swpages: c_int,
    pub swpginuse: c_int,
    pub swpgonly: c_int,
    pub nswget: c_int,
    pub nanon: c_int,
    pub unused05: c_int,
    pub unused06: c_int,
    pub faults: c_int,
    pub traps: c_int,
    pub intrs: c_int,
    pub swtch: c_int,
    pub softs: c_int,
    pub syscalls: c_int,
    pub pageins: c_int,
    pub pcphit: c_int,
    pub pcpmiss: c_int,
    pub pgswapin: c_int,
    pub pgswapout: c_int,
    pub forks: c_int,
    pub forks_ppwait: c_int,
    pub forks_sharevm: c_int,
    pub pga_zerohit: c_int,
    pub pga_zeromiss: c_int,
    pub unused09: c_int,
    pub fltnoram: c_int,
    pub fltnoanon: c_int,
    pub fltnoamap: c_int,
    pub fltpgwait: c_int,
    pub fltpgrele: c_int,
    pub fltrelck: c_int,
    pub fltnorelck: c_int,
    pub fltanget: c_int,
    pub fltanretry: c_int,
    pub fltamcopy: c_int,
    pub fltnamap: c_int,
    pub fltnomap: c_int,
    pub fltlget: c_int,
    pub fltget: c_int,
    pub flt_anon: c_int,
    pub flt_acow: c_int,
    pub flt_obj: c_int,
    pub flt_prcopy: c_int,
    pub flt_przero: c_int,
    pub fltup: c_int,
    pub fltnoup: c_int,
    pub pdwoke: c_int,
    pub pdrevs: c_int,
    pub pdswout: c_int,
    pub pdfreed: c_int,
    pub pdscans: c_int,
    pub pdanscan: c_int,
    pub pdobscan: c_int,
    pub pdreact: c_int,
    pub pdbusy: c_int,
    pub pdpageouts: c_int,
    pub pdpending: c_int,
    pub pddeact: c_int,
    pub swpskip: c_int,
    pub fpswtch: c_int,
    pub kmapent: c_int,
}

/// Port of `struct bcachestats` (`sys/mount.h`) — buffer-cache stats filled by
/// `sysctl(CTL_VFS, VFS_GENERIC, VFS_BCACHESTAT)`. All fields `int64_t`; only
/// `numbufpages` is read.
#[repr(C)]
#[derive(Default)]
pub struct bcachestats {
    pub numbufs: i64,
    pub numbufpages: i64,
    pub numdirtypages: i64,
    pub numcleanpages: i64,
    pub pendingwrites: i64,
    pub pendingreads: i64,
    pub numwrites: i64,
    pub numreads: i64,
    pub cachehits: i64,
    pub busymapped: i64,
    pub delwribufs: i64,
    pub kvaslots: i64,
    pub kvaslots_avail: i64,
}

/// Port of `struct swapent` (`sys/swap.h`) — one entry from
/// `swapctl(SWAP_STATS)`. `dev_t` is `int32_t` on OpenBSD.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct swapent {
    pub se_dev: libc::dev_t,
    pub se_flags: c_int,
    pub se_nblks: c_int,
    pub se_inuse: c_int,
    pub se_priority: c_int,
    pub se_path: [c_char; PATH_MAX],
}

/// Port of `struct cpustats` (`sys/sched.h`) — per-CPU stats from
/// `sysctl(CTL_KERN, KERN_CPUSTATS, cpu)`.
#[repr(C)]
#[derive(Default)]
pub struct cpustats {
    pub cs_time: [u64; CPUSTATES],
    pub cs_flags: u64,
}

// ── The OpenBSD `Machine` object. ────────────────────────────────────────────

/// Port of `struct CPUData_` (`OpenBSDMachine.h:18`) — per-CPU accumulated
/// tick totals and deltas.
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

    pub online: bool,
}

/// Port of htop's `struct OpenBSDMachine_` (`OpenBSDMachine.h:40`). "Extends"
/// the base [`Machine`] via `super_` (first member); `#[repr(C)]` keeps
/// `super_` at offset 0 so htop's `(OpenBSDMachine*)host` downcast — a
/// `*const Machine` obtained from an `OpenBSDMachine`, cast back — is sound
/// (used by the platform meter setters and the process-table scan).
///
/// Deviation: the C `CPUData* cpuData` heap array (managed via
/// `xReallocArray`/`free`) is modeled as an owned [`Vec<CPUData>`], sized
/// `existingCPUs + 1` (index 0 is the average, `1..=existingCPUs` the
/// per-CPU rows). `Vec` sits after `super_`, so the base-pointer cast is
/// unaffected.
#[repr(C)]
pub struct OpenBSDMachine {
    /// C `Machine super`.
    pub super_: Machine,
    /// C `kvm_t* kd`.
    pub kd: *mut kvm_t,

    /// C `memory_t wiredMem` (kB).
    pub wiredMem: u64,
    pub cacheMem: u64,
    pub activeMem: u64,
    pub pagingMem: u64,
    pub inactiveMem: u64,

    /// C `CPUData* cpuData` — index 0 = average, `1..=existingCPUs` per-CPU.
    pub cpuData: Vec<CPUData>,

    /// C `long fscale`.
    pub fscale: c_long,
    /// C `int cpuSpeed` (MHz), `-1` when unknown.
    pub cpuSpeed: c_int,
    /// C `size_t pageSize` (bytes).
    pub pageSize: usize,
    /// C `size_t pageSizeKB`.
    pub pageSizeKB: usize,
}

/// Port of `static void OpenBSDMachine_updateCPUcount(OpenBSDMachine* this)`
/// from `OpenBSDMachine.c:34`. Reads `hw.ncpuonline` (active) and `hw.ncpu`
/// (existing); on any change, resizes [`cpuData`](OpenBSDMachine::cpuData) and
/// re-primes each row, marking per-CPU online state from
/// `sysctl(KERN_CPUSTATS)`.
pub fn OpenBSDMachine_updateCPUcount(this: &mut OpenBSDMachine) {
    let nmib: [c_int; 2] = [libc::CTL_HW, libc::HW_NCPU];
    let mib: [c_int; 2] = [libc::CTL_HW, libc::HW_NCPUONLINE];
    let mut value: libc::c_uint = 0;
    let mut change = false;

    let mut size = size_of::<libc::c_uint>();
    let r = unsafe {
        libc::sysctl(
            mib.as_ptr(),
            2,
            &mut value as *mut libc::c_uint as *mut c_void,
            &mut size,
            ptr::null_mut(),
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

    let mut size = size_of::<libc::c_uint>();
    let r = unsafe {
        libc::sysctl(
            nmib.as_ptr(),
            2,
            &mut value as *mut libc::c_uint as *mut c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if r < 0 || value < 1 {
        value = this.super_.activeCPUs;
    }

    if value != this.super_.existingCPUs {
        // xReallocArray(cpuData, value + 1, sizeof(CPUData)).
        this.cpuData.resize((value as usize) + 1, CPUData::default());
        this.super_.existingCPUs = value;
        change = true;
    }

    if change {
        let dAvg = &mut this.cpuData[0];
        *dAvg = CPUData::default();
        dAvg.totalTime = 1;
        dAvg.totalPeriod = 1;
        dAvg.online = true;

        for i in 0..this.super_.existingCPUs {
            let d = &mut this.cpuData[(i as usize) + 1];
            *d = CPUData::default();
            d.totalTime = 1;
            d.totalPeriod = 1;

            let ncmib: [c_int; 3] = [libc::CTL_KERN, libc::KERN_CPUSTATS, i as c_int];
            let mut cpu_stats = cpustats::default();
            let mut size = size_of::<cpustats>();
            if unsafe {
                libc::sysctl(
                    ncmib.as_ptr(),
                    3,
                    &mut cpu_stats as *mut cpustats as *mut c_void,
                    &mut size,
                    ptr::null_mut(),
                    0,
                )
            } < 0
            {
                CRT_fatalError("ncmib sysctl call failed");
            }
            d.online = (cpu_stats.cs_flags & CPUSTATS_ONLINE) != 0;
        }
    }
}

/// Port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)` from
/// `OpenBSDMachine.c:91`. Allocates an `OpenBSDMachine`, runs the base
/// [`Machine_init`], samples the CPU count, the kernel `fscale`, the page
/// size, and opens the `kvm` handle. Returns the owning `Box<OpenBSDMachine>`
/// (C returns `&this->super`); the caller derives `*mut Machine` from
/// `&mut box.super_`.
pub fn Machine_new(usersTable: Option<usize>, userId: u32) -> Box<OpenBSDMachine> {
    let fmib: [c_int; 2] = [libc::CTL_KERN, libc::KERN_FSCALE];

    let mut this = Box::new(OpenBSDMachine {
        super_: Machine::default(),
        kd: ptr::null_mut(),
        wiredMem: 0,
        cacheMem: 0,
        activeMem: 0,
        pagingMem: 0,
        inactiveMem: 0,
        cpuData: Vec::new(),
        fscale: 0,
        cpuSpeed: 0,
        pageSize: 0,
        pageSizeKB: 0,
    });

    Machine_init(&mut this.super_, usersTable, userId);

    OpenBSDMachine_updateCPUcount(&mut this);

    let mut size = size_of::<c_long>();
    if unsafe {
        libc::sysctl(
            fmib.as_ptr(),
            2,
            &mut this.fscale as *mut c_long as *mut c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    } < 0
        || this.fscale <= 0
    {
        CRT_fatalError("fscale sysctl call failed");
    }

    let pageSize = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if pageSize <= 0 {
        CRT_fatalError("pagesize sysconf call failed");
    }
    this.pageSize = pageSize as usize;
    this.pageSizeKB = this.pageSize / ONE_K;

    let mut errbuf = [0 as c_char; _POSIX2_LINE_MAX];
    this.kd = unsafe {
        kvm_openfiles(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            KVM_NO_FILES,
            errbuf.as_mut_ptr(),
        )
    };
    if this.kd.is_null() {
        CRT_fatalError("kvm_openfiles() failed");
    }

    this.cpuSpeed = -1;

    this
}

/// Port of `void Machine_delete(Machine* super)` from `OpenBSDMachine.c:124`.
/// Closes the `kvm` handle, runs the base [`Machine_done`], and drops the
/// allocation (C's `free(this->cpuData)` / `free(this)` are the `Vec`/`Box`
/// `Drop`). Takes ownership of the `Box` (C's `Machine* super` is the base of
/// the same heap object).
pub fn Machine_delete(mut this: Box<OpenBSDMachine>) {
    if !this.kd.is_null() {
        unsafe { kvm_close(this.kd) };
    }
    // free(this->cpuData) — the Vec is dropped with the Box.
    Machine_done(&mut this.super_);
    // free(this) — the Box is dropped here.
}

/// Port of `static void OpenBSDMachine_scanMemoryInfo(OpenBSDMachine* this)`
/// from `OpenBSDMachine.c:135`. Reads `vm.uvmexp` and `vfs.bcachestat` to
/// derive the wired/cache/active/paging/inactive breakdown (in kB), then
/// totals swap via `swapctl`.
pub fn OpenBSDMachine_scanMemoryInfo(this: &mut OpenBSDMachine) {
    let uvmexp_mib: [c_int; 2] = [libc::CTL_VM, VM_UVMEXP];
    let mut uvmexp = uvmexp::default();
    let mut size_uvmexp = size_of::<uvmexp>();
    if unsafe {
        libc::sysctl(
            uvmexp_mib.as_ptr(),
            2,
            &mut uvmexp as *mut uvmexp as *mut c_void,
            &mut size_uvmexp,
            ptr::null_mut(),
            0,
        )
    } < 0
    {
        CRT_fatalError("uvmexp sysctl call failed");
    }

    let bcache_mib: [c_int; 3] = [libc::CTL_VFS, VFS_GENERIC, VFS_BCACHESTAT];
    let mut bcstats = bcachestats::default();
    let mut size_bcstats = size_of::<bcachestats>();
    if unsafe {
        libc::sysctl(
            bcache_mib.as_ptr(),
            3,
            &mut bcstats as *mut bcachestats as *mut c_void,
            &mut size_bcstats,
            ptr::null_mut(),
            0,
        )
    } < 0
    {
        CRT_fatalError("cannot get vfs.bcachestat");
    }

    // NOTE: in OpenBSD the "cached" memory is a subset of the "wired" memory.
    let page_kb = this.pageSizeKB as u64;
    this.super_.totalMem = page_kb.wrapping_mul(uvmexp.npages as i64 as u64);
    // NB: uvmexp.wired == 0!? deduct: npages - free - active - paging - numbufpages.
    let wired = uvmexp.npages as i64
        - uvmexp.free as i64
        - uvmexp.active as i64
        - uvmexp.paging as i64
        - bcstats.numbufpages;
    this.wiredMem = page_kb.wrapping_mul(wired as u64);
    this.cacheMem = page_kb.wrapping_mul(bcstats.numbufpages as u64);
    this.activeMem = page_kb.wrapping_mul(uvmexp.active as i64 as u64);
    this.pagingMem = page_kb.wrapping_mul(uvmexp.paging as i64 as u64);
    this.inactiveMem = page_kb.wrapping_mul(uvmexp.inactive as i64 as u64);

    // Taken almost directly from OpenBSD's top(1).
    let nswap = unsafe { swapctl(SWAP_NSWAP, ptr::null_mut(), 0) };
    if nswap > 0 {
        let mut swdev: Vec<swapent> = vec![
            swapent {
                se_dev: 0,
                se_flags: 0,
                se_nblks: 0,
                se_inuse: 0,
                se_priority: 0,
                se_path: [0; PATH_MAX],
            };
            nswap as usize
        ];
        let rnswap =
            unsafe { swapctl(SWAP_STATS, swdev.as_mut_ptr() as *mut c_void, nswap) };

        // Total things up (blocks are 1024/DEV_BSIZE per kB).
        let per_k = (1024 / DEV_BSIZE) as u64;
        let mut total: u64 = 0;
        let mut used: u64 = 0;
        for se in swdev.iter().take(rnswap.max(0) as usize) {
            if se.se_flags & SWF_ENABLE != 0 {
                used += (se.se_inuse as u64) / per_k;
                total += (se.se_nblks as u64) / per_k;
            }
        }

        this.super_.totalSwap = total;
        this.super_.usedSwap = used;
        // free(swdev) — the Vec is dropped here.
    } else {
        this.super_.totalSwap = 0;
        this.super_.usedSwap = 0;
    }
}

/// Port of `static void getKernelCPUTimes(unsigned int cpuId, u_int64_t*
/// times)` from `OpenBSDMachine.c:193`. Fills `times[CPUSTATES]` from
/// `sysctl(KERN_CPTIME2, cpuId)`; a failure or short read is fatal.
pub fn getKernelCPUTimes(cpuId: u32, times: &mut [u64; CPUSTATES]) {
    let mib: [c_int; 3] = [libc::CTL_KERN, libc::KERN_CPTIME2, cpuId as c_int];
    let mut length = size_of::<u64>() * CPUSTATES;
    if unsafe {
        libc::sysctl(
            mib.as_ptr(),
            3,
            times.as_mut_ptr() as *mut c_void,
            &mut length,
            ptr::null_mut(),
            0,
        )
    } == -1
        || length != size_of::<u64>() * CPUSTATES
    {
        CRT_fatalError("sysctl kern.cp_time2 failed");
    }
}

/// Port of `static void kernelCPUTimesToHtop(const u_int64_t* times, CPUData*
/// cpu)` from `OpenBSDMachine.c:201`. Diffs the raw kernel tick buckets
/// against the previous snapshot (`saturatingSub`) into the CPU-row periods,
/// then stores the new totals. OpenBSD defines `CP_SPIN`, so spin ticks are
/// folded into `sysAllTime`.
pub fn kernelCPUTimesToHtop(times: &[u64; CPUSTATES], cpu: &mut CPUData) {
    let mut totalTime: u64 = 0;
    for &t in times.iter() {
        totalTime += t;
    }

    // #ifdef CP_SPIN — OpenBSD has CP_SPIN.
    let sysAllTime = times[CP_INTR] + times[CP_SYS] + times[CP_SPIN];

    cpu.totalPeriod = saturatingSub(totalTime, cpu.totalTime);
    cpu.userPeriod = saturatingSub(times[CP_USER], cpu.userTime);
    cpu.nicePeriod = saturatingSub(times[CP_NICE], cpu.niceTime);
    cpu.sysPeriod = saturatingSub(times[CP_SYS], cpu.sysTime);
    cpu.sysAllPeriod = saturatingSub(sysAllTime, cpu.sysAllTime);
    cpu.spinPeriod = saturatingSub(times[CP_SPIN], cpu.spinTime);
    cpu.intrPeriod = saturatingSub(times[CP_INTR], cpu.intrTime);
    cpu.idlePeriod = saturatingSub(times[CP_IDLE], cpu.idleTime);

    cpu.totalTime = totalTime;
    cpu.userTime = times[CP_USER];
    cpu.niceTime = times[CP_NICE];
    cpu.sysTime = times[CP_SYS];
    cpu.sysAllTime = sysAllTime;
    cpu.spinTime = times[CP_SPIN];
    cpu.intrTime = times[CP_INTR];
    cpu.idleTime = times[CP_IDLE];
}

/// Port of `static void OpenBSDMachine_scanCPUTime(OpenBSDMachine* this)` from
/// `OpenBSDMachine.c:238`. Refreshes every online per-CPU row from
/// `KERN_CPTIME2`, accumulates the per-state average across active CPUs into
/// `cpuData[0]`, and reads `hw.cpuspeed`.
pub fn OpenBSDMachine_scanCPUTime(this: &mut OpenBSDMachine) {
    let mut kernelTimes: [u64; CPUSTATES] = [0; CPUSTATES];
    let mut avg: [u64; CPUSTATES] = [0; CPUSTATES];

    for i in 0..this.super_.existingCPUs {
        if !this.cpuData[(i as usize) + 1].online {
            continue;
        }

        getKernelCPUTimes(i, &mut kernelTimes);
        {
            let cpu = &mut this.cpuData[(i as usize) + 1];
            kernelCPUTimesToHtop(&kernelTimes, cpu);

            avg[CP_USER] += cpu.userTime;
            avg[CP_NICE] += cpu.niceTime;
            avg[CP_SYS] += cpu.sysTime;
            avg[CP_SPIN] += cpu.spinTime;
            avg[CP_INTR] += cpu.intrTime;
            avg[CP_IDLE] += cpu.idleTime;
        }
    }

    let active = this.super_.activeCPUs as u64;
    if active > 0 {
        for a in avg.iter_mut() {
            *a /= active;
        }
    }

    kernelCPUTimesToHtop(&avg, &mut this.cpuData[0]);

    let mib: [c_int; 2] = [libc::CTL_HW, HW_CPUSPEED];
    let mut cpuSpeed: c_int = 0;
    let mut size = size_of::<c_int>();
    if unsafe {
        libc::sysctl(
            mib.as_ptr(),
            2,
            &mut cpuSpeed as *mut c_int as *mut c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    } == -1
    {
        this.cpuSpeed = -1;
    } else {
        this.cpuSpeed = cpuSpeed;
    }
}

/// Port of `void Machine_scan(Machine* super)` from `OpenBSDMachine.c:281`.
pub fn Machine_scan(this: &mut OpenBSDMachine) {
    OpenBSDMachine_updateCPUcount(this);
    OpenBSDMachine_scanMemoryInfo(this);
    OpenBSDMachine_scanCPUTime(this);
}

/// Port of `bool Machine_isCPUonline(const Machine* super, unsigned int id)`
/// (`OpenBSDMachine.c:289`). Reads `cpuData[id + 1].online`; the `&Machine` is
/// the base of an `OpenBSDMachine` (C's `(const OpenBSDMachine*)super`).
///
/// # Safety
/// `host` must be the `super_` base of a live [`OpenBSDMachine`].
pub fn Machine_isCPUonline(host: &Machine, id: u32) -> bool {
    debug_assert!(id < host.existingCPUs);
    let this = host as *const Machine as *const OpenBSDMachine;
    unsafe { (*this).cpuData[(id as usize) + 1].online }
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
