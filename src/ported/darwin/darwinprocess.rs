//! Partial port of `DarwinProcess.c` — the Darwin process object.
//!
//! Ported (self-contained, on the base [`Process`] + [`Process_init`]):
//! - the [`DarwinProcess`] object struct (`Process super_` + `utime`/`stime`/
//!   `taskAccess`/`translated`).
//! - [`DarwinProcess_new`] (`DarwinProcess.c:57`).
//!
//! The remaining `pub fn`s are honest `todo!()` placeholders named after
//! their C counterparts, blocked on unported substrate:
//! - `Process_delete` / `DarwinProcess_rowWriteField` / `_compareByKey`
//!   need the `Object_setClass` / `ProcessClass` vtable wiring.
//! - the `kinfo_proc` struct (absent from `libc`), required by
//!   `DarwinProcess_setFromKInfoProc` / `_updateCmdLine`.
//! - `Process_fillStarttimeBuffer` (stub in `process.rs`) and
//!   `ProcessTable_getProcess` (stub in `processtable.rs`), required by
//!   `setFromKInfoProc` and `scanThreads`.
//! - the `DarwinMachine` struct (`darwinmachine.rs`), read by
//!   `setFromLibprocPidinfo` for `host_info.max_mem`.
//!
//! The `Process_fields[]` field-descriptor table (`DarwinProcess.c:24`) is
//! data, not a function, and is deferred until the Darwin `ProcessField`
//! layer is modeled. `gen_port_report.py` counts these `todo!()` bodies as
//! *stubbed*, not *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::mem::{size_of, size_of_val, zeroed};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::ported::darwin::darwinmachine::DarwinMachine;
use crate::ported::darwin::darwinprocesstable::{kinfo_proc, DarwinProcessTable};
use crate::ported::darwin::platform::Platform_machTicksToNanoseconds;
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::process::{
    Process, ProcessClass, ProcessState, Process_fillStarttimeBuffer, Process_getPid, Process_init,
    Process_setParent, Process_setPid, Process_setThreadGroup, Process_updateCPUFieldWidths,
    Process_updateCmdline, Process_updateComm, Process_updateExe, PROCESS_FLAG_CWD,
};
use crate::ported::richstring::RichString;
use crate::ported::row::{Row, RowClass};

/// Port of htop's `struct DarwinProcess_` (`DarwinProcess.h:18`). "Extends"
/// [`Process`] via the embedded `super_` field (htop's `Process super;`
/// first member); the remaining fields are the Darwin-only per-process
/// accumulators.
///
/// `#[repr(C)]` guarantees `super_` sits at offset 0, so htop's
/// `(DarwinProcess*)processPtr` downcast — a `*const Process` obtained from
/// a `DarwinProcess` allocation, cast back — is sound (see the layout test).
#[repr(C)]
pub struct DarwinProcess {
    /// C `Process super` — the embedded base process.
    pub super_: Process,
    /// C `uint64_t utime`.
    pub utime: u64,
    /// C `uint64_t stime`.
    pub stime: u64,
    /// C `bool taskAccess`.
    pub taskAccess: bool,
    /// C `bool translated`.
    pub translated: bool,
}

/// `DarwinProcess` "is a" `Object` (via `Process` via `Row`). The class /
/// display / compare slots delegate to the embedded [`Process`] (the
/// `DarwinProcess_class` vtable overrides no base slots that are ported
/// yet), while the base-view accessors expose this object's embedded
/// [`Row`]/[`Process`] — the mechanism a [`Table`](crate::ported::table::Table)
/// of `Box<dyn Object>` rows uses to recover them.
impl Object for DarwinProcess {
    fn klass(&self) -> &'static ObjectClass {
        self.super_.klass()
    }

    fn display(&self, out: &mut RichString) {
        self.super_.display(out)
    }

    fn compare(&self, other: &dyn Object) -> i32 {
        self.super_.compare(other)
    }

    fn row_class(&self) -> Option<&'static RowClass> {
        self.super_.row_class()
    }

    fn process_class(&self) -> Option<&'static ProcessClass> {
        self.super_.process_class()
    }

    fn as_row(&self) -> Option<&Row> {
        Some(&self.super_.super_)
    }

    fn as_row_mut(&mut self) -> Option<&mut Row> {
        Some(&mut self.super_.super_)
    }

    fn as_process(&self) -> Option<&Process> {
        Some(&self.super_)
    }

    fn as_process_mut(&mut self) -> Option<&mut Process> {
        Some(&mut self.super_)
    }
}

