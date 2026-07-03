//! Port of `OpenBSDProcessTable.c` — the OpenBSD process table.
//!
//! Ported (on the base [`ProcessTable`]/[`Table`] plumbing, the modeled
//! [`OpenBSDMachine`] + `libkvm` FFI, and `libc`'s `struct kinfo_proc`):
//! - the [`OpenBSDProcessTable`] struct (`OpenBSDProcessTable.h:17`).
//! - [`ProcessTable_new`] (`OpenBSDProcessTable.c:38`) + the scan-class vtable.
//! - [`OpenBSDProcessTable_updateCwd`] (`OpenBSDProcessTable.c:54`).
//! - [`OpenBSDProcessTable_updateProcessName`] (`OpenBSDProcessTable.c:74`).
//! - [`getpcpu`] (`OpenBSDProcessTable.c:126`).
//! - [`OpenBSDProcessTable_scanProcs`] (`OpenBSDProcessTable.c:133`).
//! - [`ProcessTable_goThroughEntries`] (`OpenBSDProcessTable.c:242`).
//!
//! Kept as a documented stub:
//! - `ProcessTable_delete` (`OpenBSDProcessTable.c:48`) is a pure `free()`
//!   teardown; Rust `Drop` reclaims the allocation (darwin precedent).
//!
//! Documented deviations from the C (not silent):
//! - `ProcessTable_findProcess(&this->super, pid)` is not a separate ported
//!   symbol; the thread-merge lookup is inlined as a direct read of the base
//!   `Table`'s pid→row map (exactly what the C `Hashtable_get` does).
//! - the trailing `ProcessTable_add` for a newly-seen process is not repeated:
//!   [`ProcessTable_getProcess`] already registers the row (the darwin
//!   precedent).
//! - `proc->user = UsersTable_getRef(...)` is skipped (the `UsersTable` handle
//!   on `Machine` is unwired); `st_uid` is still tracked.
//!
//! # Verification note
//!
//! OpenBSD is a tier-3 Rust target with no prebuilt `std`; this module cannot
//! be cross-compiled on the darwin dev host. The scan mirrors the compiled
//! darwin `ProcessTable_goThroughEntries`; `libc`'s `kinfo_proc` and the
//! `KERN_PROC_*` constants were verified against the `libc` OpenBSD module.
//! It is source-reviewed, not compile-verified.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // faithful C global name (OpenBSDProcessTable_class)
#![allow(dead_code)]

use core::any::Any;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::ported::machine::Machine;
use crate::ported::object::Object;
use crate::ported::openbsd::openbsdmachine::{kvm_getargv, kvm_getprocs, kvm_t, OpenBSDMachine};
use crate::ported::openbsd::openbsdprocess::{OpenBSDProcess, OpenBSDProcess_new};
use crate::ported::process::{
    Process, ProcessState, Process_fillStarttimeBuffer, Process_isKernelThread,
    Process_isUserlandThread, Process_setParent, Process_setThreadGroup,
    Process_updateCPUFieldWidths, Process_updateCmdline, Process_updateComm, PROCESS_FLAG_CWD,
};
use crate::ported::processtable::{
    ProcessTable, ProcessTable_cleanupEntries, ProcessTable_getProcess, ProcessTable_init,
    ProcessTable_prepareEntries,
};
use crate::ported::table::{Table, TableClass};

/// `#define PZERO 22` (`sys/param.h`) — the priority baseline.
const PZERO: i64 = 22;
/// `NODEV` (`sys/param.h`) — `(dev_t)-1`, the invalid device sentinel.
const NODEV: libc::dev_t = -1;

// `sys/proc.h` process-status codes (the `p_stat` values from the LWP).
const SIDL: i32 = 1;
const SRUN: i32 = 2;
const SSLEEP: i32 = 3;
const SSTOP: i32 = 4;
const SZOMB: i32 = 5;
const SDEAD: i32 = 6;
const SONPROC: i32 = 7;

