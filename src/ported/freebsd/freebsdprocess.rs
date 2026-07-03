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
    spaceship_nullstr, Process, ProcessClass, Process_class, Process_compareByKey_Base,
    Process_compareByParent, Process_init, Process_rowGetSortKey, Process_rowIsHighlighted,
    Process_rowIsVisible, Process_writeField,
};
use crate::ported::richstring::{RichString, RichString_appendWide};
use crate::ported::row::{spaceship_number, Row, RowClass, Row_pidDigits, Row_printLeftAlignedField};
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
pub fn Process_delete() {
    todo!("port of FreeBSDProcess.c:69 — pure free() teardown; Rust Drop handles it")
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
