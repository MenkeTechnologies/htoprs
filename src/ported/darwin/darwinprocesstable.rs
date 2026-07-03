//! Partial port of `DarwinProcessTable.c` — the Darwin process table.
//!
//! Ported (self-contained: only the base [`ProcessTable`] +
//! [`crate::ported::table::Table`] plumbing + the modeled `kinfo_proc`
//! family below):
//! - [`ProcessTable_new`] (`DarwinProcessTable.c:56`) — allocate and init
//!   the `DarwinProcessTable`.
//! - [`ProcessTable_getKInfoProcs`] (`DarwinProcessTable.c:31`) — the
//!   `sysctl(KERN_PROC_ALL)` process snapshot.
//! - the [`kinfo_proc`] struct family (`sys/sysctl.h` / `sys/proc.h`),
//!   modeled `#[repr(C)]` because `libc` does not provide it.
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `ProcessTable_getProcess` is still a stub in `processtable.rs`, so the
//!   per-entry `getProcess` → `setFromKInfoProc` → `scanThreads` pipeline in
//!   `goThroughEntries` cannot be wired up.
//! - `ProcessTable_delete` needs `Object_delete` teardown (`Drop` releases
//!   the owned fields).
//!
//! `gen_port_report.py` counts remaining `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)] // faithful C global name (DarwinProcessTable_class)
#![allow(dead_code)]

use std::io;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_short, c_uchar, c_uint, c_ushort, c_void};
use std::ptr;

use crate::ported::crt::CRT_fatalError;
use crate::ported::darwin::darwinmachine::DarwinMachine;
use crate::ported::darwin::darwinprocess::{
    DarwinProcess, DarwinProcess_new, DarwinProcess_setFromKInfoProc,
    DarwinProcess_setFromLibprocPidinfo,
};
use crate::ported::darwin::platform::Platform_schedulerTicksToNanoseconds;
use crate::ported::machine::Machine;
use crate::ported::object::Object;
use crate::ported::process::ProcessState;
use crate::ported::processtable::{
    ProcessTable, ProcessTable_cleanupEntries, ProcessTable_getProcess, ProcessTable_init,
    ProcessTable_prepareEntries,
};
use crate::ported::table::{Table, TableClass};

// ── The `kinfo_proc` struct family (`sys/sysctl.h`, `sys/proc.h`, `sys/vm.h`).
//
// `libc` does not model `kinfo_proc` for macOS, so it is transcribed here
// field-for-field from the SDK headers with `#[repr(C)]` for an exact ABI
// match — `sysctl(KERN_PROC*)` fills these by memcpy, so any offset error
// corrupts the data. Opaque kernel pointers (`struct proc*`, `caddr_t`, …)
// are modeled as raw pointers for layout only and are never dereferenced.
// The layout is pinned by a runtime test that reads back our own pid/ppid.

/// `#define MAXCOMLEN 16` (`sys/param.h`).
const MAXCOMLEN: usize = 16;
/// `#define WMESGLEN 7` (`sys/sysctl.h`).
const WMESGLEN: usize = 7;
/// `#define COMAPT_MAXLOGNAME 12` (`sys/sysctl.h`).
const COMAPT_MAXLOGNAME: usize = 12;
/// `NGROUPS` == `NGROUPS_MAX` == 16 (`sys/syslimits.h`).
const NGROUPS: usize = 16;