/// Port of htop's `struct OpenBSDProcessTable_` (`OpenBSDProcessTable.h:17`).
/// "Extends" [`ProcessTable`] via the embedded `super_` field (htop's
/// `ProcessTable super;` first member); OpenBSD adds no per-table fields.
///
/// `#[repr(C)]` guarantees `super_` at offset 0, so htop's
/// `(OpenBSDProcessTable*)tablePtr` downcast is sound.
#[repr(C)]
pub struct OpenBSDProcessTable {
    /// C `ProcessTable super`.
    pub super_: ProcessTable,
}

/// The `TableClass` scan-vtable slots for the OpenBSD process table (the
/// `.prepare`/`.iterate`/`.cleanup` glue of the common `ProcessTable_class`).
/// Each C slot takes `Table* super`, downcasts to `ProcessTable*`, and
/// delegates; here they re-express those against the `#[repr(C)]`
/// `OpenBSDProcessTable` layout (`super_: ProcessTable` at offset 0, whose
/// `super_: Table` is likewise at offset 0). Mirrors the darwin port.
impl OpenBSDProcessTable {
    /// # Safety precondition
    /// `super_` is the base of a live `OpenBSDProcessTable`.
    fn scan_prepare(super_: *mut Table) {
        let this = super_ as *mut OpenBSDProcessTable;
        ProcessTable_prepareEntries(unsafe { &mut (*this).super_ });
    }

    /// # Safety precondition
    /// `super_` is the base of a live `OpenBSDProcessTable`.
    fn scan_iterate(super_: *mut Table) {
        let this = super_ as *mut OpenBSDProcessTable;
        ProcessTable_goThroughEntries(unsafe { &mut *this });
    }

    /// # Safety precondition
    /// `super_` is the base of a live `OpenBSDProcessTable`.
    fn scan_cleanup(super_: *mut Table) {
        let this = super_ as *mut OpenBSDProcessTable;
        ProcessTable_cleanupEntries(unsafe { &mut (*this).super_ });
    }
}

/// Port of `const TableClass ProcessTable_class` (`ProcessTable.c:94`), the
/// class the OpenBSD table runs under. Only the scan-vtable half is modeled
/// (the `ObjectClass super` is class identity in Rust).
pub static OpenBSDProcessTable_class: TableClass = TableClass {
    prepare: Some(OpenBSDProcessTable::scan_prepare),
    iterate: Some(OpenBSDProcessTable::scan_iterate),
    cleanup: Some(OpenBSDProcessTable::scan_cleanup),
};

/// Port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` from `OpenBSDProcessTable.c:38`. C `xCalloc`s an
/// `OpenBSDProcessTable`, sets its class, and runs `ProcessTable_init` on the
/// embedded base with the `OpenBSDProcess` constructor class, returning
/// `&this->super`.
///
/// The returned `Box<OpenBSDProcessTable>` is the owner; the caller derives
/// `&mut box.super_` (`*mut ProcessTable`) and `&mut box.super_.super_`
/// (`*mut Table`). The table's scan class is wired here so
/// `Machine_scanTables` can dispatch prepare/iterate/cleanup through it.
pub fn ProcessTable_new(
    host: *const Machine,
    pidMatchList: Option<usize>,
) -> Box<OpenBSDProcessTable> {
    let mut this = Box::new(OpenBSDProcessTable {
        super_: ProcessTable::empty(),
    });

    ProcessTable_init(&mut this.super_, host, pidMatchList);

    // Object_setClass(this, Class(ProcessTable)) — wire the scan vtable.
    this.super_.super_.klass = &OpenBSDProcessTable_class as *const TableClass;

    this
}

/// TODO: port of `void ProcessTable_delete(Object* cast)` from
/// `OpenBSDProcessTable.c:48`. Kept stubbed: the C body is a pure teardown
/// (`ProcessTable_done(&this->super)` + `free(this)`); Rust `Drop` reclaims
/// the [`OpenBSDProcessTable`] allocation and its owned fields (the darwin
/// precedent).
pub fn ProcessTable_delete() {
    todo!("port of OpenBSDProcessTable.c:48 — pure free() teardown; Rust Drop handles it")
}

