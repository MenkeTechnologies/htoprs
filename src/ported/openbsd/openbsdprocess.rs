//! Port of `OpenBSDProcess.c` — the OpenBSD process object.
//!
//! Ported (self-contained, on the base [`Process`] + [`Process_init`]):
//! - the [`OpenBSDProcess`] object struct (`Process super_` + `addr`, the
//!   kernel u-area address used to detect main threads).
//! - [`OpenBSDProcess_new`] (`OpenBSDProcess.c:210`).
//! - the [`OpenBSDProcess_class`] vtable and the leaf column
//!   [`OpenBSDProcess_rowWriteField`] (`OpenBSDProcess.c:223`) /
//!   [`OpenBSDProcess_compareByKey`] (`OpenBSDProcess.c:240`) — both pure
//!   base-field delegations (the C `switch` carries only `default:`, i.e. no
//!   OpenBSD-specific columns yet).
//!
//! Kept as a documented stub:
//! - `Process_delete` (`OpenBSDProcess.c:217`) is a pure `free()` teardown
//!   (`Process_done` + `free(this)`, no OpenBSD-only heap fields). Rust owns
//!   the [`OpenBSDProcess`] allocation and its `Option<String>` base fields,
//!   so `Drop` reclaims them — the same precedent as the darwin/linux
//!   `Process_delete`.
//!
//! # Verification note
//!
//! OpenBSD is a tier-3 Rust target with no prebuilt `std`, so this module
//! cannot be cross-compiled on the darwin dev host. The object model mirrors
//! the compiled darwin/linux ports field-for-field; it is source-reviewed,
//! not compile-verified.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // faithful C global name (OpenBSDProcess_class)
#![allow(dead_code)]

use core::any::Any;
use std::os::raw::c_void;

use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::process::{
    Process, ProcessClass, ProcessField, ProcessFieldData, Process_class, Process_compare,
    Process_compareByKey_Base, Process_compareByParent, Process_init, Process_rowGetSortKey,
    Process_rowIsHighlighted, Process_rowIsVisible, Process_writeField, PROCESS_FLAG_CWD,
};
use crate::ported::richstring::RichString;
use crate::ported::row::{Row, RowClass, Row_display};
use crate::ported::settings::RowField;

/// Port of `#define LAST_PROCESSFIELD LAST_RESERVED_FIELD` (`Process.h:229`) for
/// the OpenBSD build. OpenBSD defines no `PLATFORM_PROCESS_FIELDS`
/// (`openbsd/ProcessField.h`), so the enum counter lands right after the shared
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
/// `openbsd/OpenBSDProcess.c:23` — the OpenBSD per-field metadata table, indexed
/// by [`ProcessField`] id. OpenBSD carries no `PROC_EXE` column. Trailing spaces
/// in the titles are significant (they set the printed column width) and
/// preserved verbatim.
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

/// Port of htop's `struct OpenBSDProcess_` (`OpenBSDProcess.h:18`). "Extends"
/// [`Process`] via the embedded `super_` field (htop's `Process super;` first
/// member); `addr` is the OpenBSD-only per-process field.
///
/// `#[repr(C)]` guarantees `super_` sits at offset 0, so htop's
/// `(OpenBSDProcess*)processPtr` downcast — a `*const Process` obtained from
/// an `OpenBSDProcess` allocation, cast back — is sound.
#[repr(C)]
pub struct OpenBSDProcess {
    /// C `Process super` — the embedded base process.
    pub super_: Process,
    /// C `uint64_t addr` — kernel virtual addr of the u-area, used to detect
    /// main threads (`kproc->p_addr`).
    pub addr: u64,
}

/// Port of `const ProcessClass OpenBSDProcess_class` (`OpenBSDProcess.c:254`).
/// The `RowClass` vtable wires the inherited `Process` slots
/// (`isHighlighted`/`isVisible`/`sortKeyString`/`compareByParent`) plus the
/// OpenBSD `writeField` ([`OpenBSDProcess_rowWriteField`]) and the
/// `compareByKey` [`ProcessClass`] slot ([`OpenBSDProcess_compareByKey`]);
/// `matchesFilter` stays `None` (blocked on the `pidMatchList` substrate, as
/// in the linux port). `.compare`/`.delete`/`.display` are realized by the
/// [`Object`] impl / `Drop`.
pub static OpenBSDProcess_class: ProcessClass = ProcessClass {
    super_: RowClass {
        super_: ObjectClass {
            extends: Some(&Process_class.super_.super_),
        },
        isHighlighted: Some(Process_rowIsHighlighted),
        isVisible: Some(Process_rowIsVisible),
        writeField: Some(OpenBSDProcess_rowWriteField),
        matchesFilter: None,
        sortKeyString: Some(Process_rowGetSortKey),
        compareByParent: Some(Process_compareByParent),
    },
    compareByKey: Some(OpenBSDProcess_compareByKey),
};

