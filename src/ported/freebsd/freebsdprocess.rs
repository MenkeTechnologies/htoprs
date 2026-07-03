//! Port of `FreeBSDProcess.c` — the FreeBSD process object.
//!
//! Ported (on the base [`Process`] + [`Process_init`] + the modeled
//! [`FreeBSDProcess`] struct / [`FreeBSDProcess_class`] vtable):
//! - the [`FreeBSDProcess`] object struct (`Process super` + `jid`/`jname`/
//!   `emul`/`sched_class`) and its [`Object`] / [`ProcessClass`] wiring.
//! - [`FreeBSDProcess_new`] (`FreeBSDProcess.c:62`).
//! - the leaf column renderer [`FreeBSDProcess_rowWriteField`]
//!   (`FreeBSDProcess.c:85`) and the comparator
//!   [`FreeBSDProcess_compareByKey`] (`FreeBSDProcess.c:120`) — the
//!   `JID`/`JAIL`/`EMULATION`/`SCHEDCLASS` platform fields plus base-field
//!   delegation.
//!
//! Still `todo!()`:
//! - `Process_delete` is a pure `free()` teardown (`Process_done` + `free` of
//!   `emul`/`jname`); Rust `Drop` reclaims the [`FreeBSDProcess`] allocation
//!   and its `Option<String>` fields (the darwin/linux `Process_delete`
//!   precedent), so there is no faithful safe-Rust analog.
//!
//! The `Process_fields[]` field-descriptor table (`FreeBSDProcess.c:24`) is
//! data, not a function, and is deferred until the FreeBSD `ProcessField`
//! layer is modeled.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)] // faithful C enum variant names (SCHEDCLASS_*)
#![allow(non_upper_case_globals)] // faithful C global name (FreeBSDProcess_class)
#![allow(dead_code)]

use core::any::Any;

use crate::ported::crt::{ColorElements as CE, ColorScheme};
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::process::{
    spaceship_nullstr, Process, ProcessClass, ProcessField, ProcessFieldData, Process_class,
    Process_compareByKey_Base, Process_compareByParent, Process_init, Process_rowGetSortKey,
    Process_rowIsHighlighted, Process_rowIsVisible, Process_writeField, PROCESS_FLAG_CWD,
    PROCESS_FLAG_SCHEDPOL,
};
use crate::ported::richstring::{RichString, RichString_appendWide};
use crate::ported::row::{
    spaceship_number, Row, RowClass, Row_pidDigits, Row_printLeftAlignedField,
};
use crate::ported::settings::RowField;
use std::os::raw::c_void;
use std::sync::atomic::Ordering;

/// Port of `typedef enum { ... } FreeBSDSchedClass` (`FreeBSDProcess.h:16`).
/// Values are load-bearing: they index [`FreeBSD_schedclassChars`] and are
/// compared numerically by [`FreeBSDProcess_compareByKey`].
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum FreeBSDSchedClass {
    SCHEDCLASS_UNKNOWN = 0,
    SCHEDCLASS_INTR_THREAD,
    SCHEDCLASS_REALTIME,
    SCHEDCLASS_TIMESHARE,
    SCHEDCLASS_IDLE,
    // MAX_SCHEDCLASS (enum terminator; = 5) — not a real class.
}

/// Port of htop's `struct FreeBSDProcess_` (`FreeBSDProcess.h:27`). "Extends"
/// [`Process`] via the embedded `super_` field (htop's `Process super;` first
/// member); the remaining fields are the FreeBSD-only per-process data.
///
/// `#[repr(C)]` guarantees `super_` sits at offset 0, so htop's
/// `(FreeBSDProcess*)super` downcast — a `*const Process`/`*const Row`
/// obtained from a `FreeBSDProcess`, cast back — is sound (see the layout
/// test). `jname`/`emul` are owned `Option<String>` (C's `char*`, `NULL` =
/// `None`).
#[repr(C)]
pub struct FreeBSDProcess {
    /// C `Process super` — the embedded base process.
    pub super_: Process,
    /// C `int jid` — jail prison ID.
    pub jid: i32,
    /// C `char* jname` — jail prison name.
    pub jname: Option<String>,
    /// C `char* emul` — ABI / syscall emulation environment.
    pub emul: Option<String>,
    /// C `FreeBSDSchedClass sched_class`.
    pub sched_class: FreeBSDSchedClass,
}

