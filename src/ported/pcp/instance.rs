//! Port of `pcp/Instance.c` + `.h` — htop's Performance Co-Pilot instance-domain
//! row: `Instance` (which "extends" [`Row`]) and its `RowClass` vtable. One
//! `Instance` is one instance of a PCP instance domain (`pmInDom`), displayed as
//! a row in an [`InDomTable`](crate::ported::pcp::indomtable::InDomTable).
//!
//! 1:1 faithful port; the C is the spec. It reuses the shared [`Row`]/[`Object`]
//! object model, the ported [`Metric`](crate::ported::pcp::metric) libpcp
//! wrapper, the [`PCPDynamicColumn`] value formatter, and the hand-declared
//! libpcp/PMAPI surface from [`crate::ported::pcp::pmapi`]; nothing is
//! redeclared. Owned `char*` fields map to `Option<String>` (`None` = C `NULL`);
//! union field reads (`pmAtomValue.cp`/`.l`/…) are `unsafe`, and the libpcp
//! `char*` from `pmNameInDom` follows the copy-then-`free` owned-string model of
//! the `Metric`/column ports.
//!
//! # Substrate limitations (reported)
//!
//! - **`Platform_dynamicColumns`** is a `pcp/Platform.c` function (not yet
//!   ported), scaffolded as a `todo!()` in [`platform`](super::platform) and
//!   imported here so the `compareByKey`/`writeField` call sites stay 1:1 until
//!   `Platform.c` lands.
//! - **Exact-type `Any` downcast** ([`Instance_writeField`]/
//!   [`Instance_compareByKey`]): [`Hashtable_get`] returns `&dyn Object`, which
//!   is downcast to [`PCPDynamicColumn`] via `Any` (exact type). C's `void*`
//!   struct-prefix aliasing would accept any `DynamicColumn`-prefixed value; the
//!   safe-Rust downcast is exact-type. Same cross-module impedance mismatch
//!   documented in `pcpdynamiccolumn.rs`.
//! - **`Instance_externalName` lazy fill** ([`Instance_externalName`]): C fills
//!   `this->name` lazily via `pmNameInDom` through the *mutable* `Row*` slot
//!   (`Row_SortKeyString` is `const char* (*)(Row*)` in C). The ported
//!   [`Row_SortKeyString`](crate::ported::row::Row_SortKeyString) slot is
//!   `fn(&dyn Object) -> Option<&[u8]>` (shared ref), so the `&mut this->name`
//!   fill cannot be performed through it without violating Rust aliasing. The
//!   ported body returns the current `name` (`None` until populated). Reported.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::any::Any;
use core::ffi::c_void;
use std::ffi::CStr;
use std::os::raw::c_int;
use std::ptr;

use crate::ported::hashtable::Hashtable_get;
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::pcp::indomtable::InDomTable;
use crate::ported::pcp::metric::{Metric_desc, Metric_fromId, Metric_instance, Metric_type};
use crate::ported::pcp::pcpdynamiccolumn::{PCPDynamicColumn, PCPDynamicColumn_writeAtomValue};
use crate::ported::pcp::platform::Platform_dynamicColumns;
use crate::ported::pcp::pmapi::{pmAtomValue, pmInDom, PM_TYPE_STRING};
use crate::ported::process::spaceship_nullstr;
use crate::ported::richstring::RichString;
use crate::ported::row::{
    spaceship_number, Row, RowClass, Row_class, Row_display, Row_done, Row_init,
};
use crate::ported::settings::{
    RowField, ScreenSettings_getActiveDirection, ScreenSettings_getActiveSortKey,
};

/// Port of `typedef struct Instance_` (`pcp/Instance.h:17`). "Extends" [`Row`]
/// via the embedded `super_` (C's first member). `char* name` → `Option<String>`
/// (`None` = C `NULL`); the `const struct InDomTable_*` back-pointer → a raw
/// `*const InDomTable`.
pub struct Instance {
    /// C `Row super`.
    pub super_: Row,
    /// C `char* name` — external instance name (`None` = C `NULL`).
    pub name: Option<String>,
    /// C `const struct InDomTable_* indom` — the owning instance domain.
    pub indom: *const InDomTable,
    /// C `unsigned int offset` — default result offset for metric searches.
    pub offset: u32,
}