/// `OpenBSDProcess` "is a" [`Object`] (via `Process` via `Row`). The class /
/// display / compare slots delegate to the embedded [`Process`], while the
/// base-view accessors expose the embedded [`Row`]/[`Process`] — the mechanism
/// a [`Table`](crate::ported::table::Table) of `Box<dyn Object>` rows uses to
/// recover them.
impl Object for OpenBSDProcess {
    /// C `Object_setClass(this, Class(OpenBSDProcess))`: the embedded
    /// [`ObjectClass`] of the [`OpenBSDProcess_class`] vtable.
    fn klass(&self) -> &'static ObjectClass {
        &OpenBSDProcess_class.super_.super_
    }

    fn display(&self, out: &mut RichString) {
        Row_display(self, out)
    }

    fn compare(&self, other: &dyn Object) -> i32 {
        Process_compare(self, other)
    }

    fn row_class(&self) -> Option<&'static RowClass> {
        Some(&OpenBSDProcess_class.super_)
    }

    fn process_class(&self) -> Option<&'static ProcessClass> {
        Some(&OpenBSDProcess_class)
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

/// Port of `Process* OpenBSDProcess_new(const Machine* host)` from
/// `OpenBSDProcess.c:210`. C `xCalloc`s an `OpenBSDProcess` (so `addr` is 0),
/// sets its class, and runs `Process_init` on the embedded base, returning
/// `(Process*)this`.
///
/// The returned `Box<OpenBSDProcess>` is the owner (C's heap allocation);
/// `&mut box.super_` is the `*mut Process`. `Object_setClass` /
/// `Class(OpenBSDProcess)` are dropped — class identity is the Rust type plus
/// the [`Object`] impl above.
pub fn OpenBSDProcess_new(host: *const Machine) -> Box<OpenBSDProcess> {
    let mut this = Box::new(OpenBSDProcess {
        super_: Process::default(),
        addr: 0,
    });

    Process_init(&mut this.super_, host as *const c_void);

    this
}

/// Port of `void Process_delete(Object* cast)` from `OpenBSDProcess.c:217` —
/// the C body is a pure teardown: `Process_done((Process*)cast)` followed by
/// `free(this)` (no OpenBSD-only heap fields to release). Take `this` by value:
/// the base teardown runs on the moved-out `super_`, the Copy `addr` scalar
/// drops trivially, and the final `free(this)` folds into the by-value consume
/// (the darwin `Process_delete` precedent).
pub fn Process_delete(this: OpenBSDProcess) {
    let OpenBSDProcess { super_, .. } = this;
    crate::ported::process::Process_done(super_);
}

/// Port of `static void OpenBSDProcess_rowWriteField(const Row* super,
/// RichString* str, ProcessField field)` from `OpenBSDProcess.c:223` — the
/// OpenBSD-specific per-field renderer. The C `switch` carries only the
/// `default:` arm ("add OpenBSD-specific fields here"), so every key
/// delegates to the base [`Process_writeField`]; the trailing
/// `RichString_appendWide` in the C is dead code (the default arm always
/// returns) and has no analog here.
///
/// This is the `writeField` [`RowClass`] slot for `OpenBSDProcess`; the C
/// `const Row* super` receiver is a `&dyn Object` downcast to
/// [`OpenBSDProcess`] (C's `(const OpenBSDProcess*)super`).
pub fn OpenBSDProcess_rowWriteField(super_: &dyn Object, str: &mut RichString, field: RowField) {
    let op = (super_ as &dyn Any)
        .downcast_ref::<OpenBSDProcess>()
        .expect("OpenBSDProcess_rowWriteField: row is not an OpenBSDProcess");

    // switch (field) { /* add OpenBSD-specific fields here */ default: ... }
    Process_writeField(&op.super_, str, field);
}

/// Port of `static int OpenBSDProcess_compareByKey(const Process* v1, const
/// Process* v2, ProcessField key)` from `OpenBSDProcess.c:240`. The C `switch`
/// carries only the `default:` arm, so every key delegates to
/// [`Process_compareByKey_Base`]. This is the `compareByKey` [`ProcessClass`]
/// slot; the C `const Process*` receivers are `&dyn Object` downcast to
/// [`OpenBSDProcess`] (C's `(const OpenBSDProcess*)`).
pub fn OpenBSDProcess_compareByKey(v1: &dyn Object, v2: &dyn Object, key: RowField) -> i32 {
    let p1 = (v1 as &dyn Any)
        .downcast_ref::<OpenBSDProcess>()
        .expect("OpenBSDProcess_compareByKey: v1 is not an OpenBSDProcess");
    let p2 = (v2 as &dyn Any)
        .downcast_ref::<OpenBSDProcess>()
        .expect("OpenBSDProcess_compareByKey: v2 is not an OpenBSDProcess");

    // switch (key) { /* add OpenBSD-specific fields here */ default: ... }
    Process_compareByKey_Base(&p1.super_, &p2.super_, key)
}
