//! Port of `SolarisProcess.c` — the Solaris/illumos process object.
//!
//! Ported struct model:
//! - the [`SolarisProcess`] object struct (`SolarisProcess.h:26`) — `Process
//!   super` plus the Solaris-only zone/task/project/pool/contract/`lwpid`
//!   identity fields — modeled `#[repr(C)]` with `super_` at offset 0 so the
//!   `(SolarisProcess*)proc` downcast is sound.
//! - the [`SolarisProcess_class`] vtable (`SolarisProcess.c:132`).
//!
//! Ported functions:
//! - [`SolarisProcess_new`] (`SolarisProcess.c:63`)
//! - [`SolarisProcess_rowWriteField`] (`SolarisProcess.c:77`)
//! - [`SolarisProcess_compareByKey`] (`SolarisProcess.c:104`)
//!
//! Still `todo!()`:
//! - `Process_delete` is a pure teardown (`Process_done` + `free(sp->zname)` +
//!   `free(sp)`); Rust `Drop` reclaims the [`SolarisProcess`] allocation and
//!   its `Option<String>` `zname`, so there is no faithful safe-Rust analog
//!   (the linux/darwin `Process_delete` precedent).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;
use core::ffi::c_void;
use std::sync::atomic::Ordering;

use crate::ported::crt::{ColorElements as CE, ColorScheme};
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::process::{
    Process, ProcessClass, ProcessField, Process_class, Process_compare,
    Process_compareByKey_Base, Process_compareByParent, Process_init, Process_rowGetSortKey,
    Process_rowIsHighlighted, Process_rowIsVisible, Process_rowMatchesFilter, Process_writeField,
};
use crate::ported::richstring::{RichString, RichString_appendWide};
use crate::ported::row::{
    spaceship_number, Row, RowClass, Row_display, Row_pidDigits, Row_printLeftAlignedField,
};
use crate::ported::settings::RowField;

/// `#define ZONENAME_MAX 256` (`<sys/zone.h>`).
const ZONENAME_MAX: u32 = 256;

// The Solaris platform `ProcessField` ids, spliced into the C
// `ReservedFields` enum by `PLATFORM_PROCESS_FIELDS` (`solaris/ProcessField.h`).
// The shared [`ProcessField`] enum uses the Linux numbering (id `100` is the
// Linux `CTID`, `101` `VPID`, …), so — as with the darwin `TRANSLATED` id —
// these live as local [`RowField`] constants and the `switch` matches the raw
// field id exactly as the C does.
const ZONEID: RowField = 100;
const ZONE: RowField = 101;
const PROJID: RowField = 102;
const TASKID: RowField = 103;
const POOLID: RowField = 104;
const CONTID: RowField = 105;
const LWPID: RowField = 106;

/// Port of `typedef struct SolarisProcess_ { … } SolarisProcess`
/// (`SolarisProcess.h:26`). "Extends" [`Process`] via the embedded `super_`
/// (htop's `Process super;` first member); `#[repr(C)]` keeps `super_` at
/// offset 0 so the C `(SolarisProcess*)proc` downcast is sound. The C `char*
/// zname` heap string is an owned `Option<String>`.
#[repr(C)]
pub struct SolarisProcess {
    /// C `Process super` — the embedded base process.
    pub super_: Process,
    /// C `zoneid_t zoneid`.
    pub zoneid: i32,
    /// C `char* zname`.
    pub zname: Option<String>,
    /// C `taskid_t taskid`.
    pub taskid: i32,
    /// C `projid_t projid`.
    pub projid: i32,
    /// C `poolid_t poolid`.
    pub poolid: i32,
    /// C `ctid_t contid`.
    pub contid: i32,
    /// C `pid_t realpid`.
    pub realpid: i32,
    /// C `pid_t realppid`.
    pub realppid: i32,
    /// C `pid_t realtgid`.
    pub realtgid: i32,
    /// C `pid_t lwpid`.
    pub lwpid: i32,
}

