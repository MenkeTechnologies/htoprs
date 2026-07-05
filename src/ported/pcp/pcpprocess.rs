//! Port of `pcp/PCPProcess.c` + `.h` — the Performance Co-Pilot per-process
//! model (`PCPProcess`, which "extends" [`Process`]) and its `ProcessClass`
//! vtable.
//!
//! Pure module: no libpcp/pmapi. It reuses the shared [`Process`]/[`Row`]
//! object model, the `ProcessClass`/`RowClass` vtables, the ported
//! `Process_row*` slots, and [`Process_writeField`]/[`Process_compareByKey_Base`].
//!
//! Field ids: PCP's `PLATFORM_PROCESS_FIELDS` (`pcp/ProcessField.h`) is the
//! Linux superset, so every PCP field id lives in the shared [`ProcessField`]
//! enum — used directly here — with one exception: `M_DT = 45`, which PCP
//! defines but the Linux platform set (and thus the shared enum) omits. It is
//! carried as a module-local [`RowField`] constant, exactly as DragonFly
//! carries `JID`/`JAIL`. The `PROCESS_FLAG_LINUX_*` scan flags are the same
//! ports LinuxProcess already declares, imported rather than re-defined.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::any::Any;
use core::ffi::c_void;
use core::ops::Deref;

use crate::ported::crt::{ColorElements as CE, ColorScheme, A_BOLD};
use crate::ported::linux::linuxprocess::{
    PROCESS_FLAG_LINUX_AUTOGROUP, PROCESS_FLAG_LINUX_CGROUP, PROCESS_FLAG_LINUX_CTXT,
    PROCESS_FLAG_LINUX_OOM, PROCESS_FLAG_LINUX_SECATTR, PROCESS_FLAG_LINUX_SMAPS,
};
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::process::{
    spaceship_nullstr, Process, ProcessClass, ProcessField, ProcessFieldData, Process_class,
    Process_compare, Process_compareByKey_Base, Process_compareByParent, Process_init,
    Process_rowGetSortKey, Process_rowIsHighlighted, Process_rowIsVisible, Process_writeField,
    PROCESS_FLAG_CWD, PROCESS_FLAG_IO,
};
use crate::ported::richstring::{RichString, RichString_appendWide};
use crate::ported::row::{
    spaceship_number, Row, RowClass, Row_display, Row_printBytes, Row_printCount, Row_printKBytes,
    Row_printRate, Row_printTime,
};
use crate::ported::settings::RowField;
use crate::ported::xutils::compareRealNumbers;

/// Port of `M_DT = 45` from `pcp/ProcessField.h` — the dirty-pages column.
/// Module-local: the shared [`ProcessField`] enum is built from the Linux
/// platform set, which omits `M_DT`; see the module docs.
pub const M_DT: RowField = 45;

/// Port of `#define LAST_PROCESSFIELD LAST_RESERVED_FIELD` for PCP. The last
/// entry of `PLATFORM_PROCESS_FIELDS` is `M_PRIV = 131`, so `LAST_RESERVED_FIELD`
/// (the implicit next enumerator) is `132`, and the table has `132` slots.
pub const LAST_PROCESSFIELD: usize = 132;

/// The unused index-0 slot and every gap between designated indices (C's
/// implicit zero-init of un-designated array entries).
const EMPTY_FIELD: ProcessFieldData = ProcessFieldData {
    name: "",
    title: None,
    description: None,
    flags: 0,
    pidColumn: false,
    defaultSortDesc: false,
    autoWidth: false,
    autoTitleRightAlign: false,
};

const fn pfd(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    flags: u32,
    pidColumn: bool,
    defaultSortDesc: bool,
    autoWidth: bool,
    autoTitleRightAlign: bool,
) -> ProcessFieldData {
    ProcessFieldData {
        name,
        title: Some(title),
        description: Some(description),
        flags,
        pidColumn,
        defaultSortDesc,
        autoWidth,
        autoTitleRightAlign,
    }
}

/// Port of `const ProcessFieldData Process_fields[]` from `pcp/PCPProcess.c` —
/// the PCP per-field metadata, indexed by field id. Trailing spaces in titles
/// are significant.
pub static Process_fields: [ProcessFieldData; LAST_PROCESSFIELD] = build_process_fields();

