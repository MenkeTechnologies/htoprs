//! Port of `NetBSDProcessTable.c` — the NetBSD process table.
//!
//! Ported struct model:
//! - the [`kinfo_proc2`] and [`kinfo_lwp`] structs (`sys/sysctl.h`),
//!   transcribed `#[repr(C)]` field-for-field because `libc` does not model
//!   them; `kvm_getproc2`/`kvm_getlwps` memcpy into these, so the offsets must
//!   be exact.
//! - the [`NetBSDProcessTable`] struct (`NetBSDProcessTable.h:20`) plus the
//!   `TableClass` scan vtable wiring.
//!
//! Ported functions:
//! - [`ProcessTable_new`] (`NetBSDProcessTable.c:40`)
//! - [`NetBSDProcessTable_updateExe`] (`NetBSDProcessTable.c:56`)
//! - [`NetBSDProcessTable_updateCwd`] (`NetBSDProcessTable.c:74`)
//! - [`NetBSDProcessTable_updateProcessName`] (`NetBSDProcessTable.c:94`)
//! - [`getpcpu`] (`NetBSDProcessTable.c:146`)
//! - [`get_active_status`] (`NetBSDProcessTable.c:153`)
//! - [`ProcessTable_goThroughEntries`] (`NetBSDProcessTable.c:171`)
//!
//! Still `todo!()`:
//! - `ProcessTable_delete` (`NetBSDProcessTable.c:50`) is a pure `free()`
//!   teardown — `ProcessTable_done(&this->super)` then `free(this)`; Rust
//!   `Drop` reclaims the owned fields (darwin/openbsd precedent).
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)] // faithful C global name (NetBSDProcessTable_class)
#![allow(dead_code)]

use core::slice;
use std::ffi::CStr;
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_ulong, c_void};
use std::ptr;

use crate::ported::machine::Machine;
use crate::ported::netbsd::netbsdmachine::NetBSDMachine;
use crate::ported::netbsd::netbsdprocess::NetBSDProcess_new;
use crate::ported::object::Object;
use crate::ported::process::{
    Process, ProcessState, Process_fillStarttimeBuffer, Process_getPid, Process_getThreadGroup,
    Process_isKernelThread, Process_isUserlandThread, Process_setParent, Process_setPid,
    Process_setThreadGroup, Process_updateCPUFieldWidths, Process_updateCmdline, Process_updateComm,
    Process_updateExe, PROCESS_FLAG_CWD,
};
use crate::ported::processtable::{
    ProcessTable, ProcessTable_cleanupEntries, ProcessTable_getProcess, ProcessTable_init,
    ProcessTable_prepareEntries,
};
use crate::ported::table::{Table, TableClass};

// ── `kinfo_proc2` / `kinfo_lwp` field-count macros (`sys/sysctl.h`).

/// `#define KI_NGROUPS 16`.
const KI_NGROUPS: usize = 16;
/// `#define KI_MAXCOMLEN 24`.
const KI_MAXCOMLEN: usize = 24;
/// `#define KI_WMESGLEN 8`.
const KI_WMESGLEN: usize = 8;
/// `#define KI_MAXLOGNAME 24`.
const KI_MAXLOGNAME: usize = 24;
/// `#define KI_MAXEMULLEN 16`.
const KI_MAXEMULLEN: usize = 16;
/// `#define KI_LNAMELEN 20`.
const KI_LNAMELEN: usize = 20;

// ── NetBSD identifiers absent from `libc` for this target.

/// `KERN_PROC_CWD` (`sys/sysctl.h`) — the `KERN_PROC_ARGS` subcommand for a
/// process's current working directory.
const KERN_PROC_CWD: c_int = 6;
/// `PZERO` (`sys/param.h`) — the base priority offset htop subtracts.
const PZERO: i64 = 22;
/// `P_SYSTEM` (`sys/proc.h`, aliased to `PK_SYSTEM`) — the kernel-thread flag
/// in `p_flag`.
const P_SYSTEM: i32 = 0x0000_0002;

