//! Partial port of `linux/LinuxProcess.c` + `linux/LinuxProcess.h` — the
//! Linux-specific process data model (`LinuxProcess`, which "extends"
//! [`Process`]) and the pure/action logic that does not depend on unported
//! substrate.
//!
//! C names are preserved verbatim (`LinuxProcess_new`, …), so
//! `non_snake_case` is allowed for the whole module.
//!
//! Ported:
//! - [`LinuxProcess`] — every `struct LinuxProcess_` field from
//!   `LinuxProcess.h:33`, embedding [`Process`] as its `super_` base (the
//!   substrate `LinuxProcessTable`/`GPU`/`LibNl` consume).
//! - [`LinuxProcess_new`] (`LinuxProcess.c:112`) — constructor.
//! - [`LinuxProcess_effectiveIOPriority`] (`LinuxProcess.c:137`) — pure.
//! - [`LinuxProcess_updateIOPriority`] (`LinuxProcess.c:153`) and
//!   [`LinuxProcess_setIOPriority`] (`LinuxProcess.c:164`) — the
//!   `ioprio_get`/`ioprio_set` syscalls (Linux-only, exactly as the C
//!   `#ifdef SYS_ioprio_*` guards them), plus
//!   [`LinuxProcess_rowSetIOPriority`] (`LinuxProcess.c:172`).
//! - [`LinuxProcess_totalIORate`] (`LinuxProcess.c:213`) — pure.
//! - [`LinuxProcess_changeAutogroupPriorityBy`] (`LinuxProcess.c:185`) and
//!   [`LinuxProcess_rowChangeAutogroupPriorityBy`] (`LinuxProcess.c:207`).
//!
//! Still stubbed (blocked on unported substrate; see each fn's doc):
//! - [`Process_delete`] (`LinuxProcess.c:119`) — a pure `free()` teardown
//!   (Rust `Drop` handles it), kept stubbed per the module port rules.
//! - [`LinuxProcess_isAutogroupEnabled`] (`LinuxProcess.c:178`) — calls the
//!   unported bare-stub `Compat_readfile` (no signature to call yet).
//! - [`LinuxProcess_rowWriteField`] (`LinuxProcess.c:226`) and
//!   [`LinuxProcess_compareByKey`] (`LinuxProcess.c:366`) — both switch on
//!   the Linux platform [`ProcessField`] ids (`M_DRS`, `RCHAR`, `OOM`, …)
//!   defined by `linux/ProcessField.h`, which the shared [`ProcessField`]
//!   enum in `process.rs` intentionally does not enumerate (it models only
//!   the reserved generic fields). They stay stubbed until that enum models
//!   the platform fields.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::machine::Machine;
use crate::ported::object::{Arg, Object, ObjectClass, Object_isA};
use crate::ported::process::{
    Process, Process_class, Process_compare, Process_getPid, Process_init,
};
use core::any::Any;
use core::ffi::c_void;

/// Port of `#define PROCDIR "/proc"` from `LinuxMachine.h:105` — the procfs
/// mount htop was compiled to read. Defined locally (as `platform.rs` also
/// does) so this module's `/proc` reads are self-contained.
const PROCDIR: &str = "/proc";

// ── IOPriority.h (`typedef int IOPriority;` + its class/data/tuple macros)
//
// `linux/IOPriority.h` is not yet a Rust module; the `LinuxProcess`
// `ioPriority` field and the IO-priority functions below need its type and
// constants, so they are modeled here. These are plain constants/type
// aliases (not free `fn`s), and the macros are inlined at each use site —
// the faithful analog of C text substitution.

/// Port of `typedef int IOPriority;` from `IOPriority.h:29`.
pub type IOPriority = i32;

/// Port of the anonymous IO-priority class enum from `IOPriority.h:14`.
const IOPRIO_CLASS_NONE: i32 = 0;
const IOPRIO_CLASS_RT: i32 = 1;
const IOPRIO_CLASS_BE: i32 = 2;
const IOPRIO_CLASS_IDLE: i32 = 3;

