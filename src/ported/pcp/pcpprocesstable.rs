//! Port of `pcp/PCPProcessTable.c` + `.h` — the Performance Co-Pilot
//! process-table scan layer. `PCPProcessTable` "extends" [`ProcessTable`] via the
//! embedded `super_` and adds no fields of its own; the whole per-process refresh
//! is driven by pulling `proc.psinfo.*` / `proc.io.*` / `proc.memory.*` metric
//! instances out of the shared PMAPI result via [`Metric_iterate`] /
//! [`Metric_instance`].
//!
//! 1:1 faithful port; the C is the spec. Structure mirrors
//! `dragonflybsd/DragonFlyBSDProcessTable.c` (the `ProcessTable_new` + scan-vtable
//! glue + `ProcessTable_goThroughEntries` + getProcess/downcast/raw-pointer
//! aliasing pattern) and reuses the ported `Metric` FFI wrapper, `PCPProcess`
//! model, and the shared `Process`/`Row`/`Table` object model. Union field reads
//! (`atom.l`, `atom.ul`, `atom.ull`, `atom.cp`) are `unsafe`, as in the `Metric`
//! layer. `pmDebugOptions` debug branches are omitted (debug chatter).
//!
//! Confined to the `pcp` cargo feature; it will not link libpcp on macOS —
//! verified by `cargo check --features pcp` + primary-source reading + the
//! port-purity gate (the tier-3 model shared by the whole `pcp/` sub-tree).
//!
//! # Forward reference
//!
//! `Platform_getBootTime` (`pcp/Platform.c`, not yet ported) is scaffolded as a
//! `todo!()` in [`platform`](super::platform) and imported here so the
//! starttime call site stays 1:1 until `Platform.c` lands.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::any::Any;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

use crate::ported::linux::cgrouputils::CGroup_filterName;
use crate::ported::linux::linuxprocess::{
    PROCESS_FLAG_LINUX_AUTOGROUP, PROCESS_FLAG_LINUX_CGROUP, PROCESS_FLAG_LINUX_CTXT,
    PROCESS_FLAG_LINUX_OOM, PROCESS_FLAG_LINUX_SECATTR, PROCESS_FLAG_LINUX_SMAPS,
};
use crate::ported::machine::Machine;
use crate::ported::object::Object;
use crate::ported::pcp::metric::{
    Metric, Metric_enabled, Metric_instance, Metric_instance_kibibytes,
    Metric_instance_milliseconds, Metric_iterate,
};
use crate::ported::pcp::pcpmachine::PCPMachine;
use crate::ported::pcp::pcpprocess::{PCPProcess, PCPProcess_new};
use crate::ported::pcp::platform::Platform_getBootTime;
use crate::ported::pcp::pmapi::{
    pmAtomValue, PM_TYPE_32, PM_TYPE_STRING, PM_TYPE_U32, PM_TYPE_U64,
};
use crate::ported::process::{
    Process, ProcessField, ProcessState, Process_fillStarttimeBuffer, Process_getPid,
    Process_getThreadGroup, Process_isKernelThread, Process_isThread, Process_isUserlandThread,
    Process_setParent, Process_setThreadGroup, Process_updateCPUFieldWidths, Process_updateCmdline,
    Process_updateComm, Process_updateExe, PROCESS_FLAG_CWD, PROCESS_FLAG_IO,
};
use crate::ported::processtable::{
    ProcessTable, ProcessTable_cleanupEntries, ProcessTable_done, ProcessTable_getProcess,
    ProcessTable_init, ProcessTable_prepareEntries,
};
use crate::ported::row::Row_updateFieldWidth;
use crate::ported::settings::RowField;
use crate::ported::table::{Table, TableClass};
use crate::ported::userstable::UsersTable;
use crate::ported::xutils::{saturatingSub, String_safeStrncpy};

use Metric::*;

/// Port of `#define ONE_K 1024` (`Macros.h`) — the IO byte→KiB divisor / rate
/// multiplier used by [`PCPProcessTable_updateIO`].
const ONE_K: u64 = 1024;

/// Port of `#define MAX_NAME 128` (`Machine.h:28`) — the `command[MAX_NAME + 1]`
/// short-command scratch buffer size.
const MAX_NAME: usize = 128;

/// Port of `typedef struct PCPProcessTable_` (`PCPProcessTable.h`). "Extends"
/// [`ProcessTable`] via the embedded `super_`; PCP adds no fields of its own
/// (the DragonFly precedent). Construct via [`ProcessTable::empty`].
pub struct PCPProcessTable {
    /// C `ProcessTable super`.
    pub super_: ProcessTable,
}

/// Scan-vtable glue (the `TableClass` slots) for `PCPProcessTable`, each
/// downcasting the base `*mut Table` and delegating — the DragonFly/FreeBSD
/// precedent. `prepare`/`cleanup` reuse the shared base entry points.
impl PCPProcessTable {
    fn scan_prepare(super_: *mut Table) {
        let this = super_ as *mut PCPProcessTable;
        // SAFETY: `super_` is the base of a live `PCPProcessTable`.
        ProcessTable_prepareEntries(unsafe { &mut (*this).super_ });
    }