/// Port of `Process* DarwinProcess_new(const Machine* host)` from
/// `DarwinProcess.c:57`. C `xCalloc`s a `DarwinProcess`, sets its class,
/// runs `Process_init` on the embedded base, then sets the Darwin fields
/// (`utime`/`stime` zero, `taskAccess` true, `translated` false) and the
/// base state to `UNKNOWN`, returning `(Process*)this`.
///
/// The returned `Box<DarwinProcess>` is the owner (C's heap allocation);
/// `&mut box.super_` is the `*mut Process`. `Object_setClass` /
/// `Class(DarwinProcess)` are dropped — class identity is the Rust type.
pub fn DarwinProcess_new(host: *const Machine) -> Box<DarwinProcess> {
    let mut this = Box::new(DarwinProcess {
        super_: Process::default(),
        utime: 0,
        stime: 0,
        taskAccess: true,
        translated: false,
    });

    Process_init(&mut this.super_, host as *const c_void);
    this.super_.state = ProcessState::UNKNOWN;

    this
}

/// TODO: port of `void Process_delete(Object* cast` from `DarwinProcess.c:71`.
pub fn Process_delete() {
    todo!("port of DarwinProcess.c:71")
}

/// TODO: port of `static void DarwinProcess_rowWriteField(const Row* super, RichString* str, ProcessField field` from `DarwinProcess.c:78`.
pub fn DarwinProcess_rowWriteField() {
    todo!("port of DarwinProcess.c:78")
}

/// TODO: port of `static int DarwinProcess_compareByKey(const Process* v1, const Process* v2, ProcessField key` from `DarwinProcess.c:96`.
pub fn DarwinProcess_compareByKey() {
    todo!("port of DarwinProcess.c:96")
}

/// Port of `static void DarwinProcess_updateExe(pid_t pid, Process* proc)`
/// from `DarwinProcess.c:109`. Reads the executable path via
/// `proc_pidpath(2)` and hands it to [`Process_updateExe`]; on failure
/// (`r <= 0`) leaves the process unchanged. `proc_pidpath` returns the path
/// length, so the string is `path[..r]` (NUL-terminated in the buffer).
pub fn DarwinProcess_updateExe(pid: libc::pid_t, proc: &mut Process) {
    let mut path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];

    let r = unsafe {
        libc::proc_pidpath(pid, path.as_mut_ptr() as *mut c_void, path.len() as u32)
    };
    if r <= 0 {
        return;
    }

    let exe = String::from_utf8_lossy(&path[..r as usize]);
    Process_updateExe(proc, Some(&exe));
}

/// Port of `static void DarwinProcess_updateCwd(pid_t pid, Process* proc)`
/// from `DarwinProcess.c:119`. Reads the current working directory via
/// `proc_pidinfo(PROC_PIDVNODEPATHINFO)`; on failure or an empty path it
/// clears `procCwd`, otherwise stores the vnode path. `libc` models
/// `vip_path` as a flat `MAXPATHLEN` buffer (`[[c_char; 32]; 32]`), so it is
/// read as one NUL-terminated byte run.
pub fn DarwinProcess_updateCwd(pid: libc::pid_t, proc: &mut Process) {
    let mut vpi: libc::proc_vnodepathinfo = unsafe { zeroed() };

    let r = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            &mut vpi as *mut libc::proc_vnodepathinfo as *mut c_void,
            size_of::<libc::proc_vnodepathinfo>() as c_int,
        )
    };
    if r <= 0 {
        proc.procCwd = None;
        return;
    }

    // `vpi.pvi_cdir.vip_path` is a contiguous MAXPATHLEN buffer.
    let path: &[u8] = unsafe {
        core::slice::from_raw_parts(
            vpi.pvi_cdir.vip_path.as_ptr() as *const u8,
            size_of_val(&vpi.pvi_cdir.vip_path),
        )
    };
    if path[0] == 0 {
        proc.procCwd = None;
        return;
    }

    let end = path.iter().position(|&b| b == 0).unwrap_or(path.len());
    proc.procCwd = Some(String::from_utf8_lossy(&path[..end]).into_owned());
}

