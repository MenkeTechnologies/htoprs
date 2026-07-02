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
//! - [`ColumnsPanel_eventHandler`] (`ColumnsPanel.c:48`) — the full
//!   key-dispatch `switch` returning [`HandlerResult`]
//!   (`IGNORED`/`HANDLED`/`BREAK_LOOP`), now that `HandlerResult`,
//!   `EVENT_PANEL_LOST_FOCUS`, and `Panel_selectByTyping` are ported in
//!   `panel.rs` and the key codes (`KEY_ENTER`/`KEY_MOUSE`/`KEY_F(n)`/
//!   `KEY_DC`/`KEY_DEL_MAC`) exist in `crt.rs`. Enter toggles move mode,
//!   mouse/lost-focus cancel it, MoveUp/MoveDn reorder (with the C
//!   `KEY_UP`/`KEY_DOWN` while-moving fallthrough), Remove drops a row, and
//!   the default case delegates to `Panel_selectByTyping`. Its C tail
//!   `if (result == HANDLED) ColumnsPanel_update(super);` is emitted
//!   faithfully but transitively hits the still-stubbed
//!   [`ColumnsPanel_update`] (see below): `HANDLED` paths mutate the panel
//!   then panic in that stub; `IGNORED` paths run to completion.
//!
//! # Stubbed (cannot be ported faithfully yet)
//!
//! - [`ColumnsPanel_delete`] (`ColumnsPanel.c:31`) — `Panel_done` +
//!   `free`. [`ColumnsPanel`] owns its fields, so `Drop` releases them;
//!   there is no algorithm to port (same precedent as every sibling
//!   `_delete`, e.g. `Panel_delete`/`ListItem_delete`).
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

use crate::ported::crt::{
    ColorElements, KEY_DC, KEY_DEL_MAC, KEY_DOWN, KEY_ENTER, KEY_F, KEY_MOUSE, KEY_RECLICK, KEY_UP,
};
use crate::ported::listitem::ListItem;
use crate::ported::object::Object;
use crate::ported::panel::{
    HandlerResult, Panel, Panel_getSelectedIndex, Panel_moveSelectedDown, Panel_moveSelectedUp,
    Panel_remove, Panel_selectByTyping, Panel_setSelectionColor, Panel_size,
    EVENT_PANEL_LOST_FOCUS,
};

// `KEY_F(n)`/char case labels from the C `switch` cannot appear as Rust match
// patterns directly (a `const fn` call and an `as` cast are not patterns), so
// bind them as module `const`s — the same idiom `panel.rs` uses for its
// `KEY_CTRL(...)`/`'^'` case labels. These are `const`, not `pub fn`, so the
// port-purity gate (which only rejects unknown `pub fn` names) is unaffected.
const KEY_F7: i32 = KEY_F(7);
const KEY_F8: i32 = KEY_F(8);
const KEY_F9: i32 = KEY_F(9);
const LEFT_BRACKET: i32 = b'[' as i32;
const MINUS: i32 = b'-' as i32;
const RIGHT_BRACKET: i32 = b']' as i32;
const PLUS: i32 = b'+' as i32;

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