/// Port of `static const char FreeBSD_schedclassChars[MAX_SCHEDCLASS]`
/// (`FreeBSDProcess.c:77`), the per-class display character. Written in
/// index order (the C uses designated initializers): `[UNKNOWN]='?'`,
/// `[INTR_THREAD]='-'`, `[REALTIME]='r'`, `[TIMESHARE]=' '`, `[IDLE]='i'`.
const FreeBSD_schedclassChars: [u8; 5] = [b'?', b'-', b'r', b' ', b'i'];

/// `JID = 100` (`freebsd/ProcessField.h:12`) — the FreeBSD platform
/// `ProcessField` ids, spliced into the C `ReservedFields` enum by
/// `PLATFORM_PROCESS_FIELDS`. Modeled as local [`RowField`] constants (data,
/// not functions): the shared [`ProcessField`](crate::ported::process::ProcessField)
/// enum reserves ids `100..` for other platforms, so the FreeBSD ids cannot
/// live on that enum; the `match` arms below compare the raw field id exactly
/// as the C `switch` does.
const JID: RowField = 100;
/// `JAIL = 101` (`freebsd/ProcessField.h:13`).
const JAIL: RowField = 101;
/// `EMULATION = 102` (`freebsd/ProcessField.h:14`).
const EMULATION: RowField = 102;
/// `SCHEDCLASS = 103` (`freebsd/ProcessField.h:15`).
const SCHEDCLASS: RowField = 103;

/// Port of `#define LAST_PROCESSFIELD LAST_RESERVED_FIELD` (`Process.h:229`) for
/// the FreeBSD build. `RowField.h` splices `PLATFORM_PROCESS_FIELDS` in before
/// `LAST_RESERVED_FIELD`; on FreeBSD that macro ends with
/// `DUMMY_BUMP_FIELD = CWD` (`freebsd/ProcessField.h`), so the enum counter
/// lands on `CWD + 1 = 127`. Also the length of [`Process_fields`].
pub const LAST_PROCESSFIELD: usize = 127;

/// `const fn` builder for one populated [`ProcessFieldData`] entry, keeping the
/// table a faithful transcription of the C designated initializers. Mirrors the
/// linux/darwin ports' `pfd` helper.
#[allow(clippy::too_many_arguments)]
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

/// The unused index-0 entry and every gap between the sparse designated
/// indices — C's implicit zero-initialization (`.name = NULL`, skipped by the
/// `--sort-key=help` listing and column lookups).
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
/// `freebsd/FreeBSDProcess.c:24` — the FreeBSD per-field metadata table, indexed
/// by [`ProcessField`] id (plus the platform [`JID`]/[`JAIL`]/[`EMULATION`]/
/// [`SCHEDCLASS`] ids). Built with `SCHEDULER_SUPPORT` on (FreeBSD provides
/// `sched_getscheduler`/`sched_setscheduler`, so the `SCHEDULERPOLICY` slot is
/// filled). Trailing spaces in the titles are significant (they set the printed
/// column width) and preserved verbatim.
#[allow(non_upper_case_globals)] // C global name `Process_fields`.
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
        "Process state (S sleeping, R running, D disk, Z zombie, T traced, W paging)",
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
    t[PF::MAJFLT as usize] = pfd(
        "MAJFLT",
        "     MAJFLT ",
        "Number of copy-on-write faults",
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
    t[PF::PERCENT_NORM_CPU as usize] = pfd(
        "PERCENT_NORM_CPU",
        "NCPU%",
        "Normalized percentage of the CPU time the process used in the last sampling (normalized by cpu count)",
        0, false, true, true, false,
    );
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
    // #ifdef SCHEDULER_SUPPORT (FreeBSD provides sched_get/setscheduler):
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
    t[JID as usize] = pfd("JID", "JID", "Jail prison ID", 0, true, false, false, false);
    t[JAIL as usize] = pfd(
        "JAIL",
        "JAIL        ",
        "Jail prison name",
        0,
        false,
        false,
        false,
        false,
    );
    t[SCHEDCLASS as usize] = pfd(
        "SCHEDCLASS",
        "SC",
        "Scheduling Class (Timesharing, Realtime, Idletime)",
        0,
        false,
        false,
        false,
        false,
    );
    t[EMULATION as usize] = pfd(
        "EMULATION",
        "EMULATION        ",
        "System call emulation environment (ABI)",
        0,
        false,
        false,
        false,
        false,
    );
    t
}

