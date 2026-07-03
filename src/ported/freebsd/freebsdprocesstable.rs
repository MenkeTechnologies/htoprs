//! Port of `FreeBSDProcessTable.c` — the FreeBSD process table.
//!
//! Ported (on the base [`ProcessTable`] + [`Table`] plumbing, the modeled
//! [`FreeBSDProcessTable`] struct, and libc's `kinfo_proc` / `kvm_*` FFI):
//! - the [`FreeBSDProcessTable`] struct (`FreeBSDProcessTable.h`) + its scan
//!   vtable ([`FreeBSDProcessTable_class`]).
//! - [`ProcessTable_new`] (`FreeBSDProcessTable.c:45`), [`ProcessTable_delete`]
//!   (`FreeBSDProcessTable.c:56` — kept `todo!()`, pure `free()` teardown →
//!   `Drop`).
//! - [`FreeBSDProcessTable_updateExe`] (`:62`),
//!   [`FreeBSDProcessTable_updateCwd`] (`:79`),
//!   [`FreeBSDProcessTable_updateProcessName`] (`:103`),
//!   [`FreeBSDProcessTable_readJailName`] (`:135`),
//!   [`ProcessTable_goThroughEntries`] (`:160`).
//!
//! Deviation (documented, as the darwin port): `proc->user =
//! UsersTable_getRef(host->usersTable, st_uid)` is skipped — `Machine::usersTable`
//! is the pre-existing opaque `Option<usize>` handle, not a live `UsersTable`,
//! so the username lookup cannot be wired faithfully yet; `st_uid` is still
//! tracked.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;
use std::ffi::CStr;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::ptr;

use crate::ported::freebsd::freebsdmachine::FreeBSDMachine;
use crate::ported::freebsd::freebsdprocess::{FreeBSDProcess, FreeBSDProcess_new, FreeBSDSchedClass};
use crate::ported::machine::Machine;
use crate::ported::object::Object;
use crate::ported::process::{
    Process, ProcessState, Process_fillStarttimeBuffer, Process_isKernelThread,
    Process_isUserlandThread, Process_setParent, Process_setPid, Process_setThreadGroup,
    Process_updateCPUFieldWidths, Process_updateCmdline, Process_updateComm, Process_updateExe,
    PROCESS_FLAG_CWD, PROCESS_NICE_UNKNOWN,
};
use crate::ported::processtable::{
    ProcessTable, ProcessTable_cleanupEntries, ProcessTable_getProcess, ProcessTable_init,
    ProcessTable_prepareEntries,
};
use crate::ported::table::{Table, TableClass};

/// `#define ONE_K 1024` (`Macros.h`).
const ONE_K: i64 = 1024;

// ── `sys/priority.h` scheduling-class macros (absent from `libc`).
/// `#define PRI_ITHD 1` — interrupt thread.
const PRI_ITHD: c_int = 1;
/// `#define PRI_REALTIME 2` — real-time process.
const PRI_REALTIME: c_int = 2;
/// `#define PRI_TIMESHARE 3` — timesharing process.
const PRI_TIMESHARE: c_int = 3;
/// `#define PRI_IDLE 4` — idle process.
const PRI_IDLE: c_int = 4;
/// `#define PRI_FIFO_BIT 8`.
const PRI_FIFO_BIT: c_int = 8;

/// `#define PRI_BASE(P) ((P) & ~PRI_FIFO_BIT)` (`sys/priority.h`).
#[inline]
fn PRI_BASE(p: c_int) -> c_int {
    p & !PRI_FIFO_BIT
}

/// `#define NOCPU (-1)` (`sys/proc.h:789`) — "for when we aren't on a CPU".
const NOCPU: c_int = -1;

/// The FreeBSD 15 `getosreldate()` reference-point bump (`FreeBSDProcessTable.c:254`).
const OSRELDATE_PUSER_THRESHOLD: c_int = 1500048;

extern "C" {
    /// `int getosreldate(void)` (`<osreldate.h>` / libc) — the running
    /// kernel's `__FreeBSD_version`. Not exposed by the `libc` crate.
    fn getosreldate() -> c_int;
}