const fn build_process_fields() -> [ProcessFieldData; LAST_PROCESSFIELD] {
    use ProcessField as PF;
    let mut t = [EMPTY_FIELD; LAST_PROCESSFIELD];
    t[PF::PID as usize] = pfd(
        "PID",
        "PID",
        "Process/thread ID",
        0,
        true,
        false,
        false,
        false,
    );
    t[PF::COMM as usize] = pfd(
        "Command",
        "Command ",
        "Command line (insert as last column only)",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::STATE as usize] = pfd(
        "STATE",
        "S ",
        "Process state (S sleeping, R running, D disk, Z zombie, T traced, W paging, I idle)",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::PPID as usize] = pfd(
        "PPID",
        "PPID",
        "Parent process ID",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::PGRP as usize] = pfd(
        "PGRP",
        "PGRP",
        "Process group ID",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::SESSION as usize] = pfd(
        "SESSION",
        "SID",
        "Process's session ID",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::TTY as usize] = pfd(
        "TTY",
        "TTY      ",
        "Controlling terminal",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::TPGID as usize] = pfd(
        "TPGID",
        "TPGID",
        "Process ID of the fg process group of the controlling terminal",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::MINFLT as usize] = pfd(
        "MINFLT",
        "     MINFLT ",
        "Number of minor faults which have not required loading a memory page from disk",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::CMINFLT as usize] = pfd(
        "CMINFLT",
        "    CMINFLT ",
        "Children processes' minor faults",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::MAJFLT as usize] = pfd(
        "MAJFLT",
        "     MAJFLT ",
        "Number of major faults which have required loading a memory page from disk",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::CMAJFLT as usize] = pfd(
        "CMAJFLT",
        "    CMAJFLT ",
        "Children processes' major faults",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::UTIME as usize] = pfd(
        "UTIME",
        " UTIME+  ",
        "User CPU time - time the process spent executing in user mode",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::STIME as usize] = pfd(
        "STIME",
        " STIME+  ",
        "System CPU time - time the kernel spent running system calls for this process",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::CUTIME as usize] = pfd(
        "CUTIME",
        " CUTIME+ ",
        "Children processes' user CPU time",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::CSTIME as usize] = pfd(
        "CSTIME",
        " CSTIME+ ",
        "Children processes' system CPU time",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::PRIORITY as usize] = pfd(
        "PRIORITY",
        "PRI ",
        "Kernel's internal priority for the process",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::NICE as usize] = pfd(
        "NICE",
        " NI ",
        "Nice value (the higher the value, the more it lets other processes take priority)",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::STARTTIME as usize] = pfd(
        "STARTTIME",
        "START ",
        "Time the process was started",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::ELAPSED as usize] = pfd(
        "ELAPSED",
        "ELAPSED  ",
        "Time since the process was started",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::PROCESSOR as usize] = pfd(
        "PROCESSOR",
        "CPU ",
        "If of the CPU the process last executed on",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::M_VIRT as usize] = pfd(
        "M_VIRT",
        " VIRT ",
        "Total program size in virtual memory",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::M_RESIDENT as usize] = pfd(
        "M_RESIDENT",
        "  RES ",
        "Resident set size, size of the text and data sections, plus stack usage",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::M_SHARE as usize] = pfd(
        "M_SHARE",
        "  SHR ",
        "Size of the process's shared pages",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::M_PRIV as usize] = pfd(
        "M_PRIV",
        " PRIV ",
        "The private memory size of the process - resident set size minus shared memory",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::M_TRS as usize] = pfd(
        "M_TRS",
        " CODE ",
        "Size of the text segment of the process",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::M_DRS as usize] = pfd(
        "M_DRS",
        " DATA ",
        "Size of the data segment plus stack usage of the process",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::M_LRS as usize] = pfd(
        "M_LRS",
        "  LIB ",
        "The library size of the process (unused since Linux 2.6; always 0)",
        0,
        false,
        true,
        false,
        false,
    );
    t[M_DT as usize] = pfd(
        "M_DT",
        " DIRTY ",
        "Size of the dirty pages of the process (unused since Linux 2.6; always 0)",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::ST_UID as usize] = pfd(
        "ST_UID",
        "UID",
        "User ID of the process owner",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::PERCENT_CPU as usize] = pfd(
        "PERCENT_CPU",
        " CPU%",
        "Percentage of the CPU time the process used in the last sampling",
        0,
        false,
        true,
        true,
        true,
    );
    t[PF::PERCENT_NORM_CPU as usize] = pfd("PERCENT_NORM_CPU", "NCPU%", "Normalized percentage of the CPU time the process used in the last sampling (normalized by cpu count)", 0, false, true, true, false);
    t[PF::PERCENT_MEM as usize] = pfd(
        "PERCENT_MEM",
        "MEM% ",
        "Percentage of the memory the process is using, based on resident memory size",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::USER as usize] = pfd(
        "USER",
        "USER       ",
        "Username of the process owner (or user ID if name cannot be determined)",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::TIME as usize] = pfd(
        "TIME",
        "  TIME+  ",
        "Total time the process has spent in user and system time",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::NLWP as usize] = pfd(
        "NLWP",
        "NLWP ",
        "Number of threads in the process",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::TGID as usize] = pfd(
        "TGID",
        "TGID",
        "Thread group ID (i.e. process ID)",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::RCHAR as usize] = pfd(
        "RCHAR",
        "RCHAR ",
        "Number of bytes the process has read",
        PROCESS_FLAG_IO,
        false,
        true,
        false,
        false,
    );
    t[PF::WCHAR as usize] = pfd(
        "WCHAR",
        "WCHAR ",
        "Number of bytes the process has written",
        PROCESS_FLAG_IO,
        false,
        true,
        false,
        false,
    );
    t[PF::SYSCR as usize] = pfd(
        "SYSCR",
        "  READ_SYSC ",
        "Number of read(2) syscalls for the process",
        PROCESS_FLAG_IO,
        false,
        true,
        false,
        false,
    );
    t[PF::SYSCW as usize] = pfd(
        "SYSCW",
        " WRITE_SYSC ",
        "Number of write(2) syscalls for the process",
        PROCESS_FLAG_IO,
        false,
        true,
        false,
        false,
    );
    t[PF::RBYTES as usize] = pfd(
        "RBYTES",
        " IO_R ",
        "Bytes of read(2) I/O for the process",
        PROCESS_FLAG_IO,
        false,
        true,
        false,
        false,
    );
    t[PF::WBYTES as usize] = pfd(
        "WBYTES",
        " IO_W ",
        "Bytes of write(2) I/O for the process",
        PROCESS_FLAG_IO,
        false,
        true,
        false,
        false,
    );
    t[PF::CNCLWB as usize] = pfd(
        "CNCLWB",
        " IO_C ",
        "Bytes of cancelled write(2) I/O",
        PROCESS_FLAG_IO,
        false,
        true,
        false,
        false,
    );
    t[PF::IO_READ_RATE as usize] = pfd(
        "IO_READ_RATE",
        " DISK READ ",
        "The I/O rate of read(2) in bytes per second for the process",
        PROCESS_FLAG_IO,
        false,
        true,
        false,
        false,
    );
    t[PF::IO_WRITE_RATE as usize] = pfd(
        "IO_WRITE_RATE",
        " DISK WRITE ",
        "The I/O rate of write(2) in bytes per second for the process",
        PROCESS_FLAG_IO,
        false,
        true,
        false,
        false,
    );
    t[PF::IO_RATE as usize] = pfd(
        "IO_RATE",
        "   DISK R/W ",
        "Total I/O rate in bytes per second",
        PROCESS_FLAG_IO,
        false,
        true,
        false,
        false,
    );
    t[PF::CGROUP as usize] = pfd(
        "CGROUP",
        "CGROUP (raw)                        ",
        "Which cgroup the process is in",
        PROCESS_FLAG_LINUX_CGROUP,
        false,
        false,
        false,
        false,
    );
    t[PF::CCGROUP as usize] = pfd(
        "CCGROUP",
        "CGROUP (compressed)                 ",
        "Which cgroup the process is in (condensed to essentials)",
        PROCESS_FLAG_LINUX_CGROUP,
        false,
        false,
        false,
        false,
    );
    t[PF::CONTAINER as usize] = pfd(
        "CONTAINER",
        "CONTAINER                           ",
        "Name of the container the process is in (guessed by heuristics)",
        PROCESS_FLAG_LINUX_CGROUP,
        false,
        false,
        false,
        false,
    );
    t[PF::OOM as usize] = pfd(
        "OOM",
        " OOM ",
        "OOM (Out-of-Memory) killer score",
        PROCESS_FLAG_LINUX_OOM,
        false,
        true,
        false,
        false,
    );
    t[PF::PERCENT_CPU_DELAY as usize] = pfd(
        "PERCENT_CPU_DELAY",
        "CPUD% ",
        "CPU delay %",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::PERCENT_IO_DELAY as usize] = pfd(
        "PERCENT_IO_DELAY",
        " IOD% ",
        "Block I/O delay %",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::PERCENT_SWAP_DELAY as usize] = pfd(
        "PERCENT_SWAP_DELAY",
        "SWAPD% ",
        "Swapin delay %",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::M_PSS as usize] = pfd("M_PSS", "  PSS ", "proportional set size, same as M_RESIDENT but each page is divided by the number of processes sharing it.", PROCESS_FLAG_LINUX_SMAPS, false, true, false, false);
    t[PF::M_SWAP as usize] = pfd(
        "M_SWAP",
        " SWAP ",
        "Size of the process's swapped pages",
        PROCESS_FLAG_LINUX_SMAPS,
        false,
        true,
        false,
        false,
    );
    t[PF::M_PSSWP as usize] = pfd("M_PSSWP", " PSSWP ", "shows proportional swap share of this mapping, Unlike \"Swap\", this does not take into account swapped out page of underlying shmem objects.", PROCESS_FLAG_LINUX_SMAPS, false, true, false, false);
    t[PF::CTXT as usize] = pfd("CTXT", " CTXT ", "Context switches (incremental sum of voluntary_ctxt_switches and nonvoluntary_ctxt_switches)", PROCESS_FLAG_LINUX_CTXT, false, true, false, false);
    t[PF::SECATTR as usize] = pfd(
        "SECATTR",
        " Security Attribute ",
        "Security attribute of the process (e.g. SELinux or AppArmor)",
        PROCESS_FLAG_LINUX_SECATTR,
        false,
        false,
        false,
        false,
    );
    t[PF::PROC_COMM as usize] = pfd(
        "COMM",
        "COMM            ",
        "comm string of the process",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::PROC_EXE as usize] = pfd(
        "EXE",
        "EXE             ",
        "Basename of exe of the process",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::CWD as usize] = pfd(
        "CWD",
        "CWD                       ",
        "The current working directory of the process",
        PROCESS_FLAG_CWD,
        false,
        false,
        false,
        false,
    );
    t[PF::AUTOGROUP_ID as usize] = pfd(
        "AUTOGROUP_ID",
        "AGRP",
        "The autogroup identifier of the process",
        PROCESS_FLAG_LINUX_AUTOGROUP,
        false,
        false,
        false,
        false,
    );
    t[PF::AUTOGROUP_NICE as usize] = pfd("AUTOGROUP_NICE", " ANI", "Nice value (the higher the value, the more other processes take priority) associated with the process autogroup", PROCESS_FLAG_LINUX_AUTOGROUP, false, false, false, false);
    t
}