    fn scan_iterate(super_: *mut Table) {
        let this = super_ as *mut PCPProcessTable;
        // SAFETY: `super_` is the base of a live `PCPProcessTable`.
        ProcessTable_goThroughEntries(unsafe { &mut *this });
    }

    fn scan_cleanup(super_: *mut Table) {
        let this = super_ as *mut PCPProcessTable;
        // SAFETY: `super_` is the base of a live `PCPProcessTable`.
        ProcessTable_cleanupEntries(unsafe { &mut (*this).super_ });
    }
}

/// The scan-vtable half of `ProcessTable_class` as it applies to the PCP table:
/// `iterate` link-resolves to [`ProcessTable_goThroughEntries`]; the C
/// `ProcessTable_new` wires this via `Object_setClass`.
pub static PCPProcessTable_class: TableClass = TableClass {
    prepare: Some(PCPProcessTable::scan_prepare),
    iterate: Some(PCPProcessTable::scan_iterate),
    cleanup: Some(PCPProcessTable::scan_cleanup),
};

/// Port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` (`PCPProcessTable.c:35`). Allocates the table (C `xCalloc`),
/// runs the base [`ProcessTable_init`] with the `PCPProcess` element class, and
/// wires the scan vtable (the C `Object_setClass(this, Class(ProcessTable))`).
/// Returns the owning `Box` (C returns `&this->super`); PCP adds no fields.
pub fn ProcessTable_new(host: *const Machine, pidMatchList: Option<usize>) -> Box<PCPProcessTable> {
    let mut this = Box::new(PCPProcessTable {
        super_: ProcessTable::empty(),
    });

    ProcessTable_init(&mut this.super_, host, pidMatchList);

    this.super_.super_.klass = &PCPProcessTable_class as *const TableClass;

    this
}

/// Port of `void ProcessTable_delete(Object* cast)` (`PCPProcessTable.c:45`).
/// The C body is `ProcessTable_done(&this->super)` then `free(this)`. Take `this`
/// by value: `ProcessTable_done` tears the base table down in place and `this`
/// drops at scope end (the `free(this)`) — the DragonFly precedent.
pub fn ProcessTable_delete(mut this: PCPProcessTable) {
    ProcessTable_done(&mut this.super_);
}

/// Port of `static inline long Metric_instance_s32(int metric, int pid, int
/// offset, long fallback)` (`PCPProcessTable.c:51`). Reads the `PM_TYPE_32`
/// value (`atom.l`) or returns `fallback`.
fn Metric_instance_s32(metric: Metric, pid: c_int, offset: c_int, fallback: i64) -> i64 {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance(metric, pid, offset, &mut value, PM_TYPE_32).is_null() {
        return unsafe { value.l } as i64;
    }
    fallback
}

/// Port of `static inline long long Metric_instance_s64(int metric, int pid, int
/// offset, long long fallback)` (`PCPProcessTable.c:58`). Reads `atom.l` (the C
/// reads the 32-bit union field even for `PM_TYPE_64` — faithful to htop) or
/// returns `fallback`.
fn Metric_instance_s64(metric: Metric, pid: c_int, offset: c_int, fallback: i64) -> i64 {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance(
        metric,
        pid,
        offset,
        &mut value,
        crate::ported::pcp::pmapi::PM_TYPE_64,
    )
    .is_null()
    {
        return unsafe { value.l } as i64;
    }
    fallback
}

/// Port of `static inline unsigned long Metric_instance_u32(int metric, int pid,
/// int offset, unsigned long fallback)` (`PCPProcessTable.c:65`). Reads the
/// `PM_TYPE_U32` value (`atom.ul`) or returns `fallback`.
fn Metric_instance_u32(metric: Metric, pid: c_int, offset: c_int, fallback: u64) -> u64 {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance(metric, pid, offset, &mut value, PM_TYPE_U32).is_null() {
        return unsafe { value.ul } as u64;
    }
    fallback
}

/// Port of `static inline unsigned long long Metric_instance_u64(int metric, int
/// pid, int offset, unsigned long long fallback)` (`PCPProcessTable.c:72`). Reads
/// the `PM_TYPE_U64` value (`atom.ull`) or returns `fallback`.
fn Metric_instance_u64(metric: Metric, pid: c_int, offset: c_int, fallback: u64) -> u64 {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance(metric, pid, offset, &mut value, PM_TYPE_U64).is_null() {
        return unsafe { value.ull };
    }
    fallback
}

/// Port of `static inline unsigned long long Metric_instance_time(int metric, int
/// pid, int offset)` (`PCPProcessTable.c:79`). Fetches the metric rescaled to
/// milliseconds then divides by 10 (centiseconds, used by the callers); 0 on
/// miss.
fn Metric_instance_time(metric: Metric, pid: c_int, offset: c_int) -> u64 {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance_milliseconds(metric, pid, offset, &mut value).is_null() {
        return unsafe { value.ull } / 10;
    }
    0
}