/// Port of `const ProcessClass FreeBSDProcess_class` (`FreeBSDProcess.c:139`).
/// The `RowClass` vtable wires the inherited `Process` slots plus the
/// FreeBSD-specific `writeField` ([`FreeBSDProcess_rowWriteField`]) and the
/// `compareByKey` [`ProcessClass`] slot ([`FreeBSDProcess_compareByKey`]).
/// `matchesFilter` stays `None` (as the linux/darwin ports); `.display` /
/// `.delete` / `.compare` are realized by the [`Object`] impl / `Drop`.
pub static FreeBSDProcess_class: ProcessClass = ProcessClass {
    super_: RowClass {
        super_: ObjectClass {
            extends: Some(&Process_class.super_.super_),
        },
        isHighlighted: Some(Process_rowIsHighlighted),
        isVisible: Some(Process_rowIsVisible),
        writeField: Some(FreeBSDProcess_rowWriteField),
        matchesFilter: None,
        sortKeyString: Some(Process_rowGetSortKey),
        compareByParent: Some(Process_compareByParent),
    },
    compareByKey: Some(FreeBSDProcess_compareByKey),
};

/// `FreeBSDProcess` "is a" `Object` (via `Process` via `Row`). The class /
/// display / compare slots resolve to [`FreeBSDProcess_class`]; the base-view
/// accessors expose this object's embedded [`Row`]/[`Process`] — the
/// mechanism a [`Table`](crate::ported::table::Table) of `Box<dyn Object>`
/// rows uses to recover them.
impl Object for FreeBSDProcess {
    /// C `Object_setClass(this, Class(FreeBSDProcess))` in [`FreeBSDProcess_new`].
    fn klass(&self) -> &'static ObjectClass {
        &FreeBSDProcess_class.super_.super_
    }

    /// C `As_Row(this)` — `FreeBSDProcess`'s [`RowClass`] vtable.
    fn row_class(&self) -> Option<&'static RowClass> {
        Some(&FreeBSDProcess_class.super_)
    }

    /// C `As_Process(this)` — `FreeBSDProcess`'s [`ProcessClass`] vtable.
    fn process_class(&self) -> Option<&'static ProcessClass> {
        Some(&FreeBSDProcess_class)
    }

    /// C `FreeBSDProcess_class.super.super.display = Row_display`.
    fn display(&self, out: &mut RichString) {
        crate::ported::row::Row_display(self, out)
    }

    /// C `FreeBSDProcess_class.super.super.compare = Process_compare`.
    fn compare(&self, other: &dyn Object) -> i32 {
        crate::ported::process::Process_compare(self, other)
    }

    /// C `(const Row*)this` — the embedded base row of a `FreeBSDProcess`.
    fn as_row(&self) -> Option<&Row> {
        Some(&self.super_.super_)
    }

    fn as_row_mut(&mut self) -> Option<&mut Row> {
        Some(&mut self.super_.super_)
    }

    /// C `(const Process*)this` — the embedded `Process` of a `FreeBSDProcess`.
    fn as_process(&self) -> Option<&Process> {
        Some(&self.super_)
    }

    fn as_process_mut(&mut self) -> Option<&mut Process> {
        Some(&mut self.super_)
    }
}

/// Port of `Process* FreeBSDProcess_new(const Machine* machine)` from
/// `FreeBSDProcess.c:62`. C `xCalloc`s a `FreeBSDProcess`, sets its class,
/// and runs `Process_init` on the embedded base, returning `(Process*)this`.
///
/// The returned `Box<FreeBSDProcess>` is the owner (C's heap allocation);
/// `&mut box.super_` is the `*mut Process`. `Object_setClass` /
/// `Class(FreeBSDProcess)` are dropped — class identity is the Rust type
/// (the [`Object`] impl above). The FreeBSD fields start at their `xCalloc`
/// zero (`jid = 0`, `jname`/`emul` `None`, `sched_class = SCHEDCLASS_UNKNOWN`).
pub fn FreeBSDProcess_new(machine: *const Machine) -> Box<FreeBSDProcess> {
    let mut this = Box::new(FreeBSDProcess {
        super_: Process::default(),
        jid: 0,
        jname: None,
        emul: None,
        sched_class: FreeBSDSchedClass::SCHEDCLASS_UNKNOWN,
    });

    Process_init(&mut this.super_, machine as *const c_void);

    this
}

