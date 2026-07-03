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
    Process, ProcessClass, Process_compareByKey_Base, Process_init, Process_writeField,
};
use crate::ported::richstring::RichString;
use crate::ported::row::{Row, RowClass};
use crate::ported::settings::RowField;
use std::os::raw::c_void;

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

/// TODO: port of `void Process_delete(Object* cast)` from
/// `NetBSDProcess.c:225`. Kept stubbed: the C body is a pure teardown —
/// `Process_done(&this->super)` followed by `free(this)` (no NetBSD-only heap
/// fields to release). Rust owns the [`NetBSDProcess`] allocation and its
/// `Option<String>` base fields, so `Drop` reclaims them automatically; there
/// is no faithful safe-Rust analog (the darwin/linux `Process_delete`
/// precedent).
pub fn Process_delete() {
    todo!("port of NetBSDProcess.c:225 — pure free() teardown; Rust Drop handles it")
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
