//! Port of `dragonflybsd/DragonFlyBSDProcess.c` + `.h` — the DragonFly BSD
//! per-process model (`DragonFlyBSDProcess`, which "extends" [`Process`]) and
//! its `ProcessClass` vtable.
//!
//! Pure module: no `libkvm`/`sysctl`. It reuses the shared [`Process`]/[`Row`]
//! object model, the `ProcessClass`/`RowClass` vtables, the ported
//! `Process_row*` slots, and [`Process_writeField`]/[`Process_compareByKey_Base`].
//!
//! Platform field ids: htop compiles a single platform, so its `ProcessField`
//! enum splices in only that platform's `PLATFORM_PROCESS_FIELDS`. The Rust
//! port compiles every platform, and the shared [`ProcessField`] enum already
//! carries the Linux fields (`CTID = 100`, `VPID = 101`), which collide with
//! DragonFly's `JID = 100` / `JAIL = 101`. So DragonFly's two platform fields
//! are module-local [`RowField`] constants and it carries its own
//! [`Process_fields`] table, exactly as each platform has its own in C.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::any::Any;
use core::ffi::c_void;
use core::ops::Deref;
use std::sync::atomic::Ordering;

use crate::ported::crt::{ColorElements as CE, ColorScheme};
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::process::{
    spaceship_nullstr, Process, ProcessClass, ProcessField, ProcessFieldData, Process_class,
    Process_compare, Process_compareByKey_Base, Process_compareByParent, Process_getPid,
    Process_init, Process_isKernelThread, Process_rowGetSortKey, Process_rowIsHighlighted,
    Process_rowIsVisible, Process_writeField, PROCESS_FLAG_CWD,
};
use crate::ported::richstring::{RichString, RichString_appendWide};
use crate::ported::row::{
    spaceship_number, Row, RowClass, Row_display, Row_pidDigits, Row_printLeftAlignedField,
};
use crate::ported::settings::RowField;

/// Port of `JID = 100` from `dragonflybsd/ProcessField.h` — the jail prison id
/// column. Module-local (collides with the shared enum's `CTID`); see the
/// module docs.
pub const JID: RowField = 100;
/// Port of `JAIL = 101` from `dragonflybsd/ProcessField.h` — the jail prison
/// name column.
pub const JAIL: RowField = 101;

/// Port of `#define LAST_PROCESSFIELD LAST_RESERVED_FIELD` for DragonFly. The
/// `DUMMY_BUMP_FIELD = CWD` in `ProcessField.h` bumps the last reserved field
/// to `CWD = 126`, so the table has `127` slots.
pub const LAST_PROCESSFIELD: usize = 127;

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

/// Port of `const ProcessFieldData Process_fields[LAST_PROCESSFIELD]` from
/// `dragonflybsd/DragonFlyBSDProcess.c` — the DragonFly per-field metadata,
/// indexed by field id. Trailing spaces in titles are significant.
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
    t[PF::STATE as usize] = pfd("STATE", "S ", "Process state (S sleeping (<20s), I Idle, Q Queued for Run, R running, D disk, Z zombie, T traced, W paging, B Blocked, A AskedPage, C Core, J Jailed)", 0, false, false, false, false);
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
    t
}

/// Port of `struct DragonFlyBSDProcess_` (`DragonFlyBSDProcess.h`). "Extends"
/// [`Process`] via the embedded `super_`, plus the two DragonFly-specific
/// fields.
#[derive(Debug, Clone, Default)]
pub struct DragonFlyBSDProcess {
    /// C `Process super`.
    pub super_: Process,
    /// C `int jid` — jail prison id.
    pub jid: i32,
    /// C `char* jname` — jail prison name (`None` = C `NULL`).
    pub jname: Option<String>,
}

/// Port of `Process* DragonFlyBSDProcess_new(const Machine* host)`
/// (`DragonFlyBSDProcess.c:55`). Allocates a zeroed process, installs the
/// `DragonFlyBSDProcess` class, and runs [`Process_init`].
pub fn DragonFlyBSDProcess_new(host: *const Machine) -> DragonFlyBSDProcess {
    let mut this = DragonFlyBSDProcess::default();
    Process_init(&mut this.super_, host as *const c_void);
    this
}

