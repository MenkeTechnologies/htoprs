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
//! [`ColumnsPanel`] struct models all four, as `#[repr(C)]` with `super_`
//! first: `ss`/`changed` are raw `*mut` back-pointers (non-owning, into a
//! caller-owned `ScreenSettings`/`bool`), reached by
//! [`ColumnsPanel_update`]'s container-of downcast of the base `Panel*`. The
//! fixed C layout is what makes that `(ColumnsPanel*) super` cast sound — see
//! the [`ColumnsPanel`] struct docs.
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
//!   faithfully and now reaches the ported [`ColumnsPanel_update`], which
//!   rewrites the screen's `fields`/`flags` from the new list order.
//!
//! - [`ColumnsPanel_add`] (`ColumnsPanel.c:137`) — resolves a field/dynamic
//!   column display name for `key` and pushes a `ListItem_new(name, key)`.
//!   The reserved-field branch indexes the now-ported `Process_fields[key].name`
//!   (bounded by the now-ported `LAST_PROCESSFIELD`); the dynamic branch reads
//!   `Hashtable_get` -> `DynamicColumn.heading`/`.name`. Takes `Panel* super`
//!   (no `ColumnsPanel` field needed), so it is self-contained.
//! - [`ColumnsPanel_fill`] (`ColumnsPanel.c:156`) / [`ColumnsPanel_new`]
//!   (`ColumnsPanel.c:164`) — prune + repopulate from `ss->fields` and the
//!   constructor, now that the `ss`/`changed` raw back-pointers are modeled.
//! - [`ColumnsPanel_update`] (`ColumnsPanel.c:181`) — rewrites
//!   `ss->fields`/`ss->flags` from the list, OR-ing `Process_fields[key].flags`.
//!   The C `ColumnsPanel* this = (ColumnsPanel*) super;` downcast is ported
//!   faithfully with the raw container-of cast on the `#[repr(C)]` layout
//!   (`super as *mut Panel as *mut ColumnsPanel`).
//!
//! # Stubbed (cannot be ported faithfully yet)
//!
//! - [`ColumnsPanel_delete`] (`ColumnsPanel.c:31`) — `Panel_done` +
//!   `free`. [`ColumnsPanel`] owns its fields, so `Drop` releases them;
//!   there is no algorithm to port (same precedent as every sibling
//!   `_delete`, e.g. `Panel_delete`/`ListItem_delete`).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::crt::{
    ColorElements, KEY_DC, KEY_DEL_MAC, KEY_DOWN, KEY_ENTER, KEY_F, KEY_MOUSE, KEY_RECLICK, KEY_UP,
};
use crate::ported::functionbar::FunctionBar_new;
use crate::ported::hashtable::{Hashtable, Hashtable_get};
use crate::ported::linux::linuxprocess::{Process_fields, LAST_PROCESSFIELD};
use crate::ported::listitem::{ListItem, ListItem_new};
use crate::ported::object::Object;
use crate::ported::panel::{
    HandlerResult, Panel, PanelClass, Panel_add, Panel_done, Panel_get, Panel_getSelectedIndex,
    Panel_moveSelectedDown, Panel_moveSelectedUp, Panel_new, Panel_prune, Panel_remove,
    Panel_selectByTyping, Panel_setHeader, Panel_setSelectionColor, Panel_size,
    EVENT_PANEL_LOST_FOCUS,
};
use crate::ported::settings::{RowField, ScreenSettings};

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

/// Port of the file-scope `static const char* const ColumnsFunctions[]`
/// from `ColumnsPanel.c:29`: `F7=MoveUp`, `F8=MoveDn`, `F9=Remove`,
/// `F10=Done`, the rest blank. The C trailing `NULL` sentinel is dropped
/// (the ported `FunctionBar_new` is length-bounded, not NUL-terminated).
static ColumnsFunctions: [&str; 10] = [
    "      ", "      ", "      ", "      ", "      ", "      ", "MoveUp", "MoveDn", "Remove",
    "Done  ",
];

