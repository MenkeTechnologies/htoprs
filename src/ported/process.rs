//! Partial port of `Process.c` + `Process.h` — the process data model
//! and its pure comparison / predicate logic. Everything requiring the
//! unported `Machine` / `Settings` / `Table` substrate, syscalls, or the
//! ncurses draw layer remains a `todo!()` stub named after its real htop
//! C function.
//!
//! Ported data model: [`Process`] (every `Process.h` field; embeds
//! [`Row`] as its `super` base — htop's emulated OOP), the [`ProcessField`]
//! column-id enum (`RowField.h` reserved fields), [`Tristate`],
//! [`ProcessMergedCommand`] + [`ProcessCmdlineHighlight`], and the
//! [`Object`] trait impl chaining `Process_class` → `Row_class`.
//!
//! Ported logic: [`Process_compareByKey_Base`] (the per-field sort
//! switch), the pure predicates [`Process_isKernelThread`],
//! [`Process_isUserlandThread`], [`Process_isThread`], the pid/parent/tgid
//! getters + setters, [`Process_init`], and the pure string/state helpers
//! [`processStateChar`], [`findCommInCmdline`],
//! [`matchCmdlinePrefixWithExeSuffix`], [`skipPotentialPath`]. C `const
//! char*` + `size_t` helpers are modeled on `&[u8]` + `usize`;
//! NUL-terminated reads treat any index at/after the slice length as the
//! terminating NUL (`0`). Out-params are returned as tuples/`Option`.
//!
//! Still stubbed (need unported substrate): [`Process_compare`] and
//! [`Process_compareByParent`] (read `settings->ss` / `ScreenSettings`),
//! [`Process_getCommand`] (reads `host->settings->showThreadNames` — the
//! ported `Settings` subset has no `showThreadNames` field and `Row::host`
//! is an opaque pointer), [`Process_makeCommandStr`] (every branch is
//! driven by a `Settings` flag; the ported subset models none of
//! `showMergedCommand` / `showProgramPath` / `findCommInCmdline` /
//! `stripExeFromCmdline` / `showThreadNames` / `shadowDistPathPrefix` /
//! `lastUpdate`, and it also needs `CRT_treeStr[TREE_STR_VERT]`, the
//! `CMDLINE_HIGHLIGHT_FLAG_*` constants, and `CRT_colors[...]`), and the
//! writeField / init-display / syscall / filter functions. The `COMM`
//! sort case in [`Process_compareByKey_Base`] delegates to the stubbed
//! [`Process_getCommand`], so that one case is not exercisable until the
//! `Settings` substrate lands (every other case is pure and tested).
//! `gen_port_report.py` counts `todo!()` bodies as *stubbed*, not
//! *ported*, so scaffolding does not inflate coverage.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::object::{Object, ObjectClass};
use crate::ported::row::{spaceship_number, Row, Row_class, Row_init};
use crate::ported::xutils::compareRealNumbers;
use core::any::Any;
use core::ffi::c_void;

/// Port of `#define SPACESHIP_NULLSTR(a, b)` from `Macros.h:37`:
/// `strcmp((a) ? (a) : "", (b) ? (b) : "")`. NULL operands (`None`)
/// compare as the empty string. Only the sign is significant for
/// sorting; `[u8]::cmp` is lexicographic on unsigned bytes, matching
/// `strcmp`'s sign.
macro_rules! spaceship_nullstr {
    ($a:expr, $b:expr) => {{
        let a: &[u8] = $a.unwrap_or(b"");
        let b: &[u8] = $b.unwrap_or(b"");
        match a.cmp(b) {
            core::cmp::Ordering::Less => -1,
            core::cmp::Ordering::Equal => 0,
            core::cmp::Ordering::Greater => 1,
        }
    }};
}

/// Port of `#define SPACESHIP_DEFAULTSTR(a, b, s)` from `Macros.h:41`:
/// `strcmp((a) ? (a) : (s), (b) ? (b) : (s))`. NULL operands (`None`)
/// fall back to the default `s` instead of the empty string.
macro_rules! spaceship_defaultstr {
    ($a:expr, $b:expr, $s:expr) => {{
        let a: &[u8] = $a.unwrap_or($s);
        let b: &[u8] = $b.unwrap_or($s);
        match a.cmp(b) {
            core::cmp::Ordering::Less => -1,
            core::cmp::Ordering::Equal => 0,
            core::cmp::Ordering::Greater => 1,
        }
    }};
}

/// Port of `static const char* const kthreadID = "KTHREAD"` from
/// `Process.c:41` — the placeholder comm/exe used for kernel threads
/// that expose no command string.
const kthreadID: &[u8] = b"KTHREAD";

/// Port of `enum ProcessState_` from `Process.h:41`. Discriminants match
/// the C enum exactly (`UNKNOWN = 1`, the rest ascending).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum ProcessState {
    UNKNOWN = 1,
    RUNNABLE,
    RUNNING,
    QUEUED,
    WAITING,
    UNINTERRUPTIBLE_WAIT,
    BLOCKED,
    PAGING,
    STOPPED,
    TRACED,
    ZOMBIE,
    DEFUNCT,
    IDLE,
    SLEEPING,
}

/// Port of `enum Tristate_` from `Process.h:31`. A three-state flag;
/// discriminants match the C enum exactly (`TRI_OFF = -1`,
/// `TRI_INITIAL = 0`, `TRI_ON = 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i8)]
#[allow(non_camel_case_types)]
pub enum Tristate {
    /// C `TRI_OFF = -1`.
    TRI_OFF = -1,
    /// C `TRI_INITIAL = 0` — the default, un-probed state.
    #[default]
    TRI_INITIAL = 0,
    /// C `TRI_ON = 1`.
    TRI_ON = 1,
}

/// Port of `struct ProcessCmdlineHighlight_` from `Process.h:63`. A
/// region of the merged command string to color.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessCmdlineHighlight {
    /// C `size_t offset` — first character to highlight.
    pub offset: usize,
    /// C `size_t length` — characters to highlight, zero if unused.
    pub length: usize,
    /// C `int attr` — the highlight attributes.
    pub attr: i32,
    /// C `int flags` — selective-highlight flags (`CMDLINE_HIGHLIGHT_FLAG_*`).
    pub flags: i32,
}

/// Port of `struct ProcessMergedCommand_` from `Process.h:74`. Populated
/// by `Process_makeCommandStr` (stubbed) with the merged Command string
/// and the highlight regions `Process_writeCommand` uses to color it.
#[derive(Debug, Clone, Default)]
pub struct ProcessMergedCommand {
    /// C `uint64_t lastUpdate` — settings-marker for cache invalidation.
    pub lastUpdate: u64,
    /// C `char* str` — the merged command string; `None` (C `NULL`) for
    /// kernel threads and zombies.
    pub str: Option<String>,
    /// C `size_t highlightCount` — active entries in `highlights`.
    pub highlightCount: usize,
    /// C `ProcessCmdlineHighlight highlights[8]`.
    pub highlights: [ProcessCmdlineHighlight; 8],
}