/// Port of `static void DragonFlyBSDProcess_rowWriteField(const Row* super,
/// RichString* str, ProcessField field)` (`DragonFlyBSDProcess.c:69`) — the
/// `writeField` [`RowClass`] slot. Handles `PID` (which prints `-1` for kernel
/// threads), the `JID`/`JAIL` jail columns, and delegates the rest to the base
/// [`Process_writeField`]. The C `const Row*` receiver is a `&dyn Object`
/// downcast to [`DragonFlyBSDProcess`].
pub fn DragonFlyBSDProcess_rowWriteField(
    super_: &dyn Object,
    str: &mut RichString,
    field: RowField,
) {
    let fp = (super_ as &dyn Any)
        .downcast_ref::<DragonFlyBSDProcess>()
        .expect("DragonFlyBSDProcess_rowWriteField: row is not a DragonFlyBSDProcess");
    let this = &fp.super_;
    let scheme = ColorScheme::active();
    let attr = CE::DEFAULT_COLOR.packed(scheme);
    let buffer: String;

    match field {
        f if f == ProcessField::PID as RowField => {
            let w = Row_pidDigits.load(Ordering::Relaxed) as usize;
            let pid = if Process_isKernelThread(this) {
                -1
            } else {
                Process_getPid(this)
            };
            buffer = format!("{pid:>w$} ");
        }
        f if f == JID => {
            let w = Row_pidDigits.load(Ordering::Relaxed) as usize;
            buffer = format!("{:>w$} ", fp.jid);
        }
        f if f == JAIL => {
            let content: &[u8] = fp.jname.as_deref().map(str::as_bytes).unwrap_or(b"");
            Row_printLeftAlignedField(str, attr, content, 11);
            return;
        }
        _ => {
            Process_writeField(this, str, field);
            return;
        }
    }

    RichString_appendWide(str, attr, buffer.as_bytes());
}

/// Port of `static int DragonFlyBSDProcess_compareByKey(const Process* v1,
/// const Process* v2, ProcessField key)` (`DragonFlyBSDProcess.c:90`).
/// Compares on the `JID`/`JAIL` jail fields, delegating other keys to
/// [`Process_compareByKey_Base`]. `key` is a [`RowField`] (int, per C) so
/// DragonFly's `JID`/`JAIL` ids are representable; the `ProcessClass`
/// `compareByKey` slot is now `RowField`-typed, so this is wired into the
/// class vtable.
pub fn DragonFlyBSDProcess_compareByKey(v1: &dyn Object, v2: &dyn Object, key: RowField) -> i32 {
    let p1 = (v1 as &dyn Any)
        .downcast_ref::<DragonFlyBSDProcess>()
        .expect("DragonFlyBSDProcess_compareByKey: v1 is not a DragonFlyBSDProcess");
    let p2 = (v2 as &dyn Any)
        .downcast_ref::<DragonFlyBSDProcess>()
        .expect("DragonFlyBSDProcess_compareByKey: v2 is not a DragonFlyBSDProcess");
    match key {
        f if f == JID => spaceship_number!(p1.jid, p2.jid),
        f if f == JAIL => spaceship_nullstr!(
            p1.jname.as_deref().map(str::as_bytes),
            p2.jname.as_deref().map(str::as_bytes)
        ),
        _ => Process_compareByKey_Base(&p1.super_, &p2.super_, key),
    }
}

/// Port of `const ProcessClass DragonFlyBSDProcess_class`
/// (`DragonFlyBSDProcess.c:106`). Wires the inherited `Process` row slots
/// (`isHighlighted`/`isVisible`/`sortKeyString`/`compareByParent`) plus the
/// DragonFly-specific `writeField`. `matchesFilter` stays `None` (its delegate
/// `Process_rowMatchesFilter` is stubbed on the `ProcessTable`/`pidMatchList`
/// substrate) and `compareByKey` stays `None` pending the `RowField` slot
/// reconciliation (see [`DragonFlyBSDProcess_compareByKey`]).
pub static DragonFlyBSDProcess_class: ProcessClass = ProcessClass {
    super_: RowClass {
        super_: ObjectClass {
            extends: Some(&Process_class.super_.super_),
        },
        isHighlighted: Some(Process_rowIsHighlighted),
        isVisible: Some(Process_rowIsVisible),
        writeField: Some(DragonFlyBSDProcess_rowWriteField),
        matchesFilter: None,
        sortKeyString: Some(Process_rowGetSortKey),
        compareByParent: Some(Process_compareByParent),
    },
    compareByKey: Some(DragonFlyBSDProcess_compareByKey),
};