/// Port of `struct PCPProcess_` (`PCPProcess.h`). "Extends" [`Process`] via
/// the embedded `super_`, plus the PCP-specific fields. Owned `char*` fields
/// map to `Option<String>` (`None` = C `NULL`); the C `Process_delete` frees
/// `cgroup_short`/`cgroup`/`secattr`, which Rust's `Drop` of these
/// `Option<String>` fields handles automatically.
#[derive(Debug, Clone, Default)]
pub struct PCPProcess {
    /// C `Process super`.
    pub super_: Process,
    /// C `unsigned int offset` — default result offset for proc metrics.
    pub offset: u32,
    /// C `unsigned long int cminflt`.
    pub cminflt: u64,
    /// C `unsigned long int cmajflt`.
    pub cmajflt: u64,
    /// C `unsigned long long int utime`.
    pub utime: u64,
    /// C `unsigned long long int stime`.
    pub stime: u64,
    /// C `unsigned long long int cutime`.
    pub cutime: u64,
    /// C `unsigned long long int cstime`.
    pub cstime: u64,
    /// C `long m_share`.
    pub m_share: i64,
    /// C `long m_priv`.
    pub m_priv: i64,
    /// C `long m_pss`.
    pub m_pss: i64,
    /// C `long m_swap`.
    pub m_swap: i64,
    /// C `long m_psswp`.
    pub m_psswp: i64,
    /// C `long m_trs`.
    pub m_trs: i64,
    /// C `long m_drs`.
    pub m_drs: i64,
    /// C `long m_lrs`.
    pub m_lrs: i64,
    /// C `long m_dt`.
    pub m_dt: i64,
    /// C `unsigned long long io_rchar` — data read (in kilobytes).
    pub io_rchar: u64,
    /// C `unsigned long long io_wchar` — data written (in kilobytes).
    pub io_wchar: u64,
    /// C `unsigned long long io_syscr` — number of read(2) syscalls.
    pub io_syscr: u64,
    /// C `unsigned long long io_syscw` — number of write(2) syscalls.
    pub io_syscw: u64,
    /// C `unsigned long long io_read_bytes` — storage data read (in kilobytes).
    pub io_read_bytes: u64,
    /// C `unsigned long long io_write_bytes` — storage data written (in kilobytes).
    pub io_write_bytes: u64,
    /// C `unsigned long long io_cancelled_write_bytes` — storage data cancelled.
    pub io_cancelled_write_bytes: u64,
    /// C `unsigned long long io_last_scan_time` — last io scan (seconds since Epoch).
    pub io_last_scan_time: u64,
    /// C `double io_rate_read_bps`.
    pub io_rate_read_bps: f64,
    /// C `double io_rate_write_bps`.
    pub io_rate_write_bps: f64,
    /// C `char* cgroup` — raw cgroup path (`None` = NULL).
    pub cgroup: Option<String>,
    /// C `char* cgroup_short` — condensed cgroup path (`None` = NULL).
    pub cgroup_short: Option<String>,
    /// C `char* container_short` — guessed container name (`None` = NULL).
    pub container_short: Option<String>,
    /// C `long int autogroup_id`.
    pub autogroup_id: i64,
    /// C `int autogroup_nice`.
    pub autogroup_nice: i32,
    /// C `unsigned int oom`.
    pub oom: u32,
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
    /// C `unsigned long ctxt_total`.
    pub ctxt_total: u64,
    /// C `unsigned long ctxt_diff`.
    pub ctxt_diff: u64,
    /// C `char* secattr` — security attribute (`None` = NULL).
    pub secattr: Option<String>,
    /// C `unsigned long long int last_mlrs_calctime`.
    pub last_mlrs_calctime: u64,
}