/// Port of `static HandlerResult ColumnsPanel_eventHandler(Panel* super,
/// int ch)` from `ColumnsPanel.c:48`. The key-dispatch `switch`: Enter
/// toggles move mode (setting/clearing the selected row's `moving` flag and
/// the follow-selection color), a mouse click cancels an in-progress move,
/// the MoveUp/MoveDn keys reorder the selection (with `KEY_UP`/`KEY_DOWN`
/// falling through only while `moving`), the Remove keys drop a row (never
/// the last one), a lost-focus event cancels a move, and any other graphic
/// char delegates to [`Panel_selectByTyping`] (a `BREAK_LOOP` from typing is
/// downgraded to `IGNORED`).
///
/// The C takes `Panel* super` and casts it to `ColumnsPanel*`; the reduced
/// model embeds the panel as `this.super_`, so — like
/// [`ColumnsPanel_cancelMoving`] — the port takes `this: &mut ColumnsPanel`.
/// The C `(ListItem*) Panel_getSelected(super)` write of `item->moving = true`
/// is reproduced by indexing `super_.items[selected]` and downcasting the
/// `&mut dyn Object` to `&mut ListItem` via the `Any` supertrait (the same
/// mutating analog `_cancelMoving` uses, since ported `Panel_getSelected`
/// hands back an immutable `&dyn Object`). The `selected < size` guard makes
/// that index always valid (the C `if (selectedItem)` null-guard is dead once
/// `size > 0`).
///
/// The C fallthrough `case KEY_UP: if (!this->moving) break; /* else
/// fallthrough */` is expressed as a guarded arm (`KEY_UP if !this.moving`)
/// ordered before the shared MoveUp arm: when `moving` is false the guard
/// arm matches and does nothing (result stays `IGNORED`); when `moving` the
/// guard fails and `KEY_UP` falls to the shared arm — bit-for-bit the C
/// fallthrough. Same for `KEY_DOWN`.
///
/// # Transitive block
///
/// The C tail `if (result == HANDLED) ColumnsPanel_update(super);` is emitted
/// faithfully, but [`ColumnsPanel_update`] is still a `todo!()` stub (it
/// rewrites `ss->fields`/`ss->flags` from `Process_fields[]`, none of which
/// the reduced `ColumnsPanel` models — no `ss` field exists). So every
/// `HANDLED`-returning path performs its panel-level mutation and *then*
/// panics in that stub; only the `IGNORED`/`BREAK_LOOP→IGNORED` paths run to
/// completion. This is the honest transitive block, not a fake port: the
/// dispatch + panel mutations are ported; the `ss` write-back is not modeled.
pub fn ColumnsPanel_eventHandler(this: &mut ColumnsPanel, ch: i32) -> HandlerResult {
    let selected = Panel_getSelectedIndex(&this.super_);
    let mut result = HandlerResult::IGNORED;
    let size = Panel_size(&this.super_);

    match ch {
        0x0a | 0x0d | KEY_ENTER | KEY_RECLICK => {
            if selected < size {
                if this.moving {
                    ColumnsPanel_cancelMoving(this);
                } else {
                    this.moving = true;
                    Panel_setSelectionColor(
                        &mut this.super_,
                        ColorElements::PANEL_SELECTION_FOLLOW,
                    );
                    // C: ListItem* selectedItem = (ListItem*) Panel_getSelected(super);
                    //    if (selectedItem) selectedItem->moving = true;
                    let sel = this.super_.selected as usize;
                    let any: &mut dyn core::any::Any = this.super_.items[sel].as_mut();
                    if let Some(item) = any.downcast_mut::<ListItem>() {
                        item.moving = true;
                    }
                }
                result = HandlerResult::HANDLED;
            }
        }
        KEY_MOUSE => {
            if this.moving {
                // Single click while in move mode: cancel move mode.
                ColumnsPanel_cancelMoving(this);
                result = HandlerResult::HANDLED;
            }
            // else: just select the item, do not enter move mode.
        }
        // C: case KEY_UP: if (!this->moving) break; /* else fallthrough */
        KEY_UP if !this.moving => {}
        KEY_UP | KEY_F7 | LEFT_BRACKET | MINUS => {
            if selected < size {
                Panel_moveSelectedUp(&mut this.super_);
            }
            result = HandlerResult::HANDLED;
        }
        // C: case KEY_DOWN: if (!this->moving) break; /* else fallthrough */
        KEY_DOWN if !this.moving => {}
        KEY_DOWN | KEY_F8 | RIGHT_BRACKET | PLUS => {
            if selected < size - 1 {
                Panel_moveSelectedDown(&mut this.super_);
            }
            result = HandlerResult::HANDLED;
        }
        KEY_F9 | KEY_DC | KEY_DEL_MAC => {
            if size > 1 && selected < size {
                Panel_remove(&mut this.super_, selected);
            }
            result = HandlerResult::HANDLED;
        }
        EVENT_PANEL_LOST_FOCUS => {
            if this.moving {
                ColumnsPanel_cancelMoving(this);
            }
            result = HandlerResult::HANDLED;
        }
        _ => {
            // C: isgraph((unsigned char)ch) == ASCII 0x21..=0x7e in the C
            // locale == `is_ascii_graphic`.
            if 0 < ch && ch < 255 && (ch as u8).is_ascii_graphic() {
                result = Panel_selectByTyping(&mut this.super_, ch);
            }
            if result == HandlerResult::BREAK_LOOP {
                result = HandlerResult::IGNORED;
            }
        }
    }

    if result == HandlerResult::HANDLED {
        ColumnsPanel_update(&mut this.super_);
    }

    result
}

