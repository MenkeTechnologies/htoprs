//! Port of `NetBSDProcess.c` — the NetBSD process object.
//!
//! Ported (self-contained, on the base [`Process`] + [`Process_init`]):
//! - the [`NetBSDProcess`] object struct (`NetBSDProcess.h:20` — just an
//!   embedded `Process super`; NetBSD adds no per-process fields).
//! - [`NetBSDProcess_new`] (`NetBSDProcess.c:218`).
//! - the leaf column renderer [`NetBSDProcess_rowWriteField`]
//!   (`NetBSDProcess.c:231`) and comparator [`NetBSDProcess_compareByKey`]
//!   (`NetBSDProcess.c:248`). NetBSD defines no platform-specific
//!   `ProcessField`s, so both switch statements have only the `default` arm
//!   and delegate wholesale to the base [`Process_writeField`] /
//!   [`Process_compareByKey_Base`].
//!
//! Still `todo!()`:
//! - `Process_delete` (`NetBSDProcess.c:225`) is a pure `free()` teardown —
//!   `Process_done(&this->super)` then `free(this)` (no NetBSD-only heap
//!   fields). Rust owns the [`NetBSDProcess`] allocation and its base
//!   `Option<String>` fields, so `Drop` reclaims them; there is no faithful
//!   safe-Rust analog (the darwin/linux `Process_delete` precedent).
//!
//! The `Process_fields[]` field-descriptor table (`NetBSDProcess.c:23`) is
//! data, not a function, and is deferred until the shared `ProcessField`
//! layer models the NetBSD column titles.
#![allow(non_snake_case)]
#![allow(dead_code)]

use core::any::Any;

use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::process::{
    Process, ProcessClass, ProcessField, ProcessFieldData, Process_compareByKey_Base, Process_init,
    Process_writeField, PROCESS_FLAG_CWD,
};
use crate::ported::richstring::RichString;
use crate::ported::row::{Row, RowClass};
use crate::ported::settings::RowField;
use std::os::raw::c_void;

/// Port of `#define LAST_PROCESSFIELD LAST_RESERVED_FIELD` (`Process.h:229`) for
/// the NetBSD build. NetBSD defines no `PLATFORM_PROCESS_FIELDS`
/// (`netbsd/ProcessField.h`), so the enum counter lands right after the shared
/// `CWD = 126`, giving `LAST_RESERVED_FIELD = 127`. Also the length of
/// [`Process_fields`].
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
/// `netbsd/NetBSDProcess.c:23` — the NetBSD per-field metadata table, indexed by
/// [`ProcessField`] id. Trailing spaces in the titles are significant (they set
/// the printed column width) and preserved verbatim.
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
        "SESN",
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
        false,
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
    t
}

/// Port of htop's `struct NetBSDProcess_` (`NetBSDProcess.h:20`). "Extends"
/// [`Process`] via the embedded `super_` field (htop's `Process super;` first
/// member); NetBSD carries no platform-specific per-process fields.
///
/// `#[repr(C)]` guarantees `super_` sits at offset 0, so htop's
/// `(NetBSDProcess*)processPtr` downcast — a `*const Process` obtained from a
/// `NetBSDProcess` allocation, cast back — is sound.
#[repr(C)]
pub struct NetBSDProcess {
    /// C `Process super` — the embedded base process.
    pub super_: Process,
}

/// `NetBSDProcess` "is a" `Object` (via `Process` via `Row`). Every class /
/// display / compare slot delegates to the embedded [`Process`] (the
/// `NetBSDProcess_class` vtable overrides no base slots that are ported yet),
/// while the base-view accessors expose this object's embedded [`Row`] /
/// [`Process`] — the mechanism a [`Table`](crate::ported::table::Table) of
/// `Box<dyn Object>` rows uses to recover them.
impl Object for NetBSDProcess {
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

/// Port of `Process* NetBSDProcess_new(const Machine* host)` from
/// `NetBSDProcess.c:218`. C `xCalloc`s a `NetBSDProcess`, sets its class, runs
/// `Process_init` on the embedded base, and returns `(Process*)this`.
///
/// The returned `Box<NetBSDProcess>` is the owner (C's heap allocation);
/// `&mut box.super_` is the `*mut Process`. `Object_setClass` /
/// `Class(NetBSDProcess)` are dropped — class identity is the Rust type.
pub fn NetBSDProcess_new(host: *const Machine) -> Box<NetBSDProcess> {
    let mut this = Box::new(NetBSDProcess {
        super_: Process::default(),
    });

    Process_init(&mut this.super_, host as *const c_void);

    this
}

/// Port of `void Process_delete(Object* cast)` from `NetBSDProcess.c:225` —
/// the C body is a pure teardown: `Process_done(&this->super)` followed by
/// `free(this)` (no NetBSD-only heap fields to release). Take `this` by value:
/// the base teardown runs on the moved-out `super_`, and the final
/// `free(this)` folds into the by-value consume (the darwin `Process_delete`
/// precedent).
pub fn Process_delete(this: NetBSDProcess) {
    let NetBSDProcess { super_, .. } = this;
    crate::ported::process::Process_done(super_);
}

/// Port of `static void NetBSDProcess_rowWriteField(const Row* super,
/// RichString* str, ProcessField field)` from `NetBSDProcess.c:231` — the
/// NetBSD-specific per-field renderer. NetBSD defines no platform-specific
/// `ProcessField`s, so the C `switch (field)` has only the `default` arm,
/// which delegates to the base [`Process_writeField`]; the unreachable
/// `RichString_appendWide` tail is omitted.
///
/// This is the `writeField` [`RowClass`] vtable slot for `NetBSDProcess`; the
/// C `const Row* super` receiver is a `&dyn Object` downcast to
/// [`NetBSDProcess`] (C's `(const NetBSDProcess*)super`).
pub fn NetBSDProcess_rowWriteField(super_: &dyn Object, str: &mut RichString, field: RowField) {
    let np = (super_ as &dyn Any)
        .downcast_ref::<NetBSDProcess>()
        .expect("NetBSDProcess_rowWriteField: row is not a NetBSDProcess");

    // switch (field) { default: Process_writeField(&np->super, str, field); return; }
    Process_writeField(&np.super_, str, field);
}

/// Port of `static int NetBSDProcess_compareByKey(const Process* v1, const
/// Process* v2, ProcessField key)` from `NetBSDProcess.c:248`. NetBSD defines
/// no platform-specific `ProcessField`s, so the C `switch (key)` has only the
/// `default` arm, which delegates to [`Process_compareByKey_Base`].
///
/// This is the `compareByKey` [`ProcessClass`] slot; the C `const Process*`
/// receivers are `&dyn Object` downcast to [`NetBSDProcess`].
pub fn NetBSDProcess_compareByKey(v1: &dyn Object, v2: &dyn Object, key: RowField) -> i32 {
    let p1 = (v1 as &dyn Any)
        .downcast_ref::<NetBSDProcess>()
        .expect("NetBSDProcess_compareByKey: v1 is not a NetBSDProcess");
    let p2 = (v2 as &dyn Any)
        .downcast_ref::<NetBSDProcess>()
        .expect("NetBSDProcess_compareByKey: v2 is not a NetBSDProcess");

    // switch (key) { default: return Process_compareByKey_Base(v1, v2, key); }
    Process_compareByKey_Base(&p1.super_, &p2.super_, key)
}