/// Port of `static inline unsigned long long Metric_instance_ONE_K(int metric,
/// int pid, int offset)` (`PCPProcessTable.c:86`). Fetches the metric rescaled to
/// kibibytes (`atom.ull`); `ULLONG_MAX` on miss.
fn Metric_instance_ONE_K(metric: Metric, pid: c_int, offset: c_int) -> u64 {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance_kibibytes(metric, pid, offset, &mut value).is_null() {
        return unsafe { value.ull };
    }
    u64::MAX
}

/// Port of `static inline char Metric_instance_char(int metric, int pid, int
/// offset, char fallback)` (`PCPProcessTable.c:93`). Reads the first char of the
/// `PM_TYPE_STRING` value (`atom.cp[0]`), frees the string, and returns it;
/// `fallback` on miss.
fn Metric_instance_char(metric: Metric, pid: c_int, offset: c_int, fallback: c_char) -> c_char {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance(metric, pid, offset, &mut value, PM_TYPE_STRING).is_null() {
        unsafe {
            let uchar = *value.cp;
            libc::free(value.cp as *mut c_void);
            return uchar;
        }
    }
    fallback
}

/// Port of `static char* setUser(UsersTable* this, unsigned int uid, int pid, int
/// offset)` (`PCPProcessTable.c:103`). Returns the cached username for `uid`, or
/// on a miss reads it from the `PCP_PROC_ID_USER` string metric, caches it, and
/// returns it. The C stores the libpcp-`malloc`'d `value.cp` in the hashtable;
/// here the owning `HashMap<u32, String>` takes an owned copy and the libpcp
/// buffer is freed (the `Option<String>` cache model — the same copy+free the
/// `Metric` layer uses for extracted strings). `NULL` (miss) is `None`.
fn setUser(this: &mut UsersTable, uid: u32, pid: c_int, offset: c_int) -> Option<String> {
    if let Some(name) = this.users.get(&uid) {
        return Some(name.clone());
    }

    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance(PCP_PROC_ID_USER, pid, offset, &mut value, PM_TYPE_STRING).is_null() {
        unsafe {
            let name = CStr::from_ptr(value.cp).to_string_lossy().into_owned();
            libc::free(value.cp as *mut c_void);
            this.users.insert(uid, name.clone());
            return Some(name);
        }
    }
    None
}

/// Port of `static inline ProcessState PCPProcessTable_getProcessState(char
/// state)` (`PCPProcessTable.c:116`). Maps the psinfo state char to the shared
/// [`ProcessState`]; any unknown char is `UNKNOWN`.
fn PCPProcessTable_getProcessState(state: c_char) -> ProcessState {
    match state as u8 {
        b'?' => ProcessState::UNKNOWN,
        b'R' => ProcessState::RUNNING,
        b'W' => ProcessState::WAITING,
        b'D' => ProcessState::UNINTERRUPTIBLE_WAIT,
        b'P' => ProcessState::PAGING,
        b'T' => ProcessState::STOPPED,
        b't' => ProcessState::TRACED,
        b'Z' => ProcessState::ZOMBIE,
        b'X' => ProcessState::DEFUNCT,
        b'I' => ProcessState::IDLE,
        b'S' => ProcessState::SLEEPING,
        _ => ProcessState::UNKNOWN,
    }
}

/// Port of `static void PCPProcessTable_updateID(Process* process, int pid, int
/// offset)` (`PCPProcessTable.c:133`). Sets the thread-group/parent pids and the
/// process state from their psinfo metrics.
fn PCPProcessTable_updateID(process: &mut Process, pid: c_int, offset: c_int) {
    Process_setThreadGroup(
        process,
        Metric_instance_u32(PCP_PROC_TGID, pid, offset, 1) as i32,
    );
    Process_setParent(
        process,
        Metric_instance_u32(PCP_PROC_PPID, pid, offset, 1) as i32,
    );
    process.state = PCPProcessTable_getProcessState(Metric_instance_char(
        PCP_PROC_STATE,
        pid,
        offset,
        b'?' as c_char,
    ));
}

