//! Port of `Process.c` + `Process.h` — the process data model, its pure
//! comparison / predicate logic, and the column-rendering / merged-command
//! layer. The `Machine` / `Settings` substrate the render and compare paths
//! read is now modeled (`Row::host` is dereferenced back to the owning
//! [`Machine`], whose `settings` carry the flags these functions consume),
//! so the module has no remaining `todo!()` stubs.
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
//! [`matchCmdlinePrefixWithExeSuffix`], [`skipPotentialPath`], the
//! field-mutators [`Process_updateComm`] / [`Process_updateExe`] /
//! [`Process_updateCmdline`], and the syscall actions
//! [`Process_setPriority`] / [`Process_rowChangePriorityBy`] /
//! [`Process_sendSignal`] / [`Process_rowSendSignal`] (POSIX
//! `getpriority`/`setpriority`/`kill`, unguarded by cfg). C `const
//! char*` + `size_t` helpers are modeled on `&[u8]` + `usize`;
//! NUL-terminated reads treat any index at/after the slice length as the
//! terminating NUL (`0`). Out-params are returned as tuples/`Option`.
//!
//! Ported render / compare / filter layer (reads the modeled `Settings`
//! via the `Row::host as *const Machine` back-pointer, as in C):
//! [`Process_writeField`] (the per-column value writer switch),
//! [`Process_writeCommand`] + [`Process_makeCommandStr`] (the merged
//! command builder with the `CMDLINE_HIGHLIGHT_FLAG_*` regions and
//! `TREE_STR_VERT` separator), [`Process_getCommand`] /
//! [`Process_getSortKey`] (drive the `COMM` sort case),
//! [`Process_compare`] (active sort key/direction via `settings->ss`) and
//! its [`Process_compareByParent`] tree-mode tie-break,
//! [`Process_matchesFilter`], and [`Process_isHighlighted`].
//! `gen_port_report.py` counts `todo!()` bodies as *stubbed*, not
//! *ported*, so scaffolding does not inflate coverage.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements as CE, ColorScheme, TreeStr};
use crate::ported::dynamiccolumn::DynamicColumn_writeField;
use crate::ported::hashtable::Hashtable_get;
use crate::ported::machine::Machine;
use crate::ported::object::{Arg, Object, ObjectClass, Object_isA};
use crate::ported::processtable::ProcessTable;
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendWide, RichString_setAttrn, RichString_size,
};
use crate::ported::row::{
    spaceship_number, PercentageAttr, Row, RowClass, Row_class, Row_display, Row_fieldWidths,
    Row_getGroupOrParent, Row_init, Row_pidDigits, Row_printCount, Row_printKBytes,
    Row_printLeftAlignedField, Row_printPercentage, Row_printTime, Row_uidDigits,
    Row_updateFieldWidth,
};
use crate::ported::scheduling::Scheduling_formatPolicy;
use crate::ported::settings::{
    RowField, ScreenSettings_getActiveDirection, ScreenSettings_getActiveSortKey, Settings,
    Settings_isReadonly,
};
use crate::ported::table::Table;
use crate::ported::xutils::{compareRealNumbers, String_contains_i, String_startsWith};
use core::ffi::c_void;
use core::ops::Deref;
use std::sync::atomic::Ordering;

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
pub(crate) use spaceship_nullstr;

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

/// Port of `#define PROCESS_FLAG_IO 0x00000001` from `Process.h:22` — the
/// per-platform scan flag requesting the process I/O counters.
pub const PROCESS_FLAG_IO: u32 = 0x00000001;
/// Port of `#define PROCESS_FLAG_CWD 0x00000002` from `Process.h:23` — the
/// scan flag requesting the working directory.
pub const PROCESS_FLAG_CWD: u32 = 0x00000002;
/// Port of `#define PROCESS_FLAG_SCHEDPOL 0x00000004` from `Process.h:24` —
/// the scan flag requesting the scheduling policy.
pub const PROCESS_FLAG_SCHEDPOL: u32 = 0x00000004;

/// Port of `#define DEFAULT_HIGHLIGHT_SECS 5` from `Process.h:26` — how
/// long a newly-changed value stays visually highlighted.
pub const DEFAULT_HIGHLIGHT_SECS: i32 = 5;

/// Port of `#define PROCESS_NICE_UNKNOWN (-INT_MAX)` from `Process.h:29` —
/// sentinel niceness for a process whose value could not be read.
pub const PROCESS_NICE_UNKNOWN: i32 = -i32::MAX;

/// Port of the `CMDLINE_HIGHLIGHT_FLAG_*` selective-highlight flags from
/// `Process.h:294`. Stored in [`ProcessCmdlineHighlight::flags`] by
/// `Process_makeCommandStr` and consulted by `Process_writeCommand` to
/// decide which regions of the merged command to color.
pub const CMDLINE_HIGHLIGHT_FLAG_SEPARATOR: i32 = 0x00000001;
/// Port of `#define CMDLINE_HIGHLIGHT_FLAG_BASENAME` from `Process.h:295`.
pub const CMDLINE_HIGHLIGHT_FLAG_BASENAME: i32 = 0x00000002;
/// Port of `#define CMDLINE_HIGHLIGHT_FLAG_COMM` from `Process.h:296`.
pub const CMDLINE_HIGHLIGHT_FLAG_COMM: i32 = 0x00000004;
/// Port of `#define CMDLINE_HIGHLIGHT_FLAG_DELETED` from `Process.h:297`.
pub const CMDLINE_HIGHLIGHT_FLAG_DELETED: i32 = 0x00000008;
/// Port of `#define CMDLINE_HIGHLIGHT_FLAG_PREFIXDIR` from `Process.h:298`.
pub const CMDLINE_HIGHLIGHT_FLAG_PREFIXDIR: i32 = 0x00000010;

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
/// by [`Process_makeCommandStr`] with the merged Command string and the
/// highlight regions [`Process_writeCommand`] uses to color it.
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

    // ── Linux platform fields, spliced in by the C `PLATFORM_PROCESS_FIELDS`
    // macro from `linux/ProcessField.h`. In the C build these share the one
    // `ReservedFields` enum with the generic fields above; htoprs targets the
    // Linux platform, so they belong on this shared enum too. The explicit
    // discriminants are verbatim from `linux/ProcessField.h`.
    CMINFLT = 11,
    CMAJFLT = 13,
    UTIME = 14,
    STIME = 15,
    CUTIME = 16,
    CSTIME = 17,
    M_SHARE = 41,
    M_TRS = 42,
    M_DRS = 43,
    M_LRS = 44,
    CTID = 100,
    VPID = 101,
    VXID = 102,
    RCHAR = 103,
    WCHAR = 104,
    SYSCR = 105,
    SYSCW = 106,
    RBYTES = 107,
    WBYTES = 108,
    CNCLWB = 109,
    IO_READ_RATE = 110,
    IO_WRITE_RATE = 111,
    IO_RATE = 112,
    CGROUP = 113,
    OOM = 114,
    IO_PRIORITY = 115,
    PERCENT_CPU_DELAY = 116,
    PERCENT_IO_DELAY = 117,
    PERCENT_SWAP_DELAY = 118,
    M_PSS = 119,
    M_SWAP = 120,
    M_PSSWP = 121,
    CTXT = 122,
    SECATTR = 123,
    AUTOGROUP_ID = 127,
    AUTOGROUP_NICE = 128,
    CCGROUP = 129,
    CONTAINER = 130,
    M_PRIV = 131,
    GPU_TIME = 132,
    GPU_PERCENT = 133,
    ISCONTAINER = 134,
}

/// Port of `struct ProcessFieldData_` (`Process.h:203`) — the per-field
/// metadata table entry describing how a [`ProcessField`] is named,
/// titled, scanned, and sorted. The C `const char*` slots that can be
/// `NULL` (`title`, `description`) become `Option<&'static str>`; `name`
/// is `&'static str` (the C table always gives it a value, `""` for the
/// unused index 0).
#[derive(Debug, Clone, Copy)]
#[allow(non_snake_case)]
pub struct ProcessFieldData {
    /// C `const char* name` — displayed in the setup menu.
    pub name: &'static str,
    /// C `const char* title` — column header on the main screen; must be
    /// the same visible width as the printed values.
    pub title: Option<&'static str>,
    /// C `const char* description` — help text in the setup menu.
    pub description: Option<&'static str>,
    /// C `uint32_t flags` — scan flag enabling an otherwise-skipped
    /// scan method (`PROCESS_FLAG_*`).
    pub flags: u32,
    /// C `bool pidColumn` — values are process identifiers (widens the
    /// column to the max-pid width).
    pub pidColumn: bool,
    /// C `bool defaultSortDesc` — sort descending by default.
    pub defaultSortDesc: bool,
    /// C `bool autoWidth` — column width auto-adjusts (min width = title
    /// length).
    pub autoWidth: bool,
    /// C `bool autoTitleRightAlign` — right-align an auto-width title
    /// (default is left).
    pub autoTitleRightAlign: bool,
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

/// Port of `typedef int (*Process_CompareByKey)(const Process*, const
/// Process*, ProcessField)` (`Process.h:242`). The C `const Process*`
/// receivers are `&dyn Object` here (downcast by the slot).
pub type Process_CompareByKey = fn(&dyn Object, &dyn Object, RowField) -> i32;

/// Port of `typedef struct ProcessClass_` (`Process.h:244`) — the `Process`
/// vtable, embedding [`RowClass`] (`super_`) and adding the `compareByKey`
/// slot. `Deref<Target = ObjectClass>` (through the embedded `RowClass`) lets
/// a `&ProcessClass` coerce to `&ObjectClass` for the class-identity API.
pub struct ProcessClass {
    pub super_: RowClass,
    pub compareByKey: Option<Process_CompareByKey>,
}

impl Deref for ProcessClass {
    type Target = ObjectClass;
    fn deref(&self) -> &ObjectClass {
        &self.super_.super_
    }
}

/// Port of `const ProcessClass Process_class` from `Process.c:1113`. The
/// `RowClass` vtable wires `isHighlighted`, `isVisible`, `writeField`,
/// `sortKeyString`, `compareByParent`, and `matchesFilter`; `compareByKey`
/// is `None` on the base `Process` (only `LinuxProcess` sets
/// it). `.compare = Process_compare` and `.delete` are realized by the
/// [`Object`] impl / `Drop`.
pub static Process_class: ProcessClass = ProcessClass {
    super_: RowClass {
        super_: ObjectClass {
            extends: Some(&Row_class.super_),
        },
        isHighlighted: Some(Process_rowIsHighlighted),
        isVisible: Some(Process_rowIsVisible),
        writeField: Some(Process_rowWriteField),
        matchesFilter: Some(Process_rowMatchesFilter),
        sortKeyString: Some(Process_rowGetSortKey),
        compareByParent: Some(Process_compareByParent),
    },
    compareByKey: None,
};

impl Object for Process {
    /// C `this->super.super.klass` — the embedded [`ObjectClass`] of the
    /// [`ProcessClass`] vtable.
    fn klass(&self) -> &'static ObjectClass {
        &Process_class.super_.super_
    }

