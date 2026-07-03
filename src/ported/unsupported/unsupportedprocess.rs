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
    Process, ProcessClass, Process_compareByKey_Base, Process_init, Process_writeField,
};
use crate::ported::richstring::RichString;
use crate::ported::row::{Row, RowClass};
use crate::ported::settings::RowField;

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
pub fn UnsupportedProcess_rowWriteField(super_: &dyn Object, str: &mut RichString, field: RowField) {
    let up = (super_ as &dyn Any)
        .downcast_ref::<UnsupportedProcess>()
        .expect("UnsupportedProcess_rowWriteField: row is not an UnsupportedProcess");

    match field {
        // No platform-specific fields — the C `switch` falls to `default`.
        _ => {
            Process_writeField(&up.super_, str, field);
        }
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

    match key {
        // No platform-specific keys — the C `switch` falls to `default`.
        _ => Process_compareByKey_Base(&p1.super_, &p2.super_, key),
    }
}