/// Port of the `ReservedFields` enum from `RowField.h:12` (the process
/// field / column-id list). `Process.h:249` aliases `typedef int32_t
/// ProcessField`; this models the fixed reserved set with the exact C
/// discriminants (note the intentional gaps — e.g. `9`, `11`, `13`–`17`
/// are platform-specific fields defined per-platform in
/// `${platform}/ProcessField.h`, not part of the generic set).
///
/// Platform-specific fields (Linux `UTIME`, `RBYTES`, … `= 11, 14, …`)
/// and runtime dynamic columns are *not* enumerated here; in htop they
/// are handled by each platform's `*_compareByKey` before it falls
/// through to [`Process_compareByKey_Base`], which only ever sees the
/// reserved fields below (plus the `_` default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
#[allow(non_camel_case_types)]
pub enum ProcessField {
    /// C `NULL_FIELD = 0`.
    NULL_FIELD = 0,
    PID = 1,
    COMM = 2,
    STATE = 3,
    PPID = 4,
    PGRP = 5,
    SESSION = 6,
    TTY = 7,
    TPGID = 8,
    MINFLT = 10,
    MAJFLT = 12,
    PRIORITY = 18,
    NICE = 19,
    STARTTIME = 21,
    PROCESSOR = 38,
    M_VIRT = 39,
    M_RESIDENT = 40,
    ST_UID = 46,
    PERCENT_CPU = 47,
    PERCENT_MEM = 48,
    USER = 49,
    TIME = 50,
    NLWP = 51,
    TGID = 52,
    PERCENT_NORM_CPU = 53,
    ELAPSED = 54,
    SCHEDULERPOLICY = 55,
    PROC_COMM = 124,
    PROC_EXE = 125,
    CWD = 126,
}

/// Port of `struct Process_` from `Process.h:81`. "Extends" [`Row`] via
/// the embedded `super` field (htop's emulated single-inheritance), and
/// carries the full per-process field set.
///
/// Field-type mapping:
/// - C `Row super;` — [`super_`](Process::super_), the embedded base
///   (raw identifier so the field name matches C's `super`). Accessed
///   through the pid/parent/tgid getters below, mirroring
///   `Process_getPid` et al.
/// - Owned C strings (`char* cmdline/tty_name/procComm/procExe/procCwd`)
///   → `Option<String>` (`None` = C `NULL`). The borrowed C `const char*
///   user` (which points into the users cache, not owned by `Process`)
///   is likewise modeled as `Option<String>` for a self-contained struct;
///   the ownership difference is noted here and does not affect any
///   ported comparison, which only reads the bytes.
/// - `uid_t st_uid` → `u32`; `time_t starttime_ctime` → `i64`;
///   `unsigned long`/`unsigned long long` counters → `u64`; `long`
///   memory sizes / priority / nlwp → `i64`; `char starttime_show[8]`
///   → `[u8; 8]`.
#[derive(Debug, Clone, Default)]
pub struct Process {
    /// C `Row super` — the embedded base class.
    pub super_: Row,

    /// C `int pgrp` — process group id.
    pub pgrp: i32,
    /// C `int session` — session id.
    pub session: i32,
    /// C `int tpgid` — foreground group of the controlling terminal.
    pub tpgid: i32,

    /// C `bool isKernelThread`.
    pub isKernelThread: bool,
    /// C `bool isUserlandThread`.
    pub isUserlandThread: bool,
    /// C `Tristate isRunningInContainer`.
    pub isRunningInContainer: Tristate,

    /// C `unsigned long int tty_nr` — controlling terminal id.
    pub tty_nr: u64,
    /// C `char* tty_name` — controlling terminal name.
    pub tty_name: Option<String>,

    /// C `uid_t st_uid` — user id.
    pub st_uid: u32,
    /// C `const char* user` — user name (borrowed in C; see struct docs).
    pub user: Option<String>,

    /// C `Tristate elevated_priv` — non-root with elevated privileges.
    pub elevated_priv: Tristate,

    /// C `unsigned long long int time` — runtime in hundredths of a second.
    pub time: u64,

    /// C `char* cmdline` — process name including arguments.
    pub cmdline: Option<String>,
    /// C `size_t cmdlineBasenameEnd`.
    pub cmdlineBasenameEnd: usize,
    /// C `size_t cmdlineBasenameStart`.
    pub cmdlineBasenameStart: usize,

    /// C `char* procComm` — the process' "command" name.
    pub procComm: Option<String>,
    /// C `char* procExe` — the main process executable.
    pub procExe: Option<String>,
    /// C `char* procCwd` — the working directory.
    pub procCwd: Option<String>,
    /// C `size_t procExeBasenameOffset` — offset of the basename in `procExe`.
    pub procExeBasenameOffset: usize,
    /// C `bool procExeDeleted`.
    pub procExeDeleted: bool,
    /// C `bool usesDeletedLib`.
    pub usesDeletedLib: bool,

    /// C `int processor` — CPU last executed on.
    pub processor: i32,
    /// C `float percent_cpu` — CPU usage last cycle.
    pub percent_cpu: f32,
    /// C `float percent_mem` — memory usage last cycle.
    pub percent_mem: f32,

    /// C `long int priority` — scheduling priority.
    pub priority: i64,
    /// C `int nice` — nice value.
    pub nice: i32,
    /// C `long int nlwp` — thread count.
    pub nlwp: i64,

    /// C `time_t starttime_ctime` — start time (epoch seconds).
    pub starttime_ctime: i64,
    /// C `char starttime_show[8]` — cached formatted start time.
    pub starttime_show: [u8; 8],

    /// C `long m_virt` — total program size (KiB).
    pub m_virt: i64,
    /// C `long m_resident` — resident set size (KiB).
    pub m_resident: i64,
    /// C `unsigned long int minflt` — minor page faults.
    pub minflt: u64,
    /// C `unsigned long int majflt` — major page faults.
    pub majflt: u64,

    /// C `ProcessState state`.
    pub state: ProcessState,
    /// C `int scheduling_policy`.
    pub scheduling_policy: i32,

    /// C `ProcessMergedCommand mergedCommand`.
    pub mergedCommand: ProcessMergedCommand,
}

impl Default for ProcessState {
    /// No C default (the field is set by `*ProcessTable_readProcess`);
    /// `UNKNOWN` is the safe sentinel (the C enum's first value, `1`) for
    /// a freshly-constructed [`Process`]. Note C's zero-initialization
    /// would leave an invalid `0`, so an explicit default is required.
    fn default() -> Self {
        ProcessState::UNKNOWN
    }
}

/// Port of `const ProcessClass Process_class` from `Process.c:1113`.
/// Its object-class super chain is `.extends = Class(Row)` with
/// `.compare = Process_compare`; only the base-class link is modeled by
/// [`ObjectClass`], and the compare slot is realized by the
/// [`Object::compare`] impl below. The `RowClass`/`ProcessClass` vtable
/// slots (`writeField`, `compareByKey`, …) live in stubbed functions.
pub static Process_class: ObjectClass = ObjectClass {
    extends: Some(&Row_class),
};