/// TODO: port of `void Process_delete(Object* cast)` from
/// `FreeBSDProcess.c:69`. Kept stubbed: the C body is a pure teardown —
/// `Process_done((Process*)cast)` followed by `free(this->emul)`,
/// `free(this->jname)`, `free(this)`. Rust owns the [`FreeBSDProcess`]
/// allocation and its `Option<String>` fields, so `Drop` reclaims them
/// automatically; there is no faithful safe-Rust analog (the darwin/linux
/// `Process_delete` precedent).
pub fn Process_delete(this: FreeBSDProcess) {
    // C `void Process_delete(Object* cast)` (FreeBSDProcess.c:69):
    // `Process_done(&this->super); free(this);`. Take `this` by value — the base
    // teardown is `Process_done` on the moved-out `super_`, the scalar fields
    // drop trivially, and `free(this)` folds into the by-value consume (the
    // darwin `Process_delete` / destructor-sweep idiom).
    let FreeBSDProcess { super_, .. } = this;
    crate::ported::process::Process_done(super_);
}

/// Port of `static void FreeBSDProcess_rowWriteField(const Row* super,
/// RichString* str, ProcessField field)` from `FreeBSDProcess.c:85` — the
/// FreeBSD-specific per-field renderer. Handles the platform `JID` /
/// `JAIL` / `EMULATION` / `SCHEDCLASS` columns and delegates every other key
/// to the base [`Process_writeField`]. Mirrors the darwin/linux
/// `*_rowWriteField`: the `JAIL`/`EMULATION` arms print a left-aligned field
/// and `return`, the `JID`/`SCHEDCLASS` arms format into a buffer, and the
/// shared tail appends it with the `DEFAULT_COLOR` attr
/// (`CRT_colors[DEFAULT_COLOR]`).
///
/// This is the `writeField` [`RowClass`] vtable slot for `FreeBSDProcess`;
/// the C `const Row* super` receiver is a `&dyn Object` downcast to
/// [`FreeBSDProcess`] (C's `(const FreeBSDProcess*)super`).
pub fn FreeBSDProcess_rowWriteField(super_: &dyn Object, str: &mut RichString, field: RowField) {
    let fp = (super_ as &dyn Any)
        .downcast_ref::<FreeBSDProcess>()
        .expect("FreeBSDProcess_rowWriteField: row is not a FreeBSDProcess");

    let scheme = ColorScheme::active();
    let attr = CE::DEFAULT_COLOR.packed(scheme);
    let buffer: String;

    match field {
        // case JID: xSnprintf(buffer, n, "%*d ", Process_pidDigits, fp->jid);
        JID => {
            let digits = Row_pidDigits.load(Ordering::Relaxed) as usize;
            buffer = format!("{:>digits$} ", fp.jid);
        }

        // case JAIL: Row_printLeftAlignedField(str, attr, jname ?: "N/A", 11); return;
        JAIL => {
            let name = fp.jname.as_deref().unwrap_or("N/A");
            Row_printLeftAlignedField(str, attr, name.as_bytes(), 11);
            return;
        }

        // case EMULATION: Row_printLeftAlignedField(str, attr, emul ?: "N/A", 16); return;
        EMULATION => {
            let name = fp.emul.as_deref().unwrap_or("N/A");
            Row_printLeftAlignedField(str, attr, name.as_bytes(), 16);
            return;
        }

        // case SCHEDCLASS: xSnprintf(buffer, n, " %c", FreeBSD_schedclassChars[sched_class]);
        SCHEDCLASS => {
            let sched_class = FreeBSD_schedclassChars[fp.sched_class as usize];
            buffer = format!(" {}", sched_class as char);
        }

        _ => {
            Process_writeField(&fp.super_, str, field);
            return;
        }
    }

    RichString_appendWide(str, attr, buffer.as_bytes());
}