/// Port of `#define InDom_getId(i_) ((i_)->indom->id)` (`pcp/Instance.h:27`).
/// Reads the owning [`InDomTable`]'s `pmInDom` through the raw back-pointer
/// (unsafe deref; a non-null `indom` is the precondition, as the C macro
/// assumes).
#[inline]
pub fn InDom_getId(this: &Instance) -> pmInDom {
    unsafe { (*this.indom).id }
}

/// Port of `#define Instance_getId(i_) ((i_)->super.id)` (`pcp/Instance.h:28`).
#[inline]
pub fn Instance_getId(this: &Instance) -> c_int {
    this.super_.id
}

/// Port of `#define Instance_setId(i_, id_) ((i_)->super.id = (id_))`
/// (`pcp/Instance.h:29`).
#[inline]
pub fn Instance_setId(this: &mut Instance, id: c_int) {
    this.super_.id = id;
}

/// Port of `Instance* Instance_new(const Machine* host, const struct InDomTable_*
/// indom)` (`Instance.c:34`). Allocates a zeroed instance (C `xCalloc`), installs
/// the `Instance` class, runs [`Row_init`], and stores the `indom` back-pointer.
/// Returns the owning `Box` (C returns the `Instance*`).
pub fn Instance_new(host: *const Machine, indom: *const InDomTable) -> Box<Instance> {
    let mut this = Box::new(Instance {
        super_: Row::default(),
        name: None,
        // C sets `this->indom = indom` after Row_init; Row_init touches only
        // `super`, so setting it here is order-equivalent.
        indom,
        offset: 0,
    });

    Row_init(&mut this.super_, host as *const c_void);

    this
}

/// Port of `void Instance_done(Instance* this)` (`Instance.c:46`). Frees
/// `this->name` (dropping the owned `Option<String>`) then runs [`Row_done`].
/// Does not free the struct storage — the `_done` contract.
pub fn Instance_done(this: &mut Instance) {
    // if (this->name) free(this->name);
    this.name = None;
    // Row_done(&this->super);
    Row_done(&this.super_);
}

/// Port of `static void Instance_delete(Object* cast)` (`Instance.c:52`). The C
/// body is `Instance_done(this); free(this);`. Taking `this` by value reproduces
/// `free(this)`; [`Instance_done`] runs the teardown, then the consumed struct
/// drops (the dragonfly `ProcessTable_delete` precedent). Not wired into a
/// vtable slot — the ported [`RowClass`] models `delete` via `Drop`.
pub fn Instance_delete(mut this: Instance) {
    Instance_done(&mut this);
}

/// Port of `static void Instance_writeField(const Row* super, RichString* str,
/// RowField field)` (`Instance.c:58`) — the `writeField` [`RowClass`] slot.
/// Looks up the dynamic column for `field` in `settings->dynamicColumns`, fetches
/// this instance's metric value, and formats it via
/// [`PCPDynamicColumn_writeAtomValue`]. The C `const Row*` receiver is a
/// `&dyn Object` downcast to [`Instance`].
pub fn Instance_writeField(super_: &dyn Object, str: &mut RichString, field: RowField) {
    let this = (super_ as &dyn Any)
        .downcast_ref::<Instance>()
        .expect("Instance_writeField: row is not an Instance");

    // int instid = Instance_getId(this);
    let instid = Instance_getId(this);

    // const Settings* settings = super->host->settings;
    let host = unsafe { &*(this.super_.host as *const Machine) };
    let settings = host
        .settings
        .as_ref()
        .expect("Instance_writeField: host->settings is NULL");

    // DynamicColumn* column = Hashtable_get(settings->dynamicColumns, field);
    // PCPDynamicColumn* cp = (PCPDynamicColumn*) column; if (!cp) return;
    let dyn_cols = match settings.dynamicColumns {
        Some(p) => unsafe { &*p },
        None => return,
    };
    let cp = match Hashtable_get(dyn_cols, field as u32) {
        Some(o) => match (o as &dyn Any).downcast_ref::<PCPDynamicColumn>() {
            Some(c) => c,
            None => return,
        },
        None => return,
    };

    // pmAtomValue atom; pmAtomValue* ap = &atom;
    // Metric metric = Metric_fromId(cp->id); const pmDesc* descp = Metric_desc(metric);
    let metric = Metric_fromId(cp.id);
    let descp = Metric_desc(metric);
    let dtype = unsafe { (*descp).type_ };

    // if (!Metric_instance(metric, instid, this->offset, ap, descp->type)) ap = NULL;
    let mut atom: pmAtomValue = unsafe { core::mem::zeroed() };
    let ap: *const pmAtomValue =
        if Metric_instance(metric, instid, this.offset as c_int, &mut atom, dtype).is_null() {
            ptr::null()
        } else {
            &atom
        };

    // PCPDynamicColumn_writeAtomValue(cp, str, settings, metric, instid, descp, ap);
    PCPDynamicColumn_writeAtomValue(cp, str, settings, metric as c_int, instid, descp, ap);

    // if (ap && descp->type == PM_TYPE_STRING) free(ap->cp);
    if !ap.is_null() && dtype == PM_TYPE_STRING {
        unsafe { libc::free(atom.cp as *mut libc::c_void) };
    }
}

