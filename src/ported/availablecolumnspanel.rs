//! Stub scaffold for `AvailableColumnsPanel.c` — the picker Panel listing
//! every column a user can add to a process screen.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Nothing is portable yet
//!
//! Every function in this file bottoms out on substrate that is not ported.
//! Rather than gut the bodies and claim a port, all nine stay honest
//! `todo!()` stubs (`gen_port_report.py` counts `todo!()` bodies as
//! *stubbed*, not *ported*). The concrete blockers:
//!
//! - **`Process_fields[]`** — the platform-provided
//!   `const ProcessFieldData Process_fields[]` table (`.name` /
//!   `.description` per column). `process.rs` models the [`ProcessField`]
//!   *id* enum but NOT this data table; `settings.rs` likewise records it
//!   as unported substrate. Blocks [`AvailableColumnsPanel_insert`] and
//!   [`AvailableColumnsPanel_addPlatformColumns`].
//! - **`LAST_PROCESSFIELD` / `ROW_DYNAMIC_FIELDS`** (`RowField.h:53`,
//!   `ROW_DYNAMIC_FIELDS == LAST_RESERVED_FIELD`) — the reserved-field
//!   count bounds are not modeled. Blocks
//!   [`AvailableColumnsPanel_insert`] and
//!   [`AvailableColumnsPanel_addPlatformColumns`].
//! - **`Hashtable` + `Hashtable_foreach`** — `hashtable.rs` ports only the
//!   `nextPrime` math; the heap table and its `foreach` dispatch have no
//!   port. The `Hashtable* dynamicColumns` parameter itself has no ported
//!   type. Blocks [`AvailableColumnsPanel_addDynamicColumns`],
//!   [`AvailableColumnsPanel_fill`], and [`AvailableColumnsPanel_new`].
//! - **The full `DynamicColumn` struct** — `dynamiccolumn.rs` models only
//!   the `name` field; the callback here reads `table`, `heading`,
//!   `description`, and `caption`, none of which exist. `DynamicColumn_name`
//!   is itself a `todo!()` stub. Blocks
//!   [`AvailableColumnsPanel_addDynamicColumn`] and
//!   [`AvailableColumnsPanel_insert`].
//! - **`HandlerResult`** — the panel event-loop result enum
//!   (`IGNORED`/`HANDLED`) is not modeled anywhere; every sibling panel's
//!   `eventHandler` is likewise still stubbed. Together with the stubbed
//!   `Panel_selectByTyping` (`Panel.c:468`) and the stubbed
//!   `ColumnsPanel_update` (`ColumnsPanel.c:181`), this blocks
//!   [`AvailableColumnsPanel_eventHandler`].
//! - **`Platform_addDynamicScreenAvailableColumns`** — the `Platform`
//!   layer is not ported. Blocks [`AvailableColumnsPanel_addDynamicScreens`].
//! - **The `columns: Panel*` back-pointer** — the C struct borrows a
//!   `Panel*` owned by the `ColumnsPanel` that spawned this picker.
//!   No ported panel models such a cross-owned mutable alias, and
//!   [`AvailableColumnsPanel_insert`] / [`AvailableColumnsPanel_eventHandler`]
//!   mutate through it. Blocks [`AvailableColumnsPanel_new`] (which stores
//!   it) and the two functions that dereference it.
//!
//! The available substrate — `Panel_prune`/`Panel_init`/`Panel_setHeader`
//! (ported in `panel.rs`), `FunctionBar_new` (`functionbar.rs`), and
//! `ListItem_init` (`listitem.rs`) — covers only the *scaffolding* of
//! [`AvailableColumnsPanel_new`]/[`AvailableColumnsPanel_fill`]; the
//! column-enumeration payload each one exists to produce is entirely
//! gated behind the blockers above, so neither can be ported without
//! faking the part that matters.
//!
//! Stubbed (cannot be ported faithfully yet):
//! - [`AvailableColumnsPanel_delete`] (`AvailableColumnsPanel.c:32`) —
//!   `Panel_done` + `free`; in Rust the owned fields are released by
//!   `Drop`, so there is no algorithm to port (same class as
//!   `Panel_delete` / `ListItem_delete`).
//! - [`AvailableColumnsPanel_insert`] (`AvailableColumnsPanel.c:38`).
//! - [`AvailableColumnsPanel_eventHandler`] (`AvailableColumnsPanel.c:47`).
//! - [`AvailableColumnsPanel_addDynamicColumn`] (`AvailableColumnsPanel.c:83`).
//! - [`AvailableColumnsPanel_addDynamicColumns`] (`AvailableColumnsPanel.c:99`).
//! - [`AvailableColumnsPanel_addPlatformColumns`] (`AvailableColumnsPanel.c:105`).
//! - [`AvailableColumnsPanel_addDynamicScreens`] (`AvailableColumnsPanel.c:116`).
//! - [`AvailableColumnsPanel_fill`] (`AvailableColumnsPanel.c:120`).
//! - [`AvailableColumnsPanel_new`] (`AvailableColumnsPanel.c:130`).
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `static void AvailableColumnsPanel_delete(Object* object)`
/// from `AvailableColumnsPanel.c:32`. Pure teardown (`Panel_done(&this->super);
/// free(this);`) — the Rust struct would own its fields and free them via
/// `Drop`, so there is no algorithm to port (same class as `Panel_delete` /
/// `ListItem_delete`, both left as stubs).
pub fn AvailableColumnsPanel_delete() {
    todo!("port of AvailableColumnsPanel.c:32 — Drop releases the panel")
}

