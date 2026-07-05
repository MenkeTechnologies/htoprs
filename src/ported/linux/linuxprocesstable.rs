//! Partial port of `linux/LinuxProcessTable.c` — the Linux `/proc` process
//! scanner and its `LinuxProcessTable` container.
//!
//! Ported: the fast integer parsers (`fast_strto*`), `strtopid`,
//! `sortTtyDrivers`, `LinuxProcessTable_getProcessState`,
//! `LinuxProcessTable_adjustTime`, `fopenat`, `LinuxProcessTable_initTtyDrivers`,
//! `ProcessTable_new`, `readFileDynamic`, `isOlderThan`, and the per-process
//! `/proc` file readers that only need already-ported substrate:
//! `readStatFile`, `readStatusFile`, `readStatmFile`, `readOomData`,
//! `readAutogroup`, `readCwd`, `readIoFile`, `readCGroupFile`,
//! `readSecattrData`, `LinuxProcessList_readExe`,
//! `LinuxProcessTable_readCmdlineFile`, `LinuxProcessList_readComm`,
//! `readSmapsFile` (with `skipEndOfLine`), `updateTtyDevice` (with glibc
//! `major`/`minor`).
//!
//! `readMaps` + `calcLibSize_helper` are ported: `LibraryData` is modeled as an
//! `Object` with `Cell` fields, so the per-inode aggregate is updated in place
//! through the shared `&dyn Object` that `Hashtable_get` returns (the faithful
//! analog of the C's `libdata->size += …` through the non-owning `void*`).
//!
//! Still stubbed (each fn's doc gives the precise blocker): the scan drivers
//! `LinuxProcessTable_recurseProcTree` / `ProcessTable_goThroughEntries`
//! (need the process-typed `ProcessTable_getProcess`/`_add`, still stubbed in
//! `processtable.rs`); `ProcessTable_delete` (pure `free()` teardown → `Drop`);
//! and `readOpenVZData` (`#ifdef HAVE_OPENVZ` reader needing the unmodeled
//! `ctid`/`vpid` fields).
#![allow(non_snake_case)]
// `LibraryData_class` and similar keep the C-style `Type_class` naming.
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::ffi::CStr;
use std::os::unix::io::FromRawFd;
use std::sync::atomic::{AtomicU64, Ordering};

use libc::ssize_t;

use crate::ported::gpumeter::GPUMeter_active;
use crate::ported::hashtable::{
    Hashtable_delete, Hashtable_foreach, Hashtable_get, Hashtable_new, Hashtable_put,
};
use crate::ported::linux::cgrouputils::{CGroup_filterContainer, CGroup_filterName};
use crate::ported::linux::compat::{
    openat_arg_t, Compat_faccessat, Compat_openat, Compat_openatArgClose, Compat_readfile,
    Compat_readfileat,
};
use crate::ported::linux::gpu::GPU_readProcessData;
use crate::ported::linux::linuxmachine::LinuxMachine;
use crate::ported::linux::linuxprocess::{
    LinuxProcess, LinuxProcess_isAutogroupEnabled, LinuxProcess_new, LinuxProcess_updateIOPriority,
    PROCESS_FLAG_LINUX_AUTOGROUP, PROCESS_FLAG_LINUX_CGROUP, PROCESS_FLAG_LINUX_CONTAINER,
    PROCESS_FLAG_LINUX_CTXT, PROCESS_FLAG_LINUX_GPU, PROCESS_FLAG_LINUX_IOPRIO,
    PROCESS_FLAG_LINUX_LRS_FIX, PROCESS_FLAG_LINUX_OOM, PROCESS_FLAG_LINUX_SECATTR,
    PROCESS_FLAG_LINUX_SMAPS,
};
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::process::{
    Process, ProcessField, ProcessState, Process_fillStarttimeBuffer, Process_getPid,
    Process_isKernelThread, Process_isUserlandThread, Process_setParent, Process_setThreadGroup,
    Process_updateCPUFieldWidths, Process_updateCmdline, Process_updateComm, Process_updateExe,
    Tristate, PROCESS_FLAG_CWD, PROCESS_FLAG_IO, PROCESS_FLAG_SCHEDPOL,
};
use crate::ported::processtable::{ProcessTable, ProcessTable_init};
use crate::ported::row::{spaceship_number, Row_updateFieldWidth};
use crate::ported::scheduling::Scheduling_readProcessPolicy;
use crate::ported::settings::RowField;
use crate::ported::userstable::{UsersTable, UsersTable_getRef};
use crate::ported::xutils::{
    saturatingSub, String_eq, String_safeStrncpy, String_startsWith, String_strchrnul,
};

/// Port of `#define PROCDIR "/proc"` (`LinuxMachine.h:105`).
const PROCDIR: &str = "/proc";
/// Port of `#define PROCTTYDRIVERSFILE PROCDIR "/tty/drivers"`
/// (`LinuxMachine.h:125`).
const PROCTTYDRIVERSFILE: &CStr = c"/proc/tty/drivers";
/// Port of `#define PROC_LINE_LENGTH 4096` (`LinuxMachine.h:129`).
const PROC_LINE_LENGTH: usize = 4096;
/// Port of `#define MAX_READ 2048` (`Machine.h:32`).
const MAX_READ: usize = 2048;
/// Port of `#define MAX_NAME 128` (`Machine.h:28`).
const MAX_NAME: usize = 128;
/// Port of `PATH_MAX` (`Compat.c:24` / `limits.h`; 4096 on Linux).
const PATH_MAX: usize = 4096;
/// Port of `#define PF_KTHREAD 0x00200000` (`LinuxProcessTable.c:61`).
const PF_KTHREAD: u64 = 0x0020_0000;
/// Port of `#define MAX_CMDLINE_BUFFER_SIZE (2 * 1024 * 1024 + 512)`
/// (`LinuxProcessTable.c:65`).
const MAX_CMDLINE_BUFFER_SIZE: usize = 2 * 1024 * 1024 + 512;

/// Port of `static ino_t rootPidNs = (ino_t)-1;` (`LinuxProcessTable.c:68`),
/// the inode number of htop's own PID namespace. The C `(ino_t)-1` sentinel
/// is modeled as `u64::MAX` in an [`AtomicU64`] (a module-private mutable C
/// static; see the `Row_uidDigits` idiom).
#[allow(non_upper_case_globals)] // faithful C identifier `rootPidNs`
static rootPidNs: AtomicU64 = AtomicU64::new(u64::MAX);

/// Port of `struct TtyDriver_` (`LinuxProcessTable.h:15`). One row of
/// `/proc/tty/drivers`, describing a tty driver's major/minor range and
/// the device-node path prefix used to reconstruct a TTY name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TtyDriver {
    /// C `char* path` — device-node path prefix (e.g. `/dev/ttyS`).
    /// `None` marks the sentinel terminator entry of the C array.
    pub path: Option<String>,
    /// C `unsigned int major`.
    pub major: u32,
    /// C `unsigned int minorFrom`.
    pub minorFrom: u32,
    /// C `unsigned int minorTo`.
    pub minorTo: u32,
}

/// Port of `struct LinuxProcessTable_` (`LinuxProcessTable.h:22`).
/// "Extends" [`ProcessTable`] via the embedded `super_` field and adds
/// the Linux-specific tty-driver table and capability flags. The
/// `#ifdef HAVE_DELAYACCT` netlink fields are present under
/// `cfg(target_os = "linux")` — the delayacct build is modeled as
/// Linux-only (see the `libnl` module); non-Linux mirrors the
/// `HAVE_DELAYACCT`-off variant that omits them.
pub struct LinuxProcessTable {
    /// C `ProcessTable super` — the embedded base table.
    pub super_: ProcessTable,
    /// C `TtyDriver* ttyDrivers` — NUL-path-terminated, major/minor-sorted
    /// array; `None` until `LinuxProcessTable_initTtyDrivers` runs.
    pub ttyDrivers: Option<Vec<TtyDriver>>,
    /// C `bool haveSmapsRollup` — `/proc/self/smaps_rollup` available.
    pub haveSmapsRollup: bool,
    /// C `bool haveAutogroup` — autogroup scheduling supported.
    pub haveAutogroup: bool,
    /// C `int netlink_family` (`#ifdef HAVE_DELAYACCT`) — the resolved
    /// TASKSTATS generic-netlink family id (or `-1` if unresolved).
    #[cfg(target_os = "linux")]
    pub netlink_family: i32,
    /// C `struct nl_sock* netlink_socket` (`#ifdef HAVE_DELAYACCT`) — the
    /// persistent generic-netlink socket used for delay-accounting queries.
    /// `neli`'s `NlSocketHandle` replaces libnl's opaque `nl_sock`; `None`
    /// until `initNetlinkSocket` opens it (see the `libnl` module).
    #[cfg(target_os = "linux")]
    pub netlink_socket: Option<neli::socket::NlSocketHandle>,
}

/// Port of `LinuxProcessTable.c:71`. Opens `pathname` (relative to the
/// directory handle `openatArg`) via [`Compat_openat`] and wraps the fd in a
/// buffered file handle. The C `FILE*` is modeled as an owned
/// [`std::fs::File`] (the `TraceScreen`/`OpenFilesScreen` idiom); `None`
/// mirrors the C `NULL` return. Only `mode == "r"` is supported, matching the
/// C `assert(String_eq(mode, "r"))`. `File::from_raw_fd` cannot fail, so the
/// C `fdopen` NULL branch (`close(fd)`) is unreachable.
fn fopenat(openatArg: openat_arg_t, pathname: &CStr, mode: &str) -> Option<std::fs::File> {
    debug_assert!(mode == "r"); // only currently supported mode

    let fd = Compat_openat(openatArg, pathname, libc::O_RDONLY);
    if fd < 0 {
        return None;
    }

    Some(unsafe { std::fs::File::from_raw_fd(fd) })
}

/// Port of `LinuxProcessTable.c:85`. Parse a `/proc` directory name into
/// a pid, returning `0` (an invalid pid) on failure. Mirrors the C
/// `strtoul(str, &endptr, 10)` semantics: leading whitespace and an
/// optional sign are consumed, the whole remaining string must be
/// digits, and the value must be in `1 ..< INT_MAX`.
pub fn strtopid(str: &str) -> i32 {
    let bytes = str.as_bytes();
    let mut i = 0;

    // strtoul() skips leading whitespace.
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    // strtoul() accepts an optional leading sign.
    let mut neg = false;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        neg = bytes[i] == b'-';
        i += 1;
    }

    let mut parsed: u64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        parsed = parsed
            .wrapping_mul(10)
            .wrapping_add((bytes[i] - b'0') as u64);
        i += 1;
    }

    // *endptr != '\0' — trailing characters remain unconsumed.
    let endptr_is_nul = i == bytes.len();
    let parsed_pid = if neg { parsed.wrapping_neg() } else { parsed };

    if parsed_pid == 0 || parsed_pid >= i32::MAX as u64 || !endptr_is_nul {
        return 0; // indicate failure by an invalid pid
    }

    parsed_pid as i32
}

/// Port of `LinuxProcessTable.c:93`. Parse an unsigned decimal integer
/// from the front of `str`, advancing the cursor past the digits it
/// consumes. `maxlen == 0` means "no explicit cap" (defaults to 20, the
/// digit count of `u64::MAX`). The C `char**` cursor is modeled as a
/// `&mut &[u8]` byte slice; end-of-string (empty slice) reads as the NUL
/// terminator, which is not a digit and stops the loop.
pub fn fast_strtoull_dec(str: &mut &[u8], mut maxlen: usize) -> u64 {
    let mut result: u64 = 0;

    if maxlen == 0 {
        maxlen = 20; // length of maximum value of 18446744073709551615
    }

    while maxlen > 0 {
        match str.first() {
            Some(&c) if c.is_ascii_digit() => {
                result = result.wrapping_mul(10);
                result = result.wrapping_add((c - b'0') as u64);
                *str = &str[1..];
            }
            _ => break,
        }
        maxlen -= 1;
    }

    result
}

/// Port of `LinuxProcessTable.c:108`. Signed decimal variant of
/// [`fast_strtoull_dec`]: consumes an optional leading `-`, then the
/// magnitude, and negates as needed.
pub fn fast_strtoll_dec(str: &mut &[u8], maxlen: usize) -> i64 {
    let mut neg = false;

    if str.first() == Some(&b'-') {
        neg = true;
        *str = &str[1..];
    }

    let res = fast_strtoull_dec(str, maxlen);
    debug_assert!(res <= i64::MAX as u64);
    let result = res as i64;

    if neg {
        -result
    } else {
        result
    }
}

/// Port of `LinuxProcessTable.c:123`. `int` decimal variant of
/// [`fast_strtoll_dec`]; `maxlen == 0` defaults to 10 (digit count of
/// `i32::MAX`).
pub fn fast_strtoi_dec(str: &mut &[u8], mut maxlen: usize) -> i32 {
    if maxlen == 0 {
        maxlen = 10; // length of maximum value of 2147483647
    }
    let result = fast_strtoll_dec(str, maxlen);
    debug_assert!(result <= i32::MAX as i64);
    debug_assert!(result >= i32::MIN as i64);
    result as i32
}

/// Port of `LinuxProcessTable.c:132`. `long` decimal variant of
/// [`fast_strtoll_dec`]. On this (LP64) target `long` is 64-bit, so the
/// C `LONG_MIN`/`LONG_MAX` bounds asserts are tautological.
pub fn fast_strtol_dec(str: &mut &[u8], maxlen: usize) -> i64 {
    fast_strtoll_dec(str, maxlen)
}

/// Port of `LinuxProcessTable.c:139`. `unsigned long` decimal variant of
/// [`fast_strtoull_dec`]. On this (LP64) target `unsigned long` is
/// 64-bit, so the C `ULONG_MAX` bounds assert is tautological.
pub fn fast_strtoul_dec(str: &mut &[u8], maxlen: usize) -> u64 {
    fast_strtoull_dec(str, maxlen)
}

/// Port of `LinuxProcessTable.c:145`. Parse an unsigned hexadecimal
/// integer from the front of `str`, advancing the cursor. `maxlen == 0`
/// defaults to 18 (digit count of `0xffffffffffffffff`). Faithful to the
/// C bit-twiddling nibble decoder: `valid_mask` gates on the low 5 bits
/// of the byte, then the byte is folded to upper case and mapped to its
/// 0–15 nibble value.
pub fn fast_strtoull_hex(str: &mut &[u8], mut maxlen: usize) -> u64 {
    let mut result: u64 = 0;
    let valid_mask: i64 = 0x03FF007E;

    if maxlen == 0 {
        maxlen = 18; // length of maximum value of 0xffffffffffffffff
    }

    while maxlen > 0 {
        maxlen -= 1;

        // (unsigned char)**str — end-of-string reads as NUL (0).
        let mut nibble: i32 = match str.first() {
            Some(&c) => c as i32,
            None => 0,
        };

        if (valid_mask & (1i64 << (nibble & 0x1F))) == 0 {
            break;
        }
        if nibble < b'0' as i32 || (nibble & !0x20) > b'F' as i32 {
            break;
        }
        let letter = if (nibble & 0x40) != 0 {
            b'A' as i32 - b'9' as i32 - 1
        } else {
            0
        };
        nibble &= !0x20; // to upper
        nibble ^= 0x10; // switch letters and digits
        nibble -= letter;
        nibble &= 0x0f;
        result <<= 4;
        result += nibble as u64;
        *str = &str[1..];
    }

    result
}

