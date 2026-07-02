//! Partial port of `ColumnsPanel.c` — htop's "Active Columns" editor panel.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. A C fn `Foo_bar(Foo* this)`
//! ports to a free fn `Foo_bar(this: &mut Foo)` (the same shape the
//! `Panel.c`/`ListItem.c` ports use: free fns, not methods).
//!
//! # Data model
//!
//! htop's `ColumnsPanel` (`ColumnsPanel.h:17`) embeds a `Panel super`,
//! plus `ScreenSettings* ss`, `bool* changed`, and `bool moving`. The
//! [`ColumnsPanel`] struct here models only the two fields the ported
//! function touches — the embedded `super` [`Panel`] and the `moving`
//! flag — following the same reduced-struct precedent as `listitem.rs`
//! (which omits `Object super`) and `settings.rs`'s `ScreenSettings`
//! (which omits every field its ported fns never read). The non-owning
//! `ss`/`changed` back-pointers are omitted because the only functions
//! that dereference them (`_fill`/`_new`/`_update`) are blocked on the
//! `ScreenSettings.fields`/`.flags` substrate (see below) and stay stubs.
//!
//! # Ported (self-contained, no unported substrate)
//!
//! - [`ColumnsPanel_cancelMoving`] (`ColumnsPanel.c:37`) — clears the
//!   `moving` flag on every list row, clears the panel's own `moving`,
//!   and restores `PANEL_SELECTION_FOCUS`. The C loop casts each
//!   `Panel_get` result to `ListItem*` and writes `item->moving`; the
//!   ported `Panel_get` hands back an immutable `&dyn Object`, so the
//!   faithful analog reaches into `super.items` and downcasts each row
//!   `&mut dyn Object` to `&mut ListItem` (via the `Any` supertrait) to
//!   write the same field. `Panel_setSelectionColor`/`PANEL_SELECTION_FOCUS`
//!   are the ported `panel.rs`/`crt.rs` substrate.
//!
//! # Stubbed (cannot be ported faithfully yet)
//!
//! - [`ColumnsPanel_delete`] (`ColumnsPanel.c:31`) — `Panel_done` +
//!   `free`. [`ColumnsPanel`] owns its fields, so `Drop` releases them;
//!   there is no algorithm to port (same precedent as every sibling
//!   `_delete`, e.g. `Panel_delete`/`ListItem_delete`).
//! - [`ColumnsPanel_eventHandler`] (`ColumnsPanel.c:48`) — returns a
//!   `HandlerResult` (`IGNORED`/`HANDLED`/`BREAK_LOOP`), an enum that is
//!   not ported in any module yet, and calls `Panel_selectByTyping`
//!   (still a `todo!()` stub in `panel.rs`) and [`ColumnsPanel_update`]
//!   (stub). No faithful body without that substrate.
//! - [`ColumnsPanel_add`] (`ColumnsPanel.c:137`) — indexes the platform
//!   `Process_fields[key].name` table and `LAST_PROCESSFIELD`, and for
//!   dynamic columns calls `Hashtable_get` for a `DynamicColumn` whose
//!   `heading` it reads. The `Process_fields[]` table, `Hashtable_get`,
//!   and `DynamicColumn.heading` are all unported.
//! - [`ColumnsPanel_fill`] (`ColumnsPanel.c:156`) — iterates `ss->fields`
//!   and calls [`ColumnsPanel_add`]. The ported `ScreenSettings`
//!   (`settings.rs`) has no `fields` array, and `_add` is itself stubbed.
//! - [`ColumnsPanel_new`] (`ColumnsPanel.c:164`) — allocates the panel
//!   and calls [`ColumnsPanel_fill`] unconditionally; blocked transitively
//!   on `_fill` and on the missing `ss->fields`.
//! - [`ColumnsPanel_update`] (`ColumnsPanel.c:181`) — rewrites
//!   `ss->fields`/`ss->flags` from the list, OR-ing `Process_fields[key].flags`.
//!   Needs `ScreenSettings.fields`/`.flags` and the `Process_fields[]`
//!   table — none ported.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::crt::ColorElements;
use crate::ported::listitem::ListItem;
use crate::ported::object::Object;
use crate::ported::panel::{Panel, Panel_setSelectionColor, Panel_size};