/// Port of `#define IOPRIO_WHO_PROCESS 1` from `IOPriority.h:21`.
const IOPRIO_WHO_PROCESS: i32 = 1;

/// Port of `#define IOPRIO_CLASS_SHIFT (13)` from `IOPriority.h:23`.
const IOPRIO_CLASS_SHIFT: i32 = 13;
/// Port of `#define IOPRIO_PRIO_MASK ((1UL << IOPRIO_CLASS_SHIFT) - 1)`
/// from `IOPriority.h:24`.
const IOPRIO_PRIO_MASK: i32 = (1 << IOPRIO_CLASS_SHIFT) - 1;

/// Port of `struct LinuxProcess_` from `LinuxProcess.h:33`. "Extends"
/// [`Process`] via the embedded `super_` field (htop's emulated
/// single-inheritance) and carries the Linux-specific per-process fields.
///
/// Field-type mapping (following the `Process`/`Affinity` precedents):
/// - C `Process super;` → [`super_`](LinuxProcess::super_) (raw identifier
///   so the name matches C's `super`).
/// - `IOPriority ioPriority` → [`IOPriority`] (`i32`).
/// - `unsigned long int` / `unsigned long long int` counters → `u64`;
///   `long` memory sizes → `i64`; `double` rates → `f64`; `float` percents
///   → `f32`; `unsigned int oom` → `u32`; `long int autogroup_id` → `i64`;
///   `int autogroup_nice` → `i32`; `uint64_t gpu_activityMs` → `u64`.
/// - Owned C strings (`char* cgroup/cgroup_short/container_short/secattr`)
///   → `Option<String>` (`None` = C `NULL`).
///
/// The `#ifdef HAVE_DELAYACCT` fields are included so the delay-accounting
/// consumer (`LibNl`) can rely on them; they correspond to the
/// `--enable-delayacct` build.
#[derive(Debug, Clone, Default)]
pub struct LinuxProcess {
    /// C `Process super` — the embedded base class.
    pub super_: Process,

    /// C `IOPriority ioPriority`.
    pub ioPriority: IOPriority,
    /// C `unsigned long int cminflt` — children's minor faults.
    pub cminflt: u64,
    /// C `unsigned long int cmajflt` — children's major faults.
    pub cmajflt: u64,
    /// C `unsigned long long int utime` — user CPU time.
    pub utime: u64,
    /// C `unsigned long long int stime` — system CPU time.
    pub stime: u64,
    /// C `unsigned long long int cutime` — children's user CPU time.
    pub cutime: u64,
    /// C `unsigned long long int cstime` — children's system CPU time.
    pub cstime: u64,
    /// C `long m_share` — shared pages.
    pub m_share: i64,
    /// C `long m_priv` — private memory size.
    pub m_priv: i64,
    /// C `long m_pss` — proportional set size.
    pub m_pss: i64,
    /// C `long m_swap` — swapped pages.
    pub m_swap: i64,
    /// C `long m_psswp` — proportional swap share.
    pub m_psswp: i64,
    /// C `long m_epss` — effective proportional set size.
    pub m_epss: i64,
    /// C `long m_trs` — `.text` segment size (CODE).
    pub m_trs: i64,
    /// C `long m_drs` — `.data` segment + stack size (DATA).
    pub m_drs: i64,
    /// C `long m_lrs` — library size.
    pub m_lrs: i64,

    /// C `unsigned long int flags` — process flags.
    pub flags: u64,

    /// C `unsigned long long io_rchar` — data read (bytes).
    pub io_rchar: u64,
    /// C `unsigned long long io_wchar` — data written (bytes).
    pub io_wchar: u64,
    /// C `unsigned long long io_syscr` — number of `read(2)` syscalls.
    pub io_syscr: u64,
    /// C `unsigned long long io_syscw` — number of `write(2)` syscalls.
    pub io_syscw: u64,
    /// C `unsigned long long io_read_bytes` — storage data read (bytes).
    pub io_read_bytes: u64,
    /// C `unsigned long long io_write_bytes` — storage data written (bytes).
    pub io_write_bytes: u64,
    /// C `unsigned long long io_cancelled_write_bytes`.
    pub io_cancelled_write_bytes: u64,
    /// C `unsigned long long io_last_scan_time_ms` — last IO scan time (ms since Epoch).
    pub io_last_scan_time_ms: u64,
    /// C `double io_rate_read_bps` — storage read rate (bytes/s).
    pub io_rate_read_bps: f64,
    /// C `double io_rate_write_bps` — storage write rate (bytes/s).
    pub io_rate_write_bps: f64,