/// Port of `LinuxProcessTable.c:172`. `qsort` comparator ordering
/// [`TtyDriver`] entries by `major`, then `minorFrom`. Returns the C
/// three-way `-1`/`0`/`1` result.
pub fn sortTtyDrivers(va: &TtyDriver, vb: &TtyDriver) -> i32 {
    let a = va;
    let b = vb;

    let r = spaceship_number!(a.major, b.major);
    if r != 0 {
        return r;
    }

    spaceship_number!(a.minorFrom, b.minorFrom)
}

/// Port of `LinuxProcessTable.c:183`. Reads and parses `/proc/tty/drivers`
/// into the major/minor-sorted [`TtyDriver`] table stored on `this`. Each
/// line has the form `name  nodepath  major  minor-range  type`. The C
/// in-place `strchr`/`atoi` tokenizer is expressed here with line/whitespace
/// splitting yielding the same fields; a partial (truncated) final line is
/// dropped, matching the C `goto finish` bail-outs. The trailing
/// `path == NULL` sentinel the C array carries is modeled as a final
/// [`TtyDriver`] with `path: None`.
fn LinuxProcessTable_initTtyDrivers(this: &mut LinuxProcessTable) {
    let mut buf = [0u8; 16384];
    let r = Compat_readfile(PROCTTYDRIVERSFILE, &mut buf);
    if r < 0 {
        return;
    }

    // atoi(): parse the leading run of decimal digits, defaulting to 0.
    let atoi = |s: &str| -> u32 {
        let mut v: u32 = 0;
        for c in s.bytes() {
            if !c.is_ascii_digit() {
                break;
            }
            v = v.wrapping_mul(10).wrapping_add((c - b'0') as u32);
        }
        v
    };

    let text = &buf[..r as usize];
    let mut ttyDrivers: Vec<TtyDriver> = Vec::new();

    for line in text.split(|&c| c == b'\n') {
        // [name]  [node path]  [major]  [minor range]  [type]
        let line = match std::str::from_utf8(line) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut it = line.split_whitespace();

        // skip first token (name)
        if it.next().is_none() {
            continue;
        }
        let path = match it.next() {
            Some(p) => p,
            None => continue, // truncation
        };
        let major = match it.next() {
            Some(m) => atoi(m),
            None => continue, // truncation
        };
        let minor = match it.next() {
            Some(m) => m,
            None => continue, // truncation
        };

        let (minorFrom, minorTo) = match minor.split_once('-') {
            Some((from, to)) => (atoi(from), atoi(to)),
            None => (atoi(minor), atoi(minor)),
        };

        ttyDrivers.push(TtyDriver {
            path: Some(path.to_string()),
            major,
            minorFrom,
            minorTo,
        });
    }

    // qsort(ttyDrivers, numDrivers, ...): sort the real entries only.
    ttyDrivers.sort_by(|a, b| match sortTtyDrivers(a, b) {
        n if n < 0 => std::cmp::Ordering::Less,
        0 => std::cmp::Ordering::Equal,
        _ => std::cmp::Ordering::Greater,
    });

    // ttyDrivers[numDrivers].path = NULL; — the sentinel terminator.
    ttyDrivers.push(TtyDriver::default());

    this.ttyDrivers = Some(ttyDrivers);
}

/// Port of `LinuxProcessTable.c:261`. Constructs the Linux process table:
/// initializes the embedded base [`ProcessTable`], loads the tty-driver
/// table, probes `/proc/self/smaps_rollup` availability, and records htop's
/// own PID-namespace inode in the `rootPidNs` static.
///
/// Signature mapping: the C `xCalloc` + `Object_setClass` heap allocation is
/// modeled as an owned [`LinuxProcessTable`] value (the `LinuxProcess_new`
/// idiom); class identity is the Rust type. The C returns `&this->super`
/// (a `ProcessTable*` upcast); the owning caller here keeps the concrete
/// value. `Hashtable* pidMatchList` is the opaque [`Option<usize>`] handle.
pub fn ProcessTable_new(host: *const Machine, pidMatchList: Option<usize>) -> LinuxProcessTable {
    let mut this = LinuxProcessTable {
        super_: ProcessTable::empty(),
        ttyDrivers: None,
        haveSmapsRollup: false,
        haveAutogroup: false,
        #[cfg(target_os = "linux")]
        netlink_family: -1,
        #[cfg(target_os = "linux")]
        netlink_socket: None,
    };

    ProcessTable_init(&mut this.super_, host, pidMatchList);

    LinuxProcessTable_initTtyDrivers(&mut this);

    // Test /proc/PID/smaps_rollup availability (faster to parse, Linux 4.14+)
    this.haveSmapsRollup =
        unsafe { libc::access(c"/proc/self/smaps_rollup".as_ptr(), libc::R_OK) } == 0;

    // Read PID namespace inode number
    let mut sb: libc::stat = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::stat(c"/proc/self/ns/pid".as_ptr(), &mut sb) };
    if r == 0 {
        rootPidNs.store(sb.st_ino as u64, Ordering::Relaxed);
    } else {
        rootPidNs.store(u64::MAX, Ordering::Relaxed);
    }

    this
}

/// Port of `void ProcessTable_delete(Object* cast)` from
/// `LinuxProcessTable.c:287`. The C body calls `ProcessTable_done(&this->super)`,
/// then `free`s each `ttyDrivers[i].path` and the array, does the
/// `#ifdef HAVE_DELAYACCT` netlink-socket destroy (omitted here — the
/// non-delayacct build variant this module commits to), and `free(this)`. Take
/// `this` by value: `ProcessTable_done` tears the base table down in place,
/// the `Option<Vec<TtyDriver>>` / `Option<String>` fields drop when `this`
/// falls out of scope (the `free(this)`), matching the darwin
/// `ProcessTable_delete` precedent.
pub fn ProcessTable_delete(mut this: LinuxProcessTable) {
    crate::ported::processtable::ProcessTable_done(&mut this.super_);
}

/// Port of `LinuxProcessTable.c:302`. Rescales a jiffy-denominated time `t`
/// to hundredths of a second using the host's `USER_HZ` (`jiffies`).
fn LinuxProcessTable_adjustTime(lhost: &LinuxMachine, t: u64) -> u64 {
    t * 100 / lhost.jiffies as u64
}

/// Port of `LinuxProcessTable.c:307`. Map the single-character process
/// state from `/proc/<pid>/stat` to a [`ProcessState`]. Taken from the
/// Linux kernel `fs/proc/array.c` state table.
pub fn LinuxProcessTable_getProcessState(state: u8) -> ProcessState {
    match state {
        b'S' => ProcessState::SLEEPING,
        b'X' => ProcessState::DEFUNCT,
        b'Z' => ProcessState::ZOMBIE,
        b't' => ProcessState::TRACED,
        b'T' => ProcessState::STOPPED,
        b'D' => ProcessState::UNINTERRUPTIBLE_WAIT,
        b'R' => ProcessState::RUNNING,
        b'P' => ProcessState::BLOCKED,
        b'I' => ProcessState::IDLE,
        _ => ProcessState::UNKNOWN,
    }
}

/// Port of `LinuxProcessTable.c:325`. Reads and parses `/proc/<pid>/stat`
/// (thread-specific data) into the [`LinuxProcess`]/[`Process`] fields,
/// copying the parenthesized `comm` into `command`. The C `char* location`
/// cursor is modeled as a byte index `loc` into the NUL-terminated read
/// buffer (out-of-range / past-NUL reads model as `0`); each field is read
/// via the `fast_strto*` helpers (which advance a `&mut &[u8]` suffix of the
/// buffer). `commLen` caps the `comm` copy exactly as the C `MINIMUM(...)`.
fn LinuxProcessTable_readStatFile(
    lp: &mut LinuxProcess,
    procFd: openat_arg_t,
    lhost: &LinuxMachine,
    scanMainThread: bool,
    command: &mut [u8],
    commLen: usize,
) -> bool {
    let mut buf = [0u8; MAX_READ + 1];

    // char path[22] = "stat"; task/<pid>/stat when scanning the main thread.
    let path = if scanMainThread {
        std::ffi::CString::new(format!("task/{}/stat", Process_getPid(&lp.super_))).unwrap()
    } else {
        std::ffi::CString::new("stat").unwrap()
    };
    let r = Compat_readfileat(procFd, &path, &mut buf);
    if r < 0 {
        return false;
    }

    // Byte at `i`, with past-end / NUL-region reads as 0 (NUL terminator).
    let byte = |i: usize| -> u8 { buf.get(i).copied().unwrap_or(0) };
    // strchr(&buf[from], ch): first index >= from holding ch before the NUL.
    let find = |from: usize, ch: u8| -> Option<usize> {
        let mut i = from;
        loop {
            let c = buf.get(i).copied().unwrap_or(0);
            if c == 0 {
                return None;
            }
            if c == ch {
                return Some(i);
            }
            i += 1;
        }
    };
    // fast_strto* helpers over the buffer suffix at `loc`, returning the new
    // cursor index alongside the value.
    let read_i = |loc: usize| -> (i32, usize) {
        let mut cur: &[u8] = &buf[loc..];
        let v = fast_strtoi_dec(&mut cur, 0);
        (v, buf.len() - cur.len())
    };
    let read_ul = |loc: usize| -> (u64, usize) {
        let mut cur: &[u8] = &buf[loc..];
        let v = fast_strtoul_dec(&mut cur, 0);
        (v, buf.len() - cur.len())
    };
    let read_ull = |loc: usize| -> (u64, usize) {
        let mut cur: &[u8] = &buf[loc..];
        let v = fast_strtoull_dec(&mut cur, 0);
        (v, buf.len() - cur.len())
    };
    let read_l = |loc: usize| -> (i64, usize) {
        let mut cur: &[u8] = &buf[loc..];
        let v = fast_strtol_dec(&mut cur, 0);
        (v, buf.len() - cur.len())
    };
    let read_ll = |loc: usize| -> (i64, usize) {
        let mut cur: &[u8] = &buf[loc..];
        let v = fast_strtoll_dec(&mut cur, 0);
        (v, buf.len() - cur.len())
    };

    /* (1) pid   -  %d */
    debug_assert_eq!(
        Process_getPid(&lp.super_),
        fast_strtoi_dec(&mut &buf[..], 0)
    );
    let mut loc = match find(0, b' ') {
        Some(i) => i,
        None => return false,
    };

    /* (2) comm  -  (%s) */
    if byte(loc) == 0 || byte(loc + 1) == 0 {
        return false;
    }
    loc += 2;
    // strrchr(location, ')')
    let end = {
        let mut e: Option<usize> = None;
        let mut i = loc;
        while byte(i) != 0 {
            if byte(i) == b')' {
                e = Some(i);
            }
            i += 1;
        }
        match e {
            Some(i) => i,
            None => return false,
        }
    };
    if end < loc {
        return false;
    }
    let size = core::cmp::min(end - loc + 1, commLen).min(command.len());
    if size > 0 {
        String_safeStrncpy(&mut command[..size], &buf[loc..]);
    }
    if byte(end) == 0 || byte(end + 1) == 0 {
        return false;
    }
    loc = end + 2;

    /* (3) state  -  %c */
    lp.super_.state = LinuxProcessTable_getProcessState(byte(loc));
    if byte(loc) == 0 || byte(loc + 1) == 0 {
        return false;
    }
    loc += 2;

    /* (4) ppid  -  %d */
    let (ppid, l) = read_i(loc);
    Process_setParent(&mut lp.super_, ppid);
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (5) pgrp  -  %d */
    let (v, l) = read_i(loc);
    lp.super_.pgrp = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (6) session  -  %d */
    let (v, l) = read_i(loc);
    lp.super_.session = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (7) tty_nr  -  %d */
    let (v, l) = read_ul(loc);
    lp.super_.tty_nr = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (8) tpgid  -  %d */
    let (v, l) = read_i(loc);
    lp.super_.tpgid = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (9) flags  -  %u */
    let (v, l) = read_ul(loc);
    lp.flags = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (10) minflt  -  %lu */
    let (v, l) = read_ull(loc);
    lp.super_.minflt = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (11) cminflt  -  %lu */
    let (v, l) = read_ull(loc);
    lp.cminflt = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (12) majflt  -  %lu */
    let (v, l) = read_ull(loc);
    lp.super_.majflt = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (13) cmajflt  -  %lu */
    let (v, l) = read_ull(loc);
    lp.cmajflt = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (14) utime  -  %lu */
    let (v, l) = read_ull(loc);
    lp.utime = LinuxProcessTable_adjustTime(lhost, v);
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (15) stime  -  %lu */
    let (v, l) = read_ull(loc);
    lp.stime = LinuxProcessTable_adjustTime(lhost, v);
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (16) cutime  -  %ld */
    let (v, l) = read_ull(loc);
    lp.cutime = LinuxProcessTable_adjustTime(lhost, v);
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (17) cstime  -  %ld */
    let (v, l) = read_ull(loc);
    lp.cstime = LinuxProcessTable_adjustTime(lhost, v);
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (18) priority  -  %ld */
    let (v, l) = read_l(loc);
    lp.super_.priority = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (19) nice  -  %ld */
    let (v, l) = read_i(loc);
    lp.super_.nice = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* (20) num_threads  -  %ld */
    let (v, l) = read_l(loc);
    lp.super_.nlwp = v;
    loc = l;
    if byte(loc) == 0 {
        return false;
    }
    loc += 1;

    /* Skip (21) itrealvalue  -  %ld */
    loc = match find(loc, b' ') {
        Some(i) => i,
        None => return false,
    };
    loc += 1;

    /* (22) starttime  -  %llu */
    if lp.super_.starttime_ctime == 0 {
        let (v, l) = read_ll(loc);
        lp.super_.starttime_ctime =
            lhost.boottime + (LinuxProcessTable_adjustTime(lhost, v as u64) / 100) as i64;
        loc = l;
    } else {
        loc = match find(loc, b' ') {
            Some(i) => i,
            None => return false,
        };
    }
    loc += 1;

    /* Skip (23) - (38) */
    for _ in 0..16 {
        loc = match find(loc, b' ') {
            Some(i) => i,
            None => return false,
        };
        loc += 1;
    }

    /* (39) processor  -  %d */
    let (v, _l) = read_i(loc);
    lp.super_.processor = v;

    /* Ignore further fields */

    lp.super_.time = lp.utime + lp.stime;

    true
}