/// Reduced model of the C `ColumnsPanel` struct (`ColumnsPanel.h:17`).
/// Only the embedded `Panel super` and the `moving` flag are modeled —
/// the two fields [`ColumnsPanel_cancelMoving`] touches. The C
/// `ScreenSettings* ss` and `bool* changed` back-pointers are omitted
/// because the only functions that read them (`_fill`/`_new`/`_update`)
/// are blocked on the missing `ScreenSettings.fields`/`.flags` substrate
/// and remain stubs. `super_` avoids the Rust `super` keyword, matching
/// the `process.rs` `super_: Row` convention.
pub struct ColumnsPanel {
    /// C `Panel super` — the embedded panel base.
    pub super_: Panel,
    /// C `bool moving` — whether the panel is in row-reorder mode.
    pub moving: bool,
}

/// TODO: port of `static void ColumnsPanel_delete(Object* object)` from
/// `ColumnsPanel.c:31`. `Panel_done(&this->super)` + `free(this)` — the
/// owned fields are released by `Drop`, so there is no algorithm to port
/// (same precedent as `Panel_delete`/`ListItem_delete`). Left as a stub.
pub fn ColumnsPanel_delete() {
    todo!("port of ColumnsPanel.c:31 — Drop releases the panel")
}

/// Port of `static void ColumnsPanel_cancelMoving(ColumnsPanel* this)`
/// from `ColumnsPanel.c:37`. Walks every row of the embedded panel and
/// clears its `moving` flag, clears the panel's own `moving`, then
/// restores `Panel_setSelectionColor(super, PANEL_SELECTION_FOCUS)`.
///
/// The C loop is `for (i < Panel_size(super)) { ListItem* item =
/// (ListItem*) Panel_get(super, i); if (item) item->moving = false; }`.
/// The ported `Panel_get` returns an immutable `&dyn Object`, so the
/// faithful mutating analog indexes `super.items` directly and downcasts
/// each row `&mut dyn Object` to `&mut ListItem` via the `Any` supertrait
/// (the safe-Rust analog of the C `(ListItem*)` cast). A `Vec` element is
/// never null, so the C `if (item)` guard is always taken.
pub fn ColumnsPanel_cancelMoving(this: &mut ColumnsPanel) {
    let super_ = &mut this.super_;
    let size = Panel_size(super_);
    for i in 0..size {
        let obj: &mut dyn Object = super_.items[i as usize].as_mut();
        let any: &mut dyn core::any::Any = obj;
        if let Some(item) = any.downcast_mut::<ListItem>() {
            item.moving = false;
        }
    }
    this.moving = false;
    Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_SELECTION_FOCUS);
}

/// TODO: port of `static HandlerResult ColumnsPanel_eventHandler(Panel* super,
/// int ch)` from `ColumnsPanel.c:48`. Returns a `HandlerResult`
/// (`IGNORED`/`HANDLED`/`BREAK_LOOP`) — an enum not ported in any module
/// yet — and depends on `Panel_selectByTyping` (a `todo!()` stub in
/// `panel.rs`) and [`ColumnsPanel_update`] (stub). Left as a stub.
pub fn ColumnsPanel_eventHandler() {
    todo!("port of ColumnsPanel.c:48 — needs HandlerResult enum + Panel_selectByTyping")
}

/// TODO: port of `static void ColumnsPanel_add(Panel* super, unsigned int key,
/// Hashtable* columns)` from `ColumnsPanel.c:137`. Reads
/// `Process_fields[key].name`/`LAST_PROCESSFIELD` and, for dynamic
/// columns, `Hashtable_get(columns, key)` then the `DynamicColumn`'s
/// `heading`/`name`. The `Process_fields[]` table, `Hashtable_get`, and
/// `DynamicColumn.heading` are all unported. Left as a stub.
pub fn ColumnsPanel_add() {
    todo!("port of ColumnsPanel.c:137 — needs Process_fields[], Hashtable_get, DynamicColumn.heading")
}

