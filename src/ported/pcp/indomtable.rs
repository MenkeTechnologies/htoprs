//! Port of `pcp/InDomTable.c` + `.h` — htop's Performance Co-Pilot
//! instance-domain table: `InDomTable` (which "extends" [`Table`] directly,
//! not `ProcessTable`) plus its scan-vtable [`TableClass`]. One `InDomTable`
//! holds all the [`Instance`]s of one PCP instance domain (`pmInDom`), refreshed
//! from a representative metric each scan cycle.
//!
//! 1:1 faithful port; the C is the spec. It reuses the shared [`Table`] base and
//! its scan-vtable machinery (the DragonFly/PCP `*ProcessTable` precedent, but
//! for a plain `Table`), the ported [`Instance`] row, and the
//! [`Metric`](crate::ported::pcp::metric) libpcp wrapper; nothing is redeclared.
//!
//! # Get-or-create ownership
//!
//! The ported [`Table`] owns its rows as `Vec<Option<Box<dyn Object>>>` and
//! references them by index (not by the raw `Row*` C stores in its hashtable).
//! So [`InDomTable_getInstance`] resolves-or-creates and returns `(preExisting,
//! idx)` — constructing and [`Table_add`]-ing a fresh [`Instance`] itself (the
//! `ProcessTable_getProcess` precedent) rather than handing back a `Row*` for
//! the caller to add. C's `Table_add` (done in `InDomTable_goThroughEntries`
//! only for a new instance) is thus folded into `getInstance`; the caller then
//! sets `offset`/`updated`/`show` on the resolved row by index. `Table_add`
//! reads only the row id, so the offset-before-add ordering of the C is
//! behavior-equivalent.
//!
//! `RowField_keyAt` (declared in `InDomTable.h`) is defined in `Row.c`, not
//! `InDomTable.c`, so it is ported in `row.rs`, not here. `InDomTable_scan`
//! (also declared in `InDomTable.h`) has no definition in `InDomTable.c`
//! (it lives in `pcp/Platform.c`), so it is not ported here either.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::any::Any;
use std::os::raw::c_int;

use crate::ported::machine::Machine;
use crate::ported::pcp::instance::{Instance, Instance_new, Instance_setId};
use crate::ported::pcp::metric::{Metric_fromId, Metric_iterate};
use crate::ported::pcp::pmapi::pmInDom;
use crate::ported::table::{
    Table, TableClass, Table_add, Table_cleanupEntries, Table_init, Table_prepareEntries,
};

/// Port of `typedef struct InDomTable_` (`pcp/InDomTable.h:18`). "Extends"
/// [`Table`] via the embedded `super_` (C's first member). The `.c` sets both
/// `this->id` and `this->metricKey`, so both are real fields (the `.h` declares
/// them). `metricKey` is `c_int` — the `InDomTable_new` `int metricKey` param.
pub struct InDomTable {
    /// C `Table super`.
    pub super_: Table,
    /// C `pmInDom id` — shared by every metric in the table.
    pub id: pmInDom,
    /// C `unsigned int metricKey` — the representative metric using this indom.
    pub metricKey: c_int,
}

/// Scan-vtable glue (the [`TableClass`] slots) for [`InDomTable`]. C wires the
/// base `Table_prepareEntries`/`Table_cleanupEntries` directly; the ported base
/// entry points take `&mut Table`, so these thin `*mut Table` wrappers adapt
/// them to the [`TableClass`] function-pointer type (the DragonFly precedent).
impl InDomTable {
    fn scan_prepare(super_: *mut Table) {
        // SAFETY: `super_` is a live `*mut Table` (the base of an `InDomTable`).
        Table_prepareEntries(unsafe { &mut *super_ });
    }

    fn scan_cleanup(super_: *mut Table) {
        // SAFETY: `super_` is a live `*mut Table` (the base of an `InDomTable`).
        Table_cleanupEntries(unsafe { &mut *super_ });
    }
}

/// Port of `static void InDomTable_iterateEntries(Table* super)`
/// (`InDomTable.c:86`) — the `iterate` [`TableClass`] slot. Downcasts the base
/// `*mut Table` to `*mut InDomTable` and drives [`InDomTable_goThroughEntries`].
pub fn InDomTable_iterateEntries(super_: *mut Table) {
    // InDomTable* this = (InDomTable*) super;
    let this = super_ as *mut InDomTable;
    // SAFETY: `super_` is the base of a live `InDomTable`.
    InDomTable_goThroughEntries(unsafe { &mut *this });
}

/// Port of `const TableClass InDomTable_class` (`InDomTable.c:91`). The C
/// `.super = { .extends = Class(Table), .delete = InDomTable_delete }` half is
/// class identity / `Drop` in Rust (the ported [`TableClass`] models only the
/// scan slots), so only `prepare`/`iterate`/`cleanup` are wired: the base
/// `Table_prepareEntries`/`Table_cleanupEntries` (via the adapters) and the
/// instance-domain [`InDomTable_iterateEntries`].
pub static InDomTable_class: TableClass = TableClass {
    prepare: Some(InDomTable::scan_prepare),
    iterate: Some(InDomTable_iterateEntries),
    cleanup: Some(InDomTable::scan_cleanup),
};