/// Port of the C `ColumnsPanel` struct (`ColumnsPanel.h:17`):
/// `{ Panel super; ScreenSettings* ss; bool* changed; bool moving; }`.
///
/// `#[repr(C)]` with `super_` as the first field is load-bearing: the C
/// vtable functions take `Panel* super` and recover the enclosing panel with
/// `ColumnsPanel* this = (ColumnsPanel*) super;` (valid because `super` is the
/// embedded first member). [`ColumnsPanel_update`] reproduces that downcast
/// with the raw container-of cast `super as *mut Panel as *mut ColumnsPanel`,
/// which is only sound when `super_` sits at offset 0 — hence the fixed C
/// layout. `ss`/`changed` are the C `ScreenSettings*`/`bool*` back-pointers
/// (raw `*mut`, non-owning), reachable through that downcast. `super_` avoids
/// the Rust `super` keyword, matching the `process.rs` `super_: Row` convention.
#[repr(C)]
pub struct ColumnsPanel {
    /// C `Panel super` — the embedded panel base. MUST stay first (offset 0)
    /// for the `(ColumnsPanel*) super` downcast in [`ColumnsPanel_update`].
    pub super_: Panel,
    /// C `ScreenSettings* ss` — non-owning back-pointer to the screen whose
    /// `fields`/`flags` [`ColumnsPanel_update`] rewrites from the list.
    pub ss: *mut ScreenSettings,
    /// C `bool* changed` — non-owning pointer to a caller-owned "settings
    /// changed" flag, set by [`ColumnsPanel_update`].
    pub changed: *mut bool,
    /// C `bool moving` — whether the panel is in row-reorder mode.
    pub moving: bool,
}

/// Port of `ColumnsPanel.c`'s `const PanelClass ColumnsPanel_class` vtable
/// (`ColumnsPanel.c:129`). C sets only `.eventHandler = ColumnsPanel_eventHandler`;
/// `.drawFunctionBar` / `.printHeader` are NULL, so those slots inherit the
/// trait defaults. Wires `event_handler` to [`ColumnsPanel_eventHandler`].
impl PanelClass for ColumnsPanel {
    fn as_panel(&self) -> &Panel {
        &self.super_
    }
    fn as_panel_mut(&mut self) -> &mut Panel {
        &mut self.super_
    }
    fn event_handler(&mut self, ev: i32) -> HandlerResult {
        ColumnsPanel_eventHandler(self, ev)
    }
}