/// Port of `static void PCPProcessTable_updateInfo(PCPProcess* pp, int pid, int
/// offset, char* command, size_t commLen)` (`PCPProcessTable.c:139`). Copies the
/// short command (or `"<unknown>"`) into `command` and refreshes the pgrp /
/// session / tty / fault / cpu-time / priority / nice / thread-count / starttime
/// psinfo fields, then `time = utime + stime`.
fn PCPProcessTable_updateInfo(pp: &mut PCPProcess, pid: c_int, offset: c_int, command: &mut [u8]) {
    // if (!Metric_instance(PCP_PROC_CMD, ...)) value.cp = xStrdup("<unknown>");
    // String_safeStrncpy(command, value.cp, commLen); free(value.cp);
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance(PCP_PROC_CMD, pid, offset, &mut value, PM_TYPE_STRING).is_null() {
        unsafe {
            String_safeStrncpy(command, CStr::from_ptr(value.cp).to_bytes());
            libc::free(value.cp as *mut c_void);
        }
    } else {
        String_safeStrncpy(command, b"<unknown>");
    }

    let process = &mut pp.super_;
    process.pgrp = Metric_instance_u32(PCP_PROC_PGRP, pid, offset, 0) as i32;
    process.session = Metric_instance_u32(PCP_PROC_SESSION, pid, offset, 0) as i32;
    process.tty_nr = Metric_instance_u32(PCP_PROC_TTY, pid, offset, 0);
    process.tpgid = Metric_instance_u32(PCP_PROC_TTYPGRP, pid, offset, 0) as i32;
    process.minflt = Metric_instance_u32(PCP_PROC_MINFLT, pid, offset, 0);
    pp.cminflt = Metric_instance_u32(PCP_PROC_CMINFLT, pid, offset, 0);
    pp.super_.majflt = Metric_instance_u32(PCP_PROC_MAJFLT, pid, offset, 0);
    pp.cmajflt = Metric_instance_u32(PCP_PROC_CMAJFLT, pid, offset, 0);
    pp.utime = Metric_instance_time(PCP_PROC_UTIME, pid, offset);
    pp.stime = Metric_instance_time(PCP_PROC_STIME, pid, offset);
    pp.cutime = Metric_instance_time(PCP_PROC_CUTIME, pid, offset);
    pp.cstime = Metric_instance_time(PCP_PROC_CSTIME, pid, offset);
    pp.super_.priority = Metric_instance_u32(PCP_PROC_PRIORITY, pid, offset, 0) as i64;
    pp.super_.nice = Metric_instance_s32(PCP_PROC_NICE, pid, offset, 0) as i32;
    pp.super_.nlwp = Metric_instance_u32(PCP_PROC_THREADS, pid, offset, 0) as i64;
    pp.super_.starttime_ctime = Metric_instance_time(PCP_PROC_STARTTIME, pid, offset) as i64;
    pp.super_.processor = Metric_instance_u32(PCP_PROC_PROCESSOR, pid, offset, 0) as i32;

    pp.super_.time = pp.utime + pp.stime;
}

/// Port of `static void PCPProcessTable_updateIO(PCPProcess* pp, int pid, int
/// offset, unsigned long long now)` (`PCPProcessTable.c:169`). Refreshes the
/// rchar/wchar/syscr/syscw/cancelled counters and derives the read/write byte
/// rates from the delta since the last scan (`now` in ms); a miss (or zero-time
/// delta) sets the sentinel `ULLONG_MAX` / `NAN`. Unsigned arithmetic wraps (C).
fn PCPProcessTable_updateIO(pp: &mut PCPProcess, pid: c_int, offset: c_int, now: u64) {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };

    pp.io_rchar = Metric_instance_ONE_K(PCP_PROC_IO_RCHAR, pid, offset);
    pp.io_wchar = Metric_instance_ONE_K(PCP_PROC_IO_WCHAR, pid, offset);
    pp.io_syscr = Metric_instance_u64(PCP_PROC_IO_SYSCR, pid, offset, u64::MAX);
    pp.io_syscw = Metric_instance_u64(PCP_PROC_IO_SYSCW, pid, offset, u64::MAX);
    pp.io_cancelled_write_bytes = Metric_instance_ONE_K(PCP_PROC_IO_CANCELLED, pid, offset);

    if !Metric_instance(PCP_PROC_IO_READB, pid, offset, &mut value, PM_TYPE_U64).is_null()
        && now.wrapping_sub(pp.io_last_scan_time) != 0
    {
        let last_read = pp.io_read_bytes;
        pp.io_read_bytes = unsafe { value.ull } / ONE_K;
        pp.io_rate_read_bps = (ONE_K.wrapping_mul(pp.io_read_bytes.wrapping_sub(last_read))
            / now.wrapping_sub(pp.io_last_scan_time)) as f64;
    } else {
        pp.io_read_bytes = u64::MAX;
        pp.io_rate_read_bps = f64::NAN;
    }

    if !Metric_instance(PCP_PROC_IO_WRITEB, pid, offset, &mut value, PM_TYPE_U64).is_null()
        && now.wrapping_sub(pp.io_last_scan_time) != 0
    {
        let last_write = pp.io_write_bytes;
        pp.io_write_bytes = unsafe { value.ull };
        pp.io_rate_write_bps = (ONE_K.wrapping_mul(pp.io_write_bytes.wrapping_sub(last_write))
            / now.wrapping_sub(pp.io_last_scan_time)) as f64;
    } else {
        pp.io_write_bytes = u64::MAX;
        pp.io_rate_write_bps = f64::NAN;
    }

    pp.io_last_scan_time = now;
}