impl Object for Process {
    /// C `this->super.super.klass` set to `&Process_class`.
    fn klass(&self) -> &'static ObjectClass {
        &Process_class
    }

    /// C `Process_class.super.super.compare = Process_compare`. Downcasts
    /// the trait object back to `Process` (the safe-Rust analog of the C
    /// `const void*` cast) and delegates to [`Process_compare`] — which
    /// is stubbed pending the `Settings` substrate, so this dispatches to
    /// a `todo!()` for now, matching the C wiring.
    fn compare(&self, other: &dyn Object) -> i32 {
        let any: &dyn Any = other;
        let o = any
            .downcast_ref::<Process>()
            .expect("Process_compare called across incompatible classes");
        Process_compare(self, o)
    }
}

/// Port of `#define TASK_COMM_LEN 16` from `Process.c:65`.
const TASK_COMM_LEN: usize = 16;

/// Port of `findCommInCmdline(const char* comm, const char* cmdline,
/// size_t cmdlineBasenameStart, size_t* pCommStart, size_t* pCommLen)`
/// from `Process.c:67`. Tokenizes `cmdline` starting at
/// `cmdlineBasenameStart` (tokens split on `\n`, basename reset after
/// each `/`) and looks for a token whose basename equals `comm` — an
/// exact length match, or a longer token when `comm` is the max comm
/// length (`TASK_COMM_LEN - 1 == 15`, i.e. a truncated comm). Returns
/// `Some((commStart, commLen))` (the two C out-params) on the first
/// match, else `None`. `comm` and `cmdline` are byte slices with no
/// trailing NUL; the C `*token` end-of-string test maps to reaching the
/// slice end.
pub fn findCommInCmdline(
    comm: &[u8],
    cmdline: &[u8],
    cmdlineBasenameStart: usize,
) -> Option<(usize, usize)> {
    let commLen = comm.len();

    let mut token = cmdlineBasenameStart;
    while token < cmdline.len() {
        let mut tokenBase = token;
        while token < cmdline.len() && cmdline[token] != b'\n' {
            if cmdline[token] == b'/' {
                tokenBase = token + 1;
            }
            token += 1;
        }
        let tokenLen = token - tokenBase;

        if (tokenLen == commLen || (tokenLen > commLen && commLen == TASK_COMM_LEN - 1))
            && cmdline[tokenBase..tokenBase + commLen] == comm[..commLen]
        {
            return Some((tokenBase, tokenLen));
        }

        if token < cmdline.len() {
            loop {
                token += 1;
                if !(token < cmdline.len() && cmdline[token] == b'\n') {
                    break;
                }
            }
        }
    }
    None
}

/// Port of `matchCmdlinePrefixWithExeSuffix(const char* cmdline, size_t*
/// cmdlineBasenameStart, const char* exe, size_t exeBaseOffset, size_t
/// exeBaseLen)` from `Process.c:99`. Returns `(matchLen,
/// cmdlineBasenameStart)`: `matchLen` is the C return value (0 = no
/// match), and the second element is the (possibly adjusted) value of
/// the `*cmdlineBasenameStart` in/out-param — updated only on the
/// relative-path success path, otherwise the input passes through
/// unchanged. NUL-terminated reads are modeled as `0` for any index at
/// or beyond the slice length.
pub fn matchCmdlinePrefixWithExeSuffix(
    cmdline: &[u8],
    cmdlineBasenameStart: usize,
    exe: &[u8],
    exeBaseOffset: usize,
    exeBaseLen: usize,
) -> (usize, usize) {
    let at = |s: &[u8], i: usize| -> u8 {
        if i < s.len() {
            s[i]
        } else {
            0
        }
    };
    // strncmp(a+ao, b+bo, n) == 0 with C NUL semantics.
    let strncmp_eq = |a: &[u8], ao: usize, b: &[u8], bo: usize, n: usize| -> bool {
        for k in 0..n {
            let ca = if ao + k < a.len() { a[ao + k] } else { 0 };
            let cb = if bo + k < b.len() { b[bo + k] } else { 0 };
            if ca != cb {
                return false;
            }
            if ca == 0 {
                break;
            }
        }
        true
    };

    /* cmdline prefix is an absolute path: it must match whole exe. */
    if at(cmdline, 0) == b'/' {
        let matchLen = exeBaseLen + exeBaseOffset;
        if strncmp_eq(cmdline, 0, exe, 0, matchLen) {
            let delim = at(cmdline, matchLen);
            if delim == 0 || delim == b'\n' || delim == b' ' {
                return (matchLen, cmdlineBasenameStart);
            }
        }
        return (0, cmdlineBasenameStart);
    }

    /* cmdline prefix is a relative path: match the basename, then reverse
     * match the cmdline prefix with the exe suffix; if that fails, back
     * up to the previous cmdline path component and retry. */
    let mut cmdlineBaseOffset = cmdlineBasenameStart;
    let mut delimFound; /* if valid basename delimiter found */
    loop {
        /* match basename */
        let matchLen = exeBaseLen + cmdlineBaseOffset;
        if cmdlineBaseOffset < exeBaseOffset
            && strncmp_eq(cmdline, cmdlineBaseOffset, exe, exeBaseOffset, exeBaseLen)
        {
            let delim = at(cmdline, matchLen);
            if delim == 0 || delim == b'\n' || delim == b' ' {
                /* reverse match the cmdline prefix and exe suffix */
                let mut i = cmdlineBaseOffset;
                let mut j = exeBaseOffset;
                while i >= 1 && j >= 1 && at(cmdline, i - 1) == at(exe, j - 1) {
                    i -= 1;
                    j -= 1;
                }

                /* full match, with exe suffix being a valid relative path */
                if i < 1 && j >= 1 && at(exe, j - 1) == b'/' {
                    return (matchLen, cmdlineBaseOffset);
                }
            }
        }

        /* Try to find the previous potential cmdlineBaseOffset - it would
         * be preceded by '/' or nothing, and delimited by ' ' or '\n' */
        delimFound = false;
        if cmdlineBaseOffset <= 2 {
            return (0, cmdlineBasenameStart);
        }
        cmdlineBaseOffset -= 2;
        while cmdlineBaseOffset > 0 {
            if delimFound {
                if at(cmdline, cmdlineBaseOffset - 1) == b'/' {
                    break;
                }
            } else if at(cmdline, cmdlineBaseOffset) == b' '
                || at(cmdline, cmdlineBaseOffset) == b'\n'
            {
                delimFound = true;
            }
            cmdlineBaseOffset -= 1;
        }

        if !delimFound {
            return (0, cmdlineBasenameStart);
        }
    }
}

/// TODO: port of `void Process_fillStarttimeBuffer(Process* this` from `Process.c:43`.
pub fn Process_fillStarttimeBuffer() {
    todo!("port of Process.c:43")
}

/// Port of `static inline char* stpcpyWithNewlineConversion(char* dstStr,
/// const char* srcStr)` from `Process.c:169`. Copies `src` into `dst`,
/// converting each `'\n'` to `' '`. The C variant writes a terminating
/// NUL and returns the pointer just past the copied bytes (stpcpy
/// semantics) so callers can chain; a `Vec<u8>` tracks its own end and
/// carries no NUL, so the append leaves `dst` extended in place and no
/// end pointer is needed. Only reachable from the stubbed
/// [`Process_makeCommandStr`] in C.
pub fn stpcpyWithNewlineConversion(dst: &mut Vec<u8>, src: &[u8]) {
    for &c in src {
        dst.push(if c == b'\n' { b' ' } else { c });
    }
}

