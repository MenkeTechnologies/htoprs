//! Port of `SolarisProcessTable.c` — the Solaris/illumos process table.
//!
//! Ported struct model:
//! - the [`SolarisProcessTable`] struct (`SolarisProcessTable.h:27`) — a thin
//!   `ProcessTable super` wrapper — plus the [`psinfo_t`]/[`lwpsinfo_t`]
//!   `/proc` snapshot structs (`<sys/procfs.h>`), transcribed `#[repr(C)]`
//!   (LP64 layout) because `libc` does not model them; `libproc`'s `proc_walk`
//!   memcpy-fills them, so any offset error corrupts the data.
//!
//! Ported functions:
//! - [`SolarisProcessTable_readZoneName`] (`SolarisProcessTable.c:34`)
//! - [`ProcessTable_new`] (`SolarisProcessTable.c:49`)
//! - [`SolarisProcessTable_updateExe`] (`SolarisProcessTable.c:65`)
//! - [`SolarisProcessTable_updateCwd`] (`SolarisProcessTable.c:78`)
//! - [`SolarisProcessTable_getProcessState`] (`SolarisProcessTable.c:92`)
//! - [`SolarisProcessTable_walkproc`] (`SolarisProcessTable.c:110`)
//! - [`ProcessTable_goThroughEntries`] (`SolarisProcessTable.c:266`)
//!
//! Still `todo!()`:
//! - `ProcessTable_delete` is a pure teardown (`ProcessTable_done` +
//!   `free(this)`); Rust `Drop` reclaims the allocation (the darwin/linux
//!   `ProcessTable_delete` precedent).
//!
//! Deviations from the C (documented, not silent):
//! - `proc->user = UsersTable_getRef(...)` is skipped (the `UsersTable` is
//!   unported); `st_uid` is still tracked.
//! - Per [`ProcessTable_getProcess`], a newly-seen process is added inside
//!   `getProcess`, so the C's trailing `ProcessTable_add` is not repeated.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_long, c_void};

use crate::ported::machine::Machine;
use crate::ported::object::Object;
use crate::ported::process::{
    Process, ProcessState, Process_fillStarttimeBuffer, Process_setParent, Process_setThreadGroup,
    Process_updateCPUFieldWidths, Process_updateCmdline, Process_updateComm, Process_updateExe,
    PROCESS_FLAG_CWD,
};
use crate::ported::processtable::{
    ProcessTable, ProcessTable_cleanupEntries, ProcessTable_getProcess, ProcessTable_init,
    ProcessTable_prepareEntries,
};
use crate::ported::solaris::solarismachine::{kstat_ctl_t, kstat_lookup_wrapper, SolarisMachine};
use crate::ported::solaris::solarisprocess::{SolarisProcess, SolarisProcess_new};
use crate::ported::table::{Table, TableClass};

/// `#define GZONE "global    "` (`SolarisProcessTable.c:31`).
const GZONE: &str = "global    ";
/// `#define UZONE "unknown   "` (`SolarisProcessTable.c:32`).
const UZONE: &str = "unknown   ";
/// `#define PATH_MAX 1024` (`<limits.h>`).
const PATH_MAX: usize = 1024;
/// `#define NZERO 20` (`<sys/param.h>`) — the "nice" origin.
const NZERO: i32 = 20;
/// `#define PRNODEV ((dev_t)-1)` (`<sys/procfs.h>`) — no controlling tty.
const PRNODEV: libc::dev_t = -1i64 as libc::dev_t;

// ── `/proc` psinfo structs (`<sys/procfs.h>`), LP64 layout. `libc` does not
// model them; `proc_walk` memcpy-fills them per LWP.

/// Port of `typedef struct timestruc { time_t tv_sec; long tv_nsec; }`
/// (`<sys/time_impl.h>`).
#[repr(C)]
#[derive(Clone, Copy)]
struct timestruc_t {
    tv_sec: libc::time_t,
    tv_nsec: c_long,
}

