//! Port of `dragonflybsd/DragonFlyBSDProcessTable.c` + `.h` — the DragonFly BSD
//! process-table scan layer. Compiled only under `#[cfg(target_os =
//! "dragonfly")]`; like the other BSD layers it is verified by primary-source
//! reading + the port-purity gate (not a cross-compile, DragonFly being a
//! tier-3 target with no prebuilt std here).
//!
//! Ported: [`ProcessTable_new`] (table alloc + scan-vtable wiring),
//! [`ProcessTable_goThroughEntries`] (the `kvm_getprocs` scan driver),
//! [`DragonFlyBSDProcessTable_updateExe`] (`/proc/<pid>/file` readlink),
//! [`DragonFlyBSDProcessTable_updateCwd`] (`kern.proc.cwd` sysctl), and
//! [`DragonFlyBSDProcessTable_updateProcessName`] (`kvm_getargv`).
//!
//! `kvm_getprocs`/`kvm_openfiles` are in the shared `freebsdlike` `libc`; only
//! `kvm_getargv` is not exposed for the DragonFly target, so it is hand-declared
//! in an `extern` block — the NetBSD `kvm_getargv2` precedent. The DragonFly
//! `p_flags`/`lwp_flags`/`kl_tdflags` and `MAXSLP` constants (also absent from
//! `libc`) are declared as module consts, read verbatim from the DragonFly
//! source at the commit htop cites.
#![allow(non_snake_case)]
#![allow(dead_code)]

use core::slice;
use std::any::Any;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::ptr;

use crate::ported::dragonflybsd::dragonflybsdmachine::{
    DragonFlyBSDMachine, DragonFlyBSDMachine_readJailName,
};
use crate::ported::dragonflybsd::dragonflybsdprocess::{
    DragonFlyBSDProcess, DragonFlyBSDProcess_new,
};
use crate::ported::machine::Machine;
use crate::ported::object::Object;
use crate::ported::process::{
    Process, ProcessState, Process_fillStarttimeBuffer, Process_isKernelThread,
    Process_isUserlandThread, Process_setParent, Process_setPid, Process_setThreadGroup,
    Process_updateCPUFieldWidths, Process_updateCmdline, Process_updateComm, Process_updateExe,
    PROCESS_FLAG_CWD,
};
use crate::ported::processtable::{
    ProcessTable, ProcessTable_cleanupEntries, ProcessTable_getProcess, ProcessTable_init,
    ProcessTable_prepareEntries,
};
use crate::ported::table::{Table, TableClass};

// DragonFly kernel constants not exposed by `libc` for this target — values
// taken verbatim from the DragonFly source at the commit htop cites
// (github.com/DragonFlyBSD/DragonFlyBSD @ c163a4d7):
//   sys/sys/proc.h — process `p_flags` and lwp `lwp_flags` bits.
const P_SWAPPEDOUT: c_int = 0x00004;
const P_SYSTEM: c_int = 0x00200;
const P_TRACED: c_int = 0x00800;
const P_JAILED: c_int = 0x01000000;
const LWP_SINTR: c_int = 0x00000008;
//   sys/sys/thread.h — lwp `kl_tdflags` bits.
const TDF_RUNNING: c_int = 0x00000001;
const TDF_RUNQ: c_int = 0x00000002;
const TDF_SINTR: c_int = 0x00000040;
//   sys/platform/pc64/include/vmparam.h — max sleep (seconds) before "idle".
const MAXSLP: c_uint = 20;
// Macros.h: ONE_K.
const ONE_K: i64 = 1024;

extern "C" {
    /// `char** kvm_getargv(kvm_t*, const struct kinfo_proc*, int nchr)`
    /// (DragonFly `kvm.h`). Not exposed by `libc` for the DragonFly target, so
    /// it is declared here (the NetBSD `kvm_getargv2` precedent). (`kvm_getprocs`
    /// IS in the shared `freebsdlike` `libc`, so the scan uses `libc::`.)
    fn kvm_getargv(
        kd: *mut c_void,
        p: *const libc::kinfo_proc,
        nchr: c_int,
    ) -> *const *const c_char;
}