/// Port of `static void OpenBSDProcessTable_updateCwd(const struct kinfo_proc*
/// kproc, Process* proc)` from `OpenBSDProcessTable.c:54`. Reads
/// `kern.proc_cwd.<pid>` via `sysctl`; on failure or an empty buffer (kernel
/// threads) it clears `procCwd`, otherwise stores the path.
pub fn OpenBSDProcessTable_updateCwd(kproc: &libc::kinfo_proc, proc: &mut Process) {
    let mib: [c_int; 3] = [libc::CTL_KERN, libc::KERN_PROC_CWD, kproc.p_pid];
    let mut buffer = [0u8; 2048];
    let mut size = buffer.len();
    if unsafe {
        libc::sysctl(
            mib.as_ptr(),
            3,
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    } != 0
    {
        proc.procCwd = None;
        return;
    }

    /* Kernel threads return an empty buffer */
    if buffer[0] == 0 {
        proc.procCwd = None;
        return;
    }

    let n = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    proc.procCwd = Some(String::from_utf8_lossy(&buffer[..n]).into_owned());
}

/// Port of `static void OpenBSDProcessTable_updateProcessName(kvm_t* kd, const
/// struct kinfo_proc* kproc, Process* proc)` from `OpenBSDProcessTable.c:74`.
/// Sets the comm from `p_comm`, then (like OpenBSD's top(1)) rebuilds the full
/// command line from `kvm_getargv`, joining the args with spaces and trimming
/// `argv[0]` at its first unescaped space (the `'kdeinit5: Running…'` case) to
/// find the basename boundary. Any failure falls back to `p_comm`.
pub fn OpenBSDProcessTable_updateProcessName(
    kd: *mut kvm_t,
    kproc: &libc::kinfo_proc,
    proc: &mut Process,
) {
    // Read a NUL-terminated fixed C char array into an owned lossy String (the
    // C treats `p_comm` as a C string inline); nested so it stays a faithful
    // translation without a module-level non-C function.
    fn cstr_field(buf: &[c_char]) -> String {
        let bytes: &[u8] =
            unsafe { core::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len()) };
        let n = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..n]).into_owned()
    }
    // C fallback `Process_updateCmdline(proc, comm, 0, strlen(comm))`; nested
    // for the same reason (inline in the C, not a distinct htop function).
    fn updateCmdlineFromComm(proc: &mut Process, comm: &str) {
        let end = comm.len();
        let arg = if comm.is_empty() { None } else { Some(comm) };
        Process_updateCmdline(proc, arg, 0, end);
    }
    let comm = cstr_field(&kproc.p_comm);
    Process_updateComm(proc, Some(&comm));

    let argv = unsafe { kvm_getargv(kd, kproc, 500) };
    if argv.is_null() {
        updateCmdlineFromComm(proc, &comm);
        return;
    }

    // Collect the NUL-terminated argv (valid until the next kvm call).
    let mut args: Vec<&[u8]> = Vec::new();
    unsafe {
        let mut i: isize = 0;
        loop {
            let p = *argv.offset(i);
            if p.is_null() {
                break;
            }
            args.push(std::ffi::CStr::from_ptr(p).to_bytes());
            i += 1;
        }
    }
    if args.is_empty() {
        updateCmdlineFromComm(proc, &comm);
        return;
    }

    // end = min(strlen(arg0), len - 1); `len - 1 >= strlen(arg0)` always.
    let arg0 = args[0];
    let byte_at = |b: &[u8], j: usize| -> u8 {
        if j < b.len() {
            b[j]
        } else {
            0 // NUL terminator, as the C reads arg[0][end]
        }
    };
    let mut end = arg0.len();
    // check if cmdline ended earlier, e.g 'kdeinit5: Running...'
    let mut j = end;
    while j > 0 {
        if byte_at(arg0, j) == b' ' && byte_at(arg0, j - 1) != b'\\' {
            end = if byte_at(arg0, j - 1) == b':' { j - 1 } else { j };
        }
        j -= 1;
    }

    // s = "arg0 arg1 … " (each arg followed by a space, as strlcat does).
    let mut s = String::new();
    for a in &args {
        s.push_str(&String::from_utf8_lossy(a));
        s.push(' ');
    }

    // `end` is a byte offset into arg0 (== the head of `s`); lossy decoding of
    // non-UTF-8 argv can shift lengths, so clamp into the string bounds.
    let end = end.min(s.len());
    Process_updateCmdline(proc, Some(&s), 0, end);
}