/// Port of `const ProcessClass SolarisProcess_class` (`SolarisProcess.c:132`).
/// The `RowClass` vtable wires the inherited `Process` slots plus the
/// Solaris-specific `writeField` ([`SolarisProcess_rowWriteField`]) and the
/// `compareByKey` [`ProcessClass`] slot ([`SolarisProcess_compareByKey`]).
/// `.display = Row_display`, `.delete = Process_delete` and `.compare =
/// Process_compare` are realized by the [`Object`] impl / `Drop`.
pub static SolarisProcess_class: ProcessClass = ProcessClass {
    super_: RowClass {
        super_: ObjectClass {
            extends: Some(&Process_class.super_.super_),
        },
        isHighlighted: Some(Process_rowIsHighlighted),
        isVisible: Some(Process_rowIsVisible),
        writeField: Some(SolarisProcess_rowWriteField),
        matchesFilter: Some(Process_rowMatchesFilter),
        sortKeyString: Some(Process_rowGetSortKey),
        compareByParent: Some(Process_compareByParent),
    },
    compareByKey: Some(SolarisProcess_compareByKey),
};

impl Object for SolarisProcess {
    /// C `Object_setClass(this, Class(SolarisProcess))`: the embedded
    /// [`ObjectClass`] of the [`SolarisProcess_class`] vtable.
    fn klass(&self) -> &'static ObjectClass {
        &SolarisProcess_class.super_.super_
    }

    /// C `As_Row(this)` — `SolarisProcess`'s [`RowClass`] vtable.
    fn row_class(&self) -> Option<&'static RowClass> {
        Some(&SolarisProcess_class.super_)
    }

    /// C `As_Process(this)` — `SolarisProcess`'s [`ProcessClass`] vtable.
    fn process_class(&self) -> Option<&'static ProcessClass> {
        Some(&SolarisProcess_class)
    }

    /// C `(const Row*)this` — the embedded base [`Row`] of a `SolarisProcess`.
    fn as_row(&self) -> Option<&Row> {
        Some(&self.super_.super_)
    }

    /// Mutable view of the embedded [`Row`].
    fn as_row_mut(&mut self) -> Option<&mut Row> {
        Some(&mut self.super_.super_)
    }

    /// C `(const Process*)this` — the embedded [`Process`] of a `SolarisProcess`.
    fn as_process(&self) -> Option<&Process> {
        Some(&self.super_)
    }

    /// Mutable view of the embedded [`Process`].
    fn as_process_mut(&mut self) -> Option<&mut Process> {
        Some(&mut self.super_)
    }

    /// C `SolarisProcess_class.super.super.display = Row_display`.
    fn display(&self, out: &mut RichString) {
        Row_display(self, out)
    }

    /// C `SolarisProcess_class.super.super.compare = Process_compare`. Passes
    /// the concrete objects so `Process_compare` dispatches this class's
    /// `compareByKey` slot.
    fn compare(&self, other: &dyn Object) -> i32 {
        Process_compare(self, other)
    }
}

/// Port of `Process* SolarisProcess_new(const Machine* host)` from
/// `SolarisProcess.c:63`. C `xCalloc`s a `SolarisProcess` (every field
/// zero/`NULL`), sets its class, and runs [`Process_init`] on the embedded
/// base. The returned `Box<SolarisProcess>` is the owner (C's heap
/// allocation); `&mut box.super_` is the `*mut Process`. `Object_setClass` /
/// `Class(SolarisProcess)` are dropped — class identity is the Rust type.
pub fn SolarisProcess_new(host: *const Machine) -> Box<SolarisProcess> {
    let mut this = Box::new(SolarisProcess {
        super_: Process::default(),
        zoneid: 0,
        zname: None,
        taskid: 0,
        projid: 0,
        poolid: 0,
        contid: 0,
        realpid: 0,
        realppid: 0,
        realtgid: 0,
        lwpid: 0,
    });

    Process_init(&mut this.super_, host as *const c_void);

    this
}

/// TODO: port of `void Process_delete(Object* cast)` from `SolarisProcess.c:70`.
/// Kept stubbed: the C body is a pure teardown — `Process_done` +
/// `free(sp->zname)` + `free(sp)`. Rust owns the [`SolarisProcess`] allocation
/// and its `Option<String>` `zname`, so `Drop` reclaims them automatically;
/// there is no faithful safe-Rust analog (the linux/darwin `Process_delete`
/// precedent).
pub fn Process_delete() {
    todo!("port of SolarisProcess.c:70 — pure free() teardown; Rust Drop handles it")
}