/// Port of `typedef struct DragonFlyBSDProcessTable_`
/// (`DragonFlyBSDProcessTable.h`). "Extends" [`ProcessTable`] via the embedded
/// `super_`; DragonFly adds no fields of its own. (No derives: the shared
/// [`ProcessTable`] models trait-object/handle fields and is neither `Debug`
/// nor `Default`; construct via [`ProcessTable::empty`].)
pub struct DragonFlyBSDProcessTable {
    /// C `ProcessTable super`.
    pub super_: ProcessTable,
}

/// Scan-vtable glue (the `TableClass` slots) for `DragonFlyBSDProcessTable`,
/// each downcasting the base `*mut Table` and delegating — the FreeBSD/darwin
/// precedent. `prepare`/`cleanup` reuse the shared base entry points.
impl DragonFlyBSDProcessTable {
    fn scan_prepare(super_: *mut Table) {
        let this = super_ as *mut DragonFlyBSDProcessTable;
        // SAFETY: `super_` is the base of a live `DragonFlyBSDProcessTable`.
        ProcessTable_prepareEntries(unsafe { &mut (*this).super_ });
    }

    fn scan_iterate(super_: *mut Table) {
        let this = super_ as *mut DragonFlyBSDProcessTable;
        // SAFETY: `super_` is the base of a live `DragonFlyBSDProcessTable`.
        ProcessTable_goThroughEntries(unsafe { &mut *this });
    }

    fn scan_cleanup(super_: *mut Table) {
        let this = super_ as *mut DragonFlyBSDProcessTable;
        // SAFETY: `super_` is the base of a live `DragonFlyBSDProcessTable`.
        ProcessTable_cleanupEntries(unsafe { &mut (*this).super_ });
    }
}

/// The scan-vtable half of `ProcessTable_class` as it applies to the DragonFly
/// table: `iterate` link-resolves to [`ProcessTable_goThroughEntries`], the C
/// `ProcessTable_new` wires this via `Object_setClass`.
pub static DragonFlyBSDProcessTable_class: TableClass = TableClass {
    prepare: Some(DragonFlyBSDProcessTable::scan_prepare),
    iterate: Some(DragonFlyBSDProcessTable::scan_iterate),
    cleanup: Some(DragonFlyBSDProcessTable::scan_cleanup),
};

/// Port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` (`DragonFlyBSDProcessTable.c:30`). Allocates the table, runs
/// the base [`ProcessTable_init`], and wires the scan vtable (the C
/// `Object_setClass(this, Class(ProcessTable))`). Returns the owning `Box`
/// (C returns `&this->super`); DragonFly adds no fields, so no extra init.
pub fn ProcessTable_new(
    host: *const Machine,
    pidMatchList: Option<usize>,
) -> Box<DragonFlyBSDProcessTable> {
    let mut this = Box::new(DragonFlyBSDProcessTable {
        super_: ProcessTable::empty(),
    });

    ProcessTable_init(&mut this.super_, host, pidMatchList);

    this.super_.super_.klass = &DragonFlyBSDProcessTable_class as *const TableClass;

    this
}

/// Port of `void ProcessTable_delete(Object* cast)`
/// (`DragonFlyBSDProcessTable.c:40`). The C body is `ProcessTable_done(&this->super)`
/// then `free(this)`. Take `this` by value: `ProcessTable_done` tears the base
/// table down in place and `this` drops at scope end (the `free(this)`),
/// matching the darwin `ProcessTable_delete` precedent.
pub fn ProcessTable_delete(mut this: DragonFlyBSDProcessTable) {
    crate::ported::processtable::ProcessTable_done(&mut this.super_);
}