/// Port of `LinuxProcessTable.c:549`. Reads `/proc/<pid>/status`, detecting
/// container membership (the `NSpid:` line listing more than one pid
/// namespace) and summing the voluntary + nonvoluntary context switches into
/// the [`LinuxProcess`] `ctxt_total`/`ctxt_diff` counters. The C `fgets`
/// line loop over a `FILE*` opened by [`fopenat`] becomes a buffered
/// line iterator; `sscanf(..., "\t%lu")` becomes a strip-prefix + trim +
/// parse.
///
/// Signature mapping: the C takes `Process*` and immediately downcasts to
/// `LinuxProcess*`; the faithful Rust receiver is the concrete
/// `&mut LinuxProcess`, reaching the base via `super_`.
fn LinuxProcessTable_readStatusFile(process: &mut LinuxProcess, procFd: openat_arg_t) -> bool {
    use std::io::BufRead;

    let mut ctxt: u64 = 0;
    process.super_.isRunningInContainer = Tristate::TRI_OFF;

    let statusfile = match fopenat(procFd, c"status", "r") {
        Some(f) => f,
        None => return false,
    };

    let reader = std::io::BufReader::new(statusfile);
    for line in reader.lines() {
        let buffer = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if String_startsWith(&buffer, "NSpid:") {
            // Count the distinct numeric fields (each a pid in one namespace).
            let bytes = buffer.as_bytes();
            let mut pid_ns_count = 0;
            let mut i = 0;
            while i < bytes.len() && !bytes[i].is_ascii_digit() {
                i += 1;
            }
            while i < bytes.len() {
                if bytes[i].is_ascii_digit() {
                    pid_ns_count += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                while i < bytes.len() && !bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }

            if pid_ns_count > 1 {
                process.super_.isRunningInContainer = Tristate::TRI_ON;
            }
        } else if String_startsWith(&buffer, "voluntary_ctxt_switches:") {
            if let Some(v) = buffer
                .strip_prefix("voluntary_ctxt_switches:")
                .and_then(|s| s.trim().parse::<u64>().ok())
            {
                ctxt += v;
            }
        } else if String_startsWith(&buffer, "nonvoluntary_ctxt_switches:") {
            if let Some(v) = buffer
                .strip_prefix("nonvoluntary_ctxt_switches:")
                .and_then(|s| s.trim().parse::<u64>().ok())
            {
                ctxt += v;
            }
        }
    }

    process.ctxt_diff = if ctxt > process.ctxt_total {
        ctxt - process.ctxt_total
    } else {
        0
    };
    process.ctxt_total = ctxt;

    true
}

/// Port of `static bool LinuxProcessTable_updateUser(const Machine* host,
/// Process* process, openat_arg_t procFd, const LinuxProcess* mainTask)` from
/// `LinuxProcessTable.c:628`.
///
/// A thread (`mainTask` present) copies its owning process's uid/user; a real
/// process `fstat`s its `/proc/<pid>` handle (`procFd`) and, when the owner uid
/// changed, resolves the name through the machine's `UsersTable` cache. `host->
/// usersTable` is the opaque `Option<usize>` handle (a separately-leaked
/// `UsersTable` reached by raw pointer, so the `&mut` never aliases `host` — the
/// `DarwinProcessTable_scanThreads` precedent); `UsersTable_getRef`'s borrowed
/// `&str` is cloned into the owned `Process::user`. `HAVE_OPENAT` is defined, so
/// the `fstat(procFd, &sb)` branch is the compiled form.
pub fn LinuxProcessTable_updateUser(
    host: &Machine,
    process: &mut Process,
    procFd: openat_arg_t,
    mainTask: Option<&LinuxProcess>,
) -> bool {
    // if (mainTask) { copy uid/user from the owning process; return true; }
    if let Some(mainTask) = mainTask {
        process.st_uid = mainTask.super_.st_uid;
        process.user = mainTask.super_.user.clone();
        return true;
    }

    // struct stat sb; int statok = fstat(procFd, &sb);
    let mut sb: libc::stat = unsafe { std::mem::zeroed() };
    let statok = unsafe { libc::fstat(procFd, &mut sb) };
    if statok == -1 {
        return false;
    }

    // if (process->st_uid != sb.st_uid) { update + resolve the user name }
    if process.st_uid != sb.st_uid {
        process.st_uid = sb.st_uid;
        // process->user = UsersTable_getRef(host->usersTable, sb.st_uid);
        process.user = match host.usersTable {
            Some(ut) => {
                // SAFETY: `usersTable` is the machine's leaked `*mut UsersTable`
                // handle; it is a distinct allocation, so the `&mut` cannot
                // alias `host`.
                let ut = unsafe { &mut *(ut as *mut UsersTable) };
                UsersTable_getRef(ut, sb.st_uid).map(str::to_owned)
            }
            None => None,
        };
    }

    true
}

/// Port of `static void LinuxProcessTable_readIoFile(LinuxProcess* lp,
/// openat_arg_t procFd, bool scanMainThread)` from `LinuxProcessTable.c:655`.
/// Reads `/proc/<pid>/io` (or `task/<tid>/io` when `scanMainThread`) and
/// updates the per-process IO counters and derived read/write byte rates.
/// A read failure resets every counter to its "unknown" sentinel
/// (`ULLONG_MAX` / `NAN`) and records the scan time. Otherwise the
/// `strsep(&buf, "\n")` line loop is reproduced with [`str::split`], and the
/// per-field prefixes (`rchar: `/`wchar: `/`read_bytes: `/`write_bytes: `/
/// `syscr: `/`syscw: `/`cancelled_write_bytes: `) are matched with
/// [`str::strip_prefix`], mirroring the C `line[i]` guards + `String_startsWith`.
/// The rates use [`saturatingSub`] on the byte and time deltas (`ms → s`),
/// yielding `NAN` when `time_delta == 0`, exactly as the C.
///
/// The C derives `host` from `process->super.super.host`; the ported reader
/// takes it as an explicit `&LinuxMachine` param (the [`LinuxProcessTable_readStatmFile`]
/// convention), reading `realtimeMs` from `host.super_`.
///
/// `strtoull(ptr, NULL, 10)` is the local `strtoull` closure: it skips leading
/// whitespace and an optional sign, then accumulates decimal digits saturating
/// at [`u64::MAX`] (C's `ULLONG_MAX`), matching `strtoull`'s overflow clamp.
fn LinuxProcessTable_readIoFile(
    lp: &mut LinuxProcess,
    procFd: openat_arg_t,
    host: &LinuxMachine,
    scanMainThread: bool,
) {
    let realtimeMs = host.super_.realtimeMs;

    // char path[20] = "io"; if (scanMainThread) snprintf "task/<pid>/io".
    let path = if scanMainThread {
        std::ffi::CString::new(format!("task/{}/io", Process_getPid(&lp.super_))).unwrap()
    } else {
        std::ffi::CString::new("io").unwrap()
    };

    let mut buffer = [0u8; 1024];
    let r = Compat_readfileat(procFd, &path, &mut buffer);
    if r < 0 {
        lp.io_rate_read_bps = f64::NAN;
        lp.io_rate_write_bps = f64::NAN;
        lp.io_rchar = u64::MAX;
        lp.io_wchar = u64::MAX;
        lp.io_syscr = u64::MAX;
        lp.io_syscw = u64::MAX;
        lp.io_read_bytes = u64::MAX;
        lp.io_write_bytes = u64::MAX;
        lp.io_cancelled_write_bytes = u64::MAX;
        lp.io_last_scan_time_ms = realtimeMs;
        return;
    }

    let last_read = lp.io_read_bytes;
    let last_write = lp.io_write_bytes;
    let time_delta = saturatingSub(realtimeMs, lp.io_last_scan_time_ms);

    // strtoull(s, NULL, 10): skip whitespace + optional sign, saturate at u64::MAX.
    let strtoull = |s: &str| -> u64 {
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let mut v: u64 = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            v = v.saturating_mul(10).saturating_add((b[i] - b'0') as u64);
            i += 1;
        }
        v
    };

    let text = std::str::from_utf8(&buffer[..r as usize]).unwrap_or("");
    for line in text.split('\n') {
        // C switches on line[0], then String_startsWith on the remainder.
        if let Some(rest) = line.strip_prefix("rchar: ") {
            lp.io_rchar = strtoull(rest);
        } else if let Some(rest) = line.strip_prefix("read_bytes: ") {
            lp.io_read_bytes = strtoull(rest);
            lp.io_rate_read_bps = if time_delta != 0 {
                saturatingSub(lp.io_read_bytes, last_read) as f64 * 1000. / time_delta as f64
            } else {
                f64::NAN
            };
        } else if let Some(rest) = line.strip_prefix("wchar: ") {
            lp.io_wchar = strtoull(rest);
        } else if let Some(rest) = line.strip_prefix("write_bytes: ") {
            lp.io_write_bytes = strtoull(rest);
            lp.io_rate_write_bps = if time_delta != 0 {
                saturatingSub(lp.io_write_bytes, last_write) as f64 * 1000. / time_delta as f64
            } else {
                f64::NAN
            };
        } else if let Some(rest) = line.strip_prefix("syscr: ") {
            lp.io_syscr = strtoull(rest);
        } else if let Some(rest) = line.strip_prefix("syscw: ") {
            lp.io_syscw = strtoull(rest);
        } else if let Some(rest) = line.strip_prefix("cancelled_write_bytes: ") {
            lp.io_cancelled_write_bytes = strtoull(rest);
        }
    }

    lp.io_last_scan_time_ms = realtimeMs;
}

/// Port of `typedef struct LibraryData_ { uint64_t size; bool exec; }` from
/// `LinuxProcessTable.c:722`, the per-inode accumulator stored in the `readMaps`
/// `Hashtable`. `size`/`exec` are [`Cell`]s so `readMaps` can update them in
/// place through the shared `&dyn Object` that [`Hashtable_get`] returns — the
/// faithful analog of the C's `libdata->size += …` through the non-owning
/// `void*` (there is no `Hashtable_getMut` to port).
#[derive(Default)]
struct LibraryData {
    size: std::cell::Cell<u64>,
    exec: std::cell::Cell<bool>,
}

/// Port of `static const ObjectClass` identity for `LibraryData` (the C type
/// has no `ObjectClass`, being a raw `xCalloc`'d struct, but the ported
/// `Hashtable` stores `Object`s; `extends: None` gives it a stable class id).
static LibraryData_class: ObjectClass = ObjectClass { extends: None };

impl Object for LibraryData {
    fn klass(&self) -> &'static ObjectClass {
        &LibraryData_class
    }
}

/// Port of `static void LinuxProcessTable_calcLibSize_helper(ATTR_UNUSED
/// ht_key_t key, void* value, void* data)` from `LinuxProcessTable.c:727`. The
/// `Hashtable_foreach` callback: adds an executable library's mapped `size` to
/// the running `data` total (non-exec entries contribute nothing). The C
/// `void* value`/`void* data` become `&dyn Object` (downcast to `LibraryData`)
/// and `&mut u64`; the `ATTR_UNUSED` key is dropped.
pub fn LinuxProcessTable_calcLibSize_helper(value: &dyn Object, data: &mut u64) {
    // C: if (!data) return; if (!value) return; — both always present here.
    let v = (value as &dyn core::any::Any)
        .downcast_ref::<LibraryData>()
        .expect("calcLibSize_helper: value is not a LibraryData");
    // if (!v->exec) return;
    if !v.exec.get() {
        return;
    }
    // *d += v->size;
    *data += v.size.get();
}

/// Port of `static void LinuxProcessTable_readMaps(LinuxProcess* process,
/// openat_arg_t procFd, const LinuxMachine* host, bool calcSize, bool
/// checkDeletedLib)` from `LinuxProcessTable.c:745`.
///
/// Parses `/proc/<pid>/maps`. When `calcSize`, accumulates each file-backed
/// region's byte span into a per-inode `LibraryData` (in a `Hashtable`), then
/// sets `m_lrs` to the total executable-library size in pages. When
/// `checkDeletedLib`, sets `usesDeletedLib` if an executable mapping's path ends
/// in ` (deleted)` (excluding `/memfd:` and the VirtualBox `/dev/zero` false
/// positive). The C fixed `char buffer[1024]` is a full owned line here (the
/// only deviation — a mapping line longer than 1023 bytes would be split by the
/// C `fgets`); the byte-level parse and `fast_strtoull_*` cursor advance match.
pub fn LinuxProcessTable_readMaps(
    process: &mut LinuxProcess,
    procFd: openat_arg_t,
    host: &LinuxMachine,
    calcSize: bool,
    checkDeletedLib: bool,
) {
    use std::io::BufRead;

    // proc->usesDeletedLib = false;
    process.super_.usesDeletedLib = false;

    // FILE* mapsfile = fopenat(procFd, "maps", "r"); if (!mapsfile) return;
    let file = match fopenat(procFd, c"maps", "r") {
        Some(f) => f,
        None => return,
    };
    let mut reader = std::io::BufReader::new(file);

    // Hashtable* ht = NULL; if (calcSize) ht = Hashtable_new(64, true);
    let mut ht = calcSize.then(|| Hashtable_new(64, true));

    let mut line: Vec<u8> = Vec::new();
    // while (fgets(buffer, sizeof(buffer), mapsfile)) { ... }
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        // Short circuit test: Look for a slash — if (!strchr(buffer, '/')) continue;
        if !line.contains(&b'/') {
            continue;
        }

        // Parse "%Lx-%Lx %4s %x %2x:%2x %Ld" via an advancing byte cursor.
        let mut rp: &[u8] = &line;

        let map_start = fast_strtoull_hex(&mut rp, 16);
        if rp.first() != Some(&b'-') {
            continue;
        }
        rp = &rp[1..];

        let map_end = fast_strtoull_hex(&mut rp, 16);
        if rp.first() != Some(&b' ') {
            continue;
        }
        rp = &rp[1..];

        // if (!readptr[0] || !readptr[1] || !readptr[2] || !readptr[3]) continue;
        if rp.len() < 4 || rp[0] == 0 || rp[1] == 0 || rp[2] == 0 || rp[3] == 0 {
            continue;
        }
        let map_execute = rp[2] == b'x';
        rp = &rp[4..];
        if rp.first() != Some(&b' ') {
            continue;
        }
        rp = &rp[1..];

        // while (*readptr > ' ') readptr++;  (skip the file offset hex)
        while rp.first().is_some_and(|&c| c > b' ') {
            rp = &rp[1..];
        }
        if rp.first() != Some(&b' ') {
            continue;
        }
        rp = &rp[1..];

        let map_devmaj = fast_strtoull_hex(&mut rp, 4);
        if rp.first() != Some(&b':') {
            continue;
        }
        rp = &rp[1..];

        let map_devmin = fast_strtoull_hex(&mut rp, 4);
        if rp.first() != Some(&b' ') {
            continue;
        }
        rp = &rp[1..];

        // Minor shortcut: no file for this region -> skip.
        if map_devmaj == 0 && map_devmin == 0 {
            continue;
        }

        let map_inode = fast_strtoull_dec(&mut rp, 0);
        if map_inode == 0 {
            continue;
        }

        if let Some(ht) = ht.as_mut() {
            // LibraryData* libdata = Hashtable_get(ht, inode);
            // if (!libdata) { libdata = xCalloc(...); Hashtable_put(ht, inode, libdata); }
            let key = map_inode as u32;
            if Hashtable_get(ht, key).is_none() {
                Hashtable_put(ht, key, Box::new(LibraryData::default()));
            }
            // libdata->size += map_end - map_start; libdata->exec |= map_execute;
            let lib = (Hashtable_get(ht, key).unwrap() as &dyn core::any::Any)
                .downcast_ref::<LibraryData>()
                .expect("readMaps: Hashtable holds LibraryData");
            lib.size.set(lib.size.get() + (map_end - map_start));
            lib.exec.set(lib.exec.get() | map_execute);
        }

        if checkDeletedLib && map_execute && !process.super_.usesDeletedLib {
            // while (*readptr == ' ') readptr++;
            while rp.first() == Some(&b' ') {
                rp = &rp[1..];
            }
            // if (*readptr != '/') continue;
            if rp.first() != Some(&b'/') {
                continue;
            }
            // if (String_startsWith(readptr, "/memfd:")) continue;
            if rp.starts_with(b"/memfd:") {
                continue;
            }
            // Virtualbox maps /dev/zero — false positive, ignore.
            if rp == b"/dev/zero (deleted)\n" {
                continue;
            }
            // if (strstr(readptr, " (deleted)\n")) { usesDeletedLib = true; ... }
            let needle = b" (deleted)\n";
            if rp.windows(needle.len()).any(|w| w == needle) {
                process.super_.usesDeletedLib = true;
                if !calcSize {
                    break;
                }
            }
        }
    }

    // fclose(mapsfile) — the BufReader (and its File) drop here.
    drop(reader);

    if let Some(ht) = ht.take() {
        // uint64_t total_size = 0;
        // Hashtable_foreach(ht, LinuxProcessTable_calcLibSize_helper, &total_size);
        let mut total_size: u64 = 0;
        Hashtable_foreach(&ht, &mut |_key, value| {
            LinuxProcessTable_calcLibSize_helper(value, &mut total_size);
        });
        // Hashtable_delete(ht);
        Hashtable_delete(ht);
        // process->m_lrs = total_size / host->pageSize;
        process.m_lrs = (total_size / host.pageSize as u64) as i64;
    }
}