/// Port of `struct extern_proc` (`sys/proc.h`), embedded as `kp_proc`. The
/// leading `p_un` union (two `struct proc*` **or** a `timeval`, both 16
/// bytes) is modeled as its `p_starttime` interpretation — the only member
/// htop reads.
#[repr(C)]
pub struct extern_proc {
    pub p_starttime: libc::timeval,
    pub p_vmspace: *mut c_void,
    pub p_sigacts: *mut c_void,
    pub p_flag: c_int,
    pub p_stat: c_char,
    pub p_pid: libc::pid_t,
    pub p_oppid: libc::pid_t,
    pub p_dupfd: c_int,
    pub user_stack: *mut c_char,
    pub exit_thread: *mut c_void,
    pub p_debugger: c_int,
    pub sigwait: c_int,
    pub p_estcpu: c_uint,
    pub p_cpticks: c_int,
    pub p_pctcpu: c_uint,
    pub p_wchan: *mut c_void,
    pub p_wmesg: *mut c_char,
    pub p_swtime: c_uint,
    pub p_slptime: c_uint,
    pub p_realtimer: libc::itimerval,
    pub p_rtime: libc::timeval,
    pub p_uticks: u64,
    pub p_sticks: u64,
    pub p_iticks: u64,
    pub p_traceflag: c_int,
    pub p_tracep: *mut c_void,
    pub p_siglist: c_int,
    pub p_textvp: *mut c_void,
    pub p_holdcnt: c_int,
    pub p_sigmask: libc::sigset_t,
    pub p_sigignore: libc::sigset_t,
    pub p_sigcatch: libc::sigset_t,
    pub p_priority: c_uchar,
    pub p_usrpri: c_uchar,
    pub p_nice: c_char,
    pub p_comm: [c_char; MAXCOMLEN + 1],
    pub p_pgrp: *mut c_void,
    pub p_addr: *mut c_void,
    pub p_xstat: c_ushort,
    pub p_acflag: c_ushort,
    pub p_ru: *mut c_void,
}

/// Port of `struct _pcred` (`sys/sysctl.h`) — process credentials.
#[repr(C)]
pub struct _pcred {
    pub pc_lock: [c_char; 72],
    pub pc_ucred: *mut c_void,
    pub p_ruid: libc::uid_t,
    pub p_svuid: libc::uid_t,
    pub p_rgid: libc::gid_t,
    pub p_svgid: libc::gid_t,
    pub p_refcnt: c_int,
}

/// Port of `struct _ucred` (`sys/sysctl.h`) — current credentials.
#[repr(C)]
pub struct _ucred {
    pub cr_ref: i32,
    pub cr_uid: libc::uid_t,
    pub cr_ngroups: c_short,
    pub cr_groups: [libc::gid_t; NGROUPS],
}

/// Port of `struct vmspace` (`sys/vm.h`) — a placeholder address-space
/// struct (all `dummy*`), embedded by value in [`eproc`]; only its size
/// (which shifts every field after `e_vm`) matters here.
#[repr(C)]
pub struct vmspace {
    pub dummy: i32,
    pub dummy2: *mut c_char,
    pub dummy3: [i32; 5],
    pub dummy4: [*mut c_char; 3],
}

/// Port of `struct eproc` (`sys/sysctl.h`), embedded as `kp_eproc`.
#[repr(C)]
pub struct eproc {
    pub e_paddr: *mut c_void,
    pub e_sess: *mut c_void,
    pub e_pcred: _pcred,
    pub e_ucred: _ucred,
    pub e_vm: vmspace,
    pub e_ppid: libc::pid_t,
    pub e_pgid: libc::pid_t,
    pub e_jobc: c_short,
    pub e_tdev: libc::dev_t,
    pub e_tpgid: libc::pid_t,
    pub e_tsess: *mut c_void,
    pub e_wmesg: [c_char; WMESGLEN + 1],
    pub e_xsize: i32,
    pub e_xrssize: c_short,
    pub e_xccount: c_short,
    pub e_xswrss: c_short,
    pub e_flag: i32,
    pub e_login: [c_char; COMAPT_MAXLOGNAME],
    pub e_spare: [i32; 4],
}

/// Port of `struct kinfo_proc` (`sys/sysctl.h`) — one process entry
/// returned by `sysctl(KERN_PROC*)`.
#[repr(C)]
pub struct kinfo_proc {
    pub kp_proc: extern_proc,
    pub kp_eproc: eproc,
}

/// Port of htop's `struct DarwinProcessTable_` (`DarwinProcessTable.h:16`).
/// "Extends" [`ProcessTable`] via the embedded `super_` field (htop's
/// `ProcessTable super;` first member); `global_diff` is the Darwin-only
/// per-scan accumulator.
///
/// `#[repr(C)]` guarantees `super_` at offset 0, so htop's
/// `(DarwinProcessTable*)tablePtr` downcast is sound.
#[repr(C)]
pub struct DarwinProcessTable {
    /// C `ProcessTable super` — the embedded base process table.
    pub super_: ProcessTable,
    /// C `uint64_t global_diff`.
    pub global_diff: u64,
}