    /// C `char* cgroup` — raw cgroup path.
    pub cgroup: Option<String>,
    /// C `char* cgroup_short` — condensed cgroup path.
    pub cgroup_short: Option<String>,
    /// C `char* container_short` — guessed container name.
    pub container_short: Option<String>,
    /// C `unsigned int oom` — OOM killer score.
    pub oom: u32,

    // `#ifdef HAVE_DELAYACCT` — delay-accounting fields.
    /// C `unsigned long long int delay_read_time`.
    pub delay_read_time: u64,
    /// C `unsigned long long cpu_delay_total`.
    pub cpu_delay_total: u64,
    /// C `unsigned long long blkio_delay_total`.
    pub blkio_delay_total: u64,
    /// C `unsigned long long swapin_delay_total`.
    pub swapin_delay_total: u64,
    /// C `float cpu_delay_percent`.
    pub cpu_delay_percent: f32,
    /// C `float blkio_delay_percent`.
    pub blkio_delay_percent: f32,
    /// C `float swapin_delay_percent`.
    pub swapin_delay_percent: f32,

    /// C `unsigned long ctxt_total` — cumulative context switches.
    pub ctxt_total: u64,
    /// C `unsigned long ctxt_diff` — context switches this cycle.
    pub ctxt_diff: u64,
    /// C `char* secattr` — security attribute (SELinux/AppArmor).
    pub secattr: Option<String>,
    /// C `unsigned long long int last_mlrs_calctime`.
    pub last_mlrs_calctime: u64,

    /// C `unsigned long long int gpu_time` — total GPU time (ns).
    pub gpu_time: u64,
    /// C `float gpu_percent` — GPU utilization (%).
    pub gpu_percent: f32,
    /// C `uint64_t gpu_activityMs` — 0 if active, else last scan time (ms).
    pub gpu_activityMs: u64,

    /// C `long int autogroup_id` — CFS autogroup identifier.
    pub autogroup_id: i64,
    /// C `int autogroup_nice` — autogroup nice value.
    pub autogroup_nice: i32,
}

/// Port of `const ProcessClass LinuxProcess_class` from `LinuxProcess.c:459`.
/// Only the class-identity link is modeled by [`ObjectClass`]
/// (`.super.super.extends = Class(Process)`); the vtable slots
/// (`.writeField = LinuxProcess_rowWriteField`, `.compareByKey =
/// LinuxProcess_compareByKey`, `.delete = Process_delete`, `.display =
/// Row_display`, `.compare = Process_compare`) are realized by the ported
/// free functions and the [`Object`] impl below, mirroring how
/// `Process_class` models `Process`'s vtable.
pub static LinuxProcess_class: ObjectClass = ObjectClass {
    extends: Some(&Process_class),
};

impl Object for LinuxProcess {
    /// C `Object_setClass(this, Class(LinuxProcess))` in [`LinuxProcess_new`]:
    /// the object's class is `&LinuxProcess_class`.
    fn klass(&self) -> &'static ObjectClass {
        &LinuxProcess_class
    }

    /// C `LinuxProcess_class.super.super.compare = Process_compare`.
    /// Downcasts the peer back to [`LinuxProcess`] (the safe-Rust analog of
    /// the C `const void*` cast) and delegates to [`Process_compare`] on the
    /// embedded bases — stubbed pending the `Settings` substrate, matching
    /// the C wiring (and `Process`'s own `Object::compare`).
    fn compare(&self, other: &dyn Object) -> i32 {
        let o = (other as &dyn Any)
            .downcast_ref::<LinuxProcess>()
            .expect("LinuxProcess compare called across incompatible classes");
        Process_compare(&self.super_, &o.super_)
    }
}