/// Port of `LinuxProcessTable.c:840`. Reads `/proc/<pid>/statm`
/// (process-shared data): total program size and RSS (both scaled to KiB),
/// shared/text/data sizes, and derives private RSS. Thread tasks copy
/// `m_virt`/`m_resident` from the main task. The C `sscanf("%ld %ld ...")`
/// of the seven fields is modeled by whitespace-splitting and decimal
/// parsing, mirroring `sscanf`'s "stop at the first field that fails to
/// convert" semantics (and requiring all seven, `r == 7`, before scaling).
fn LinuxProcessTable_readStatmFile(
    process: &mut LinuxProcess,
    procFd: openat_arg_t,
    host: &LinuxMachine,
    mainTask: Option<&LinuxProcess>,
) -> bool {
    if let Some(mt) = mainTask {
        process.super_.m_virt = mt.super_.m_virt;
        process.super_.m_resident = mt.super_.m_resident;
        return true;
    }

    let mut statmdata = [0u8; 128];
    if Compat_readfileat(procFd, c"statm", &mut statmdata) < 1 {
        return false;
    }

    let nul = statmdata
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(statmdata.len());
    let text = std::str::from_utf8(&statmdata[..nul]).unwrap_or("");

    // sscanf("%ld %ld %ld %ld %ld %ld %ld"): parse up to seven longs, stopping
    // at the first token that fails to convert (matching sscanf's return).
    let mut fields = [0i64; 7];
    let mut r = 0usize;
    for tok in text.split_ascii_whitespace() {
        if r >= 7 {
            break;
        }
        match tok.parse::<i64>() {
            Ok(v) => {
                fields[r] = v;
                r += 1;
            }
            Err(_) => break,
        }
    }

    // Assign the fields sscanf would have written (in order).
    if r >= 1 {
        process.super_.m_virt = fields[0];
    }
    if r >= 2 {
        process.super_.m_resident = fields[1];
    }
    if r >= 3 {
        process.m_share = fields[2];
    }
    if r >= 4 {
        process.m_trs = fields[3];
    }
    // fields[4] is unused since Linux 2.6 (always 0)
    if r >= 6 {
        process.m_drs = fields[5];
    }
    // fields[6] is unused since Linux 2.6 (always 0)

    if r == 7 {
        process.super_.m_virt *= host.pageSizeKB as i64;
        process.super_.m_resident *= host.pageSizeKB as i64;

        process.m_priv = process.super_.m_resident - (process.m_share * host.pageSizeKB as i64);
    }

    r == 7
}

/// Port of `static inline bool skipEndOfLine(FILE* fp)` from `XUtils.h:162`
/// (attributed to `LinuxProcessTable.c` in the C-name snapshot; both consumers
/// — [`LinuxProcessTable_readSmapsFile`] and the still-stubbed
/// `LinuxProcessTable_readOpenVZData` — live here). Consumes bytes until a
/// `\n` is seen, returning `true`; returns `false` at EOF without one. The C
/// `fgets(buffer, 1024, fp)` + `strchr(buffer, '\n')` chunk loop is a plain
/// "read until newline", modeled here as a byte pull on the shared reader so
/// the file position advances exactly as the C `FILE*` would.
fn skipEndOfLine<R: std::io::Read>(fp: &mut R) -> bool {
    let mut b = [0u8; 1];
    loop {
        match fp.read(&mut b) {
            Ok(0) | Err(_) => return false,
            Ok(_) => {
                if b[0] == b'\n' {
                    return true;
                }
            }
        }
    }
}

/// Port of `static bool LinuxProcessTable_readSmapsFile(LinuxProcess*
/// process, openat_arg_t procFd, bool haveSmapsRollup)` from
/// `LinuxProcessTable.c:897`. Opens `smaps_rollup` (or `smaps`), zeroes the
/// three PSS/swap counters, then walks the file `fgets`-chunk by chunk,
/// summing the `Pss:`/`Swap:`/`SwapPss:` values via `strtol`. The kernel
/// returns data in `PAGE_SIZE`-or-less chunks, so a `char buffer[256]` that
/// fills without a `\n` is a partial line whose tail is discarded by
/// `skipEndOfLine`.
///
/// The C `fgets(buffer, sizeof(buffer), fp)` (≤255 chars, stops after `\n`) is
/// reproduced by a manual 255-byte window over a shared `BufReader` — not
/// `BufRead::read_line`, which would swallow the partial-line case — so the
/// `skipEndOfLine` branch fires exactly as in the C. `strtol(buffer + N, NULL,
/// 10)` becomes `strtol10` on the byte slice after the key prefix.
pub fn LinuxProcessTable_readSmapsFile(
    process: &mut LinuxProcess,
    procFd: openat_arg_t,
    haveSmapsRollup: bool,
) -> bool {
    use std::io::Read;

    // strtol(s, NULL, 10) on a byte slice: skip leading whitespace, take an
    // optional sign and the leading decimal run.
    fn strtol10(s: &[u8]) -> i64 {
        let mut i = 0;
        while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
            i += 1;
        }
        let mut neg = false;
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            neg = s[i] == b'-';
            i += 1;
        }
        let mut val: i64 = 0;
        while i < s.len() && s[i].is_ascii_digit() {
            val = val * 10 + (s[i] - b'0') as i64;
            i += 1;
        }
        if neg {
            -val
        } else {
            val
        }
    }

    //http://elixir.free-electrons.com/linux/v4.10/source/fs/proc/task_mmu.c#L719
    //kernel will return data in chunks of size PAGE_SIZE or less.
    let fp = match fopenat(
        procFd,
        if haveSmapsRollup {
            c"smaps_rollup"
        } else {
            c"smaps"
        },
        "r",
    ) {
        Some(f) => f,
        None => return false,
    };
    let mut fp = std::io::BufReader::new(fp);

    process.m_pss = 0;
    process.m_swap = 0;
    process.m_psswp = 0;

    // char buffer[256]; while (fgets(buffer, sizeof(buffer), fp)) { ... }
    // fgets reads at most 255 bytes and stops after a '\n'.
    let mut byte = [0u8; 1];
    loop {
        let mut buffer: Vec<u8> = Vec::with_capacity(256);
        let mut sawNewline = false;
        while buffer.len() < 255 {
            match fp.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    buffer.push(byte[0]);
                    if byte[0] == b'\n' {
                        sawNewline = true;
                        break;
                    }
                }
            }
        }
        // fgets returns NULL only when nothing was read (EOF).
        if buffer.is_empty() {
            break;
        }

        if !sawNewline {
            // Partial line, skip to end of this line
            if !skipEndOfLine(&mut fp) {
                return false;
            }
        }

        let line = String::from_utf8_lossy(&buffer);
        if String_startsWith(&line, "Pss:") {
            process.m_pss += strtol10(&buffer[4..]);
        } else if String_startsWith(&line, "Swap:") {
            process.m_swap += strtol10(&buffer[5..]);
        } else if String_startsWith(&line, "SwapPss:") {
            process.m_psswp += strtol10(&buffer[8..]);
        }
    }

    // fclose(fp) — fp dropped here.
    true
}

/// TODO: port of `static void LinuxProcessTable_readOpenVZData(LinuxProcess*
/// process, openat_arg_t procFd)` from `LinuxProcessTable.c:934` (guarded by
/// `#ifdef HAVE_OPENVZ`; only reached from the equally-guarded call site in
/// `recurseProcTree`). Blocked on two counts: (1) it reads/writes
/// `process->ctid` (a `char*`) and `process->vpid`, but neither field is
/// modeled on [`LinuxProcess`] (only `m_lrs` et al. exist); and (2) this build
/// has not committed to the `HAVE_OPENVZ` variant. (The partial-line
/// `skipEndOfLine` dependency is now ported.) Stays stubbed until those land.
pub fn LinuxProcessTable_readOpenVZData() {
    todo!("port of LinuxProcessTable.c:934 — needs ctid/vpid fields + HAVE_OPENVZ")
}

/// Port of `static void LinuxProcessTable_readCGroupFile(LinuxProcess*
/// process, openat_arg_t procFd)` from `LinuxProcessTable.c:1024`. Reads
/// `/proc/<pid>/cgroup`, keeping the third `:`-delimited field of each line
/// (the cgroup path), joining them with `;` into a `PROC_LINE_LENGTH`-capped
/// `output` string, then updates the raw [`ProcessField::CGROUP`] width and
/// stores it. When the path changed it recomputes the shortened
/// [`CGroup_filterName`] ("CCGROUP") and [`CGroup_filterContainer`]
/// ("CONTAINER") forms (falling back to the raw cgroup / `"N/A"` widths); an
/// unchanged path only refreshes the widths from the cached short forms. A
/// missing file clears all three cached strings.
///
/// The C `output[PROC_LINE_LENGTH + 1]` buffer with the `at`/`left` cursor and
/// `snprintf` truncation is reproduced on a byte `Vec` with a `left` budget:
/// each group segment is truncated at `\n` (the C `*eol_w = '\0'`), a `;`
/// separator is charged one byte, and a segment that would overflow `left`
/// copies `left - 1` bytes and stops (the C truncation `break`). `fopenat` +
/// `fgets` become the ported [`fopenat`] + a [`BufRead::read_line`] loop.
/// `String_strchrnul`/`String_eq`/`free_and_xStrdup` map to the ported
/// helpers / `Option<String>` assignment.
fn LinuxProcessTable_readCGroupFile(process: &mut LinuxProcess, procFd: openat_arg_t) {
    use std::io::BufRead;

    let file = match fopenat(procFd, c"cgroup", "r") {
        Some(f) => f,
        None => {
            // free() + NULL all three cached strings.
            process.cgroup = None;
            process.cgroup_short = None;
            process.container_short = None;
            return;
        }
    };

    let mut output: Vec<u8> = Vec::new();
    let mut left = PROC_LINE_LENGTH;
    let mut reader = std::io::BufReader::new(file);
    let mut line = String::new();
    // while (!feof(file) && left > 0)
    while left > 0 {
        line.clear();
        // const char* ok = fgets(...); if (!ok) break;
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        let bytes = line.as_bytes();
        // Skip the first two ':'-delimited fields.
        let mut group = 0usize;
        for _ in 0..2 {
            group += String_strchrnul(&line[group..], b':');
            if group >= bytes.len() {
                // !*group — no further ':'
                break;
            }
            group += 1; // group++ past the ':'
        }

        // eol = strchrnul(group, '\n'); *eol_w = '\0';
        let group_end = group + String_strchrnul(&line[group..], b'\n');
        let group_bytes = &bytes[group..group_end];

        // if (at != output) { *at = ';'; at++; left--; }
        if !output.is_empty() {
            if left == 0 {
                break;
            }
            output.push(b';');
            left -= 1;
        }

        // int wrote = snprintf(at, left, "%s", group);
        let wrote = group_bytes.len();
        if wrote >= left {
            // Truncated: snprintf copies left - 1 bytes then we are done.
            let n = left.saturating_sub(1);
            output.extend_from_slice(&group_bytes[..n]);
            break;
        }
        output.extend_from_slice(group_bytes);
        left -= wrote;
    }
    // fclose(file) — reader dropped here.
    drop(reader);

    let output = String::from_utf8_lossy(&output).into_owned();

    // bool changed = !process->cgroup || !String_eq(process->cgroup, output);
    let changed = match &process.cgroup {
        Some(c) => !String_eq(c, &output),
        None => true,
    };

    Row_updateFieldWidth(ProcessField::CGROUP as RowField, output.len());
    // free_and_xStrdup(&process->cgroup, output);
    process.cgroup = Some(output);

    if !changed {
        // CCGROUP: from cached short form, else the raw cgroup width.
        match &process.cgroup_short {
            Some(cs) => Row_updateFieldWidth(ProcessField::CCGROUP as RowField, cs.len()),
            None => Row_updateFieldWidth(
                ProcessField::CCGROUP as RowField,
                process.cgroup.as_deref().unwrap().len(),
            ),
        }
        match &process.container_short {
            Some(cs) => Row_updateFieldWidth(ProcessField::CONTAINER as RowField, cs.len()),
            None => Row_updateFieldWidth(ProcessField::CONTAINER as RowField, "N/A".len()),
        }
        return;
    }

    // char* cgroup_short = CGroup_filterName(process->cgroup);
    let cgroup_short = CGroup_filterName(process.cgroup.as_deref().unwrap());
    match cgroup_short {
        Some(cs) => {
            Row_updateFieldWidth(ProcessField::CCGROUP as RowField, cs.len());
            process.cgroup_short = Some(cs);
        }
        None => {
            // CCGROUP aliases the normal CGROUP if shortening fails.
            Row_updateFieldWidth(
                ProcessField::CCGROUP as RowField,
                process.cgroup.as_deref().unwrap().len(),
            );
            process.cgroup_short = None;
        }
    }

    // char* container_short = CGroup_filterContainer(process->cgroup);
    let container_short = CGroup_filterContainer(process.cgroup.as_deref().unwrap());
    match container_short {
        Some(cs) => {
            Row_updateFieldWidth(ProcessField::CONTAINER as RowField, cs.len());
            process.container_short = Some(cs);
        }
        None => {
            // CONTAINER is just "N/A" if shortening fails.
            Row_updateFieldWidth(ProcessField::CONTAINER as RowField, "N/A".len());
            process.container_short = None;
        }
    }
}