/// Port of `Process* PCPProcess_new(const Machine* host)`
/// (`PCPProcess.c:96`). Allocates a zeroed process, installs the `PCPProcess`
/// class, and runs [`Process_init`].
pub fn PCPProcess_new(host: *const Machine) -> PCPProcess {
    let mut this = PCPProcess::default();
    Process_init(&mut this.super_, host as *const c_void);
    this
}

/// Port of `static void PCPProcess_printDelay(float delay_percent, char*
/// buffer, size_t n)` (`PCPProcess.c:112`). Formats a delay percentage or
/// `" N/A  "` for a negative (or NaN) value. `isNonnegative(x)` is `x >= 0.0`
/// (NaN compares false, matching the C helper).
fn PCPProcess_printDelay(delay_percent: f32) -> String {
    if delay_percent >= 0.0 {
        format!("{delay_percent:4.1}  ")
    } else {
        " N/A  ".to_string()
    }
}

/// Port of `static double PCPProcess_totalIORate(const PCPProcess* pp)`
/// (`PCPProcess.c:120`). Sums the read/write byte rates, treating a negative
/// (or NaN) rate as absent; returns `NAN` when both are absent.
fn PCPProcess_totalIORate(pp: &PCPProcess) -> f64 {
    let mut total_rate = f64::NAN;
    if pp.io_rate_read_bps >= 0.0 {
        total_rate = pp.io_rate_read_bps;
        if pp.io_rate_write_bps >= 0.0 {
            total_rate += pp.io_rate_write_bps;
        }
    } else if pp.io_rate_write_bps >= 0.0 {
        total_rate = pp.io_rate_write_bps;
    }
    total_rate
}