/// Port of `static struct kinfo_proc* ProcessTable_getKInfoProcs(size_t*
/// count)` from `DarwinProcessTable.c:31`. Runs `sysctl(KERN_PROC_ALL)`:
/// first with a NULL buffer to size the result, then into an owned
/// `Vec<kinfo_proc>`, growing on `ENOMEM` (the same `16 * retry² *
/// sizeof` slack as the C, up to 4 tries). The returned length replaces
/// C's `*count`. A sizing failure or exhausted retries is fatal
/// ([`CRT_fatalError`], as in the C).
pub fn ProcessTable_getKInfoProcs() -> Vec<kinfo_proc> {
    let mut mib: [c_int; 4] = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_ALL, 0];

    for retry in 0..4usize {
        let mut size: usize = 0;
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                4,
                ptr::null_mut(),
                &mut size,
                ptr::null_mut(),
                0,
            )
        } < 0
            || size == 0
        {
            CRT_fatalError("Unable to get size of kproc_infos");
        }

        size += 16 * retry * retry * size_of::<kinfo_proc>();
        // xRealloc(size) → a Vec sized to hold `size / sizeof` entries.
        let cap = size / size_of::<kinfo_proc>();
        let mut procs: Vec<kinfo_proc> = Vec::with_capacity(cap);

        let mut got = size;
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                4,
                procs.as_mut_ptr() as *mut c_void,
                &mut got,
                ptr::null_mut(),
                0,
            )
        };
        if rc == 0 {
            // The kernel wrote `got` bytes; sysctl initialized every one.
            let count = got / size_of::<kinfo_proc>();
            unsafe { procs.set_len(count) };
            return procs;
        }

        if io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
            break;
        }
    }

    CRT_fatalError("Unable to get kinfo_procs");
}

/// The `TableClass` scan-vtable slots for the Darwin process table. These are
/// the `Table*`-downcasting glue the C `ProcessTable_class` (`ProcessTable.c:94`)
/// stores as `.prepare`/`.iterate`/`.cleanup`: each C slot takes `Table* super`
/// and casts it to `ProcessTable*` before delegating. They live on the `impl`
/// (not as free fns) because they are vtable glue with no standalone C symbol
/// — the same structural pattern as the `RowClass` slot dispatch — and each
/// simply re-expresses the corresponding `ProcessTable_class` slot against the
/// `#[repr(C)]` `DarwinProcessTable` layout (`super_: ProcessTable` at offset
/// 0, whose `super_: Table` is likewise at offset 0, so the `*mut Table` →
/// `*mut DarwinProcessTable` cast is sound).
impl DarwinProcessTable {
    /// C `ProcessTable_class.prepare` (`ProcessTable_prepareEntries(Table*)`):
    /// downcast then delegate to the base [`ProcessTable_prepareEntries`].
    ///
    /// # Safety precondition
    /// `super_` is the base of a live `DarwinProcessTable`.
    fn scan_prepare(super_: *mut Table) {
        let this = super_ as *mut DarwinProcessTable;
        // SAFETY: `super_` is the base of a live `DarwinProcessTable`.
        ProcessTable_prepareEntries(unsafe { &mut (*this).super_ });
    }

    /// C `ProcessTable_class.iterate` (`ProcessTable_iterateEntries(Table*)`,
    /// which calls `ProcessTable_goThroughEntries`). The common
    /// `ProcessTable_iterateEntries` port routes to the *stubbed* base
    /// `ProcessTable_goThroughEntries`, so this dispatches straight to the
    /// Darwin [`ProcessTable_goThroughEntries`] — the same platform symbol C
    /// link-resolves.
    ///
    /// # Safety precondition
    /// `super_` is the base of a live `DarwinProcessTable`.
    fn scan_iterate(super_: *mut Table) {
        let this = super_ as *mut DarwinProcessTable;
        // SAFETY: `super_` is the base of a live `DarwinProcessTable`.
        ProcessTable_goThroughEntries(unsafe { &mut *this });
    }