/// Port of `static int FreeBSDProcess_compareByKey(const Process* v1, const
/// Process* v2, ProcessField key)` from `FreeBSDProcess.c:120`. Compares two
/// processes on the FreeBSD platform `JID` / `JAIL` / `EMULATION` /
/// `SCHEDCLASS` fields, delegating unhandled keys to
/// [`Process_compareByKey_Base`]. This is the `compareByKey`
/// [`ProcessClass`] slot; the C `const Process*` receivers are `&dyn Object`
/// downcast to `FreeBSDProcess` (C's `(const FreeBSDProcess*)`).
pub fn FreeBSDProcess_compareByKey(v1: &dyn Object, v2: &dyn Object, key: RowField) -> i32 {
    let p1 = (v1 as &dyn Any)
        .downcast_ref::<FreeBSDProcess>()
        .expect("FreeBSDProcess_compareByKey: v1 is not a FreeBSDProcess");
    let p2 = (v2 as &dyn Any)
        .downcast_ref::<FreeBSDProcess>()
        .expect("FreeBSDProcess_compareByKey: v2 is not a FreeBSDProcess");

    match key {
        JID => spaceship_number!(p1.jid, p2.jid),
        JAIL => spaceship_nullstr!(
            p1.jname.as_deref().map(str::as_bytes),
            p2.jname.as_deref().map(str::as_bytes)
        ),
        EMULATION => spaceship_nullstr!(
            p1.emul.as_deref().map(str::as_bytes),
            p2.emul.as_deref().map(str::as_bytes)
        ),
        SCHEDCLASS => spaceship_number!(p1.sched_class as i32, p2.sched_class as i32),
        _ => Process_compareByKey_Base(&p1.super_, &p2.super_, key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn super_is_at_offset_zero_for_sound_downcast() {
        // htop's `(FreeBSDProcess*)super` downcast is only sound if the
        // embedded base sits at offset 0; `#[repr(C)]` guarantees it.
        assert_eq!(core::mem::offset_of!(FreeBSDProcess, super_), 0);

        let host = 0xF00D as *const Machine;
        let fp = FreeBSDProcess_new(host);
        let base: *const Process = &fp.super_;
        let back = base as *const FreeBSDProcess;
        assert_eq!(back, &*fp as *const FreeBSDProcess);
    }

    #[test]
    fn new_sets_freebsd_defaults() {
        let host = 0xF00D as *const Machine;
        let fp = FreeBSDProcess_new(host);
        assert_eq!(fp.jid, 0);
        assert!(fp.jname.is_none());
        assert!(fp.emul.is_none());
        assert!(fp.sched_class == FreeBSDSchedClass::SCHEDCLASS_UNKNOWN);
        assert_eq!(fp.super_.super_.host, host as *const c_void);
    }

    #[test]
    fn schedclass_chars_match_c_designated_initializers() {
        assert_eq!(
            FreeBSD_schedclassChars[FreeBSDSchedClass::SCHEDCLASS_UNKNOWN as usize],
            b'?'
        );
        assert_eq!(
            FreeBSD_schedclassChars[FreeBSDSchedClass::SCHEDCLASS_INTR_THREAD as usize],
            b'-'
        );
        assert_eq!(
            FreeBSD_schedclassChars[FreeBSDSchedClass::SCHEDCLASS_REALTIME as usize],
            b'r'
        );
        assert_eq!(
            FreeBSD_schedclassChars[FreeBSDSchedClass::SCHEDCLASS_TIMESHARE as usize],
            b' '
        );
        assert_eq!(
            FreeBSD_schedclassChars[FreeBSDSchedClass::SCHEDCLASS_IDLE as usize],
            b'i'
        );
    }

    #[test]
    fn compareByKey_orders_platform_fields() {
        let host = 0xF00D as *const Machine;
        let mut a = FreeBSDProcess_new(host);
        let mut b = FreeBSDProcess_new(host);
        a.jid = 3;
        b.jid = 7;
        assert!(FreeBSDProcess_compareByKey(&*a, &*b, JID) < 0);

        a.jname = Some("alpha".into());
        b.jname = Some("beta".into());
        assert!(FreeBSDProcess_compareByKey(&*a, &*b, JAIL) < 0);

        a.sched_class = FreeBSDSchedClass::SCHEDCLASS_IDLE;
        b.sched_class = FreeBSDSchedClass::SCHEDCLASS_REALTIME;
        assert!(FreeBSDProcess_compareByKey(&*a, &*b, SCHEDCLASS) > 0);
    }
}
