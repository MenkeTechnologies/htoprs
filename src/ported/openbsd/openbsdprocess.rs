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
    Process, ProcessClass, Process_class, Process_compare, Process_compareByKey_Base,
    Process_compareByParent, Process_init, Process_rowGetSortKey, Process_rowIsHighlighted,
    Process_rowIsVisible, Process_writeField,
};
use crate::ported::richstring::RichString;
use crate::ported::row::{Row, RowClass, Row_display};
use crate::ported::settings::RowField;

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

/// TODO: port of `void Process_delete(Object* cast)` from
/// `OpenBSDProcess.c:217`. Kept stubbed: the C body is a pure teardown —
/// `Process_done((Process*)cast)` followed by `free(this)` (no OpenBSD-only
/// heap fields to release). Rust owns the [`OpenBSDProcess`] allocation and
/// its `Option<String>` base fields, so `Drop` reclaims them automatically;
/// there is no faithful safe-Rust analog (the darwin/linux `Process_delete`
/// precedent).
pub fn Process_delete() {
    todo!("port of OpenBSDProcess.c:217 — pure free() teardown; Rust Drop handles it")
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