/// Port of `static void PCPProcessTable_updateMemory(PCPProcess* pp, int pid, int
/// offset)` (`PCPProcessTable.c:203`). Refreshes the virt/rss/share/text/lib/data
/// /dirty memory fields (all in kB), deriving `m_priv = m_resident - m_share`.
fn PCPProcessTable_updateMemory(pp: &mut PCPProcess, pid: c_int, offset: c_int) {
    pp.super_.m_virt = Metric_instance_u32(PCP_PROC_MEM_SIZE, pid, offset, 0) as i64;
    pp.super_.m_resident = Metric_instance_u32(PCP_PROC_MEM_RSS, pid, offset, 0) as i64;
    pp.m_share = Metric_instance_u32(PCP_PROC_MEM_SHARE, pid, offset, 0) as i64;
    pp.m_priv = pp.super_.m_resident - pp.m_share;
    pp.m_trs = Metric_instance_u32(PCP_PROC_MEM_TEXTRS, pid, offset, 0) as i64;
    pp.m_lrs = Metric_instance_u32(PCP_PROC_MEM_LIBRS, pid, offset, 0) as i64;
    pp.m_drs = Metric_instance_u32(PCP_PROC_MEM_DATRS, pid, offset, 0) as i64;
    pp.m_dt = Metric_instance_u32(PCP_PROC_MEM_DIRTY, pid, offset, 0) as i64;
}

/// Port of `static void PCPProcessTable_updateSmaps(PCPProcess* pp, pid_t pid, int
/// offset)` (`PCPProcessTable.c:214`). Refreshes the smaps pss / swap / swappss.
fn PCPProcessTable_updateSmaps(pp: &mut PCPProcess, pid: c_int, offset: c_int) {
    pp.m_pss = Metric_instance_u64(PCP_PROC_SMAPS_PSS, pid, offset, 0) as i64;
    pp.m_swap = Metric_instance_u64(PCP_PROC_SMAPS_SWAP, pid, offset, 0) as i64;
    pp.m_psswp = Metric_instance_u64(PCP_PROC_SMAPS_SWAPPSS, pid, offset, 0) as i64;
}

/// Port of `static void PCPProcessTable_readOomData(PCPProcess* pp, int pid, int
/// offset)` (`PCPProcessTable.c:220`). Refreshes the OOM killer score.
fn PCPProcessTable_readOomData(pp: &mut PCPProcess, pid: c_int, offset: c_int) {
    pp.oom = Metric_instance_u32(PCP_PROC_OOMSCORE, pid, offset, 0) as u32;
}

/// Port of `static void PCPProcessTable_readAutogroup(PCPProcess* pp, int pid, int
/// offset)` (`PCPProcessTable.c:224`). Refreshes the autogroup id / nice.
fn PCPProcessTable_readAutogroup(pp: &mut PCPProcess, pid: c_int, offset: c_int) {
    pp.autogroup_id = Metric_instance_s64(PCP_PROC_AUTOGROUP_ID, pid, offset, -1);
    pp.autogroup_nice = Metric_instance_s32(PCP_PROC_AUTOGROUP_NICE, pid, offset, 0) as i32;
}

/// Port of `static void PCPProcessTable_readCtxtData(PCPProcess* pp, int pid, int
/// offset)` (`PCPProcessTable.c:229`). Sums the voluntary + non-voluntary context
/// switches, tracking the per-scan delta (clamped at 0).
fn PCPProcessTable_readCtxtData(pp: &mut PCPProcess, pid: c_int, offset: c_int) {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    let mut ctxt: u64 = 0;

    if !Metric_instance(PCP_PROC_VCTXSW, pid, offset, &mut value, PM_TYPE_U32).is_null() {
        ctxt += unsafe { value.ul } as u64;
    }
    if !Metric_instance(PCP_PROC_NVCTXSW, pid, offset, &mut value, PM_TYPE_U32).is_null() {
        ctxt += unsafe { value.ul } as u64;
    }

    pp.ctxt_diff = if ctxt > pp.ctxt_total {
        ctxt - pp.ctxt_total
    } else {
        0
    };
    pp.ctxt_total = ctxt;
}

/// Port of `static char* setString(Metric metric, int pid, int offset, char*
/// string)` (`PCPProcessTable.c:242`). Frees the old string (the caller's `=`
/// assignment drops the old `Option<String>`) and returns the metric's string
/// value, or `None` on a miss. The libpcp-`malloc`'d buffer is copied into an
/// owned `String` and freed (the `Option<String>` model).
fn setString(metric: Metric, pid: c_int, offset: c_int) -> Option<String> {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance(metric, pid, offset, &mut value, PM_TYPE_STRING).is_null() {
        unsafe {
            let s = CStr::from_ptr(value.cp).to_string_lossy().into_owned();
            libc::free(value.cp as *mut c_void);
            Some(s)
        }
    } else {
        None
    }
}

/// Port of `static void PCPProcessTable_updateTTY(Process* process, int pid, int
/// offset)` (`PCPProcessTable.c:253`). Refreshes the tty name string.
fn PCPProcessTable_updateTTY(process: &mut Process, pid: c_int, offset: c_int) {
    process.tty_name = setString(PCP_PROC_TTYNAME, pid, offset);
}