/// Port of `Process* LinuxProcess_new(const Machine* host)` from
/// `LinuxProcess.c:112`. C `xCalloc`s the struct (so every field is
/// zero/`NULL`), sets its class to `LinuxProcess`, and runs
/// [`Process_init`] on the embedded base. The heap pointer is modeled as an
/// owned value returned by move (the `Affinity_new` idiom); the
/// zero-initialization is [`Default`], and the class is realized by the
/// [`Object`] impl above rather than a stored `klass` pointer. `host` is
/// only stored/forwarded, never dereferenced, so it stays a borrowed
/// `*const` pointer.
pub fn LinuxProcess_new(host: *const Machine) -> LinuxProcess {
    let mut this = LinuxProcess::default();
    Process_init(&mut this.super_, host as *const c_void);
    this
}

/// TODO: port of `void Process_delete(Object* cast)` from
/// `LinuxProcess.c:119`. Kept stubbed: the C body is a pure teardown —
/// `Process_done(...)` followed by `free(container_short/cgroup_short/
/// cgroup/secattr)` and `free(this)`. Rust owns those `Option<String>`
/// allocations and the struct itself, so `Drop` reclaims them
/// automatically; there is no faithful safe-Rust analog (the
/// `Affinity_delete` / `History_delete` precedent).
pub fn Process_delete() {
    todo!("port of LinuxProcess.c:119 — pure free() teardown; Rust Drop handles it")
}