/// Port of `typedef struct FreeBSDProcessTable_` (`FreeBSDProcessTable.h`).
/// "Extends" the base [`ProcessTable`] via `super_` (first member);
/// `#[repr(C)]` keeps it at offset 0 (the `*mut Table` → `*mut
/// FreeBSDProcessTable` scan-vtable downcast is sound).
#[repr(C)]
pub struct FreeBSDProcessTable {
    /// C `ProcessTable super` — the embedded base process table.
    pub super_: ProcessTable,
    /// C `int osreldate` — cached `getosreldate()`.
    pub osreldate: c_int,
}

/// Scan-vtable glue (the `TableClass` slots) for `FreeBSDProcessTable`, each
/// downcasting the base `*mut Table` and delegating — the darwin precedent.
impl FreeBSDProcessTable {
    fn scan_prepare(super_: *mut Table) {
        let this = super_ as *mut FreeBSDProcessTable;
        // SAFETY: `super_` is the base of a live `FreeBSDProcessTable`.
        ProcessTable_prepareEntries(unsafe { &mut (*this).super_ });
    }

    fn scan_iterate(super_: *mut Table) {
        let this = super_ as *mut FreeBSDProcessTable;
        // SAFETY: `super_` is the base of a live `FreeBSDProcessTable`.
        ProcessTable_goThroughEntries(unsafe { &mut *this });
    }

    fn scan_cleanup(super_: *mut Table) {
        let this = super_ as *mut FreeBSDProcessTable;
        // SAFETY: `super_` is the base of a live `FreeBSDProcessTable`.
        ProcessTable_cleanupEntries(unsafe { &mut (*this).super_ });
    }
}

/// Port of `const TableClass ProcessTable_class` as it applies to the FreeBSD
/// table: htop's `FreeBSDProcessTable` runs under the common
/// `ProcessTable_class`, whose `iterate` link-resolves to the FreeBSD
/// `ProcessTable_goThroughEntries`. Only the scan-vtable half is modeled.
pub static FreeBSDProcessTable_class: TableClass = TableClass {
    prepare: Some(FreeBSDProcessTable::scan_prepare),
    iterate: Some(FreeBSDProcessTable::scan_iterate),
    cleanup: Some(FreeBSDProcessTable::scan_cleanup),
};

/// Port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` from `FreeBSDProcessTable.c:45`. Allocates a
/// `FreeBSDProcessTable`, caches `getosreldate()`, and runs `ProcessTable_init`
/// on the embedded base with the `FreeBSDProcess` constructor class.
///
/// The returned `Box<FreeBSDProcessTable>` owns the allocation (C's `xCalloc`);
/// the caller derives `&mut box.super_` / `&mut box.super_.super_`. The row
/// constructor class tag is dropped (class identity is the Rust type), but the
/// table's scan class is wired to [`FreeBSDProcessTable_class`] so
/// `Machine_scanTables` dispatches prepare/iterate/cleanup through it.
pub fn ProcessTable_new(
    host: *const Machine,
    pidMatchList: Option<usize>,
) -> Box<FreeBSDProcessTable> {
    let mut this = Box::new(FreeBSDProcessTable {
        super_: ProcessTable::empty(),
        osreldate: unsafe { getosreldate() },
    });

    ProcessTable_init(&mut this.super_, host, pidMatchList);

    this.super_.super_.klass = &FreeBSDProcessTable_class as *const TableClass;

    this
}

/// TODO: port of `void ProcessTable_delete(Object* cast)` from
/// `FreeBSDProcessTable.c:56`. Kept stubbed: the C body is `ProcessTable_done`
/// + `free(this)` — Rust `Drop` reclaims the [`FreeBSDProcessTable`]
/// allocation and the base table's owned rows, so there is no faithful
/// safe-Rust analog (the darwin `ProcessTable_delete` precedent).
pub fn ProcessTable_delete() {
    todo!("port of FreeBSDProcessTable.c:56 — pure free() teardown; Rust Drop handles it")
}

/// Decodes a fixed-size, NUL-terminated `[c_char; N]` kernel string field
/// (`ki_comm`, `ki_emul`, `jnamebuf`) to an owned lossy `String`. A macro
/// rather than a free `fn` because the C inlines this NUL-scan at each use
/// site (it is not a distinct htop C function), so it expands inline exactly
/// like the C — keeping the port a faithful translation with no module-level
/// non-C function.
macro_rules! fixed_cstr {
    ($buf:expr) => {{
        let buf: &[c_char] = $buf;
        // SAFETY: `buf` is a valid `[c_char]`; reinterpreting as bytes for the
        // NUL scan does not read past its length.
        let bytes: &[u8] =
            unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len()) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }};
}