/// Port of `static const char* Instance_externalName(Row* super)`
/// (`Instance.c:81`) — the `sortKeyString` [`RowClass`] slot.
///
/// C lazily fills `this->name` via `pmNameInDom(InDom_getId, Instance_getId,
/// &this->name)` when it is still `NULL`, then returns it. The ported
/// [`Row_SortKeyString`](crate::ported::row::Row_SortKeyString) slot is
/// `fn(&dyn Object) -> Option<&[u8]>` (shared ref), so the `&mut this->name`
/// fill cannot be performed here without violating aliasing (see the module
/// note). The ported body returns the current `name` (`None` until populated).
pub fn Instance_externalName(super_: &dyn Object) -> Option<&[u8]> {
    let this = (super_ as &dyn Any)
        .downcast_ref::<Instance>()
        .expect("Instance_externalName: row is not an Instance");
    // C: if (!this->name) (void)pmNameInDom(InDom_getId(this), Instance_getId(this), &this->name);
    // — the lazy fill needs `&mut this->name`, unavailable through this shared
    // slot; return the current name (reported substrate limitation).
    this.name.as_deref().map(str::as_bytes)
}

/// Port of `static int Instance_compareByKey(const Row* v1, const Row* v2, int
/// key)` (`Instance.c:90`). Looks up the dynamic column for `key` in the global
/// [`Platform_dynamicColumns`] registry, fetches each instance's metric value,
/// and three-way-compares by the metric's type (reversed operand order, matching
/// the C `SPACESHIP_*(atom2, atom1)`). Returns `0` for a negative `key`, `-1`
/// when the column or either instance is missing.
pub fn Instance_compareByKey(i1: &Instance, i2: &Instance, key: RowField) -> c_int {
    // if (key < 0) return 0;
    if key < 0 {
        return 0;
    }

    // Hashtable* dc = Platform_dynamicColumns();
    // const PCPDynamicColumn* column = Hashtable_get(dc, key); if (!column) return -1;
    let dc = unsafe { &*Platform_dynamicColumns() };
    let column = match Hashtable_get(dc, key as u32) {
        Some(o) => match (o as &dyn Any).downcast_ref::<PCPDynamicColumn>() {
            Some(c) => c,
            None => return -1,
        },
        None => return -1,
    };

    // Metric metric = Metric_fromId(column->id); unsigned int type = Metric_type(metric);
    let metric = Metric_fromId(column.id);
    let type_ = Metric_type(metric);

    // pmAtomValue atom1 = {0}, atom2 = {0};
    let mut atom1: pmAtomValue = unsafe { core::mem::zeroed() };
    let mut atom2: pmAtomValue = unsafe { core::mem::zeroed() };
    if Metric_instance(
        metric,
        Instance_getId(i1),
        i1.offset as c_int,
        &mut atom1,
        type_,
    )
    .is_null()
        || Metric_instance(
            metric,
            Instance_getId(i2),
            i2.offset as c_int,
            &mut atom2,
            type_,
        )
        .is_null()
    {
        if type_ == PM_TYPE_STRING {
            unsafe {
                libc::free(atom1.cp as *mut libc::c_void);
                libc::free(atom2.cp as *mut libc::c_void);
            }
        }
        return -1;
    }

    unsafe {
        match type_ {
            PM_TYPE_STRING => {
                // int cmp = SPACESHIP_NULLSTR(atom2.cp, atom1.cp); free(atom2.cp); free(atom1.cp);
                let s2 = if atom2.cp.is_null() {
                    None
                } else {
                    Some(CStr::from_ptr(atom2.cp).to_bytes())
                };
                let s1 = if atom1.cp.is_null() {
                    None
                } else {
                    Some(CStr::from_ptr(atom1.cp).to_bytes())
                };
                let cmp = spaceship_nullstr!(s2, s1);
                libc::free(atom2.cp as *mut libc::c_void);
                libc::free(atom1.cp as *mut libc::c_void);
                cmp
            }
            crate::ported::pcp::pmapi::PM_TYPE_32 => spaceship_number!(atom2.l, atom1.l),
            crate::ported::pcp::pmapi::PM_TYPE_U32 => spaceship_number!(atom2.ul, atom1.ul),
            crate::ported::pcp::pmapi::PM_TYPE_64 => spaceship_number!(atom2.ll, atom1.ll),
            crate::ported::pcp::pmapi::PM_TYPE_U64 => spaceship_number!(atom2.ull, atom1.ull),
            crate::ported::pcp::pmapi::PM_TYPE_FLOAT => spaceship_number!(atom2.f, atom1.f),
            crate::ported::pcp::pmapi::PM_TYPE_DOUBLE => spaceship_number!(atom2.d, atom1.d),
            // default: break; → return 0.
            _ => 0,
        }
    }
}