/// Port of `LinuxProcessTable.c:1022`. Reads `/proc/<pid>/oom_score` into the
/// [`LinuxProcess`] `oom` field (thread tasks copy from the main task).
/// Defaults to `UINT_MAX` and only accepts a value terminated by NUL, `\n`,
/// or space, exactly as the C guards. `fast_strtoull_dec` is capped at the
/// number of bytes read.
fn LinuxProcessTable_readOomData(
    process: &mut LinuxProcess,
    procFd: openat_arg_t,
    mainTask: Option<&LinuxProcess>,
) {
    if let Some(mt) = mainTask {
        process.oom = mt.oom;
        return;
    }

    let mut buffer = [0u8; PROC_LINE_LENGTH + 1];

    process.oom = u32::MAX; // UINT_MAX
    let oomRead = Compat_readfileat(procFd, c"oom_score", &mut buffer);
    if oomRead < 1 {
        return;
    }

    let mut cur: &[u8] = &buffer[..];
    let oom = fast_strtoull_dec(&mut cur, oomRead as usize);
    let next = buffer.len() - cur.len();
    let c = buffer.get(next).copied().unwrap_or(0);
    if c != 0 && c != b'\n' && c != b' ' {
        return;
    }

    if oom > u32::MAX as u64 {
        return;
    }

    process.oom = oom as u32;
}

/// Port of `LinuxProcessTable.c:1052`. Reads `/proc/<pid>/autogroup` (CFS
/// autogroup id + nice), copying from the main task for threads. The C
/// `sscanf("/autogroup-%ld nice %d", ...)` (requiring both fields, `ok == 2`)
/// is modeled by a prefix strip + whitespace split. `autogroup_id` stays `-1`
/// on any parse failure.
fn LinuxProcessTable_readAutogroup(
    process: &mut LinuxProcess,
    procFd: openat_arg_t,
    mainTask: Option<&LinuxProcess>,
) {
    if let Some(mt) = mainTask {
        process.autogroup_id = mt.autogroup_id;
        return;
    }

    process.autogroup_id = -1;

    let mut autogroup = [0u8; 64];
    let amtRead = Compat_readfileat(procFd, c"autogroup", &mut autogroup);
    if amtRead < 0 {
        return;
    }

    let nul = autogroup
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(autogroup.len());
    let content = std::str::from_utf8(&autogroup[..nul]).unwrap_or("");

    // "/autogroup-%ld nice %d"
    let parsed = (|| -> Option<(i64, i32)> {
        let rest = content.strip_prefix("/autogroup-")?;
        let mut it = rest.split_whitespace();
        let identity: i64 = it.next()?.parse().ok()?;
        if it.next()? != "nice" {
            return None;
        }
        let nice: i32 = it.next()?.parse().ok()?;
        Some((identity, nice))
    })();

    if let Some((identity, nice)) = parsed {
        process.autogroup_id = identity;
        process.autogroup_nice = nice;
    }
}

/// Port of `static void LinuxProcessTable_readSecattrData(LinuxProcess*
/// process, openat_arg_t procFd, const LinuxProcess* mainTask)` from
/// `LinuxProcessTable.c:1182`. Thread tasks copy the main task's `secattr`
/// (or clear it when absent). For a main task it reads `/proc/<pid>/attr/current`
/// (the SELinux/AppArmor security context), clears `secattr` on a short read
/// (`< 1`), otherwise trims at the first `\n`, updates the
/// [`ProcessField::SECATTR`] column width, and stores the value.
fn LinuxProcessTable_readSecattrData(
    process: &mut LinuxProcess,
    procFd: openat_arg_t,
    mainTask: Option<&LinuxProcess>,
) {
    if let Some(mt) = mainTask {
        // free_and_xStrdup(&process->secattr, mainSecAttr) or free + NULL.
        process.secattr = mt.secattr.clone();
        return;
    }

    let mut buffer = [0u8; PROC_LINE_LENGTH + 1];
    let attrdata = Compat_readfileat(procFd, c"attr/current", &mut buffer);
    if attrdata < 1 {
        process.secattr = None;
        return;
    }

    // char* newline = strchr(buffer, '\n'); if (newline) *newline = '\0';
    // strlen stops at the newline (or the NUL terminator otherwise).
    let end = buffer
        .iter()
        .position(|&b| b == b'\n' || b == 0)
        .unwrap_or(buffer.len());
    let text = String::from_utf8_lossy(&buffer[..end]).into_owned();

    Row_updateFieldWidth(ProcessField::SECATTR as RowField, text.len());
    process.secattr = Some(text);
}

/// Port of `LinuxProcessTable.c:1111`. Resolves `/proc/<pid>/cwd` (the
/// process working directory) via `readlinkat`, storing it in
/// [`Process::procCwd`]; threads copy from the main task. The C
/// `#if HAVE_READLINKAT && HAVE_OPENAT` branch (the one this build commits
/// to) is ported; the `Compat_readlink` fallback is the other build variant.
fn LinuxProcessTable_readCwd(
    process: &mut LinuxProcess,
    procFd: openat_arg_t,
    mainTask: Option<&LinuxProcess>,
) {
    if let Some(mt) = mainTask {
        // free_and_xStrdup(mainCwd) when set, else free + NULL.
        process.super_.procCwd = mt.super_.procCwd.clone();
        return;
    }

    let mut pathBuffer = [0u8; PATH_MAX + 1];
    let r = unsafe {
        libc::readlinkat(
            procFd,
            c"cwd".as_ptr(),
            pathBuffer.as_mut_ptr() as *mut libc::c_char,
            pathBuffer.len() - 1,
        )
    };

    if r < 0 {
        process.super_.procCwd = None;
        return;
    }

    process.super_.procCwd = Some(String::from_utf8_lossy(&pathBuffer[..r as usize]).into_owned());
}

/// Port of `LinuxProcessTable.c:1145`. Resolves `/proc/<pid>/exe` via
/// `readlinkat`, handling the kernel `" (deleted)"` suffix (stripping it and
/// flagging `procExeDeleted`), and updates the executable path through
/// [`Process_updateExe`]. Threads copy from the main task. The
/// suffix/comparison logic runs on raw bytes (paths may not be UTF-8);
/// the final value is handed to `Process_updateExe` as a (lossily-decoded)
/// string, matching the `Option<String> procExe` model. The
/// `HAVE_READLINKAT && HAVE_OPENAT` branch is ported (committed build
/// variant).
fn LinuxProcessList_readExe(
    process: &mut Process,
    procFd: openat_arg_t,
    mainTask: Option<&LinuxProcess>,
) {
    if let Some(mt) = mainTask {
        Process_updateExe(process, mt.super_.procExe.as_deref());
        process.procExeDeleted = mt.super_.procExeDeleted;
        return;
    }

    let mut filename = [0u8; PATH_MAX + 1];
    let amtRead = unsafe {
        libc::readlinkat(
            procFd,
            c"exe".as_ptr(),
            filename.as_mut_ptr() as *mut libc::c_char,
            filename.len() - 1,
        )
    };

    if amtRead > 0 {
        let mut fbytes = filename[..amtRead as usize].to_vec();

        // if (!procExe || (!procExeDeleted && !String_eq(filename, procExe)) || procExeDeleted)
        let differs = process
            .procExe
            .as_deref()
            .map(|e| e.as_bytes() != fbytes.as_slice())
            .unwrap_or(true);
        // Faithful mirror of the C boolean; kept verbatim to match the SPEC.
        #[allow(clippy::nonminimal_bool)]
        let cond = process.procExe.is_none()
            || (!process.procExeDeleted && differs)
            || process.procExeDeleted;

        if cond {
            const DELETED_MARKER: &[u8] = b" (deleted)";
            let markerLen = DELETED_MARKER.len();
            let filenameLen = fbytes.len();

            if filenameLen > markerLen {
                let oldExeDeleted = process.procExeDeleted;

                process.procExeDeleted = &fbytes[filenameLen - markerLen..] == DELETED_MARKER;

                if process.procExeDeleted {
                    fbytes.truncate(filenameLen - markerLen);
                }

                if oldExeDeleted != process.procExeDeleted {
                    process.mergedCommand.lastUpdate = 0;
                }
            }

            let s = String::from_utf8_lossy(&fbytes).into_owned();
            Process_updateExe(process, Some(&s));
        }
    } else if process.procExe.is_some() {
        Process_updateExe(process, None);
        process.procExeDeleted = false;
    }
}

/// Port of `LinuxProcessTable.c:1194`. Reads a whole `/proc` file whose size
/// is not known in advance, growing the buffer (starting at 512 bytes,
/// doubling) up to `MAX_CMDLINE_BUFFER_SIZE` while each read fills the buffer.
/// The C `char*` result + `ssize_t* amtRead` out-param become a returned
/// `Some((buffer, amtRead))`; `None` mirrors the C `NULL` (nothing read /
/// error). The buffer keeps its full allocated length (data is NUL-terminated
/// at `amtRead` by [`Compat_readfileat`]), matching the C caller expectations.
fn readFileDynamic(procFd: openat_arg_t, filename: &CStr) -> Option<(Vec<u8>, ssize_t)> {
    let mut bufferSize: usize = 512;
    let mut buffer = vec![0u8; bufferSize];

    let mut amtRead = Compat_readfileat(procFd, filename, &mut buffer);

    // If the buffer was full, the file might be larger; retry with more space.
    while amtRead > 0 && amtRead as usize == bufferSize - 1 && bufferSize < MAX_CMDLINE_BUFFER_SIZE
    {
        bufferSize *= 2;
        buffer.resize(bufferSize, 0);
        amtRead = Compat_readfileat(procFd, filename, &mut buffer);
    }

    if amtRead <= 0 {
        return None;
    }

    Some((buffer, amtRead))
}

/// Port of `LinuxProcessTable.c:1219`. Reads `/proc/<pid>/cmdline`, first
/// refreshing the exe link ([`LinuxProcessList_readExe`]), then splitting the
/// NUL-delimited argument vector and computing the basename token
/// `[tokenStart, tokenEnd)` for display. Ports the full argument-parsing
/// heuristic for processes that flatten their cmdline with spaces (e.g.
/// chrome), including the `faccessat` path-existence cross-validation. The C
/// `char*` cursor arithmetic on the mutable buffer is modeled with byte
/// indices into a `Vec<u8>`; `(size_t)-1` sentinels map to [`usize::MAX`]
/// (`NPOS`), whose unsigned comparisons match C `size_t` exactly.
///
/// Signature mapping: takes the concrete `&mut Process` (the C `Process*`);
/// `mainTask` is the [`Option`] of a borrowed [`LinuxProcess`].
fn LinuxProcessTable_readCmdlineFile(
    process: &mut Process,
    procFd: openat_arg_t,
    mainTask: Option<&LinuxProcess>,
) -> bool {
    LinuxProcessList_readExe(process, procFd, mainTask);

    let (mut command, amtRead) = match readFileDynamic(procFd, c"cmdline") {
        Some(v) => v,
        None => return false,
    };
    let amtRead = amtRead as usize;

    const NPOS: usize = usize::MAX; // (size_t)-1

    let mut tokenEnd = NPOS;
    let mut tokenStart = NPOS;
    let mut lastChar = 0usize;
    let mut argSepNUL = false;
    let mut argSepSpace = false;

    for i in 0..amtRead {
        let argChar = command[i];

        // newline used as delimiter -> non-printable placeholder
        if argChar == b'\n' {
            command[i] = b'\r';
            continue;
        }

        if argChar == b'\0' {
            command[i] = b'\n';

            if tokenEnd == NPOS {
                tokenEnd = i;
            }

            continue;
        }

        // NUL byte in the middle of command
        if tokenEnd != NPOS {
            argSepNUL = true;
        }

        if argChar <= b' ' {
            argSepSpace = true;
        }

        // last '/' before end of token = start of basename
        if argChar == b'/' && tokenEnd == NPOS {
            tokenStart = i + 1;
        }

        lastChar = i;
    }

    command[lastChar + 1] = b'\0';

    // faccessat(AT_FDCWD, bytes, F_OK, AT_SYMLINK_NOFOLLOW); interior NUL -> -1.
    let faccess = |bytes: &[u8]| -> i32 {
        match std::ffi::CString::new(bytes) {
            Ok(cs) => Compat_faccessat(libc::AT_FDCWD, &cs, libc::F_OK, libc::AT_SYMLINK_NOFOLLOW),
            Err(_) => -1,
        }
    };

    if !argSepNUL && argSepSpace {
        /* Argument parsing heuristic for processes that flatten their
         * command line with spaces instead of NUL bytes. */
        tokenStart = NPOS;
        tokenEnd = NPOS;

        let exeLen = process.procExe.as_ref().map(|s| s.len()).unwrap_or(0);

        let starts_with_exe = process
            .procExe
            .as_deref()
            .map(|e| command[..=lastChar].starts_with(e.as_bytes()))
            .unwrap_or(false);

        if process.procExe.is_some()
            && starts_with_exe
            && exeLen < lastChar
            && command[exeLen] <= b' '
        {
            tokenStart = process.procExeBasenameOffset;
            tokenEnd = exeLen;
        }
        // Check if the space is part of a filename for an existing file.
        else if faccess(&command[..=lastChar]) != 0 {
            // Path does not exist; search for the part that does.
            let mut tokenArg0Start = NPOS;

            for i in 0..=lastChar {
                let cmdChar = command[i];

                if cmdChar <= b' ' {
                    if tokenEnd != NPOS {
                        // Split on every further separator
                        command[i] = b'\n';
                        continue;
                    }

                    // Found our first argument
                    command[i] = b'\0';

                    let found = faccess(&command[..i]) == 0;

                    // Restore if this wasn't it
                    command[i] = if found { b'\n' } else { cmdChar };

                    if found {
                        tokenEnd = i;
                    }
                    if tokenArg0Start == NPOS {
                        tokenArg0Start = if tokenStart == NPOS { 0 } else { tokenStart };
                    }

                    continue;
                }

                if tokenEnd != NPOS {
                    continue;
                }

                if cmdChar == b'/' {
                    // Normal path separator
                    tokenStart = i + 1;
                } else if cmdChar == b'\\'
                    && (tokenStart == NPOS || tokenStart == 0 || command[tokenStart - 1] == b'\\')
                {
                    // Windows Path separator (WINE)
                    tokenStart = i + 1;
                } else if cmdChar == b':' && (command[i + 1] != b'/' && command[i + 1] != b'\\') {
                    // Colon not part of a Windows Path
                    tokenEnd = i;
                } else if tokenStart == NPOS {
                    // Relative path
                    tokenStart = i;
                }
            }

            if tokenEnd == NPOS {
                tokenStart = tokenArg0Start;

                // No token delimiter found, forcibly split
                for i in 0..=lastChar {
                    if command[i] <= b' ' {
                        command[i] = b'\n';
                        if tokenEnd == NPOS {
                            tokenEnd = i;
                        }
                    }
                }
            }
        }

        // Reset if start is behind end.
        if tokenStart >= tokenEnd {
            tokenStart = NPOS;
            tokenEnd = NPOS;
        }
    }

    if tokenStart == NPOS {
        tokenStart = 0;
    }

    if tokenEnd == NPOS {
        tokenEnd = lastChar + 1;
    }

    let s = String::from_utf8_lossy(&command[..=lastChar]).into_owned();
    Process_updateCmdline(process, Some(&s), tokenStart, tokenEnd);

    true
}