/// Port of `static int LinuxProcess_effectiveIOPriority(const LinuxProcess*
/// this)` from `LinuxProcess.c:137`. When the process has no explicit IO
/// priority class (`IOPRIO_CLASS_NONE`), the effective priority is derived
/// from the CPU nice level in the best-effort class (see note [1] in the C
/// source); otherwise the stored `ioPriority` is returned. The
/// `IOPriority_class` / `IOPriority_tuple` macros are inlined verbatim
/// (`>> IOPRIO_CLASS_SHIFT`, `(class << SHIFT) | data`).
fn LinuxProcess_effectiveIOPriority(this: &LinuxProcess) -> i32 {
    if (this.ioPriority >> IOPRIO_CLASS_SHIFT) == IOPRIO_CLASS_NONE {
        return (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | ((this.super_.nice + 20) / 5);
    }

    this.ioPriority
}

/// Port of `IOPriority LinuxProcess_updateIOPriority(Process* p)` from
/// `LinuxProcess.c:153`. Gathers the thread's IO scheduling class+priority
/// via the `ioprio_get` syscall and caches it in `ioPriority`.
///
/// Signature mapping: the C body immediately does `LinuxProcess* this =
/// (LinuxProcess*) p;`, so the faithful Rust receiver is the concrete
/// `&mut LinuxProcess` (Rust embedding cannot recover the outer struct from
/// `&Process`). Exactly as the C `#ifdef SYS_ioprio_get` guards the
/// syscall (other OSes masquerading as Linux lack it), the syscall path is
/// `#[cfg(target_os = "linux")]`; elsewhere the result is `0`, matching the
/// C `IOPriority ioprio = 0;` fallthrough.
pub fn LinuxProcess_updateIOPriority(this: &mut LinuxProcess) -> IOPriority {
    #[cfg(target_os = "linux")]
    let ioprio: IOPriority = unsafe {
        libc::syscall(
            libc::SYS_ioprio_get as libc::c_long,
            IOPRIO_WHO_PROCESS,
            Process_getPid(&this.super_),
        )
    } as i32;
    #[cfg(not(target_os = "linux"))]
    let ioprio: IOPriority = 0;

    this.ioPriority = ioprio;
    ioprio
}

/// Port of `static bool LinuxProcess_setIOPriority(Process* p, Arg ioprio)`
/// from `LinuxProcess.c:164`. Applies the requested IO priority via the
/// `ioprio_set` syscall, then re-reads it (via
/// [`LinuxProcess_updateIOPriority`]) and reports whether it took effect.
///
/// `Arg ioprio` is the [`Arg::I`] arm (the C reads `ioprio.i`
/// unconditionally, so the `Arg::V` arm is impossible here). The syscall is
/// `#[cfg(target_os = "linux")]`, mirroring the C `#ifdef SYS_ioprio_set`.
fn LinuxProcess_setIOPriority(this: &mut LinuxProcess, ioprio: Arg) -> bool {
    let i = match ioprio {
        Arg::I(i) => i,
        Arg::V(_) => panic!("LinuxProcess_setIOPriority: Arg must carry the priority in arg.i"),
    };

    #[cfg(target_os = "linux")]
    unsafe {
        libc::syscall(
            libc::SYS_ioprio_set as libc::c_long,
            IOPRIO_WHO_PROCESS,
            Process_getPid(&this.super_),
            i,
        );
    }

    LinuxProcess_updateIOPriority(this) == i
}

/// Port of `bool LinuxProcess_rowSetIOPriority(Row* super, Arg ioprio)` from
/// `LinuxProcess.c:172`. Casts the `Row*` to a `Process*` (here a
/// [`LinuxProcess`], the concrete object), asserts it really is a
/// [`Process`] via [`Object_isA`], and delegates to
/// [`LinuxProcess_setIOPriority`]. The C `(Process*) super` cast validated
/// by `assert(Object_isA(..., &Process_class))` becomes the `Object_isA`
/// guard plus a mutable `Any` downcast (the `Affinity_rowSet` idiom).
pub fn LinuxProcess_rowSetIOPriority(super_: &mut dyn Object, ioprio: Arg) -> bool {
    debug_assert!(Object_isA(Some(super_ as &dyn Object), &Process_class));
    let p = (super_ as &mut dyn Any)
        .downcast_mut::<LinuxProcess>()
        .expect("LinuxProcess_rowSetIOPriority: row is not a LinuxProcess");
    LinuxProcess_setIOPriority(p, ioprio)
}

/// TODO: port of `bool LinuxProcess_isAutogroupEnabled(void)` from
/// `LinuxProcess.c:178`. Blocked: the C body reads
/// `PROCDIR "/sys/kernel/sched_autogroup_enabled"` through
/// `Compat_readfile`, which is still an unported bare stub in
/// `linux/compat.rs` (no `(path, buf, size) -> ssize_t` signature to call),
/// so the faithful call cannot be written yet.
pub fn LinuxProcess_isAutogroupEnabled() {
    todo!("port of LinuxProcess.c:178 — needs Compat_readfile signature")
}

/// Port of `static bool LinuxProcess_changeAutogroupPriorityBy(Process* p,
/// Arg delta)` from `LinuxProcess.c:185`. Opens `PROCDIR/<pid>/autogroup`
/// for read+write, parses the `"/autogroup-%ld nice %d"` line, and — if
/// that succeeds — rewinds and writes back `nice + delta.i`, returning
/// whether the write succeeded.
///
/// The C `fopen(..., "r+")` maps to [`std::fs::OpenOptions`] read+write; the
/// `fscanf` → `fseek(0)` → `fputs` sequence is preserved (read the line,
/// parse the two fields — the C `ok == 2` — then seek to the start and
/// write the new nice as decimal text). On any missing file / parse
/// failure the function returns `false`, exactly as the C `!file` and
/// `ok != 2` paths do. `delta` is the [`Arg::I`] arm (`delta.i`).
fn LinuxProcess_changeAutogroupPriorityBy(p: &Process, delta: Arg) -> bool {
    use std::io::{Read, Seek, SeekFrom, Write};

    let delta_i = match delta {
        Arg::I(i) => i,
        Arg::V(_) => {
            panic!("LinuxProcess_changeAutogroupPriorityBy: Arg must carry the delta in arg.i")
        }
    };

    let pid = Process_getPid(p);
    let path = format!("{}/{}/autogroup", PROCDIR, pid);

    // fopen(buffer, "r+")
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(_) => return false,
    };

    // int ok = fscanf(file, "/autogroup-%ld nice %d", &identity, &nice);
    // Read the content and parse the two whitespace-separated fields the
    // C format string expects: "/autogroup-<id>", "nice", "<n>".
    let mut content = String::new();
    if file.read_to_string(&mut content).is_err() {
        return false;
    }
    let parse_nice = || -> Option<i32> {
        let mut it = content.split_whitespace();
        let tok0 = it.next()?; // "/autogroup-%ld"
        let _identity: i64 = tok0.strip_prefix("/autogroup-")?.parse().ok()?;
        if it.next()? != "nice" {
            return None;
        }
        it.next()?.parse::<i32>().ok()
    };

    let mut success = false;
    if let Some(nice) = parse_nice() {
        // fseek(file, 0L, SEEK_SET) == 0
        if file.seek(SeekFrom::Start(0)).is_ok() {
            // xSnprintf(buffer, ..., "%d", nice + delta.i);
            // success = fputs(buffer, file) > 0;
            let buffer = format!("{}", nice + delta_i);
            success = file
                .write(buffer.as_bytes())
                .map(|w| w > 0)
                .unwrap_or(false);
        }
    }

    // fclose(file) — the file is dropped/closed here.
    success
}