/// Port of `static void PCPProcess_rowWriteField(const Row* super, RichString*
/// str, ProcessField field)` (`PCPProcess.c:133`) — the `writeField`
/// [`RowClass`] slot. Handles the PCP memory/IO/cgroup/delay/context/security/
/// autogroup columns and delegates the rest to the base [`Process_writeField`].
/// The C `const Row*` receiver is a `&dyn Object` downcast to [`PCPProcess`].
pub fn PCPProcess_rowWriteField(super_: &dyn Object, str: &mut RichString, field: RowField) {
    use ProcessField as PF;

    let pp = (super_ as &dyn Any)
        .downcast_ref::<PCPProcess>()
        .expect("PCPProcess_rowWriteField: row is not a PCPProcess");

    let host = unsafe { &*(pp.super_.super_.host as *const Machine) };
    let coloring = host
        .settings
        .as_ref()
        .expect("PCPProcess_rowWriteField: host->settings is NULL")
        .highlightMegabytes;
    let scheme = ColorScheme::active();
    let mut attr = CE::DEFAULT_COLOR.packed(scheme);
    let buffer: String;

    match field {
        f if f == PF::CMINFLT as RowField => {
            Row_printCount(str, pp.cminflt, coloring);
            return;
        }
        f if f == PF::CMAJFLT as RowField => {
            Row_printCount(str, pp.cmajflt, coloring);
            return;
        }
        f if f == PF::M_DRS as RowField => {
            Row_printBytes(str, pp.m_drs as u64, coloring);
            return;
        }
        f if f == M_DT => {
            Row_printBytes(str, pp.m_dt as u64, coloring);
            return;
        }
        f if f == PF::M_LRS as RowField => {
            Row_printBytes(str, pp.m_lrs as u64, coloring);
            return;
        }
        f if f == PF::M_TRS as RowField => {
            Row_printBytes(str, pp.m_trs as u64, coloring);
            return;
        }
        f if f == PF::M_SHARE as RowField => {
            Row_printBytes(str, pp.m_share as u64, coloring);
            return;
        }
        f if f == PF::M_PRIV as RowField => {
            Row_printKBytes(str, pp.m_priv as u64, coloring);
            return;
        }
        f if f == PF::M_PSS as RowField => {
            Row_printKBytes(str, pp.m_pss as u64, coloring);
            return;
        }
        f if f == PF::M_SWAP as RowField => {
            Row_printKBytes(str, pp.m_swap as u64, coloring);
            return;
        }
        f if f == PF::M_PSSWP as RowField => {
            Row_printKBytes(str, pp.m_psswp as u64, coloring);
            return;
        }
        f if f == PF::UTIME as RowField => {
            Row_printTime(str, pp.utime, coloring);
            return;
        }
        f if f == PF::STIME as RowField => {
            Row_printTime(str, pp.stime, coloring);
            return;
        }
        f if f == PF::CUTIME as RowField => {
            Row_printTime(str, pp.cutime, coloring);
            return;
        }
        f if f == PF::CSTIME as RowField => {
            Row_printTime(str, pp.cstime, coloring);
            return;
        }
        f if f == PF::RCHAR as RowField => {
            Row_printBytes(str, pp.io_rchar, coloring);
            return;
        }
        f if f == PF::WCHAR as RowField => {
            Row_printBytes(str, pp.io_wchar, coloring);
            return;
        }
        f if f == PF::SYSCR as RowField => {
            Row_printCount(str, pp.io_syscr, coloring);
            return;
        }
        f if f == PF::SYSCW as RowField => {
            Row_printCount(str, pp.io_syscw, coloring);
            return;
        }
        f if f == PF::RBYTES as RowField => {
            Row_printBytes(str, pp.io_read_bytes, coloring);
            return;
        }
        f if f == PF::WBYTES as RowField => {
            Row_printBytes(str, pp.io_write_bytes, coloring);
            return;
        }
        f if f == PF::CNCLWB as RowField => {
            Row_printBytes(str, pp.io_cancelled_write_bytes, coloring);
            return;
        }
        f if f == PF::IO_READ_RATE as RowField => {
            Row_printRate(str, pp.io_rate_read_bps, coloring);
            return;
        }
        f if f == PF::IO_WRITE_RATE as RowField => {
            Row_printRate(str, pp.io_rate_write_bps, coloring);
            return;
        }
        f if f == PF::IO_RATE as RowField => {
            Row_printRate(str, PCPProcess_totalIORate(pp), coloring);
            return;
        }
        f if f == PF::CGROUP as RowField => {
            let s = pp.cgroup.as_deref().unwrap_or("N/A");
            buffer = format!("{s:<35.35} ");
        }
        f if f == PF::CCGROUP as RowField => {
            let s = pp
                .cgroup_short
                .as_deref()
                .or(pp.cgroup.as_deref())
                .unwrap_or("N/A");
            buffer = format!("{s:<35.35} ");
        }
        f if f == PF::CONTAINER as RowField => {
            let s = pp.container_short.as_deref().unwrap_or("N/A");
            buffer = format!("{s:<35.35} ");
        }
        f if f == PF::OOM as RowField => {
            buffer = format!("{:>4} ", pp.oom);
        }
        f if f == PF::PERCENT_CPU_DELAY as RowField => {
            buffer = PCPProcess_printDelay(pp.cpu_delay_percent);
        }
        f if f == PF::PERCENT_IO_DELAY as RowField => {
            buffer = PCPProcess_printDelay(pp.blkio_delay_percent);
        }
        f if f == PF::PERCENT_SWAP_DELAY as RowField => {
            buffer = PCPProcess_printDelay(pp.swapin_delay_percent);
        }
        f if f == PF::CTXT as RowField => {
            if pp.ctxt_diff > 1000 {
                attr |= A_BOLD;
            }
            buffer = format!("{:>5} ", pp.ctxt_diff);
        }
        f if f == PF::SECATTR as RowField => {
            let s = pp.secattr.as_deref().unwrap_or("?");
            buffer = format!("{s:<30}   ");
        }
        f if f == PF::AUTOGROUP_ID as RowField => {
            if pp.autogroup_id != -1 {
                buffer = format!("{:>4} ", pp.autogroup_id);
            } else {
                attr = CE::PROCESS_SHADOW.packed(scheme);
                buffer = " N/A ".to_string();
            }
        }
        f if f == PF::AUTOGROUP_NICE as RowField => {
            if pp.autogroup_id != -1 {
                buffer = format!("{:>3} ", pp.autogroup_nice);
                attr = if pp.autogroup_nice < 0 {
                    CE::PROCESS_HIGH_PRIORITY.packed(scheme)
                } else if pp.autogroup_nice > 0 {
                    CE::PROCESS_LOW_PRIORITY.packed(scheme)
                } else {
                    CE::PROCESS_SHADOW.packed(scheme)
                };
            } else {
                attr = CE::PROCESS_SHADOW.packed(scheme);
                buffer = "N/A ".to_string();
            }
        }
        _ => {
            Process_writeField(&pp.super_, str, field);
            return;
        }
    }

    RichString_appendWide(str, attr, buffer.as_bytes());
}