/// Port of `static void FreeBSDProcessTable_updateExe(const struct kinfo_proc*
/// kproc, Process* proc)` from `FreeBSDProcessTable.c:62`. Kernel threads
/// clear the exe; otherwise the executable path comes from
/// `sysctl(KERN_PROC_PATHNAME)`, cleared on failure.
pub fn FreeBSDProcessTable_updateExe(kproc: &libc::kinfo_proc, proc: &mut Process) {
    if Process_isKernelThread(proc) {
        Process_updateExe(proc, None);
        return;
    }

    let mut mib: [c_int; 4] = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PATHNAME,
        kproc.ki_pid,
    ];
    let mut buffer = [0u8; 2048];
    let mut size = buffer.len();
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            4,
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        Process_updateExe(proc, None);
        return;
    }

    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    let exe = String::from_utf8_lossy(&buffer[..end]);
    Process_updateExe(proc, Some(&exe));
}

/// Port of `static void FreeBSDProcessTable_updateCwd(const struct kinfo_proc*
/// kproc, Process* proc)` from `FreeBSDProcessTable.c:79` (the `KERN_PROC_CWD`
/// branch). Reads the working directory via `sysctl(KERN_PROC_CWD)`, clearing
/// `procCwd` on failure or an empty (kernel-thread) buffer.
pub fn FreeBSDProcessTable_updateCwd(kproc: &libc::kinfo_proc, proc: &mut Process) {
    let mut mib: [c_int; 4] = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_CWD,
        kproc.ki_pid,
    ];
    let mut buffer = [0u8; 2048];
    let mut size = buffer.len();
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            4,
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        proc.procCwd = None;
        return;
    }

    // Kernel threads return an empty buffer.
    if buffer[0] == 0 {
        proc.procCwd = None;
        return;
    }

    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    proc.procCwd = Some(String::from_utf8_lossy(&buffer[..end]).into_owned());
}

/// Port of `static void FreeBSDProcessTable_updateProcessName(kvm_t* kd, const
/// struct kinfo_proc* kproc, Process* proc)` from `FreeBSDProcessTable.c:103`.
/// Sets the short command (`ki_comm`) and reconstructs the full command line
/// from the process argv (`kvm_getargv`), joining args with spaces and
/// recording the end of `argv[0]` as the basename boundary. When argv is
/// unavailable it falls back to `ki_comm`.
pub fn FreeBSDProcessTable_updateProcessName(
    kd: *mut libc::kvm_t,
    kproc: &libc::kinfo_proc,
    proc: &mut Process,
) {
    let comm = fixed_cstr!(&kproc.ki_comm);
    Process_updateComm(proc, Some(&comm));

    let argv = unsafe { libc::kvm_getargv(kd, kproc as *const libc::kinfo_proc, 0) };
    if argv.is_null() || unsafe { (*argv).is_null() } {
        Process_updateCmdline(proc, Some(&comm), 0, comm.len());
        return;
    }

    let mut cmdline = String::new();
    let mut end = 0usize;
    let mut i: isize = 0;
    loop {
        let p = unsafe { *argv.offset(i) };
        if p.is_null() {
            break;
        }
        let arg = unsafe { CStr::from_ptr(p) }.to_string_lossy();
        cmdline.push_str(&arg);
        if end == 0 {
            end = cmdline.len();
        }
        cmdline.push(' ');
        i += 1;
    }
    // Drop the trailing separator space (C `at--; *at = '\0';`).
    if cmdline.ends_with(' ') {
        cmdline.pop();
    }

    let end = end.min(cmdline.len());
    Process_updateCmdline(proc, Some(&cmdline), 0, end);
}