/// Port of `static void DarwinProcess_updateCmdLine(const struct kinfo_proc*
/// k, Process* proc)` from `DarwinProcess.c:138`. Sets the short command
/// (`p_comm`) and then reconstructs the full command line from the process's
/// raw argument space (`sysctl(KERN_PROCARGS2)`): after the `argc`/exec_path
/// header it joins `argv[0..argc]` with spaces, recording the end of
/// `argv[0]` as the basename boundary. Any failure falls back to `p_comm`
/// (the C `ERROR_A`/`ERROR_B` paths). The C mutates the buffer in place
/// (inter-arg NULs → spaces); the owned `Vec` here does the same.
pub fn DarwinProcess_updateCmdLine(k: &kinfo_proc, proc: &mut Process) {
    let pid = k.kp_proc.p_pid;

    // Process_updateComm(proc, k->kp_proc.p_comm)
    let comm_field: &[u8] = unsafe {
        core::slice::from_raw_parts(
            k.kp_proc.p_comm.as_ptr() as *const u8,
            k.kp_proc.p_comm.len(),
        )
    };
    let comm_len = comm_field.iter().position(|&b| b == 0).unwrap_or(comm_field.len());
    let comm = String::from_utf8_lossy(&comm_field[..comm_len]).into_owned();
    Process_updateComm(proc, Some(&comm));

    // Parse the full argument vector out of KERN_PROCARGS2. Any failure
    // returns None and falls through to the short p_comm below.
    let parsed = (|| -> Option<(String, usize)> {
        // Maximum argument space size.
        let mut argmax: c_int = 0;
        let mut sz = size_of::<c_int>();
        let mut mib_max = [libc::CTL_KERN, libc::KERN_ARGMAX];
        if unsafe {
            libc::sysctl(
                mib_max.as_mut_ptr(),
                2,
                &mut argmax as *mut c_int as *mut c_void,
                &mut sz,
                ptr::null_mut(),
                0,
            )
        } == -1
            || argmax <= 0
        {
            return None;
        }

        // Raw argument space of the process.
        let mut procargs = vec![0u8; argmax as usize];
        let mut size = argmax as usize;
        let mut mib_args = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        if unsafe {
            libc::sysctl(
                mib_args.as_mut_ptr(),
                3,
                procargs.as_mut_ptr() as *mut c_void,
                &mut size,
                ptr::null_mut(),
                0,
            )
        } == -1
        {
            return None;
        }
        procargs.truncate(size);

        // Layout: [int argc][exec_path\0][\0…][argv0\0argv1\0…].
        if procargs.len() < size_of::<c_int>() {
            return None;
        }
        let nargs = i32::from_ne_bytes([procargs[0], procargs[1], procargs[2], procargs[3]]);
        let n = procargs.len();
        let mut cp = size_of::<c_int>();

        // Skip the saved exec_path, then its trailing NUL padding.
        while cp < n && procargs[cp] != 0 {
            cp += 1;
        }
        if cp == n {
            return None;
        }
        while cp < n && procargs[cp] == 0 {
            cp += 1;
        }
        if cp == n {
            return None;
        }

        // sp = start of argv[0]; walk argc args, turning each inter-arg NUL
        // into a space and tracking argv[0]'s end as the basename boundary.
        let sp = cp;
        let mut c = 0i32;
        let mut np: Option<usize> = None;
        let mut end = 0usize;
        while c < nargs && cp < n {
            if procargs[cp] == 0 {
                c += 1;
                if let Some(prev) = np {
                    procargs[prev] = b' ';
                }
                np = Some(cp);
                if end == 0 {
                    end = cp - sp;
                }
            }
            cp += 1;
        }

        let np = np?;
        if np == sp {
            return None;
        }
        if end == 0 {
            end = np - sp;
        }

        let cmdline = String::from_utf8_lossy(&procargs[sp..np]).into_owned();
        // `end` is a byte offset in the original buffer; lossy decoding can
        // shift lengths on non-UTF-8 argv, so clamp to the string bounds.
        let end = end.min(cmdline.len());
        Some((cmdline, end))
    })();

    match parsed {
        Some((cmdline, end)) => Process_updateCmdline(proc, Some(&cmdline), 0, end),
        None => {
            let end = comm.len();
            let arg = if comm.is_empty() { None } else { Some(comm.as_str()) };
            Process_updateCmdline(proc, arg, 0, end);
        }
    }
}