/// TODO: port of `static void AvailableColumnsPanel_insert(AvailableColumnsPanel* this,
/// int at, int key)` from `AvailableColumnsPanel.c:38`. Chooses the column
/// name via `key >= ROW_DYNAMIC_FIELDS ? DynamicColumn_name(key) :
/// Process_fields[key].name` and inserts a `ListItem_new(name, key)` into
/// `this->columns`. Blocked on the unported `Process_fields[]` table,
/// `ROW_DYNAMIC_FIELDS` (`RowField.h:53`), the stubbed `DynamicColumn_name`
/// (`DynamicColumn.c:36`), and the foreign-owned `columns: Panel*`
/// back-pointer.
pub fn AvailableColumnsPanel_insert() {
    todo!("port of AvailableColumnsPanel.c:38 — needs Process_fields[]/ROW_DYNAMIC_FIELDS/DynamicColumn_name + columns Panel*")
}

/// TODO: port of `static HandlerResult AvailableColumnsPanel_eventHandler(Panel* super,
/// int ch)` from `AvailableColumnsPanel.c:47`. On Enter/reclick/F5 it inserts
/// the selected column into `this->columns` and calls `ColumnsPanel_update`;
/// otherwise it falls through to `Panel_selectByTyping`. Blocked on the
/// unmodeled `HandlerResult` enum, the stubbed `Panel_selectByTyping`
/// (`Panel.c:468`), the stubbed `ColumnsPanel_update` (`ColumnsPanel.c:181`),
/// and the foreign-owned `columns: Panel*` back-pointer.
pub fn AvailableColumnsPanel_eventHandler() {
    todo!("port of AvailableColumnsPanel.c:47 — needs HandlerResult + Panel_selectByTyping + ColumnsPanel_update")
}

/// TODO: port of `static void AvailableColumnsPanel_addDynamicColumn(ht_key_t key,
/// void* value, void* data)` from `AvailableColumnsPanel.c:83`. A
/// `Hashtable_foreach` callback that skips `DynamicScreen` columns
/// (`column->table`), formats `"<heading|name> - <description|caption>"`,
/// and `Panel_add`s a `ListItem_new`. Blocked on the incomplete
/// `DynamicColumn` struct — `dynamiccolumn.rs` models only `name`, not the
/// `table` / `heading` / `description` / `caption` fields this reads.
pub fn AvailableColumnsPanel_addDynamicColumn() {
    todo!("port of AvailableColumnsPanel.c:83 — needs DynamicColumn table/heading/description/caption fields")
}