/// Port of `static char* FreeBSDProcessTable_readJailName(const struct
/// kinfo_proc* kproc)` from `FreeBSDProcessTable.c:135`. Resolves a jail's
/// name via `jail_get(JID → name)`. `ki_jid == 0` (host) is `"-"`; a mismatch
/// / lookup failure is `None`.
pub fn FreeBSDProcessTable_readJailName(kproc: &libc::kinfo_proc) -> Option<String> {
    if kproc.ki_jid == 0 {
        return Some("-".to_string());
    }

    let mut jnamebuf = [0 as c_char; libc::MAXHOSTNAMELEN as usize];
    let key_jid = b"jid\0";
    let key_name = b"name\0";

    let mut jiov: [libc::iovec; 4] = [
        libc::iovec {
            iov_base: key_jid.as_ptr() as *mut c_void,
            iov_len: key_jid.len(),
        },
        libc::iovec {
            iov_base: &kproc.ki_jid as *const c_int as *mut c_void,
            iov_len: size_of::<c_int>(),
        },
        libc::iovec {
            iov_base: key_name.as_ptr() as *mut c_void,
            iov_len: key_name.len(),
        },
        libc::iovec {
            iov_base: jnamebuf.as_mut_ptr() as *mut c_void,
            iov_len: jnamebuf.len(),
        },
    ];

    let jid = unsafe { libc::jail_get(jiov.as_mut_ptr(), 4, 0) };
    if jid == kproc.ki_jid {
        Some(fixed_cstr!(&jnamebuf))
    } else {
        None
    }
}