/// `NODEV` (`sys/types.h`) — the invalid device sentinel, `(dev_t)-1`.
const NODEV: libc::dev_t = -1;
/// `MAXNAMLEN` (`sys/syslimits.h`) — max filename length.
const MAXNAMLEN: c_int = 255;

extern "C" {
    // `char* devname_r(dev_t dev, mode_t type, char* buf, int len)` — the
    // reentrant tty-name lookup; not exposed by `libc`.
    fn devname_r(
        dev: libc::dev_t,
        mode: libc::mode_t,
        buf: *mut c_char,
        len: c_int,
    ) -> *mut c_char;
}

/// Port of `static char* DarwinProcess_getDevname(dev_t dev)` from
/// `DarwinProcess.c:280`. Resolves a character device number to its
/// `/dev` name via `devname_r`, returning `None` for `NODEV` or on failure.
/// The C returns an `xStrdup`'d string; the Rust owner is the `String`.
pub fn DarwinProcess_getDevname(dev: libc::dev_t) -> Option<String> {
    if dev == NODEV {
        return None;
    }

    // char buf[sizeof("/dev/") + MAXNAMLEN]
    let mut buf = [0 as c_char; 6 + MAXNAMLEN as usize];
    let name = unsafe { devname_r(dev, libc::S_IFCHR, buf.as_mut_ptr(), MAXNAMLEN) };
    if name.is_null() {
        return None;
    }

    let s = unsafe { std::ffi::CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    Some(s)
}

/// `#define P_TRANSLATED 0x00020000` (`sys/proc.h`) — process runs under
/// Rosetta 2 translation.
const P_TRANSLATED: c_int = 0x00020000;
/// `#define PROCESS_FLAG_TTY 0x00000100` (`darwin/DarwinProcess.h`).
const PROCESS_FLAG_TTY: u32 = 0x00000100;

/// Port of `void DarwinProcess_setFromKInfoProc(Process* proc, const struct
/// kinfo_proc* ps, bool exists)` from `DarwinProcess.c:292`. Fills a process
/// from its `kinfo_proc`: the immutable identity fields on first sight
/// (`!exists`) — pid/ppid/pgrp/tpgid, translated flag, tty device, start
/// time, exe and command line — plus the always-refreshed nice/priority/
/// state. The expensive TTY and CWD lookups are gated on the active
/// screen's scan flags (`host->settings->ss->flags`). The
/// `(DarwinProcess*)proc` downcast (sound via `#[repr(C)]`) is used only to
/// write `translated`.
pub fn DarwinProcess_setFromKInfoProc(proc: &mut Process, ps: &kinfo_proc, exists: bool) {
    let ep = &ps.kp_proc;

    // const Settings* settings = proc->super.host->settings; -> ss->flags.
    // Read once through the raw host pointer; absent settings request no
    // extra scans (0 flags).
    let flags: u32 = unsafe {
        let host = proc.super_.host as *const Machine;
        host.as_ref()
            .and_then(|m| m.settings.as_ref())
            .and_then(|s| s.screens.get(s.ssIndex as usize))
            .map_or(0, |ss| ss.flags)
    };

    /* First, the "immutable" parts */
    if !exists {
        Process_setPid(proc, ep.p_pid);
        Process_setThreadGroup(proc, ep.p_pid);
        Process_setParent(proc, ps.kp_eproc.e_ppid);
        proc.pgrp = ps.kp_eproc.e_pgid;
        proc.session = 0; /* TODO Get the session id */
        proc.tpgid = ps.kp_eproc.e_tpgid;
        proc.isKernelThread = false;
        proc.isUserlandThread = false;
        // dp->translated = ps->kp_proc.p_flag & P_TRANSLATED
        let translated = (ep.p_flag & P_TRANSLATED) != 0;
        unsafe {
            (*(proc as *mut Process as *mut DarwinProcess)).translated = translated;
        }
        proc.tty_nr = ps.kp_eproc.e_tdev as u64;
        proc.tty_name = None;

        proc.starttime_ctime = ep.p_starttime.tv_sec as i64;
        Process_fillStarttimeBuffer(proc);

        DarwinProcess_updateExe(ep.p_pid, proc);
        DarwinProcess_updateCmdLine(ps, proc);

        if flags & PROCESS_FLAG_CWD != 0 {
            DarwinProcess_updateCwd(ep.p_pid, proc);
        }
    }

    if proc.tty_name.is_none() && proc.tty_nr as libc::dev_t != NODEV {
        // The call to devname() is extremely expensive (lstat), so only
        // fetch TTY info when the TTY field is enabled in the settings.
        if flags & PROCESS_FLAG_TTY != 0 {
            proc.tty_name = DarwinProcess_getDevname(proc.tty_nr as libc::dev_t);
            if proc.tty_name.is_none() {
                /* devname failed: prevent us from calling it again */
                proc.tty_nr = NODEV as u64;
            }
        }
    }

    /* Mutable information */
    proc.nice = ep.p_nice as i32;
    proc.priority = ep.p_priority as i64;

    proc.state = if ep.p_stat as u32 == libc::SZOMB {
        ProcessState::ZOMBIE
    } else {
        ProcessState::UNKNOWN
    };

    /* Make sure the updated flag is set */
    proc.super_.updated = true;
}

/// `#define ONE_K 1024` (`Macros.h`).
const ONE_K: u64 = 1024;

/// Port of `void DarwinProcess_setFromLibprocPidinfo(DarwinProcess* proc,
/// DarwinProcessTable* dpt, double timeIntervalNS)` from
/// `DarwinProcess.c:364`. Reads per-task counters via
/// `proc_pidinfo(PROC_PIDTASKINFO)` and derives CPU%/memory/thread stats:
/// CPU% from the user+system mach-tick delta over `timeIntervalNS`, memory%
/// against the host's `max_mem`, plus virt/resident sizes, fault count,
/// thread count and run state. On a task-access failure it clears
/// `taskAccess` and returns. Also accumulates the table's task/thread
/// counters. `dhost` is recovered from `host` via the sound `#[repr(C)]`
/// `*Machine`→`*DarwinMachine` downcast.
pub fn DarwinProcess_setFromLibprocPidinfo(
    proc: &mut DarwinProcess,
    dpt: &mut DarwinProcessTable,
    timeIntervalNS: f64,
) {
    let mut pti: libc::proc_taskinfo = unsafe { zeroed() };
    let size = size_of::<libc::proc_taskinfo>() as c_int;

    let got = unsafe {
        libc::proc_pidinfo(
            Process_getPid(&proc.super_),
            libc::PROC_PIDTASKINFO,
            0,
            &mut pti as *mut libc::proc_taskinfo as *mut c_void,
            size,
        )
    };
    if got != size {
        proc.taskAccess = false;
        return;
    }

    // const DarwinMachine* dhost = (const DarwinMachine*) proc->super.super.host;
    let dhost = proc.super_.super_.host as *const DarwinMachine;
    let max_mem = unsafe { (*dhost).host_info.max_mem };

    let total_existing_time_ns = proc.stime + proc.utime;
    let user_time_ns = Platform_machTicksToNanoseconds(pti.pti_total_user);
    let system_time_ns = Platform_machTicksToNanoseconds(pti.pti_total_system);
    let total_current_time_ns = user_time_ns + system_time_ns;

    if total_existing_time_ns < total_current_time_ns {
        let total_time_diff_ns = total_current_time_ns - total_existing_time_ns;
        proc.super_.percent_cpu = ((total_time_diff_ns as f64 / timeIntervalNS) * 100.0) as f32;
    } else {
        proc.super_.percent_cpu = 0.0;
    }
    Process_updateCPUFieldWidths(proc.super_.percent_cpu);

    proc.super_.state = if pti.pti_numrunning > 0 {
        ProcessState::RUNNING
    } else {
        ProcessState::SLEEPING
    };
    // Convert from nanoseconds to hundredths of seconds
    proc.super_.time = total_current_time_ns / 10_000_000;
    proc.super_.nlwp = pti.pti_threadnum as i64;
    proc.super_.m_virt = (pti.pti_virtual_size / ONE_K) as i64;
    proc.super_.m_resident = (pti.pti_resident_size / ONE_K) as i64;
    proc.super_.majflt = pti.pti_faults as u64;
    proc.super_.percent_mem = (pti.pti_resident_size as f64 * 100.0 / max_mem as f64) as f32;

    proc.stime = system_time_ns;
    proc.utime = user_time_ns;

    // dpt->super.kernelThreads += 0 (pti.pti_threads_system unused in C)
    dpt.super_.userlandThreads += pti.pti_threadnum as u32;
    dpt.super_.totalTasks += pti.pti_threadnum as u32;
    dpt.super_.runningTasks += pti.pti_numrunning as u32;
}

/// TODO: port of `void DarwinProcess_scanThreads(DarwinProcess* dp, DarwinProcessTable* dpt` from `DarwinProcess.c:410`.
pub fn DarwinProcess_scanThreads() {
    todo!("port of DarwinProcess.c:410")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_darwin_defaults_and_unknown_state() {
        let host = 0xF00D as *const Machine;
        let p = DarwinProcess_new(host);

        // Darwin per-process accumulators start per the C constructor.
        assert_eq!(p.utime, 0);
        assert_eq!(p.stime, 0);
        assert!(p.taskAccess);
        assert!(!p.translated);
        // Process_init wired the base row's host back-pointer, and the
        // constructor forces state UNKNOWN after init.
        assert_eq!(p.super_.state, ProcessState::UNKNOWN);
        assert_eq!(p.super_.super_.host, host as *const c_void);
    }

    #[test]
    fn super_is_at_offset_zero_for_sound_downcast() {
        // The `(DarwinProcess*)processPtr` downcast htop uses is only sound
        // if the embedded base sits at offset 0. `#[repr(C)]` guarantees it;
        // this pins the invariant and proves the pointer round-trips.
        assert_eq!(core::mem::offset_of!(DarwinProcess, super_), 0);

        let host = 0xF00D as *const Machine;
        let dp = DarwinProcess_new(host);
        let base: *const Process = &dp.super_;
        let back = base as *const DarwinProcess;
        assert_eq!(back, &*dp as *const DarwinProcess);
    }

    #[test]
    fn updateExe_sets_exe_from_own_pid() {
        // Our own pid always has a resolvable executable path, so
        // proc_pidpath succeeds and Process_updateExe records a non-empty exe.
        let host = 0xF00D as *const Machine;
        let mut dp = DarwinProcess_new(host);
        let pid = unsafe { libc::getpid() };

        DarwinProcess_updateExe(pid, &mut dp.super_);

        let exe = dp.super_.procExe.as_deref().unwrap_or("");
        assert!(!exe.is_empty());
        assert!(exe.starts_with('/'));
    }

    #[test]
    fn updateCwd_sets_absolute_cwd_from_own_pid() {
        // Our own process's cwd is always readable and absolute.
        let host = 0xF00D as *const Machine;
        let mut dp = DarwinProcess_new(host);
        let pid = unsafe { libc::getpid() };

        DarwinProcess_updateCwd(pid, &mut dp.super_);

        let cwd = dp.super_.procCwd.as_deref().expect("own cwd resolves");
        assert!(cwd.starts_with('/'));
    }

    #[test]
    fn updateCmdLine_sets_comm_and_cmdline_from_own_procargs() {
        // Build a kinfo_proc for our own pid, then parse its argument space.
        let pid = unsafe { libc::getpid() };
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
        let mut kp: kinfo_proc = unsafe { zeroed() };
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
        assert_eq!(rc, 0);

        let host = 0xF00D as *const Machine;
        let mut dp = DarwinProcess_new(host);
        DarwinProcess_updateCmdLine(&kp, &mut dp.super_);

        // Our own argv is readable, so both the short comm and the full
        // command line are populated, and the basename lies within cmdline.
        assert!(dp.super_.procComm.is_some());
        let cmdline = dp.super_.cmdline.as_deref().expect("own cmdline resolves");
        assert!(!cmdline.is_empty());
        assert!(dp.super_.cmdlineBasenameEnd <= cmdline.len());
    }

    #[test]
    fn getDevname_resolves_dev_null_and_rejects_nodev() {
        // NODEV is the invalid sentinel.
        assert_eq!(DarwinProcess_getDevname(NODEV), None);

        // /dev/null's device number resolves back to the name "null".
        let mut st: libc::stat = unsafe { zeroed() };
        let rc = unsafe { libc::stat(b"/dev/null\0".as_ptr() as *const c_char, &mut st) };
        assert_eq!(rc, 0);
        assert_eq!(DarwinProcess_getDevname(st.st_rdev).as_deref(), Some("null"));
    }

    #[test]
    fn setFromKInfoProc_fills_identity_from_own_kinfo() {
        use crate::ported::process::{Process_getParent, Process_getPid};
        use crate::ported::settings::{ScreenSettings, Settings};

        // kinfo_proc for our own process.
        let pid = unsafe { libc::getpid() };
        let ppid = unsafe { libc::getppid() };
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
        let mut kp: kinfo_proc = unsafe { zeroed() };
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
        assert_eq!(rc, 0);

        // Host with realtimeMs (for the start-time buffer) and one screen
        // whose flags request neither CWD nor TTY (so no extra lookups).
        let mut machine = Machine::default();
        machine.realtimeMs = 1_700_000_000_000;
        machine.settings = Some(Settings {
            screens: vec![ScreenSettings::default()],
            ssIndex: 0,
            ..Default::default()
        });

        let mut dp = DarwinProcess_new(&machine as *const Machine);
        DarwinProcess_setFromKInfoProc(&mut dp.super_, &kp, false);

        // Identity fields come straight from the kernel struct.
        assert_eq!(Process_getPid(&dp.super_), pid);
        assert_eq!(Process_getParent(&dp.super_), ppid);
        assert_eq!(dp.super_.pgrp, kp.kp_eproc.e_pgid);
        assert_eq!(dp.super_.tpgid, kp.kp_eproc.e_tpgid);
        // The command line was reconstructed and the row marked updated.
        assert!(dp.super_.cmdline.is_some());
        assert!(dp.super_.super_.updated);
        // We are not a zombie.
        assert_eq!(dp.super_.state, ProcessState::UNKNOWN);
    }

    #[test]
    fn setFromLibprocPidinfo_fills_stats_for_own_pid() {
        use crate::ported::darwin::darwinmachine::{host_basic_info_data_t, DarwinMachine};
        use crate::ported::darwin::darwinprocesstable::DarwinProcessTable;
        use crate::ported::linux::linuxmachine::ZfsArcStats;
        use crate::ported::processtable::ProcessTable;

        // Address-stable DarwinMachine host with a known max_mem (16 GiB).
        let dm = Box::new(DarwinMachine {
            super_: Machine::default(),
            host_info: host_basic_info_data_t {
                max_mem: 16u64 << 30,
                ..Default::default()
            },
            vm_stats: unsafe { zeroed() },
            prev_load: ptr::null_mut(),
            curr_load: ptr::null_mut(),
            GPUService: 0,
            zfs: ZfsArcStats::default(),
        });

        // A DarwinProcess for our own pid; its Row.host points at the base.
        let mut dp = DarwinProcess_new(&dm.super_ as *const Machine);
        Process_setPid(&mut dp.super_, unsafe { libc::getpid() });

        let mut dpt = Box::new(DarwinProcessTable {
            super_: ProcessTable::empty(),
            global_diff: 0,
        });

        DarwinProcess_setFromLibprocPidinfo(&mut dp, &mut dpt, 1e9);

        // Our own task is readable, so the stats populate.
        assert!(dp.taskAccess);
        assert!(dp.super_.nlwp >= 1); // at least one thread
        assert!(dp.super_.m_resident > 0); // some resident memory
        assert!(dp.super_.percent_mem >= 0.0);
        assert!(dpt.super_.totalTasks >= 1);
        assert!(matches!(
            dp.super_.state,
            ProcessState::RUNNING | ProcessState::SLEEPING
        ));
    }
}
