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
//! - `LinuxProcess_effectiveIOPriority` (`LinuxProcess.c:137`) — pure.
//! - [`LinuxProcess_updateIOPriority`] (`LinuxProcess.c:153`) and
//!   `LinuxProcess_setIOPriority` (`LinuxProcess.c:164`) — the
//!   `ioprio_get`/`ioprio_set` syscalls (Linux-only, exactly as the C
//!   `#ifdef SYS_ioprio_*` guards them), plus
//!   [`LinuxProcess_rowSetIOPriority`] (`LinuxProcess.c:172`).
//! - `LinuxProcess_totalIORate` (`LinuxProcess.c:213`) — pure.
//! - `LinuxProcess_changeAutogroupPriorityBy` (`LinuxProcess.c:185`) and
//!   [`LinuxProcess_rowChangeAutogroupPriorityBy`] (`LinuxProcess.c:207`).
//! - [`LinuxProcess_isAutogroupEnabled`] (`LinuxProcess.c:178`) — reads
//!   `sched_autogroup_enabled` via the now-ported `Compat_readfile`.
//!
//! Also ported:
//! - [`Process_delete`] (`LinuxProcess.c:119`) — the by-value teardown
//!   consuming `super_` into `Process_done`, the Linux-only heap fields
//!   dropping with the destructured remainder (darwin precedent).
//!
//! Still stubbed (blocked on unported substrate; see each fn's doc):
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

use crate::ported::crt::{ColorElements as CE, ColorScheme, A_BOLD};
use crate::ported::linux::compat::Compat_readfile;
use crate::ported::linux::linuxmachine::LinuxMachine;
use crate::ported::machine::Machine;
use crate::ported::object::{Arg, Object, ObjectClass, Object_isA};
use crate::ported::process::{
    spaceship_nullstr, Process, ProcessClass, ProcessField, ProcessFieldData, Process_class,
    Process_compare, Process_compareByKey_Base, Process_compareByParent, Process_getPid,
    Process_init, Process_rowGetSortKey, Process_rowIsHighlighted, Process_rowIsVisible,
    Process_writeField, Tristate, PROCESS_FLAG_CWD, PROCESS_FLAG_IO, PROCESS_FLAG_SCHEDPOL,
};
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_appendWide};
use crate::ported::row::{
    spaceship_number, PercentageAttr, RowClass, Row_fieldWidths, Row_printBytes, Row_printCount,
    Row_printKBytes, Row_printNanoseconds, Row_printPercentage, Row_printRate, Row_printTime,
};
use crate::ported::settings::RowField;
use crate::ported::xutils::compareRealNumbers;
use core::any::Any;
use core::ffi::c_void;
use std::ffi::CString;
use std::sync::atomic::Ordering;

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

// ── linux/LinuxProcess.h scan-method flags (`PROCESS_FLAG_LINUX_*`).
// Verbatim from `LinuxProcess.h:21`; they extend the generic
// `PROCESS_FLAG_IO`/`_CWD`/`_SCHEDPOL` from `Process.h`.

/// Port of `#define PROCESS_FLAG_LINUX_IOPRIO 0x00000100` (`LinuxProcess.h:21`).
pub const PROCESS_FLAG_LINUX_IOPRIO: u32 = 0x00000100;
/// Port of `#define PROCESS_FLAG_LINUX_OPENVZ 0x00000200` (`LinuxProcess.h:22`).
pub const PROCESS_FLAG_LINUX_OPENVZ: u32 = 0x00000200;
/// Port of `#define PROCESS_FLAG_LINUX_VSERVER 0x00000400` (`LinuxProcess.h:23`).
pub const PROCESS_FLAG_LINUX_VSERVER: u32 = 0x00000400;
/// Port of `#define PROCESS_FLAG_LINUX_CGROUP 0x00000800` (`LinuxProcess.h:24`).
pub const PROCESS_FLAG_LINUX_CGROUP: u32 = 0x00000800;
/// Port of `#define PROCESS_FLAG_LINUX_OOM 0x00001000` (`LinuxProcess.h:25`).
pub const PROCESS_FLAG_LINUX_OOM: u32 = 0x00001000;
/// Port of `#define PROCESS_FLAG_LINUX_SMAPS 0x00002000` (`LinuxProcess.h:26`).
pub const PROCESS_FLAG_LINUX_SMAPS: u32 = 0x00002000;
/// Port of `#define PROCESS_FLAG_LINUX_CTXT 0x00004000` (`LinuxProcess.h:27`).
pub const PROCESS_FLAG_LINUX_CTXT: u32 = 0x00004000;
/// Port of `#define PROCESS_FLAG_LINUX_SECATTR 0x00008000` (`LinuxProcess.h:28`).
pub const PROCESS_FLAG_LINUX_SECATTR: u32 = 0x00008000;
/// Port of `#define PROCESS_FLAG_LINUX_LRS_FIX 0x00010000` (`LinuxProcess.h:29`).
pub const PROCESS_FLAG_LINUX_LRS_FIX: u32 = 0x00010000;
/// Port of `#define PROCESS_FLAG_LINUX_DELAYACCT 0x00040000` (`LinuxProcess.h:30`).
pub const PROCESS_FLAG_LINUX_DELAYACCT: u32 = 0x00040000;
/// Port of `#define PROCESS_FLAG_LINUX_AUTOGROUP 0x00080000` (`LinuxProcess.h:31`).
pub const PROCESS_FLAG_LINUX_AUTOGROUP: u32 = 0x00080000;
/// Port of `#define PROCESS_FLAG_LINUX_GPU 0x00100000` (`LinuxProcess.h:32`).
pub const PROCESS_FLAG_LINUX_GPU: u32 = 0x00100000;
/// Port of `#define PROCESS_FLAG_LINUX_CONTAINER 0x00200000` (`LinuxProcess.h:33`).
pub const PROCESS_FLAG_LINUX_CONTAINER: u32 = 0x00200000;