/// Port of `LinuxProcessTable.c:1396`. Reads `/proc/<pid>/comm` (the process
/// "command" name) and updates it via [`Process_updateComm`]; a failed read
/// clears it (`None`). The C `command[amtRead - 1] = '\0'` drops the trailing
/// newline, modeled by slicing off the last byte.
fn LinuxProcessList_readComm(process: &mut Process, procFd: openat_arg_t) {
    match readFileDynamic(procFd, c"comm") {
        Some((command, amtRead)) => {
            let end = (amtRead as usize).saturating_sub(1);
            let s = String::from_utf8_lossy(&command[..end]).into_owned();
            Process_updateComm(process, Some(&s));
        }
        None => Process_updateComm(process, None),
    }
}

/// Port of `static char* LinuxProcessTable_updateTtyDevice(TtyDriver*
/// ttyDrivers, unsigned long int tty_nr)` from `LinuxProcessTable.c:1514`.
/// Splits `tty_nr` into a `major`/`minor` pair, then walks the
/// major/minor-sorted [`TtyDriver`] table looking for the range that owns the
/// device. Within the matching driver it probes candidate device nodes
/// (`<path>/<idx>` then `<path><idx>`, for `idx = min - minorFrom` and then
/// `idx = min`), returning the first path whose `stat().st_rdev` decodes back
/// to the same `major`/`minor`. Failing that it stats the bare driver `path`
/// (matching the whole `tty_nr`), and finally falls back to a synthetic
/// `"/dev/<maj>:<min>"` string. Always returns an owned string (the C
/// `xStrdup`/`xAsprintf` never yield `NULL`).
///
/// The `major()`/`minor()` device-number macros (`sys/sysmacros.h`) are
/// replicated as nested fns using glibc's `gnu_dev_major`/`gnu_dev_minor` bit
/// layout — the same layout `libc::major`/`libc::minor` expose on the Linux
/// arm (verified in `libc-0.2.176` `.../linux/mod.rs:5959`). Because `tty_nr`
/// and `st_rdev` are always Linux-kernel-encoded `dev_t`s at runtime, the
/// glibc layout is used unconditionally so the darwin build (whose
/// `libc::major` uses a different Darwin `dev_t` layout and returns `i32`)
/// decodes them identically. `stat(path, &sb)` becomes `fs::metadata`
/// (follows symlinks, like `stat`) + `MetadataExt::rdev`; `xAsprintf` /
/// `xSnprintf` become `format!`.
pub fn LinuxProcessTable_updateTtyDevice(ttyDrivers: &[TtyDriver], tty_nr: u64) -> String {
    use std::os::unix::fs::MetadataExt;

    // glibc gnu_dev_major (sys/sysmacros.h).
    fn major(dev: u64) -> u32 {
        (((dev & 0x0000_0000_000f_ff00) >> 8) | ((dev & 0xffff_f000_0000_0000) >> 32)) as u32
    }
    // glibc gnu_dev_minor (sys/sysmacros.h).
    fn minor(dev: u64) -> u32 {
        (((dev & 0x0000_0000_0000_00ff) >> 0) | ((dev & 0x0000_0fff_fff0_0000) >> 12)) as u32
    }

    let maj = major(tty_nr);
    let min = minor(tty_nr);

    let mut i: isize = -1;
    loop {
        i += 1;
        let idx_i = i as usize;
        // if ((!ttyDrivers[i].path) || maj < ttyDrivers[i].major) break;
        let driver = match ttyDrivers.get(idx_i) {
            Some(d) if d.path.is_some() => d,
            // Sentinel terminator entry (NULL path) or out of bounds.
            _ => break,
        };
        if maj < driver.major {
            break;
        }
        if maj > driver.major {
            continue;
        }
        if min < driver.minorFrom {
            break;
        }
        if min > driver.minorTo {
            continue;
        }

        let path = driver.path.as_deref().unwrap();

        let mut idx = min - driver.minorFrom;

        loop {
            // "%s/%d"
            let mut fullPath = format!("{path}/{idx}");
            if let Ok(sb) = std::fs::metadata(&fullPath) {
                let rdev = sb.rdev();
                if major(rdev) == maj && minor(rdev) == min {
                    return fullPath;
                }
            }

            // "%s%d"
            fullPath = format!("{path}{idx}");
            if let Ok(sb) = std::fs::metadata(&fullPath) {
                let rdev = sb.rdev();
                if major(rdev) == maj && minor(rdev) == min {
                    return fullPath;
                }
            }

            if idx == min {
                break;
            }

            idx = min;
        }

        // int err = stat(ttyDrivers[i].path, &sb);
        if let Ok(sb) = std::fs::metadata(path) {
            if tty_nr == sb.rdev() {
                return path.to_string();
            }
        }
    }

    format!("/dev/{maj}:{min}")
}

/// Port of `LinuxProcessTable.c:1466`. True iff `proc` has been alive for
/// more than `seconds`, using the host's current realtime clock and the
/// process's parsed start time. Reads `proc->super.host->realtimeMs` through
/// the opaque `*const Machine` handle (the `GPU_readProcessData` idiom).
/// Returns `false` while the start time is not yet parsed.
fn isOlderThan(proc: &Process, seconds: u32) -> bool {
    let host = proc.super_.host as *const Machine;
    let realtimeMs = unsafe { (*host).realtimeMs };

    debug_assert!(realtimeMs > 0);

    /* Starttime might not yet be parsed */
    if proc.starttime_ctime <= 0 {
        return false;
    }

    let realtime = realtimeMs / 1000;

    if realtime < proc.starttime_ctime as u64 {
        return false;
    }

    realtime - proc.starttime_ctime as u64 > seconds as u64
}

/// C `static ino_t rootPidNs` (`LinuxProcessTable.c:68`): the PID-namespace
/// inode of the root, set at table init from `/proc/self/ns/pid`. That init is
/// not ported, so this stays `-1` (unset) and the container-namespace check in
/// [`LinuxProcessTable_recurseProcTree`] is inert — faithful where `/proc` is
/// absent (macOS) and a no-op until the init lands.
static ROOT_PID_NS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);