/// Port of `void ProcessTable_goThroughEntries(ProcessTable* super)` from
/// `FreeBSDProcessTable.c:160`. Walks the `kvm_getprocs(KERN_PROC_PROC)`
/// snapshot, finding-or-creating each process, filling its immutable identity
/// on first sight and its mutable stats (memory / CPU% / priority / scheduling
/// class / state) every scan.
///
/// Deviation (documented): the `proc->user = UsersTable_getRef(...)`
/// assignments are skipped — the `usersTable` handle is opaque (see the module
/// docs). `SCHEDULER_SUPPORT` is undefined here (as in the reference build), so
/// the `Scheduling_readProcessPolicy` block is omitted.
pub fn ProcessTable_goThroughEntries(fpt: &mut FreeBSDProcessTable) {
    let host = fpt.super_.super_.host;
    let fhost = host as *const FreeBSDMachine;

    // const Settings* settings = host->settings;
    let (hideKernelThreads, hideUserlandThreads, updateProcessNames, ss_flags) = unsafe {
        let s = (*host).settings.as_ref();
        (
            s.is_some_and(|s| s.hideKernelThreads),
            s.is_some_and(|s| s.hideUserlandThreads),
            s.is_some_and(|s| s.updateProcessNames),
            s.and_then(|s| s.screens.get(s.ssIndex as usize))
                .map_or(0u32, |ss| ss.flags),
        )
    };

    let osreldate = fpt.osreldate;
    let kd = unsafe { (*fhost).kd };
    let pageSizeKb = unsafe { (*fhost).pageSizeKb } as i64;
    let kernelFScale = unsafe { (*fhost).kernelFScale } as f64;
    let totalMem = unsafe { (*host).totalMem } as f64;
    let realtimeMs = unsafe { (*host).realtimeMs };

    let mut count: c_int = 0;
    let kprocs = unsafe { libc::kvm_getprocs(kd, libc::KERN_PROC_PROC, 0, &mut count) };
    if kprocs.is_null() {
        return;
    }

    let nodev = (-1i32) as libc::dev_t;

    for i in 0..count as isize {
        let kproc: &libc::kinfo_proc = unsafe { &*kprocs.offset(i) };

        let (preExisting, idx) = ProcessTable_getProcess(&mut fpt.super_, kproc.ki_pid, |h| {
            FreeBSDProcess_new(h) as Box<dyn Object>
        });

        // Recover a raw `*mut FreeBSDProcess` for this row (checked borrow
        // ends here). `Object: Any` → downcast to the concrete row type.
        let fp: *mut FreeBSDProcess = {
            let obj: &mut dyn Object = fpt.super_.super_.rows[idx].as_mut().unwrap().as_mut();
            (obj as &mut dyn Any)
                .downcast_mut::<FreeBSDProcess>()
                .unwrap()
        };

        // SAFETY: `fp` aliases a field inside `*fpt_ptr`; process-field writes
        // and the table's *disjoint* counter fields never touch the same
        // memory, mirroring htop's raw `FreeBSDProcess*`/`FreeBSDProcessTable*`
        // pointer graph. `rows` is not reallocated between deriving `fp` and
        // using it (no further `getProcess` this iteration).
        let fpt_ptr = fpt as *mut FreeBSDProcessTable;
        unsafe {
            if !preExisting {
                (*fp).jid = kproc.ki_jid;
                Process_setPid(&mut (*fp).super_, kproc.ki_pid);
                Process_setThreadGroup(&mut (*fp).super_, kproc.ki_pid);
                Process_setParent(&mut (*fp).super_, kproc.ki_ppid);
                (*fp).super_.isKernelThread =
                    kproc.ki_pid != 1 && (kproc.ki_flag & libc::P_SYSTEM as c_long) != 0;
                (*fp).super_.isUserlandThread = false;
                (*fp).super_.tpgid = kproc.ki_tpgid;
                (*fp).super_.session = kproc.ki_sid;
                (*fp).super_.pgrp = kproc.ki_pgid;
                (*fp).super_.st_uid = kproc.ki_uid;
                (*fp).super_.starttime_ctime = kproc.ki_start.tv_sec as i64;
                if (*fp).super_.starttime_ctime < 0 {
                    (*fp).super_.starttime_ctime = (realtimeMs / 1000) as i64;
                }
                Process_fillStarttimeBuffer(&mut (*fp).super_);
                // proc->user = UsersTable_getRef(...) — usersTable opaque, skipped.
                // ProcessTable_add — getProcess already registered the row.

                FreeBSDProcessTable_updateExe(kproc, &mut (*fp).super_);
                FreeBSDProcessTable_updateProcessName(kd, kproc, &mut (*fp).super_);

                if ss_flags & PROCESS_FLAG_CWD != 0 {
                    FreeBSDProcessTable_updateCwd(kproc, &mut (*fp).super_);
                }

                (*fp).jname = FreeBSDProcessTable_readJailName(kproc);

                (*fp).super_.tty_nr = kproc.ki_tdev;
                let name = if kproc.ki_tdev as libc::dev_t != nodev {
                    libc::devname(kproc.ki_tdev as libc::dev_t, libc::S_IFCHR)
                } else {
                    ptr::null_mut()
                };
                if name.is_null() {
                    (*fp).super_.tty_name = None;
                } else {
                    (*fp).super_.tty_name =
                        Some(CStr::from_ptr(name).to_string_lossy().into_owned());
                }
            } else {
                if (*fp).jid != kproc.ki_jid {
                    // process can enter jail anytime
                    (*fp).jid = kproc.ki_jid;
                    (*fp).jname = FreeBSDProcessTable_readJailName(kproc);
                }
                // reapers can reparent a process anytime
                Process_setParent(&mut (*fp).super_, kproc.ki_ppid);
                if (*fp).super_.st_uid != kproc.ki_uid {
                    // some processes change users (eg. to lower privs)
                    (*fp).super_.st_uid = kproc.ki_uid;
                    // proc->user = UsersTable_getRef(...) — usersTable opaque.
                }
                if updateProcessNames {
                    FreeBSDProcessTable_updateProcessName(kd, kproc, &mut (*fp).super_);
                }
            }

            (*fp).emul = Some(fixed_cstr!(&kproc.ki_emul));

            // from FreeBSD source /src/usr.bin/top/machine.c
            (*fp).super_.m_virt = kproc.ki_size as i64 / ONE_K;
            (*fp).super_.m_resident = kproc.ki_rssize as i64 * pageSizeKb;
            (*fp).super_.nlwp = kproc.ki_numthreads as i64;
            (*fp).super_.time = (kproc.ki_runtime + 5000) / 10000;

            (*fp).super_.percent_cpu = (100.0 * kproc.ki_pctcpu as f64 / kernelFScale) as f32;
            (*fp).super_.percent_mem =
                (100.0 * (*fp).super_.m_resident as f64 / totalMem) as f32;
            Process_updateCPUFieldWidths((*fp).super_.percent_cpu);

            if kproc.ki_stat == libc::SRUN && kproc.ki_oncpu != NOCPU {
                (*fp).super_.processor = kproc.ki_oncpu;
            } else {
                (*fp).super_.processor = kproc.ki_lastcpu;
            }

            (*fp).super_.majflt = kproc.ki_cow as u64;

            let refpoint = if osreldate >= OSRELDATE_PUSER_THRESHOLD {
                libc::PUSER
            } else {
                libc::PZERO
            };
            (*fp).super_.priority = kproc.ki_pri.pri_level as i64 - refpoint as i64;

            match PRI_BASE(kproc.ki_pri.pri_class as c_int) {
                PRI_ITHD => {
                    (*fp).sched_class = FreeBSDSchedClass::SCHEDCLASS_INTR_THREAD;
                    (*fp).super_.nice = 0;
                }
                PRI_REALTIME => {
                    (*fp).sched_class = FreeBSDSchedClass::SCHEDCLASS_REALTIME;
                    (*fp).super_.nice = if kproc.ki_flag & libc::P_KPROC as c_long != 0 {
                        kproc.ki_pri.pri_native as c_int - libc::PRI_MIN_REALTIME
                    } else {
                        kproc.ki_pri.pri_user as c_int - libc::PRI_MIN_REALTIME
                    };
                }
                PRI_IDLE => {
                    (*fp).sched_class = FreeBSDSchedClass::SCHEDCLASS_IDLE;
                    (*fp).super_.nice = if kproc.ki_flag & libc::P_KPROC as c_long != 0 {
                        kproc.ki_pri.pri_native as c_int - libc::PRI_MIN_IDLE
                    } else {
                        kproc.ki_pri.pri_user as c_int - libc::PRI_MIN_IDLE
                    };
                }
                PRI_TIMESHARE => {
                    (*fp).sched_class = FreeBSDSchedClass::SCHEDCLASS_TIMESHARE;
                    (*fp).super_.nice = kproc.ki_nice as c_int - libc::NZERO;
                }
                _ => {
                    (*fp).sched_class = FreeBSDSchedClass::SCHEDCLASS_UNKNOWN;
                    (*fp).super_.nice = PROCESS_NICE_UNKNOWN;
                }
            }

            (*fp).super_.state = match kproc.ki_stat {
                libc::SIDL => ProcessState::IDLE,
                libc::SRUN => ProcessState::RUNNING,
                libc::SSLEEP => ProcessState::SLEEPING,
                libc::SSTOP => ProcessState::STOPPED,
                libc::SZOMB => ProcessState::ZOMBIE,
                libc::SWAIT => ProcessState::WAITING,
                libc::SLOCK => ProcessState::BLOCKED,
                _ => ProcessState::UNKNOWN,
            };

            if Process_isKernelThread(&(*fp).super_) {
                (*fpt_ptr).super_.kernelThreads += 1;
            }

            // SCHEDULER_SUPPORT is undefined here (reference build parity).

            let is_kernel = Process_isKernelThread(&(*fp).super_);
            let is_userland = Process_isUserlandThread(&(*fp).super_);
            (*fp).super_.super_.show =
                !((hideKernelThreads && is_kernel) || (hideUserlandThreads && is_userland));

            (*fpt_ptr).super_.totalTasks += 1;
            if (*fp).super_.state == ProcessState::RUNNING {
                (*fpt_ptr).super_.runningTasks += 1;
            }
            (*fp).super_.super_.updated = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn super_is_at_offset_zero() {
        assert_eq!(core::mem::offset_of!(FreeBSDProcessTable, super_), 0);
    }

    #[test]
    fn new_caches_osreldate_and_wires_scan_class() {
        let host = 0xF00D as *const Machine;
        let pt = ProcessTable_new(host, Some(0xBEEF));

        assert!(pt.osreldate > 0);
        assert_eq!(pt.super_.super_.host, host);
        assert_eq!(pt.super_.pidMatchList.map(|p| p as usize), Some(0xBEEF));
        assert!(pt.super_.super_.rows.is_empty());
        assert!(!pt.super_.super_.klass.is_null());
    }

    #[test]
    fn readJailName_reports_host_for_jid_zero() {
        // Build a zeroed kinfo_proc with ki_jid = 0 (the host).
        let mut kp: libc::kinfo_proc = unsafe { core::mem::zeroed() };
        kp.ki_jid = 0;
        assert_eq!(FreeBSDProcessTable_readJailName(&kp).as_deref(), Some("-"));
    }
}