/// Port of `static void PCPProcessTable_readCGroups(PCPProcess* pp, int pid, int
/// offset)` (`PCPProcessTable.c:257`). Refreshes the raw cgroup string, then the
/// filtered `cgroup_short` / `container_short` (falling back to the raw cgroup /
/// `"N/A"` width when the filter yields nothing), updating the CCGROUP /
/// CONTAINER column widths.
fn PCPProcessTable_readCGroups(pp: &mut PCPProcess, pid: c_int, offset: c_int) {
    pp.cgroup = setString(PCP_PROC_CGROUPS, pid, offset);

    // Own a copy of the raw cgroup so `pp` can be mutated below (the C reads
    // `pp->cgroup` and writes `pp->cgroup_short`/`container_short` in place).
    let cgroup = pp.cgroup.clone();
    if let Some(cgroup) = cgroup.as_deref() {
        match CGroup_filterName(cgroup) {
            Some(cgroup_short) => {
                Row_updateFieldWidth(ProcessField::CCGROUP as RowField, cgroup_short.len());
                pp.cgroup_short = Some(cgroup_short);
            }
            None => {
                // CCGROUP is alias to normal CGROUP if shortening fails
                Row_updateFieldWidth(ProcessField::CCGROUP as RowField, cgroup.len());
                pp.cgroup_short = None;
            }
        }

        match CGroup_filterName(cgroup) {
            Some(container_short) => {
                Row_updateFieldWidth(ProcessField::CONTAINER as RowField, container_short.len());
                pp.container_short = Some(container_short);
            }
            None => {
                Row_updateFieldWidth(ProcessField::CONTAINER as RowField, "N/A".len());
                pp.container_short = None;
            }
        }
    } else {
        pp.cgroup_short = None;
        pp.container_short = None;
    }
}

/// Port of `static void PCPProcessTable_readSecattrData(PCPProcess* pp, int pid,
/// int offset)` (`PCPProcessTable.c:292`). Refreshes the security-attribute
/// string.
fn PCPProcessTable_readSecattrData(pp: &mut PCPProcess, pid: c_int, offset: c_int) {
    pp.secattr = setString(PCP_PROC_LABELS, pid, offset);
}

/// Port of `static void PCPProcessTable_readCwd(PCPProcess* pp, int pid, int
/// offset)` (`PCPProcessTable.c:296`). Refreshes the current-working-directory
/// string.
fn PCPProcessTable_readCwd(pp: &mut PCPProcess, pid: c_int, offset: c_int) {
    pp.super_.procCwd = setString(PCP_PROC_CWD, pid, offset);
}

/// Port of `static void PCPProcessTable_updateUsername(Process* process, int pid,
/// int offset, UsersTable* users)` (`PCPProcessTable.c:300`). Refreshes the
/// process uid and resolves its username via [`setUser`]. `users` is the
/// machine's opaque `usersTable` handle (a separately-leaked `*mut UsersTable`,
/// the linux/darwin precedent); a `None` handle leaves the user unresolved.
fn PCPProcessTable_updateUsername(
    process: &mut Process,
    pid: c_int,
    offset: c_int,
    users: Option<usize>,
) {
    process.st_uid = Metric_instance_u32(PCP_PROC_ID_UID, pid, offset, 0) as u32;
    process.user = match users {
        Some(ut) => {
            // SAFETY: `usersTable` is the machine's leaked `*mut UsersTable`
            // handle; it is a distinct allocation, so the `&mut` cannot alias
            // the borrowed `process`.
            let ut = unsafe { &mut *(ut as *mut UsersTable) };
            setUser(ut, process.st_uid, pid, offset)
        }
        None => None,
    };
}

/// Port of `static void PCPProcessTable_updateCmdline(Process* process, int pid,
/// int offset, const char* comm)` (`PCPProcessTable.c:305`). Reads the psargs
/// string: a miss marks a (non-zombie) process a kernel thread and clears the
/// cmdline; otherwise it strips a `(...)`-wrapped kernel-thread name, computes
/// the basename window (`tokenStart` after the last `/`, reset to 0 when the
/// args contain whitespace), sets the cmdline / comm, and refreshes the exe.
fn PCPProcessTable_updateCmdline(process: &mut Process, pid: c_int, offset: c_int, comm: &str) {
    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if Metric_instance(PCP_PROC_PSARGS, pid, offset, &mut value, PM_TYPE_STRING).is_null() {
        if process.state != ProcessState::ZOMBIE {
            process.isKernelThread = true;
        }
        Process_updateCmdline(process, None, 0, 0);
        return;
    }

    // char* command = value.cp; size_t length = strlen(command);
    let raw: Vec<u8> = unsafe { CStr::from_ptr(value.cp).to_bytes().to_vec() };
    unsafe {
        libc::free(value.cp as *mut c_void);
    }

    let mut start = 0usize;
    let mut length = raw.len();
    if raw.first().copied() != Some(b'(') {
        process.isKernelThread = false;
    } else {
        // if (command[length - 1] == ')') command[--length] = '\0';
        if length > 0 && raw[length - 1] == b')' {
            length -= 1;
        }
        // ++command; --length;
        start += 1;
        length -= 1;
        process.isKernelThread = true;
    }

    let command = &raw[start..start + length];

    let mut token_start = 0usize;
    let mut arg_sep_space = false;
    for (i, &c) in command.iter().enumerate() {
        // next character after the last '/' is the basename start
        if c == b'/' {
            token_start = i + 1;
        }
        // special-case arguments for problematic situations like "find /"
        if c <= b' ' {
            arg_sep_space = true;
        }
    }
    let token_end = length;
    if arg_sep_space {
        token_start = 0;
    }

    let command_str = String::from_utf8_lossy(command).into_owned();
    Process_updateCmdline(process, Some(&command_str), token_start, token_end);

    Process_updateComm(process, Some(comm));

    let mut value: pmAtomValue = unsafe { core::mem::zeroed() };
    if !Metric_instance(PCP_PROC_EXE, pid, offset, &mut value, PM_TYPE_STRING).is_null() {
        unsafe {
            let bytes = CStr::from_ptr(value.cp).to_bytes();
            if bytes.is_empty() {
                Process_updateExe(process, None);
            } else {
                let s = String::from_utf8_lossy(bytes).into_owned();
                Process_updateExe(process, Some(&s));
            }
            libc::free(value.cp as *mut c_void);
        }
    }
}

