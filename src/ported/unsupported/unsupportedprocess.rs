//! Port of `UnsupportedProcess.c` — the fallback per-process object.
//!
//! Ported (self-contained: only the base [`Process`] + [`Process_init`] and
//! the base field renderer/comparator, since the fallback adds no
//! platform-specific process fields):
//! - [`UnsupportedProcess_new`] (`UnsupportedProcess.c:47`)
//! - [`UnsupportedProcess_rowWriteField`] (`UnsupportedProcess.c:61`)
//! - [`UnsupportedProcess_compareByKey`] (`UnsupportedProcess.c:82`)
//!
//! Still `todo!()`:
//! - `Process_delete` — the C body is a pure teardown (`Process_done(super)`
//!   then `free(cast)`, no platform-specific heap fields). Rust owns the
//!   [`UnsupportedProcess`] allocation and its `Option<String>` base fields, so
//!   `Drop` reclaims them; there is no faithful safe-Rust analog (the same
//!   blocker the native darwin/linux `Process_delete` ports carry).
//!
//! The C `const ProcessFieldData Process_fields[LAST_PROCESSFIELD]` table
//! (`UnsupportedProcess.c:18`) is process-field *data*, not a `todo!()`
//! function, and is out of this port's function scope.
#![allow(non_snake_case)]
#![allow(dead_code)]

use core::any::Any;
use std::os::raw::c_void;

use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::process::{
    Process, ProcessClass, ProcessField, ProcessFieldData, Process_compareByKey_Base, Process_init,
    Process_writeField,
};
use crate::ported::richstring::RichString;
use crate::ported::row::{Row, RowClass};
use crate::ported::settings::RowField;

/// Port of `#define LAST_PROCESSFIELD LAST_RESERVED_FIELD` (`Process.h:229`) for
/// the unsupported build. It defines no `PLATFORM_PROCESS_FIELDS`
/// (`unsupported/ProcessField.h`), so the enum counter lands right after the
/// shared `CWD = 126`, giving `LAST_RESERVED_FIELD = 127`. Also the length of
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
/// `unsupported/UnsupportedProcess.c:25` — the stub-platform per-field metadata
/// table, indexed by [`ProcessField`] id. It carries no `PROC_COMM`/`PROC_EXE`/
/// `CWD` columns and no scan flags. Trailing spaces in the titles are
/// significant (they set the printed column width) and preserved verbatim.
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
    t
}

/// Port of `typedef struct UnsupportedProcess_` (`UnsupportedProcess.h`).
/// The fallback process embeds the base [`Process`] and adds no
/// platform-specific fields (the C struct's body is the `/* Add platform
/// specific fields */` comment). `#[repr(C)]` keeps `super_` at offset 0 so
/// the C `(UnsupportedProcess*)super` downcast is sound.
#[repr(C)]
pub struct UnsupportedProcess {
    /// C `Process super` — the embedded base process.
    pub super_: Process,
}

/// `UnsupportedProcess` "is a" `Object` (via `Process` via `Row`). Every slot
/// delegates to the embedded [`Process`] — the fallback class overrides only
/// `writeField`/`compareByKey`, both of which just re-dispatch to the base —
/// while the base-view accessors expose the embedded [`Row`]/[`Process`].
impl Object for UnsupportedProcess {
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

/// Port of `Process* UnsupportedProcess_new(const Machine* host)`
/// (`UnsupportedProcess.c:47`). C `xCalloc`s an `UnsupportedProcess`, sets its
/// class, runs `Process_init` on the embedded base, and returns `(Process*)`.
///
/// The returned `Box<UnsupportedProcess>` is the owner (C's heap allocation);
/// `&mut box.super_` is the `*mut Process`. `Object_setClass` /
/// `Class(UnsupportedProcess)` are dropped — class identity is the Rust type.
pub fn UnsupportedProcess_new(host: *const Machine) -> Box<UnsupportedProcess> {
    let mut this = Box::new(UnsupportedProcess {
        super_: Process::default(),
    });

    Process_init(&mut this.super_, host as *const c_void);

    this
}

/// TODO: port of `void Process_delete(Object* cast)` from
/// `UnsupportedProcess.c:54`. Kept stubbed: the C body is a pure teardown —
/// `Process_done(super)` followed by `free(cast)` (no platform-specific heap
/// fields to release). Rust owns the [`UnsupportedProcess`] allocation and its
/// `Option<String>` base fields, so `Drop` reclaims them automatically; there
/// is no faithful safe-Rust analog (the darwin/linux `Process_delete`
/// precedent).
pub fn Process_delete() {
    todo!("port of UnsupportedProcess.c:54 — pure free() teardown; Rust Drop handles it")
}

/// Port of `static void UnsupportedProcess_rowWriteField(const Row* super,
/// RichString* str, ProcessField field)` (`UnsupportedProcess.c:61`) — the
/// fallback per-field renderer. The C `switch` has only a `default` arm that
/// delegates to [`Process_writeField`] (the fallback adds no platform
/// columns), so the C's `buffer`/`attr`/`coloring` locals and the trailing
/// `RichString_appendWide` are unreachable (the C flags them `(void) coloring;
/// (void) n;`); this port omits that dead tail.
///
/// This is the `writeField` [`RowClass`] vtable slot for `UnsupportedProcess`;
/// the C `const Row* super` receiver is a `&dyn Object` downcast to
/// [`UnsupportedProcess`] (C's `(const UnsupportedProcess*)super`).
pub fn UnsupportedProcess_rowWriteField(
    super_: &dyn Object,
    str: &mut RichString,
    field: RowField,
) {
    let up = (super_ as &dyn Any)
        .downcast_ref::<UnsupportedProcess>()
        .expect("UnsupportedProcess_rowWriteField: row is not an UnsupportedProcess");

    {
        Process_writeField(&up.super_, str, field);
    }
}

/// Port of `static int UnsupportedProcess_compareByKey(const Process* v1, const
/// Process* v2, ProcessField key)` (`UnsupportedProcess.c:82`). The C `switch`
/// has only a `default` arm delegating to [`Process_compareByKey_Base`] (the
/// fallback adds no platform keys); the C's `(void) p1; (void) p2;` downcasts
/// are unused, mirrored here by dropping straight to the base comparator.
///
/// This is the `compareByKey` [`ProcessClass`] slot; the C `const Process*`
/// receivers are `&dyn Object` downcast to `UnsupportedProcess`.
pub fn UnsupportedProcess_compareByKey(v1: &dyn Object, v2: &dyn Object, key: RowField) -> i32 {
    let p1 = (v1 as &dyn Any)
        .downcast_ref::<UnsupportedProcess>()
        .expect("UnsupportedProcess_compareByKey: v1 is not an UnsupportedProcess");
    let p2 = (v2 as &dyn Any)
        .downcast_ref::<UnsupportedProcess>()
        .expect("UnsupportedProcess_compareByKey: v2 is not an UnsupportedProcess");

    Process_compareByKey_Base(&p1.super_, &p2.super_, key)
}