/// Port of `#define LAST_PROCESSFIELD LAST_RESERVED_FIELD` (`Process.h:229`).
/// `LAST_RESERVED_FIELD` is the enum entry immediately after the last field
/// (`ISCONTAINER = 134`), so it is `135` — also the length of
/// [`Process_fields`].
pub const LAST_PROCESSFIELD: usize = 135;

/// `const fn` helper building one [`ProcessFieldData`] entry. Keeps the
/// table below terse while spelling out every field at each call site, so
/// it stays a faithful transcription of the C designated initializers.
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

/// The unused/zero index-0 entry (`[0] = { .name = "", .title = NULL, … }`)
/// and every gap between the sparse designated indices. Matches C's
/// implicit zero-initialization of un-designated array slots.
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

/// Port of `const ProcessFieldData Process_fields[LAST_PROCESSFIELD]` from
/// `linux/LinuxProcess.c` — the per-field metadata table, indexed by
/// [`ProcessField`] id. Built for the modern default Linux configure:
/// `HAVE_OPENVZ`/`HAVE_VSERVER` off (so the `CTID`/`VPID`/`VXID` slots stay
/// empty, exactly as the C `#ifdef`s leave them), `HAVE_DELAYACCT` on (the
/// `--enable-delayacct` build the `LinuxProcess` delay fields already assume),
/// and `SCHEDULER_SUPPORT` on. Trailing spaces in the titles are significant
/// (they set the printed column width) and are preserved verbatim.
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
        true,
        false,
        false,
        false,
    );
    t[PF::PGRP as usize] = pfd(
        "PGRP",
        "PGRP",
        "Process group ID",
        0,
        true,
        false,
        false,
        false,
    );
    t[PF::SESSION as usize] = pfd(
        "SESSION",
        "SID",
        "Process's session ID",
        0,
        true,
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
        true,
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
        "Id of the CPU the process last executed on",
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
        "Size of the .text segment of the process (CODE)",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::M_DRS as usize] = pfd(
        "M_DRS",
        " DATA ",
        "Size of the .data segment plus stack usage of the process (DATA)",
        0,
        false,
        true,
        false,
        false,
    );
    t[PF::M_LRS as usize] = pfd(
        "M_LRS",
        "  LIB ",
        "The library size of the process (calculated from memory maps)",
        PROCESS_FLAG_LINUX_LRS_FIX,
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
        true,
        false,
        false,
        false,
    );
    // HAVE_OPENVZ off: CTID/VPID slots stay EMPTY_FIELD.
    // HAVE_VSERVER off: VXID slot stays EMPTY_FIELD.
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
        "  DISK READ ",
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
        "CGROUP (raw)",
        "Which cgroup the process is in",
        PROCESS_FLAG_LINUX_CGROUP,
        false,
        false,
        true,
        false,
    );
    t[PF::CCGROUP as usize] = pfd(
        "CCGROUP",
        "CGROUP (compressed)",
        "Which cgroup the process is in (condensed to essentials)",
        PROCESS_FLAG_LINUX_CGROUP,
        false,
        false,
        true,
        false,
    );
    t[PF::CONTAINER as usize] = pfd(
        "CONTAINER",
        "CONTAINER",
        "Name of the container the process is in (guessed by heuristics)",
        PROCESS_FLAG_LINUX_CGROUP,
        false,
        false,
        true,
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
    t[PF::IO_PRIORITY as usize] = pfd(
        "IO_PRIORITY",
        "IO ",
        "I/O priority",
        PROCESS_FLAG_LINUX_IOPRIO,
        false,
        false,
        false,
        false,
    );
    // HAVE_DELAYACCT on:
    t[PF::PERCENT_CPU_DELAY as usize] = pfd(
        "PERCENT_CPU_DELAY",
        "CPUD% ",
        "CPU delay %",
        PROCESS_FLAG_LINUX_DELAYACCT,
        false,
        true,
        false,
        false,
    );
    t[PF::PERCENT_IO_DELAY as usize] = pfd(
        "PERCENT_IO_DELAY",
        " IOD% ",
        "Block I/O delay %",
        PROCESS_FLAG_LINUX_DELAYACCT,
        false,
        true,
        false,
        false,
    );
    t[PF::PERCENT_SWAP_DELAY as usize] = pfd(
        "PERCENT_SWAP_DELAY",
        "SWPD% ",
        "Swapin delay %",
        PROCESS_FLAG_LINUX_DELAYACCT,
        false,
        true,
        false,
        false,
    );
    t[PF::M_PSS as usize] = pfd("M_PSS", "  PSS ", "proportional set size, same as M_RESIDENT but each page is divided by the number of processes sharing it", PROCESS_FLAG_LINUX_SMAPS, false, true, false, false);
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
    t[PF::M_PSSWP as usize] = pfd("M_PSSWP", " PSSWP ", "shows proportional swap share of this mapping, unlike \"Swap\", this does not take into account swapped out page of underlying shmem objects", PROCESS_FLAG_LINUX_SMAPS, false, true, false, false);
    t[PF::CTXT as usize] = pfd("CTXT", " CTXT ", "Context switches (incremental sum of voluntary_ctxt_switches and nonvoluntary_ctxt_switches)", PROCESS_FLAG_LINUX_CTXT, false, true, false, false);
    t[PF::SECATTR as usize] = pfd(
        "SECATTR",
        "Security Attribute",
        "Security attribute of the process (e.g. SELinux or AppArmor)",
        PROCESS_FLAG_LINUX_SECATTR,
        false,
        false,
        true,
        false,
    );
    t[PF::PROC_COMM as usize] = pfd(
        "COMM",
        "COMM            ",
        "comm string of the process from /proc/[pid]/comm",
        0,
        false,
        false,
        false,
        false,
    );
    t[PF::PROC_EXE as usize] = pfd(
        "EXE",
        "EXE             ",
        "Basename of exe of the process from /proc/[pid]/exe",
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
    t[PF::ISCONTAINER as usize] = pfd(
        "ISCONTAINER",
        "CONT ",
        "Whether the process is running inside a child container",
        PROCESS_FLAG_LINUX_CONTAINER,
        false,
        false,
        false,
        false,
    );
    // SCHEDULER_SUPPORT on:
    t[PF::SCHEDULERPOLICY as usize] = pfd(
        "SCHEDULERPOLICY",
        "SCHED ",
        "Current scheduling policy of the process",
        PROCESS_FLAG_SCHEDPOL,
        false,
        false,
        false,
        false,
    );
    t[PF::GPU_TIME as usize] = pfd(
        "GPU_TIME",
        "GPU_TIME ",
        "Total GPU time",
        PROCESS_FLAG_LINUX_GPU,
        false,
        true,
        false,
        false,
    );
    t[PF::GPU_PERCENT as usize] = pfd(
        "GPU_PERCENT",
        " GPU% ",
        "Percentage of the GPU time the process used in the last sampling",
        PROCESS_FLAG_LINUX_GPU,
        false,
        true,
        false,
        false,
    );
    t
}

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

    /// C `char* ctid` — OpenVZ container id.
    ///
    /// The C guards this with `#ifdef HAVE_OPENVZ`; the port carries it
    /// unconditionally — a minor documented deviation, same as other
    /// conditional fields the port always-includes.
    pub ctid: Option<String>,
    /// C `pid_t vpid` — OpenVZ virtual pid.
    ///
    /// The C guards this with `#ifdef HAVE_OPENVZ`; the port carries it
    /// unconditionally (see [`Self::ctid`]).
    pub vpid: i32,

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
/// The `RowClass` vtable wires the inherited `Process` slots (`isHighlighted`,
/// `isVisible`, `sortKeyString`, `compareByParent`) plus the Linux-specific
/// `writeField` ([`LinuxProcess_rowWriteField`]) and the `compareByKey`
/// [`ProcessClass`] slot ([`LinuxProcess_compareByKey`]); `matchesFilter`
/// stays `None` (blocked on the `ProcessTable`/`pidMatchList` substrate).
/// `.compare = Process_compare` and `.delete` are realized by the [`Object`]
/// impl / `Drop`.
pub static LinuxProcess_class: ProcessClass = ProcessClass {
    super_: RowClass {
        super_: ObjectClass {
            extends: Some(&Process_class.super_.super_),
        },
        isHighlighted: Some(Process_rowIsHighlighted),
        isVisible: Some(Process_rowIsVisible),
        writeField: Some(LinuxProcess_rowWriteField),
        matchesFilter: None,
        sortKeyString: Some(Process_rowGetSortKey),
        compareByParent: Some(Process_compareByParent),
    },
    compareByKey: Some(LinuxProcess_compareByKey),
};

impl Object for LinuxProcess {
    /// C `Object_setClass(this, Class(LinuxProcess))` in [`LinuxProcess_new`]:
    /// the embedded [`ObjectClass`] of the [`ProcessClass`] vtable.
    fn klass(&self) -> &'static ObjectClass {
        &LinuxProcess_class.super_.super_
    }

    /// C `As_Row(this)` — `LinuxProcess`'s [`RowClass`] vtable.
    fn row_class(&self) -> Option<&'static RowClass> {
        Some(&LinuxProcess_class.super_)
    }

    /// C `(const Row*)this` — the embedded base (`super_.super_`) of a
    /// `LinuxProcess`.
    fn as_row(&self) -> Option<&crate::ported::row::Row> {
        Some(&self.super_.super_)
    }

    /// C `(Row*)this` — the mutable embedded base, so `Table_add` /
    /// `Table_cleanupRow` can stamp/flag a `LinuxProcess` row (as `DarwinProcess`).
    fn as_row_mut(&mut self) -> Option<&mut crate::ported::row::Row> {
        Some(&mut self.super_.super_)
    }

    /// C `(const Process*)this` — the embedded `Process` (`super_`) of a
    /// `LinuxProcess`, so `Process`-level slots work on a `LinuxProcess`.
    fn as_process(&self) -> Option<&Process> {
        Some(&self.super_)
    }

    /// C `(Process*)this` — the mutable embedded `Process`, so
    /// `ProcessTable_getProcess` and the `/proc` scan can mutate a
    /// `LinuxProcess` row through a base-`Process` view (as `DarwinProcess`).
    fn as_process_mut(&mut self) -> Option<&mut Process> {
        Some(&mut self.super_)
    }

    /// C `As_Process(this)` — `LinuxProcess`'s [`ProcessClass`] vtable, whose
    /// `compareByKey` slot is `LinuxProcess_compareByKey`.
    fn process_class(&self) -> Option<&'static ProcessClass> {
        Some(&LinuxProcess_class)
    }

    /// C `LinuxProcess_class.super.super.display = Row_display`.
    fn display(&self, out: &mut RichString) {
        crate::ported::row::Row_display(self, out)
    }

    /// C `LinuxProcess_class.super.super.compare = Process_compare`.
    /// Downcasts the peer back to [`LinuxProcess`] (the safe-Rust analog of
    /// the C `const void*` cast) and delegates to [`Process_compare`] on the
    /// embedded bases — stubbed pending the `Settings` substrate, matching
    /// the C wiring (and `Process`'s own `Object::compare`).
    fn compare(&self, other: &dyn Object) -> i32 {
        // Pass the concrete objects (not the embedded `Process`) so
        // `Process_compare` dispatches `LinuxProcess`'s `compareByKey` slot.
        Process_compare(self, other)
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

/// Port of `void Process_delete(Object* cast)` from `LinuxProcess.c:119`.
/// The C body downcasts to `LinuxProcess*`, calls `Process_done(&this->super)`,
/// then `free`s the Linux-only heap fields (`cgroup`, `cgroup_short`,
/// `container_short`, `secattr`) and `free(this)`. Take `this` by value: the
/// base teardown is `Process_done` on the moved-out `super_`, the Linux-only
/// `Option<String>` fields drop when the destructured remainder falls out of
/// scope, and the final `free(this)` folds into the by-value consume (the
/// darwin `Process_delete` precedent).
pub fn Process_delete(this: LinuxProcess) {
    let LinuxProcess { super_, .. } = this;
    crate::ported::process::Process_done(super_);
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
/// `LinuxProcess_setIOPriority`. The C `(Process*) super` cast validated
/// by `assert(Object_isA(..., &Process_class))` becomes the `Object_isA`
/// guard plus a mutable `Any` downcast (the `Affinity_rowSet` idiom).
pub fn LinuxProcess_rowSetIOPriority(super_: &mut dyn Object, ioprio: Arg) -> bool {
    debug_assert!(Object_isA(Some(super_ as &dyn Object), &Process_class));
    let p = (super_ as &mut dyn Any)
        .downcast_mut::<LinuxProcess>()
        .expect("LinuxProcess_rowSetIOPriority: row is not a LinuxProcess");
    LinuxProcess_setIOPriority(p, ioprio)
}

/// Port of `bool LinuxProcess_isAutogroupEnabled(void)` from
/// `LinuxProcess.c:178`. Reads `PROCDIR "/sys/kernel/sched_autogroup_enabled"`
/// into a 16-byte buffer via [`Compat_readfile`]; returns `false` on any read
/// error (`< 0`), else `true` iff the first byte is `'1'`. The C string-literal
/// concatenation `PROCDIR "/sys/..."` is rebuilt from the `PROCDIR` const and
/// passed as a NUL-terminated [`CString`], matching `Compat_readfile`'s
/// `const char*` parameter.
pub fn LinuxProcess_isAutogroupEnabled() -> bool {
    let mut buf = [0u8; 16];
    let path = CString::new(format!("{}/sys/kernel/sched_autogroup_enabled", PROCDIR))
        .expect("PROCDIR path contains no interior NUL");
    if Compat_readfile(&path, &mut buf) < 0 {
        return false;
    }
    buf[0] == b'1'
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
/// `LinuxProcess_changeAutogroupPriorityBy` on the embedded base. Same
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

/// Port of `static void LinuxProcess_rowWriteField(const Row* super,
/// RichString* str, ProcessField field)` from `LinuxProcess.c:226` — the
/// Linux-specific per-field renderer. Handles the Linux platform fields and
/// delegates every other key to the base [`Process_writeField`]. Mirrors
/// [`crate::ported::process::Process_writeField`]'s structure: `return` arms
/// delegate to a `Row_print*` helper, `break` arms format into a buffer and
/// pick a color that the shared tail appends. `HAVE_OPENVZ`/`HAVE_VSERVER`
/// are off (no `CTID`/`VPID`/`VXID` arms); `HAVE_DELAYACCT` is on.
///
/// This is the `writeField` [`RowClass`] vtable slot for `LinuxProcess`; the
/// C `const Row* super` receiver is a `&dyn Object` downcast to
/// [`LinuxProcess`] (C's `(const LinuxProcess*)super`).
pub fn LinuxProcess_rowWriteField(super_: &dyn Object, str: &mut RichString, field: RowField) {
    use ProcessField as PF;

    let this = (super_ as &dyn Any)
        .downcast_ref::<LinuxProcess>()
        .expect("LinuxProcess_rowWriteField: row is not a LinuxProcess");
    let host = unsafe { &*(this.super_.super_.host as *const Machine) };
    let lhost = unsafe { &*(this.super_.super_.host as *const LinuxMachine) };
    let coloring = host
        .settings
        .as_ref()
        .expect("LinuxProcess_rowWriteField: host->settings is NULL")
        .highlightMegabytes;
    let scheme = ColorScheme::active();
    let n = 255usize;
    let mut attr = CE::DEFAULT_COLOR.packed(scheme);
    let buffer: String;

    // Row_printPercentage returns the text + a PercentageAttr; map it to attr.
    macro_rules! pct {
        ($val:expr, $width:expr) => {{
            let mut pa = PercentageAttr::Unchanged;
            let s = Row_printPercentage($val, n, $width, &mut pa);
            match pa {
                PercentageAttr::Shadow => attr = CE::PROCESS_SHADOW.packed(scheme),
                PercentageAttr::Megabytes => attr = CE::PROCESS_MEGABYTES.packed(scheme),
                PercentageAttr::Unchanged => {}
            }
            s
        }};
    }

    match field {
        f if f == PF::CMINFLT as RowField => {
            Row_printCount(str, this.cminflt, coloring);
            return;
        }
        f if f == PF::CMAJFLT as RowField => {
            Row_printCount(str, this.cmajflt, coloring);
            return;
        }
        f if f == PF::GPU_PERCENT as RowField => {
            buffer = pct!(this.gpu_percent, 5);
        }
        f if f == PF::GPU_TIME as RowField => {
            Row_printNanoseconds(str, this.gpu_time, coloring);
            return;
        }
        f if f == PF::M_DRS as RowField => {
            Row_printBytes(
                str,
                (this.m_drs as u64).wrapping_mul(lhost.pageSize as u64),
                coloring,
            );
            return;
        }
        f if f == PF::M_LRS as RowField => {
            if this.m_lrs != 0 {
                Row_printBytes(
                    str,
                    (this.m_lrs as u64).wrapping_mul(lhost.pageSize as u64),
                    coloring,
                );
                return;
            }
            attr = CE::PROCESS_SHADOW.packed(scheme);
            buffer = "  N/A ".to_string();
        }
        f if f == PF::M_TRS as RowField => {
            Row_printBytes(
                str,
                (this.m_trs as u64).wrapping_mul(lhost.pageSize as u64),
                coloring,
            );
            return;
        }
        f if f == PF::M_SHARE as RowField => {
            Row_printBytes(
                str,
                (this.m_share as u64).wrapping_mul(lhost.pageSize as u64),
                coloring,
            );
            return;
        }
        f if f == PF::M_PRIV as RowField => {
            Row_printKBytes(str, this.m_priv as u64, coloring);
            return;
        }
        f if f == PF::M_PSS as RowField => {
            Row_printKBytes(str, this.m_pss as u64, coloring);
            return;
        }
        f if f == PF::M_SWAP as RowField => {
            Row_printKBytes(str, this.m_swap as u64, coloring);
            return;
        }
        f if f == PF::M_PSSWP as RowField => {
            Row_printKBytes(str, this.m_psswp as u64, coloring);
            return;
        }
        f if f == PF::UTIME as RowField => {
            Row_printTime(str, this.utime, coloring);
            return;
        }
        f if f == PF::STIME as RowField => {
            Row_printTime(str, this.stime, coloring);
            return;
        }
        f if f == PF::CUTIME as RowField => {
            Row_printTime(str, this.cutime, coloring);
            return;
        }
        f if f == PF::CSTIME as RowField => {
            Row_printTime(str, this.cstime, coloring);
            return;
        }
        f if f == PF::RCHAR as RowField => {
            Row_printBytes(str, this.io_rchar, coloring);
            return;
        }
        f if f == PF::WCHAR as RowField => {
            Row_printBytes(str, this.io_wchar, coloring);
            return;
        }
        f if f == PF::SYSCR as RowField => {
            Row_printCount(str, this.io_syscr, coloring);
            return;
        }
        f if f == PF::SYSCW as RowField => {
            Row_printCount(str, this.io_syscw, coloring);
            return;
        }
        f if f == PF::RBYTES as RowField => {
            Row_printBytes(str, this.io_read_bytes, coloring);
            return;
        }
        f if f == PF::WBYTES as RowField => {
            Row_printBytes(str, this.io_write_bytes, coloring);
            return;
        }
        f if f == PF::CNCLWB as RowField => {
            Row_printBytes(str, this.io_cancelled_write_bytes, coloring);
            return;
        }
        f if f == PF::IO_READ_RATE as RowField => {
            Row_printRate(str, this.io_rate_read_bps, coloring);
            return;
        }
        f if f == PF::IO_WRITE_RATE as RowField => {
            Row_printRate(str, this.io_rate_write_bps, coloring);
            return;
        }
        f if f == PF::IO_RATE as RowField => {
            Row_printRate(str, LinuxProcess_totalIORate(this), coloring);
            return;
        }
        f if f == PF::CGROUP as RowField => {
            let w = Row_fieldWidths[PF::CGROUP as usize].load(Ordering::Relaxed) as usize;
            let s = this.cgroup.as_deref().unwrap_or("N/A");
            let buf = format!("{s:<w$.w$} ");
            RichString_appendWide(str, attr, buf.as_bytes());
            return;
        }
        f if f == PF::CCGROUP as RowField => {
            let w = Row_fieldWidths[PF::CCGROUP as usize].load(Ordering::Relaxed) as usize;
            let s = this
                .cgroup_short
                .as_deref()
                .or(this.cgroup.as_deref())
                .unwrap_or("N/A");
            let buf = format!("{s:<w$.w$} ");
            RichString_appendWide(str, attr, buf.as_bytes());
            return;
        }
        f if f == PF::CONTAINER as RowField => {
            let w = Row_fieldWidths[PF::CONTAINER as usize].load(Ordering::Relaxed) as usize;
            let s = this.container_short.as_deref().unwrap_or("N/A");
            let buf = format!("{s:<w$.w$} ");
            RichString_appendWide(str, attr, buf.as_bytes());
            return;
        }
        f if f == PF::OOM as RowField => {
            // xSnprintf(buffer, n, "%4u ", lp->oom); — unconditional, default
            // attr. The vendored C has no sentinel/"N/A" branch for OOM.
            buffer = format!("{:>4} ", this.oom);
        }
        f if f == PF::IO_PRIORITY as RowField => {
            let klass = this.ioPriority >> IOPRIO_CLASS_SHIFT;
            let data = this.ioPriority & IOPRIO_PRIO_MASK;
            buffer = if klass == IOPRIO_CLASS_NONE {
                format!("B{} ", (this.super_.nice + 20) / 5)
            } else if klass == IOPRIO_CLASS_BE {
                format!("B{data} ")
            } else if klass == IOPRIO_CLASS_RT {
                attr = CE::PROCESS_HIGH_PRIORITY.packed(scheme);
                format!("R{data} ")
            } else if klass == IOPRIO_CLASS_IDLE {
                attr = CE::PROCESS_LOW_PRIORITY.packed(scheme);
                "id ".to_string()
            } else {
                "?? ".to_string()
            };
        }
        f if f == PF::PERCENT_CPU_DELAY as RowField => {
            buffer = pct!(this.cpu_delay_percent, 5);
        }
        f if f == PF::PERCENT_IO_DELAY as RowField => {
            buffer = pct!(this.blkio_delay_percent, 5);
        }
        f if f == PF::PERCENT_SWAP_DELAY as RowField => {
            buffer = pct!(this.swapin_delay_percent, 5);
        }
        f if f == PF::CTXT as RowField => {
            if this.ctxt_diff > 1000 {
                attr |= A_BOLD;
            }
            buffer = format!("{:>5} ", this.ctxt_diff);
        }
        f if f == PF::SECATTR as RowField => {
            let w = Row_fieldWidths[PF::SECATTR as usize].load(Ordering::Relaxed) as usize;
            let s = this.secattr.as_deref().unwrap_or("N/A");
            let buf = format!("{s:<w$.w$} ");
            RichString_appendWide(str, attr, buf.as_bytes());
            return;
        }
        f if f == PF::AUTOGROUP_ID as RowField => {
            if this.autogroup_id != -1 {
                buffer = format!("{:>4} ", this.autogroup_id);
            } else {
                attr = CE::PROCESS_SHADOW.packed(scheme);
                buffer = " N/A ".to_string();
            }
        }
        f if f == PF::AUTOGROUP_NICE as RowField => {
            if this.autogroup_id != -1 {
                buffer = format!("{:>3} ", this.autogroup_nice);
                attr = if this.autogroup_nice < 0 {
                    CE::PROCESS_HIGH_PRIORITY.packed(scheme)
                } else if this.autogroup_nice > 0 {
                    CE::PROCESS_LOW_PRIORITY.packed(scheme)
                } else {
                    CE::PROCESS_SHADOW.packed(scheme)
                };
            } else {
                attr = CE::PROCESS_SHADOW.packed(scheme);
                buffer = "N/A ".to_string();
            }
        }
        f if f == PF::ISCONTAINER as RowField => {
            buffer = match this.super_.isRunningInContainer {
                Tristate::TRI_ON => "YES  ".to_string(),
                Tristate::TRI_OFF => "NO   ".to_string(),
                _ => {
                    attr = CE::PROCESS_SHADOW.packed(scheme);
                    "N/A  ".to_string()
                }
            };
        }
        _ => {
            Process_writeField(&this.super_, str, field);
            return;
        }
    }

    RichString_appendAscii(str, attr, buffer.as_bytes());
}

/// Port of `static int LinuxProcess_compareByKey(const Process* v1, const
/// Process* v2, ProcessField key)` from `LinuxProcess.c:366`. Compares two
/// processes on a Linux platform field, delegating unhandled keys to
/// [`Process_compareByKey_Base`]. This is the `compareByKey`
/// [`ProcessClass`] slot; the C `const Process*` receivers are `&dyn Object`
/// downcast to `LinuxProcess` (C's `(const LinuxProcess*)`). The base fields
/// (`isRunningInContainer`) and comparison are reached through `super_`.
/// `HAVE_OPENVZ`/`HAVE_VSERVER` are off (matching [`Process_fields`]), so the
/// `CTID`/`VPID`/`VXID` arms are absent exactly as the C `#ifdef`s omit them;
/// `HAVE_DELAYACCT` is on.
pub fn LinuxProcess_compareByKey(v1: &dyn Object, v2: &dyn Object, key: RowField) -> i32 {
    let p1 = (v1 as &dyn Any)
        .downcast_ref::<LinuxProcess>()
        .expect("LinuxProcess_compareByKey: v1 is not a LinuxProcess");
    let p2 = (v2 as &dyn Any)
        .downcast_ref::<LinuxProcess>()
        .expect("LinuxProcess_compareByKey: v2 is not a LinuxProcess");
    match key {
        k if k == ProcessField::M_DRS as RowField => spaceship_number!(p1.m_drs, p2.m_drs),
        k if k == ProcessField::M_LRS as RowField => spaceship_number!(p1.m_lrs, p2.m_lrs),
        k if k == ProcessField::M_TRS as RowField => spaceship_number!(p1.m_trs, p2.m_trs),
        k if k == ProcessField::M_SHARE as RowField => spaceship_number!(p1.m_share, p2.m_share),
        k if k == ProcessField::M_PRIV as RowField => spaceship_number!(p1.m_priv, p2.m_priv),
        k if k == ProcessField::M_PSS as RowField => spaceship_number!(p1.m_pss, p2.m_pss),
        k if k == ProcessField::M_SWAP as RowField => spaceship_number!(p1.m_swap, p2.m_swap),
        k if k == ProcessField::M_PSSWP as RowField => spaceship_number!(p1.m_psswp, p2.m_psswp),
        k if k == ProcessField::UTIME as RowField => spaceship_number!(p1.utime, p2.utime),
        k if k == ProcessField::CUTIME as RowField => spaceship_number!(p1.cutime, p2.cutime),
        k if k == ProcessField::STIME as RowField => spaceship_number!(p1.stime, p2.stime),
        k if k == ProcessField::CSTIME as RowField => spaceship_number!(p1.cstime, p2.cstime),
        k if k == ProcessField::RCHAR as RowField => spaceship_number!(p1.io_rchar, p2.io_rchar),
        k if k == ProcessField::WCHAR as RowField => spaceship_number!(p1.io_wchar, p2.io_wchar),
        k if k == ProcessField::SYSCR as RowField => spaceship_number!(p1.io_syscr, p2.io_syscr),
        k if k == ProcessField::SYSCW as RowField => spaceship_number!(p1.io_syscw, p2.io_syscw),
        k if k == ProcessField::RBYTES as RowField => {
            spaceship_number!(p1.io_read_bytes, p2.io_read_bytes)
        }
        k if k == ProcessField::WBYTES as RowField => {
            spaceship_number!(p1.io_write_bytes, p2.io_write_bytes)
        }
        k if k == ProcessField::CNCLWB as RowField => {
            spaceship_number!(p1.io_cancelled_write_bytes, p2.io_cancelled_write_bytes)
        }
        k if k == ProcessField::IO_READ_RATE as RowField => {
            compareRealNumbers(p1.io_rate_read_bps, p2.io_rate_read_bps)
        }
        k if k == ProcessField::IO_WRITE_RATE as RowField => {
            compareRealNumbers(p1.io_rate_write_bps, p2.io_rate_write_bps)
        }
        k if k == ProcessField::IO_RATE as RowField => {
            compareRealNumbers(LinuxProcess_totalIORate(p1), LinuxProcess_totalIORate(p2))
        }
        k if k == ProcessField::CGROUP as RowField => spaceship_nullstr!(
            p1.cgroup.as_deref().map(str::as_bytes),
            p2.cgroup.as_deref().map(str::as_bytes)
        ),
        k if k == ProcessField::CCGROUP as RowField => spaceship_nullstr!(
            p1.cgroup_short.as_deref().map(str::as_bytes),
            p2.cgroup_short.as_deref().map(str::as_bytes)
        ),
        k if k == ProcessField::CONTAINER as RowField => spaceship_nullstr!(
            p1.container_short.as_deref().map(str::as_bytes),
            p2.container_short.as_deref().map(str::as_bytes)
        ),
        k if k == ProcessField::OOM as RowField => spaceship_number!(p1.oom, p2.oom),
        k if k == ProcessField::PERCENT_CPU_DELAY as RowField => {
            compareRealNumbers(p1.cpu_delay_percent as f64, p2.cpu_delay_percent as f64)
        }
        k if k == ProcessField::PERCENT_IO_DELAY as RowField => {
            compareRealNumbers(p1.blkio_delay_percent as f64, p2.blkio_delay_percent as f64)
        }
        k if k == ProcessField::PERCENT_SWAP_DELAY as RowField => compareRealNumbers(
            p1.swapin_delay_percent as f64,
            p2.swapin_delay_percent as f64,
        ),
        k if k == ProcessField::IO_PRIORITY as RowField => spaceship_number!(
            LinuxProcess_effectiveIOPriority(p1),
            LinuxProcess_effectiveIOPriority(p2)
        ),
        k if k == ProcessField::CTXT as RowField => spaceship_number!(p1.ctxt_diff, p2.ctxt_diff),
        k if k == ProcessField::SECATTR as RowField => spaceship_nullstr!(
            p1.secattr.as_deref().map(str::as_bytes),
            p2.secattr.as_deref().map(str::as_bytes)
        ),
        k if k == ProcessField::AUTOGROUP_ID as RowField => {
            spaceship_number!(p1.autogroup_id, p2.autogroup_id)
        }
        k if k == ProcessField::AUTOGROUP_NICE as RowField => {
            spaceship_number!(p1.autogroup_nice, p2.autogroup_nice)
        }
        k if k == ProcessField::GPU_PERCENT as RowField => {
            let r = compareRealNumbers(p1.gpu_percent as f64, p2.gpu_percent as f64);
            if r != 0 {
                r
            } else {
                spaceship_number!(p1.gpu_time, p2.gpu_time)
            }
        }
        k if k == ProcessField::GPU_TIME as RowField => spaceship_number!(p1.gpu_time, p2.gpu_time),
        k if k == ProcessField::ISCONTAINER as RowField => spaceship_number!(
            p1.super_.isRunningInContainer as i32,
            p2.super_.isRunningInContainer as i32
        ),
        _ => Process_compareByKey_Base(&p1.super_, &p2.super_, key as RowField),
    }
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

    /// Spot-checks the [`Process_fields`] transcription against
    /// `linux/LinuxProcess.c`: the table length is `LAST_PROCESSFIELD`, the
    /// unused index 0 is empty, representative entries carry the exact
    /// name/title/flags/bool set, and the `HAVE_OPENVZ`/`HAVE_VSERVER`-gated
    /// slots stay empty in this build.
    #[test]
    fn process_fields_table_matches_c() {
        assert_eq!(Process_fields.len(), LAST_PROCESSFIELD);
        assert_eq!(LAST_PROCESSFIELD, 135);

        let empty = &Process_fields[0];
        assert_eq!(empty.name, "");
        assert!(empty.title.is_none());

        let pid = &Process_fields[ProcessField::PID as usize];
        assert_eq!(pid.name, "PID");
        assert_eq!(pid.title, Some("PID"));
        assert!(pid.pidColumn);
        assert!(!pid.defaultSortDesc);

        // Trailing-space titles are significant (column width).
        assert_eq!(
            Process_fields[ProcessField::COMM as usize].title,
            Some("Command ")
        );

        // PERCENT_CPU: desc + autoWidth + autoTitleRightAlign.
        let cpu = &Process_fields[ProcessField::PERCENT_CPU as usize];
        assert!(cpu.defaultSortDesc && cpu.autoWidth && cpu.autoTitleRightAlign);

        // Flag transcription.
        assert_eq!(
            Process_fields[ProcessField::M_LRS as usize].flags,
            PROCESS_FLAG_LINUX_LRS_FIX
        );
        assert_eq!(
            Process_fields[ProcessField::RCHAR as usize].flags,
            PROCESS_FLAG_IO
        );
        assert_eq!(
            Process_fields[ProcessField::SCHEDULERPOLICY as usize].flags,
            PROCESS_FLAG_SCHEDPOL
        );

        // OPENVZ/VSERVER off → these slots are the empty default.
        assert_eq!(Process_fields[ProcessField::CTID as usize].name, "");
        assert_eq!(Process_fields[ProcessField::VXID as usize].name, "");
    }

    /// [`LinuxProcess_compareByKey`]: platform fields compare on the
    /// concrete `LinuxProcess` data, GPU_PERCENT breaks ties by `gpu_time`,
    /// and an unhandled reserved key delegates to
    /// [`Process_compareByKey_Base`] (which orders by PID).
    #[test]
    fn compare_by_key_orders_platform_and_delegates_base() {
        let mut a = LinuxProcess_new(core::ptr::null());
        let mut b = LinuxProcess_new(core::ptr::null());

        a.utime = 10;
        b.utime = 20;
        assert!(
            LinuxProcess_compareByKey(
                &a as &dyn Object,
                &b as &dyn Object,
                ProcessField::UTIME as RowField
            ) < 0
        );
        assert!(
            LinuxProcess_compareByKey(
                &b as &dyn Object,
                &a as &dyn Object,
                ProcessField::UTIME as RowField
            ) > 0
        );

        // GPU_PERCENT ties break on gpu_time.
        a.gpu_percent = 5.0;
        b.gpu_percent = 5.0;
        a.gpu_time = 100;
        b.gpu_time = 200;
        assert!(
            LinuxProcess_compareByKey(
                &a as &dyn Object,
                &b as &dyn Object,
                ProcessField::GPU_PERCENT as RowField
            ) < 0
        );

        // Reserved key (PID) → base comparison, ordered by Row id.
        a.super_.super_.id = 7;
        b.super_.super_.id = 3;
        assert!(
            LinuxProcess_compareByKey(
                &a as &dyn Object,
                &b as &dyn Object,
                ProcessField::PID as RowField
            ) > 0
        );
    }

    /// [`LinuxProcess_rowWriteField`] renders Linux platform fields and
    /// delegates other keys to the base [`Process_writeField`]. Uses fields
    /// with fixed widths (no dependence on the pid/uid digit globals).
    #[test]
    fn row_write_field_renders_linux_and_delegates() {
        use crate::ported::process::ProcessState;
        use crate::ported::richstring::{RichString, RichString_size};
        use crate::ported::settings::RowField;
        use crate::ported::settings::Settings;

        let mut machine = Machine::default();
        machine.settings = Some(Settings::default());
        let mut lp = LinuxProcess_new(core::ptr::null());
        lp.super_.super_.host = &machine as *const Machine as *const c_void;

        let render = |lp: &LinuxProcess, field: RowField| -> i32 {
            let mut rs = RichString::default();
            LinuxProcess_rowWriteField(lp as &dyn Object, &mut rs, field);
            RichString_size(&rs)
        };

        // OOM renders "%4u " unconditionally (C has no N/A sentinel): a small
        // value pads to width 4 + trailing space = 5 cols; a 10-digit value
        // overflows the field to 10 + space = 11.
        lp.oom = 3;
        assert_eq!(render(&lp, ProcessField::OOM as RowField), 5); // "   3 "
        lp.oom = u32::MAX;
        assert_eq!(render(&lp, ProcessField::OOM as RowField), 11); // "4294967295 "
                                                                    // ISCONTAINER default (TRI_INITIAL) → "N/A  " (5).
        assert_eq!(render(&lp, ProcessField::ISCONTAINER as RowField), 5);
        // A base field (STATE running) delegates to Process_writeField → "R ".
        lp.super_.state = ProcessState::RUNNING;
        assert_eq!(render(&lp, ProcessField::STATE as RowField), 2);
    }

    /// The full display vtable chain: `Object::display` → `Row_display` →
    /// `row_class().writeField` (`LinuxProcess_rowWriteField`) renders each
    /// active-screen field. Confirms the `RowClass` vtable dispatch resolves
    /// to the `LinuxProcess` slot.
    #[test]
    fn display_dispatches_writefield_through_rowclass_vtable() {
        use crate::ported::process::ProcessState;
        use crate::ported::richstring::{RichString, RichString_size};
        use crate::ported::settings::{RowField, ScreenSettings, Settings};

        let mut machine = Machine::default();
        let mut settings = Settings::default();
        settings.screens = vec![ScreenSettings {
            fields: vec![
                ProcessField::PID as RowField,
                ProcessField::STATE as RowField,
            ],
            ..Default::default()
        }];
        machine.settings = Some(settings);

        let mut lp = LinuxProcess_new(core::ptr::null());
        lp.super_.super_.host = &machine as *const Machine as *const c_void;
        lp.super_.state = ProcessState::RUNNING;

        // Dispatch through the Object::display slot (= Row_display).
        let mut out = RichString::default();
        (&lp as &dyn Object).display(&mut out);
        // PID (>=6 cols) + STATE "R " (2) were both written by the vtable slot.
        assert!(RichString_size(&out) >= 8);
    }

    /// [`Process_compare`] reads the active sort key from the host settings and
    /// dispatches the concrete `LinuxProcess` `compareByKey` slot (via
    /// `process_class`), then applies the sort direction.
    #[test]
    fn process_compare_dispatches_platform_key_and_direction() {
        use crate::ported::process::Process_compare;
        use crate::ported::settings::{ScreenSettings, Settings};

        let mut machine = Machine::default();
        let mut settings = Settings::default();
        settings.screens = vec![ScreenSettings {
            fields: vec![ProcessField::UTIME as RowField],
            sortKey: ProcessField::UTIME as RowField,
            direction: 1,
            ..Default::default()
        }];
        machine.settings = Some(settings);

        let mut a = LinuxProcess_new(core::ptr::null());
        let mut b = LinuxProcess_new(core::ptr::null());
        a.super_.super_.host = &machine as *const Machine as *const c_void;
        b.super_.super_.host = &machine as *const Machine as *const c_void;
        a.utime = 10;
        b.utime = 20;
        a.super_.super_.id = 1;
        b.super_.super_.id = 2;

        // Ascending UTIME: a(10) < b(20).
        assert!(Process_compare(&a as &dyn Object, &b as &dyn Object) < 0);
        assert!(Process_compare(&b as &dyn Object, &a as &dyn Object) > 0);

        // Descending flips the result.
        machine.settings.as_mut().unwrap().screens[0].direction = -1;
        assert!(Process_compare(&a as &dyn Object, &b as &dyn Object) > 0);
    }
}