/// Port of `static double getpcpu(const OpenBSDMachine* ohost, const struct
/// kinfo_proc* kp)` from `OpenBSDProcessTable.c:126` (taken from OpenBSD's
/// ps(1)).
pub fn getpcpu(ohost: &OpenBSDMachine, kp: &libc::kinfo_proc) -> f64 {
    if ohost.fscale == 0 {
        return 0.0;
    }
    100.0 * kp.p_pctcpu as f64 / ohost.fscale as f64
}

/// Port of `static void OpenBSDProcessTable_scanProcs(OpenBSDProcessTable*
/// this)` from `OpenBSDProcessTable.c:133`. Walks the `kvm_getprocs` snapshot
/// (kernel threads + LWPs), merges threads into their containing process, and
/// fills each row from its `kinfo_proc`.
pub fn OpenBSDProcessTable_scanProcs(this: &mut OpenBSDProcessTable) {
    let host = this.super_.super_.host;
    let ohost = host as *const OpenBSDMachine;

    let (hideKernelThreads, hideUserlandThreads, updateProcessNames, cwdFlag) = unsafe {
        let settings = (*host)
            .settings
            .as_ref()
            .expect("OpenBSDProcessTable_scanProcs: settings unset");
        let ss = &settings.screens[settings.ssIndex as usize];
        (
            settings.hideKernelThreads,
            settings.hideUserlandThreads,
            settings.updateProcessNames,
            (ss.flags & PROCESS_FLAG_CWD) != 0,
        )
    };

    let kd = unsafe { (*ohost).kd };
    let pageSizeKB = unsafe { (*ohost).pageSizeKB } as i64;
    let totalMem = unsafe { (*host).totalMem };
    let activeCPUs = unsafe { (*host).activeCPUs };

    let mut count: c_int = 0;
    let kprocs = unsafe {
        kvm_getprocs(
            kd,
            libc::KERN_PROC_KTHREAD | libc::KERN_PROC_SHOW_THREADS,
            0,
            size_of::<libc::kinfo_proc>(),
            &mut count,
        )
    };
    if kprocs.is_null() {
        return;
    }

    for i in 0..count as isize {
        let kproc: &libc::kinfo_proc = unsafe { &*kprocs.offset(i) };

        /* Ignore main threads: a thread (p_tid != -1) whose containing process
        is already tracked either duplicates the main thread (skip) or adds an
        LWP (bump nlwp). ProcessTable_findProcess == the base pid→row map. */
        if kproc.p_tid != -1 {
            if let Some(&idx) = this.super_.super_.table.get(&kproc.p_pid) {
                let cont: &mut dyn Object =
                    this.super_.super_.rows[idx].as_mut().unwrap().as_mut();
                let cont_any: &mut dyn Any = cont;
                let cont_op = cont_any
                    .downcast_mut::<OpenBSDProcess>()
                    .expect("scanProcs: containing row is not an OpenBSDProcess");
                if cont_op.addr == kproc.p_addr {
                    continue;
                }
                cont_op.super_.nlwp += 1;
            }
        }

        let pid = if kproc.p_tid == -1 {
            kproc.p_pid
        } else {
            kproc.p_tid
        };
        let (preExisting, idx) = ProcessTable_getProcess(&mut this.super_, pid, |h| {
            OpenBSDProcess_new(h) as Box<dyn Object>
        });

        // Recover a raw `*mut OpenBSDProcess` for this row (checked borrow
        // ends here). `Object: Any`, so upcast then downcast to the concrete
        // type the row was built as. No further `getProcess` runs this
        // iteration, so `rows` is not reallocated and the pointer stays valid.
        let op: *mut OpenBSDProcess = {
            let obj: &mut dyn Object = this.super_.super_.rows[idx].as_mut().unwrap().as_mut();
            let any: &mut dyn Any = obj;
            any.downcast_mut::<OpenBSDProcess>().unwrap()
        };
        let this_ptr = this as *mut OpenBSDProcessTable;

        // SAFETY: `op` aliases a field inside `*this_ptr`; the fills mutate the
        // process fields and the table's *disjoint* counter fields, mirroring
        // htop's raw `OpenBSDProcess*` / `OpenBSDProcessTable*` pointer graph.
        unsafe {
            if !preExisting {
                let proc = &mut (*op).super_;
                Process_setParent(proc, kproc.p_ppid);
                Process_setThreadGroup(proc, kproc.p_pid);
                proc.tpgid = kproc.p_tpgid;
                proc.session = kproc.p_sid;
                proc.pgrp = kproc.p__pgid;
                proc.isKernelThread = proc.pgrp == 0;
                proc.isUserlandThread = kproc.p_tid != -1;
                proc.starttime_ctime = kproc.p_ustart_sec as i64;
                Process_fillStarttimeBuffer(proc);
                // ProcessTable_add — already done inside getProcess.

                OpenBSDProcessTable_updateProcessName(kd, kproc, proc);

                if cwdFlag {
                    OpenBSDProcessTable_updateCwd(kproc, proc);
                }

                proc.tty_nr = kproc.p_tdev as u64;
                let name_ptr = if (kproc.p_tdev as libc::dev_t) != NODEV {
                    libc::devname(kproc.p_tdev as libc::dev_t, libc::S_IFCHR)
                } else {
                    ptr::null_mut()
                };
                if name_ptr.is_null() {
                    proc.tty_name = None;
                } else {
                    let name = std::ffi::CStr::from_ptr(name_ptr).to_string_lossy().into_owned();
                    if name == "??" {
                        proc.tty_name = None;
                    } else {
                        proc.tty_name = Some(name);
                    }
                }
            } else if updateProcessNames {
                OpenBSDProcessTable_updateProcessName(kd, kproc, &mut (*op).super_);
            }

            (*op).addr = kproc.p_addr;

            let proc = &mut (*op).super_;
            proc.m_virt = kproc.p_vm_dsize as i64 * pageSizeKB;
            proc.m_resident = kproc.p_vm_rssize as i64 * pageSizeKB;

            proc.percent_mem = proc.m_resident as f32 / totalMem as f32 * 100.0;
            let pcpu = getpcpu(&*ohost, kproc) as f32;
            proc.percent_cpu = pcpu.clamp(0.0, activeCPUs as f32 * 100.0);
            Process_updateCPUFieldWidths(proc.percent_cpu);

            proc.nice = kproc.p_nice as i32 - 20;
            proc.time = 100
                * (kproc.p_rtime_sec as u64
                    + ((kproc.p_rtime_usec as u64 + 500000) / 1000000));
            proc.priority = kproc.p_priority as i64 - PZERO;
            proc.processor = kproc.p_cpuid as i32;
            proc.minflt = kproc.p_uru_minflt;
            proc.majflt = kproc.p_uru_majflt;
            proc.nlwp = 1;

            if proc.st_uid != kproc.p_uid {
                proc.st_uid = kproc.p_uid;
                // proc->user = UsersTable_getRef(...) — UsersTable unwired.
            }

            /* p_stat → ProcessState (sys/proc.h L420). */
            proc.state = match kproc.p_stat as i32 {
                SIDL => ProcessState::IDLE,
                SRUN => ProcessState::RUNNABLE,
                SSLEEP => ProcessState::SLEEPING,
                SSTOP => ProcessState::STOPPED,
                SZOMB => ProcessState::ZOMBIE,
                SDEAD => ProcessState::DEFUNCT,
                SONPROC => ProcessState::RUNNING,
                _ => ProcessState::UNKNOWN,
            };

            let isKernel = Process_isKernelThread(proc);
            let isUser = Process_isUserlandThread(proc);
            if isKernel {
                (*this_ptr).super_.kernelThreads += 1;
            } else if isUser {
                (*this_ptr).super_.userlandThreads += 1;
            }

            (*this_ptr).super_.totalTasks += 1;
            if proc.state == ProcessState::RUNNING {
                (*this_ptr).super_.runningTasks += 1;
            }

            proc.super_.show =
                !((hideKernelThreads && isKernel) || (hideUserlandThreads && isUser));
            proc.super_.updated = true;
        }
    }
}

/// Port of `void ProcessTable_goThroughEntries(ProcessTable* super)` from
/// `OpenBSDProcessTable.c:242`.
pub fn ProcessTable_goThroughEntries(this: &mut OpenBSDProcessTable) {
    OpenBSDProcessTable_scanProcs(this);
}