    /// C `ProcessTable_class.cleanup` (`ProcessTable_cleanupEntries(Table*)`):
    /// downcast then delegate to the base [`ProcessTable_cleanupEntries`].
    ///
    /// # Safety precondition
    /// `super_` is the base of a live `DarwinProcessTable`.
    fn scan_cleanup(super_: *mut Table) {
        let this = super_ as *mut DarwinProcessTable;
        // SAFETY: `super_` is the base of a live `DarwinProcessTable`.
        ProcessTable_cleanupEntries(unsafe { &mut (*this).super_ });
    }
}

/// Port of `const TableClass ProcessTable_class` (`ProcessTable.c:94`), the
/// class the Darwin `DarwinProcessTable` runs under (htop's Darwin table uses
/// the common `ProcessTable_class`, whose `iterate` link-resolves to the
/// Darwin `ProcessTable_goThroughEntries`). Only the scan-vtable half is
/// modeled (see [`TableClass`]); the `ObjectClass super` (`extends Table`,
/// `delete = ProcessTable_delete`) is class identity in Rust.
pub static DarwinProcessTable_class: TableClass = TableClass {
    prepare: Some(DarwinProcessTable::scan_prepare),
    iterate: Some(DarwinProcessTable::scan_iterate),
    cleanup: Some(DarwinProcessTable::scan_cleanup),
};

/// Port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` from `DarwinProcessTable.c:56`. C `xCalloc`s a
/// `DarwinProcessTable` (zeroing `global_diff`), sets its class, runs
/// `ProcessTable_init` on the embedded base with the `DarwinProcess`
/// constructor class, and returns `&this->super`.
///
/// The returned `Box<DarwinProcessTable>` is the owner (C's heap
/// allocation); the caller derives the graph pointers `&mut box.super_`
/// (`*mut ProcessTable`) and `&mut box.super_.super_` (`*mut Table`). The
/// `Class(DarwinProcess)` row-constructor class tag is dropped (class
/// identity is the Rust type — see [`ProcessTable_init`]), but the *table's*
/// scan class is wired here: C's `Object_setClass(this, Class(...))` sets
/// `super.klass`, which the scan macros dispatch through, so the base
/// [`Table::klass`] is pointed at [`DarwinProcessTable_class`].
pub fn ProcessTable_new(
    host: *const Machine,
    pidMatchList: Option<usize>,
) -> Box<DarwinProcessTable> {
    let mut this = Box::new(DarwinProcessTable {
        super_: ProcessTable::empty(),
        global_diff: 0,
    });

    ProcessTable_init(&mut this.super_, host, pidMatchList);

    // Object_setClass(this, Class(...)) — wire the scan vtable so
    // Machine_scanTables can dispatch prepare/iterate/cleanup through it.
    this.super_.super_.klass = &DarwinProcessTable_class as *const TableClass;

    this
}

/// TODO: port of `void ProcessTable_delete(Object* cast` from `DarwinProcessTable.c:66`.
pub fn ProcessTable_delete() {
    todo!("port of DarwinProcessTable.c:66")
}