/// Port of `static void SolarisProcess_rowWriteField(const Row* super,
/// RichString* str, ProcessField field)` from `SolarisProcess.c:77` — the
/// Solaris-specific per-field renderer. Formats the zone/task/project/pool/
/// contract ids and the LWP-encoded pid/ppid/tgid/lwpid into a right-aligned
/// `Process_pidDigits`-wide field, left-aligns the zone name, and delegates
/// every other key to the base [`Process_writeField`].
///
/// This is the `writeField` [`RowClass`] slot; the C `const Row* super`
/// receiver is a `&dyn Object` downcast to [`SolarisProcess`].
pub fn SolarisProcess_rowWriteField(super_: &dyn Object, str: &mut RichString, field: RowField) {
    use ProcessField as PF;

    let sp = (super_ as &dyn Any)
        .downcast_ref::<SolarisProcess>()
        .expect("SolarisProcess_rowWriteField: row is not a SolarisProcess");

    let scheme = ColorScheme::active();
    let attr = CE::DEFAULT_COLOR.packed(scheme);
    let w = Row_pidDigits.load(Ordering::Relaxed) as usize;
    let buffer: String;

    match field {
        // add Solaris-specific fields here
        ZONEID => buffer = format!("{:>width$} ", sp.zoneid, width = w),
        PROJID => buffer = format!("{:>width$} ", sp.projid, width = w),
        TASKID => buffer = format!("{:>width$} ", sp.taskid, width = w),
        POOLID => buffer = format!("{:>width$} ", sp.poolid, width = w),
        CONTID => buffer = format!("{:>width$} ", sp.contid, width = w),
        ZONE => {
            let z = sp.zname.as_deref().unwrap_or("global");
            Row_printLeftAlignedField(str, attr, z.as_bytes(), ZONENAME_MAX / 4);
            return;
        }
        f if f == PF::PID as RowField => buffer = format!("{:>width$} ", sp.realpid, width = w),
        f if f == PF::PPID as RowField => buffer = format!("{:>width$} ", sp.realppid, width = w),
        f if f == PF::TGID as RowField => buffer = format!("{:>width$} ", sp.realtgid, width = w),
        LWPID => buffer = format!("{:>width$} ", sp.lwpid, width = w),
        _ => {
            Process_writeField(&sp.super_, str, field);
            return;
        }
    }

    RichString_appendWide(str, attr, buffer.as_bytes());
}

/// Port of `static int SolarisProcess_compareByKey(const Process* v1, const
/// Process* v2, ProcessField key)` from `SolarisProcess.c:104`. Compares two
/// processes on the Solaris platform fields, delegating unhandled keys to
/// [`Process_compareByKey_Base`]. The C `const Process*` receivers are
/// `&dyn Object` downcast to [`SolarisProcess`].
pub fn SolarisProcess_compareByKey(v1: &dyn Object, v2: &dyn Object, key: RowField) -> i32 {
    use ProcessField as PF;

    let p1 = (v1 as &dyn Any)
        .downcast_ref::<SolarisProcess>()
        .expect("SolarisProcess_compareByKey: v1 is not a SolarisProcess");
    let p2 = (v2 as &dyn Any)
        .downcast_ref::<SolarisProcess>()
        .expect("SolarisProcess_compareByKey: v2 is not a SolarisProcess");

    match key {
        ZONEID => spaceship_number!(p1.zoneid, p2.zoneid),
        PROJID => spaceship_number!(p1.projid, p2.projid),
        TASKID => spaceship_number!(p1.taskid, p2.taskid),
        POOLID => spaceship_number!(p1.poolid, p2.poolid),
        CONTID => spaceship_number!(p1.contid, p2.contid),
        ZONE => {
            // strcmp(p1->zname ?: "global", p2->zname ?: "global")
            let z1 = p1.zname.as_deref().unwrap_or("global");
            let z2 = p2.zname.as_deref().unwrap_or("global");
            z1.cmp(z2) as i32
        }
        f if f == PF::PID as RowField => spaceship_number!(p1.realpid, p2.realpid),
        f if f == PF::PPID as RowField => spaceship_number!(p1.realppid, p2.realppid),
        LWPID => spaceship_number!(p1.lwpid, p2.lwpid),
        _ => Process_compareByKey_Base(&p1.super_, &p2.super_, key),
    }
}