/// Port of `static void DragonFlyBSDProcessTable_updateExe(const struct
/// kinfo_proc* kproc, Process* proc)` (`DragonFlyBSDProcessTable.c:64`), the
/// active (`readlink`) variant. Resolves the executable via
/// `readlink("/proc/<pid>/file")`; kernel threads (and any read error) leave
/// `procExe` untouched — the C's early `return`s.
pub fn DragonFlyBSDProcessTable_updateExe(kproc: &libc::kinfo_proc, proc: &mut Process) {
    // if (Process_isKernelThread(proc)) return;
    if Process_isKernelThread(proc) {
        return;
    }

    // xSnprintf(path, sizeof(path), "/proc/%d/file", kproc->kp_pid);
    let path = std::ffi::CString::new(format!("/proc/{}/file", kproc.kp_pid)).unwrap();

    // ssize_t ret = readlink(path, target, sizeof(target) - 1); if (ret <= 0) return;
    let mut target = [0u8; libc::PATH_MAX as usize];
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

    // target[ret] = '\0'; Process_updateExe(proc, target);
    let s = String::from_utf8_lossy(&target[..ret as usize]);
    Process_updateExe(proc, Some(&s));
}

/// Port of `static void DragonFlyBSDProcessTable_updateCwd(const struct
/// kinfo_proc* kproc, Process* proc)` (`DragonFlyBSDProcessTable.c:80`). Reads
/// the working directory via the `{ CTL_KERN, KERN_PROC, KERN_PROC_CWD, pid }`
/// sysctl; a failed read or an empty buffer (kernel threads) clears `procCwd`.
pub fn DragonFlyBSDProcessTable_updateCwd(kproc: &libc::kinfo_proc, proc: &mut Process) {
    let mut mib: [c_int; 4] = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_CWD,
        kproc.kp_pid,
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

/// Port of `static void DragonFlyBSDProcessTable_updateProcessName(kvm_t* kd,
/// const struct kinfo_proc* kproc, Process* proc)`
/// (`DragonFlyBSDProcessTable.c:100`). Sets the short command from `kp_comm`,
/// then rebuilds the full command line from `kvm_getargv` (joining args with
/// spaces, `end` = length of `argv[0]`); any failure falls back to `kp_comm`.
pub fn DragonFlyBSDProcessTable_updateProcessName(
    kd: *mut c_void,
    kproc: &libc::kinfo_proc,
    proc: &mut Process,
) {
    // Read a NUL-terminated fixed C char buffer into an owned lossy String (the
    // C treats `kp_comm` as an inline C string); nested so it stays a faithful
    // translation without a module-level non-C function.
    fn c_field_to_string(buf: &[c_char]) -> String {
        let bytes: &[u8] = unsafe { slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len()) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }

    // Process_updateComm(proc, kproc->kp_comm);
    let comm = c_field_to_string(&kproc.kp_comm);
    Process_updateComm(proc, Some(&comm));

    // char** argv = kvm_getargv(kd, kproc, 0);
    let argv = unsafe { kvm_getargv(kd, kproc as *const libc::kinfo_proc, 0) };
    // if (!argv || !argv[0]) { Process_updateCmdline(proc, kp_comm, 0, strlen); return; }
    if argv.is_null() || unsafe { (*argv).is_null() } {
        Process_updateCmdline(proc, Some(&comm), 0, comm.len());
        return;
    }

    // Join argv with spaces; `end` is the length of argv[0] (C `stpcpy` loop,
    // recording `end` after the first arg, then dropping the trailing space).
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
    // C: at--; *at = '\0';  — drop the trailing separator space.
    if cmdline.ends_with(' ') {
        cmdline.pop();
    }

    let end = end.min(cmdline.len());
    Process_updateCmdline(proc, Some(&cmdline), 0, end);
}

/// Port of `void ProcessTable_goThroughEntries(ProcessTable* super)`
/// (`DragonFlyBSDProcessTable.c:133`). The main scan: `kvm_getprocs` over all
/// processes (plus LWPs when userland threads are shown), constructing/refreshing
/// each `DragonFlyBSDProcess` from its `kinfo_proc` and mapping the DragonFly
/// `kp_stat`/`kl_stat`/flag bits onto the shared [`ProcessState`].
///
/// Mirrors the FreeBSD scan's ownership model: `ProcessTable_getProcess` returns
/// `(preExisting, idx)` and registers the fresh row itself (so no separate
/// `ProcessTable_add`); a raw `*mut DragonFlyBSDProcess` is downcast from
/// `rows[idx]` and a `*mut DragonFlyBSDProcessTable` is taken so the disjoint
/// per-table counter writes don't alias the per-process writes.
///
/// Deviations (documented): the C's `proc->user = UsersTable_getRef(...)` is
/// skipped — `Machine::usersTable` is an opaque handle here (the FreeBSD-scan
/// precedent), `st_uid` is still tracked. The C's `ATTR_UNUSED isIdleProcess`
/// is a dead write (never read) and is omitted.
pub fn ProcessTable_goThroughEntries(this: &mut DragonFlyBSDProcessTable) {
    let host = this.super_.super_.host;
    let dhost = host as *const DragonFlyBSDMachine;

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

    let kd = unsafe { (*dhost).kd }; // kvm_t == c_void, usable directly
    let pageSizeKb = unsafe { (*dhost).pageSizeKb } as i64;
    let kernelFScale = unsafe { (*dhost).kernelFScale } as f64;
    let totalMem = unsafe { (*host).totalMem } as f64;

    // char** ... kvm_getprocs(kd, KERN_PROC_ALL | (!hideUserlandThreads ? KERN_PROC_FLAG_LWP : 0), 0, &count)
    let op = libc::KERN_PROC_ALL
        | if !hideUserlandThreads {
            libc::KERN_PROC_FLAG_LWP
        } else {
            0
        };
    let mut count: c_int = 0;
    let kprocs = unsafe { libc::kvm_getprocs(kd, op, 0, &mut count) };
    if kprocs.is_null() {
        return;
    }

    let nodev = (-1i32) as libc::dev_t;

    for i in 0..count as isize {
        let kproc: &libc::kinfo_proc = unsafe { &*kprocs.offset(i) };

        // dragonflybsd kernel processes all share the same pid, so the kernel
        // thread address doubles as the unique identifier.
        let key_pid = if kproc.kp_ktaddr != 0 {
            kproc.kp_ktaddr as i32
        } else {
            kproc.kp_pid
        };

        let (preExisting, idx) = ProcessTable_getProcess(&mut this.super_, key_pid, |h| {
            Box::new(DragonFlyBSDProcess_new(h)) as Box<dyn Object>
        });

        // Recover a raw `*mut DragonFlyBSDProcess` for this row (checked borrow
        // ends here). `Object: Any` → downcast to the concrete row type.
        let dfp: *mut DragonFlyBSDProcess = {
            let obj: &mut dyn Object = this.super_.super_.rows[idx].as_mut().unwrap().as_mut();
            (obj as &mut dyn Any)
                .downcast_mut::<DragonFlyBSDProcess>()
                .unwrap()
        };

        let dpt_ptr = this as *mut DragonFlyBSDProcessTable;
        unsafe {
            if !preExisting {
                (*dfp).jid = kproc.kp_jailid;
                if kproc.kp_ktaddr != 0 && (kproc.kp_flags & P_SYSTEM) != 0 {
                    // dfb kernel threads all share the same pid; use the kernel
                    // thread address as a unique identifier.
                    Process_setPid(&mut (*dfp).super_, kproc.kp_ktaddr as i32);
                    (*dfp).super_.isKernelThread = true;
                } else {
                    Process_setPid(&mut (*dfp).super_, kproc.kp_pid);
                    (*dfp).super_.isKernelThread = false;
                }
                (*dfp).super_.isUserlandThread = kproc.kp_nthreads > 1;
                Process_setParent(&mut (*dfp).super_, kproc.kp_ppid);
                (*dfp).super_.tpgid = kproc.kp_tpgid;
                Process_setThreadGroup(&mut (*dfp).super_, kproc.kp_pid);
                (*dfp).super_.pgrp = kproc.kp_pgid;
                (*dfp).super_.session = kproc.kp_sid;
                (*dfp).super_.st_uid = kproc.kp_uid;
                (*dfp).super_.processor = kproc.kp_lwp.kl_origcpu;
                (*dfp).super_.starttime_ctime = kproc.kp_start.tv_sec as i64;
                Process_fillStarttimeBuffer(&mut (*dfp).super_);
                // proc->user = UsersTable_getRef(...) — usersTable opaque, skipped.

                (*dfp).super_.tty_nr = kproc.kp_tdev as u64;
                let name = if kproc.kp_tdev != nodev {
                    libc::devname(kproc.kp_tdev, libc::S_IFCHR as libc::mode_t)
                } else {
                    ptr::null_mut()
                };
                if name.is_null() {
                    (*dfp).super_.tty_name = None;
                } else {
                    (*dfp).super_.tty_name =
                        Some(CStr::from_ptr(name).to_string_lossy().into_owned());
                }

                DragonFlyBSDProcessTable_updateExe(kproc, &mut (*dfp).super_);
                DragonFlyBSDProcessTable_updateProcessName(kd, kproc, &mut (*dfp).super_);

                if ss_flags & PROCESS_FLAG_CWD != 0 {
                    DragonFlyBSDProcessTable_updateCwd(kproc, &mut (*dfp).super_);
                }
                // ProcessTable_add — getProcess already registered the row.

                (*dfp).jname = Some(DragonFlyBSDMachine_readJailName(&*dhost, kproc.kp_jailid));
            } else {
                (*dfp).super_.processor = kproc.kp_lwp.kl_cpuid;
                if (*dfp).jid != kproc.kp_jailid {
                    // process can enter jail anytime
                    (*dfp).jid = kproc.kp_jailid;
                    (*dfp).jname = Some(DragonFlyBSDMachine_readJailName(&*dhost, kproc.kp_jailid));
                }
                // if there are reapers in the system, process can get reparented anytime
                Process_setParent(&mut (*dfp).super_, kproc.kp_ppid);
                if (*dfp).super_.st_uid != kproc.kp_uid {
                    // some processes change users (eg. to lower privs)
                    (*dfp).super_.st_uid = kproc.kp_uid;
                    // proc->user = UsersTable_getRef(...) — usersTable opaque.
                }
                if updateProcessNames {
                    DragonFlyBSDProcessTable_updateProcessName(kd, kproc, &mut (*dfp).super_);
                }
            }

            (*dfp).super_.m_virt = kproc.kp_vm_map_size as i64 / ONE_K;
            (*dfp).super_.m_resident = kproc.kp_vm_rssize as i64 * pageSizeKb;
            (*dfp).super_.nlwp = kproc.kp_nthreads as i64;
            (*dfp).super_.time =
                (kproc.kp_lwp.kl_uticks + kproc.kp_lwp.kl_sticks + kproc.kp_lwp.kl_iticks) / 10000;

            (*dfp).super_.percent_cpu =
                (100.0 * kproc.kp_lwp.kl_pctcpu as f64 / kernelFScale) as f32;
            (*dfp).super_.percent_mem = (100.0 * (*dfp).super_.m_resident as f64 / totalMem) as f32;
            Process_updateCPUFieldWidths((*dfp).super_.percent_cpu);

            if kproc.kp_lwp.kl_pid != -1 {
                (*dfp).super_.priority = kproc.kp_lwp.kl_prio as i64;
            } else {
                (*dfp).super_.priority = -(kproc.kp_lwp.kl_tdprio as i64);
            }

            (*dfp).super_.nice = match kproc.kp_lwp.kl_rtprio.type_ {
                libc::RTP_PRIO_REALTIME => {
                    libc::PRIO_MIN - 1 - libc::RTP_PRIO_MAX as c_int
                        + kproc.kp_lwp.kl_rtprio.prio as c_int
                }
                libc::RTP_PRIO_IDLE => libc::PRIO_MAX + 1 + kproc.kp_lwp.kl_rtprio.prio as c_int,
                libc::RTP_PRIO_THREAD => {
                    libc::PRIO_MIN
                        - 1
                        - libc::RTP_PRIO_MAX as c_int
                        - kproc.kp_lwp.kl_rtprio.prio as c_int
                }
                _ => kproc.kp_nice,
            };

            // Map DragonFly proc/lwp state onto the shared enum. (Taken from
            // sys/sys/proc_common.h.) The `procstat`/`lwpstat` libc enums are
            // exhaustive, so the C switch `default`s are structurally unreachable.
            let mut state = match kproc.kp_stat {
                libc::procstat::SIDL => ProcessState::IDLE,
                libc::procstat::SACTIVE => match kproc.kp_lwp.kl_stat {
                    libc::lwpstat::LSSLEEP => {
                        if kproc.kp_lwp.kl_flags & LWP_SINTR != 0 {
                            // interruptible wait short/long
                            if kproc.kp_lwp.kl_slptime >= MAXSLP {
                                ProcessState::IDLE
                            } else {
                                ProcessState::SLEEPING
                            }
                        } else if kproc.kp_lwp.kl_tdflags & TDF_SINTR != 0 {
                            // interruptible lwkt wait
                            ProcessState::SLEEPING
                        } else {
                            // uninterruptible (lwkt) wait
                            ProcessState::UNINTERRUPTIBLE_WAIT
                        }
                    }
                    libc::lwpstat::LSRUN => {
                        if kproc.kp_lwp.kl_tdflags & (TDF_RUNNING | TDF_RUNQ) == 0 {
                            ProcessState::QUEUED
                        } else {
                            ProcessState::RUNNING
                        }
                    }
                    libc::lwpstat::LSSTOP => ProcessState::STOPPED,
                },
                libc::procstat::SSTOP => ProcessState::STOPPED,
                libc::procstat::SZOMB => ProcessState::ZOMBIE,
                libc::procstat::SCORE => ProcessState::BLOCKED,
            };

            if kproc.kp_flags & P_SWAPPEDOUT != 0 {
                state = ProcessState::SLEEPING;
            }
            if kproc.kp_flags & P_TRACED != 0 {
                state = ProcessState::TRACED;
            }
            if kproc.kp_flags & P_JAILED != 0 {
                state = ProcessState::TRACED;
            }
            (*dfp).super_.state = state;

            if Process_isKernelThread(&(*dfp).super_) {
                (*dpt_ptr).super_.kernelThreads += 1;
            }

            (*dpt_ptr).super_.totalTasks += 1;

            if (*dfp).super_.state == ProcessState::RUNNING {
                (*dpt_ptr).super_.runningTasks += 1;
            }

            let is_kernel = Process_isKernelThread(&(*dfp).super_);
            let is_userland = Process_isUserlandThread(&(*dfp).super_);
            (*dfp).super_.super_.show =
                !((hideKernelThreads && is_kernel) || (hideUserlandThreads && is_userland));
            (*dfp).super_.super_.updated = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The DragonFly process table embeds the shared [`ProcessTable`] base
    /// (constructed via `ProcessTable::empty`, the C `xCalloc` analog).
    #[test]
    fn embeds_processtable_base() {
        let t = DragonFlyBSDProcessTable {
            super_: ProcessTable::empty(),
        };
        assert!(t.super_.pidMatchList.is_none());
    }
}