/// Port of `InDomTable* InDomTable_new(Machine* host, pmInDom indom, int
/// metricKey)` (`InDomTable.c:31`). Allocates the table (C `xCalloc`), stores
/// `metricKey`/`id`, runs the base [`Table_init`], and wires the scan vtable
/// (C's `Object_setClass(this, Class(InDomTable))`, modeled as the `klass`
/// pointer on the base `Table`). Returns the owning `Box` (C returns the
/// `InDomTable*`).
pub fn InDomTable_new(host: *const Machine, indom: pmInDom, metricKey: c_int) -> Box<InDomTable> {
    let mut this = Box::new(InDomTable {
        super_: Table::empty(),
        // this->metricKey = metricKey; this->id = indom;
        id: indom,
        metricKey,
    });

    // Table_init(super, Class(Instance), host);  — the Instance class tag is
    // class identity in Rust (concrete row type), so the param is dropped.
    Table_init(&mut this.super_, host);

    this.super_.klass = &InDomTable_class as *const TableClass;

    this
}

/// Port of `void InDomTable_done(InDomTable* this)` (`InDomTable.c:43`).
/// `Table_done(&this->super)`.
pub fn InDomTable_done(this: &mut InDomTable) {
    crate::ported::table::Table_done(&mut this.super_);
}

/// Port of `static void InDomTable_delete(Object* cast)` (`InDomTable.c:47`).
/// The C body is `InDomTable_done(this); free(this);`. Taking `this` by value
/// reproduces `free(this)`; [`InDomTable_done`] runs the teardown, then the
/// consumed struct drops (the dragonfly `ProcessTable_delete` precedent). Not
/// wired into the [`TableClass`] — the ported class models `delete` via `Drop`.
pub fn InDomTable_delete(mut this: InDomTable) {
    InDomTable_done(&mut this);
}

/// Port of `static Instance* InDomTable_getInstance(InDomTable* this, int id,
/// bool* preExisting)` (`InDomTable.c:53`). Resolves the instance with `id` from
/// the base table's lookup, or constructs a fresh one via [`Instance_new`] +
/// [`Instance_setId`] and registers it with [`Table_add`]. Returns
/// `(preExisting, idx)` — the row's index in `super_.rows` (the
/// `ProcessTable_getProcess` model; see the module note on the folded
/// `Table_add`).
fn InDomTable_getInstance(this: &mut InDomTable, id: c_int) -> (bool, usize) {
    // Instance* inst = (Instance*) Hashtable_get(super->table, id);
    if let Some(&idx) = this.super_.table.get(&id) {
        // assert(Instance_getId(inst) == id);  (the Vector_indexOf assert is
        // implied by a valid `idx`).
        debug_assert_eq!(
            this.super_.rows[idx].as_ref().unwrap().as_row().unwrap().id,
            id
        );
        return (true, idx);
    }

    // inst = Instance_new(super->host, this); assert(inst->name == NULL);
    // Instance_setId(inst, id);
    let host = this.super_.host;
    let this_ptr = this as *const InDomTable;
    let mut inst = Instance_new(host, this_ptr);
    debug_assert!(inst.name.is_none());
    Instance_setId(&mut inst, id);

    let idx = this.super_.rows.len();
    // Table_add(super, row) — folded here from goThroughEntries (see module note).
    Table_add(&mut this.super_, inst);
    (false, idx)
}

/// Port of `static void InDomTable_goThroughEntries(InDomTable* this)`
/// (`InDomTable.c:68`). Iterates every instance of the representative metric
/// ([`Metric_iterate`]), resolving-or-creating each [`Instance`] via
/// [`InDomTable_getInstance`], stamping its `offset`, and marking the row
/// `updated`/`show`. (The C's conditional `Table_add` for a new instance is
/// folded into `getInstance`; see the module note.)
pub fn InDomTable_goThroughEntries(this: &mut InDomTable) {
    // int instid = -1, offset = -1;
    let mut instid: c_int = -1;
    let mut offset: c_int = -1;

    // while (Metric_iterate(this->metricKey, &instid, &offset, sizeof(Instance)))
    while Metric_iterate(
        Metric_fromId(this.metricKey as usize),
        &mut instid,
        &mut offset,
        core::mem::size_of::<Instance>(),
    ) {
        let (_preExisting, idx) = InDomTable_getInstance(this, instid);

        // inst->offset = offset >= 0 ? offset : 0;
        let obj = this.super_.rows[idx].as_mut().unwrap().as_mut();
        let inst = (obj as &mut dyn Any)
            .downcast_mut::<Instance>()
            .expect("InDomTable_goThroughEntries: row is not an Instance");
        inst.offset = if offset >= 0 { offset as u32 } else { 0 };
        // row->updated = true; row->show = true;
        inst.super_.updated = true;
        inst.super_.show = true;
    }
}