/// Port of `static bool LinuxProcessTable_recurseProcTree(LinuxProcessTable*
/// this, openat_arg_t parentFd, const LinuxMachine* lhost, const char* dirname,
/// const LinuxProcess* mainTask)` from `LinuxProcessTable.c:1588`.
///
/// The recursive `/proc` (and per-process `task/`) directory walk: for each PID
/// directory it fetches/constructs the process (`ProcessTable_getProcess`),
/// runs the always-on readers (`readStatmFile`/`readStatFile`/`updateUser`/
/// `readCmdlineFile`/`readComm`) plus each `ss.flags`-gated reader, recurses
/// into `task/` for the thread list, and marks the row updated/shown.
///
/// `mainTask` is a raw `*const LinuxProcess` into the owning table's `rows` (the
/// C's `const LinuxProcess*`); the current process is likewise reached as a
/// `*mut LinuxProcess` via `rows[idx]` (the `DarwinProcessTable::goThroughEntries`
/// precedent). The pointee is the `Box`'s heap allocation, so it stays valid
/// across the thread recursion's `rows` growth. `this` (the table) and `lhost`
/// (the machine) are disjoint objects, so the `&mut this` / `&lhost` borrows do
/// not conflict; the only aliasing is `lp_ptr`/`mainTask` into `rows`, handled
/// with raw pointers exactly as the C's `Process*` graph does.
///
/// Deviations: `ProcessTable_getProcess` already adds a new process, so the C's
/// trailing `ProcessTable_add` is not repeated and the error path removes the
/// just-added row; the Linux-only `SYS_capget` privilege probe is
/// `#[cfg(target_os = "linux")]`; the `#ifdef HAVE_OPENVZ`/`HAVE_DELAYACCT`
/// branches are compile-time-off in this build and omitted.
pub fn LinuxProcessTable_recurseProcTree(
    this: &mut LinuxProcessTable,
    parentFd: openat_arg_t,
    lhost: &LinuxMachine,
    dirname: &std::ffi::CStr,
    mainTask: Option<*const LinuxProcess>,
) -> bool {
    use std::sync::atomic::Ordering;

    let host = &lhost.super_;
    let settings = host
        .settings
        .as_ref()
        .expect("recurseProcTree: host->settings is NULL");
    let ss_flags = settings.screens[settings.ssIndex as usize].flags;
    let hide_kernel = settings.hideKernelThreads;
    let hide_userland = settings.hideUserlandThreads;
    let hide_container = settings.hideRunningInContainer;
    let highlight_deleted_exe = settings.highlightDeletedExe;
    let update_process_names = settings.updateProcessNames;
    let show_thread_names = settings.showThreadNames;

    // pt->runningTasks = lhost->runningTasks;
    this.super_.runningTasks = lhost.runningTasks;

    // int dirFd = openat(parentFd, dirname, O_RDONLY|O_DIRECTORY|O_NOFOLLOW);
    let dir_fd = Compat_openat(
        parentFd,
        dirname,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
    );
    if dir_fd < 0 {
        return false;
    }
    // DIR* dir = fdopendir(dirFd);
    let dir = unsafe { libc::fdopendir(dir_fd) };
    if dir.is_null() {
        Compat_openatArgClose(dir_fd);
        return false;
    }

    // Reach `mainTask` as an `Option<&LinuxProcess>` for the leaf readers.
    let main_task_ref = || mainTask.map(|p| unsafe { &*p });

    // while ((entry = readdir(dir)) != NULL)
    loop {
        let entry = unsafe { libc::readdir(dir) };
        if entry.is_null() {
            break;
        }

        // Ignore all non-directories.
        let d_type = unsafe { (*entry).d_type };
        if d_type != libc::DT_DIR && d_type != libc::DT_UNKNOWN {
            continue;
        }

        let name_cstr = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        let name_bytes = name_cstr.to_bytes();
        // The RedHat kernel hides threads with a dot.
        let name = if name_bytes.first() == Some(&b'.') {
            &name_bytes[1..]
        } else {
            name_bytes
        };
        // Just skip all non-number directories.
        if name.first().is_none_or(|&c| !c.is_ascii_digit()) {
            continue;
        }
        let name_str = match std::str::from_utf8(name) {
            Ok(s) => s,
            Err(_) => continue,
        };
        // pid_t pid = strtopid(name);
        let pid = strtopid(name_str);
        if pid == 0 {
            continue;
        }
        // Skip task directory of main thread.
        if let Some(mt) = mainTask {
            if pid == Process_getPid(unsafe { &(*mt).super_ }) {
                continue;
            }
        }

        // int procFd = openat(dirFd, entry->d_name, O_RDONLY|O_DIRECTORY|O_NOFOLLOW);
        let procFd = Compat_openat(
            dir_fd,
            name_cstr,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        );
        if procFd < 0 {
            continue;
        }

        // Process* proc = ProcessTable_getProcess(pt, pid, &preExisting, LinuxProcess_new);
        let (pre_existing, idx) =
            crate::ported::processtable::ProcessTable_getProcess(&mut this.super_, pid, |h| {
                Box::new(LinuxProcess_new(h)) as Box<dyn Object>
            });
        // Recover a `*mut LinuxProcess` for this row via a checked borrow that
        // ends immediately. The row was built as a `LinuxProcess`.
        let lp_ptr: *mut LinuxProcess = {
            let obj: &mut dyn Object = this.super_.super_.rows[idx].as_mut().unwrap().as_mut();
            (obj as &mut dyn core::any::Any)
                .downcast_mut::<LinuxProcess>()
                .unwrap()
        };
        let this_ptr: *mut LinuxProcessTable = this;

        // Process_setThreadGroup(proc, mainTask ? getPid(mainTask) : pid);
        let thread_group = match mainTask {
            Some(mt) => Process_getPid(unsafe { &(*mt).super_ }),
            None => pid,
        };
        unsafe {
            Process_setThreadGroup(&mut (*lp_ptr).super_, thread_group);
            (*lp_ptr).super_.isUserlandThread = Process_getPid(&(*lp_ptr).super_) != thread_group;
        }

        // As the tasks/threads are a flat view under the main entry, recurse
        // only when this is the main thread.
        if mainTask.is_none() {
            LinuxProcessTable_recurseProcTree(
                unsafe { &mut *this_ptr },
                procFd,
                lhost,
                c"task",
                Some(lp_ptr as *const LinuxProcess),
            );
        }

        // Short-circuit subsequent scans of hidden threads/kthreads/containers.
        if pre_existing && hide_kernel && Process_isKernelThread(unsafe { &(*lp_ptr).super_ }) {
            unsafe {
                (*lp_ptr).super_.super_.updated = true;
                (*lp_ptr).super_.super_.show = false;
                (*this_ptr).super_.kernelThreads += 1;
                (*this_ptr).super_.totalTasks += 1;
            }
            Compat_openatArgClose(procFd);
            continue;
        }
        if pre_existing && hide_userland && Process_isUserlandThread(unsafe { &(*lp_ptr).super_ }) {
            unsafe {
                (*lp_ptr).super_.super_.updated = true;
                (*lp_ptr).super_.super_.show = false;
                (*this_ptr).super_.userlandThreads += 1;
                (*this_ptr).super_.totalTasks += 1;
            }
            Compat_openatArgClose(procFd);
            continue;
        }
        if pre_existing
            && hide_container
            && unsafe { (*lp_ptr).super_.isRunningInContainer } == Tristate::TRI_ON
        {
            unsafe {
                (*lp_ptr).super_.super_.updated = true;
                (*lp_ptr).super_.super_.show = false;
            }
            Compat_openatArgClose(procFd);
            continue;
        }

        let scan_main_thread = !hide_userland
            && !Process_isKernelThread(unsafe { &(*lp_ptr).super_ })
            && mainTask.is_none();

        // A `goto errorReadingProcess` on a failed leaf read; the tail after the
        // block runs the cleanup.
        let mut stat_command = [0u8; MAX_NAME + 1];
        let ok = 'body: {
            if !LinuxProcessTable_readStatmFile(
                unsafe { &mut *lp_ptr },
                procFd,
                lhost,
                main_task_ref(),
            ) {
                break 'body false;
            }

            // M_LRS: recalculate (readMaps) or copy from the main task.
            unsafe {
                let prev = (*lp_ptr).super_.usesDeletedLib;
                let is_kthread = (*lp_ptr).super_.isKernelThread;
                let is_uthread = (*lp_ptr).super_.isUserlandThread;
                if !is_kthread
                    && !is_uthread
                    && ((ss_flags & PROCESS_FLAG_LINUX_LRS_FIX) != 0
                        || (highlight_deleted_exe
                            && !(*lp_ptr).super_.procExeDeleted
                            && isOlderThan(&(*lp_ptr).super_, 10)))
                {
                    let passed = host.realtimeMs - (*lp_ptr).last_mlrs_calctime;
                    let recheck = (libc::rand() as u64) % 2048;
                    if passed > recheck {
                        (*lp_ptr).last_mlrs_calctime = host.realtimeMs;
                        LinuxProcessTable_readMaps(
                            &mut *lp_ptr,
                            procFd,
                            lhost,
                            (ss_flags & PROCESS_FLAG_LINUX_LRS_FIX) != 0,
                            highlight_deleted_exe,
                        );
                    }
                } else {
                    let from_main = is_uthread && mainTask.is_some();
                    (*lp_ptr).super_.usesDeletedLib =
                        from_main && (*mainTask.unwrap()).super_.usesDeletedLib;
                    (*lp_ptr).m_lrs = if from_main {
                        (*mainTask.unwrap()).m_lrs
                    } else {
                        0
                    };
                }
                if prev != (*lp_ptr).super_.usesDeletedLib {
                    (*lp_ptr).super_.mergedCommand.lastUpdate = 0;
                }
            }

            let lasttimes = unsafe { (*lp_ptr).utime + (*lp_ptr).stime };
            let last_tty_nr = unsafe { (*lp_ptr).super_.tty_nr };
            if !LinuxProcessTable_readStatFile(
                unsafe { &mut *lp_ptr },
                procFd,
                lhost,
                scan_main_thread,
                &mut stat_command,
                MAX_NAME + 1,
            ) {
                break 'body false;
            }

            unsafe {
                if (*lp_ptr).flags & PF_KTHREAD != 0 {
                    (*lp_ptr).super_.isKernelThread = true;
                }
                if last_tty_nr != (*lp_ptr).super_.tty_nr {
                    if let Some(drivers) = &(*this_ptr).ttyDrivers {
                        (*lp_ptr).super_.tty_name = Some(LinuxProcessTable_updateTtyDevice(
                            drivers,
                            (*lp_ptr).super_.tty_nr,
                        ));
                    }
                }

                (*lp_ptr).super_.percent_cpu = f32::NAN;
                if lhost.period > 0.0 {
                    let pcpu = saturatingSub((*lp_ptr).utime + (*lp_ptr).stime, lasttimes) as f64
                        / lhost.period
                        * 100.0;
                    (*lp_ptr).super_.percent_cpu =
                        (pcpu as f32).min(host.activeCPUs as f32 * 100.0);
                }
                (*lp_ptr).super_.percent_mem =
                    (*lp_ptr).super_.m_resident as f32 / host.totalMem as f32 * 100.0;
                Process_updateCPUFieldWidths((*lp_ptr).super_.percent_cpu);
            }

            if !LinuxProcessTable_updateUser(
                host,
                unsafe { &mut (*lp_ptr).super_ },
                procFd,
                main_task_ref(),
            ) {
                break 'body false;
            }

            // Container / PID-namespace check.
            unsafe {
                if (*lp_ptr).super_.isRunningInContainer == Tristate::TRI_INITIAL
                    && ROOT_PID_NS.load(Ordering::Relaxed) != -1
                {
                    let mut sb: libc::stat = std::mem::zeroed();
                    if crate::ported::linux::compat::Compat_fstatat(
                        procFd, c"ns/pid", c"ns/pid", &mut sb, 0,
                    ) == 0
                    {
                        (*lp_ptr).super_.isRunningInContainer =
                            if sb.st_ino as i64 != ROOT_PID_NS.load(Ordering::Relaxed) {
                                Tristate::TRI_ON
                            } else {
                                Tristate::TRI_OFF
                            };
                    }
                }

                if (ss_flags & PROCESS_FLAG_LINUX_CTXT) != 0
                    || ((hide_container || (ss_flags & PROCESS_FLAG_LINUX_CONTAINER) != 0)
                        && (*lp_ptr).super_.isRunningInContainer == Tristate::TRI_INITIAL)
                {
                    (*lp_ptr).super_.isRunningInContainer = Tristate::TRI_OFF;
                    if !LinuxProcessTable_readStatusFile(&mut *lp_ptr, procFd) {
                        break 'body false;
                    }
                }
            }

            unsafe {
                if !pre_existing {
                    if (*lp_ptr).super_.isKernelThread {
                        Process_updateCmdline(&mut (*lp_ptr).super_, None, 0, 0);
                    } else {
                        // statCommand as &str up to its NUL terminator.
                        let sc_end = stat_command
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(stat_command.len());
                        let sc = std::str::from_utf8(&stat_command[..sc_end]).unwrap_or("");
                        if !LinuxProcessTable_readCmdlineFile(
                            &mut (*lp_ptr).super_,
                            procFd,
                            main_task_ref(),
                        ) {
                            Process_updateCmdline(&mut (*lp_ptr).super_, Some(sc), 0, sc.len());
                        }
                        LinuxProcessList_readComm(&mut (*lp_ptr).super_, procFd);
                    }
                    Process_fillStarttimeBuffer(&mut (*lp_ptr).super_);
                    // ProcessTable_add(pt, proc) — already added by getProcess.
                } else if update_process_names && (*lp_ptr).super_.state != ProcessState::ZOMBIE {
                    if (*lp_ptr).super_.isKernelThread {
                        Process_updateCmdline(&mut (*lp_ptr).super_, None, 0, 0);
                    } else {
                        // statCommand as &str up to its NUL terminator.
                        let sc_end = stat_command
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(stat_command.len());
                        let sc = std::str::from_utf8(&stat_command[..sc_end]).unwrap_or("");
                        if !LinuxProcessTable_readCmdlineFile(
                            &mut (*lp_ptr).super_,
                            procFd,
                            main_task_ref(),
                        ) {
                            Process_updateCmdline(&mut (*lp_ptr).super_, Some(sc), 0, sc.len());
                        }
                        LinuxProcessList_readComm(&mut (*lp_ptr).super_, procFd);
                    }
                }
            }

            // Non-critical, independent information.
            unsafe {
                // Permitted capabilities for a non-root process (thread-specific).
                // Linux-only `syscall(SYS_capget, &header, &data)`; `__user_cap_*`
                // are declared inline (absent from `libc`) with the stable
                // `_LINUX_CAPABILITY_VERSION_3` layout (two 32-bit data blocks).
                #[cfg(target_os = "linux")]
                if (*lp_ptr).super_.st_uid != 0
                    && (*lp_ptr).super_.elevated_priv != Tristate::TRI_OFF
                {
                    #[repr(C)]
                    struct CapHeader {
                        version: u32,
                        pid: i32,
                    }
                    #[repr(C)]
                    #[derive(Clone, Copy)]
                    struct CapData {
                        effective: u32,
                        permitted: u32,
                        inheritable: u32,
                    }
                    let header = CapHeader {
                        version: 0x2008_0522, // _LINUX_CAPABILITY_VERSION_3
                        pid: Process_getPid(&(*lp_ptr).super_),
                    };
                    let mut data = [CapData {
                        effective: 0,
                        permitted: 0,
                        inheritable: 0,
                    }; 2];
                    let res = libc::syscall(
                        libc::SYS_capget,
                        &header as *const CapHeader,
                        data.as_mut_ptr(),
                    );
                    (*lp_ptr).super_.elevated_priv =
                        if res == 0 && (data[0].permitted != 0 || data[1].permitted != 0) {
                            Tristate::TRI_ON
                        } else {
                            Tristate::TRI_OFF
                        };
                }

                if (ss_flags & PROCESS_FLAG_LINUX_CGROUP) != 0 {
                    LinuxProcessTable_readCGroupFile(&mut *lp_ptr, procFd);
                }

                if (ss_flags & PROCESS_FLAG_LINUX_SMAPS) != 0
                    && !Process_isKernelThread(&(*lp_ptr).super_)
                {
                    if mainTask.is_none() {
                        // Read smaps every other pass for performance.
                        static SMAPS_FLAG: std::sync::atomic::AtomicI32 =
                            std::sync::atomic::AtomicI32::new(0);
                        let flag = SMAPS_FLAG.load(Ordering::Relaxed);
                        if (pid & 1) == flag {
                            LinuxProcessTable_readSmapsFile(
                                &mut *lp_ptr,
                                procFd,
                                (*this_ptr).haveSmapsRollup,
                            );
                        }
                        if pid == 1 {
                            SMAPS_FLAG.store(if flag == 0 { 1 } else { 0 }, Ordering::Relaxed);
                        }
                    } else {
                        let mt = &*mainTask.unwrap();
                        (*lp_ptr).m_pss = mt.m_pss;
                        (*lp_ptr).m_swap = mt.m_swap;
                        (*lp_ptr).m_psswp = mt.m_psswp;
                    }
                }

                if (ss_flags & PROCESS_FLAG_IO) != 0 {
                    LinuxProcessTable_readIoFile(&mut *lp_ptr, procFd, lhost, scan_main_thread);
                }

                if (ss_flags & PROCESS_FLAG_LINUX_OOM) != 0 {
                    LinuxProcessTable_readOomData(&mut *lp_ptr, procFd, main_task_ref());
                }

                if (ss_flags & PROCESS_FLAG_LINUX_IOPRIO) != 0 {
                    LinuxProcess_updateIOPriority(&mut *lp_ptr);
                }

                if (ss_flags & PROCESS_FLAG_LINUX_SECATTR) != 0 {
                    LinuxProcessTable_readSecattrData(&mut *lp_ptr, procFd, main_task_ref());
                }

                if (ss_flags & PROCESS_FLAG_CWD) != 0 {
                    LinuxProcessTable_readCwd(&mut *lp_ptr, procFd, main_task_ref());
                }

                if (ss_flags & PROCESS_FLAG_LINUX_AUTOGROUP) != 0 && (*this_ptr).haveAutogroup {
                    LinuxProcessTable_readAutogroup(&mut *lp_ptr, procFd, main_task_ref());
                }

                if (ss_flags & PROCESS_FLAG_SCHEDPOL) != 0 {
                    Scheduling_readProcessPolicy(&mut (*lp_ptr).super_);
                }

                if (ss_flags & PROCESS_FLAG_LINUX_GPU) != 0 || GPUMeter_active() {
                    if let Some(mt) = mainTask {
                        (*lp_ptr).gpu_time = (*mt).gpu_time;
                    } else {
                        GPU_readProcessData(&mut *this_ptr, &mut *lp_ptr, procFd);
                    }
                }
            }

            true
        };

        if !ok {
            // errorReadingProcess:
            Compat_openatArgClose(procFd);
            if !pre_existing {
                // A really short-lived process: remove the row getProcess added.
                this.super_.super_.rows[idx] = None;
                this.super_.super_.table.remove(&pid);
            }
            continue;
        }

        // Final section.
        unsafe {
            if (*lp_ptr).super_.cmdline.is_none()
                && stat_command[0] != 0
                && ((*lp_ptr).super_.state == ProcessState::ZOMBIE
                    || Process_isKernelThread(&(*lp_ptr).super_)
                    || show_thread_names)
            {
                let sc_end = stat_command
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(stat_command.len());
                let sc = std::str::from_utf8(&stat_command[..sc_end]).unwrap_or("");
                Process_updateCmdline(&mut (*lp_ptr).super_, Some(sc), 0, sc.len());
            }

            (*lp_ptr).super_.super_.updated = true;
        }
        Compat_openatArgClose(procFd);

        unsafe {
            if hide_container && (*lp_ptr).super_.isRunningInContainer == Tristate::TRI_ON {
                (*lp_ptr).super_.super_.show = false;
                continue;
            }

            if Process_isKernelThread(&(*lp_ptr).super_) {
                (*this_ptr).super_.kernelThreads += 1;
            } else if Process_isUserlandThread(&(*lp_ptr).super_) {
                (*this_ptr).super_.userlandThreads += 1;
            }

            // Set show now that we know if the entry is a thread.
            (*lp_ptr).super_.super_.show = !((hide_kernel
                && Process_isKernelThread(&(*lp_ptr).super_))
                || (hide_userland && Process_isUserlandThread(&(*lp_ptr).super_)));

            (*this_ptr).super_.totalTasks += 1;
        }
    }

    unsafe { libc::closedir(dir) };
    true
}