impl Object for DragonFlyBSDProcess {
    /// C `Object_setClass(this, Class(DragonFlyBSDProcess))`: the embedded
    /// [`ObjectClass`] of the `DragonFlyBSDProcess` vtable.
    fn klass(&self) -> &'static ObjectClass {
        &DragonFlyBSDProcess_class.super_.super_
    }

    /// C `As_Row(this)` — this process's [`RowClass`] vtable.
    fn row_class(&self) -> Option<&'static RowClass> {
        Some(&DragonFlyBSDProcess_class.super_)
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

    /// C `As_Process(this)` — `DragonFlyBSDProcess`'s [`ProcessClass`] vtable,
    /// whose `compareByKey` slot is `DragonFlyBSDProcess_compareByKey`.
    fn process_class(&self) -> Option<&'static ProcessClass> {
        Some(&DragonFlyBSDProcess_class)
    }

    /// C `DragonFlyBSDProcess_class.super.super.display = Row_display`.
    fn display(&self, out: &mut RichString) {
        Row_display(self, out)
    }

    /// C `.compare = Process_compare`, downcasting the peer to
    /// `DragonFlyBSDProcess` and comparing the embedded `Process` bases.
    fn compare(&self, other: &dyn Object) -> i32 {
        // Pass the concrete objects (not the embedded `Process`) so
        // `Process_compare` dispatches `DragonFlyBSDProcess`'s `compareByKey`.
        Process_compare(self, other)
    }
}

/// Deref so `&DragonFlyBSDProcess_class` coerces to `&ObjectClass` for the
/// class-identity API, exactly as [`ProcessClass`]'s own `Deref` (this is a
/// `ProcessClass` value, whose `Deref` already does this) — re-stated here for
/// the module's own `Object_isA` guards.
const _: fn() = || {
    fn assert_deref<T: Deref<Target = ObjectClass>>() {}
    assert_deref::<ProcessClass>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::object::Object_isA;

    /// The class chain: `DragonFlyBSDProcess` is a `DragonFlyBSDProcess`, a
    /// `Process`, and a `Row` (via the embedded `ObjectClass` extends chain).
    #[test]
    fn class_chain_extends_process() {
        let p = DragonFlyBSDProcess_new(core::ptr::null());
        let obj: &dyn Object = &p;
        assert!(Object_isA(Some(obj), &DragonFlyBSDProcess_class));
        assert!(Object_isA(Some(obj), &Process_class));
        // row_class wires the DragonFly writeField + inherited slots.
        let rc = obj.row_class().unwrap();
        assert!(rc.writeField.is_some());
        assert!(rc.isHighlighted.is_some());
        assert!(rc.matchesFilter.is_none());
    }

    /// The [`Process_fields`] table carries the DragonFly jail columns and the
    /// shared reserved fields, with significant trailing-space titles.
    #[test]
    fn process_fields_table_has_jail_columns() {
        assert_eq!(Process_fields.len(), LAST_PROCESSFIELD);
        assert_eq!(Process_fields[JID as usize].name, "JID");
        assert!(Process_fields[JID as usize].pidColumn);
        assert_eq!(Process_fields[JAIL as usize].name, "JAIL");
        assert_eq!(Process_fields[JAIL as usize].title, Some("JAIL        "));
        assert_eq!(
            Process_fields[ProcessField::PID as usize].title,
            Some("PID")
        );
        // Index 0 is the empty sentinel.
        assert_eq!(Process_fields[0].name, "");
    }

    /// [`DragonFlyBSDProcess_compareByKey`]: JID orders numerically; an
    /// unhandled key delegates to the base (PID order).
    #[test]
    fn compare_by_key_jail_and_delegate() {
        let mut a = DragonFlyBSDProcess_new(core::ptr::null());
        let mut b = DragonFlyBSDProcess_new(core::ptr::null());
        a.jid = 1;
        b.jid = 2;
        assert!(DragonFlyBSDProcess_compareByKey(&a as &dyn Object, &b as &dyn Object, JID) < 0);
        // Reserved key (PID) → base comparison by Row id.
        a.super_.super_.id = 7;
        b.super_.super_.id = 3;
        assert!(
            DragonFlyBSDProcess_compareByKey(
                &a as &dyn Object,
                &b as &dyn Object,
                ProcessField::PID as RowField
            ) > 0
        );
    }
}