/// TODO: port of `void ColumnsPanel_fill(ColumnsPanel* this,
/// ScreenSettings* ss, Hashtable* columns)` from `ColumnsPanel.c:156`.
/// Iterates `ss->fields` calling [`ColumnsPanel_add`]. The ported
/// `ScreenSettings` (`settings.rs`) has no `fields` array and `_add` is
/// itself stubbed. Left as a stub.
pub fn ColumnsPanel_fill() {
    todo!("port of ColumnsPanel.c:156 — needs ScreenSettings.fields + ColumnsPanel_add")
}

/// TODO: port of `ColumnsPanel* ColumnsPanel_new(ScreenSettings* ss,
/// Hashtable* columns, bool* changed)` from `ColumnsPanel.c:164`.
/// Allocates the panel and calls [`ColumnsPanel_fill`] unconditionally;
/// blocked transitively on `_fill` and the missing `ss->fields`. Left as
/// a stub.
pub fn ColumnsPanel_new() {
    todo!("port of ColumnsPanel.c:164 — needs ColumnsPanel_fill + ScreenSettings.fields")
}

/// TODO: port of `void ColumnsPanel_update(Panel* super)` from
/// `ColumnsPanel.c:181`. Rewrites `ss->fields`/`ss->flags` from the list,
/// OR-ing `Process_fields[key].flags`. Needs `ScreenSettings.fields`/
/// `.flags` and the `Process_fields[]` table — none ported. Left as a
/// stub.
pub fn ColumnsPanel_update() {
    todo!("port of ColumnsPanel.c:181 — needs ScreenSettings.fields/.flags + Process_fields[]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::panel::{Panel_add, Panel_new};

    /// Build a `ColumnsPanel` whose embedded panel holds `n` `ListItem`
    /// rows, all with `moving == true` and the given `moving` panel flag.
    fn panel_with_moving_rows(n: usize, moving: bool) -> ColumnsPanel {
        let mut super_ = Panel_new(0, 0, 10, 10, None);
        for i in 0..n {
            Panel_add(
                &mut super_,
                Box::new(ListItem {
                    value: format!("field{i}"),
                    key: i as i32,
                    moving: true,
                }),
            );
        }
        ColumnsPanel { super_, moving }
    }

    /// Read back the `moving` flag of row `i` via the same `Any` downcast
    /// the ported function uses.
    fn row_moving(cp: &ColumnsPanel, i: usize) -> bool {
        let obj: &dyn Object = cp.super_.items[i].as_ref();
        let any: &dyn core::any::Any = obj;
        any.downcast_ref::<ListItem>().unwrap().moving
    }

    #[test]
    fn cancel_moving_clears_all_rows_and_panel_flag() {
        let mut cp = panel_with_moving_rows(3, true);
        // Enter the follow-selection color as the C move mode would.
        Panel_setSelectionColor(&mut cp.super_, ColorElements::PANEL_SELECTION_FOLLOW);

        ColumnsPanel_cancelMoving(&mut cp);

        assert!(!cp.moving);
        for i in 0..3 {
            assert!(!row_moving(&cp, i), "row {i} moving not cleared");
        }
        assert_eq!(
            cp.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
    }

    #[test]
    fn cancel_moving_on_empty_panel_only_touches_flags() {
        let mut cp = panel_with_moving_rows(0, true);
        Panel_setSelectionColor(&mut cp.super_, ColorElements::PANEL_SELECTION_FOLLOW);

        ColumnsPanel_cancelMoving(&mut cp);

        assert!(!cp.moving);
        assert_eq!(Panel_size(&cp.super_), 0);
        assert_eq!(
            cp.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
    }

    #[test]
    fn cancel_moving_is_idempotent() {
        let mut cp = panel_with_moving_rows(2, false);
        // Already-cleared rows stay cleared; running twice is a no-op.
        for i in 0..2 {
            let obj: &mut dyn Object = cp.super_.items[i].as_mut();
            let any: &mut dyn core::any::Any = obj;
            any.downcast_mut::<ListItem>().unwrap().moving = false;
        }
        ColumnsPanel_cancelMoving(&mut cp);
        ColumnsPanel_cancelMoving(&mut cp);
        assert!(!cp.moving);
        for i in 0..2 {
            assert!(!row_moving(&cp, i));
        }
    }
}