/// Port of `static void ColumnsPanel_delete(Object* object)` from
/// `ColumnsPanel.c:31`: `Panel_done(&this->super); free(this);`. Taking
/// `this` by value consumes the panel; the embedded `super_` [`Panel`] is
/// handed to [`Panel_done`] (mirroring the C call graph), and the `moving`
/// flag plus the non-owning `ss`/`changed` back-pointers drop with the struct
/// free (C frees neither — they alias caller-owned state).
pub fn ColumnsPanel_delete(this: ColumnsPanel) {
    let ColumnsPanel {
        super_,
        moving,
        ss,
        changed,
    } = this;
    Panel_done(super_);
    let _ = moving;
    let _ = ss;
    let _ = changed;
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
        let obj: &mut dyn Object = super_.items[i as usize].object_mut();
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
/// The C tail `if (result == HANDLED) ColumnsPanel_update(super);` is emitted
/// faithfully and now reaches the ported [`ColumnsPanel_update`]: every
/// `HANDLED`-returning path performs its panel-level mutation and then
/// rewrites the screen's `fields`/`flags` from the new list order (requiring
/// the panel's `ss`/`changed` back-pointers to be live). The
/// `IGNORED`/`BREAK_LOOP`-to-`IGNORED` paths skip the update.
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
                    let any: &mut dyn core::any::Any = this.super_.items[sel].object_mut();
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

/// Port of `static void ColumnsPanel_add(Panel* super, unsigned int key,
/// Hashtable* columns)` from `ColumnsPanel.c:137`. Resolves the display
/// `name` for `key` and pushes a `ListItem_new(name, key)` onto the panel.
///
/// For a reserved process field (`key < LAST_PROCESSFIELD`) the name is
/// `Process_fields[key].name`; otherwise it is a dynamic column, looked up
/// with [`Hashtable_get`] and named by its `heading` (preferred) or `name`.
/// The C `assert(column)` + `if (!column) name = NULL` graceful path is a
/// `None` arm here (mapped to the `"- "` fallback below). The C
/// `Process_fields[key].name` is a `const char*` that is `NULL` for the
/// unused table gaps; this repo models that `NULL` as the empty string
/// (`ProcessFieldData.name` is `&'static str`, `""` for a gap), so the C
/// `if (name == NULL) name = "- "` becomes `if name.is_empty()`.
pub fn ColumnsPanel_add(super_: &mut Panel, key: u32, columns: &Hashtable) {
    // C: const char* name;
    let name: &str = if (key as usize) < LAST_PROCESSFIELD {
        // C: name = Process_fields[key].name;
        Process_fields[key as usize].name
    } else {
        // C: const DynamicColumn* column = Hashtable_get(columns, key);
        //    assert(column);
        //    if (!column) { name = NULL; }
        //    else { name = column->heading ? column->heading : column->name; }
        match Hashtable_get(columns, key) {
            Some(obj) => {
                let column = obj
                    .as_dynamic_column()
                    .expect("ColumnsPanel_add: dynamic column entry is not a DynamicColumn");
                column.heading.as_deref().unwrap_or(column.name.as_str())
            }
            None => "", // C NULL -> "- " fallback below
        }
    };
    // C: if (name == NULL) name = "- ";  (C NULL modeled as "" here)
    let name = if name.is_empty() { "- " } else { name };
    // C: Panel_add(super, (Object*) ListItem_new(name, key));
    Panel_add(super_, Box::new(ListItem_new(name, key as i32)));
}

/// Port of `void ColumnsPanel_fill(ColumnsPanel* this, ScreenSettings* ss,
/// Hashtable* columns)` from `ColumnsPanel.c:156`. [`Panel_prune`]s the panel,
/// walks `ss->fields` (the NUL-terminated `RowField` list) calling
/// [`ColumnsPanel_add`] for each, then stores `this->ss = ss`.
///
/// `ss` is the raw `*mut ScreenSettings` back-pointer (the C `ScreenSettings*`,
/// non-owning). The C loop `for (const RowField* fields = ss->fields; *fields;
/// fields++)` stops at the `0` terminator; the `fields` `Vec` is read by index
/// through the raw pointer each step (a fresh unsafe deref, so it never holds a
/// borrow across the `&mut this.super_` used by [`ColumnsPanel_add`]), and the
/// same `f == 0` break reproduces the sentinel stop for a `Vec` that carries
/// its terminator.
///
/// # Safety
///
/// `ss` must be a valid, non-aliased `ScreenSettings` pointer for the duration
/// of the call (it is not the same object as any field of `this`).
pub fn ColumnsPanel_fill(this: &mut ColumnsPanel, ss: *mut ScreenSettings, columns: &Hashtable) {
    // C: Panel* super = &this->super; Panel_prune(super);
    Panel_prune(&mut this.super_);
    // C: for (const RowField* fields = ss->fields; *fields; fields++)
    //       ColumnsPanel_add(super, *fields, columns);
    // SAFETY: `ss` is a valid back-pointer distinct from `this`'s storage;
    // borrowing it shared is sound (ColumnsPanel_add touches only `this`).
    let ss_ref = unsafe { &*ss };
    for i in 0..ss_ref.fields.len() {
        let f = ss_ref.fields[i];
        if f == 0 {
            break; // C: *fields == 0 terminates the loop
        }
        ColumnsPanel_add(&mut this.super_, f as u32, columns);
    }
    // C: this->ss = ss;
    this.ss = ss;
}

/// Port of `ColumnsPanel* ColumnsPanel_new(ScreenSettings* ss,
/// Hashtable* columns, bool* changed)` from `ColumnsPanel.c:164`. Builds the
/// `1x1` panel with the `ColumnsFunctions` bar, stores the `ss`/`changed`
/// back-pointers, sets the "Active Columns" header, and populates the list
/// via [`ColumnsPanel_fill`].
///
/// `ss`/`changed` are the raw `*mut ScreenSettings`/`*mut bool` back-pointers
/// (the C `ScreenSettings*`/`bool*`, non-owning). The C `Class(ListItem)`/
/// `owner` args to `Panel_init` only type the underlying `Vector`; the ported
/// [`Panel_new`] drops them, matching every sibling panel port. Returned by
/// value like the other `_new` ports (the C `AllocThis` heap object).
pub fn ColumnsPanel_new(
    ss: *mut ScreenSettings,
    columns: &Hashtable,
    changed: *mut bool,
) -> ColumnsPanel {
    // C: FunctionBar* fuBar = FunctionBar_new(ColumnsFunctions, NULL, NULL);
    let fuBar = FunctionBar_new(Some(&ColumnsFunctions[..]), None, None);
    // C: Panel_init(super, 1, 1, 1, 1, Class(ListItem), true, fuBar);
    let super_ = Panel_new(1, 1, 1, 1, Some(fuBar));

    // C: this->ss = ss; this->changed = changed; this->moving = false;
    let mut this = ColumnsPanel {
        super_,
        ss,
        changed,
        moving: false,
    };

    // C: Panel_setHeader(super, "Active Columns");
    Panel_setHeader(&mut this.super_, "Active Columns");

    // C: ColumnsPanel_fill(this, ss, columns);
    ColumnsPanel_fill(&mut this, ss, columns);

    this
}

/// Port of `void ColumnsPanel_update(Panel* super)` from `ColumnsPanel.c:181`.
/// Rewrites `ss->fields`/`ss->flags` from the current list order, OR-ing
/// `Process_fields[key].flags` for every reserved field, and sets `*changed`.
///
/// The C body opens with `ColumnsPanel* this = (ColumnsPanel*) super;` — the
/// classic C OOP downcast of a base pointer to its enclosing subclass. It is
/// reproduced faithfully with the raw container-of cast
/// `super as *mut Panel as *mut ColumnsPanel` (sound because `#[repr(C)]` puts
/// `super_` at offset 0; see [`ColumnsPanel`]). After the cast the original
/// `super_` borrow is dropped and everything goes through the recovered
/// `this`. The C NUL-terminated `xRealloc`'d `fields` array becomes a `Vec`
/// of length `size + 1` with a trailing `0` terminator (`ProcessField`/
/// `RowField` `0` == `NULL_PROCESSFIELD`). `changed`/`ss` are dereferenced
/// through their raw back-pointers, as the C dereferences `this->changed`/
/// `this->ss`.
///
/// The signature keeps the C `Panel* super` so both [`ColumnsPanel_eventHandler`]'s
/// `HANDLED` tail (`&mut this.super_`) and `AvailableColumnsPanel_eventHandler`'s
/// `*mut Panel` back-pointer can call it with a base-panel pointer.
///
/// # Safety
///
/// `super_` must be the embedded `Panel` of a live `ColumnsPanel`, and that
/// panel's `ss`/`changed` valid non-null back-pointers.
pub fn ColumnsPanel_update(super_: &mut Panel) {
    // C: ColumnsPanel* this = (ColumnsPanel*) super;
    // SAFETY: `super_` is the offset-0 `Panel` of a real `#[repr(C)]`
    // `ColumnsPanel`, so the container-of cast recovers the enclosing struct.
    let this = unsafe { &mut *(super_ as *mut Panel as *mut ColumnsPanel) };

    // C: int size = Panel_size(super);
    let size = Panel_size(&this.super_);
    // C: *(this->changed) = true;
    // SAFETY: `changed` is the caller-owned flag pointer wired at construction.
    unsafe {
        *this.changed = true;
    }
    // C: this->ss->fields = xRealloc(..., size + 1); this->ss->flags = 0;
    // SAFETY: `ss` is the non-null back-pointer wired at construction.
    let ss = unsafe { &mut *this.ss };
    ss.fields = vec![0; (size + 1) as usize];
    ss.flags = 0;
    for i in 0..size {
        // C: int key = ((ListItem*) Panel_get(super, i))->key;
        let key = {
            let obj = Panel_get(&this.super_, i);
            let any: &dyn core::any::Any = obj;
            any.downcast_ref::<ListItem>()
                .expect("ColumnsPanel_update: panel item is not a ListItem")
                .key
        };
        // C: this->ss->fields[i] = key;
        ss.fields[i as usize] = key as RowField;
        // C: if (key < LAST_PROCESSFIELD) this->ss->flags |= Process_fields[key].flags;
        if (key as usize) < LAST_PROCESSFIELD {
            ss.flags |= Process_fields[key as usize].flags;
        }
    }
    // C: this->ss->fields[size] = 0;  (already 0 from the vec![0; ...] fill)
    ss.fields[size as usize] = 0;
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
        // ss/changed are null here; only the HANDLED-path tests (which reach
        // ColumnsPanel_update) wire live pointers, via `drive` below.
        ColumnsPanel {
            super_,
            ss: core::ptr::null_mut(),
            changed: core::ptr::null_mut(),
            moving,
        }
    }

    /// Read back the `moving` flag of row `i` via the same `Any` downcast
    /// the ported function uses.
    fn row_moving(cp: &ColumnsPanel, i: usize) -> bool {
        let obj: &dyn Object = cp.super_.items[i].object();
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
            let obj: &mut dyn Object = cp.super_.items[i].object_mut();
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
    // The C tail `if (result == HANDLED) ColumnsPanel_update(super)` now
    // reaches the ported `ColumnsPanel_update`, which rewrites the screen's
    // `fields`/`flags` from the new list order and sets `*changed`. The
    // `HANDLED`-path tests wire a live `ScreenSettings`/`changed` pair into the
    // panel via `drive` and assert both the panel-level mutation and the
    // resulting `fields` list; the `IGNORED`/`BREAK_LOOP`-to-`IGNORED` paths
    // never call update, so they run against the null back-pointers unchanged.

    /// The `ListItem.value` of row `i`, via the same `Any` downcast the port
    /// uses.
    fn row_value(cp: &ColumnsPanel, i: usize) -> String {
        let obj: &dyn Object = cp.super_.items[i].object();
        let any: &dyn core::any::Any = obj;
        any.downcast_ref::<ListItem>().unwrap().value.clone()
    }

    /// Wire a fresh `ScreenSettings` + `changed` flag into `cp`, drive `ch`
    /// through the event handler (whose `HANDLED` tail runs the ported
    /// [`ColumnsPanel_update`]), then null the back-pointers so they cannot
    /// dangle past the borrowed locals. Returns `(result, changed, ss.fields)`.
    fn drive(cp: &mut ColumnsPanel, ch: i32) -> (HandlerResult, bool, Vec<RowField>) {
        let mut ss = ScreenSettings::default();
        let mut changed = false;
        cp.ss = &mut ss;
        cp.changed = &mut changed;
        let result = ColumnsPanel_eventHandler(cp, ch);
        cp.ss = core::ptr::null_mut();
        cp.changed = core::ptr::null_mut();
        (result, changed, ss.fields)
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
            let obj: &mut dyn Object = cp.super_.items[i].object_mut();
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
            let obj: &mut dyn Object = cp.super_.items[i].object_mut();
            (obj as &mut dyn core::any::Any)
                .downcast_mut::<ListItem>()
                .unwrap()
                .moving = false;
        }
        cp.super_.selected = 1;
        let (r, changed, fields) = drive(&mut cp, KEY_ENTER);
        assert_eq!(r, HandlerResult::HANDLED);
        assert!(cp.moving);
        assert_eq!(
            cp.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOLLOW
        );
        assert!(row_moving(&cp, 1), "selected row should be marked moving");
        assert!(!row_moving(&cp, 0));
        // Update rewrote fields from the (unchanged) list order + terminator.
        assert!(changed);
        assert_eq!(fields, vec![0, 1, 2, 0]);
    }

    #[test]
    fn enter_while_moving_cancels_move_then_runs_update() {
        let mut cp = panel_with_moving_rows(3, true);
        Panel_setSelectionColor(&mut cp.super_, ColorElements::PANEL_SELECTION_FOLLOW);
        let (r, changed, fields) = drive(&mut cp, KEY_ENTER);
        assert_eq!(r, HandlerResult::HANDLED);
        assert!(!cp.moving);
        assert_eq!(
            cp.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
        assert!(changed);
        assert_eq!(fields, vec![0, 1, 2, 0]);
    }

    #[test]
    fn f7_moves_selection_up_then_runs_update() {
        let mut cp = panel_with_moving_rows(3, false); // field0,field1,field2
        cp.super_.selected = 2;
        let (r, changed, fields) = drive(&mut cp, KEY_F7);
        assert_eq!(r, HandlerResult::HANDLED);
        // field2 swapped with field1; selection followed up to 1.
        assert_eq!(row_value(&cp, 1), "field2");
        assert_eq!(row_value(&cp, 2), "field1");
        assert_eq!(cp.super_.selected, 1);
        // fields track the new key order [0, 2, 1] + terminator.
        assert!(changed);
        assert_eq!(fields, vec![0, 2, 1, 0]);
    }

    #[test]
    fn f8_moves_selection_down_then_runs_update() {
        let mut cp = panel_with_moving_rows(3, false);
        cp.super_.selected = 0;
        let (r, changed, fields) = drive(&mut cp, KEY_F8);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(row_value(&cp, 0), "field1");
        assert_eq!(row_value(&cp, 1), "field0");
        assert_eq!(cp.super_.selected, 1);
        assert!(changed);
        assert_eq!(fields, vec![1, 0, 2, 0]);
    }

    #[test]
    fn up_while_moving_falls_through_to_move_up() {
        // moving=true makes KEY_UP fall through to the MoveUp arm (C:81-84).
        let mut cp = panel_with_moving_rows(3, true);
        cp.super_.selected = 2;
        let (r, _changed, fields) = drive(&mut cp, KEY_UP);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(row_value(&cp, 1), "field2");
        assert_eq!(cp.super_.selected, 1);
        assert_eq!(fields, vec![0, 2, 1, 0]);
    }

    #[test]
    fn down_while_moving_falls_through_to_move_down() {
        let mut cp = panel_with_moving_rows(3, true);
        cp.super_.selected = 0;
        let (r, _changed, fields) = drive(&mut cp, KEY_DOWN);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(row_value(&cp, 0), "field1");
        assert_eq!(cp.super_.selected, 1);
        assert_eq!(fields, vec![1, 0, 2, 0]);
    }

    #[test]
    fn f9_removes_row_then_runs_update() {
        let mut cp = panel_with_moving_rows(3, false);
        cp.super_.selected = 1;
        let (r, changed, fields) = drive(&mut cp, KEY_F9);
        assert_eq!(r, HandlerResult::HANDLED);
        // C: size > 1 && selected < size -> Panel_remove(super, 1).
        assert_eq!(Panel_size(&cp.super_), 2);
        assert_eq!(row_value(&cp, 0), "field0");
        assert_eq!(row_value(&cp, 1), "field2");
        // fields track the surviving key order [0, 2] + terminator.
        assert!(changed);
        assert_eq!(fields, vec![0, 2, 0]);
    }

    #[test]
    fn f9_on_single_row_keeps_it_but_still_runs_update() {
        // size == 1: the `size > 1` guard blocks removal, but result is still
        // HANDLED, so the update still fires.
        let mut cp = panel_with_moving_rows(1, false);
        let (r, changed, fields) = drive(&mut cp, KEY_F9);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_size(&cp.super_), 1);
        assert!(changed);
        assert_eq!(fields, vec![0, 0]);
    }

    #[test]
    fn lost_focus_while_moving_cancels_then_runs_update() {
        let mut cp = panel_with_moving_rows(3, true);
        Panel_setSelectionColor(&mut cp.super_, ColorElements::PANEL_SELECTION_FOLLOW);
        let (r, changed, fields) = drive(&mut cp, EVENT_PANEL_LOST_FOCUS);
        assert_eq!(r, HandlerResult::HANDLED);
        assert!(!cp.moving);
        assert_eq!(
            cp.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
        assert!(changed);
        assert_eq!(fields, vec![0, 1, 2, 0]);
    }

    // ── ColumnsPanel_add ──────────────────────────────────────────────

    use crate::ported::hashtable::{Hashtable_new, Hashtable_put};
    use crate::ported::panel::Panel_get;
    use crate::ported::process::ProcessField;

    /// The added row's `ListItem`, via the same `Any` downcast the port uses.
    fn added_row(panel: &Panel, i: i32) -> &ListItem {
        let obj: &dyn Object = Panel_get(panel, i);
        (obj as &dyn core::any::Any)
            .downcast_ref::<ListItem>()
            .expect("ColumnsPanel_add row is not a ListItem")
    }

    #[test]
    fn add_reserved_field_uses_process_fields_name() {
        let mut panel = Panel_new(0, 0, 10, 10, None);
        let columns = Hashtable_new(10, false);
        // C: key < LAST_PROCESSFIELD -> name = Process_fields[key].name.
        let key = ProcessField::PID as u32;
        assert!((key as usize) < LAST_PROCESSFIELD);
        ColumnsPanel_add(&mut panel, key, &columns);
        assert_eq!(Panel_size(&panel), 1);
        let item = added_row(&panel, 0);
        assert_eq!(item.value, Process_fields[key as usize].name);
        assert!(!item.value.is_empty());
        assert_eq!(item.key, key as i32);
    }

    #[test]
    fn add_empty_field_gap_falls_back_to_dash() {
        // Process_fields[0] is the unused table gap (name "" == C NULL),
        // so C's `if (name == NULL) name = "- "` fires.
        let mut panel = Panel_new(0, 0, 10, 10, None);
        let columns = Hashtable_new(10, false);
        assert!(Process_fields[0].name.is_empty());
        ColumnsPanel_add(&mut panel, 0, &columns);
        let item = added_row(&panel, 0);
        assert_eq!(item.value, "- ");
        assert_eq!(item.key, 0);
    }

    #[test]
    fn add_dynamic_column_prefers_heading_then_name() {
        let mut panel = Panel_new(0, 0, 10, 10, None);
        let mut columns = Hashtable_new(10, true);
        // key >= LAST_PROCESSFIELD -> dynamic-column branch.
        let k_head = (LAST_PROCESSFIELD as u32) + 5;
        let k_name = (LAST_PROCESSFIELD as u32) + 6;
        columns_put(&mut columns, k_head, "internal_a", Some("Heading A"));
        columns_put(&mut columns, k_name, "internal_b", None);

        ColumnsPanel_add(&mut panel, k_head, &columns);
        ColumnsPanel_add(&mut panel, k_name, &columns);
        // Missing key: C assert(column) + graceful NULL -> "- ".
        ColumnsPanel_add(&mut panel, (LAST_PROCESSFIELD as u32) + 99, &columns);

        assert_eq!(added_row(&panel, 0).value, "Heading A"); // heading preferred
        assert_eq!(added_row(&panel, 0).key, k_head as i32);
        assert_eq!(added_row(&panel, 1).value, "internal_b"); // heading None -> name
        assert_eq!(added_row(&panel, 2).value, "- "); // absent -> fallback
    }

    fn columns_put(
        columns: &mut crate::ported::hashtable::Hashtable,
        key: u32,
        name: &str,
        heading: Option<&str>,
    ) {
        Hashtable_put(
            columns,
            key,
            Box::new(crate::ported::dynamiccolumn::DynamicColumn {
                name: name.to_string(),
                heading: heading.map(|s| s.to_string()),
                caption: None,
                description: None,
                width: 0,
                enabled: true,
                table: core::ptr::null(),
            }),
        );
    }

    // ── ColumnsPanel_new / _fill ──────────────────────────────────────

    #[test]
    fn new_fills_list_from_ss_fields_and_stores_backpointers() {
        // ss->fields is the NUL-terminated reserved-field list; _new -> _fill
        // adds one ListItem per non-zero field, in order, and stores the
        // ss/changed back-pointers.
        let mut ss = ScreenSettings::default();
        ss.fields = vec![1, 2, 0]; // two reserved fields + terminator
        let columns = Hashtable_new(10, false);
        let mut changed = false;

        let cp = ColumnsPanel_new(&mut ss, &columns, &mut changed);

        assert_eq!(Panel_size(&cp.super_), 2);
        assert_eq!(added_row(&cp.super_, 0).key, 1);
        assert_eq!(added_row(&cp.super_, 1).key, 2);
        // Header rendered and back-pointers wired.
        assert!(!changed); // _new/_fill never touch `changed` (only _update)
        assert_eq!(cp.ss, &mut ss as *mut ScreenSettings);
        assert_eq!(cp.changed, &mut changed as *mut bool);
    }
}