/// TODO: port of `void Process_makeCommandStr(Process* this, const
/// Settings* settings)` from `Process.c:183`. Core inputs entirely
/// unmodeled: every branch is driven by a `Settings` flag, and the
/// ported `Settings` subset (`settings.rs`) models none of the seven it
/// reads — `showMergedCommand`, `showProgramPath`, `findCommInCmdline`,
/// `stripExeFromCmdline`, `showThreadNames`, `shadowDistPathPrefix`
/// (`Process.c:186-191`), and `lastUpdate` (`Process.c:193`, the
/// cache-invalidation stamp). It further needs the field separator
/// `CRT_treeStr[TREE_STR_VERT]` (`Process.c:213`) — the `TREE_STR` tables
/// are unported — and the `CMDLINE_HIGHLIGHT_FLAG_*` constants + the
/// `CRT_colors[...]` palette (`Process.c:307-310`), neither defined in
/// the port. The pure Process-field inputs it consumes (`cmdline`,
/// `procComm`, `procExe`, `cmdlineBasenameStart/End`,
/// `procExeBasenameOffset`, `procExeDeleted`, `usesDeletedLib`, `state`)
/// *are* modeled, and its string helpers [`stpcpyWithNewlineConversion`],
/// [`findCommInCmdline`], [`matchCmdlinePrefixWithExeSuffix`] are ported —
/// but with the Settings flags absent there is no faithful subset to
/// port, so the whole body stays a stub.
pub fn Process_makeCommandStr() {
    todo!("port of Process.c:183 — needs Settings flags (showMergedCommand/showProgramPath/findCommInCmdline/stripExeFromCmdline/showThreadNames/shadowDistPathPrefix/lastUpdate) + CRT_treeStr + CMDLINE_HIGHLIGHT_FLAG_* + CRT_colors")
}

/// TODO: port of `void Process_writeCommand(const Process* this, int attr, int baseAttr, RichString* str` from `Process.c:471`.
pub fn Process_writeCommand() {
    todo!("port of Process.c:471")
}

/// Port of `processStateChar(ProcessState state)` from `Process.c:545`.
/// Maps a [`ProcessState`] to its single-character display code. The C
/// `default: assert(0); return '!'` path is unreachable here — a valid
/// `ProcessState` value covers every arm — so the match is exhaustive.
pub fn processStateChar(state: ProcessState) -> char {
    match state {
        ProcessState::UNKNOWN => '?',
        ProcessState::RUNNABLE => 'U',
        ProcessState::RUNNING => 'R',
        ProcessState::QUEUED => 'Q',
        ProcessState::WAITING => 'W',
        ProcessState::UNINTERRUPTIBLE_WAIT => 'D',
        ProcessState::BLOCKED => 'B',
        ProcessState::PAGING => 'P',
        ProcessState::STOPPED => 'T',
        ProcessState::TRACED => 't',
        ProcessState::ZOMBIE => 'Z',
        ProcessState::DEFUNCT => 'X',
        ProcessState::IDLE => 'I',
        ProcessState::SLEEPING => 'S',
    }
}

/// TODO: port of `static void Process_rowWriteField(const Row* super, RichString* str, RowField field` from `Process.c:567`.
pub fn Process_rowWriteField() {
    todo!("port of Process.c:567")
}

/// TODO: port of `void Process_writeField(const Process* this, RichString* str, RowField field` from `Process.c:573`.
pub fn Process_writeField() {
    todo!("port of Process.c:573")
}

/// TODO: port of `void Process_done(Process* this` from `Process.c:795`.
pub fn Process_done() {
    todo!("port of Process.c:795")
}

/// TODO: port of `const char* Process_getCommand(const Process* this)`
/// from `Process.c:831`. Blocked on a single missing input:
/// `this->super.host->settings->showThreadNames` (`Process.c:834`). Two
/// gaps make it unreachable — [`Row::host`](crate::ported::row::Row::host)
/// is an opaque `*const c_void` (the `Machine` deref to reach `settings`
/// is unavailable), and even given `settings`, the ported `Settings`
/// subset (`settings.rs`) carries no `showThreadNames` field. The other
/// three inputs the C body reads *are* modeled — `mergedCommand.str`
/// ([`Process::mergedCommand`]), `cmdline` ([`Process::cmdline`]), and
/// [`Process_isUserlandThread`] — so only the flag blocks it. Signature is
/// set so the `COMM` case of [`Process_compareByKey_Base`] can call it
/// faithfully; the body stays a stub. Returns the command bytes
/// (C `const char*`).
pub fn Process_getCommand(this: &Process) -> Option<&[u8]> {
    let _ = this;
    todo!("port of Process.c:831 — needs settings->showThreadNames (Settings subset lacks the field; Row::host is an opaque pointer)")
}

/// TODO: port of `static const char* Process_getSortKey(const Process* this` from `Process.c:818`.
pub fn Process_getSortKey() {
    todo!("port of Process.c:818")
}

/// TODO: port of `const char* Process_rowGetSortKey(Row* super` from `Process.c:822`.
pub fn Process_rowGetSortKey() {
    todo!("port of Process.c:822")
}

/// TODO: port of `static bool Process_isHighlighted(const Process* this` from `Process.c:829`.
pub fn Process_isHighlighted() {
    todo!("port of Process.c:829")
}

/// TODO: port of `bool Process_rowIsHighlighted(const Row* super` from `Process.c:835`.
pub fn Process_rowIsHighlighted() {
    todo!("port of Process.c:835")
}

/// TODO: port of `static bool Process_isVisible(const Process* p, const Settings* settings` from `Process.c:842`.
pub fn Process_isVisible() {
    todo!("port of Process.c:842")
}

/// TODO: port of `bool Process_rowIsVisible(const Row* super, const Table* table` from `Process.c:848`.
pub fn Process_rowIsVisible() {
    todo!("port of Process.c:848")
}

/// TODO: port of `static bool Process_matchesFilter(const Process* this, const Table* table` from `Process.c:855`.
pub fn Process_matchesFilter() {
    todo!("port of Process.c:855")
}

/// TODO: port of `bool Process_rowMatchesFilter(const Row* super, const Table* table` from `Process.c:872`.
pub fn Process_rowMatchesFilter() {
    todo!("port of Process.c:872")
}

/// Port of `void Process_init(Process* this, const Machine* host)` from
/// `Process.c:878`. Runs the base [`Row_init`] then sets the two
/// process-specific defaults the C body assigns
/// (`cmdlineBasenameEnd = 0`, `st_uid = (uid_t)-1`). Pure — `host` is
/// only stored, never dereferenced.
pub fn Process_init(this: &mut Process, host: *const c_void) {
    Row_init(&mut this.super_, host);

    this.cmdlineBasenameEnd = 0;
    this.st_uid = u32::MAX; // (uid_t)-1
}

/// TODO: port of `static bool Process_setPriority(Process* this, int priority` from `Process.c:885`.
pub fn Process_setPriority() {
    todo!("port of Process.c:885")
}