// Process `p_realstat` values (`sys/proc.h`).
/// `SIDL` — process being created.
const SIDL: u64 = 1;
/// `SACTIVE` — runnable/sleeping.
const SACTIVE: u64 = 2;
/// `SSTOP` — stopped.
const SSTOP: u64 = 4;
/// `SZOMB` — zombie.
const SZOMB: u64 = 5;
/// `SDEAD` — dead/dying.
const SDEAD: u64 = 6;

/// `typedef struct { uint32_t __bits[4]; } ki_sigset_t;` (`sys/sysctl.h`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ki_sigset_t {
    pub __bits: [u32; 4],
}

extern "C" {
    /// `struct kinfo_proc2* kvm_getproc2(kvm_t*, int op, int arg, size_t
    /// elemsize, int* cnt)` (`kvm.h`). Not exposed by `libc`.
    fn kvm_getproc2(
        kd: *mut c_void,
        op: c_int,
        arg: c_int,
        elemsize: usize,
        cnt: *mut c_int,
    ) -> *const kinfo_proc2;

    /// `struct kinfo_lwp* kvm_getlwps(kvm_t*, int pid, u_long paddr, size_t
    /// elemsize, int* cnt)` (`kvm.h`). Not exposed by `libc`.
    fn kvm_getlwps(
        kd: *mut c_void,
        pid: c_int,
        paddr: c_ulong,
        elemsize: usize,
        cnt: *mut c_int,
    ) -> *const kinfo_lwp;

    /// `char** kvm_getargv2(kvm_t*, const struct kinfo_proc2*, int nchr)`
    /// (`kvm.h`). Not exposed by `libc`.
    fn kvm_getargv2(kd: *mut c_void, p: *const kinfo_proc2, nchr: c_int) -> *const *const c_char;
}

/// Port of `struct kinfo_proc2` (`sys/sysctl.h`) — the per-process snapshot
/// `kvm_getproc2` fills. Transcribed field-for-field (`uint64_t` → `u64`,
/// `int32_t` → `i32`, …) so every offset matches the kernel's memcpy.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct kinfo_proc2 {
    pub p_forw: u64,
    pub p_back: u64,
    pub p_paddr: u64,
    pub p_addr: u64,
    pub p_fd: u64,
    pub p_cwdi: u64,
    pub p_stats: u64,
    pub p_limit: u64,
    pub p_vmspace: u64,
    pub p_sigacts: u64,
    pub p_sess: u64,
    pub p_tsess: u64,
    pub p_ru: u64,
    pub p_eflag: i32,
    pub p_exitsig: i32,
    pub p_flag: i32,
    pub p_pid: i32,
    pub p_ppid: i32,
    pub p_sid: i32,
    pub p__pgid: i32,
    pub p_tpgid: i32,
    pub p_uid: u32,
    pub p_ruid: u32,
    pub p_gid: u32,
    pub p_rgid: u32,
    pub p_groups: [u32; KI_NGROUPS],
    pub p_ngroups: i16,
    pub p_jobc: i16,
    pub p_tdev: u32,
    pub p_estcpu: u32,
    pub p_rtime_sec: u32,
    pub p_rtime_usec: u32,
    pub p_cpticks: i32,
    pub p_pctcpu: u32,
    pub p_swtime: u32,
    pub p_slptime: u32,
    pub p_schedflags: i32,
    pub p_uticks: u64,
    pub p_sticks: u64,
    pub p_iticks: u64,
    pub p_tracep: u64,
    pub p_traceflag: i32,
    pub p_holdcnt: i32,
    pub p_siglist: ki_sigset_t,
    pub p_sigmask: ki_sigset_t,
    pub p_sigignore: ki_sigset_t,
    pub p_sigcatch: ki_sigset_t,
    pub p_stat: i8,
    pub p_priority: u8,
    pub p_usrpri: u8,
    pub p_nice: u8,
    pub p_xstat: u16,
    pub p_acflag: u16,
    pub p_comm: [c_char; KI_MAXCOMLEN],
    pub p_wmesg: [c_char; KI_WMESGLEN],
    pub p_wchan: u64,
    pub p_login: [c_char; KI_MAXLOGNAME],
    pub p_vm_rssize: i32,
    pub p_vm_tsize: i32,
    pub p_vm_dsize: i32,
    pub p_vm_ssize: i32,
    pub p_uvalid: i64,
    pub p_ustart_sec: u32,
    pub p_ustart_usec: u32,
    pub p_uutime_sec: u32,
    pub p_uutime_usec: u32,
    pub p_ustime_sec: u32,
    pub p_ustime_usec: u32,
    pub p_uru_maxrss: u64,
    pub p_uru_ixrss: u64,
    pub p_uru_idrss: u64,
    pub p_uru_isrss: u64,
    pub p_uru_minflt: u64,
    pub p_uru_majflt: u64,
    pub p_uru_nswap: u64,
    pub p_uru_inblock: u64,
    pub p_uru_oublock: u64,
    pub p_uru_msgsnd: u64,
    pub p_uru_msgrcv: u64,
    pub p_uru_nsignals: u64,
    pub p_uru_nvcsw: u64,
    pub p_uru_nivcsw: u64,
    pub p_uctime_sec: u32,
    pub p_uctime_usec: u32,
    pub p_cpuid: u64,
    pub p_realflag: u64,
    pub p_nlwps: u64,
    pub p_nrlwps: u64,
    pub p_realstat: u64,
    pub p_svuid: u32,
    pub p_svgid: u32,
    pub p_ename: [c_char; KI_MAXEMULLEN],
    pub p_vm_vsize: i64,
    pub p_vm_msize: i64,
}