/// Port of `static int PCPProcess_compareByKey(const Process* v1, const
/// Process* v2, ProcessField key)` (`PCPProcess.c:214`). Compares on the PCP
/// per-field data; reserved keys (`key < LAST_PROCESSFIELD`) fall through to
/// [`Process_compareByKey_Base`]. The `key` is a [`RowField`] (int, per C).
pub fn PCPProcess_compareByKey(v1: &dyn Object, v2: &dyn Object, key: RowField) -> i32 {
    use ProcessField as PF;

    let p1 = (v1 as &dyn Any)
        .downcast_ref::<PCPProcess>()
        .expect("PCPProcess_compareByKey: v1 is not a PCPProcess");
    let p2 = (v2 as &dyn Any)
        .downcast_ref::<PCPProcess>()
        .expect("PCPProcess_compareByKey: v2 is not a PCPProcess");

    match key {
        f if f == PF::M_DRS as RowField => spaceship_number!(p1.m_drs, p2.m_drs),
        f if f == M_DT => spaceship_number!(p1.m_dt, p2.m_dt),
        f if f == PF::M_LRS as RowField => spaceship_number!(p1.m_lrs, p2.m_lrs),
        f if f == PF::M_TRS as RowField => spaceship_number!(p1.m_trs, p2.m_trs),
        f if f == PF::M_SHARE as RowField => spaceship_number!(p1.m_share, p2.m_share),
        f if f == PF::M_PRIV as RowField => spaceship_number!(p1.m_priv, p2.m_priv),
        f if f == PF::M_PSS as RowField => spaceship_number!(p1.m_pss, p2.m_pss),
        f if f == PF::M_SWAP as RowField => spaceship_number!(p1.m_swap, p2.m_swap),
        f if f == PF::M_PSSWP as RowField => spaceship_number!(p1.m_psswp, p2.m_psswp),
        f if f == PF::UTIME as RowField => spaceship_number!(p1.utime, p2.utime),
        f if f == PF::CUTIME as RowField => spaceship_number!(p1.cutime, p2.cutime),
        f if f == PF::STIME as RowField => spaceship_number!(p1.stime, p2.stime),
        f if f == PF::CSTIME as RowField => spaceship_number!(p1.cstime, p2.cstime),
        f if f == PF::RCHAR as RowField => spaceship_number!(p1.io_rchar, p2.io_rchar),
        f if f == PF::WCHAR as RowField => spaceship_number!(p1.io_wchar, p2.io_wchar),
        f if f == PF::SYSCR as RowField => spaceship_number!(p1.io_syscr, p2.io_syscr),
        f if f == PF::SYSCW as RowField => spaceship_number!(p1.io_syscw, p2.io_syscw),
        f if f == PF::RBYTES as RowField => spaceship_number!(p1.io_read_bytes, p2.io_read_bytes),
        f if f == PF::WBYTES as RowField => spaceship_number!(p1.io_write_bytes, p2.io_write_bytes),
        f if f == PF::CNCLWB as RowField => {
            spaceship_number!(p1.io_cancelled_write_bytes, p2.io_cancelled_write_bytes)
        }
        f if f == PF::IO_READ_RATE as RowField => {
            compareRealNumbers(p1.io_rate_read_bps, p2.io_rate_read_bps)
        }
        f if f == PF::IO_WRITE_RATE as RowField => {
            compareRealNumbers(p1.io_rate_write_bps, p2.io_rate_write_bps)
        }
        f if f == PF::IO_RATE as RowField => {
            compareRealNumbers(PCPProcess_totalIORate(p1), PCPProcess_totalIORate(p2))
        }
        f if f == PF::CGROUP as RowField => spaceship_nullstr!(
            p1.cgroup.as_deref().map(str::as_bytes),
            p2.cgroup.as_deref().map(str::as_bytes)
        ),
        f if f == PF::CCGROUP as RowField => spaceship_nullstr!(
            p1.cgroup_short.as_deref().map(str::as_bytes),
            p2.cgroup_short.as_deref().map(str::as_bytes)
        ),
        f if f == PF::CONTAINER as RowField => spaceship_nullstr!(
            p1.container_short.as_deref().map(str::as_bytes),
            p2.container_short.as_deref().map(str::as_bytes)
        ),
        f if f == PF::OOM as RowField => spaceship_number!(p1.oom, p2.oom),
        f if f == PF::PERCENT_CPU_DELAY as RowField => {
            compareRealNumbers(p1.cpu_delay_percent as f64, p2.cpu_delay_percent as f64)
        }
        f if f == PF::PERCENT_IO_DELAY as RowField => {
            compareRealNumbers(p1.blkio_delay_percent as f64, p2.blkio_delay_percent as f64)
        }
        f if f == PF::PERCENT_SWAP_DELAY as RowField => compareRealNumbers(
            p1.swapin_delay_percent as f64,
            p2.swapin_delay_percent as f64,
        ),
        f if f == PF::CTXT as RowField => spaceship_number!(p1.ctxt_diff, p2.ctxt_diff),
        f if f == PF::SECATTR as RowField => spaceship_nullstr!(
            p1.secattr.as_deref().map(str::as_bytes),
            p2.secattr.as_deref().map(str::as_bytes)
        ),
        f if f == PF::AUTOGROUP_ID as RowField => {
            spaceship_number!(p1.autogroup_id, p2.autogroup_id)
        }
        f if f == PF::AUTOGROUP_NICE as RowField => {
            spaceship_number!(p1.autogroup_nice, p2.autogroup_nice)
        }
        // C: `default: if (key < LAST_PROCESSFIELD) return
        // Process_compareByKey_Base(v1, v2, key); return
        // PCPDynamicColumn_compareByKey(p1, p2, key);`. The dynamic-column
        // comparator lives in `pcp/PCPDynamicColumn.c` (not yet ported); no
        // dynamic columns are registered without that subsystem, so every
        // reachable key is `< LAST_PROCESSFIELD` and routes to the base
        // comparator — the Linux-port default. Wiring the dynamic-column path
        // is deferred until `PCPDynamicColumn.c` is ported.
        _ => Process_compareByKey_Base(&p1.super_, &p2.super_, key),
    }
}