    /// C `As_Row(this)` — `Process`'s [`RowClass`] vtable.
    fn row_class(&self) -> Option<&'static RowClass> {
        Some(&Process_class.super_)
    }

    /// C `(const Row*)this` — the embedded base of a `Process`.
    fn as_row(&self) -> Option<&Row> {
        Some(&self.super_)
    }

    /// C `(const Process*)this` — a `Process` is its own embedded `Process`.
    fn as_process(&self) -> Option<&Process> {
        Some(self)
    }

    /// Mutable view of the embedded `Row` base.
    fn as_row_mut(&mut self) -> Option<&mut Row> {
        Some(&mut self.super_)
    }

    /// Mutable view of self as a `Process`.
    fn as_process_mut(&mut self) -> Option<&mut Process> {
        Some(self)
    }

    /// C `As_Process(this)` — `Process`'s [`ProcessClass`] vtable.
    fn process_class(&self) -> Option<&'static ProcessClass> {
        Some(&Process_class)
    }

    /// C `Process_class.super.super.display = Row_display`.
    fn display(&self, out: &mut RichString) {
        Row_display(self, out)
    }

    /// C `Process_class.super.super.compare = Process_compare`. Delegates to
    /// [`Process_compare`], which reads the sort key from the host settings
    /// and dispatches the concrete `compareByKey` slot.
    fn compare(&self, other: &dyn Object) -> i32 {
        Process_compare(self, other)
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

/// Port of `void Process_fillStarttimeBuffer(Process* this)` from
/// `Process.c:43`. Formats `starttime_ctime` into the cached
/// `starttime_show` via `localtime_r` + `strftime`, choosing the format by
/// age relative to `now`: `%R` (HH:MM) within a day, `%b%d` within a year,
/// else `%Y`.
///
/// C reads `now` from `this->super.host->realtime.tv_sec`. The ported
/// [`Machine`] tracks `realtimeMs` from the same clock, and
/// `tv_sec == realtimeMs / 1000` exactly, so `now` is derived from it —
/// avoiding a redundant `timeval` field. `Row::host` (a `*const c_void`) is
/// cast back to the `*const Machine` it points at in the live graph.
pub fn Process_fillStarttimeBuffer(this: &mut Process) {
    // now = this->super.host->realtime.tv_sec
    let host = this.super_.host as *const Machine;
    let now = unsafe { ((*host).realtimeMs / 1000) as i64 };

    let ctime = this.starttime_ctime as libc::time_t;
    let mut date: libc::tm = unsafe { core::mem::zeroed() };
    unsafe {
        libc::localtime_r(&ctime, &mut date);
    }

    // NUL-terminated strftime format strings (same thresholds as the C).
    let fmt: &[u8] = if this.starttime_ctime > now - 86400 {
        b"%R \0"
    } else if this.starttime_ctime > now - 364 * 86400 {
        b"%b%d \0"
    } else {
        b" %Y \0"
    };

    unsafe {
        libc::strftime(
            this.starttime_show.as_mut_ptr() as *mut libc::c_char,
            this.starttime_show.len() - 1, // C: sizeof(starttime_show) - 1
            fmt.as_ptr() as *const libc::c_char,
            &date,
        );
    }
}

/// Port of `static inline char* stpcpyWithNewlineConversion(char* dstStr,
/// const char* srcStr)` from `Process.c:169`. Copies `src` into `dst`,
/// converting each `'\n'` to `' '`. The C variant writes a terminating
/// NUL and returns the pointer just past the copied bytes (stpcpy
/// semantics) so callers can chain; a `Vec<u8>` tracks its own end and
/// carries no NUL, so the append leaves `dst` extended in place and no
/// end pointer is needed. Only reachable from
/// [`Process_makeCommandStr`], as in C.
pub fn stpcpyWithNewlineConversion(dst: &mut Vec<u8>, src: &[u8]) {
    for &c in src {
        dst.push(if c == b'\n' { b' ' } else { c });
    }
}

/// Port of `void Process_makeCommandStr(Process* this, const
/// Settings* settings)` from `Process.c:183`. Rebuilds the cached
/// merged-command string in `this->mergedCommand`: every branch is driven
/// by a `Settings` flag (`showMergedCommand`, `showProgramPath`,
/// `findCommInCmdline`, `stripExeFromCmdline`, `showThreadNames`,
/// `shadowDistPathPrefix` — `Process.c:186-191` — and the `lastUpdate`
/// cache-invalidation stamp, `Process.c:193`), all of which the modeled
/// [`Settings`] now carries. The field separator is
/// `CRT_treeStr[TREE_STR_VERT]` ([`TreeStr::TREE_STR_VERT`],
/// `Process.c:213`); highlight regions use the `CMDLINE_HIGHLIGHT_FLAG_*`
/// constants + the `CRT_colors[...]` palette (via [`ColorScheme`],
/// `Process.c:307-310`). Consumes the pure Process-field inputs (`cmdline`,
/// `procComm`, `procExe`, `cmdlineBasenameStart/End`,
/// `procExeBasenameOffset`, `procExeDeleted`, `usesDeletedLib`, `state`)
/// and the string helpers [`stpcpyWithNewlineConversion`],
/// [`findCommInCmdline`], [`matchCmdlinePrefixWithExeSuffix`].
pub fn Process_makeCommandStr(this: &mut Process, settings: &Settings) {
    let show_merged_command = settings.showMergedCommand;
    let show_program_path = settings.showProgramPath;
    let search_comm_in_cmdline = settings.findCommInCmdline;
    let strip_exe_from_cmdline = settings.stripExeFromCmdline;
    let show_thread_names = settings.showThreadNames;
    let shadow_dist_path_prefix = settings.shadowDistPathPrefix;
    let settings_stamp = settings.lastUpdate;

    // Nothing to (re)generate for: a kernel thread; a zombie from before htop
    // was watching; or a cache still current for this settings stamp.
    if Process_isKernelThread(this) {
        return;
    }
    if this.state == ProcessState::ZOMBIE && this.mergedCommand.str.is_none() {
        return;
    }
    if this.mergedCommand.lastUpdate >= settings_stamp {
        return;
    }
    this.mergedCommand.lastUpdate = settings_stamp;

    // Everything derived from `this`, computed up front so the build loop
    // below touches only locals (no aliasing with `this.mergedCommand`).
    let is_thread = Process_isThread(this);
    let is_userland_thread = Process_isUserlandThread(this);
    let scheme = ColorScheme::active();
    let base_attr = if is_thread {
        CE::PROCESS_THREAD_BASENAME
    } else {
        CE::PROCESS_BASENAME
    }
    .packed(scheme);
    let comm_attr = if is_thread {
        CE::PROCESS_THREAD_COMM
    } else {
        CE::PROCESS_COMM
    }
    .packed(scheme);
    let del_exe_attr = CE::FAILED_READ.packed(scheme);
    let del_lib_attr = CE::PROCESS_TAG.packed(scheme);
    let shadow_attr = CE::PROCESS_SHADOW.packed(scheme);
    let sep_attr = CE::FAILED_READ.packed(scheme);
    let proc_exe_deleted = this.procExeDeleted;
    let uses_deleted_lib = this.usesDeletedLib;
    let proc_exe_basename_offset = this.procExeBasenameOffset;
    let cmdline_basename_end = this.cmdlineBasenameEnd;

    // Owned copies of the three source strings (byte slices, no trailing NUL).
    let cmdline_owned: Option<Vec<u8>> = this.cmdline.as_ref().map(|s| s.as_bytes().to_vec());
    let proc_comm: Option<&[u8]> = this.procComm.as_deref().map(str::as_bytes);
    let proc_exe: Option<&[u8]> = this.procExe.as_deref().map(str::as_bytes);
    // Bind proc_comm/proc_exe to owned copies to avoid borrowing `this` while
    // we later take `&mut this.mergedCommand`.
    let proc_comm: Option<Vec<u8>> = proc_comm.map(|s| s.to_vec());
    let proc_exe: Option<Vec<u8>> = proc_exe.map(|s| s.to_vec());

    // The field separator "│" (`TREE_STR_VERT`) — chosen to never match a
    // valid search/filter string. Its byte length differs from its 1-column
    // display width; `mb_mismatch` tracks the running difference so highlight
    // offsets stay in display-column units, as in C.
    let separator = TreeStr::TREE_STR_VERT.glyph();
    let separator_len = separator.len();

    // Working state (the C `str`/`strStart` buffer + highlight table).
    let mut buf: Vec<u8> = Vec::new();
    let mut highlights: Vec<ProcessCmdlineHighlight> = Vec::new();
    let mut mb_mismatch: usize = 0;

    // C `WRITE_HIGHLIGHT(offset, length, attr, flags)` — record a highlight at
    // the current buffer position; offsets are in display columns.
    // Local (depth > 0, so exempt from the port-purity fn gate) commit helper,
    // mirroring C's final state: reset the highlight table, then store the
    // count and the built string.
    fn commit(this: &mut Process, buf: &[u8], highlights: &[ProcessCmdlineHighlight]) {
        let mc = &mut this.mergedCommand;
        for h in mc.highlights.iter_mut() {
            *h = ProcessCmdlineHighlight::default();
        }
        mc.highlightCount = highlights.len();
        for (i, h) in highlights.iter().enumerate() {
            mc.highlights[i] = *h;
        }
        mc.str = Some(String::from_utf8_lossy(buf).into_owned());
    }

    macro_rules! write_highlight {
        ($offset:expr, $length:expr, $attr:expr, $flags:expr) => {{
            // C `ARRAYSIZE(mc->highlights)` == 8.
            if highlights.len() < 8 {
                highlights.push(ProcessCmdlineHighlight {
                    offset: buf.len() + $offset - mb_mismatch,
                    length: $length,
                    attr: $attr,
                    flags: $flags,
                });
            }
        }};
    }
    // C `WRITE_SEPARATOR`.
    macro_rules! write_separator {
        () => {{
            write_highlight!(0, 1, sep_attr, CMDLINE_HIGHLIGHT_FLAG_SEPARATOR);
            mb_mismatch += separator_len - 1;
            buf.extend_from_slice(separator.as_bytes());
        }};
    }

    // C `CHECK_AND_MARK_DIST_PATH_PREFIXES` — the matched distribution path
    // prefix length (first match wins; the prefixes are mutually exclusive, so
    // a flat scan matches the C `switch`). `None` = no prefix to shadow.
    let dist_prefix_len = |s: &[u8]| -> Option<usize> {
        const PREFIXES: &[&[u8]] = &[
            b"/bin/",
            b"/lib/",
            b"/lib32/",
            b"/lib64/",
            b"/libx32/",
            b"/sbin/",
            b"/usr/bin/",
            b"/usr/libexec/",
            b"/usr/lib/",
            b"/usr/lib32/",
            b"/usr/lib64/",
            b"/usr/libx32/",
            b"/usr/local/bin/",
            b"/usr/local/lib/",
            b"/usr/local/sbin/",
            b"/usr/sbin/",
            b"/nix/store/",
            b"/run/current-system/",
        ];
        PREFIXES.iter().find(|p| s.starts_with(p)).map(|p| p.len())
    };

    // Shortcuts to the source strings as slices ("(zombie)" fallback below).
    let cmdline_present = cmdline_owned.is_some();
    let mut cmdline: &[u8] = match &cmdline_owned {
        Some(c) => c,
        None => b"(zombie)",
    };
    let proc_comm_s: Option<&[u8]> = proc_comm.as_deref();
    let proc_exe_s: Option<&[u8]> = proc_exe.as_deref();

    let mut cmdline_basename_start = if cmdline_present {
        this.cmdlineBasenameStart
    } else {
        0
    };
    let mut cmdline_basename_len =
        if cmdline_present && cmdline_basename_end > cmdline_basename_start {
            cmdline_basename_end - cmdline_basename_start
        } else {
            0
        };

    // Exe / cmdline prefix matching (mirrors the C block).
    let mut match_len = 0usize;
    let mut exe_basename_offset = 0usize;
    let mut exe_basename_len = 0usize;
    if let Some(pe) = proc_exe_s {
        exe_basename_offset = proc_exe_basename_offset;
        exe_basename_len = pe.len() - exe_basename_offset;

        if cmdline_present {
            let (ml, new_start) = matchCmdlinePrefixWithExeSuffix(
                cmdline,
                cmdline_basename_start,
                pe,
                exe_basename_offset,
                exe_basename_len,
            );
            match_len = ml;
            cmdline_basename_start = new_start;
        }
        if match_len != 0 {
            cmdline_basename_len = exe_basename_len;
        }
    }

    // ── Fallback to cmdline (no merged command available) ──────────────────
    if !show_merged_command || proc_exe_s.is_none() || proc_comm_s.is_none() {
        if (show_merged_command || (is_userland_thread && show_thread_names))
            && proc_comm_s.is_some_and(|c| !c.is_empty())
        {
            let pc = proc_comm_s.unwrap();
            let n = (TASK_COMM_LEN - 1).min(pc.len());
            let from = &cmdline[cmdline_basename_start.min(cmdline.len())..];
            if from.len() < n || from[..n] != pc[..n] {
                write_highlight!(0, pc.len(), comm_attr, CMDLINE_HIGHLIGHT_FLAG_COMM);
                buf.extend_from_slice(pc);
                if !show_merged_command {
                    commit(this, &buf, &highlights);
                    return;
                }
                write_separator!();
            }
        }

        if shadow_dist_path_prefix && show_program_path {
            if let Some(plen) = dist_prefix_len(cmdline) {
                write_highlight!(0, plen, shadow_attr, CMDLINE_HIGHLIGHT_FLAG_PREFIXDIR);
            }
        }

        if cmdline_basename_len > 0 {
            let off = if show_program_path {
                cmdline_basename_start
            } else {
                0
            };
            write_highlight!(
                off,
                cmdline_basename_len,
                base_attr,
                CMDLINE_HIGHLIGHT_FLAG_BASENAME
            );
            if proc_exe_deleted {
                write_highlight!(
                    off,
                    cmdline_basename_len,
                    del_exe_attr,
                    CMDLINE_HIGHLIGHT_FLAG_DELETED
                );
            } else if uses_deleted_lib {
                write_highlight!(
                    off,
                    cmdline_basename_len,
                    del_lib_attr,
                    CMDLINE_HIGHLIGHT_FLAG_DELETED
                );
            }
        }

        let tail = if show_program_path {
            cmdline
        } else {
            &cmdline[cmdline_basename_start.min(cmdline.len())..]
        };
        stpcpyWithNewlineConversion(&mut buf, tail);
        commit(this, &buf, &highlights);
        return;
    }

    // ── Merged command (exe + comm + cmdline) ──────────────────────────────
    let proc_exe_s = proc_exe_s.unwrap();
    let proc_comm_s = proc_comm_s.unwrap();

    let mut comm_len = 0usize;
    let mut have_comm_in_exe = false;
    if !is_userland_thread || show_thread_names {
        // strncmp(procExe + exeBasenameOffset, procComm, TASK_COMM_LEN - 1) == 0
        // — a FIXED length of 15, NOT MINIMUM(15, strlen(comm)) as the cmdline
        // fallback uses. A comm shorter than the exe basename therefore fails:
        // comm's terminating NUL is compared against the exe's next byte, so the
        // exe basename must equal comm exactly within 15 bytes. `get(i)` yielding
        // 0 past each slice's end models the C strings' NUL terminators, matching
        // strncmp's stop-at-NUL behavior.
        let exe_tail = &proc_exe_s[exe_basename_offset.min(proc_exe_s.len())..];
        have_comm_in_exe = (0..TASK_COMM_LEN - 1).all(|i| {
            exe_tail.get(i).copied().unwrap_or(0) == proc_comm_s.get(i).copied().unwrap_or(0)
        });
    }
    if have_comm_in_exe {
        comm_len = exe_basename_len;
    }

    let mut have_comm_in_cmdline = false;
    let mut comm_start = 0usize;
    if !have_comm_in_exe
        && cmdline_present
        && search_comm_in_cmdline
        && (!is_userland_thread || show_thread_names)
    {
        if let Some((cs, cl)) = findCommInCmdline(proc_comm_s, cmdline, cmdline_basename_start) {
            have_comm_in_cmdline = true;
            comm_start = cs;
            comm_len = cl;
        }
    }

    if !strip_exe_from_cmdline {
        match_len = 0;
    }
    if match_len != 0 {
        cmdline = &cmdline[match_len.min(cmdline.len())..];
        if have_comm_in_cmdline {
            if comm_start == cmdline_basename_start {
                have_comm_in_exe = true;
                have_comm_in_cmdline = false;
                comm_start = 0;
            } else {
                comm_start -= match_len;
            }
        }
    }

    // Copy exe first.
    if show_program_path {
        if shadow_dist_path_prefix {
            if let Some(plen) = dist_prefix_len(proc_exe_s) {
                write_highlight!(0, plen, shadow_attr, CMDLINE_HIGHLIGHT_FLAG_PREFIXDIR);
            }
        }
        if have_comm_in_exe {
            write_highlight!(
                exe_basename_offset,
                comm_len,
                comm_attr,
                CMDLINE_HIGHLIGHT_FLAG_COMM
            );
        }
        write_highlight!(
            exe_basename_offset,
            exe_basename_len,
            base_attr,
            CMDLINE_HIGHLIGHT_FLAG_BASENAME
        );
        if proc_exe_deleted {
            write_highlight!(
                exe_basename_offset,
                exe_basename_len,
                del_exe_attr,
                CMDLINE_HIGHLIGHT_FLAG_DELETED
            );
        } else if uses_deleted_lib {
            write_highlight!(
                exe_basename_offset,
                exe_basename_len,
                del_lib_attr,
                CMDLINE_HIGHLIGHT_FLAG_DELETED
            );
        }
        buf.extend_from_slice(proc_exe_s);
    } else {
        if have_comm_in_exe {
            write_highlight!(0, comm_len, comm_attr, CMDLINE_HIGHLIGHT_FLAG_COMM);
        }
        write_highlight!(
            0,
            exe_basename_len,
            base_attr,
            CMDLINE_HIGHLIGHT_FLAG_BASENAME
        );
        if proc_exe_deleted {
            write_highlight!(
                0,
                exe_basename_len,
                del_exe_attr,
                CMDLINE_HIGHLIGHT_FLAG_DELETED
            );
        } else if uses_deleted_lib {
            write_highlight!(
                0,
                exe_basename_len,
                del_lib_attr,
                CMDLINE_HIGHLIGHT_FLAG_DELETED
            );
        }
        buf.extend_from_slice(&proc_exe_s[exe_basename_offset.min(proc_exe_s.len())..]);
    }

    let mut have_comm_field = false;
    if !have_comm_in_exe && !have_comm_in_cmdline && (!is_userland_thread || show_thread_names) {
        write_separator!();
        write_highlight!(0, proc_comm_s.len(), comm_attr, CMDLINE_HIGHLIGHT_FLAG_COMM);
        buf.extend_from_slice(proc_comm_s);
        have_comm_field = true;
    }

    if match_len == 0 || (have_comm_field && !cmdline.is_empty()) {
        write_separator!();
    }

    if shadow_dist_path_prefix {
        if let Some(plen) = dist_prefix_len(cmdline) {
            write_highlight!(0, plen, shadow_attr, CMDLINE_HIGHLIGHT_FLAG_PREFIXDIR);
        }
    }

    if !have_comm_in_exe
        && have_comm_in_cmdline
        && !have_comm_field
        && (!is_userland_thread || show_thread_names)
    {
        write_highlight!(comm_start, comm_len, comm_attr, CMDLINE_HIGHLIGHT_FLAG_COMM);
    }

    if !cmdline.is_empty() {
        stpcpyWithNewlineConversion(&mut buf, cmdline);
    }

    commit(this, &buf, &highlights);
}

/// Port of `void Process_writeCommand(const Process* this, int attr, int
/// baseAttr, RichString* str)` from `Process.c:494`. Appends the process's
/// command to `str`: when the cached merged-command string is present it is
/// appended and its recorded highlight regions are re-applied (filtered by
/// the `highlightBaseName`/`highlightDeletedExe` settings); otherwise the raw
/// `cmdline` is appended, trimmed to its basename per `showProgramPath`, with
/// the basename span highlighted when `highlightBaseName` is set.
///
/// Reads `this->super.host->settings` via the established
/// `Row::host as *const Machine` deref (the host is a live back-pointer, as
/// in C).
pub fn Process_writeCommand(this: &Process, attr: i32, baseAttr: i32, str: &mut RichString) {
    let mc = &this.mergedCommand;
    let str_start = RichString_size(str) as usize;

    // C `const Settings* settings = this->super.host->settings;`
    let host = unsafe { &*(this.super_.host as *const Machine) };
    let settings = host
        .settings
        .as_ref()
        .expect("Process_writeCommand: host->settings is NULL");
    let highlight_base_name = settings.highlightBaseName;
    let highlight_separator = true;
    let highlight_deleted = settings.highlightDeletedExe;

    let merged_command = match &mc.str {
        // C `if (!mergedCommand)` — no cached string: render the cmdline.
        None => {
            let cmdline_full = this.cmdline.as_deref().unwrap_or("");
            let cmdline_bytes = cmdline_full.as_bytes();
            let mut len: usize = 0;
            let mut basename: usize = 0;
            let mut cmd_offset: usize = 0; // C's `cmdline += basename`
            let mut str_start = str_start;

            if highlight_base_name || !settings.showProgramPath {
                for i in 0..this.cmdlineBasenameEnd.min(cmdline_bytes.len()) {
                    if cmdline_bytes[i] == b'/' {
                        basename = i + 1;
                    } else if cmdline_bytes[i] == b':' {
                        len = i + 1;
                        break;
                    }
                }
                if len == 0 {
                    if settings.showProgramPath {
                        str_start += basename;
                    } else {
                        cmd_offset = basename;
                    }
                    len = this.cmdlineBasenameEnd - basename;
                }
            }

            RichString_appendWide(
                str,
                attr,
                &cmdline_bytes[cmd_offset.min(cmdline_bytes.len())..],
            );
            if settings.highlightBaseName {
                RichString_setAttrn(str, baseAttr, str_start, len);
            }
            return;
        }
        Some(s) => s,
    };

    RichString_appendWide(str, attr, merged_command.as_bytes());

    // C `CLAMP(mc->highlightCount, 0, ARRAYSIZE(mc->highlights))`.
    let hl_count = mc.highlightCount.min(mc.highlights.len());
    for hl in &mc.highlights[..hl_count] {
        if hl.length == 0 {
            continue;
        }
        if hl.flags & CMDLINE_HIGHLIGHT_FLAG_SEPARATOR != 0 && !highlight_separator {
            continue;
        }
        if hl.flags & CMDLINE_HIGHLIGHT_FLAG_BASENAME != 0 && !highlight_base_name {
            continue;
        }
        if hl.flags & CMDLINE_HIGHLIGHT_FLAG_DELETED != 0 && !highlight_deleted {
            continue;
        }
        if hl.flags & CMDLINE_HIGHLIGHT_FLAG_PREFIXDIR != 0 && !highlight_deleted {
            continue;
        }
        RichString_setAttrn(str, hl.attr, str_start + hl.offset, hl.length);
    }
}

/// Port of `processStateChar(ProcessState state)` from `Process.c:568`.
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

/// Port of `static void Process_rowWriteField(const Row* super, RichString*
/// str, RowField field)` from `Process.c:590` — the `writeField`
/// [`RowClass`] vtable slot for `Process`. Downcasts the object (C's
/// `(const Process*)super`) and delegates to [`Process_writeField`].
pub fn Process_rowWriteField(super_: &dyn Object, str: &mut RichString, field: RowField) {
    debug_assert!(Object_isA(Some(super_), &Process_class));
    let this = super_
        .as_process()
        .expect("Process_rowWriteField: row is not a Process");
    Process_writeField(this, str, field);
}

/// Port of `void Process_writeField(const Process* this, RichString* str,
/// RowField field)` from `Process.c:596` — the base per-field renderer. Each
/// arm either delegates to a `Row_print*` helper / `Process_writeCommand` /
/// `Row_printLeftAlignedField` (the C `return` arms) or formats into a text
/// buffer and picks a color, which the shared tail appends (the C `break`
/// arms). `CRT_colors[X]` is `ColorElements::X.packed(active_scheme)`;
/// `Process_pidDigits`/`Process_uidDigits` are the [`Row_pidDigits`]/
/// [`Row_uidDigits`] globals; `host->settings` is reached via the established
/// `Row::host as *const Machine` deref.
pub fn Process_writeField(this: &Process, str: &mut RichString, field: RowField) {
    use ProcessField as PF;
    use ProcessState::*;

    let host = unsafe { &*(this.super_.host as *const Machine) };
    let settings = host
        .settings
        .as_ref()
        .expect("Process_writeField: host->settings is NULL");
    let coloring = settings.highlightMegabytes;
    let scheme = ColorScheme::active();
    let n = 255usize; // C `sizeof(buffer) - 1`
    let mut attr = CE::DEFAULT_COLOR.packed(scheme);
    // The text buffer for the C `break` arms; the `return` arms never reach
    // the shared tail append. Each fall-through arm assigns it.
    let buffer: String;

    match field {
        f if f == PF::COMM as RowField => {
            let mut baseattr = CE::PROCESS_BASENAME.packed(scheme);
            if settings.highlightThreads && Process_isThread(this) {
                attr = CE::PROCESS_THREAD.packed(scheme);
                baseattr = CE::PROCESS_THREAD_BASENAME.packed(scheme);
            }
            let ss = &settings.screens[settings.ssIndex as usize];
            let indent = this.super_.indent;
            if !ss.treeView || indent == 0 {
                Process_writeCommand(this, attr, baseattr, str);
                return;
            }
            // Build the tree-prefix glyphs (C accumulates into `buffer`).
            let last_item = indent < 0;
            let mut tree = String::new();
            let mut ind: u32 = if indent < 0 {
                (-indent) as u32
            } else {
                indent as u32
            };
            while ind > 1 {
                if ind & 1 != 0 {
                    tree.push_str(TreeStr::TREE_STR_VERT.glyph());
                    tree.push_str("  ");
                } else {
                    tree.push_str("   ");
                }
                ind >>= 1;
            }
            let draw = if last_item {
                TreeStr::TREE_STR_BEND
            } else {
                TreeStr::TREE_STR_RTEE
            };
            let openshut = if this.super_.showChildren {
                TreeStr::TREE_STR_SHUT
            } else {
                TreeStr::TREE_STR_OPEN
            };
            tree.push_str(draw.glyph());
            tree.push_str(openshut.glyph());
            tree.push(' ');
            RichString_appendWide(str, CE::PROCESS_TREE.packed(scheme), tree.as_bytes());
            Process_writeCommand(this, attr, baseattr, str);
            return;
        }
        f if f == PF::PROC_COMM as RowField => {
            let (a, content): (i32, &[u8]) = match &this.procComm {
                Some(pc) => {
                    let a = if Process_isUserlandThread(this) {
                        CE::PROCESS_THREAD_COMM.packed(scheme)
                    } else {
                        CE::PROCESS_COMM.packed(scheme)
                    };
                    (a, pc.as_bytes())
                }
                None => {
                    let c: &[u8] = if Process_isKernelThread(this) {
                        kthreadID
                    } else {
                        b"N/A"
                    };
                    (CE::PROCESS_SHADOW.packed(scheme), c)
                }
            };
            Row_printLeftAlignedField(str, a, content, (TASK_COMM_LEN - 1) as u32);
            return;
        }
        f if f == PF::PROC_EXE as RowField => {
            let (a, content): (i32, &[u8]) = match &this.procExe {
                Some(pe) => {
                    let mut a = if Process_isUserlandThread(this) {
                        CE::PROCESS_THREAD_BASENAME.packed(scheme)
                    } else {
                        CE::PROCESS_BASENAME.packed(scheme)
                    };
                    if settings.highlightDeletedExe {
                        if this.procExeDeleted {
                            a = CE::FAILED_READ.packed(scheme);
                        } else if this.usesDeletedLib {
                            a = CE::PROCESS_TAG.packed(scheme);
                        }
                    }
                    (a, &pe.as_bytes()[this.procExeBasenameOffset..])
                }
                None => {
                    let c: &[u8] = if Process_isKernelThread(this) {
                        kthreadID
                    } else {
                        b"N/A"
                    };
                    (CE::PROCESS_SHADOW.packed(scheme), c)
                }
            };
            Row_printLeftAlignedField(str, a, content, (TASK_COMM_LEN - 1) as u32);
            return;
        }
        f if f == PF::CWD as RowField => {
            let (a, content): (i32, &[u8]) = match &this.procCwd {
                None => (CE::PROCESS_SHADOW.packed(scheme), b"N/A".as_slice()),
                Some(c) if String_startsWith(c, "/proc/") && c.contains(" (deleted)") => (
                    CE::PROCESS_SHADOW.packed(scheme),
                    b"main thread terminated".as_slice(),
                ),
                Some(c) => (attr, c.as_bytes()),
            };
            Row_printLeftAlignedField(str, a, content, 25);
            return;
        }
        f if f == PF::ELAPSED as RowField => {
            let rt = host.realtimeMs;
            let st = (this.starttime_ctime as u64).wrapping_mul(1000);
            let dt = if rt < st { 0 } else { rt - st };
            Row_printTime(str, dt / 10, coloring);
            return;
        }
        f if f == PF::MAJFLT as RowField => {
            Row_printCount(str, this.majflt, coloring);
            return;
        }
        f if f == PF::MINFLT as RowField => {
            Row_printCount(str, this.minflt, coloring);
            return;
        }
        f if f == PF::M_RESIDENT as RowField => {
            Row_printKBytes(str, this.m_resident as u64, coloring);
            return;
        }
        f if f == PF::M_VIRT as RowField => {
            Row_printKBytes(str, this.m_virt as u64, coloring);
            return;
        }
        f if f == PF::NICE as RowField => {
            if this.nice == PROCESS_NICE_UNKNOWN {
                buffer = "N/A ".to_string();
                attr = CE::PROCESS_SHADOW.packed(scheme);
            } else {
                buffer = format!("{:>3} ", this.nice);
                attr = if this.nice < 0 {
                    CE::PROCESS_HIGH_PRIORITY.packed(scheme)
                } else if this.nice > 0 {
                    CE::PROCESS_LOW_PRIORITY.packed(scheme)
                } else {
                    CE::PROCESS_SHADOW.packed(scheme)
                };
            }
        }
        f if f == PF::NLWP as RowField => {
            if this.nlwp == 1 {
                attr = CE::PROCESS_SHADOW.packed(scheme);
            }
            buffer = format!("{:>4} ", this.nlwp);
        }
        f if f == PF::PERCENT_CPU as RowField => {
            let mut pa = PercentageAttr::Unchanged;
            let w = Row_fieldWidths[PF::PERCENT_CPU as usize].load(Ordering::Relaxed);
            buffer = Row_printPercentage(this.percent_cpu, n, w, &mut pa);
            match pa {
                PercentageAttr::Shadow => attr = CE::PROCESS_SHADOW.packed(scheme),
                PercentageAttr::Megabytes => attr = CE::PROCESS_MEGABYTES.packed(scheme),
                PercentageAttr::Unchanged => {}
            }
        }
        f if f == PF::PERCENT_NORM_CPU as RowField => {
            let mut pa = PercentageAttr::Unchanged;
            let w = Row_fieldWidths[PF::PERCENT_CPU as usize].load(Ordering::Relaxed);
            let cpu_pct = this.percent_cpu / host.activeCPUs as f32;
            buffer = Row_printPercentage(cpu_pct, n, w, &mut pa);
            match pa {
                PercentageAttr::Shadow => attr = CE::PROCESS_SHADOW.packed(scheme),
                PercentageAttr::Megabytes => attr = CE::PROCESS_MEGABYTES.packed(scheme),
                PercentageAttr::Unchanged => {}
            }
        }
        f if f == PF::PERCENT_MEM as RowField => {
            let mut pa = PercentageAttr::Unchanged;
            buffer = Row_printPercentage(this.percent_mem, n, 4, &mut pa);
            match pa {
                PercentageAttr::Shadow => attr = CE::PROCESS_SHADOW.packed(scheme),
                PercentageAttr::Megabytes => attr = CE::PROCESS_MEGABYTES.packed(scheme),
                PercentageAttr::Unchanged => {}
            }
        }
        f if f == PF::PGRP as RowField => {
            let w = Row_pidDigits.load(Ordering::Relaxed) as usize;
            buffer = format!("{:>w$} ", this.pgrp);
        }
        f if f == PF::PID as RowField => {
            let w = Row_pidDigits.load(Ordering::Relaxed) as usize;
            buffer = format!("{:>w$} ", Process_getPid(this));
        }
        f if f == PF::PPID as RowField => {
            let w = Row_pidDigits.load(Ordering::Relaxed) as usize;
            buffer = format!("{:>w$} ", Process_getParent(this));
        }
        f if f == PF::PRIORITY as RowField => {
            buffer = if this.priority <= -100 {
                " RT ".to_string()
            } else {
                format!("{:>3} ", this.priority)
            };
        }
        f if f == PF::PROCESSOR as RowField => {
            // Settings_cpuId(settings, cpu) = countCPUsFromOne ? cpu+1 : cpu.
            let cpu_id = if settings.countCPUsFromOne {
                this.processor + 1
            } else {
                this.processor
            };
            buffer = format!("{:>3} ", cpu_id);
        }
        f if f == PF::SCHEDULERPOLICY as RowField => {
            let s = if this.scheduling_policy >= 0 {
                Scheduling_formatPolicy(this.scheduling_policy)
            } else {
                "N/A"
            };
            buffer = format!("{s:<5} ");
        }
        f if f == PF::SESSION as RowField => {
            let w = Row_pidDigits.load(Ordering::Relaxed) as usize;
            buffer = format!("{:>w$} ", this.session);
        }
        f if f == PF::STARTTIME as RowField => {
            // C `"%s"` on the NUL-terminated `starttime_show[8]`.
            let end = this
                .starttime_show
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(this.starttime_show.len());
            buffer = String::from_utf8_lossy(&this.starttime_show[..end]).into_owned();
        }
        f if f == PF::STATE as RowField => {
            buffer = format!("{} ", processStateChar(this.state));
            attr = match this.state {
                RUNNABLE | RUNNING | TRACED => CE::PROCESS_RUN_STATE.packed(scheme),
                BLOCKED | DEFUNCT | STOPPED | UNINTERRUPTIBLE_WAIT | ZOMBIE => {
                    CE::PROCESS_D_STATE.packed(scheme)
                }
                QUEUED | WAITING | IDLE | SLEEPING => CE::PROCESS_SHADOW.packed(scheme),
                UNKNOWN | PAGING => attr,
            };
        }
        f if f == PF::ST_UID as RowField => {
            let w = Row_uidDigits.load(Ordering::Relaxed) as usize;
            buffer = format!("{:>w$} ", this.st_uid);
        }
        f if f == PF::TIME as RowField => {
            Row_printTime(str, this.time, coloring);
            return;
        }
        f if f == PF::TGID as RowField => {
            if Process_getThreadGroup(this) == Process_getPid(this) {
                attr = CE::PROCESS_SHADOW.packed(scheme);
            }
            let w = Row_pidDigits.load(Ordering::Relaxed) as usize;
            buffer = format!("{:>w$} ", Process_getThreadGroup(this));
        }
        f if f == PF::TPGID as RowField => {
            let w = Row_pidDigits.load(Ordering::Relaxed) as usize;
            buffer = format!("{:>w$} ", this.tpgid);
        }
        f if f == PF::TTY as RowField => match &this.tty_name {
            None => {
                attr = CE::PROCESS_SHADOW.packed(scheme);
                buffer = "(no tty) ".to_string();
            }
            Some(t) => {
                let name = if String_startsWith(t, "/dev/") {
                    &t[5..]
                } else {
                    t.as_str()
                };
                buffer = format!("{name:<8} ");
            }
        },
        f if f == PF::USER as RowField => {
            if this.elevated_priv == Tristate::TRI_ON {
                attr = CE::PROCESS_PRIV.packed(scheme);
            } else if host.htopUserId != this.st_uid {
                attr = CE::PROCESS_SHADOW.packed(scheme);
            }
            if let Some(u) = &this.user {
                Row_printLeftAlignedField(str, attr, u.as_bytes(), 10);
                return;
            }
            buffer = format!("{:<10} ", this.st_uid);
        }
        _ => {
            // Dynamic column, or (in C) an assert-guarded unreachable.
            if DynamicColumn_writeField(this, str, field as u32) {
                return;
            }
            debug_assert!(false, "Process_writeField: default key reached");
            buffer = "- ".to_string();
        }
    }

    RichString_appendAscii(str, attr, buffer.as_bytes());
}

/// Port of `void Process_done(Process* this)` from `Process.c:818`: a pure
/// `free()` teardown of `cmdline`, `procComm`, `procExe`, `procCwd`,
/// `mergedCommand.str`, and `tty_name`. Every one of those C `char*`s is an
/// owned `Option<String>` on [`Process`], so taking `this` by value and
/// dropping it reclaims them all — the whole C free routine, with no
/// separate struct free (C's caller does `free(this)` after `Process_done`;
/// the by-value consume folds both together).
pub fn Process_done(this: Process) {
    let _ = this;
}

/// Port of `const char* Process_getCommand(const Process* this)`
/// from `Process.c:831`. Reads `this->super.host->settings->showThreadNames`
/// (`Process.c:834`) via the `Row::host as *const Machine` deref: a
/// userland thread with `showThreadNames` set, or a process with no cached
/// merged-command string, renders its raw `cmdline`; otherwise the cached
/// `mergedCommand.str` is returned. Drives the `COMM` case of
/// [`Process_compareByKey_Base`]. Returns the command bytes
/// (C `const char*`).
pub fn Process_getCommand(this: &Process) -> Option<&[u8]> {
    let host = unsafe { &*(this.super_.host as *const Machine) };
    let settings = host
        .settings
        .as_ref()
        .expect("Process_getCommand: host->settings is NULL");
    if (Process_isUserlandThread(this) && settings.showThreadNames)
        || this.mergedCommand.str.is_none()
    {
        return this.cmdline.as_deref().map(str::as_bytes);
    }
    this.mergedCommand.str.as_deref().map(str::as_bytes)
}

/// Port of `static const char* Process_getSortKey(const Process* this)`
/// from `Process.c:841`: `return Process_getCommand(this)`. A thin
/// delegation to [`Process_getCommand`]. Returns the command bytes
/// (C `const char*`).
pub fn Process_getSortKey(this: &Process) -> Option<&[u8]> {
    Process_getCommand(this)
}

/// Port of `const char* Process_rowGetSortKey(Row* super)` from
/// `Process.c:845`. Casts the `Row*` to `Process*` (the `Object_isA`
/// guard + `Any` downcast idiom, matching the C `(const Process*) super`
/// + `assert(Object_isA(...))`) and delegates to [`Process_getSortKey`].
pub fn Process_rowGetSortKey(super_: &dyn Object) -> Option<&[u8]> {
    debug_assert!(Object_isA(Some(super_), &Process_class));
    let this = super_
        .as_process()
        .expect("Process_rowGetSortKey: row is not a Process");
    Process_getSortKey(this)
}

/// Port of `static bool Process_isHighlighted(const Process* this)`
/// from `Process.c:852`. True when the row belongs to another user and
/// `shadowOtherUsers` is set, so the display shadows it. Reads
/// `this->super.host->settings->shadowOtherUsers` and `host->htopUserId` via
/// the established `Row::host as *const Machine` deref.
pub fn Process_isHighlighted(this: &Process) -> bool {
    let host = unsafe { &*(this.super_.host as *const Machine) };
    let settings = host
        .settings
        .as_ref()
        .expect("Process_isHighlighted: host->settings is NULL");
    settings.shadowOtherUsers && this.st_uid != host.htopUserId
}

/// Port of `bool Process_rowIsHighlighted(const Row* super)` from
/// `Process.c:858`. Casts the `Row*` to `Process*` (the `Object_isA`
/// guard + `Any` downcast idiom) and delegates to
/// [`Process_isHighlighted`]; the wiring is faithful.
pub fn Process_rowIsHighlighted(super_: &dyn Object) -> bool {
    debug_assert!(Object_isA(Some(super_), &Process_class));
    let this = super_
        .as_process()
        .expect("Process_rowIsHighlighted: row is not a Process");
    Process_isHighlighted(this)
}

/// Port of `static bool Process_isVisible(const Process* p, const Settings*
/// settings)` from `Process.c:865`. Hides userland threads when
/// `hideUserlandThreads` is set; otherwise every process is visible.
pub fn Process_isVisible(p: &Process, settings: &Settings) -> bool {
    if settings.hideUserlandThreads {
        return !Process_isThread(p);
    }
    true
}

/// Port of `bool Process_rowIsVisible(const Row* super, const Table* table)`
/// from `Process.c:871` — the `isVisible` [`RowClass`] slot for `Process`.
/// Downcasts and delegates to [`Process_isVisible`] with the table's host
/// settings.
pub fn Process_rowIsVisible(super_: &dyn Object, table: &Table) -> bool {
    debug_assert!(Object_isA(Some(super_), &Process_class));
    let this = super_
        .as_process()
        .expect("Process_rowIsVisible: row is not a Process");
    let host = unsafe { &*table.host };
    let settings = host
        .settings
        .as_ref()
        .expect("Process_rowIsVisible: table->host->settings is NULL");
    Process_isVisible(this, settings)
}

/// Port of `static bool Process_matchesFilter(const Process* this, const
/// Table* table)` from `Process.c:878`. Returns whether the display must
/// filter this process out, via the three C mechanisms: (1) a per-user view
/// (`host->userId != -1 && st_uid != userId`); (2) the incremental filter
/// (`incFilter` set and not a case-insensitive substring of the command);
/// (3) the pid-match list (`pidMatchList` set and the thread group absent
/// from it).
///
/// Signature mapping: `host->userId` is `(uid_t)-1` when unset — `u32::MAX`
/// here. `Process_getCommand` returns `&[u8]` (bytes from an owned
/// `String`, valid UTF-8); [`String_contains_i`] takes `&str`, so the bytes
/// are re-borrowed as `&str` (a `None`/invalid command maps to `""`, as the
/// filter never matches on an absent command). `host->activeTable` is a
/// borrowed `*mut Table`; the C `(const ProcessTable*) host->activeTable`
/// downcast is the `#[repr(C)]` `*const ProcessTable` cast (the base `Table`
/// sits at offset 0). The C `assert(Object_isA(pt, &ProcessTable_class))` is
/// dropped — the port models no `Object`/class tag on `ProcessTable`.
/// `pidMatchList` is a borrowed `*mut Hashtable`, dereferenced for
/// [`Hashtable_get`] keyed by [`Process_getThreadGroup`].
pub fn Process_matchesFilter(this: &Process, table: &Table) -> bool {
    // const Machine* host = table->host;
    let host = unsafe { &*table.host };
    if host.userId != u32::MAX && this.st_uid != host.userId {
        return true;
    }

    // const char* incFilter = table->incFilter;
    if let Some(incFilter) = table.incFilter.as_deref() {
        let cmd_bytes = Process_getCommand(this).unwrap_or(b"");
        let cmd = core::str::from_utf8(cmd_bytes).unwrap_or("");
        if !String_contains_i(cmd, incFilter, true) {
            return true;
        }
    }

    // const ProcessTable* pt = (const ProcessTable*) host->activeTable;
    let pt = unsafe {
        let t = host
            .activeTable
            .expect("Process_matchesFilter: host->activeTable is NULL");
        &*(t as *const ProcessTable)
    };
    if let Some(pml) = pt.pidMatchList {
        let list = unsafe { &*pml };
        if Hashtable_get(list, Process_getThreadGroup(this) as u32).is_none() {
            return true;
        }
    }

    false
}

/// Port of `bool Process_rowMatchesFilter(const Row* super, const Table*
/// table)` from `Process.c:895`. Casts the `Row*` to `Process*` (the
/// `Object_isA` guard + `Any` downcast idiom) and delegates to
/// [`Process_matchesFilter`]. Wired into the `Process_class` `matchesFilter`
/// [`RowClass`] slot.
pub fn Process_rowMatchesFilter(super_: &dyn Object, table: &Table) -> bool {
    debug_assert!(Object_isA(Some(super_), &Process_class));
    let this = super_
        .as_process()
        .expect("Process_rowMatchesFilter: row is not a Process");
    Process_matchesFilter(this, table)
}

/// Port of `void Process_init(Process* this, const Machine* host)` from
/// `Process.c:901`. Runs the base [`Row_init`] then sets the two
/// process-specific defaults the C body assigns
/// (`cmdlineBasenameEnd = 0`, `st_uid = (uid_t)-1`). Pure — `host` is
/// only stored, never dereferenced.
pub fn Process_init(this: &mut Process, host: *const c_void) {
    Row_init(&mut this.super_, host);

    this.cmdlineBasenameEnd = 0;
    this.st_uid = u32::MAX; // (uid_t)-1
}

/// Port of `static bool Process_setPriority(Process* this, int priority)`
/// from `Process.c:908`. Refuses in read-only mode ([`Settings_isReadonly`]),
/// then `setpriority(PRIO_PROCESS, pid, priority)`. On success, the cached
/// `nice` is refreshed only when the kernel actually changed the value
/// (`old_prio != getpriority(...)` re-read) — htop's guard against a no-op
/// call leaving a stale `nice`. Returns whether `setpriority` succeeded.
///
/// The `getpriority`/`setpriority` syscalls are POSIX (present on the
/// darwin dev host), so this needs no cfg gate. `who` is the pid widened
/// to `id_t`, matching the C implicit `pid_t`→`id_t` promotion.
pub fn Process_setPriority(this: &mut Process, priority: i32) -> bool {
    if Settings_isReadonly() {
        return false;
    }

    let who = Process_getPid(this) as libc::id_t;
    let old_prio = unsafe { libc::getpriority(libc::PRIO_PROCESS, who as _) };

    let err = unsafe { libc::setpriority(libc::PRIO_PROCESS, who as _, priority) };

    if err == 0 && old_prio != unsafe { libc::getpriority(libc::PRIO_PROCESS, who as _) } {
        this.nice = priority;
    }
    err == 0
}

/// Port of `bool Process_rowChangePriorityBy(Row* super, Arg delta)` from
/// `Process.c:921`. Casts the `Row*` to `Process*` (the `Object_isA`
/// guard + mutable `Any` downcast idiom), then nudges the priority by
/// `delta.i` relative to the current `nice`. The C `(int)this->nice +
/// delta.i` is `i32` arithmetic; `delta` is the [`Arg::I`] arm (the
/// `Arg::V` arm is impossible here, matching the unconditional C `delta.i`
/// union read).
pub fn Process_rowChangePriorityBy(super_: &mut dyn Object, delta: Arg) -> bool {
    debug_assert!(Object_isA(Some(super_ as &dyn Object), &Process_class));
    // Panel items are platform subclasses (DarwinProcess/LinuxProcess), not a
    // bare Process, so `downcast_mut::<Process>()` — which needs the exact
    // concrete type — fails ("row is not a Process"). `as_process_mut()` is the
    // faithful `(Process*)super` upcast the subclasses override.
    let this = super_
        .as_process_mut()
        .expect("Process_rowChangePriorityBy: row is not a Process");
    let delta_i = match delta {
        Arg::I(i) => i,
        Arg::V(_) => panic!("Process_rowChangePriorityBy: Arg must carry the delta in arg.i"),
    };
    let priority = this.nice + delta_i;
    Process_setPriority(this, priority)
}

/// Port of `static bool Process_sendSignal(Process* this, Arg sgn)` from
/// `Process.c:927`. A thin wrapper over `kill(pid, sgn.i)`, returning
/// whether the syscall succeeded. `sgn` is the [`Arg::I`] arm carrying the
/// signal number (the `Arg::V` arm is impossible, matching the C `sgn.i`
/// union read).
pub fn Process_sendSignal(this: &Process, sgn: Arg) -> bool {
    let signum = match sgn {
        Arg::I(i) => i,
        Arg::V(_) => panic!("Process_sendSignal: Arg must carry the signal in arg.i"),
    };
    unsafe { libc::kill(Process_getPid(this), signum) == 0 }
}

/// Port of `bool Process_rowSendSignal(Row* super, Arg sgn)` from
/// `Process.c:931`. Casts the `Row*` to `Process*` (the `Object_isA` guard
/// + `Any` downcast) and delegates to [`Process_sendSignal`]. `sendSignal`
/// only reads the pid, so a shared `&Process` suffices.
pub fn Process_rowSendSignal(super_: &mut dyn Object, sgn: Arg) -> bool {
    debug_assert!(Object_isA(Some(super_ as &dyn Object), &Process_class));
    // `&mut dyn Object` to match `MainPanel_foreachRowFn` (C's non-const
    // `Row*`); `sendSignal` only reads the pid, so an immutable `as_process`
    // view suffices.
    let this = super_
        .as_process()
        .expect("Process_rowSendSignal: row is not a Process");
    Process_sendSignal(this, sgn)
}

/// Port of `int Process_compare(const void* v1, const void* v2)`
/// from `Process.c:914`. Reads the active screen's sort key/direction from
/// `p1->super.host->settings->ss` (via the `Row::host as *const Machine`
/// deref) with [`ScreenSettings_getActiveSortKey`] /
/// [`ScreenSettings_getActiveDirection`], dispatches the per-field
/// comparison ([`Process_compareByKey_Base`], or a subclass `compareByKey`
/// slot), tie-breaks equal rows by PID, then applies the sort direction.
/// Signature matches the two-`Process` C comparator.
pub fn Process_compare(v1: &dyn Object, v2: &dyn Object) -> i32 {
    let p1 = v1
        .as_process()
        .expect("Process_compare: v1 is not a Process");
    let p2 = v2
        .as_process()
        .expect("Process_compare: v2 is not a Process");

    // C `const ScreenSettings* ss = p1->super.host->settings->ss;`
    let host = unsafe { &*(p1.super_.host as *const Machine) };
    let settings = host
        .settings
        .as_ref()
        .expect("Process_compare: host->settings is NULL");
    let ss = &settings.screens[settings.ssIndex as usize];

    let key = ScreenSettings_getActiveSortKey(ss);

    // C `Process_compareByKey(p1, p2, key)` macro: dispatch the concrete
    // `As_Process(p1)->compareByKey` slot if set, else the base comparator.
    let result = match v1.process_class().and_then(|pc| pc.compareByKey) {
        Some(f) => f(v1, v2, key),
        None => Process_compareByKey_Base(p1, p2, key),
    };

    // Tie-breaker (keeps tree mode stable): order by PID.
    if result == 0 {
        return spaceship_number!(Process_getPid(p1), Process_getPid(p2));
    }

    if ScreenSettings_getActiveDirection(ss) == 1 {
        result
    } else {
        -result
    }
}

/// Port of `int Process_compareByParent(const Row* r1, const Row* r2)`
/// from `Process.c:931`. Orders by group-or-parent (roots sort as `0`
/// via [`Row_getGroupOrParent`]), tie-breaking with [`Process_compare`]
/// — the stable tree-mode ordering. The two C `Row*` are cast to
/// `Process*`; here the `Object_isA` guard + `Any` downcast idiom (as in
/// [`Process_rowGetSortKey`]) yields the `Process` views, the Row-level
/// group/parent read goes through the embedded `super_`, and the
/// tie-break passes those views to [`Process_compare`], matching the
/// [`Process_getSortKey`] → [`Process_getCommand`] precedent.
pub fn Process_compareByParent(r1: &dyn Object, r2: &dyn Object) -> i32 {
    debug_assert!(Object_isA(Some(r1), &Process_class));
    debug_assert!(Object_isA(Some(r2), &Process_class));
    let p1 = r1
        .as_process()
        .expect("Process_compareByParent: row is not a Process");
    let p2 = r2
        .as_process()
        .expect("Process_compareByParent: row is not a Process");

    let result = spaceship_number!(
        if p1.super_.isRoot {
            0
        } else {
            Row_getGroupOrParent(&p1.super_)
        },
        if p2.super_.isRoot {
            0
        } else {
            Row_getGroupOrParent(&p2.super_)
        }
    );

    if result != 0 {
        return result;
    }

    Process_compare(p1, p2)
}

/// Port of `int Process_compareByKey_Base(const Process* p1, const
/// Process* p2, ProcessField key)` from `Process.c:966`. The per-field
/// sort comparator: for each column id it compares the corresponding
/// field with `SPACESHIP_NUMBER` / `SPACESHIP_NULLSTR` /
/// `SPACESHIP_DEFAULTSTR` / [`compareRealNumbers`], line-for-line with
/// the C switch.
///
/// The `COMM` case delegates to [`Process_getCommand`], which reads the
/// modeled `Settings` via the `Row::host` back-pointer; every other arm
/// is pure. The C `default:` (an `assert(0)` "should never be reached"
/// path that still returns a PID compare) maps to the `_` arm.
pub fn Process_compareByKey_Base(p1: &Process, p2: &Process, key: RowField) -> i32 {
    // `key` is a RowField (int) so it can carry platform field ids from any
    // platform; the reserved fields this handles are matched against their
    // shared `ProcessField` discriminants, and anything else (a dynamic or
    // other-platform id) falls to the `_` default — a PID compare, matching C.
    use ProcessField as PF;
    match key {
        k if k == PF::PERCENT_CPU as RowField || k == PF::PERCENT_NORM_CPU as RowField => {
            compareRealNumbers(p1.percent_cpu as f64, p2.percent_cpu as f64)
        }
        k if k == PF::PERCENT_MEM as RowField => spaceship_number!(p1.m_resident, p2.m_resident),
        k if k == PF::COMM as RowField => {
            spaceship_nullstr!(Process_getCommand(p1), Process_getCommand(p2))
        }
        k if k == PF::PROC_COMM as RowField => {
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
        k if k == PF::PROC_EXE as RowField => {
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
        k if k == PF::CWD as RowField => spaceship_nullstr!(
            p1.procCwd.as_deref().map(str::as_bytes),
            p2.procCwd.as_deref().map(str::as_bytes)
        ),
        k if k == PF::ELAPSED as RowField => {
            let r = -spaceship_number!(p1.starttime_ctime, p2.starttime_ctime);
            if r != 0 {
                r
            } else {
                spaceship_number!(Process_getPid(p1), Process_getPid(p2))
            }
        }
        k if k == PF::MAJFLT as RowField => spaceship_number!(p1.majflt, p2.majflt),
        k if k == PF::MINFLT as RowField => spaceship_number!(p1.minflt, p2.minflt),
        k if k == PF::M_RESIDENT as RowField => spaceship_number!(p1.m_resident, p2.m_resident),
        k if k == PF::M_VIRT as RowField => spaceship_number!(p1.m_virt, p2.m_virt),
        k if k == PF::NICE as RowField => spaceship_number!(p1.nice, p2.nice),
        k if k == PF::NLWP as RowField => spaceship_number!(p1.nlwp, p2.nlwp),
        k if k == PF::PGRP as RowField => spaceship_number!(p1.pgrp, p2.pgrp),
        k if k == PF::PID as RowField => spaceship_number!(Process_getPid(p1), Process_getPid(p2)),
        k if k == PF::PPID as RowField => {
            spaceship_number!(Process_getParent(p1), Process_getParent(p2))
        }
        k if k == PF::PRIORITY as RowField => spaceship_number!(p1.priority, p2.priority),
        k if k == PF::PROCESSOR as RowField => spaceship_number!(p1.processor, p2.processor),
        k if k == PF::SCHEDULERPOLICY as RowField => {
            spaceship_number!(p1.scheduling_policy, p2.scheduling_policy)
        }
        k if k == PF::SESSION as RowField => spaceship_number!(p1.session, p2.session),
        k if k == PF::STARTTIME as RowField => {
            let r = spaceship_number!(p1.starttime_ctime, p2.starttime_ctime);
            if r != 0 {
                r
            } else {
                spaceship_number!(Process_getPid(p1), Process_getPid(p2))
            }
        }
        k if k == PF::STATE as RowField => spaceship_number!(p1.state as i32, p2.state as i32),
        k if k == PF::ST_UID as RowField => spaceship_number!(p1.st_uid, p2.st_uid),
        k if k == PF::TIME as RowField => spaceship_number!(p1.time, p2.time),
        k if k == PF::TGID as RowField => {
            spaceship_number!(Process_getThreadGroup(p1), Process_getThreadGroup(p2))
        }
        k if k == PF::TPGID as RowField => spaceship_number!(p1.tpgid, p2.tpgid),
        k if k == PF::TTY as RowField => {
            /* Order no tty last */
            spaceship_defaultstr!(
                p1.tty_name.as_deref().map(str::as_bytes),
                p2.tty_name.as_deref().map(str::as_bytes),
                b"\x7f"
            )
        }
        k if k == PF::USER as RowField => spaceship_nullstr!(
            p1.user.as_deref().map(str::as_bytes),
            p2.user.as_deref().map(str::as_bytes)
        ),
        // C default: `assert(0)` "should never be reached" — returns a
        // PID compare. Reached only for NULL_FIELD or a non-reserved key.
        _ => {
            // CRT_debug("Process_compareByKey_Base() called with key %d", key);
            crate::CRT_debug!("Process_compareByKey_Base() called with key {}", key);
            spaceship_number!(Process_getPid(p1), Process_getPid(p2))
        }
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
/// from `Process.c:1043`. No-op when both the stored `procComm` and the
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
/// `Process.c:1056`. If `cmdline` starts with `/`, scans up to `end`
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

/// Port of `void Process_updateCmdline(Process* this, const char* cmdline,
/// size_t basenameStart, size_t basenameEnd)` from `Process.c:1077`.
///
/// No-op when both the stored and new `cmdline` are `None` (C `NULL`), or
/// when both are present and equal (`String_eq`, inlined as `==`).
/// Otherwise it replaces `cmdline` and recomputes the basename window.
/// Kernel threads have no basename, so both offsets reset to 0
/// (`Process_isKernelThread`). Otherwise `cmdlineBasenameStart` is the
/// caller's `basenameStart` when it is nonzero (or when `cmdline` is
/// `NULL`), else the heuristic [`skipPotentialPath`] over the new cmdline
/// bounded by `basenameEnd`; `cmdlineBasenameEnd` is `basenameEnd`
/// verbatim. Finally the merged-command cache marker is reset.
///
/// The three C `assert`s (`basenameStart`/`basenameEnd` vs
/// `strlen(cmdline)`) become `debug_assert!`s, with `strlen` mapped to the
/// UTF-8 byte length `c.len()` — the same `char`-index basis the C uses.
/// The C `free(this->cmdline)` is implicit (the old `String` drops when the
/// field is overwritten); `skipPotentialPath` runs on the new cmdline's
/// bytes, only ever reached when `cmdline` is `Some`.
pub fn Process_updateCmdline(
    this: &mut Process,
    cmdline: Option<&str>,
    basenameStart: usize,
    basenameEnd: usize,
) {
    debug_assert!(cmdline.map_or(basenameStart == 0, |c| basenameStart < c.len()));
    debug_assert!(basenameEnd > basenameStart || (basenameEnd == 0 && basenameStart == 0));
    debug_assert!(cmdline.map_or(basenameEnd == 0, |c| basenameEnd <= c.len()));

    if this.cmdline.is_none() && cmdline.is_none() {
        return;
    }

    if let (Some(cur), Some(new)) = (&this.cmdline, cmdline) {
        if cur == new {
            return;
        }
    }

    if Process_isKernelThread(this) {
        // kernel threads have no basename
        this.cmdlineBasenameStart = 0;
        this.cmdlineBasenameEnd = 0;
    } else {
        // C: (basenameStart || !cmdline) ? basenameStart
        //                                 : skipPotentialPath(cmdline, basenameEnd)
        this.cmdlineBasenameStart = match cmdline {
            Some(c) if basenameStart == 0 => skipPotentialPath(c.as_bytes(), basenameEnd),
            _ => basenameStart,
        };
        this.cmdlineBasenameEnd = basenameEnd;
    }

    this.cmdline = cmdline.map(|s| s.to_string());

    this.mergedCommand.lastUpdate = 0;
}

/// Port of `void Process_updateExe(Process* this, const char* exe)` from
/// `Process.c:1102`. No-op when both the stored `procExe` and the new
/// `exe` are `None` (C `NULL`), or when both are present and equal
/// (`String_eq`, inlined as `==`). Otherwise it replaces `procExe`
/// (`xStrdup(exe)` → an owned `String`, `NULL` → `None`), recomputes the
/// basename offset, and resets the merged-command cache marker.
///
/// The C `strrchr(exe, '/')` + guard
/// `lastSlash && *(lastSlash + 1) != '\0' && lastSlash != exe` maps to
/// [`str::rfind`] on the ASCII `'/'`: a match at byte `pos` yields
/// `pos + 1` only when the slash is neither the final byte
/// (`*(lastSlash + 1) != '\0'` → `pos + 1 < exe.len()`) nor the first
/// (`lastSlash != exe` → `pos != 0`); every other case yields `0`. Byte
/// lengths (`exe.len()`) preserve the C `char`-index arithmetic. The C
/// `free(this->procExe)` is implicit — the old `String` is dropped when
/// the field is overwritten.
pub fn Process_updateExe(this: &mut Process, exe: Option<&str>) {
    if this.procExe.is_none() && exe.is_none() {
        return;
    }

    if let (Some(cur), Some(new)) = (&this.procExe, exe) {
        if cur == new {
            return;
        }
    }

    match exe {
        Some(exe) => {
            let last_slash = exe.rfind('/');
            this.procExeBasenameOffset = match last_slash {
                Some(pos) if pos + 1 < exe.len() && pos != 0 => pos + 1,
                _ => 0,
            };
            this.procExe = Some(exe.to_string());
        }
        None => {
            this.procExe = None;
            this.procExeBasenameOffset = 0;
        }
    }

    this.mergedCommand.lastUpdate = 0;
}

/// Port of `void Process_updateCPUFieldWidths(float percentage)` from
/// `Process.c:1122`. Grows the `PERCENT_CPU` / `PERCENT_NORM_CPU` column
/// widths to fit the largest CPU% seen: 4 below 99.9%, else enough digits for
/// the integer part plus two chars (the `.` and one precision digit).
pub fn Process_updateCPUFieldWidths(percentage: f32) {
    // C `!isgreaterequal(percentage, 99.9F)` — false for NaN (quiet compare).
    if !(percentage >= 99.9_f32) {
        Row_updateFieldWidth(ProcessField::PERCENT_CPU as RowField, 4);
        Row_updateFieldWidth(ProcessField::PERCENT_NORM_CPU as RowField, 4);
        return;
    }

    // Two extra characters: one for "." and another for precision. C computes
    // `ceil(log10(percentage + 0.1)) + 2` in floating point and truncates to
    // the `uint8_t width`; keep the whole expression in `f32` and cast to `u8`
    // last (matching the C type) so a non-finite `percentage` — e.g. the first
    // darwin sample's zero-delta CPU% — saturates to `u8::MAX` instead of
    // overflowing an intermediate `i32 + 2`.
    let width = ((percentage + 0.1).log10().ceil() + 2.0) as u8 as usize;
    Row_updateFieldWidth(ProcessField::PERCENT_CPU as RowField, width);
    Row_updateFieldWidth(ProcessField::PERCENT_NORM_CPU as RowField, width);
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
    fn process_reaches_row_and_process_via_accessors_not_any_downcast() {
        // Regression: panel items are platform `Process` objects (a bare
        // `Process` here; `DarwinProcess`/`LinuxProcess` in the app), so an
        // exact-type `Any` downcast to `Row`/`Process` MISSES the concrete
        // type. That silently broke kill-signal delivery, untag-all, tree
        // tag/expand/collapse, MainPanel_selectedRow, affinity, and scheduling.
        // The `as_row()`/`as_process()` vtable accessors are the correct upcast;
        // existing tests only used bare `Row` fixtures, so they never caught it.
        use core::any::Any;
        let mut p = Process::default();
        p.super_.id = 4242;
        let obj: &dyn Object = &p;
        assert!(
            (obj as &dyn Any).downcast_ref::<Row>().is_none(),
            "exact-type Any downcast to Row must miss a Process"
        );
        assert_eq!(obj.as_row().map(|r| r.id), Some(4242));
        assert!(obj.as_process().is_some());
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
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PID as RowField),
            -1
        );
        assert_eq!(
            Process_compareByKey_Base(&b, &a, ProcessField::PID as RowField),
            1
        );
        assert_eq!(
            Process_compareByKey_Base(&a, &a, ProcessField::PID as RowField),
            0
        );
    }

    #[test]
    fn compare_percent_cpu_float_and_nan() {
        let mut a = proc();
        let mut b = proc();
        a.percent_cpu = 1.0;
        b.percent_cpu = 2.0;
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PERCENT_CPU as RowField),
            -1
        );
        // PERCENT_NORM_CPU compares the same field.
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PERCENT_NORM_CPU as RowField),
            -1
        );
        // NaN is ordered less than any value (compareRealNumbers).
        a.percent_cpu = f32::NAN;
        b.percent_cpu = 1.0;
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PERCENT_CPU as RowField),
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
            Process_compareByKey_Base(&a, &b, ProcessField::PERCENT_MEM as RowField),
            -1
        );
    }

    #[test]
    fn compare_state_by_enum_order() {
        let mut a = proc();
        let mut b = proc();
        a.state = ProcessState::RUNNING; // 3
        b.state = ProcessState::SLEEPING; // 14
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::STATE as RowField),
            -1
        );
        assert_eq!(
            Process_compareByKey_Base(&a, &a, ProcessField::STATE as RowField),
            0
        );
    }

    #[test]
    fn compare_proc_comm_string() {
        let mut a = proc();
        let mut b = proc();
        a.procComm = Some("alpha".to_string());
        b.procComm = Some("beta".to_string());
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::PROC_COMM as RowField),
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
            Process_compareByKey_Base(&a, &b, ProcessField::PROC_COMM as RowField),
            -1
        );
        // Non-kernel with no procComm => "" which sorts before "aaa".
        let c = proc(); // not kernel, procComm None => ""
        assert_eq!(
            Process_compareByKey_Base(&c, &b, ProcessField::PROC_COMM as RowField),
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
            Process_compareByKey_Base(&a, &b, ProcessField::STARTTIME as RowField),
            -1
        );
        // Distinct starttime dominates.
        b.starttime_ctime = 200;
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::STARTTIME as RowField),
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
            Process_compareByKey_Base(&a, &b, ProcessField::ELAPSED as RowField),
            -1
        );
        // Equal starttime => tie-break by pid.
        b.starttime_ctime = 200;
        Process_setPid(&mut a, 3);
        Process_setPid(&mut b, 8);
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::ELAPSED as RowField),
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
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::TTY as RowField),
            -1
        );
        // Two missing ttys compare equal (both "\x7f").
        let c = proc();
        assert_eq!(
            Process_compareByKey_Base(&b, &c, ProcessField::TTY as RowField),
            0
        );
    }

    #[test]
    fn compare_user_nullstr() {
        let mut a = proc();
        let mut b = proc();
        a.user = Some("alice".to_string());
        b.user = Some("bob".to_string());
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::USER as RowField),
            -1
        );
        // NULL user compares as "" (sorts first).
        let c = proc();
        assert_eq!(
            Process_compareByKey_Base(&c, &a, ProcessField::USER as RowField),
            -1
        );
    }

    #[test]
    fn compare_default_arm_falls_back_to_pid() {
        // NULL_FIELD hits the `_` (C default/assert) arm => pid compare.
        let mut a = proc();
        let mut b = proc();
        Process_setPid(&mut a, 1);
        Process_setPid(&mut b, 2);
        assert_eq!(
            Process_compareByKey_Base(&a, &b, ProcessField::NULL_FIELD as RowField),
            -1
        );
    }

    #[test]
    fn process_klass_chain_extends_row() {
        // Process_class -> Row_class -> Object_class.
        let p = proc();
        assert!(core::ptr::eq(p.klass(), &Process_class.super_.super_));
        assert!(crate::ported::object::Object_isA(
            Some(&p as &dyn Object),
            &Process_class
        ));
        assert!(crate::ported::object::Object_isA(
            Some(&p as &dyn Object),
            &Row_class
        ));
    }

    // ── Process_updateExe (Process.c:1102) ────────────────────────────
    #[test]
    fn update_exe_both_none_is_noop() {
        let mut p = proc();
        p.mergedCommand.lastUpdate = 7; // must survive the early return
        Process_updateExe(&mut p, None);
        assert_eq!(p.procExe, None);
        assert_eq!(p.mergedCommand.lastUpdate, 7);
    }

    #[test]
    fn update_exe_equal_string_is_noop() {
        let mut p = proc();
        p.procExe = Some("/usr/bin/htop".to_string());
        p.procExeBasenameOffset = 999; // stale marker preserved by the no-op
        p.mergedCommand.lastUpdate = 7;
        Process_updateExe(&mut p, Some("/usr/bin/htop"));
        assert_eq!(p.procExe.as_deref(), Some("/usr/bin/htop"));
        assert_eq!(p.procExeBasenameOffset, 999);
        assert_eq!(p.mergedCommand.lastUpdate, 7);
    }

    #[test]
    fn update_exe_sets_basename_offset_past_last_slash() {
        let mut p = proc();
        p.mergedCommand.lastUpdate = 7;
        Process_updateExe(&mut p, Some("/usr/bin/htop"));
        assert_eq!(p.procExe.as_deref(), Some("/usr/bin/htop"));
        // last '/' is byte 8; offset is 9 (start of "htop").
        assert_eq!(p.procExeBasenameOffset, 9);
        assert_eq!(&"/usr/bin/htop"[p.procExeBasenameOffset..], "htop");
        assert_eq!(p.mergedCommand.lastUpdate, 0); // cache invalidated
    }

    #[test]
    fn update_exe_trailing_slash_yields_zero_offset() {
        // strrchr finds the final '/', but *(lastSlash+1) == '\0' -> 0.
        let mut p = proc();
        Process_updateExe(&mut p, Some("/usr/bin/"));
        assert_eq!(p.procExeBasenameOffset, 0);
    }

    #[test]
    fn update_exe_leading_slash_only_yields_zero_offset() {
        // Slash at position 0: lastSlash == exe -> 0.
        let mut p = proc();
        Process_updateExe(&mut p, Some("/init"));
        assert_eq!(p.procExeBasenameOffset, 0);
    }

    #[test]
    fn update_exe_no_slash_yields_zero_offset() {
        let mut p = proc();
        Process_updateExe(&mut p, Some("bash"));
        assert_eq!(p.procExe.as_deref(), Some("bash"));
        assert_eq!(p.procExeBasenameOffset, 0);
    }

    #[test]
    fn update_exe_clears_when_new_is_none() {
        let mut p = proc();
        p.procExe = Some("/usr/bin/htop".to_string());
        p.procExeBasenameOffset = 9;
        p.mergedCommand.lastUpdate = 7;
        Process_updateExe(&mut p, None);
        assert_eq!(p.procExe, None);
        assert_eq!(p.procExeBasenameOffset, 0);
        assert_eq!(p.mergedCommand.lastUpdate, 0);
    }

    // ── Process_updateCmdline (Process.c:1077) ────────────────────────
    #[test]
    fn update_cmdline_both_none_is_noop() {
        let mut p = proc();
        p.mergedCommand.lastUpdate = 7;
        Process_updateCmdline(&mut p, None, 0, 0);
        assert_eq!(p.cmdline, None);
        assert_eq!(p.mergedCommand.lastUpdate, 7);
    }

    #[test]
    fn update_cmdline_equal_string_is_noop() {
        let mut p = proc();
        p.cmdline = Some("/bin/sh -c foo".to_string());
        p.cmdlineBasenameStart = 5;
        p.mergedCommand.lastUpdate = 7;
        Process_updateCmdline(&mut p, Some("/bin/sh -c foo"), 5, 7);
        assert_eq!(p.cmdlineBasenameStart, 5); // untouched by the no-op
        assert_eq!(p.mergedCommand.lastUpdate, 7);
    }

    #[test]
    fn update_cmdline_honours_explicit_basename_start() {
        // basenameStart != 0 -> used verbatim, no skipPotentialPath.
        let mut p = proc();
        Process_updateCmdline(&mut p, Some("/usr/bin/htop --tree"), 9, 13);
        assert_eq!(p.cmdline.as_deref(), Some("/usr/bin/htop --tree"));
        assert_eq!(p.cmdlineBasenameStart, 9);
        assert_eq!(p.cmdlineBasenameEnd, 13);
        assert_eq!(p.mergedCommand.lastUpdate, 0);
    }

    #[test]
    fn update_cmdline_derives_basename_via_skip_potential_path() {
        // basenameStart == 0 and non-kernel -> skipPotentialPath("/usr/bin/htop", 13).
        let mut p = proc();
        Process_updateCmdline(&mut p, Some("/usr/bin/htop"), 0, 13);
        // last path component starts just past "/usr/bin/".
        assert_eq!(p.cmdlineBasenameStart, 9);
        assert_eq!(p.cmdlineBasenameEnd, 13);
    }

    #[test]
    fn update_cmdline_kernel_thread_has_no_basename() {
        let mut p = proc();
        p.isKernelThread = true;
        Process_updateCmdline(&mut p, Some("[kworker/0:0]"), 1, 8);
        assert_eq!(p.cmdlineBasenameStart, 0);
        assert_eq!(p.cmdlineBasenameEnd, 0);
        assert_eq!(p.cmdline.as_deref(), Some("[kworker/0:0]"));
    }

    // ── Process_sendSignal / rowSendSignal (Process.c:904/908) ────────
    #[test]
    fn send_signal_zero_to_self_succeeds() {
        // signal 0 performs no delivery, only an error/permission check,
        // so kill(self, 0) must return 0 (success).
        let mut p = proc();
        Process_setPid(&mut p, unsafe { libc::getpid() });
        assert!(Process_sendSignal(&p, Arg::I(0)));
    }

    #[test]
    #[should_panic]
    fn send_signal_rejects_arg_v() {
        let p = proc();
        let _ = Process_sendSignal(&p, Arg::V(core::ptr::null_mut()));
    }

    #[test]
    fn row_send_signal_delegates_through_object_isa() {
        let mut p = proc();
        Process_setPid(&mut p, unsafe { libc::getpid() });
        assert!(Process_rowSendSignal(&mut p as &mut dyn Object, Arg::I(0)));
    }

    #[test]
    #[should_panic]
    fn row_send_signal_rejects_non_process() {
        // A bare Row's class chain is Row -> Object, never Process.
        let mut row = crate::ported::row::Row::default();
        let _ = Process_rowSendSignal(&mut row as &mut dyn Object, Arg::I(0));
    }

    #[test]
    #[should_panic]
    fn row_change_priority_rejects_non_process() {
        // Guard fires (downcast) before any setpriority syscall runs.
        let mut row = crate::ported::row::Row::default();
        let _ = Process_rowChangePriorityBy(&mut row as &mut dyn Object, Arg::I(1));
    }

    #[test]
    fn fill_starttime_buffer_formats_recent_start_as_hh_mm() {
        // `now` is derived from the host Machine's realtimeMs; a start one
        // minute ago selects the "%R " (HH:MM) format.
        let mut machine = Machine::default();
        machine.realtimeMs = 1_700_000_000_000; // arbitrary epoch ms
        let now_secs = (machine.realtimeMs / 1000) as i64;

        let mut p = proc();
        p.super_.host = &machine as *const Machine as *const c_void;
        p.starttime_ctime = now_secs - 60;

        Process_fillStarttimeBuffer(&mut p);

        let end = p
            .starttime_show
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(p.starttime_show.len());
        let shown = std::str::from_utf8(&p.starttime_show[..end]).unwrap();
        assert!(!shown.is_empty());
        // "%R " is HH:MM followed by a space.
        assert!(shown.contains(':'));
    }

    #[test]
    fn fill_starttime_buffer_formats_old_start_as_year() {
        let mut machine = Machine::default();
        machine.realtimeMs = 1_700_000_000_000;
        let now_secs = (machine.realtimeMs / 1000) as i64;

        let mut p = proc();
        p.super_.host = &machine as *const Machine as *const c_void;
        // Two years ago -> " %Y " (the year).
        p.starttime_ctime = now_secs - 2 * 365 * 86400;

        Process_fillStarttimeBuffer(&mut p);

        let end = p
            .starttime_show
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(p.starttime_show.len());
        let shown = std::str::from_utf8(&p.starttime_show[..end])
            .unwrap()
            .trim();
        // A 4-digit year.
        assert_eq!(shown.len(), 4);
        assert!(shown.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn update_cmdline_clears_when_new_is_none() {
        let mut p = proc();
        p.cmdline = Some("/bin/sh".to_string());
        p.mergedCommand.lastUpdate = 7;
        Process_updateCmdline(&mut p, None, 0, 0);
        assert_eq!(p.cmdline, None);
        // non-kernel path with cmdline None -> basenameStart stays the passed 0.
        assert_eq!(p.cmdlineBasenameStart, 0);
        assert_eq!(p.cmdlineBasenameEnd, 0);
        assert_eq!(p.mergedCommand.lastUpdate, 0);
    }

    /// [`Process_writeCommand`]: the cached merged-command string is appended
    /// verbatim; with no cached string and `showProgramPath == false` +
    /// `highlightBaseName`, only the basename of the cmdline is rendered.
    #[test]
    fn write_command_merged_and_cmdline_basename() {
        use crate::ported::settings::Settings;

        // Merged-command path: appended verbatim (7 chars).
        let mut machine = Machine::default();
        machine.settings = Some(Settings::default());
        let mut p = Process::default();
        p.super_.host = &machine as *const Machine as *const c_void;
        p.mergedCommand.str = Some("foo bar".to_string());
        p.mergedCommand.highlightCount = 0;
        let mut rs = RichString::default();
        Process_writeCommand(&p, 0, 0, &mut rs);
        assert_eq!(RichString_size(&rs), 7);

        // Cmdline path: "/bin/ls" trimmed to basename "ls" (2 chars).
        let mut settings = Settings::default();
        settings.highlightBaseName = true;
        settings.showProgramPath = false;
        machine.settings = Some(settings);
        let mut p = Process::default();
        p.super_.host = &machine as *const Machine as *const c_void;
        p.cmdline = Some("/bin/ls".to_string());
        p.cmdlineBasenameEnd = 7;
        let mut rs = RichString::default();
        Process_writeCommand(&p, 0, 0, &mut rs);
        assert_eq!(RichString_size(&rs), 2);
    }

    /// [`Process_writeField`] renders representative `break`-arm fields to the
    /// expected visible width. Uses fields that don't read the process-wide
    /// pid/uid digit globals, so the assertions are race-free.
    #[test]
    fn write_field_renders_representative_fields() {
        use crate::ported::settings::Settings;

        let mut machine = Machine::default();
        machine.settings = Some(Settings::default());
        let mut p = Process::default();
        p.super_.host = &machine as *const Machine as *const c_void;

        let render = |p: &Process, field: RowField| -> i32 {
            let mut rs = RichString::default();
            Process_writeField(p, &mut rs, field);
            RichString_size(&rs)
        };

        // NICE = 0 → "  0 " (4 visible cols).
        p.nice = 0;
        assert_eq!(render(&p, ProcessField::NICE as RowField), 4);
        // NICE unknown → "N/A " (4).
        p.nice = PROCESS_NICE_UNKNOWN;
        assert_eq!(render(&p, ProcessField::NICE as RowField), 4);
        // PRIORITY <= -100 → " RT " (4).
        p.priority = -100;
        assert_eq!(render(&p, ProcessField::PRIORITY as RowField), 4);
        // STATE running → "R " (2).
        p.state = ProcessState::RUNNING;
        assert_eq!(render(&p, ProcessField::STATE as RowField), 2);
    }

    /// [`Process_isVisible`]: userland threads are hidden only when
    /// `hideUserlandThreads` is set; everything else is always visible.
    #[test]
    fn is_visible_gates_userland_threads() {
        use crate::ported::settings::Settings;

        let mut p = Process::default();
        p.isUserlandThread = true;

        let mut s = Settings::default();
        s.hideUserlandThreads = false;
        assert!(Process_isVisible(&p, &s)); // not hiding → visible

        s.hideUserlandThreads = true;
        assert!(!Process_isVisible(&p, &s)); // hiding → thread hidden
        p.isUserlandThread = false;
        assert!(Process_isVisible(&p, &s)); // non-thread stays visible
    }

    /// [`Process_getCommand`]: returns the merged command when present, the
    /// raw cmdline otherwise, and always the cmdline for a userland thread
    /// when `showThreadNames` is set.
    #[test]
    fn get_command_picks_cmdline_or_merged() {
        use crate::ported::settings::Settings;

        let mut machine = Machine::default();
        machine.settings = Some(Settings::default());
        let mut p = Process::default();
        p.super_.host = &machine as *const Machine as *const c_void;
        p.cmdline = Some("cmd".to_string());
        p.mergedCommand.str = Some("merged".to_string());

        // Non-thread, merged present → merged.
        assert_eq!(Process_getCommand(&p), Some(b"merged".as_slice()));
        // No merged → cmdline.
        p.mergedCommand.str = None;
        assert_eq!(Process_getCommand(&p), Some(b"cmd".as_slice()));

        // Userland thread + showThreadNames → cmdline even with a merged str.
        let mut s = Settings::default();
        s.showThreadNames = true;
        machine.settings = Some(s);
        p.mergedCommand.str = Some("merged".to_string());
        p.isUserlandThread = true;
        assert_eq!(Process_getCommand(&p), Some(b"cmd".as_slice()));
    }

    /// [`Process_makeCommandStr`] fallback path (no exe/comm): builds the
    /// merged string from the cmdline and highlights the basename — full path
    /// under `showProgramPath`, basename-only otherwise.
    #[test]
    fn make_command_str_fallback_cmdline() {
        use crate::ported::settings::Settings;

        // "/usr/bin/foo bar": basename "foo" at bytes [9, 12).
        let mk = |show_program_path: bool| -> Process {
            let mut s = Settings::default();
            s.showProgramPath = show_program_path;
            s.lastUpdate = 1;
            let mut p = Process::default();
            p.state = ProcessState::RUNNING;
            p.cmdline = Some("/usr/bin/foo bar".to_string());
            p.cmdlineBasenameStart = 9;
            p.cmdlineBasenameEnd = 12;
            Process_makeCommandStr(&mut p, &s);
            p
        };

        // showProgramPath = true → full path, basename highlight at offset 9.
        let p = mk(true);
        assert_eq!(p.mergedCommand.str.as_deref(), Some("/usr/bin/foo bar"));
        assert!(p.mergedCommand.highlightCount >= 1);
        let hl = &p.mergedCommand.highlights[0];
        assert_eq!(
            (hl.offset, hl.length, hl.flags),
            (9, 3, CMDLINE_HIGHLIGHT_FLAG_BASENAME)
        );

        // showProgramPath = false → basename only, highlight at offset 0.
        let p = mk(false);
        assert_eq!(p.mergedCommand.str.as_deref(), Some("foo bar"));
        let hl = &p.mergedCommand.highlights[0];
        assert_eq!(
            (hl.offset, hl.length, hl.flags),
            (0, 3, CMDLINE_HIGHLIGHT_FLAG_BASENAME)
        );
    }

    /// [`Process_makeCommandStr`] merged path: a `comm` that is a proper prefix
    /// of a longer exe basename must NOT be treated as "comm in exe". The C uses
    /// a FIXED `strncmp(procExe + off, procComm, TASK_COMM_LEN - 1)` (not the
    /// cmdline path's `MINIMUM(15, strlen)`), so `strncmp("foobar", "foo", 15)`
    /// fails at comm's NUL vs the exe's `'b'`. The comm is therefore emitted as
    /// its own `│`-separated field, not folded into the exe basename.
    #[test]
    fn make_command_str_comm_prefix_of_exe_is_a_separate_field() {
        use crate::ported::settings::Settings;

        let mut s = Settings::default();
        s.showMergedCommand = true;
        s.showProgramPath = false;
        s.findCommInCmdline = false;
        s.stripExeFromCmdline = false;
        s.shadowDistPathPrefix = false;
        s.lastUpdate = 1;

        let mut p = Process::default();
        p.state = ProcessState::RUNNING;
        p.procExe = Some("/usr/bin/foobar".to_string()); // basename "foobar" at 9
        p.procExeBasenameOffset = 9;
        p.procComm = Some("foo".to_string()); // a proper prefix of "foobar"
        p.cmdline = Some("somethingelse".to_string());
        p.cmdlineBasenameStart = 0;

        Process_makeCommandStr(&mut p, &s);
        // "foobar" + sep + "foo" (separate comm field) + sep + cmdline, where the
        // separator is TREE_STR_VERT ("|" in the ASCII/non-UTF-8 test env). The
        // pre-fix prefix-match folded comm in, yielding "foobar|somethingelse".
        assert_eq!(
            p.mergedCommand.str.as_deref(),
            Some("foobar|foo|somethingelse")
        );
    }

    /// [`Process_updateCPUFieldWidths`]: never shrinks below 4, and grows for
    /// out-of-range percentages (the width is monotonic via
    /// [`Row_updateFieldWidth`]).
    #[test]
    fn update_cpu_field_widths_floor_and_growth() {
        Process_updateCPUFieldWidths(50.0);
        let w = Row_fieldWidths[ProcessField::PERCENT_CPU as usize].load(Ordering::Relaxed);
        assert!(w >= 4);
        // 999.9% → ceil(log10(1000)) + 2 = 5.
        Process_updateCPUFieldWidths(999.9);
        let w2 = Row_fieldWidths[ProcessField::PERCENT_CPU as usize].load(Ordering::Relaxed);
        assert!(w2 >= 5);
    }
}