/// Port of `struct kinfo_lwp` (`sys/sysctl.h`) — the per-LWP snapshot
/// `kvm_getlwps` fills. Only `l_stat` is read by htop, but the full layout is
/// transcribed for an exact ABI match.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct kinfo_lwp {
    pub l_forw: u64,
    pub l_back: u64,
    pub l_laddr: u64,
    pub l_addr: u64,
    pub l_lid: i32,
    pub l_flag: i32,
    pub l_swtime: u32,
    pub l_slptime: u32,
    pub l_schedflags: i32,
    pub l_holdcnt: i32,
    pub l_priority: u8,
    pub l_usrpri: u8,
    pub l_stat: i8,
    pub l_pad1: i8,
    pub l_pad2: i32,
    pub l_wmesg: [c_char; KI_WMESGLEN],
    pub l_wchan: u64,
    pub l_cpuid: u64,
    pub l_rtime_sec: u32,
    pub l_rtime_usec: u32,
    pub l_cpticks: u32,
    pub l_pctcpu: u32,
    pub l_pid: u32,
    pub l_name: [c_char; KI_LNAMELEN],
}

/// Port of `typedef struct NetBSDProcessTable_` (`NetBSDProcessTable.h:20`) —
/// just an embedded `ProcessTable super`. `#[repr(C)]` keeps `super_` (whose
/// `super_: Table` is likewise at offset 0) at offset 0, so the `*mut Table` →
/// `*mut NetBSDProcessTable` cast in the scan vtable is sound.
#[repr(C)]
pub struct NetBSDProcessTable {
    /// C `ProcessTable super`.
    pub super_: ProcessTable,
}

/// Port of `static void NetBSDProcessTable_updateExe(const struct kinfo_proc2*
/// kproc, Process* proc)` from `NetBSDProcessTable.c:56`. Reads the executable
/// path via `sysctl(KERN_PROC_ARGS, KERN_PROC_PATHNAME)`; a sysctl failure or
/// an empty buffer (kernel threads) clears the exe.
pub fn NetBSDProcessTable_updateExe(kproc: &kinfo_proc2, proc: &mut Process) {
    let mib: [c_int; 4] = [
        libc::CTL_KERN,
        libc::KERN_PROC_ARGS,
        kproc.p_pid,
        libc::KERN_PROC_PATHNAME,
    ];
    let mut buffer = [0u8; 2048];
    let mut size = buffer.len();
    if unsafe {
        libc::sysctl(
            mib.as_ptr(),
            4,
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
            ptr::null(),
            0,
        )
    } != 0
    {
        Process_updateExe(proc, None);
        return;
    }

    /* Kernel threads return an empty buffer */
    if buffer[0] == 0 {
        Process_updateExe(proc, None);
        return;
    }

    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    let exe = String::from_utf8_lossy(&buffer[..end]);
    Process_updateExe(proc, Some(&exe));
}