/// TODO: port of `static void ColumnsPanel_add(Panel* super, unsigned int key,
/// Hashtable* columns)` from `ColumnsPanel.c:137`. Reads
/// `Process_fields[key].name`/`LAST_PROCESSFIELD` and, for dynamic
/// columns, `Hashtable_get(columns, key)` then the `DynamicColumn`'s
/// `heading`/`name`. The `Process_fields[]` table, `Hashtable_get`, and
/// `DynamicColumn.heading` are all unported. Left as a stub.
pub fn ColumnsPanel_add() {
    todo!(
        "port of ColumnsPanel.c:137 — needs Process_fields[], Hashtable_get, DynamicColumn.heading"
    )
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
/// `.flags` and the `Process_fields[]` table — none ported, and the reduced
/// [`ColumnsPanel`] has no `ss`/`changed` fields at all. Left as a stub. The
/// signature matches the C `Panel* super` so [`ColumnsPanel_eventHandler`]'s
/// `HANDLED` tail can call it faithfully; every such call reaches this
/// `todo!()` (the transitive block documented on the event handler).
pub fn ColumnsPanel_update(super_: &mut Panel) {
    let _ = super_;
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

    // ── eventHandler ──────────────────────────────────────────────────
    //
    // The C tail `if (result == HANDLED) ColumnsPanel_update(super)` reaches
    // the still-stubbed `ColumnsPanel_update` (`todo!()`), so every
    // `HANDLED`-returning path mutates the panel and *then* panics. The
    // `IGNORED`/`BREAK_LOOP→IGNORED` paths run to completion and are asserted
    // directly; the `HANDLED` paths are driven through `catch_unwind`, which
    // both confirms the transitive block (the stub fired) and lets the panel
    // mutation — performed before the panic — be inspected afterwards.

    /// The `ListItem.value` of row `i`, via the same `Any` downcast the port
    /// uses.
    fn row_value(cp: &ColumnsPanel, i: usize) -> String {
        let obj: &dyn Object = cp.super_.items[i].as_ref();
        let any: &dyn core::any::Any = obj;
        any.downcast_ref::<ListItem>().unwrap().value.clone()
    }

    /// Drive a `HANDLED`-returning key and assert the tail hit the
    /// `ColumnsPanel_update` `todo!()` stub (the documented transitive block).
    /// The panel mutation done before the panic persists in `cp`.
    fn expect_update_stub_panic(cp: &mut ColumnsPanel, ch: i32) {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ColumnsPanel_eventHandler(cp, ch);
        }));
        assert!(
            r.is_err(),
            "HANDLED tail should reach the ColumnsPanel_update stub and panic"
        );
    }

    #[test]
    fn enter_on_empty_panel_is_ignored() {
        // selected (0) < size (0) is false -> result stays IGNORED, no update.
        let mut cp = panel_with_moving_rows(0, false);
        let r = ColumnsPanel_eventHandler(&mut cp, KEY_ENTER);
        assert_eq!(r, HandlerResult::IGNORED);
        assert!(!cp.moving);
    }

    #[test]
    fn mouse_without_move_mode_is_ignored() {
        let mut cp = panel_with_moving_rows(3, false);
        for i in 0..3 {
            let obj: &mut dyn Object = cp.super_.items[i].as_mut();
            (obj as &mut dyn core::any::Any)
                .downcast_mut::<ListItem>()
                .unwrap()
                .moving = false;
        }
        let r = ColumnsPanel_eventHandler(&mut cp, KEY_MOUSE);
        assert_eq!(r, HandlerResult::IGNORED);
        assert!(!cp.moving);
    }

    #[test]
    fn arrow_keys_without_move_mode_are_ignored() {
        // C: case KEY_UP/KEY_DOWN: if (!this->moving) break; -> IGNORED.
        let mut up = panel_with_moving_rows(3, false);
        assert_eq!(
            ColumnsPanel_eventHandler(&mut up, KEY_UP),
            HandlerResult::IGNORED
        );
        assert_eq!(up.super_.selected, 0);

        let mut down = panel_with_moving_rows(3, false);
        assert_eq!(
            ColumnsPanel_eventHandler(&mut down, KEY_DOWN),
            HandlerResult::IGNORED
        );
        assert_eq!(down.super_.selected, 0);
    }

    #[test]
    fn default_hash_delegates_to_select_by_typing_and_is_ignored() {
        // '#' is graphic, so the default case calls Panel_selectByTyping,
        // which special-cases '#' -> IGNORED. No HANDLED, no update panic.
        let mut cp = panel_with_moving_rows(3, false);
        let r = ColumnsPanel_eventHandler(&mut cp, b'#' as i32);
        assert_eq!(r, HandlerResult::IGNORED);
    }

    #[test]
    fn default_q_on_empty_buffer_breaks_then_downgrades_to_ignored() {
        // Panel_selectByTyping('q') on an empty buffer returns BREAK_LOOP;
        // the handler downgrades BREAK_LOOP -> IGNORED (C:118-119).
        let mut cp = panel_with_moving_rows(2, false);
        let r = ColumnsPanel_eventHandler(&mut cp, b'q' as i32);
        assert_eq!(r, HandlerResult::IGNORED);
    }

    #[test]
    fn default_nongraphic_is_ignored() {
        // 0x08 (backspace) is non-graphic: guard false, result stays IGNORED.
        let mut cp = panel_with_moving_rows(2, false);
        let r = ColumnsPanel_eventHandler(&mut cp, 0x08);
        assert_eq!(r, HandlerResult::IGNORED);
    }

    #[test]
    fn enter_enters_move_mode_then_hits_update_stub() {
        let mut cp = panel_with_moving_rows(3, false);
        // Rows start with moving=true from the fixture; clear so we can see
        // the handler set the *selected* row's flag.
        for i in 0..3 {
            let obj: &mut dyn Object = cp.super_.items[i].as_mut();
            (obj as &mut dyn core::any::Any)
                .downcast_mut::<ListItem>()
                .unwrap()
                .moving = false;
        }
        cp.super_.selected = 1;
        expect_update_stub_panic(&mut cp, KEY_ENTER);
        assert!(cp.moving);
        assert_eq!(
            cp.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOLLOW
        );
        assert!(row_moving(&cp, 1), "selected row should be marked moving");
        assert!(!row_moving(&cp, 0));
    }

    #[test]
    fn enter_while_moving_cancels_move_then_hits_update_stub() {
        let mut cp = panel_with_moving_rows(3, true);
        Panel_setSelectionColor(&mut cp.super_, ColorElements::PANEL_SELECTION_FOLLOW);
        expect_update_stub_panic(&mut cp, KEY_ENTER);
        assert!(!cp.moving);
        assert_eq!(
            cp.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
    }

    #[test]
    fn f7_moves_selection_up_then_hits_update_stub() {
        let mut cp = panel_with_moving_rows(3, false); // field0,field1,field2
        cp.super_.selected = 2;
        expect_update_stub_panic(&mut cp, KEY_F7);
        // field2 swapped with field1; selection followed up to 1.
        assert_eq!(row_value(&cp, 1), "field2");
        assert_eq!(row_value(&cp, 2), "field1");
        assert_eq!(cp.super_.selected, 1);
    }

    #[test]
    fn f8_moves_selection_down_then_hits_update_stub() {
        let mut cp = panel_with_moving_rows(3, false);
        cp.super_.selected = 0;
        expect_update_stub_panic(&mut cp, KEY_F8);
        assert_eq!(row_value(&cp, 0), "field1");
        assert_eq!(row_value(&cp, 1), "field0");
        assert_eq!(cp.super_.selected, 1);
    }

    #[test]
    fn up_while_moving_falls_through_to_move_up() {
        // moving=true makes KEY_UP fall through to the MoveUp arm (C:81-84).
        let mut cp = panel_with_moving_rows(3, true);
        cp.super_.selected = 2;
        expect_update_stub_panic(&mut cp, KEY_UP);
        assert_eq!(row_value(&cp, 1), "field2");
        assert_eq!(cp.super_.selected, 1);
    }

    #[test]
    fn down_while_moving_falls_through_to_move_down() {
        let mut cp = panel_with_moving_rows(3, true);
        cp.super_.selected = 0;
        expect_update_stub_panic(&mut cp, KEY_DOWN);
        assert_eq!(row_value(&cp, 0), "field1");
        assert_eq!(cp.super_.selected, 1);
    }

    #[test]
    fn f9_removes_row_then_hits_update_stub() {
        let mut cp = panel_with_moving_rows(3, false);
        cp.super_.selected = 1;
        expect_update_stub_panic(&mut cp, KEY_F9);
        // C: size > 1 && selected < size -> Panel_remove(super, 1).
        assert_eq!(Panel_size(&cp.super_), 2);
        assert_eq!(row_value(&cp, 0), "field0");
        assert_eq!(row_value(&cp, 1), "field2");
    }

    #[test]
    fn f9_on_single_row_keeps_it_but_still_hits_update_stub() {
        // size == 1: the `size > 1` guard blocks removal, but result is still
        // HANDLED, so the update stub still fires.
        let mut cp = panel_with_moving_rows(1, false);
        expect_update_stub_panic(&mut cp, KEY_F9);
        assert_eq!(Panel_size(&cp.super_), 1);
    }

    #[test]
    fn lost_focus_while_moving_cancels_then_hits_update_stub() {
        let mut cp = panel_with_moving_rows(3, true);
        Panel_setSelectionColor(&mut cp.super_, ColorElements::PANEL_SELECTION_FOLLOW);
        expect_update_stub_panic(&mut cp, EVENT_PANEL_LOST_FOCUS);
        assert!(!cp.moving);
        assert_eq!(
            cp.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
    }
}