/// `#define PRCLSZ 8` (`<sys/procfs.h>`).
const PRCLSZ: usize = 8;
/// `#define PRFNSZ 16` (`<sys/procfs.h>`).
const PRFNSZ: usize = 16;
/// `#define PRARGSZ 80` (`<sys/procfs.h>`).
const PRARGSZ: usize = 80;

/// Port of `typedef struct lwpsinfo { … } lwpsinfo_t` (`<sys/procfs.h>`) —
/// per-LWP `/proc` summary.
#[repr(C)]
#[derive(Clone, Copy)]
struct lwpsinfo_t {
    pr_flag: c_int,
    pr_lwpid: i32,
    pr_addr: usize,
    pr_wchan: usize,
    pr_stype: c_char,
    pr_state: c_char,
    pr_sname: c_char,
    pr_nice: c_char,
    pr_syscall: i16,
    pr_oldpri: c_char,
    pr_cpu: c_char,
    pr_pri: c_int,
    pr_pctcpu: u16,
    pr_pad: u16,
    pr_start: timestruc_t,
    pr_time: timestruc_t,
    pr_clname: [c_char; PRCLSZ],
    pr_name: [c_char; PRFNSZ],
    pr_onpro: i32,
    pr_bindpro: i32,
    pr_bindpset: i32,
    pr_lgrp: i32,
    pr_last_onproc: u64,
    pr_filler: [c_int; 4],
}

/// Port of `typedef struct psinfo { … } psinfo_t` (`<sys/procfs.h>`) —
/// per-process `/proc` summary, ending with the representative LWP's
/// [`lwpsinfo_t`].
#[repr(C)]
struct psinfo_t {
    pr_flag: c_int,
    pr_nlwp: c_int,
    pr_pid: libc::pid_t,
    pr_ppid: libc::pid_t,
    pr_pgid: libc::pid_t,
    pr_sid: libc::pid_t,
    pr_uid: libc::uid_t,
    pr_euid: libc::uid_t,
    pr_gid: libc::gid_t,
    pr_egid: libc::gid_t,
    pr_addr: usize,
    pr_size: usize,
    pr_rssize: usize,
    pr_pad1: usize,
    pr_ttydev: libc::dev_t,
    pr_pctcpu: u16,
    pr_pctmem: u16,
    pr_start: timestruc_t,
    pr_time: timestruc_t,
    pr_ctime: timestruc_t,
    pr_fname: [c_char; PRFNSZ],
    pr_psargs: [c_char; PRARGSZ],
    pr_wstat: c_int,
    pr_argc: c_int,
    pr_argv: usize,
    pr_envp: usize,
    pr_dmodel: c_char,
    pr_pad2: [c_char; 3],
    pr_taskid: i32,
    pr_projid: i32,
    pr_nzomb: c_int,
    pr_poolid: i32,
    pr_zoneid: i32,
    pr_contract: i32,
    pr_filler: [c_int; 1],
    pr_lwp: lwpsinfo_t,
}

/// `typedef int proc_walk_f(psinfo_t*, lwpsinfo_t*, void*)` (`<libproc.h>`) —
/// the `proc_walk` per-LWP callback type.
type proc_walk_f = extern "C" fn(*mut psinfo_t, *mut lwpsinfo_t, *mut c_void) -> c_int;
/// `#define PR_WALK_LWP 1` (`<libproc.h>`) — walk all LWPs.
const PR_WALK_LWP: c_int = 1;

#[link(name = "proc")]
extern "C" {
    // `int proc_walk(proc_walk_f* func, void* arg, int flags)`.
    fn proc_walk(func: proc_walk_f, arg: *mut c_void, flags: c_int) -> c_int;
}

/// Port of `typedef struct SolarisProcessTable_ { ProcessTable super; }
/// SolarisProcessTable` (`SolarisProcessTable.h:27`). `#[repr(C)]` keeps
/// `super_` at offset 0 so the `(SolarisProcessTable*)tablePtr` downcast is
/// sound.
#[repr(C)]
pub struct SolarisProcessTable {
    /// C `ProcessTable super` — the embedded base process table.
    pub super_: ProcessTable,
}