/// Port of `static void NetBSDProcessTable_updateCwd(const struct kinfo_proc2*
/// kproc, Process* proc)` from `NetBSDProcessTable.c:74`. Reads the cwd via
/// `sysctl(KERN_PROC_ARGS, KERN_PROC_CWD)`; a failure or empty buffer clears
/// `procCwd`.
pub fn NetBSDProcessTable_updateCwd(kproc: &kinfo_proc2, proc: &mut Process) {
    let mib: [c_int; 4] = [
        libc::CTL_KERN,
        libc::KERN_PROC_ARGS,
        kproc.p_pid,
        KERN_PROC_CWD,
    ];
    let mut buffer = [0u8; 2048];
    let mut size = buffer.len();
    if unsafe {
        libc::sysctl(
            mib.as_ptr(),
            4,
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
            ptr::null(),
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

    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    proc.procCwd = Some(String::from_utf8_lossy(&buffer[..end]).into_owned());
}

/// Port of `static void NetBSDProcessTable_updateProcessName(kvm_t* kd, const
/// struct kinfo_proc2* kproc, Process* proc)` from `NetBSDProcessTable.c:94`.
/// Sets the short command (`p_comm`) and reconstructs the full command line
/// from `kvm_getargv2`, joining the args with spaces and recording the end of
/// `argv[0]` as the basename boundary (with the same `':'`/`'\\'` heuristics
/// as the C). Any failure falls back to `p_comm`.
pub fn NetBSDProcessTable_updateProcessName(
    kd: *mut c_void,
    kproc: &kinfo_proc2,
    proc: &mut Process,
) {
    // Read a NUL-terminated fixed C char buffer into an owned lossy String (the
    // C treats `p_comm` as a C string inline); nested so it stays a faithful
    // translation without a module-level non-C function.
    fn c_field_to_string(buf: &[c_char]) -> String {
        let bytes: &[u8] = unsafe { slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len()) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }
    let comm = c_field_to_string(&kproc.p_comm);
    Process_updateComm(proc, Some(&comm));

    /*
     * Like NetBSD's top(1), we try to fall back to the command name
     * (argv[0]) if we fail to construct the full command.
     */
    let arg = unsafe { kvm_getargv2(kd, kproc as *const kinfo_proc2, 500) };
    if arg.is_null() || unsafe { (*arg).is_null() } {
        Process_updateCmdline(proc, Some(&comm), 0, comm.len());
        return;
    }

    // Collect the argument C strings.
    let mut args: Vec<&[u8]> = Vec::new();
    let mut i = 0isize;
    loop {
        let p = unsafe { *arg.offset(i) };
        if p.is_null() {
            break;
        }
        args.push(unsafe { CStr::from_ptr(p) }.to_bytes());
        i += 1;
    }

    // Join argv with trailing spaces (mirrors the strlcat loop).
    let mut s = String::new();
    let mut end = 0usize;
    for (idx, a) in args.iter().enumerate() {
        s.push_str(&String::from_utf8_lossy(a));
        if idx == 0 {
            // end = MINIMUM(strlen(arg[0]), len - 1) == strlen(arg[0]).
            end = a.len();
            /* check if cmdline ended earlier, e.g 'kdeinit5: Running...' */
            let byte_at = |j: usize| -> u8 {
                if j < a.len() {
                    a[j]
                } else {
                    0
                }
            };
            let mut j = end;
            while j > 0 {
                if byte_at(j) == b' ' && byte_at(j - 1) != b'\\' {
                    end = if byte_at(j - 1) == b':' { j - 1 } else { j };
                }
                j -= 1;
            }
        }
        /* the trailing space should get truncated anyway */
        s.push(' ');
    }

    Process_updateCmdline(proc, Some(&s), 0, end);
}

/*
 * Borrowed with modifications from NetBSD's top(1).
 */
/// Port of `static double getpcpu(const NetBSDMachine* nhost, const struct
/// kinfo_proc2* kp)` from `NetBSDProcessTable.c:146`.
pub fn getpcpu(nhost: &NetBSDMachine, kp: &kinfo_proc2) -> f64 {
    if nhost.fscale == 0 {
        return 0.0;
    }

    100.0 * kp.p_pctcpu as f64 / nhost.fscale as f64
}

/// Port of `static ProcessState get_active_status(const NetBSDMachine* nhost,
/// const struct kinfo_proc2* kproc)` from `NetBSDProcessTable.c:153`. Reads the
/// process's LWPs via `kvm_getlwps` and maps the first LWP with a recognized
/// state.
pub fn get_active_status(nhost: &NetBSDMachine, kproc: &kinfo_proc2) -> ProcessState {
    let mut nlwps: c_int = 0;
    let klwps = unsafe {
        kvm_getlwps(
            nhost.kd,
            kproc.p_pid,
            kproc.p_paddr as c_ulong,
            size_of::<kinfo_lwp>(),
            &mut nlwps,
        )
    };
    if klwps.is_null() {
        return ProcessState::UNKNOWN;
    }
    // We only consider the first LWP that has one of the states below.
    let lwps = unsafe { slice::from_raw_parts(klwps, nlwps.max(0) as usize) };
    for lwp in lwps {
        match lwp.l_stat as c_int {
            libc::LSONPROC => return ProcessState::RUNNING,
            libc::LSRUN => return ProcessState::RUNNABLE,
            libc::LSSLEEP => return ProcessState::SLEEPING,
            libc::LSSTOP => return ProcessState::STOPPED,
            _ => {}
        }
    }
    ProcessState::UNKNOWN
}

/// Port of `ProcessTable_goThroughEntries(ProcessTable* super)` from
/// `NetBSDProcessTable.c:171`. The NetBSD process scan: walks the
/// `kvm_getproc2(KERN_PROC_ALL)` snapshot, finding-or-creating each process
/// and filling it from its `kinfo_proc2`.
///
/// Deviation (documented, not silent): `proc->user = UsersTable_getRef(...)`
/// is skipped — the `UsersTable` is unported — while `st_uid` is still
/// tracked, matching the darwin scan.
pub fn ProcessTable_goThroughEntries(this: &mut NetBSDProcessTable) {
    let host = this.super_.super_.host;
    let nhost = host as *const NetBSDMachine;

    // Cache the host/settings scalars the loop reads (host outlives the scan).
    let (host_total_mem, host_active_cpus) = unsafe { ((*host).totalMem, (*host).activeCPUs) };
    let (hide_kernel, hide_userland, update_names, cwd_flag) = unsafe {
        let settings = (*host).settings.as_ref().expect("host settings unset");
        let ss_flags = settings.screens[settings.ssIndex as usize].flags;
        (
            settings.hideKernelThreads,
            settings.hideUserlandThreads,
            settings.updateProcessNames,
            ss_flags & PROCESS_FLAG_CWD != 0,
        )
    };
    let (nhost_kd, nhost_page_kb) = unsafe { ((*nhost).kd, (*nhost).pageSizeKB) };

    let mut count: c_int = 0;
    let kprocs = unsafe {
        kvm_getproc2(
            nhost_kd,
            libc::KERN_PROC_ALL,
            0,
            size_of::<kinfo_proc2>(),
            &mut count,
        )
    };
    if kprocs.is_null() {
        return;
    }
    let kprocs = unsafe { slice::from_raw_parts(kprocs, count.max(0) as usize) };

    for kproc in kprocs {
        let (pre_existing, idx) =
            ProcessTable_getProcess(&mut this.super_, kproc.p_pid, |h| {
                NetBSDProcess_new(h) as Box<dyn Object>
            });

        // Recover a raw `*mut Process` for this row via a checked borrow.
        let proc_ptr: *mut Process = {
            let obj: &mut dyn Object = this.super_.super_.rows[idx].as_mut().unwrap().as_mut();
            obj.as_process_mut().unwrap() as *mut Process
        };

        let this_ptr = this as *mut NetBSDProcessTable;

        // SAFETY: `proc_ptr` aliases a field inside a row of `*this_ptr`; the
        // fill calls mutate the process fields and the table's *disjoint*
        // counter fields, never the same memory, mirroring htop's raw
        // `Process*` / `ProcessTable*` graph. `rows` is not reallocated
        // between deriving `proc_ptr` and using it (no further `getProcess`
        // this iteration).
        unsafe {
            let proc = &mut *proc_ptr;

            proc.super_.show = !((hide_kernel && Process_isKernelThread(proc))
                || (hide_userland && Process_isUserlandThread(proc)));

            if !pre_existing {
                Process_setPid(proc, kproc.p_pid);
                Process_setParent(proc, kproc.p_ppid);
                Process_setThreadGroup(proc, kproc.p_pid);
                proc.tpgid = kproc.p_tpgid;
                proc.session = kproc.p_sid;
                proc.pgrp = kproc.p__pgid;
                proc.isKernelThread = (kproc.p_flag & P_SYSTEM) != 0;
                // eh? — pid != tgid
                proc.isUserlandThread = Process_getPid(proc) != Process_getThreadGroup(proc);
                proc.starttime_ctime = kproc.p_ustart_sec as i64;
                Process_fillStarttimeBuffer(proc);
                // ProcessTable_add is performed inside ProcessTable_getProcess
                // (the Rust deviation), so it is not repeated here.

                proc.tty_nr = kproc.p_tdev as u64;
                // KERN_PROC_TTY_NODEV is NODEV (all-ones dev_t); a real tty
                // has p_tdev != NODEV.
                let name: *mut c_char = if kproc.p_tdev != u32::MAX {
                    libc::devname(kproc.p_tdev as libc::dev_t, libc::S_IFCHR)
                } else {
                    ptr::null_mut()
                };
                if name.is_null() {
                    proc.tty_name = None;
                } else {
                    proc.tty_name =
                        Some(CStr::from_ptr(name).to_string_lossy().into_owned());
                }

                NetBSDProcessTable_updateExe(kproc, proc);
                NetBSDProcessTable_updateProcessName(nhost_kd, kproc, proc);
            } else if update_names {
                NetBSDProcessTable_updateProcessName(nhost_kd, kproc, proc);
            }

            if cwd_flag {
                NetBSDProcessTable_updateCwd(kproc, proc);
            }

            if proc.st_uid != kproc.p_uid {
                proc.st_uid = kproc.p_uid;
                // proc->user = UsersTable_getRef(...) — UsersTable unported.
            }

            proc.m_virt = kproc.p_vm_vsize;
            proc.m_resident = kproc.p_vm_rssize as i64;

            proc.percent_mem = ((proc.m_resident as f64 * nhost_page_kb as f64)
                / host_total_mem as f64
                * 100.0) as f32;
            proc.percent_cpu = getpcpu(&*nhost, kproc)
                .clamp(0.0, host_active_cpus as f64 * 100.0) as f32;
            Process_updateCPUFieldWidths(proc.percent_cpu);

            proc.nlwp = kproc.p_nlwps as i64;
            proc.nice = kproc.p_nice as i32 - 20;
            proc.time = 100
                * (kproc.p_rtime_sec as u64
                    + ((kproc.p_rtime_usec as u64 + 500_000) / 1_000_000));
            proc.priority = kproc.p_priority as i64 - PZERO;
            proc.processor = kproc.p_cpuid as i32;
            proc.minflt = kproc.p_uru_minflt;
            proc.majflt = kproc.p_uru_majflt;

            /* TODO: According to NetBSD proc.h, SDYING should be a regarded state */
            proc.state = match kproc.p_realstat {
                SIDL => ProcessState::IDLE,
                SACTIVE => get_active_status(&*nhost, kproc),
                SSTOP => ProcessState::STOPPED,
                SZOMB => ProcessState::ZOMBIE,
                SDEAD => ProcessState::DEFUNCT,
                _ => ProcessState::UNKNOWN,
            };

            if Process_isKernelThread(proc) {
                (*this_ptr).super_.kernelThreads += 1;
            } else if Process_isUserlandThread(proc) {
                (*this_ptr).super_.userlandThreads += 1;
            }

            (*this_ptr).super_.totalTasks += 1;
            if proc.state == ProcessState::RUNNING {
                (*this_ptr).super_.runningTasks += 1;
            }
            proc.super_.updated = true;
        }
    }
}

/// The `TableClass` scan-vtable slots for the NetBSD process table — the
/// `Table*`-downcasting glue the C `ProcessTable_class` (`ProcessTable.c:94`)
/// stores as `.prepare`/`.iterate`/`.cleanup`, re-expressed against the
/// `#[repr(C)]` `NetBSDProcessTable` layout (`super_: ProcessTable` at offset
/// 0, whose `super_: Table` is likewise at offset 0).
impl NetBSDProcessTable {
    /// C `ProcessTable_class.prepare` — downcast then delegate to the base
    /// [`ProcessTable_prepareEntries`].
    ///
    /// # Safety precondition
    /// `super_` is the base of a live `NetBSDProcessTable`.
    fn scan_prepare(super_: *mut Table) {
        let this = super_ as *mut NetBSDProcessTable;
        // SAFETY: `super_` is the base of a live `NetBSDProcessTable`.
        ProcessTable_prepareEntries(unsafe { &mut (*this).super_ });
    }

    /// C `ProcessTable_class.iterate` — dispatch to the NetBSD
    /// [`ProcessTable_goThroughEntries`] (the platform symbol C link-resolves).
    ///
    /// # Safety precondition
    /// `super_` is the base of a live `NetBSDProcessTable`.
    fn scan_iterate(super_: *mut Table) {
        let this = super_ as *mut NetBSDProcessTable;
        // SAFETY: `super_` is the base of a live `NetBSDProcessTable`.
        ProcessTable_goThroughEntries(unsafe { &mut *this });
    }

    /// C `ProcessTable_class.cleanup` — downcast then delegate to the base
    /// [`ProcessTable_cleanupEntries`].
    ///
    /// # Safety precondition
    /// `super_` is the base of a live `NetBSDProcessTable`.
    fn scan_cleanup(super_: *mut Table) {
        let this = super_ as *mut NetBSDProcessTable;
        // SAFETY: `super_` is the base of a live `NetBSDProcessTable`.
        ProcessTable_cleanupEntries(unsafe { &mut (*this).super_ });
    }
}

/// Port of `const TableClass ProcessTable_class` (`ProcessTable.c:94`), the
/// class the NetBSD `NetBSDProcessTable` runs under. Only the scan-vtable half
/// is modeled (see [`TableClass`]); the `ObjectClass super` is class identity
/// in Rust.
pub static NetBSDProcessTable_class: TableClass = TableClass {
    prepare: Some(NetBSDProcessTable::scan_prepare),
    iterate: Some(NetBSDProcessTable::scan_iterate),
    cleanup: Some(NetBSDProcessTable::scan_cleanup),
};

/// Port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` from `NetBSDProcessTable.c:40`. Allocates and inits the
/// `NetBSDProcessTable`, wiring the scan vtable so `Machine_scanTables` can
/// dispatch prepare/iterate/cleanup through it.
pub fn ProcessTable_new(host: *const Machine, pidMatchList: Option<usize>) -> Box<NetBSDProcessTable> {
    let mut this = Box::new(NetBSDProcessTable {
        super_: ProcessTable::empty(),
    });

    ProcessTable_init(&mut this.super_, host, pidMatchList);

    this.super_.super_.klass = &NetBSDProcessTable_class as *const TableClass;

    this
}

/// TODO: port of `void ProcessTable_delete(Object* cast)` from
/// `NetBSDProcessTable.c:50`. Kept stubbed: the C body is a pure teardown —
/// `ProcessTable_done(&this->super)` then `free(this)`; Rust `Drop` reclaims
/// the owned fields (darwin/openbsd `ProcessTable_delete` precedent).
pub fn ProcessTable_delete() {
    todo!("port of NetBSDProcessTable.c:50 — pure free() teardown; Rust Drop handles it")
}