/// TODO: port of `bool Process_rowChangePriorityBy(Row* super, Arg delta` from `Process.c:898`.
pub fn Process_rowChangePriorityBy() {
    todo!("port of Process.c:898")
}

/// TODO: port of `static bool Process_sendSignal(Process* this, Arg sgn` from `Process.c:904`.
pub fn Process_sendSignal() {
    todo!("port of Process.c:904")
}

/// TODO: port of `bool Process_rowSendSignal(Row* super, Arg sgn` from `Process.c:908`.
pub fn Process_rowSendSignal() {
    todo!("port of Process.c:908")
}

/// TODO: port of `int Process_compare(const void* v1, const void* v2)`
/// from `Process.c:914`. Not portable yet: reads
/// `p1->super.host->settings->ss` and calls
/// `ScreenSettings_getActiveSortKey` / `ScreenSettings_getActiveDirection`
/// — the unported `Settings` / `ScreenSettings` substrate. The per-field
/// comparison it dispatches to *is* ported (see
/// [`Process_compareByKey_Base`]); only the active-key/direction lookup
/// is missing. Signature matches the two-`Process` C comparator.
pub fn Process_compare(p1: &Process, p2: &Process) -> i32 {
    let _ = (p1, p2);
    todo!("port of Process.c:914 — needs Settings/ScreenSettings substrate")
}

/// TODO: port of `int Process_compareByParent(const Row* r1, const Row* r2)`
/// from `Process.c:931`. The group-or-parent prefix is pure, but the
/// tie-break calls [`Process_compare`] (which needs the `Settings`
/// substrate), so the whole function stays stubbed until that lands.
pub fn Process_compareByParent() {
    todo!("port of Process.c:931 — tie-break needs Process_compare (Settings)")
}

/// Port of `int Process_compareByKey_Base(const Process* p1, const
/// Process* p2, ProcessField key)` from `Process.c:943`. The per-field
/// sort comparator: for each column id it compares the corresponding
/// field with `SPACESHIP_NUMBER` / `SPACESHIP_NULLSTR` /
/// `SPACESHIP_DEFAULTSTR` / [`compareRealNumbers`], line-for-line with
/// the C switch.
///
/// The `COMM` case delegates to [`Process_getCommand`] (stubbed — needs
/// the `Settings` substrate), so only that one arm is not yet
/// exercisable; every other arm is pure. The C `default:` (an
/// `assert(0)` "should never be reached" path that still returns a PID
/// compare) maps to the `_` arm.
pub fn Process_compareByKey_Base(p1: &Process, p2: &Process, key: ProcessField) -> i32 {
    match key {
        ProcessField::PERCENT_CPU | ProcessField::PERCENT_NORM_CPU => {
            compareRealNumbers(p1.percent_cpu as f64, p2.percent_cpu as f64)
        }
        ProcessField::PERCENT_MEM => spaceship_number!(p1.m_resident, p2.m_resident),
        ProcessField::COMM => {
            spaceship_nullstr!(Process_getCommand(p1), Process_getCommand(p2))
        }
        ProcessField::PROC_COMM => {
            let comm1: &[u8] = match &p1.procComm {
                Some(c) => c.as_bytes(),
                None => {
                    if Process_isKernelThread(p1) {
                        kthreadID
                    } else {
                        b""
                    }
                }
            };
            let comm2: &[u8] = match &p2.procComm {
                Some(c) => c.as_bytes(),
                None => {
                    if Process_isKernelThread(p2) {
                        kthreadID
                    } else {
                        b""
                    }
                }
            };
            spaceship_nullstr!(Some(comm1), Some(comm2))
        }
        ProcessField::PROC_EXE => {
            let exe1: &[u8] = match &p1.procExe {
                Some(e) => &e.as_bytes()[p1.procExeBasenameOffset..],
                None => {
                    if Process_isKernelThread(p1) {
                        kthreadID
                    } else {
                        b""
                    }
                }
            };
            let exe2: &[u8] = match &p2.procExe {
                Some(e) => &e.as_bytes()[p2.procExeBasenameOffset..],
                None => {
                    if Process_isKernelThread(p2) {
                        kthreadID
                    } else {
                        b""
                    }
                }
            };
            spaceship_nullstr!(Some(exe1), Some(exe2))
        }
        ProcessField::CWD => spaceship_nullstr!(
            p1.procCwd.as_deref().map(str::as_bytes),
            p2.procCwd.as_deref().map(str::as_bytes)
        ),
        ProcessField::ELAPSED => {
            let r = -spaceship_number!(p1.starttime_ctime, p2.starttime_ctime);
            if r != 0 {
                r
            } else {
                spaceship_number!(Process_getPid(p1), Process_getPid(p2))
            }
        }
        ProcessField::MAJFLT => spaceship_number!(p1.majflt, p2.majflt),
        ProcessField::MINFLT => spaceship_number!(p1.minflt, p2.minflt),
        ProcessField::M_RESIDENT => spaceship_number!(p1.m_resident, p2.m_resident),
        ProcessField::M_VIRT => spaceship_number!(p1.m_virt, p2.m_virt),
        ProcessField::NICE => spaceship_number!(p1.nice, p2.nice),
        ProcessField::NLWP => spaceship_number!(p1.nlwp, p2.nlwp),
        ProcessField::PGRP => spaceship_number!(p1.pgrp, p2.pgrp),
        ProcessField::PID => spaceship_number!(Process_getPid(p1), Process_getPid(p2)),
        ProcessField::PPID => spaceship_number!(Process_getParent(p1), Process_getParent(p2)),
        ProcessField::PRIORITY => spaceship_number!(p1.priority, p2.priority),
        ProcessField::PROCESSOR => spaceship_number!(p1.processor, p2.processor),
        ProcessField::SCHEDULERPOLICY => {
            spaceship_number!(p1.scheduling_policy, p2.scheduling_policy)
        }
        ProcessField::SESSION => spaceship_number!(p1.session, p2.session),
        ProcessField::STARTTIME => {
            let r = spaceship_number!(p1.starttime_ctime, p2.starttime_ctime);
            if r != 0 {
                r
            } else {
                spaceship_number!(Process_getPid(p1), Process_getPid(p2))
            }
        }
        ProcessField::STATE => spaceship_number!(p1.state as i32, p2.state as i32),
        ProcessField::ST_UID => spaceship_number!(p1.st_uid, p2.st_uid),
        ProcessField::TIME => spaceship_number!(p1.time, p2.time),
        ProcessField::TGID => {
            spaceship_number!(Process_getThreadGroup(p1), Process_getThreadGroup(p2))
        }
        ProcessField::TPGID => spaceship_number!(p1.tpgid, p2.tpgid),
        ProcessField::TTY => {
            /* Order no tty last */
            spaceship_defaultstr!(
                p1.tty_name.as_deref().map(str::as_bytes),
                p2.tty_name.as_deref().map(str::as_bytes),
                b"\x7f"
            )
        }
        ProcessField::USER => spaceship_nullstr!(
            p1.user.as_deref().map(str::as_bytes),
            p2.user.as_deref().map(str::as_bytes)
        ),
        // C default: `assert(0)` "should never be reached" — returns a
        // PID compare. Reached only for NULL_FIELD or a non-reserved key.
        _ => spaceship_number!(Process_getPid(p1), Process_getPid(p2)),
    }
}