/// Port of `static char* SolarisProcessTable_readZoneName(kstat_ctl_t* kd,
/// SolarisProcess* sproc)` from `SolarisProcessTable.c:34`. `zoneid == 0` is
/// the global zone; without a kstat handle the zone is unknown; otherwise the
/// name comes from the `zones:<zoneid>` kstat's `ks_name`.
pub fn SolarisProcessTable_readZoneName(kd: *mut kstat_ctl_t, sproc: &SolarisProcess) -> String {
    if sproc.zoneid == 0 {
        GZONE.to_string()
    } else if kd.is_null() {
        UZONE.to_string()
    } else {
        let ks = unsafe { kstat_lookup_wrapper(kd, "zones", sproc.zoneid, None) };
        if ks.is_null() {
            UZONE.to_string()
        } else {
            // xStrdup(ks->ks_name) — the NUL-terminated kstat name.
            let name = unsafe { &(*ks).ks_name };
            let bytes =
                unsafe { core::slice::from_raw_parts(name.as_ptr() as *const u8, name.len()) };
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            String::from_utf8_lossy(&bytes[..end]).into_owned()
        }
    }
}

/// The `TableClass` scan-vtable slots for the Solaris process table (the
/// `prepare`/`iterate`/`cleanup` glue that C's `ProcessTable_class` stores);
/// each downcasts `Table* super` to `SolarisProcessTable` (sound: `super_:
/// ProcessTable` at offset 0, whose `super_: Table` is likewise at offset 0)
/// and delegates. Mirrors the darwin table's scan glue.
impl SolarisProcessTable {
    fn scan_prepare(super_: *mut Table) {
        let this = super_ as *mut SolarisProcessTable;
        ProcessTable_prepareEntries(unsafe { &mut (*this).super_ });
    }

    fn scan_iterate(super_: *mut Table) {
        let this = super_ as *mut SolarisProcessTable;
        ProcessTable_goThroughEntries(unsafe { &mut *this });
    }

    fn scan_cleanup(super_: *mut Table) {
        let this = super_ as *mut SolarisProcessTable;
        ProcessTable_cleanupEntries(unsafe { &mut (*this).super_ });
    }
}

/// Port of `const TableClass ProcessTable_class` (`ProcessTable.c:94`) as it
/// binds for the Solaris table (whose `iterate` link-resolves to the Solaris
/// [`ProcessTable_goThroughEntries`]).
pub static SolarisProcessTable_class: TableClass = TableClass {
    prepare: Some(SolarisProcessTable::scan_prepare),
    iterate: Some(SolarisProcessTable::scan_iterate),
    cleanup: Some(SolarisProcessTable::scan_cleanup),
};

/// Port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` from `SolarisProcessTable.c:49`. Allocates a
/// `SolarisProcessTable`, sets its scan class, and runs [`ProcessTable_init`]
/// on the embedded base. Returns the owning `Box`; the caller derives
/// `&mut box.super_` (`*mut ProcessTable`) / `&mut box.super_.super_`
/// (`*mut Table`). The `Class(SolarisProcess)` row-constructor tag is dropped
/// (the constructor is passed explicitly to [`ProcessTable_getProcess`]).
pub fn ProcessTable_new(
    host: *const Machine,
    pidMatchList: Option<usize>,
) -> Box<SolarisProcessTable> {
    let mut this = Box::new(SolarisProcessTable {
        super_: ProcessTable::empty(),
    });

    ProcessTable_init(&mut this.super_, host, pidMatchList);

    // Object_setClass(this, Class(ProcessTable)) — wire the scan vtable.
    this.super_.super_.klass = &SolarisProcessTable_class as *const TableClass;

    this
}

/// TODO: port of `void ProcessTable_delete(Object* cast)` from
/// `SolarisProcessTable.c:59`. Kept stubbed: the C body is a pure teardown —
/// `ProcessTable_done(&this->super)` + `free(this)`. Rust owns the allocation,
/// so `Drop` reclaims it (the darwin/linux `ProcessTable_delete` precedent).
pub fn ProcessTable_delete() {
    todo!("port of SolarisProcessTable.c:59 — pure free() teardown; Rust Drop handles it")
}