/// TODO: port of `static void AvailableColumnsPanel_addDynamicColumns(AvailableColumnsPanel* this,
/// Hashtable* dynamicColumns)` from `AvailableColumnsPanel.c:99`. Drives
/// `Hashtable_foreach(dynamicColumns, AvailableColumnsPanel_addDynamicColumn,
/// this)`. Blocked on the unported `Hashtable` type and `Hashtable_foreach`
/// (`hashtable.rs` ports only `nextPrime`), and transitively on the blocked
/// `AvailableColumnsPanel_addDynamicColumn` callback.
pub fn AvailableColumnsPanel_addDynamicColumns() {
    todo!("port of AvailableColumnsPanel.c:99 — needs Hashtable + Hashtable_foreach")
}

/// TODO: port of `static void AvailableColumnsPanel_addPlatformColumns(AvailableColumnsPanel* this)`
/// from `AvailableColumnsPanel.c:105`. Loops `1..LAST_PROCESSFIELD`, and for
/// each field with a description, formats `"<name> - <description>"` and
/// `Panel_add`s a `ListItem_new`. Blocked on the unported `Process_fields[]`
/// table and the `LAST_PROCESSFIELD` bound.
pub fn AvailableColumnsPanel_addPlatformColumns() {
    todo!("port of AvailableColumnsPanel.c:105 — needs Process_fields[] + LAST_PROCESSFIELD")
}

/// TODO: port of `static void AvailableColumnsPanel_addDynamicScreens(AvailableColumnsPanel* this,
/// const char* screen)` from `AvailableColumnsPanel.c:116`. Delegates to
/// `Platform_addDynamicScreenAvailableColumns(&this->super, screen)`. Blocked
/// on the unported `Platform` layer.
pub fn AvailableColumnsPanel_addDynamicScreens() {
    todo!("port of AvailableColumnsPanel.c:116 — needs Platform_addDynamicScreenAvailableColumns")
}

/// TODO: port of `void AvailableColumnsPanel_fill(AvailableColumnsPanel* this,
/// const char* dynamicScreen, Hashtable* dynamicColumns)` from
/// `AvailableColumnsPanel.c:120`. `Panel_prune`s the panel, then either
/// `addDynamicScreens` (dynamic screen) or `addPlatformColumns` +
/// `addDynamicColumns`. `Panel_prune` is ported, but every enumeration
/// branch it dispatches to is a blocked stub, and the `Hashtable*`
/// parameter has no ported type — a body that called the stubs would just
/// panic, so this stays a stub.
pub fn AvailableColumnsPanel_fill() {
    todo!("port of AvailableColumnsPanel.c:120 — dispatch targets (addPlatformColumns/addDynamicColumns/addDynamicScreens) all blocked")
}

/// TODO: port of `AvailableColumnsPanel* AvailableColumnsPanel_new(Panel* columns,
/// Hashtable* dynamicColumns)` from `AvailableColumnsPanel.c:130`. Allocates
/// the panel, builds its `FunctionBar_new`, `Panel_init`s it, sets the
/// "Available Columns" header, stores the `columns` back-pointer, and calls
/// `AvailableColumnsPanel_fill`. `FunctionBar_new` / `Panel_init` /
/// `Panel_setHeader` are ported, but this is blocked on the foreign-owned
/// `columns: Panel*` field (no ported cross-owned-panel alias) and on the
/// blocked `AvailableColumnsPanel_fill` it must call to populate the list.
pub fn AvailableColumnsPanel_new() {
    todo!("port of AvailableColumnsPanel.c:130 — needs columns Panel* field + AvailableColumnsPanel_fill")
}