/// Port of `const ProcessClass PCPProcess_class` (`PCPProcess.c:294`). Wires
/// the inherited `Process` row slots plus the PCP-specific `writeField` and
/// `compareByKey`. `matchesFilter` stays `None` (its delegate
/// `Process_rowMatchesFilter` is stubbed on the `ProcessTable`/`pidMatchList`
/// substrate), matching the Linux/DragonFly precedent.
pub static PCPProcess_class: ProcessClass = ProcessClass {
    super_: RowClass {
        super_: ObjectClass {
            extends: Some(&Process_class.super_.super_),
        },
        isHighlighted: Some(Process_rowIsHighlighted),
        isVisible: Some(Process_rowIsVisible),
        writeField: Some(PCPProcess_rowWriteField),
        matchesFilter: None,
        sortKeyString: Some(Process_rowGetSortKey),
        compareByParent: Some(Process_compareByParent),
    },
    compareByKey: Some(PCPProcess_compareByKey),
};

impl Object for PCPProcess {
    /// C `Object_setClass(this, Class(PCPProcess))`: the embedded
    /// [`ObjectClass`] of the `PCPProcess` vtable.
    fn klass(&self) -> &'static ObjectClass {
        &PCPProcess_class.super_.super_
    }

    /// C `As_Row(this)` — this process's [`RowClass`] vtable.
    fn row_class(&self) -> Option<&'static RowClass> {
        Some(&PCPProcess_class.super_)
    }

    /// C `(const Row*)this` — the embedded base (`super_.super_`).
    fn as_row(&self) -> Option<&Row> {
        Some(&self.super_.super_)
    }

    /// C `(const Process*)this` — the embedded `Process` (`super_`).
    fn as_process(&self) -> Option<&Process> {
        Some(&self.super_)
    }

    /// Mutable `(Row*)this` — required by `ProcessTable_getProcess`/`Table_add`
    /// when the scan registers a fresh row (the darwin/linux precedent).
    fn as_row_mut(&mut self) -> Option<&mut Row> {
        Some(&mut self.super_.super_)
    }

    /// Mutable `(Process*)this` — required by `ProcessTable_getProcess` to hand
    /// back the process for in-place field updates during the scan.
    fn as_process_mut(&mut self) -> Option<&mut Process> {
        Some(&mut self.super_)
    }

    /// C `As_Process(this)` — `PCPProcess`'s [`ProcessClass`] vtable, whose
    /// `compareByKey` slot is `PCPProcess_compareByKey`.
    fn process_class(&self) -> Option<&'static ProcessClass> {
        Some(&PCPProcess_class)
    }

    /// C `PCPProcess_class.super.super.display = Row_display`.
    fn display(&self, out: &mut RichString) {
        Row_display(self, out)
    }

    /// C `.compare = Process_compare`, dispatching `PCPProcess`'s
    /// `compareByKey` via the concrete objects.
    fn compare(&self, other: &dyn Object) -> i32 {
        Process_compare(self, other)
    }
}