/// Port of `static void SolarisProcessTable_updateExe(pid_t pid, Process*
/// proc)` from `SolarisProcessTable.c:65`. Reads the `/proc/<pid>/path/a.out`
/// symlink and hands the target to [`Process_updateExe`]; a failed/empty
/// readlink leaves the process unchanged.
pub fn SolarisProcessTable_updateExe(pid: libc::pid_t, proc: &mut Process) {
    let path = CString::new(format!("/proc/{}/path/a.out", pid)).unwrap();

    let mut target = [0u8; PATH_MAX];
    let ret = unsafe {
        libc::readlink(
            path.as_ptr(),
            target.as_mut_ptr() as *mut c_char,
            target.len() - 1,
        )
    };
    if ret <= 0 {
        return;
    }

    let s = String::from_utf8_lossy(&target[..ret as usize]).into_owned();
    Process_updateExe(proc, Some(&s));
}

/// Port of `static void SolarisProcessTable_updateCwd(pid_t pid, Process*
/// proc)` from `SolarisProcessTable.c:78`. Reads the `/proc/<pid>/cwd`
/// symlink into `proc.procCwd`; a failed/empty readlink leaves it unchanged.
pub fn SolarisProcessTable_updateCwd(pid: libc::pid_t, proc: &mut Process) {
    let path = CString::new(format!("/proc/{}/cwd", pid)).unwrap();

    let mut target = [0u8; PATH_MAX];
    let ret = unsafe {
        libc::readlink(
            path.as_ptr(),
            target.as_mut_ptr() as *mut c_char,
            target.len() - 1,
        )
    };
    if ret <= 0 {
        return;
    }

    proc.procCwd = Some(String::from_utf8_lossy(&target[..ret as usize]).into_owned());
}

/// Port of `static inline ProcessState
/// SolarisProcessTable_getProcessState(char state)` from
/// `SolarisProcessTable.c:92`. Maps the Solaris scheduler-state character to a
/// [`ProcessState`].
pub fn SolarisProcessTable_getProcessState(state: c_char) -> ProcessState {
    match state as u8 as char {
        'S' => ProcessState::SLEEPING,
        'R' => ProcessState::RUNNABLE,
        'O' => ProcessState::RUNNING,
        'Z' => ProcessState::ZOMBIE,
        'T' => ProcessState::STOPPED,
        'I' => ProcessState::IDLE,
        _ => ProcessState::UNKNOWN,
    }
}