/// Port of `bool LinuxProcess_rowChangeAutogroupPriorityBy(Row* super, Arg
/// delta)` from `LinuxProcess.c:207`. Casts the `Row*` to a `Process*`
/// (here a [`LinuxProcess`]), asserts it really is a [`Process`] via
/// [`Object_isA`], and delegates to
/// [`LinuxProcess_changeAutogroupPriorityBy`] on the embedded base. Same
/// `Row*`→`Process*` mapping as [`LinuxProcess_rowSetIOPriority`]; a shared
/// `&Process` suffices since the callee only reads the pid.
pub fn LinuxProcess_rowChangeAutogroupPriorityBy(super_: &dyn Object, delta: Arg) -> bool {
    debug_assert!(Object_isA(Some(super_), &Process_class));
    let lp = (super_ as &dyn Any)
        .downcast_ref::<LinuxProcess>()
        .expect("LinuxProcess_rowChangeAutogroupPriorityBy: row is not a LinuxProcess");
    LinuxProcess_changeAutogroupPriorityBy(&lp.super_, delta)
}

/// Port of `static double LinuxProcess_totalIORate(const LinuxProcess* lp)`
/// from `LinuxProcess.c:213`. Sums the non-negative read/write IO rates
/// (`NAN` when neither is available). `isNonnegative(x)` (`Macros.h:141`) is
/// `isgreaterequal(x, 0.0)`, i.e. `x >= 0.0` (false for `NaN`), inlined at
/// each use site.
fn LinuxProcess_totalIORate(lp: &LinuxProcess) -> f64 {
    let mut totalRate = f64::NAN;
    if lp.io_rate_read_bps >= 0.0 {
        totalRate = lp.io_rate_read_bps;
        if lp.io_rate_write_bps >= 0.0 {
            totalRate += lp.io_rate_write_bps;
        }
    } else if lp.io_rate_write_bps >= 0.0 {
        totalRate = lp.io_rate_write_bps;
    }
    totalRate
}

/// TODO: port of `static void LinuxProcess_rowWriteField(const Row* super,
/// RichString* str, ProcessField field)` from `LinuxProcess.c:226`.
/// Blocked: the `switch (field)` dispatches on the Linux platform
/// [`ProcessField`] ids (`M_DRS`, `M_LRS`, `RCHAR`, `OOM`, `IO_PRIORITY`,
/// `CGROUP`, …) defined in `linux/ProcessField.h`, but the shared
/// [`ProcessField`](crate::ported::process::ProcessField) enum intentionally
/// models only the reserved generic fields — the platform variants are not
/// enumerated, so the switch arms cannot be written yet. (The `Row_print*`
/// primitives it calls are already ported; only the field ids are missing.)
pub fn LinuxProcess_rowWriteField() {
    todo!("port of LinuxProcess.c:226 — needs Linux ProcessField variants in the shared enum")
}