/// Port of `void ProcessTable_goThroughEntries(ProcessTable* super)` from
/// `DarwinProcessTable.c:72`. The Darwin process scan: computes the global
/// CPU-time delta (from the `Machine_scan` load snapshots), then walks the
/// `sysctl(KERN_PROC_ALL)` snapshot, finding-or-creating each process and
/// filling it from its `kinfo_proc` and libproc task info.
///
/// Deviations (documented, not silent):
/// - `DarwinProcess_scanThreads` is not called — it needs heavy mach FFI
///   (`task_for_pid`/`task_threads`/`thread_info`) and `task_for_pid` is
///   SIP-restricted for non-self processes on modern macOS, so per-process
///   thread rows are not yet emitted.
/// - `proc->user = UsersTable_getRef(...)` is skipped (the `UsersTable` is
///   unported); `st_uid` is still tracked.
/// - Per [`ProcessTable_getProcess`], a newly-seen process is added inside
///   `getProcess`, so the C's trailing `ProcessTable_add` is not repeated.
pub fn ProcessTable_goThroughEntries(dpt: &mut DarwinProcessTable) {
    let host = dpt.super_.super_.host;
    let dhost = host as *const DarwinMachine;

    // dpt->global_diff = Σ over CPUs of (curr_load - prev_load) cpu_ticks.
    dpt.global_diff = 0;
    let (existing_cpus, active_cpus) = unsafe { ((*host).existingCPUs, (*host).activeCPUs) };
    unsafe {
        let curr = (*dhost).curr_load;
        let prev = (*dhost).prev_load;
        if !curr.is_null() && !prev.is_null() {
            for i in 0..existing_cpus as usize {
                let c = &*curr.add(i);
                let p = &*prev.add(i);
                for j in 0..libc::CPU_STATE_MAX as usize {
                    dpt.global_diff += (c.cpu_ticks[j] as u64).wrapping_sub(p.cpu_ticks[j] as u64);
                }
            }
        }
    }

    let ticks_ns = Platform_schedulerTicksToNanoseconds(dpt.global_diff as f64);
    let time_interval_ns = if active_cpus > 0 {
        ticks_ns / active_cpus as f64
    } else {
        ticks_ns
    };

    // kinfo_procs always succeed and carry the basic info; libproc fills in
    // the rest per process.
    let procs = ProcessTable_getKInfoProcs();

    for kp in &procs {
        let pid = kp.kp_proc.p_pid;

        let (pre_existing, idx) = ProcessTable_getProcess(&mut dpt.super_, pid, |h| {
            DarwinProcess_new(h) as Box<dyn Object>
        });

        // Recover a raw `*mut DarwinProcess` for this row via a normal
        // checked borrow (which ends here). `Object: Any`, so upcast to
        // `dyn Any` and downcast to the concrete type the row was built as.
        let dproc: *mut DarwinProcess = {
            let obj: &mut dyn Object = dpt.super_.super_.rows[idx].as_mut().unwrap().as_mut();
            let any: &mut dyn core::any::Any = obj;
            any.downcast_mut::<DarwinProcess>().unwrap()
        };

        // SAFETY: `dproc` aliases a field inside `*dpt_ptr`; the fill calls
        // mutate the process fields and the table's *disjoint* counter fields
        // — never the same memory — faithfully mirroring htop's raw
        // `DarwinProcess*` / `DarwinProcessTable*` pointer graph. `rows` is
        // not reallocated between deriving `dproc` and using it (no further
        // `getProcess` this iteration), so the pointer stays valid.
        let dpt_ptr = dpt as *mut DarwinProcessTable;
        unsafe {
            DarwinProcess_setFromKInfoProc(&mut (*dproc).super_, kp, pre_existing);
            DarwinProcess_setFromLibprocPidinfo(&mut *dproc, &mut *dpt_ptr, time_interval_ns);

            // Deduce further process states not covered by libproc.
            let p_stat = kp.kp_proc.p_stat as u32;
            if p_stat == libc::SZOMB {
                (*dproc).super_.state = ProcessState::ZOMBIE;
            } else if p_stat == libc::SSTOP {
                (*dproc).super_.state = ProcessState::STOPPED;
            }

            let uid = kp.kp_eproc.e_ucred.cr_uid;
            if (*dproc).super_.st_uid != uid {
                (*dproc).super_.st_uid = uid;
                // proc->user = UsersTable_getRef(...) — UsersTable unported.
            }

            // DarwinProcess_scanThreads(dproc, dpt) — deferred (see fn docs).

            (*dpt_ptr).super_.totalTasks += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_inits_base_table_with_host_and_filter() {
        // A distinct non-null `*const Machine` stand-in; ProcessTable_new
        // only stores it on the base table (never dereferences it).
        let host = 0xF00D as *const Machine;
        let filter = Some(0xBEEF_usize);

        let pt = ProcessTable_new(host, filter);

        // global_diff zeroed like the C xCalloc.
        assert_eq!(pt.global_diff, 0);
        // ProcessTable_init stored the filter list and Table_init wired the
        // host back-pointer on the embedded base table.
        assert_eq!(pt.super_.pidMatchList.map(|p| p as usize), filter);
        assert_eq!(pt.super_.super_.host, host);
        // Base table starts empty (no rows registered yet).
        assert!(pt.super_.super_.rows.is_empty());
        // Counters start at zero.
        assert_eq!(pt.super_.totalTasks, 0);
        assert_eq!(pt.super_.runningTasks, 0);
    }

    /// The load-bearing ABI test: query our own process by pid and read the
    /// kernel-filled struct back. `e_ppid`/`e_tdev` sit *after* the embedded
    /// `vmspace`, and `cr_uid` sits after `_pcred`, so a correct pid/ppid/uid
    /// read-back proves the whole layout (all nested struct sizes) is right.
    #[test]
    fn kinfo_proc_layout_matches_kernel_for_own_pid() {
        let pid = unsafe { libc::getpid() };
        let ppid = unsafe { libc::getppid() };
        let uid = unsafe { libc::getuid() };

        let mut mib: [c_int; 4] = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
        let mut kp: kinfo_proc = unsafe { core::mem::zeroed() };
        let mut size = size_of::<kinfo_proc>();
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                4,
                &mut kp as *mut kinfo_proc as *mut c_void,
                &mut size,
                ptr::null_mut(),
                0,
            )
        };

        assert_eq!(rc, 0, "sysctl(KERN_PROC_PID) failed");
        // Exactly one record came back → our struct size matches the kernel's.
        assert_eq!(size, size_of::<kinfo_proc>());
        assert_eq!(kp.kp_proc.p_pid, pid);
        assert_eq!(kp.kp_eproc.e_ppid, ppid);
        assert_eq!(kp.kp_eproc.e_ucred.cr_uid, uid);
    }

    #[test]
    fn getKInfoProcs_snapshot_contains_our_pid() {
        let procs = ProcessTable_getKInfoProcs();
        assert!(!procs.is_empty());
        let me = unsafe { libc::getpid() };
        assert!(procs.iter().any(|p| p.kp_proc.p_pid == me));
    }

    /// The end-to-end scan: build a real DarwinMachine host, scan it, then
    /// enumerate every live process into the table. This exercises the whole
    /// darwin data layer — getKInfoProcs → getProcess → setFromKInfoProc →
    /// setFromLibprocPidinfo — against the running system.
    #[test]
    fn goThroughEntries_enumerates_live_processes_including_self() {
        use crate::ported::darwin::darwinmachine::{
            host_basic_info_data_t, DarwinMachine, DarwinMachine_freeCPULoadInfo,
            DarwinMachine_getHostInfo, Machine_scan,
        };
        use crate::ported::darwin::platform::Platform_init;
        use crate::ported::linux::linuxmachine::ZfsArcStats;
        use crate::ported::machine::{ScreenSettings, Settings};
        use crate::ported::process::Process_getPid;

        Platform_init();

        let mut dm = Box::new(DarwinMachine {
            super_: Machine::default(),
            host_info: host_basic_info_data_t::default(),
            vm_stats: unsafe { core::mem::zeroed() },
            prev_load: ptr::null_mut(),
            curr_load: ptr::null_mut(),
            GPUService: 0,
            zfs: ZfsArcStats::default(),
        });
        dm.super_.activeCPUs = 1;
        dm.super_.existingCPUs = 1;
        dm.super_.settings = Some(Settings {
            screens: vec![ScreenSettings::default()],
            ssIndex: 0,
            ..Default::default()
        });
        DarwinMachine_getHostInfo(&mut dm.host_info);
        // Two scans a moment apart so prev_load/curr_load both exist and the
        // CPU-tick delta (hence the time interval) is > 0 — as it always is
        // in real use, where Machine_new pre-populates prev_load. A single
        // scan would leave prev_load null and yield a zero interval.
        Machine_scan(&mut dm);
        std::thread::sleep(std::time::Duration::from_millis(50));
        Machine_scan(&mut dm);

        let mut dpt = ProcessTable_new(&dm.super_ as *const Machine, None);
        ProcessTable_goThroughEntries(&mut dpt);

        // Every process on the host is enumerated, including this test.
        let me = unsafe { libc::getpid() };
        let found = dpt
            .super_
            .super_
            .rows
            .iter()
            .flatten()
            .any(|o| Process_getPid(o.as_process().unwrap()) == me);
        assert!(found, "own pid not found in the scan");
        assert!(dpt.super_.super_.rows.len() > 1, "expected many processes");
        assert!(dpt.super_.totalTasks > 0);

        DarwinMachine_freeCPULoadInfo(&mut dm.curr_load);
    }
}