/// Deref so `&PCPProcess_class` coerces to `&ObjectClass` for the
/// class-identity API, exactly as [`ProcessClass`]'s own `Deref`.
const _: fn() = || {
    fn assert_deref<T: Deref<Target = ObjectClass>>() {}
    assert_deref::<ProcessClass>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::object::Object_isA;

    /// The class chain: `PCPProcess` is a `PCPProcess`, a `Process`, and a
    /// `Row` (via the embedded `ObjectClass` extends chain).
    #[test]
    fn class_chain_extends_process() {
        let p = PCPProcess_new(core::ptr::null());
        let obj: &dyn Object = &p;
        assert!(Object_isA(Some(obj), &PCPProcess_class));
        assert!(Object_isA(Some(obj), &Process_class));
        let rc = obj.row_class().unwrap();
        assert!(rc.writeField.is_some());
        assert!(rc.isHighlighted.is_some());
        assert!(rc.matchesFilter.is_none());
    }

    /// The [`Process_fields`] table carries the PCP columns with significant
    /// trailing-space titles, including the module-local `M_DT` slot.
    #[test]
    fn process_fields_table_has_pcp_columns() {
        assert_eq!(Process_fields.len(), LAST_PROCESSFIELD);
        assert_eq!(Process_fields[M_DT as usize].name, "M_DT");
        assert_eq!(Process_fields[M_DT as usize].title, Some(" DIRTY "));
        assert!(Process_fields[M_DT as usize].defaultSortDesc);
        assert_eq!(
            Process_fields[ProcessField::CGROUP as usize].title,
            Some("CGROUP (raw)                        ")
        );
        assert_eq!(
            Process_fields[ProcessField::CGROUP as usize].flags,
            PROCESS_FLAG_LINUX_CGROUP
        );
        assert_eq!(
            Process_fields[ProcessField::RCHAR as usize].flags,
            PROCESS_FLAG_IO
        );
        assert!(Process_fields[ProcessField::PID as usize].pidColumn);
        assert_eq!(Process_fields[0].name, "");
    }

    /// [`PCPProcess_compareByKey`]: M_DT / IO-rate ordering, and delegation of
    /// a reserved key to the base comparator.
    #[test]
    fn compare_by_key_pcp_and_delegate() {
        let mut a = PCPProcess_new(core::ptr::null());
        let mut b = PCPProcess_new(core::ptr::null());
        a.m_dt = 1;
        b.m_dt = 2;
        assert!(PCPProcess_compareByKey(&a as &dyn Object, &b as &dyn Object, M_DT) < 0);
        a.io_rate_read_bps = 100.0;
        b.io_rate_read_bps = 50.0;
        assert!(
            PCPProcess_compareByKey(
                &a as &dyn Object,
                &b as &dyn Object,
                ProcessField::IO_READ_RATE as RowField
            ) > 0
        );
        // Reserved key (PID) → base comparison by Row id.
        a.super_.super_.id = 7;
        b.super_.super_.id = 3;
        assert!(
            PCPProcess_compareByKey(
                &a as &dyn Object,
                &b as &dyn Object,
                ProcessField::PID as RowField
            ) > 0
        );
    }

    /// [`PCPProcess_totalIORate`]: sum when both present, NaN when both absent.
    #[test]
    fn total_io_rate_semantics() {
        let mut p = PCPProcess_new(core::ptr::null());
        p.io_rate_read_bps = 10.0;
        p.io_rate_write_bps = 5.0;
        assert_eq!(PCPProcess_totalIORate(&p), 15.0);
        p.io_rate_read_bps = f64::NAN;
        p.io_rate_write_bps = f64::NAN;
        assert!(PCPProcess_totalIORate(&p).is_nan());
    }
}
