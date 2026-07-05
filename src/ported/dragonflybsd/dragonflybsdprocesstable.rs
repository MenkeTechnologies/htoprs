//! Port of `dragonflybsd/DragonFlyBSDProcessTable.c` + `.h` — the DragonFly BSD
//! process-table scan layer. Compiled only under `#[cfg(target_os =
//! "dragonfly")]`; like the other BSD layers it is verified by primary-source
//! reading + the port-purity gate (not a cross-compile, DragonFly being a
//! tier-3 target with no prebuilt std here).
//!
//! Ported: [`DragonFlyBSDProcessTable_updateExe`] (`/proc/<pid>/file` readlink),
//! [`DragonFlyBSDProcessTable_updateCwd`] (`kern.proc.cwd` sysctl), and
//! [`DragonFlyBSDProcessTable_updateProcessName`] (`kvm_getargv`). `kvm_getargv`
//! is not exposed by `libc` for the DragonFly target, so it is hand-declared in
//! an `extern` block — the NetBSD `kvm_getargv2` precedent.
//!
//! Still stubbed: `goThroughEntries` / `ProcessTable_new` need `kvm_openfiles` +
//! `kvm_getprocs` (the whole scan driver), likewise hand-declarable when ported.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use core::slice;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

use crate::ported::process::{
    Process, Process_isKernelThread, Process_updateCmdline, Process_updateComm, Process_updateExe,
};
use crate::ported::processtable::ProcessTable;

extern "C" {
    /// `char** kvm_getargv(kvm_t*, const struct kinfo_proc*, int nchr)`
    /// (DragonFly `kvm.h`). Not exposed by `libc` for the DragonFly target, so
    /// it is declared here (the NetBSD `kvm_getargv2` precedent).
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

/// TODO: port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` (`DragonFlyBSDProcessTable.c:30`). Allocates the table and
/// runs `ProcessTable_init`; trivial but paired with the kvm scan below, so
/// it is scaffolded with the rest of the DragonFly-only layer.
pub fn ProcessTable_new() {
    todo!("port of DragonFlyBSDProcessTable.c:30 — ProcessTable_init + kvm (DragonFly-only)")
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

/// TODO: port of `void ProcessTable_goThroughEntries(ProcessTable* super)`
/// (`DragonFlyBSDProcessTable.c:133`). The main scan: `kvm_getprocs` over all
/// processes, filling each `Process`/`DragonFlyBSDProcess` from its
/// `kinfo_proc`. DragonFly `libkvm`; the platform's data source.
pub fn ProcessTable_goThroughEntries() {
    todo!("port of DragonFlyBSDProcessTable.c:133 — kvm_getprocs scan (DragonFly-only)")
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