/// TODO: port of `static int LinuxProcess_compareByKey(const Process* v1,
/// const Process* v2, ProcessField key)` from `LinuxProcess.c:366`. Blocked
/// for the same reason as [`LinuxProcess_rowWriteField`]: the `switch (key)`
/// compares the Linux platform [`ProcessField`] ids, which the shared
/// [`ProcessField`](crate::ported::process::ProcessField) enum does not
/// model. The per-field comparison helper [`LinuxProcess_effectiveIOPriority`]
/// and [`LinuxProcess_totalIORate`] it uses are already ported.
pub fn LinuxProcess_compareByKey() {
    todo!("port of LinuxProcess.c:366 — needs Linux ProcessField variants in the shared enum")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`LinuxProcess_new`] zero-initializes every field (C `xCalloc`) and
    /// runs [`Process_init`], which sets `st_uid = (uid_t)-1` and
    /// `cmdlineBasenameEnd = 0` on the embedded base.
    #[test]
    fn linuxprocess_new_initializes_base() {
        let lp = LinuxProcess_new(core::ptr::null());
        assert_eq!(lp.super_.st_uid, u32::MAX);
        assert_eq!(lp.super_.cmdlineBasenameEnd, 0);
        assert_eq!(lp.ioPriority, 0);
        assert_eq!(lp.oom, 0);
        assert!(lp.cgroup.is_none());
        // Class identity: the object is a LinuxProcess, which extends Process.
        assert!(Object_isA(Some(&lp as &dyn Object), &LinuxProcess_class));
        assert!(Object_isA(Some(&lp as &dyn Object), &Process_class));
    }

    /// [`LinuxProcess_effectiveIOPriority`]: with class `IOPRIO_CLASS_NONE`
    /// (stored `ioPriority == 0`), the effective priority is the
    /// best-effort tuple derived from `nice` — `(BE << 13) | (nice + 20)/5`.
    #[test]
    fn effective_io_priority_derives_from_nice_when_none() {
        let mut lp = LinuxProcess_new(core::ptr::null());
        lp.ioPriority = 0; // IOPRIO_CLASS_NONE, data 0
        lp.super_.nice = 0;
        let expected = (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | ((0 + 20) / 5);
        assert_eq!(LinuxProcess_effectiveIOPriority(&lp), expected);

        lp.super_.nice = -20;
        let expected = (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | ((-20 + 20) / 5);
        assert_eq!(LinuxProcess_effectiveIOPriority(&lp), expected);
    }

    /// When a real class is set (e.g. RT), the stored priority passes
    /// through unchanged.
    #[test]
    fn effective_io_priority_passthrough_when_classed() {
        let mut lp = LinuxProcess_new(core::ptr::null());
        // IOPriority_tuple(IOPRIO_CLASS_RT, 4)
        lp.ioPriority = (IOPRIO_CLASS_RT << IOPRIO_CLASS_SHIFT) | 4;
        lp.super_.nice = 5; // must be ignored
        assert_eq!(LinuxProcess_effectiveIOPriority(&lp), lp.ioPriority);
    }

    /// [`LinuxProcess_totalIORate`] mirrors the C branch table: both rates
    /// present → sum; only one present → that one; neither → `NaN`.
    #[test]
    fn total_io_rate_combines_available_rates() {
        let mut lp = LinuxProcess_new(core::ptr::null());

        lp.io_rate_read_bps = 100.0;
        lp.io_rate_write_bps = 50.0;
        assert_eq!(LinuxProcess_totalIORate(&lp), 150.0);

        lp.io_rate_read_bps = 100.0;
        lp.io_rate_write_bps = f64::NAN;
        assert_eq!(LinuxProcess_totalIORate(&lp), 100.0);

        lp.io_rate_read_bps = f64::NAN;
        lp.io_rate_write_bps = 42.0;
        assert_eq!(LinuxProcess_totalIORate(&lp), 42.0);

        lp.io_rate_read_bps = f64::NAN;
        lp.io_rate_write_bps = f64::NAN;
        assert!(LinuxProcess_totalIORate(&lp).is_nan());
    }
}