/// Port of `pid_t Process_getPid(const Process* this)` from
/// `Process.h:258`: `(pid_t)this->super.id`.
pub fn Process_getPid(this: &Process) -> i32 {
    this.super_.id
}

/// Port of `pid_t Process_getParent(const Process* this)` from
/// `Process.h:274`: `(pid_t)this->super.parent`.
pub fn Process_getParent(this: &Process) -> i32 {
    this.super_.parent
}

/// Port of `pid_t Process_getThreadGroup(const Process* this)` from
/// `Process.h:266`: `(pid_t)this->super.group`.
pub fn Process_getThreadGroup(this: &Process) -> i32 {
    this.super_.group
}

/// Port of `void Process_setPid(Process* this, pid_t pid)` from
/// `Process.h:254`: `this->super.id = pid`.
pub fn Process_setPid(this: &mut Process, pid: i32) {
    this.super_.id = pid;
}

/// Port of `void Process_setThreadGroup(Process* this, pid_t pid)` from
/// `Process.h:262`: `this->super.group = pid`.
pub fn Process_setThreadGroup(this: &mut Process, pid: i32) {
    this.super_.group = pid;
}

/// Port of `void Process_setParent(Process* this, pid_t pid)` from
/// `Process.h:270`: `this->super.parent = pid`.
pub fn Process_setParent(this: &mut Process, pid: i32) {
    this.super_.parent = pid;
}

/// Port of `bool Process_isKernelThread(const Process* this)` from
/// `Process.h:282`: returns the `isKernelThread` flag.
pub fn Process_isKernelThread(this: &Process) -> bool {
    this.isKernelThread
}

/// Port of `bool Process_isUserlandThread(const Process* this)` from
/// `Process.h:286`: returns the `isUserlandThread` flag.
pub fn Process_isUserlandThread(this: &Process) -> bool {
    this.isUserlandThread
}

/// Port of `bool Process_isThread(const Process* this)` from
/// `Process.h:290`: `Process_isUserlandThread(this) ||
/// Process_isKernelThread(this)`.
pub fn Process_isThread(this: &Process) -> bool {
    Process_isUserlandThread(this) || Process_isKernelThread(this)
}

/// Port of `void Process_updateComm(Process* this, const char* comm)`
/// from `Process.c:1020`. No-op when both the stored `procComm` and the
/// new `comm` are `None` (C `NULL`), or when both are present and equal
/// (`String_eq`, inlined as `==`). Otherwise it replaces `procComm`
/// (`xStrdup(comm)` → an owned `String`, `NULL` → `None`) and resets the
/// merged-command cache marker so the display string is regenerated. The
/// C `free(this->procComm)` is implicit — the old `String` is dropped
/// when the field is overwritten.
pub fn Process_updateComm(this: &mut Process, comm: Option<&str>) {
    if this.procComm.is_none() && comm.is_none() {
        return;
    }

    if let (Some(cur), Some(new)) = (&this.procComm, comm) {
        if cur == new {
            return;
        }
    }

    this.procComm = comm.map(|s| s.to_string());

    this.mergedCommand.lastUpdate = 0;
}

/// Port of `skipPotentialPath(const char* cmdline, size_t end)` from
/// `Process.c:1033`. If `cmdline` starts with `/`, scans up to `end`
/// bytes and returns the offset just past the last `/` that begins a
/// non-empty path component, stopping early at an unescaped space or a
/// `": "` delimiter. Returns 0 when `cmdline` is not an absolute path.
/// NUL-terminated reads are modeled as `0` for any index at or beyond
/// the slice length (the C `cmdline[i + 1]` NUL lookahead).
pub fn skipPotentialPath(cmdline: &[u8], end: usize) -> usize {
    let at = |i: usize| -> u8 {
        if i < cmdline.len() {
            cmdline[i]
        } else {
            0
        }
    };

    if at(0) != b'/' {
        return 0;
    }

    let mut slash = 0;
    let mut i = 1;
    while i < end {
        if at(i) == b'/' && at(i + 1) != 0 {
            slash = i + 1;
            i += 1;
            continue;
        }

        if at(i) == b' ' && at(i - 1) != b'\\' {
            return slash;
        }

        if at(i) == b':' && at(i + 1) == b' ' {
            return slash;
        }

        i += 1;
    }

    slash
}

/// TODO: port of `void Process_updateCmdline(Process* this, const char* cmdline, size_t basenameStart, size_t basenameEnd` from `Process.c:1054`.
pub fn Process_updateCmdline() {
    todo!("port of Process.c:1054")
}

/// TODO: port of `void Process_updateExe(Process* this, const char* exe` from `Process.c:1079`.
pub fn Process_updateExe() {
    todo!("port of Process.c:1079")
}