/// Port of `static bool PCPProcessTable_updateProcesses(PCPProcessTable* this)`
/// (`PCPProcessTable.c:355`). Iterates every `proc.psinfo.pid` instance, building
/// or refreshing its [`PCPProcess`] and running the per-flag update passes, then
/// tallies the total / running / kernel / userland counters.
///
/// Mirrors the DragonFly scan's ownership model: [`ProcessTable_getProcess`]
/// returns `(preExisting, idx)` and registers the fresh row itself (so no
/// separate `ProcessTable_add`); a raw `*mut PCPProcess` is downcast from
/// `rows[idx]` and a `*mut PCPProcessTable` is taken so the disjoint per-table
/// counter writes don't alias the per-process writes.
fn PCPProcessTable_updateProcesses(this: &mut PCPProcessTable) -> bool {
    let host = this.super_.super_.host;
    let phost = host as *const PCPMachine;

    // const Settings* settings = host->settings; ... flags = settings->ss->flags
    let (hideKernelThreads, hideUserlandThreads, updateProcessNames, showThreadNames, flags) = unsafe {
        let settings = (*host)
            .settings
            .as_ref()
            .expect("PCPProcessTable_updateProcesses: host->settings (C dereferences it)");
        (
            settings.hideKernelThreads,
            settings.hideUserlandThreads,
            settings.updateProcessNames,
            settings.showThreadNames,
            settings.screens[settings.ssIndex as usize].flags,
        )
    };

    let now = unsafe { ((*phost).timestamp * 1000.0) as u64 };
    let mut pid: c_int = -1;
    let mut offset: c_int = -1;

    // for every process ...
    while Metric_iterate(
        PCP_PROC_PID,
        &mut pid,
        &mut offset,
        core::mem::size_of::<PCPProcess>(),
    ) {
        let (preExisting, idx) = ProcessTable_getProcess(&mut this.super_, pid, |h| {
            Box::new(PCPProcess_new(h)) as Box<dyn Object>
        });

        // Recover a raw `*mut PCPProcess` for this row (checked borrow ends
        // here). `Object: Any` → downcast to the concrete row type.
        let pp: *mut PCPProcess = {
            let obj: &mut dyn Object = this.super_.super_.rows[idx].as_mut().unwrap().as_mut();
            (obj as &mut dyn Any).downcast_mut::<PCPProcess>().unwrap()
        };
        let ppt = this as *mut PCPProcessTable;

        unsafe {
            let proc: *mut Process = &mut (*pp).super_;

            PCPProcessTable_updateID(&mut *proc, pid, offset);
            (*proc).isUserlandThread = Process_getPid(&*proc) != Process_getThreadGroup(&*proc);
            (*pp).offset = if offset >= 0 { offset as u32 } else { 0 };

            // These conditions short-circuit subsequent scans of a hidden thread
            // (they never trigger on first occurrence — see the C comment).
            if preExisting && hideKernelThreads && Process_isKernelThread(&*proc) {
                (*proc).super_.updated = true;
                (*proc).super_.show = false;
                if (*proc).state == ProcessState::RUNNING {
                    (*ppt).super_.runningTasks += 1;
                }
                (*ppt).super_.kernelThreads += 1;
                (*ppt).super_.totalTasks += 1;
                continue;
            }
            if preExisting && hideUserlandThreads && Process_isUserlandThread(&*proc) {
                (*proc).super_.updated = true;
                (*proc).super_.show = false;
                if (*proc).state == ProcessState::RUNNING {
                    (*ppt).super_.runningTasks += 1;
                }
                (*ppt).super_.userlandThreads += 1;
                (*ppt).super_.totalTasks += 1;
                continue;
            }

            if flags & PROCESS_FLAG_IO != 0 {
                PCPProcessTable_updateIO(&mut *pp, pid, offset, now);
            }

            PCPProcessTable_updateMemory(&mut *pp, pid, offset);

            if (flags & PROCESS_FLAG_LINUX_SMAPS != 0)
                && !Process_isKernelThread(&*proc)
                && Metric_enabled(PCP_PROC_SMAPS_PSS)
            {
                PCPProcessTable_updateSmaps(&mut *pp, pid, offset);
            }

            let mut command = [0u8; MAX_NAME + 1];
            let tty_nr = (*proc).tty_nr;
            let lasttimes = (*pp).utime + (*pp).stime;

            PCPProcessTable_updateInfo(&mut *pp, pid, offset, &mut command);
            (*proc).starttime_ctime += Platform_getBootTime() as i64;
            if tty_nr != (*proc).tty_nr {
                PCPProcessTable_updateTTY(&mut *proc, pid, offset);
            }

            // The C passes the `char command[]` pointer (NUL-terminated) to
            // `Process_updateCmdline`/`updateComm`; carry it as an owned lossy
            // `String` of the bytes up to the NUL.
            let command_str = {
                let end = command
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(command.len());
                String::from_utf8_lossy(&command[..end]).into_owned()
            };

            (*proc).percent_cpu = f32::NAN;
            let period = (*phost).period;
            if period > 0.0 {
                let percent_cpu = (saturatingSub((*pp).utime + (*pp).stime, lasttimes) as f64
                    / period
                    * 100.0) as f32;
                // MINIMUM(percent_cpu, host->activeCPUs * 100.0F) — C `a < b ? a : b`.
                let cap = (*host).activeCPUs as f32 * 100.0_f32;
                (*proc).percent_cpu = if percent_cpu < cap { percent_cpu } else { cap };
            }
            (*proc).percent_mem =
                ((*proc).m_resident as f64 / (*host).totalMem as f64 * 100.0) as f32;
            Process_updateCPUFieldWidths((*proc).percent_cpu);

            PCPProcessTable_updateUsername(&mut *proc, pid, offset, (*host).usersTable);

            if !preExisting {
                PCPProcessTable_updateCmdline(&mut *proc, pid, offset, &command_str);
                Process_fillStarttimeBuffer(&mut *proc);
                // ProcessTable_add — getProcess already registered the row.
            } else if updateProcessNames && (*proc).state != ProcessState::ZOMBIE {
                PCPProcessTable_updateCmdline(&mut *proc, pid, offset, &command_str);
            }

            if flags & PROCESS_FLAG_LINUX_CGROUP != 0 {
                PCPProcessTable_readCGroups(&mut *pp, pid, offset);
            }
            if flags & PROCESS_FLAG_LINUX_OOM != 0 {
                PCPProcessTable_readOomData(&mut *pp, pid, offset);
            }
            if flags & PROCESS_FLAG_LINUX_CTXT != 0 {
                PCPProcessTable_readCtxtData(&mut *pp, pid, offset);
            }
            if flags & PROCESS_FLAG_LINUX_SECATTR != 0 {
                PCPProcessTable_readSecattrData(&mut *pp, pid, offset);
            }
            if flags & PROCESS_FLAG_CWD != 0 {
                PCPProcessTable_readCwd(&mut *pp, pid, offset);
            }
            if flags & PROCESS_FLAG_LINUX_AUTOGROUP != 0 {
                PCPProcessTable_readAutogroup(&mut *pp, pid, offset);
            }

            if (*proc).state == ProcessState::ZOMBIE && (*proc).cmdline.is_none() && command[0] != 0
            {
                Process_updateCmdline(&mut *proc, Some(&command_str), 0, command_str.len());
            } else if Process_isThread(&*proc) {
                if (showThreadNames || Process_isKernelThread(&*proc)) && command[0] != 0 {
                    Process_updateCmdline(&mut *proc, Some(&command_str), 0, command_str.len());
                }

                if Process_isKernelThread(&*proc) {
                    (*ppt).super_.kernelThreads += 1;
                } else {
                    (*ppt).super_.userlandThreads += 1;
                }
            }

            // Set at the end when we know if a new entry is a thread.
            let is_kernel = Process_isKernelThread(&*proc);
            let is_userland = Process_isUserlandThread(&*proc);
            (*proc).super_.show =
                !((hideKernelThreads && is_kernel) || (hideUserlandThreads && is_userland));

            (*ppt).super_.totalTasks += 1;
            if (*proc).state == ProcessState::RUNNING {
                (*ppt).super_.runningTasks += 1;
            }
            (*proc).super_.updated = true;
        }
    }
    true
}

/// Port of `void ProcessTable_goThroughEntries(ProcessTable* super)`
/// (`PCPProcessTable.c:485`). Downcasts to the PCP table and delegates to
/// [`PCPProcessTable_updateProcesses`].
pub fn ProcessTable_goThroughEntries(this: &mut PCPProcessTable) {
    PCPProcessTable_updateProcesses(this);
}