/// Port of `static int Instance_compare(const void* v1, const void* v2)`
/// (`Instance.c:141`) — the `compare` `Object` slot. Reads the active sort key
/// from `host->settings->ss`, compares via [`Instance_compareByKey`], applies a
/// PID (instance-id) tie-breaker, then flips the sign by the active direction.
/// The C `const void*` args become `&dyn Object` downcast to [`Instance`].
pub fn Instance_compare(v1: &dyn Object, v2: &dyn Object) -> i32 {
    let i1 = (v1 as &dyn Any)
        .downcast_ref::<Instance>()
        .expect("Instance_compare: v1 is not an Instance");
    let i2 = (v2 as &dyn Any)
        .downcast_ref::<Instance>()
        .expect("Instance_compare: v2 is not an Instance");

    // const ScreenSettings* ss = i1->super.host->settings->ss;
    let host = unsafe { &*(i1.super_.host as *const Machine) };
    let settings = host
        .settings
        .as_ref()
        .expect("Instance_compare: host->settings is NULL");
    let ss = &settings.screens[settings.ssIndex as usize];

    let key = ScreenSettings_getActiveSortKey(ss);
    let result = Instance_compareByKey(i1, i2, key);

    // Tie-breaker (needed to make tree mode more stable).
    if result == 0 {
        return spaceship_number!(Instance_getId(i1), Instance_getId(i2));
    }

    if ScreenSettings_getActiveDirection(ss) == 1 {
        result
    } else {
        -result
    }
}

/// Port of `const RowClass Instance_class` (`Instance.c:155`). Wires the base
/// `Row` slots plus the instance-specific `sortKeyString`/`writeField`; the
/// C `.super` `display`/`delete`/`compare` (ObjectClass-level) are realized by
/// the [`Object`] trait impl below. `extends = &Row_class.super_` (the
/// `ObjectClass` embedded in [`Row_class`]).
pub static Instance_class: RowClass = RowClass {
    super_: ObjectClass {
        extends: Some(&Row_class.super_),
    },
    isHighlighted: None,
    isVisible: None,
    writeField: Some(Instance_writeField),
    matchesFilter: None,
    sortKeyString: Some(Instance_externalName),
    compareByParent: None,
};

impl Object for Instance {
    /// C `Object_setClass(this, Class(Instance))`: the embedded [`ObjectClass`]
    /// of the `Instance` vtable.
    fn klass(&self) -> &'static ObjectClass {
        &Instance_class.super_
    }

    /// C `As_Row(this)` — this instance's [`RowClass`] vtable.
    fn row_class(&self) -> Option<&'static RowClass> {
        Some(&Instance_class)
    }

    /// C `(const Row*)this` — the embedded base (`super_`).
    fn as_row(&self) -> Option<&Row> {
        Some(&self.super_)
    }

    /// Mutable `(Row*)this` — needed when the [`InDomTable`] scan registers a
    /// fresh row and sets its `offset`/`updated`/`show` in place.
    fn as_row_mut(&mut self) -> Option<&mut Row> {
        Some(&mut self.super_)
    }

    /// C `Instance_class.super.display = Row_display`.
    fn display(&self, out: &mut RichString) {
        Row_display(self, out)
    }

    /// C `.compare = Instance_compare`.
    fn compare(&self, other: &dyn Object) -> i32 {
        Instance_compare(self, other)
    }
}