/// TODO: port of `void Process_updateCPUFieldWidths(float percentage` from `Process.c:1099`.
pub fn Process_updateCPUFieldWidths() {
    todo!("port of Process.c:1099")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_state_char_maps_every_state() {
        assert_eq!(processStateChar(ProcessState::UNKNOWN), '?');
        assert_eq!(processStateChar(ProcessState::RUNNABLE), 'U');
        assert_eq!(processStateChar(ProcessState::RUNNING), 'R');
        assert_eq!(processStateChar(ProcessState::QUEUED), 'Q');
        assert_eq!(processStateChar(ProcessState::WAITING), 'W');
        assert_eq!(processStateChar(ProcessState::UNINTERRUPTIBLE_WAIT), 'D');
        assert_eq!(processStateChar(ProcessState::BLOCKED), 'B');
        assert_eq!(processStateChar(ProcessState::PAGING), 'P');
        assert_eq!(processStateChar(ProcessState::STOPPED), 'T');
        assert_eq!(processStateChar(ProcessState::TRACED), 't');
        assert_eq!(processStateChar(ProcessState::ZOMBIE), 'Z');
        assert_eq!(processStateChar(ProcessState::DEFUNCT), 'X');
        assert_eq!(processStateChar(ProcessState::IDLE), 'I');
        assert_eq!(processStateChar(ProcessState::SLEEPING), 'S');
    }

    #[test]
    fn process_state_discriminants_match_c() {
        // C: UNKNOWN = 1, the rest ascending (Process.h:41).
        assert_eq!(ProcessState::UNKNOWN as u8, 1);
        assert_eq!(ProcessState::SLEEPING as u8, 14);
    }

    #[test]
    fn find_comm_exact_token_match() {
        // Tokens split on '\n' (the C inner loop breaks only on '\n');
        // cmdlineBasenameStart points at 'b' of "bash" (index 9).
        let cmdline = b"/usr/bin/bash\n--login";
        assert_eq!(findCommInCmdline(b"bash", cmdline, 9), Some((9, 4)));
    }

    #[test]
    fn find_comm_resets_basename_after_slash() {
        // Starting the scan before a slash: tokenBase resets past '/'.
        let cmdline = b"/usr/bin/bash";
        assert_eq!(findCommInCmdline(b"bash", cmdline, 0), Some((9, 4)));
    }

    #[test]
    fn find_comm_no_match_returns_none() {
        let cmdline = b"/usr/bin/zsh";
        assert_eq!(findCommInCmdline(b"bash", cmdline, 0), None);
        // empty cmdline: loop never enters.
        assert_eq!(findCommInCmdline(b"bash", b"", 0), None);
    }

    #[test]
    fn find_comm_truncated_comm_allows_longer_token() {
        // commLen == TASK_COMM_LEN - 1 (15): a longer token still matches
        // on its 15-char prefix.
        let comm = b"012345678901234"; // 15 bytes
        assert_eq!(comm.len(), TASK_COMM_LEN - 1);
        let cmdline = b"0123456789012345678"; // 19 bytes, prefix matches
        assert_eq!(findCommInCmdline(comm, cmdline, 0), Some((0, 19)));
        // With a comm of non-max length, a longer token must NOT match.
        assert_eq!(findCommInCmdline(b"0123", b"01234567", 0), None);
    }

    #[test]
    fn find_comm_skips_consecutive_newlines() {
        // tokens split on '\n'; multiple newlines are collapsed.
        let cmdline = b"foo\n\n\nbar";
        assert_eq!(findCommInCmdline(b"bar", cmdline, 0), Some((6, 3)));
    }

    #[test]
    fn skip_potential_path_non_absolute_returns_zero() {
        assert_eq!(skipPotentialPath(b"bash --login", 12), 0);
        assert_eq!(skipPotentialPath(b"", 0), 0);
    }

    #[test]
    fn skip_potential_path_returns_after_last_slash() {
        // "/usr/bin/bash" -> offset just past the last '/' (index 9).
        let c = b"/usr/bin/bash";
        assert_eq!(skipPotentialPath(c, c.len()), 9);
    }

    #[test]
    fn skip_potential_path_stops_at_unescaped_space() {
        // "/usr/bin/bash --login": scanning stops at the space; the last
        // component slash was at index 9.
        let c = b"/usr/bin/bash --login";
        assert_eq!(skipPotentialPath(c, c.len()), 9);
    }

    #[test]
    fn skip_potential_path_escaped_space_does_not_stop() {
        // Escaped space (preceded by '\\') is not a delimiter, so the
        // scan continues past it to the final "/d" component (slash = 8).
        let c = b"/a/b\\ c/d";
        assert_eq!(skipPotentialPath(c, c.len()), 8);
    }

    #[test]
    fn skip_potential_path_stops_at_colon_space() {
        // ": " delimiter stops the scan.
        let c = b"/usr/bin/foo: bar";
        assert_eq!(skipPotentialPath(c, c.len()), 9);
    }

    #[test]
    fn skip_potential_path_trailing_slash_not_counted() {
        // A '/' whose next byte is NUL (end of slice) does not advance
        // slash: cmdline[i + 1] != '\0' guard fails.
        let c = b"/usr/bin/";
        assert_eq!(skipPotentialPath(c, c.len()), 5);
    }

    #[test]
    fn match_exe_absolute_path_full_match() {
        // exe = "/usr/bin/bash", exeBaseOffset = 9 ("bash" at 9),
        // exeBaseLen = 4. Absolute cmdline must match the whole exe.
        let exe = b"/usr/bin/bash";
        let cmdline = b"/usr/bin/bash --login";
        let (matchLen, base) = matchCmdlinePrefixWithExeSuffix(cmdline, 9, exe, 9, 4);
        assert_eq!(matchLen, 13); // exeBaseLen + exeBaseOffset
        assert_eq!(base, 9); // unchanged on absolute path
    }

    #[test]
    fn match_exe_absolute_path_no_match() {
        let exe = b"/usr/bin/bash";
        let cmdline = b"/usr/bin/zsh";
        let (matchLen, base) = matchCmdlinePrefixWithExeSuffix(cmdline, 9, exe, 9, 3);
        assert_eq!(matchLen, 0);
        assert_eq!(base, 9);
    }

    #[test]
    fn match_exe_absolute_bad_delimiter() {
        // cmdline continues the basename past the matched prefix with a
        // non-delimiter char, so the match is rejected.
        let exe = b"/usr/bin/bash";
        let cmdline = b"/usr/bin/bashx";
        let (matchLen, _) = matchCmdlinePrefixWithExeSuffix(cmdline, 9, exe, 9, 4);
        assert_eq!(matchLen, 0);
    }

    #[test]
    fn match_exe_relative_path_reverse_match() {
        // exe = "/usr/bin/bash" (basename "bash" at offset 9), cmdline is
        // the relative "bin/bash" with basename "bash" at offset 4. The
        // reverse match walks "bin/" back to exe's "/bin/".
        let exe = b"/usr/bin/bash";
        let cmdline = b"bin/bash";
        let (matchLen, base) = matchCmdlinePrefixWithExeSuffix(cmdline, 4, exe, 9, 4);
        assert_eq!(matchLen, 8); // exeBaseLen(4) + cmdlineBaseOffset(4)
        assert_eq!(base, 4);
    }

    #[test]
    fn match_exe_relative_no_match() {
        let exe = b"/usr/bin/bash";
        let cmdline = b"bin/zsh";
        let (matchLen, base) = matchCmdlinePrefixWithExeSuffix(cmdline, 4, exe, 9, 3);
        assert_eq!(matchLen, 0);
        assert_eq!(base, 4); // in-param passes through unchanged
    }

    // ── Process data model: struct, predicates, getters, compare ──────

    #[test]
    fn tristate_discriminants_match_c() {
        assert_eq!(Tristate::TRI_OFF as i8, -1);
        assert_eq!(Tristate::TRI_INITIAL as i8, 0);
        assert_eq!(Tristate::TRI_ON as i8, 1);
        assert_eq!(Tristate::default(), Tristate::TRI_INITIAL);
    }

    #[test]
    fn process_field_discriminants_match_c() {
        // A few spot checks against RowField.h ReservedFields values.
        assert_eq!(ProcessField::PID as i32, 1);
        assert_eq!(ProcessField::COMM as i32, 2);
        assert_eq!(ProcessField::ST_UID as i32, 46);
        assert_eq!(ProcessField::PROC_COMM as i32, 124);
        assert_eq!(ProcessField::CWD as i32, 126);
    }

    #[test]
    fn process_default_state_is_unknown() {
        // C zero-init would give an invalid 0; our Default picks UNKNOWN.
        let p = Process::default();
        assert_eq!(p.state, ProcessState::UNKNOWN);
    }

    #[test]
    fn process_init_runs_row_init_and_sets_defaults() {
        let mut p = Process::default();
        let host = 0xBEEF_usize as *const c_void;
        Process_init(&mut p, host);
        // Process-specific defaults from the C body.
        assert_eq!(p.st_uid, u32::MAX); // (uid_t)-1
        assert_eq!(p.cmdlineBasenameEnd, 0);
        // Row_init ran on the embedded base.
        assert_eq!(p.super_.host, host);
        assert!(p.super_.show);
        assert!(p.super_.showChildren);
    }

    // Getters/setters route through the embedded Row (super).
    #[test]
    fn pid_parent_tgid_getters_and_setters() {
        let mut p = Process::default();
        Process_setPid(&mut p, 4321);
        Process_setParent(&mut p, 1);
        Process_setThreadGroup(&mut p, 4000);
        assert_eq!(Process_getPid(&p), 4321);
        assert_eq!(Process_getParent(&p), 1);
        assert_eq!(Process_getThreadGroup(&p), 4000);
        // They map to the exact Row fields.
        assert_eq!(p.super_.id, 4321);
        assert_eq!(p.super_.parent, 1);
        assert_eq!(p.super_.group, 4000);
    }

    #[test]
    fn thread_predicates() {
        let mut p = Process::default();
        assert!(!Process_isKernelThread(&p));
        assert!(!Process_isUserlandThread(&p));
        assert!(!Process_isThread(&p));

        p.isKernelThread = true;
        assert!(Process_isKernelThread(&p));
        assert!(Process_isThread(&p)); // kernel => thread

        p.isKernelThread = false;
        p.isUserlandThread = true;
        assert!(Process_isUserlandThread(&p));
        assert!(Process_isThread(&p)); // userland => thread
    }

    // Comparison helpers.
    fn proc() -> Process {
        Process::default()
    }

    #[test]
    fn compare_pid_numeric() {
        let mut a = proc();
        let mut b = proc();
        Process_setPid(&mut a, 100);
        Process_setPid(&mut b, 200);
        assert_eq!(Process_compareByKey_Base(&a, &b, ProcessField::PID), -1);
        assert_eq!(Process_compareByKey_Base(&b, &a, ProcessField::PID), 1);
        assert_eq!(Process_compareByKey_Base(&a, &a, ProcessField::PID), 0);
    }

    #[test]
    fn compare_percent_cpu_float_and_nan() {
        let mut a = proc();
        let mut b = proc();
        a.percent_cpu = 1.0;
        b.percent_cpu = 2.0;
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PERCENT_CPU),
            -1
        );
        // PERCENT_NORM_CPU compares the same field.
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PERCENT_NORM_CPU),
            -1
        );
        // NaN is ordered less than any value (compareRealNumbers).
        a.percent_cpu = f32::NAN;
        b.percent_cpu = 1.0;
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PERCENT_CPU),
            -1
        );
    }

    #[test]
    fn compare_percent_mem_uses_resident_set() {
        // C compares m_resident for PERCENT_MEM, not percent_mem.
        let mut a = proc();
        let mut b = proc();
        a.m_resident = 500;
        b.m_resident = 1000;
        a.percent_mem = 99.0; // must be ignored
        b.percent_mem = 1.0;
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PERCENT_MEM),
            -1
        );
    }

    #[test]
    fn compare_state_by_enum_order() {
        let mut a = proc();
        let mut b = proc();
        a.state = ProcessState::RUNNING; // 3
        b.state = ProcessState::SLEEPING; // 14
        assert_eq!(Process_compareByKey_Base(&a, &b, ProcessField::STATE), -1);
        assert_eq!(Process_compareByKey_Base(&a, &a, ProcessField::STATE), 0);
    }

    #[test]
    fn compare_proc_comm_string() {
        let mut a = proc();
        let mut b = proc();
        a.procComm = Some("alpha".to_string());
        b.procComm = Some("beta".to_string());
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PROC_COMM),
            -1
        );
    }

    #[test]
    fn compare_proc_comm_kthread_fallback() {
        // No procComm + kernel thread => uses kthreadID "KTHREAD".
        let mut a = proc();
        a.isKernelThread = true; // procComm None => "KTHREAD"
        let mut b = proc();
        b.procComm = Some("aaa".to_string());
        // "KTHREAD" (K=0x4B) < "aaa" (a=0x61) => -1.
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PROC_COMM),
            -1
        );
        // Non-kernel with no procComm => "" which sorts before "aaa".
        let c = proc(); // not kernel, procComm None => ""
        assert_eq!(
            Process_compareByKey_Base(&c, &b, ProcessField::PROC_COMM),
            -1
        );
    }

    #[test]
    fn compare_starttime_tie_breaks_on_pid() {
        let mut a = proc();
        let mut b = proc();
        a.starttime_ctime = 100;
        b.starttime_ctime = 100;
        Process_setPid(&mut a, 5);
        Process_setPid(&mut b, 9);
        // Equal starttime => tie-break by pid: 5 < 9 => -1.
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::STARTTIME),
            -1
        );
        // Distinct starttime dominates.
        b.starttime_ctime = 200;
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::STARTTIME),
            -1
        );
    }

    #[test]
    fn compare_elapsed_negates_starttime() {
        // ELAPSED: r = -SPACESHIP(starttime); later start => less elapsed.
        let mut a = proc();
        let mut b = proc();
        a.starttime_ctime = 200; // started later => smaller elapsed
        b.starttime_ctime = 100;
        // SPACESHIP(200,100)=1 => r=-1.
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::ELAPSED),
            -1
        );
        // Equal starttime => tie-break by pid.
        b.starttime_ctime = 200;
        Process_setPid(&mut a, 3);
        Process_setPid(&mut b, 8);
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::ELAPSED),
            -1
        );
    }

    #[test]
    fn compare_tty_orders_no_tty_last() {
        // TTY uses SPACESHIP_DEFAULTSTR(..., "\x7f"): a missing tty
        // defaults to 0x7F so it sorts after any real tty name.
        let mut a = proc();
        a.tty_name = Some("tty1".to_string());
        let b = proc(); // no tty_name => "\x7f"
        // "tty1" (t=0x74) < "\x7f" => real tty sorts first.
        assert_eq!(Process_compareByKey_Base(&a, &b, ProcessField::TTY), -1);
        // Two missing ttys compare equal (both "\x7f").
        let c = proc();
        assert_eq!(Process_compareByKey_Base(&b, &c, ProcessField::TTY), 0);
    }

    #[test]
    fn compare_user_nullstr() {
        let mut a = proc();
        let mut b = proc();
        a.user = Some("alice".to_string());
        b.user = Some("bob".to_string());
        assert_eq!(Process_compareByKey_Base(&a, &b, ProcessField::USER), -1);
        // NULL user compares as "" (sorts first).
        let c = proc();
        assert_eq!(Process_compareByKey_Base(&c, &a, ProcessField::USER), -1);
    }

    #[test]
    fn compare_default_arm_falls_back_to_pid() {
        // NULL_FIELD hits the `_` (C default/assert) arm => pid compare.
        let mut a = proc();
        let mut b = proc();
        Process_setPid(&mut a, 1);
        Process_setPid(&mut b, 2);
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::NULL_FIELD),
            -1
        );
    }

    #[test]
    fn process_klass_chain_extends_row() {
        // Process_class -> Row_class -> Object_class.
        let p = proc();
        assert!(core::ptr::eq(p.klass(), &Process_class));
        assert!(crate::ported::object::Object_isA(
            Some(&p as &dyn Object),
            &Process_class
        ));
        assert!(crate::ported::object::Object_isA(
            Some(&p as &dyn Object),
            &Row_class
        ));
    }
}