/// Port of `static int SolarisProcessTable_walkproc(psinfo_t* _psinfo,
/// lwpsinfo_t* _lwpsinfo, void* listptr)` from `SolarisProcessTable.c:110`.
/// The `proc_walk` callback: for each LWP, finds-or-creates the process
/// (master LWP keyed `pid*1024`, else the LWP-encoded id), fills the common
/// fields from `psinfo`/`lwpsinfo`, then branches on whether this is the
/// representative LWP to set the process- vs thread-level state and update the
/// table's task/thread counters.
pub extern "C" fn SolarisProcessTable_walkproc(
    _psinfo: *mut psinfo_t,
    _lwpsinfo: *mut lwpsinfo_t,
    listptr: *mut c_void,
) -> c_int {
    let ps = unsafe { &*_psinfo };
    let lwp = unsafe { &*_lwpsinfo };

    // Reads a NUL-terminated fixed `char[]` field (pr_fname/pr_psargs).
    let field_str = |buf: &[c_char]| -> String {
        let bytes = unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len()) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    };

    // Setup process list
    let pt = unsafe { &mut *(listptr as *mut ProcessTable) };
    let host = pt.super_.host; // const Machine*
    let shost = host as *const SolarisMachine;

    let lwpid_real = lwp.pr_lwpid;
    if lwpid_real > 1023 {
        return 0;
    }

    let lwpid = ps.pr_pid * 1024 + lwpid_real;
    let onMasterLWP = lwp.pr_lwpid == ps.pr_lwp.pr_lwpid;
    let getpid = if onMasterLWP { ps.pr_pid * 1024 } else { lwpid };

    // Settings snapshot (host->settings->{ss->flags,hideKernelThreads,
    // hideUserlandThreads}); absent settings request no extra scans.
    let (flags, hideKernelThreads, hideUserlandThreads) = unsafe {
        (*host).settings.as_ref().map_or((0u32, false, false), |s| {
            (
                s.screens.get(s.ssIndex as usize).map_or(0, |ss| ss.flags),
                s.hideKernelThreads,
                s.hideUserlandThreads,
            )
        })
    };

    let (preExisting, idx) =
        ProcessTable_getProcess(pt, getpid, |h| SolarisProcess_new(h) as Box<dyn Object>);

    let pt_ptr = pt as *mut ProcessTable;
    let sproc: *mut SolarisProcess = {
        let obj: &mut dyn Object = pt.super_.rows[idx].as_mut().unwrap().as_mut();
        let any: &mut dyn Any = obj;
        any.downcast_mut::<SolarisProcess>().unwrap()
    };

    // SAFETY: `sproc` points into `rows[idx]`'s heap `Box` (disjoint from the
    // table's own counter fields reached via `pt_ptr`); `rows` is not
    // reallocated between deriving `sproc` and using it. Mirrors htop's raw
    // `SolarisProcess*` / `ProcessTable*` pointer graph.
    let sp = unsafe { &mut *sproc };

    // Common code pass 1
    sp.super_.super_.show = false;
    sp.taskid = ps.pr_taskid;
    sp.projid = ps.pr_projid;
    sp.poolid = ps.pr_poolid;
    sp.contid = ps.pr_contract;
    sp.super_.priority = lwp.pr_pri as i64;
    sp.super_.nice = lwp.pr_nice as i32 - NZERO;
    sp.super_.processor = lwp.pr_onpro;
    sp.super_.state = SolarisProcessTable_getProcessState(lwp.pr_sname);
    // NOTE: this 'percentage' is a 16-bit BINARY FRACTION where 1.0 = 0x8000.
    sp.super_.percent_mem = (ps.pr_pctmem as f64 / 32768.0 * 100.0) as f32;
    sp.super_.pgrp = ps.pr_pgid;
    sp.super_.nlwp = ps.pr_nlwp as i64;
    sp.super_.session = ps.pr_sid;

    sp.super_.tty_nr = ps.pr_ttydev;
    let name = if ps.pr_ttydev != PRNODEV {
        unsafe { libc::ttyname(ps.pr_ttydev as c_int) }
    } else {
        core::ptr::null_mut()
    };
    if name.is_null() {
        sp.super_.tty_name = None;
    } else {
        sp.super_.tty_name = Some(
            unsafe { std::ffi::CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned(),
        );
    }

    sp.super_.m_resident = ps.pr_rssize as i64; // KB
    sp.super_.m_virt = ps.pr_size as i64; // KB

    if sp.super_.st_uid != ps.pr_euid {
        sp.super_.st_uid = ps.pr_euid;
        // proc->user = UsersTable_getRef(...) — UsersTable unported.
    }

    if !preExisting {
        sp.realpid = ps.pr_pid;
        sp.lwpid = lwpid_real;
        sp.zoneid = ps.pr_zoneid;
        sp.zname = Some(SolarisProcessTable_readZoneName(unsafe { (*shost).kd }, sp));
        SolarisProcessTable_updateExe(ps.pr_pid, &mut sp.super_);

        Process_updateComm(&mut sp.super_, Some(&field_str(&ps.pr_fname)));
        Process_updateCmdline(&mut sp.super_, Some(&field_str(&ps.pr_psargs)), 0, 0);

        if flags & PROCESS_FLAG_CWD != 0 {
            SolarisProcessTable_updateCwd(ps.pr_pid, &mut sp.super_);
        }
    }

    // End common code pass 1

    if onMasterLWP {
        // Are we on the representative LWP?
        Process_setParent(&mut sp.super_, ps.pr_ppid * 1024);
        Process_setThreadGroup(&mut sp.super_, ps.pr_ppid * 1024);
        sp.realppid = ps.pr_ppid;
        sp.realtgid = ps.pr_ppid;

        // See note above about this BINARY FRACTION
        sp.super_.percent_cpu = (ps.pr_pctcpu as f64 / 32768.0 * 100.0) as f32;
        Process_updateCPUFieldWidths(sp.super_.percent_cpu);

        sp.super_.time =
            (ps.pr_time.tv_sec * 100 + ps.pr_time.tv_nsec / 10_000_000) as u64;
        if !preExisting {
            // Tasks done only for NEW processes
            sp.super_.isUserlandThread = false;
            sp.super_.starttime_ctime = ps.pr_start.tv_sec;
        }

        // Update proc and thread counts based on settings
        if sp.super_.isKernelThread && !hideKernelThreads {
            unsafe {
                (*pt_ptr).kernelThreads += sp.super_.nlwp as u32;
                (*pt_ptr).totalTasks += sp.super_.nlwp as u32 + 1;
                if sp.super_.state == ProcessState::RUNNING {
                    (*pt_ptr).runningTasks += 1;
                }
            }
        } else if !sp.super_.isKernelThread {
            unsafe {
                if sp.super_.state == ProcessState::RUNNING {
                    (*pt_ptr).runningTasks += 1;
                }
                if hideUserlandThreads {
                    (*pt_ptr).totalTasks += 1;
                } else {
                    (*pt_ptr).userlandThreads += sp.super_.nlwp as u32;
                    (*pt_ptr).totalTasks += sp.super_.nlwp as u32 + 1;
                }
            }
        }
        sp.super_.super_.show = !(hideKernelThreads && sp.super_.isKernelThread);
    } else {
        // We are not in the master LWP, so jump to the LWP handling code
        sp.super_.percent_cpu = (lwp.pr_pctcpu as f64 / 32768.0 * 100.0) as f32;
        Process_updateCPUFieldWidths(sp.super_.percent_cpu);

        sp.super_.time =
            (lwp.pr_time.tv_sec * 100 + lwp.pr_time.tv_nsec / 10_000_000) as u64;
        if !preExisting {
            // Tasks done only for NEW LWPs
            sp.super_.isUserlandThread = true;
            Process_setParent(&mut sp.super_, ps.pr_pid * 1024);
            Process_setThreadGroup(&mut sp.super_, ps.pr_pid * 1024);
            sp.realppid = ps.pr_pid;
            sp.realtgid = ps.pr_pid;
            sp.super_.starttime_ctime = lwp.pr_start.tv_sec;
        }

        // Top-level process only gets this for the representative LWP
        if sp.super_.isKernelThread && !hideKernelThreads {
            sp.super_.super_.show = true;
        }
        if !sp.super_.isKernelThread && !hideUserlandThreads {
            sp.super_.super_.show = true;
        }
    } // Top-level LWP or subordinate LWP

    // Common code pass 2

    if !preExisting {
        if sp.realppid <= 0 && !(sp.realpid <= 1) {
            sp.super_.isKernelThread = true;
        } else {
            sp.super_.isKernelThread = false;
        }

        Process_fillStarttimeBuffer(&mut sp.super_);
        // ProcessTable_add(pt, proc) — already added by ProcessTable_getProcess.
    }

    sp.super_.super_.updated = true;

    // End common code pass 2

    0
}

/// Port of `void ProcessTable_goThroughEntries(ProcessTable* super)` from
/// `SolarisProcessTable.c:266`. Resets the kernel-thread counter and walks
/// every LWP via `proc_walk(&SolarisProcessTable_walkproc, …, PR_WALK_LWP)`.
pub fn ProcessTable_goThroughEntries(this: &mut SolarisProcessTable) {
    this.super_.kernelThreads = 1;
    unsafe {
        proc_walk(
            SolarisProcessTable_walkproc,
            &mut this.super_ as *mut ProcessTable as *mut c_void,
            PR_WALK_LWP,
        );
    }
}