/// Port of `void ProcessTable_goThroughEntries(ProcessTable* super)` from
/// `LinuxProcessTable.c:1951`. Refreshes the autogroup-enabled flag, shifts the
/// per-tick GPU counters, then kicks off the `/proc` walk via
/// [`LinuxProcessTable_recurseProcTree`] from `AT_FDCWD` / `PROCDIR`. The C's
/// `LinuxMachine*` is reached through the table's `host` back-pointer (a
/// distinct object from the table, so the later `&mut this` / `&lhost` borrows
/// are disjoint).
pub fn ProcessTable_goThroughEntries(this: &mut LinuxProcessTable) {
    // Machine* host = super->super.host; LinuxMachine* lhost = (LinuxMachine*)host;
    // SAFETY: the table's `host` points to the owning LinuxMachine.
    let host_ptr = this.super_.super_.host as *mut LinuxMachine;

    let ss_flags = {
        let s = unsafe { &*host_ptr }
            .super_
            .settings
            .as_ref()
            .expect("goThroughEntries: host->settings is NULL");
        s.screens[s.ssIndex as usize].flags
    };

    // haveAutogroup = (flag set) ? LinuxProcess_isAutogroupEnabled() : false.
    this.haveAutogroup =
        (ss_flags & PROCESS_FLAG_LINUX_AUTOGROUP) != 0 && LinuxProcess_isAutogroupEnabled();

    // Shift GPU values: prevGpuTime = curGpuTime; curGpuTime = 0; per-engine too.
    unsafe {
        let lhost = &mut *host_ptr;
        lhost.prevGpuTime = lhost.curGpuTime;
        lhost.curGpuTime = 0;
        let mut engine = lhost.gpuEngineData.as_deref_mut();
        while let Some(e) = engine {
            e.prevTime = e.curTime;
            e.curTime = 0;
            engine = e.next.as_deref_mut();
        }
    }

    // recurseProcTree(this, AT_FDCWD, lhost, PROCDIR, NULL).
    let lhost_ref: &LinuxMachine = unsafe { &*host_ptr };
    LinuxProcessTable_recurseProcTree(this, libc::AT_FDCWD, lhost_ref, c"/proc", None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_strtoull_dec_parses_and_advances() {
        let s = b"12345rest";
        let mut cur: &[u8] = s;
        assert_eq!(fast_strtoull_dec(&mut cur, 0), 12345);
        assert_eq!(cur, b"rest");

        // maxlen caps the number of digits consumed.
        let mut cur2: &[u8] = b"12345";
        assert_eq!(fast_strtoull_dec(&mut cur2, 3), 123);
        assert_eq!(cur2, b"45");

        // Empty / non-digit input yields 0 and does not advance.
        let mut cur3: &[u8] = b"abc";
        assert_eq!(fast_strtoull_dec(&mut cur3, 0), 0);
        assert_eq!(cur3, b"abc");
    }

    #[test]
    fn fast_strtoll_dec_handles_sign() {
        let mut cur: &[u8] = b"-42 ";
        assert_eq!(fast_strtoll_dec(&mut cur, 0), -42);
        assert_eq!(cur, b" ");

        let mut cur2: &[u8] = b"7";
        assert_eq!(fast_strtoll_dec(&mut cur2, 0), 7);
    }

    #[test]
    fn fast_strtoi_and_strtol_and_strtoul() {
        let mut cur: &[u8] = b"-2147483648";
        assert_eq!(fast_strtoi_dec(&mut cur, 0), i32::MIN);

        let mut cur2: &[u8] = b"-5";
        assert_eq!(fast_strtol_dec(&mut cur2, 0), -5);

        let mut cur3: &[u8] = b"100";
        assert_eq!(fast_strtoul_dec(&mut cur3, 0), 100);
    }

    #[test]
    fn fast_strtoull_hex_parses_mixed_case() {
        let mut cur: &[u8] = b"deadBEEF!";
        assert_eq!(fast_strtoull_hex(&mut cur, 0), 0xdeadbeef);
        assert_eq!(cur, b"!");

        let mut cur2: &[u8] = b"ff";
        assert_eq!(fast_strtoull_hex(&mut cur2, 0), 0xff);

        // Non-hex input stops immediately at 0.
        let mut cur3: &[u8] = b"zzz";
        assert_eq!(fast_strtoull_hex(&mut cur3, 0), 0);
        assert_eq!(cur3, b"zzz");
    }

    #[test]
    fn strtopid_accepts_valid_and_rejects_junk() {
        assert_eq!(strtopid("1234"), 1234);
        assert_eq!(strtopid("0"), 0); // zero is not a valid pid
        assert_eq!(strtopid("12a"), 0); // trailing garbage
        assert_eq!(strtopid(""), 0);
        assert_eq!(strtopid("-5"), 0); // negative wraps huge -> rejected
    }

    #[test]
    fn sortTtyDrivers_orders_by_major_then_minor() {
        let a = TtyDriver {
            path: None,
            major: 4,
            minorFrom: 64,
            minorTo: 95,
        };
        let b = TtyDriver {
            path: None,
            major: 4,
            minorFrom: 0,
            minorTo: 63,
        };
        let c = TtyDriver {
            path: None,
            major: 5,
            minorFrom: 0,
            minorTo: 1,
        };

        assert_eq!(sortTtyDrivers(&a, &b), 1);
        assert_eq!(sortTtyDrivers(&b, &a), -1);
        assert_eq!(sortTtyDrivers(&a, &c), -1);
        assert_eq!(sortTtyDrivers(&a, &a), 0);
    }

    #[test]
    fn isOlderThan_compares_against_host_realtime() {
        use crate::ported::machine::Machine;
        use crate::ported::process::Process;
        use core::ffi::c_void;

        let mut host = Machine::default();
        host.realtimeMs = 100_000; // 100 s of realtime

        let mut proc = Process::default();
        proc.super_.host = &host as *const Machine as *const c_void;

        // Alive since t=50s -> 50s old: older than 10s, not older than 60s.
        proc.starttime_ctime = 50;
        assert!(isOlderThan(&proc, 10));
        assert!(!isOlderThan(&proc, 60));

        // Unparsed start time (<= 0) is never "older than".
        proc.starttime_ctime = 0;
        assert!(!isOlderThan(&proc, 0));
    }

    #[test]
    fn updateUser_mainTask_copies_owner_identity() {
        use crate::ported::machine::Machine;
        use crate::ported::process::Process;

        let host = Machine::default();

        // The owning process (mainTask) carries the resolved identity.
        let mut main = LinuxProcess::default();
        main.super_.st_uid = 4321;
        main.super_.user = Some("owner".to_string());

        // A thread starts with a different, stale identity.
        let mut thread = Process::default();
        thread.st_uid = 0;
        thread.user = None;

        // procFd is unused on the mainTask branch (-1 would fault if read).
        let ok = LinuxProcessTable_updateUser(&host, &mut thread, -1, Some(&main));
        assert!(ok);
        assert_eq!(thread.st_uid, 4321);
        assert_eq!(thread.user.as_deref(), Some("owner"));
    }

    #[test]
    fn updateUser_real_process_fstats_and_resolves_name() {
        use crate::ported::machine::Machine;
        use crate::ported::process::Process;
        use crate::ported::userstable::UsersTable_new;
        use std::os::unix::io::AsRawFd;

        // The machine's opaque usersTable handle: a leaked UsersTable reached
        // by raw pointer, exactly as the real Machine holds it.
        let ut = Box::into_raw(Box::new(UsersTable_new()));
        let mut host = Machine::default();
        host.usersTable = Some(ut as usize);

        // /dev/null is deterministically owned by uid 0 (root) on every Unix,
        // so fstat(procFd).st_uid == 0 and getpwuid(0) resolves to "root".
        let f = std::fs::File::open("/dev/null").expect("open /dev/null");

        let mut proc = Process::default();
        proc.st_uid = u32::MAX; // force the "uid changed" branch
        proc.user = None;

        let ok = LinuxProcessTable_updateUser(&host, &mut proc, f.as_raw_fd(), None);
        assert!(ok);
        assert_eq!(proc.st_uid, 0);
        assert_eq!(proc.user.as_deref(), Some("root"));

        // SAFETY: reclaim the leaked table (test-only cleanliness).
        unsafe { drop(Box::from_raw(ut)) };
    }

    #[test]
    fn updateUser_returns_false_on_bad_fd() {
        use crate::ported::machine::Machine;
        use crate::ported::process::Process;

        let host = Machine::default();
        let mut proc = Process::default();
        // fstat on a closed/invalid fd fails (EBADF) -> statok == -1 -> false.
        let ok = LinuxProcessTable_updateUser(&host, &mut proc, -1, None);
        assert!(!ok);
    }

    /// Opens `dir` as a dirfd (the `openat_arg_t` a real scan passes), so
    /// `fopenat(procFd, "maps")` reads the synthetic file. Both work on macOS.
    fn open_dirfd(dir: &std::path::Path) -> openat_arg_t {
        let c = std::ffi::CString::new(dir.to_str().unwrap()).unwrap();
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY) };
        assert!(fd >= 0, "open dirfd");
        fd
    }

    #[test]
    fn readMaps_calcsize_sums_executable_library_pages() {
        // Two file-backed regions of inode 131074 (one exec, one not) and one
        // exec region of inode 262145. calcLibSize sums only executable-tagged
        // libraries' total mapped bytes: (0x1000 + 0x1000) + 0x10000 = 73728,
        // / pageSize 4096 = 18 pages. The no-'/' anon line is skipped.
        let dir = std::env::temp_dir().join("htoprs_readmaps_calcsize");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("maps"),
            b"00400000-00401000 r-xp 00000000 08:01 131074 /usr/bin/cat\n\
              00401000-00402000 r--p 00001000 08:01 131074 /usr/bin/cat\n\
              7f0000000000-7f0000010000 r-xp 00000000 08:01 262145 /usr/lib/libc.so\n\
              7f0000010000-7f0000020000 rw-p 00000000 00:00 0 \n"
                .as_ref(),
        )
        .unwrap();
        let fd = open_dirfd(&dir);

        let mut lp = LinuxProcess::default();
        let lhost = LinuxMachine {
            pageSize: 4096,
            ..Default::default()
        };
        LinuxProcessTable_readMaps(&mut lp, fd, &lhost, true, false);
        unsafe { libc::close(fd) };
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(lp.m_lrs, 18);
        assert!(!lp.super_.usesDeletedLib); // checkDeletedLib was false
    }

    #[test]
    fn readMaps_checkdeleted_flags_deleted_executable_mapping() {
        // An executable mapping whose path ends " (deleted)" sets usesDeletedLib;
        // /memfd: and the VirtualBox /dev/zero false positive do not.
        let dir = std::env::temp_dir().join("htoprs_readmaps_deleted");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("maps"),
            b"7f1000000000-7f1000001000 r-xp 00000000 08:01 111 /memfd:foo (deleted)\n\
              7f1000001000-7f1000002000 r-xp 00000000 00:05 222 /dev/zero (deleted)\n\
              7f1000002000-7f1000003000 r-xp 00000000 08:01 333 /usr/lib/gone.so (deleted)\n"
                .as_ref(),
        )
        .unwrap();
        let fd = open_dirfd(&dir);

        let mut lp = LinuxProcess::default();
        let lhost = LinuxMachine::default();
        LinuxProcessTable_readMaps(&mut lp, fd, &lhost, false, true);
        unsafe { libc::close(fd) };
        std::fs::remove_dir_all(&dir).ok();

        assert!(lp.super_.usesDeletedLib);
    }

    #[test]
    fn readMaps_missing_file_is_a_noop() {
        // fopenat fails on a dir with no "maps" -> early return, usesDeletedLib
        // reset to false, m_lrs untouched.
        let dir = std::env::temp_dir().join("htoprs_readmaps_empty");
        std::fs::create_dir_all(&dir).unwrap();
        let fd = open_dirfd(&dir);
        let mut lp = LinuxProcess::default();
        lp.m_lrs = 999;
        let lhost = LinuxMachine::default();
        LinuxProcessTable_readMaps(&mut lp, fd, &lhost, true, true);
        unsafe { libc::close(fd) };
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(lp.m_lrs, 999); // untouched (early return before calcSize)
    }

    #[test]
    fn recurseProcTree_scans_a_synthetic_proc_directory() {
        use crate::ported::machine::{ScreenSettings, Settings};

        // Build a synthetic /proc containing one PID directory "42" with the
        // always-on files the core scan reads (ss.flags = 0 → no gated readers).
        let root = std::env::temp_dir().join("htoprs_recurse_proc");
        let _ = std::fs::remove_dir_all(&root);
        let p = root.join("42");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(
            p.join("stat"),
            b"42 (testproc) R 1 42 42 0 -1 0 100 0 0 0 10 5 0 0 20 0 1 0 137 \
              4194304 50 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n"
                .as_ref(),
        )
        .unwrap();
        std::fs::write(p.join("statm"), b"1024 50 10 5 0 20 0\n".as_ref()).unwrap();
        std::fs::write(p.join("cmdline"), b"testproc\0".as_ref()).unwrap();
        std::fs::write(p.join("comm"), b"testproc\n".as_ref()).unwrap();
        // scanMainThread reads task/<pid>/stat for the main thread's times.
        let task = p.join("task/42");
        std::fs::create_dir_all(&task).unwrap();
        std::fs::write(
            task.join("stat"),
            b"42 (testproc) R 1 42 42 0 -1 0 100 0 0 0 10 5 0 0 20 0 1 0 137 \
              4194304 50 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n"
                .as_ref(),
        )
        .unwrap();

        let mut lm = LinuxMachine {
            pageSize: 4096,
            jiffies: 100,
            period: 100.0,
            runningTasks: 1,
            super_: Machine {
                existingCPUs: 1,
                activeCPUs: 1,
                totalMem: 1_000_000,
                realtimeMs: 1_000_000,
                settings: Some(Settings {
                    screens: vec![ScreenSettings::default()],
                    ssIndex: 0,
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let host_ptr = &mut lm.super_ as *mut Machine;
        let mut pt = ProcessTable_new(host_ptr, None);

        // recurseProcTree(this, AT_FDCWD, lhost, <abs root>, NULL).
        let dir_c = std::ffi::CString::new(root.to_str().unwrap()).unwrap();
        let ok = LinuxProcessTable_recurseProcTree(&mut pt, libc::AT_FDCWD, &lm, &dir_c, None);
        let _ = std::fs::remove_dir_all(&root);
        assert!(ok, "recurseProcTree returned false");

        // PID 42 was enumerated into the table with its parsed identity.
        assert!(
            pt.super_.super_.table.contains_key(&42),
            "pid 42 not in table"
        );
        let idx = pt.super_.super_.table[&42];
        let proc = pt.super_.super_.rows[idx]
            .as_ref()
            .unwrap()
            .as_process()
            .unwrap();
        assert_eq!(Process_getPid(proc), 42);
        assert_eq!(pt.super_.totalTasks, 1, "one task counted");
        // The row is shown and marked updated by the scan.
        assert!(proc.super_.updated);
    }

    #[test]
    fn getProcessState_maps_known_and_unknown() {
        assert_eq!(
            LinuxProcessTable_getProcessState(b'R'),
            ProcessState::RUNNING
        );
        assert_eq!(
            LinuxProcessTable_getProcessState(b'S'),
            ProcessState::SLEEPING
        );
        assert_eq!(
            LinuxProcessTable_getProcessState(b'Z'),
            ProcessState::ZOMBIE
        );
        assert_eq!(
            LinuxProcessTable_getProcessState(b'D'),
            ProcessState::UNINTERRUPTIBLE_WAIT
        );
        assert_eq!(
            LinuxProcessTable_getProcessState(b'?'),
            ProcessState::UNKNOWN
        );
    }
}
