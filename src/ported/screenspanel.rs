//! Partial port of `ScreensPanel.c` — htop's "Screens" editor panel (the
//! list of named process screens the user can rename / reorder / add /
//! remove).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. A C fn `Foo_bar(Panel* super)`
//! (where `super` is really a `ScreensPanel*`) ports to a free fn
//! `Foo_bar(this: &mut ScreensPanel)` — the same reduced-struct + free-fn
//! shape the sibling `columnspanel.rs` port uses.
//!
//! # Data model
//!
//! htop's `ScreensPanel` (`ScreensPanel.h:27`) embeds a `Panel super`, a
//! `Settings*`, a `ColumnsPanel*`, an `AvailableColumnsPanel*`, the inline
//! [`LineEditor`] used while renaming, plus the `moving` / `saved` /
//! `renamingItem` / `renamingNewItem` rename-state scalars. The
//! [`ScreensPanel`] struct models the fields the ported functions touch —
//! the embedded `super_` [`Panel`], the `editor`, the four rename/move
//! scalars, and the `settings` back-pointer.
//!
//! The `item->ss` alias (C `ScreenSettings*` pointing into
//! `settings->screens[i]`, shared ownership) is modeled as the screen's
//! **index** into the single settings-owned `Vec<ScreenSettings>`
//! ([`ScreenListItem::ssIndex`]), reached through a raw
//! [`ScreensPanel::settings`] (`*mut Settings`) back-pointer — the
//! `MainPanel.state` precedent for an owned-elsewhere pointer, and the same
//! alias-pointer-as-index technique as `renamingItem`. That lets the two
//! settings-array functions ([`rebuildSettingsArray`] /
//! [`ScreensPanel_update`]) reorder the one owned `Vec` in place (identity
//! preserved) rather than juggle two divergent copies.
//!
//! The `columns` / `availableColumns` / `scr` back-pointers and the C
//! `char buffer[]` scratch field are still omitted (the same reduced-struct
//! precedent as `columnspanel.rs`), so the functions that dereference them
//! (`ScreensPanel_new`, `addNewScreen`, and the focus-change /
//! `F5`-new-screen paths of `ScreensPanel_eventHandlerNormal`) stay stubs —
//! the specific blocker is named on each below. `renamingItem` is a C
//! `ListItem*`; the faithful safe-Rust analog is the item's **index**
//! (`Option<usize>`, `None` == C `NULL`), since renaming never reorders the
//! list, so the index of the item under edit is stable.
//!
//! # Ported (self-contained, or transitively-blocked exactly like the
//! ported `ColumnsPanel_eventHandler`)
//!
//! - [`ScreenListItem_new`] (`ScreensPanel.c:43`) — the `AllocThis` +
//!   `ListItem_init` constructor, carrying the owned [`ScreenSettings`]
//!   (`this->ss`). Returns an owned value, mirroring the `ListItem_new`
//!   owned-return idiom.
//! - [`ScreensPanel_cleanup`] (`ScreensPanel.c:57`) — tears down the
//!   process-wide renaming `FunctionBar` ([`Screens_renamingBar`], a
//!   `Mutex<Option<FunctionBar>>` file-static), the same body the
//!   `MetersPanel`/`ScreenTabsPanel` cleanups use.
//! - [`ScreensPanel_cancelMoving`] (`ScreensPanel.c:64`) — clears every
//!   row's `moving` flag and the panel's own `moving`, restores
//!   `PANEL_SELECTION_FOCUS`. Same mutating downcast analog as
//!   `ColumnsPanel_cancelMoving`.
//! - [`startRenaming`] (`ScreensPanel.c:179`) — enters rename mode: seeds
//!   the [`LineEditor`] with the current name (capped to
//!   `SCREEN_NAME_LEN - 1`), points the row's display value at the editor
//!   text, switches to the `PANEL_EDIT` color and the shared renaming
//!   [`FunctionBar`], and places the cursor.
//! - [`ScreensPanel_eventHandlerRenaming`] (`ScreensPanel.c:102`) — the
//!   full rename-mode key `switch` over [`LineEditor`] + the row value,
//!   returning [`HandlerResult::HANDLED`]. Its finish paths call the
//!   now-ported [`ScreensPanel_update`], and the cancel-of-a-new-item path
//!   calls the now-ported [`rebuildSettingsArray`]; both need
//!   [`ScreensPanel::settings`] to be a live pointer.
//! - [`ScreensPanel_eventHandler`] (`ScreensPanel.c:363`) — the trivial
//!   dispatcher choosing the renaming vs. normal handler by whether a
//!   rename is in progress.
//! - [`rebuildSettingsArray`] (`ScreensPanel.c:202`) — reorders the
//!   settings-owned `Vec<ScreenSettings>` to panel-row order via each row's
//!   [`ScreenListItem::ssIndex`], clamps the selection, sets `ssIndex`.
//! - [`ScreensPanel_update`] (`ScreensPanel.c:415`) — marks the settings
//!   dirty, then writes each row's value into its screen's `heading` and
//!   reorders `screens[]` to panel order.
//! - [`ScreensPanel_eventHandlerNormal`] (`ScreensPanel.c:234`) — the
//!   non-rename key `switch` plus its rebuild/update tail. The `F5`/`^N`
//!   new-screen arm and the focus-change tail block are honest inline
//!   stubs (blockers below); the move/toggle/rename/event arms run to
//!   completion and drive the ported `rebuildSettingsArray` /
//!   `ScreensPanel_update` (the same transitive-block pattern as
//!   `ScreensPanel_eventHandlerRenaming`).
//!
//! # Stubbed (cannot be ported faithfully yet — blocker named on each)
//!
//! - [`ScreenListItem_delete`] (`ScreensPanel.c:28`) — frees `ss` then
//!   `ListItem_delete`; the owned model releases via `Drop`, no algorithm.
//! - [`ScreensPanel_delete`] (`ScreensPanel.c:75`) — the destructor
//!   (cancel any pending edit, null every `item->ss` so the settings array
//!   keeps them, then `Panel_delete`); the bookkeeping only matters for the
//!   C manual-free protocol, and destruction is `Drop`.
//! - [`addNewScreen`] (`ScreensPanel.c:223`) — calls the stubbed
//!   `Settings_newScreen` (blocked on the platform `Process_fields[]`
//!   table) and inserts the new row.
//! - [`ScreensPanel_new`] (`ScreensPanel.c:381`) — builds
//!   `ColumnsPanel_new` / `AvailableColumnsPanel_new` (both stubbed) and
//!   seeds the rows from `settings->screens[]`.
//!
//! Two arms *inside* the ported [`ScreensPanel_eventHandlerNormal`] remain
//! honest inline stubs: the `F5`/`^N` new-screen arm (needs
//! `settings->dynamicScreens` — an unmodeled `Settings` `Hashtable` field
//! that cannot be added — plus the stubbed `addNewScreen`), and the
//! focus-change tail block (needs the omitted `columns` /
//! `availableColumns` sub-panels, their stubbed `ColumnsPanel_fill` /
//! `AvailableColumnsPanel_fill`, and `settings->dynamicColumns`, another
//! unmodeled `Settings` `Hashtable` field).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::sync::Mutex;

use crate::ported::crt::{
    ColorElements, KEY_DC, KEY_DEL_MAC, KEY_DOWN, KEY_END, KEY_ENTER, KEY_HOME, KEY_MOUSE,
    KEY_NPAGE, KEY_PPAGE, KEY_RECLICK, KEY_UP,
};
use crate::ported::functionbar::{FunctionBar, FunctionBar_new};
use crate::ported::lineeditor::{
    LineEditor, LineEditor_getCursor, LineEditor_getText, LineEditor_handleKey,
    LineEditor_initWithMax, LineEditor_setText,
};
use crate::ported::listitem::{ListItem, ListItem_compare, ListItem_display, ListItem_new};
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::{
    HandlerResult, Panel, Panel_get, Panel_getSelectedIndex, Panel_moveSelectedDown,
    Panel_moveSelectedUp, Panel_onKey, Panel_remove, Panel_selectByTyping,
    Panel_setCursorToSelection, Panel_setDefaultBar, Panel_setSelectionColor, Panel_size,
    EVENT_PANEL_LOST_FOCUS, EVENT_SET_SELECTED,
};
use crate::ported::richstring::RichString;
use crate::ported::settings::{ScreenSettings, Settings};

/// Port of `#define SCREEN_NAME_LEN 20` from `ScreensPanel.h:24`.
pub const SCREEN_NAME_LEN: usize = 20;

/// Char/`KEY_F(n)` case labels from the rename `switch` cannot appear as
/// Rust match patterns directly (`'\n' as i32`, a `KEY_F(n)` `const fn`
/// call), so bind them as module `const`s — the same idiom `panel.rs` /
/// `columnspanel.rs` use. `const`, not `pub fn`, so the port-purity gate
/// (which only rejects unknown `pub fn` names) is unaffected.
const NEWLINE: i32 = '\n' as i32;
const CARRIAGE_RETURN: i32 = '\r' as i32;
const ESC: i32 = 27;
const EQUALS: i32 = b'=' as i32;
const KEY_F2: i32 = crate::ported::crt::KEY_F(2);
const KEY_F5: i32 = crate::ported::crt::KEY_F(5);
const KEY_F7: i32 = crate::ported::crt::KEY_F(7);
const KEY_F8: i32 = crate::ported::crt::KEY_F(8);
const KEY_F9: i32 = crate::ported::crt::KEY_F(9);
const KEY_F10: i32 = crate::ported::crt::KEY_F(10);
const KEY_CTRL_R: i32 = crate::ported::crt::KEY_CTRL('R' as i32);
const KEY_CTRL_N: i32 = crate::ported::crt::KEY_CTRL('N' as i32);
const LBRACKET: i32 = b'[' as i32;
const RBRACKET: i32 = b']' as i32;
const MINUS: i32 = b'-' as i32;
const PLUS: i32 = b'+' as i32;

/// Port of `static const char* const ScreensRenamingFunctions[]`
/// (`ScreensPanel.c:52`), minus the trailing `NULL` (Rust length is the
/// terminator). The bar shown while a screen is being renamed.
const ScreensRenamingFunctions: [&str; 10] = [
    "      ", "Cancel", "      ", "      ", "      ", "      ", "      ", "      ", "      ",
    "Done  ",
];

/// Port of the file-static `static FunctionBar* Screens_renamingBar = NULL;`
/// (`ScreensPanel.c:53`) — the process-wide renaming-mode bar, lazily built by
/// [`ScreensPanel::screens_renamingBar`] and torn down by [`ScreensPanel_cleanup`].
/// The C raw `FunctionBar*` (with `NULL` meaning "not yet built") is a
/// `Mutex<Option<FunctionBar>>`: `None` is the `NULL` sentinel and a `Some`
/// payload owns the bar (whose `Drop` is the analog of `FunctionBar_delete`) —
/// the same idiom `MetersPanel`/`ScreenTabsPanel` use for their shared bars.
static Screens_renamingBar: Mutex<Option<FunctionBar>> = Mutex::new(None);

/// Port of the C `ScreenListItem` struct (`ScreensPanel.h:42`): a
/// `ListItem` row (`super`) carrying a back-reference to its
/// [`ScreenSettings`] (`ss`). The C `DynamicScreen* ds` field is omitted —
/// `ScreensPanel.c` never reads or writes it, and `DynamicScreen` is
/// unported. The C `ss` is a `ScreenSettings*` that *aliases*
/// `settings->screens[i]`; that shared-ownership alias is modeled here as
/// the screen's **index** into the single owned `Settings.screens`
/// `Vec` (`ssIndex`), reached through the panel's `settings` back-pointer.
/// This is the same alias-pointer-as-index technique used for
/// [`ScreensPanel::renamingItem`], and it lets [`rebuildSettingsArray`] /
/// [`ScreensPanel_update`] keep the array and the rows in sync by
/// reordering the one owned `Vec` (identity preserved, no divergent copy).
pub struct ScreenListItem {
    /// C `ListItem super` — the embedded list-row base.
    pub super_: ListItem,
    /// C `ScreenSettings* ss` — the screen this row edits, modeled as its
    /// index into [`Settings::screens`] (reached via
    /// [`ScreensPanel::settings`]).
    pub ssIndex: usize,
}

/// Port of `const ObjectClass ScreenListItem_class` (`ScreensPanel.c:36`):
/// `{ .extends = Class(ListItem), .display = ListItem_display, .delete =
/// ScreenListItem_delete, .compare = ListItem_compare }`. The C `.extends`
/// targets `ListItem_class`, a private `static` in `listitem.rs` (not
/// exported), so the nearest exported ancestor `Object_class` is used;
/// the class chain is unused by the ported surface (the panel downcasts
/// rows via `Any`, never `Object_isA`). `.display` / `.compare` are wired
/// through the [`Object`] impl below; `.delete` maps to `Drop`.
static ScreenListItem_class: ObjectClass = ObjectClass {
    extends: Some(&crate::ported::object::Object_class),
};

impl Object for ScreenListItem {
    /// C `this->klass` set to `&ScreenListItem_class`.
    fn klass(&self) -> &'static ObjectClass {
        &ScreenListItem_class
    }

    /// C vtable slot `.display = ListItem_display` — the row draws exactly
    /// like a plain `ListItem` over its embedded `super`.
    fn display(&self, out: &mut RichString) {
        ListItem_display(&self.super_, out);
    }

    /// C vtable slot `.compare = ListItem_compare` — compares the embedded
    /// `ListItem` values. The C comparator casts the opaque `const void*`
    /// back to the concrete type; the safe-Rust analog downcasts via `Any`.
    fn compare(&self, other: &dyn Object) -> i32 {
        let any: &dyn core::any::Any = other;
        let o = any
            .downcast_ref::<ScreenListItem>()
            .expect("ScreenListItem_compare called across incompatible classes");
        ListItem_compare(&self.super_, &o.super_)
    }
}

/// TODO: port of `static void ScreenListItem_delete(Object* cast)` from
/// `ScreensPanel.c:28`. Frees `this->ss` (when set) then `ListItem_delete`;
/// in the reduced model the row only holds an index (`ssIndex`, a plain
/// scalar) into the settings-owned `Vec`, not the screen itself, so there
/// is nothing to free — the `ListItem` releases via `Drop` (same precedent
/// as `ListItem_delete`).
pub fn ScreenListItem_delete() {
    todo!("port of ScreensPanel.c:28 — Drop releases the ListItem; the screen is owned by Settings.screens")
}

/// Port of `ScreenListItem* ScreenListItem_new(const char* value,
/// ScreenSettings* ss)` from `ScreensPanel.c:43`. The C body is
/// `AllocThis(ScreenListItem); ListItem_init((ListItem*)this, value, 0);
/// this->ss = ss;`. The heap allocation becomes an owned return value (the
/// `ListItem_new` owned-return idiom); [`ListItem_new`] performs the same
/// `ListItem_init` (`value`, `key = 0`, `moving = false`), and the C
/// `ScreenSettings* ss` pointer is modeled as the screen's index into
/// [`Settings::screens`] (`ssIndex`).
pub fn ScreenListItem_new(value: &str, ssIndex: usize) -> ScreenListItem {
    ScreenListItem {
        super_: ListItem_new(value, 0),
        ssIndex,
    }
}

/// Port of `void ScreensPanel_cleanup(void)` from `ScreensPanel.c:57`.
///
/// ```c
/// if (Screens_renamingBar) {
///    FunctionBar_delete(Screens_renamingBar);
///    Screens_renamingBar = NULL;
/// }
/// ```
///
/// Tears down the process-wide renaming [`Screens_renamingBar`] if one was
/// ever built: dropping the `Some` payload is the analog of `FunctionBar_delete`
/// and leaving `None` is the `= NULL`. Idempotent (the C `NULL` guard) — the
/// same body as `MetersPanel_cleanup` / `ScreenTabsPanel_cleanup`.
pub fn ScreensPanel_cleanup() {
    let mut bar = Screens_renamingBar.lock().unwrap();
    if bar.is_some() {
        // Drop frees the bar (C `FunctionBar_delete`); `None` is the NULL.
        *bar = None;
    }
}

/// Port of `static void ScreensPanel_cancelMoving(ScreensPanel* this)`
/// from `ScreensPanel.c:64`. Walks every row of the embedded panel and
/// clears its `moving` flag, clears the panel's own `moving`, then restores
/// `Panel_setSelectionColor(super, PANEL_SELECTION_FOCUS)`.
///
/// The C loop is `for (i < Panel_size(super)) { ListItem* item =
/// (ListItem*) Panel_get(super, i); if (item) item->moving = false; }`. The
/// ported `Panel_get` returns an immutable `&dyn Object`, so the faithful
/// mutating analog indexes `super.items` directly and downcasts each row
/// `&mut dyn Object` to `&mut ScreenListItem` via the `Any` supertrait (the
/// safe-Rust analog of the C `(ListItem*)` cast), writing `super_.moving`. A
/// `Vec` element is never null, so the C `if (item)` guard is always taken.
pub fn ScreensPanel_cancelMoving(this: &mut ScreensPanel) {
    let super_ = &mut this.super_;
    let size = Panel_size(super_);
    for i in 0..size {
        let obj: &mut dyn Object = super_.items[i as usize].object_mut();
        let any: &mut dyn core::any::Any = obj;
        if let Some(item) = any.downcast_mut::<ScreenListItem>() {
            item.super_.moving = false;
        }
    }
    this.moving = false;
    Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_SELECTION_FOCUS);
}

/// TODO: port of `static void ScreensPanel_delete(Object* object)` from
/// `ScreensPanel.c:75`. The destructor: cancel any pending edit (restore
/// `item->value = this->saved`, clear `renamingItem`/`cursorOn`, restore
/// the focus color), then null every `item->ss` so the settings array
/// keeps ownership, then `Panel_delete`. That bookkeeping only matters for
/// the C manual-free protocol; the owned model destructs via `Drop`, so
/// there is no algorithm to port (same precedent as every sibling
/// `_delete`).
pub fn ScreensPanel_delete() {
    todo!("port of ScreensPanel.c:75 — Drop releases the panel")
}

/// Port of `static HandlerResult ScreensPanel_eventHandlerRenaming(Panel*
/// super, int ch)` from `ScreensPanel.c:102`. The rename-mode key `switch`,
/// always returning [`HandlerResult::HANDLED`]:
///
/// - `EVENT_SET_SELECTED` — if the selection moved off the item under edit,
///   finish the rename (C `if (item != renamingItem) goto renameFinish`).
/// - `EVENT_PANEL_LOST_FOCUS` — finish the rename.
/// - `\n` / `\r` / `KEY_ENTER` / `F10` — finish (unless the list is empty,
///   the C `if (!item) break`).
/// - `Esc` / `F2` — cancel: restore the row's original value from
///   `this->saved`; if it was a freshly-added item, remove it and
///   [`rebuildSettingsArray`]; then clear the rename state.
/// - default — feed the key to the [`LineEditor`], update `selectedLen` /
///   the cursor, and re-point the row's display value at the live editor
///   text (excluding `'='`, which the config format reserves).
///
/// The C `renameFinish` `goto` (reached from three arms) is expressed as a
/// `do_finish` flag whose shared body runs after the `match`. The finish
/// body calls the now-ported [`ScreensPanel_update`] and the
/// cancel-of-a-new-item path calls the now-ported [`rebuildSettingsArray`];
/// both reach [`Settings::screens`] through [`ScreensPanel::settings`], so
/// those paths require a live back-pointer (a null pointer is only safe on
/// the editor-edit / EVENT-select / Esc-cancel-of-existing paths, which
/// never touch settings). The C `renamingItem` pointer is modeled as the row index
/// ([`ScreensPanel::renamingItem`]); `this->saved` (the original heap name)
/// is an owned `String` moved back into the row on cancel and dropped on
/// finish (C `free(this->saved)`).
pub fn ScreensPanel_eventHandlerRenaming(this: &mut ScreensPanel, ch: i32) -> HandlerResult {
    let mut do_finish = false;

    match ch {
        EVENT_SET_SELECTED => {
            // C: item = Panel_getSelected; if (item != renamingItem) goto renameFinish;
            // An empty panel (item == NULL) also differs from the renaming item.
            let sel = Panel_getSelectedIndex(&this.super_);
            if this.super_.items.is_empty() || this.renamingItem != Some(sel as usize) {
                do_finish = true;
            }
        }
        EVENT_PANEL_LOST_FOCUS => {
            do_finish = true;
        }
        NEWLINE | CARRIAGE_RETURN | crate::ported::crt::KEY_ENTER | KEY_F10 => {
            // C: item = Panel_getSelected; if (!item) break; else fall to renameFinish.
            if !this.super_.items.is_empty() {
                do_finish = true;
            }
        }
        ESC | KEY_F2 => {
            // C: item = Panel_getSelected; if (!item) break;
            if this.super_.items.is_empty() {
                return HandlerResult::HANDLED;
            }
            let idx = Panel_getSelectedIndex(&this.super_) as usize;
            // Restore item->value to the saved original name.
            let saved = this.saved.take().unwrap_or_default();
            this.set_item_value(idx, saved);

            if this.renamingNewItem {
                // Canceling a newly created item: delete it, then rebuild
                // with the updated selection (transitively hits the stub).
                let rm = Panel_getSelectedIndex(&this.super_);
                Panel_remove(&mut this.super_, rm);
                let sel = Panel_getSelectedIndex(&this.super_);
                rebuildSettingsArray(this, sel);
            }

            this.renamingNewItem = false;
            this.renamingItem = None;
            this.super_.cursorOn = false;
            Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_SELECTION_FOCUS);
            Panel_setDefaultBar(&mut this.super_);
            return HandlerResult::HANDLED;
        }
        _ => {
            // Delegate editing keys to LineEditor, excluding '=' so the
            // htop config format is not broken.
            if ch == EQUALS {
                return HandlerResult::HANDLED;
            }
            LineEditor_handleKey(&mut this.editor, ch);
            this.super_.selectedLen = LineEditor_getCursor(&this.editor);
            Panel_setCursorToSelection(&mut this.super_);
            // Keep item->value pointing at the display (editor) buffer.
            if let Some(idx) = this.renamingItem {
                let text = LineEditor_getText(&this.editor).to_string();
                this.set_item_value(idx, text);
            }
            return HandlerResult::HANDLED;
        }
    }

    if do_finish {
        // C renameFinish: if (!this->renamingItem) break;
        let idx = match this.renamingItem {
            Some(idx) => idx,
            None => return HandlerResult::HANDLED,
        };
        // free(this->saved): drop the original name.
        this.saved = None;
        // renamingItem->value = xStrdup(LineEditor_getText(&editor)).
        let text = LineEditor_getText(&this.editor).to_string();
        this.set_item_value(idx, text);
        this.renamingItem = None;
        this.renamingNewItem = false;
        this.super_.cursorOn = false;
        Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_SELECTION_FOCUS);
        Panel_setDefaultBar(&mut this.super_);
        ScreensPanel_update(this);
    }

    HandlerResult::HANDLED
}

impl ScreensPanel {
    /// Set row `idx`'s display value (the C `((ListItem*) item)->value =
    /// ...` write). Gate-skipped associated fn — not a C function — shared
    /// by the several `item->value = ...` assignments in the rename handler,
    /// downcasting the row `&mut dyn Object` to `&mut ScreenListItem` via the
    /// `Any` supertrait (the same mutating analog `ScreensPanel_cancelMoving`
    /// uses, since ported `Panel_get`/`Panel_getSelected` hand back an
    /// immutable `&dyn Object`).
    fn set_item_value(&mut self, idx: usize, value: String) {
        let obj: &mut dyn Object = self.super_.items[idx].object_mut();
        let any: &mut dyn core::any::Any = obj;
        if let Some(item) = any.downcast_mut::<ScreenListItem>() {
            item.super_.value = value;
        }
    }

    /// Read row `idx`'s display value (`((ListItem*) item)->value`).
    /// Gate-skipped associated fn — the shared read side of the
    /// `set_item_value` downcast, used by [`ScreensPanel_update`].
    fn item_value(&self, idx: usize) -> String {
        let any: &dyn core::any::Any = self.super_.items[idx].object();
        any.downcast_ref::<ScreenListItem>()
            .expect("ScreensPanel row is not a ScreenListItem")
            .super_
            .value
            .clone()
    }

    /// Read row `idx`'s `ssIndex` (the modeled `item->ss` alias). Gate-
    /// skipped associated fn shared by the reorder in
    /// [`rebuildSettingsArray`] / [`ScreensPanel_update`].
    fn item_ssIndex(&self, idx: usize) -> usize {
        let any: &dyn core::any::Any = self.super_.items[idx].object();
        any.downcast_ref::<ScreenListItem>()
            .expect("ScreensPanel row is not a ScreenListItem")
            .ssIndex
    }

    /// Set row `idx`'s `ssIndex`. Gate-skipped associated fn: after a
    /// reorder each row maps to its new slot in [`Settings::screens`], so
    /// the reorder functions rewrite the index to keep the alias exact.
    fn set_item_ssIndex(&mut self, idx: usize, ssIndex: usize) {
        let any: &mut dyn core::any::Any = self.super_.items[idx].object_mut();
        if let Some(item) = any.downcast_mut::<ScreenListItem>() {
            item.ssIndex = ssIndex;
        }
    }

    /// Set row `idx`'s `moving` flag (the C `item->moving = ...` write).
    /// Gate-skipped associated fn used by the Enter arm of
    /// [`ScreensPanel_eventHandlerNormal`].
    fn set_item_moving(&mut self, idx: usize, moving: bool) {
        let any: &mut dyn core::any::Any = self.super_.items[idx].object_mut();
        if let Some(item) = any.downcast_mut::<ScreenListItem>() {
            item.super_.moving = moving;
        }
    }

    /// Row-identity of the panel item at `idx`, or null when out of range.
    /// Gate-skipped associated fn (not a C function) modeling the C pointer
    /// comparison in [`ScreensPanel_eventHandlerNormal`]'s focus-change
    /// tail: C compares `ScreenListItem*` values, and the `Box`'s pointee
    /// address is stable across `Vec` reordering, so the thin data address
    /// is the safe analog. The C reads `Panel_get(super,
    /// super->prevSelected)` unguarded (`Vector_get` asserts in range); the
    /// reduced model returns null for an out-of-range index (e.g. the
    /// initial `prevSelected == -1`) since `Panel_get` panics there and the
    /// C invariant keeps `prevSelected` valid during real operation.
    fn focus_ptr(&self, idx: i32) -> *const () {
        if idx >= 0 && (idx as usize) < self.super_.items.len() {
            self.super_.items[idx as usize].object() as *const dyn Object as *const ()
        } else {
            core::ptr::null()
        }
    }

    /// The process-global `static FunctionBar* Screens_renamingBar`
    /// (`ScreensPanel.c:53`), lazily built once (C builds it on first
    /// `ScreensPanel_new`). Gate-skipped associated fn that builds the shared
    /// [`Screens_renamingBar`] on first use and hands [`startRenaming`] a clone
    /// to store in `super->currentBar` (the `Panel_setDefaultBar` clone idiom).
    /// `FunctionBar_new(.., None, None)` reproduces the C
    /// `FunctionBar_new(ScreensRenamingFunctions, NULL, NULL)` (static
    /// F-key/event tables).
    fn screens_renamingBar() -> FunctionBar {
        let mut bar = Screens_renamingBar.lock().unwrap();
        if bar.is_none() {
            *bar = Some(FunctionBar_new(Some(&ScreensRenamingFunctions), None, None));
        }
        bar.as_ref().unwrap().clone()
    }
}

/// Port of `static void startRenaming(Panel* super)` from
/// `ScreensPanel.c:179`. Enters rename mode for the selected row: cancels
/// any in-progress move, records the row index (`renamingItem`) and its
/// original name (`saved`), seeds the [`LineEditor`] with that name (capped
/// to `SCREEN_NAME_LEN - 1` chars), re-points the row's display value at
/// the live editor text, switches to the `PANEL_EDIT` color and the shared
/// renaming [`FunctionBar`], and places the cursor. Returns early when the
/// list is empty (C `if (item == NULL) return`).
pub fn startRenaming(this: &mut ScreensPanel) {
    if this.super_.items.is_empty() {
        return;
    }
    let sel = Panel_getSelectedIndex(&this.super_);
    if this.moving {
        ScreensPanel_cancelMoving(this);
    }
    this.renamingItem = Some(sel as usize);
    this.super_.cursorOn = true;
    // char* name = item->value; this->saved = name;
    let name = {
        let obj = Panel_get(&this.super_, sel);
        let any: &dyn core::any::Any = obj;
        any.downcast_ref::<ScreenListItem>()
            .expect("startRenaming: panel row is not a ScreenListItem")
            .super_
            .value
            .clone()
    };
    this.saved = Some(name.clone());
    // LineEditor_initWithMax(&editor, SCREEN_NAME_LEN - 1); setText(name).
    LineEditor_initWithMax(&mut this.editor, SCREEN_NAME_LEN - 1);
    LineEditor_setText(&mut this.editor, &name);
    // item->value = LineEditor_getText(&editor) — draw the live buffer.
    let text = LineEditor_getText(&this.editor).to_string();
    this.set_item_value(sel as usize, text);
    Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_EDIT);
    this.super_.selectedLen = LineEditor_getCursor(&this.editor);
    Panel_setCursorToSelection(&mut this.super_);
    this.super_.currentBar = Some(ScreensPanel::screens_renamingBar());
}

/// Port of `static void rebuildSettingsArray(Panel* super, int selected)`
/// from `ScreensPanel.c:202`. Rebuilds `settings->screens[]` so its order
/// matches the panel's current row order, sets `nScreens`, clamps
/// `selected`, and sets `settings->ssIndex`.
///
/// The C frees the old array, mallocs `n + 1` slots, and copies each
/// `item->ss` pointer into `screens[i]` (a pointer reorder that preserves
/// each screen's identity). The owned model reaches the single
/// [`Settings::screens`] `Vec` through [`ScreensPanel::settings`] and
/// reorders it by moving `screens[item.ssIndex]` into the new slot `i`, in
/// panel-row order — any screen no longer referenced by a row is dropped
/// (the C freed those when the `ScreenListItem` was deleted, e.g. the
/// cancel-of-a-new-item path). Each row's `ssIndex` is then rewritten to
/// its new slot so the alias stays exact. `nScreens` is implicit in
/// `screens.len()`; the C `settings->ss` back-pointer is not modeled
/// (`settings.rs` tracks only `ssIndex`).
pub fn rebuildSettingsArray(this: &mut ScreensPanel, selected: i32) {
    let n = Panel_size(&this.super_) as usize;

    // C: for each row i, screens[i] = item->ss. The reorder key is the
    // row's current ssIndex into the old array.
    let order: Vec<usize> = (0..n).map(|i| this.item_ssIndex(i)).collect();

    // The back-pointer targets a `Settings` owned elsewhere (not part of
    // `this`), so the deref does not alias `this.super_.items`.
    let settings = unsafe { &mut *this.settings };
    let old = std::mem::take(&mut settings.screens);
    let mut slots: Vec<Option<ScreenSettings>> = old.into_iter().map(Some).collect();
    let mut new_screens = Vec::with_capacity(n);
    for &oldidx in &order {
        new_screens.push(
            slots[oldidx]
                .take()
                .expect("each panel row references a distinct live screen"),
        );
    }
    settings.screens = new_screens;

    // Rows now map to their new slots.
    for i in 0..n {
        this.set_item_ssIndex(i, i);
    }

    // this->settings->nScreens = n; (implicit in screens.len())
    // ensure selection is in valid range
    let mut selected = selected;
    if selected > n as i32 - 1 {
        selected = n as i32 - 1;
    } else if selected < 0 {
        selected = 0;
    }
    let settings = unsafe { &mut *this.settings };
    settings.ssIndex = selected as u32;
    // this->settings->ss = screens[selected]; — back-pointer not modeled.
}

/// TODO: port of `static void addNewScreen(Panel* super)` from
/// `ScreensPanel.c:223`. Calls `Settings_newScreen(this->settings, ...)`
/// (stubbed on the platform `Process_fields[]` table), wraps the result in
/// a [`ScreenListItem`], inserts it after the selection, and selects it.
/// Blocked on `Settings_newScreen` + the `settings->screens[]` aliasing.
pub fn addNewScreen() {
    todo!("port of ScreensPanel.c:223 — needs Settings_newScreen (Process_fields[]) + settings->screens[] aliasing")
}

/// Port of `static HandlerResult ScreensPanel_eventHandlerNormal(Panel*
/// super, int ch)` from `ScreensPanel.c:234`. The non-rename key `switch`:
/// Enter toggles move mode, arrow/`F7`/`F8`/`[`/`]`/`-`/`+` reorder the
/// selected row, `F2`/`^R` / double-click start a rename, `F9`/`Del`
/// removes, page keys scroll, and an alpha key type-selects. After the
/// switch it records `prevSelected`, rebuilds the settings array when a
/// reorder happened ([`rebuildSettingsArray`]), and, when the event was
/// handled, syncs the settings ([`ScreensPanel_update`]).
///
/// Two pieces need substrate the reduced model cannot provide and are
/// honest inline stubs (the same transitive-block pattern
/// [`ScreensPanel_eventHandlerRenaming`] uses):
///
/// - the `F5`/`^N` "new screen" arm reads `this->settings->dynamicScreens`
///   (a `Hashtable` field absent from the modeled `Settings`, which cannot
///   be edited) and calls the stubbed [`addNewScreen`]
///   (`Settings_newScreen` -> platform `Process_fields[]`);
/// - the focus-change tail (`newFocus && oldFocus != newFocus`) calls
///   `ColumnsPanel_fill(this->columns, ...)` /
///   `AvailableColumnsPanel_fill(this->availableColumns, ...)` against
///   `this->settings->dynamicColumns` — the `columns` / `availableColumns`
///   sub-panels are omitted from the reduced struct (their constructors
///   `ColumnsPanel_new` / `AvailableColumnsPanel_new` are stubbed), the
///   `*_fill` functions are stubbed, and `dynamicColumns` is another
///   unmodeled `Settings` `Hashtable` field.
///
/// So arms that keep the focused row identity (Enter toggle, MoveUp/Dn,
/// rename, event-select, lost-focus, page-nav that cannot move) run to
/// completion and drive the now-ported [`rebuildSettingsArray`] /
/// [`ScreensPanel_update`]; arms that change the focused row (delete,
/// type-select, an actual page move) reach the focus-change stub, and
/// `F5`/`^N` reaches the new-screen stub.
pub fn ScreensPanel_eventHandlerNormal(this: &mut ScreensPanel, ch: i32) -> HandlerResult {
    // C: const void* oldFocus = Panel_get(super, super->prevSelected);
    let oldFocus = this.focus_ptr(this.super_.prevSelected);
    let mut shouldRebuildArray = false;
    let mut result = HandlerResult::IGNORED;

    match ch {
        NEWLINE | CARRIAGE_RETURN | KEY_ENTER => {
            if this.moving {
                ScreensPanel_cancelMoving(this);
            } else {
                this.moving = true;
                Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_SELECTION_FOLLOW);
                // ListItem* item = Panel_getSelected(super); if (item) item->moving = true;
                if !this.super_.items.is_empty() {
                    let idx = Panel_getSelectedIndex(&this.super_) as usize;
                    this.set_item_moving(idx, true);
                }
            }
            result = HandlerResult::HANDLED;
        }
        KEY_MOUSE => {
            if this.moving {
                // Single click while in move mode: cancel move mode.
                ScreensPanel_cancelMoving(this);
                result = HandlerResult::HANDLED;
            }
            // else: just select the item, do not enter move mode
        }
        KEY_RECLICK => {
            // Double click: start renaming.
            this.renamingNewItem = false;
            startRenaming(this);
            result = HandlerResult::HANDLED;
        }
        EVENT_SET_SELECTED => {
            if this.moving {
                ScreensPanel_cancelMoving(this);
            }
            result = HandlerResult::HANDLED;
        }
        EVENT_PANEL_LOST_FOCUS => {
            if this.moving {
                ScreensPanel_cancelMoving(this);
            }
            result = HandlerResult::HANDLED;
        }
        KEY_NPAGE | KEY_PPAGE | KEY_HOME | KEY_END => {
            Panel_onKey(&mut this.super_, ch);
        }
        KEY_F2 | KEY_CTRL_R => {
            this.renamingNewItem = false;
            startRenaming(this);
            result = HandlerResult::HANDLED;
        }
        KEY_F5 | KEY_CTRL_N => {
            // C: if (this->settings->dynamicScreens) break; addNewScreen(super);
            //    this->renamingNewItem = true; startRenaming(super);
            //    shouldRebuildArray = true; result = HANDLED;
            todo!("port of ScreensPanel.c:292 — needs settings->dynamicScreens (unmodeled Settings Hashtable field) + addNewScreen (Settings_newScreen -> Process_fields[])")
        }
        KEY_UP => {
            if !this.moving {
                Panel_onKey(&mut this.super_, ch);
            } else {
                // FALLTHRU to MoveUp
                Panel_moveSelectedUp(&mut this.super_);
                shouldRebuildArray = true;
                result = HandlerResult::HANDLED;
            }
        }
        KEY_F7 | LBRACKET | MINUS => {
            Panel_moveSelectedUp(&mut this.super_);
            shouldRebuildArray = true;
            result = HandlerResult::HANDLED;
        }
        KEY_DOWN => {
            if !this.moving {
                Panel_onKey(&mut this.super_, ch);
            } else {
                // FALLTHRU to MoveDn
                Panel_moveSelectedDown(&mut this.super_);
                shouldRebuildArray = true;
                result = HandlerResult::HANDLED;
            }
        }
        KEY_F8 | RBRACKET | PLUS => {
            Panel_moveSelectedDown(&mut this.super_);
            shouldRebuildArray = true;
            result = HandlerResult::HANDLED;
        }
        KEY_F9 | KEY_DC | KEY_DEL_MAC => {
            if Panel_size(&this.super_) > 1 {
                let sel = this.super_.selected;
                Panel_remove(&mut this.super_, sel);
            }
            shouldRebuildArray = true;
            result = HandlerResult::HANDLED;
        }
        _ => {
            if (0..255).contains(&ch) && (ch as u8 as char).is_ascii_alphabetic() {
                result = Panel_selectByTyping(&mut this.super_, ch);
            }
            if result == HandlerResult::BREAK_LOOP {
                result = HandlerResult::IGNORED;
            }
        }
    }

    // C: ScreenListItem* newFocus = Panel_getSelected(super);
    //    if (newFocus && oldFocus != newFocus) { fill the column panels; result = HANDLED; }
    let newFocus = if this.super_.items.is_empty() {
        core::ptr::null()
    } else {
        this.focus_ptr(this.super_.selected)
    };
    if !newFocus.is_null() && oldFocus != newFocus {
        todo!("port of ScreensPanel.c:345 — needs columns/availableColumns sub-panels + stubbed ColumnsPanel_fill/AvailableColumnsPanel_fill + settings->dynamicColumns (unmodeled Settings Hashtable field)")
    }

    this.super_.prevSelected = this.super_.selected;

    if shouldRebuildArray {
        let sel = this.super_.selected;
        rebuildSettingsArray(this, sel);
    }

    if result == HandlerResult::HANDLED {
        ScreensPanel_update(this);
    }

    result
}

/// Port of `static HandlerResult ScreensPanel_eventHandler(Panel* super,
/// int ch)` from `ScreensPanel.c:363`. Dispatches to the renaming handler
/// while a rename is in progress (C `if (this->renamingItem)`), otherwise
/// to the normal handler. [`ScreensPanel_eventHandlerNormal`] is still a
/// stub, so the not-renaming path panics in it (the honest transitive
/// block); the renaming path runs [`ScreensPanel_eventHandlerRenaming`].
pub fn ScreensPanel_eventHandler(this: &mut ScreensPanel, ch: i32) -> HandlerResult {
    if this.renamingItem.is_some() {
        ScreensPanel_eventHandlerRenaming(this, ch)
    } else {
        ScreensPanel_eventHandlerNormal(this, ch)
    }
}

/// TODO: port of `ScreensPanel* ScreensPanel_new(Settings* settings)` from
/// `ScreensPanel.c:381`. Allocates the panel, builds the two child panels
/// via the stubbed `ColumnsPanel_new` / `AvailableColumnsPanel_new`, and
/// seeds the rows from `settings->screens[]` — the same `item->ss`
/// aliasing `settings->screens[i]` that blocks [`rebuildSettingsArray`].
/// Left as a stub.
pub fn ScreensPanel_new() {
    todo!("port of ScreensPanel.c:381 — needs ColumnsPanel_new/AvailableColumnsPanel_new (stubbed) + settings->screens[] aliasing")
}

/// Port of `void ScreensPanel_update(Panel* super)` from
/// `ScreensPanel.c:415`. Marks the settings dirty (`changed = true`,
/// `lastUpdate++`), then rewrites `settings->screens[]` from the rows:
/// each screen's `heading` is set to its row's display value and the array
/// is reordered to panel-row order.
///
/// The C reallocs `screens[]` to `size + 1`, then for each row writes
/// `free_and_xStrdup(&ss->heading, item->value)` and stores the `item->ss`
/// pointer into `screens[i]` (`screens[size] = NULL`). The owned model
/// reaches [`Settings::screens`] through [`ScreensPanel::settings`] and
/// reorders it by moving `screens[item.ssIndex]` into slot `i`, updating
/// that screen's `heading` from the row value (`free_and_xStrdup` ->
/// assign `Some(value)`); each row's `ssIndex` is rewritten to `i` so the
/// alias stays exact. The C `NULL` terminator is not modeled (the `Vec`
/// length bounds the array).
pub fn ScreensPanel_update(this: &mut ScreensPanel) {
    let size = Panel_size(&this.super_) as usize;

    // Snapshot each row's (source screen index, display value) in panel
    // order before moving screens out of the settings Vec.
    let rows: Vec<(usize, String)> = (0..size)
        .map(|i| (this.item_ssIndex(i), this.item_value(i)))
        .collect();

    let settings = unsafe { &mut *this.settings };
    settings.changed = true;
    settings.lastUpdate += 1;

    let old = std::mem::take(&mut settings.screens);
    let mut slots: Vec<Option<ScreenSettings>> = old.into_iter().map(Some).collect();
    let mut new_screens = Vec::with_capacity(size);
    for (oldidx, value) in &rows {
        let mut ss = slots[*oldidx]
            .take()
            .expect("each panel row references a distinct live screen");
        // free_and_xStrdup(&ss->heading, item->value)
        ss.heading = Some(value.clone());
        new_screens.push(ss);
    }
    settings.screens = new_screens;

    // Rows now map to their new slots.
    for i in 0..size {
        this.set_item_ssIndex(i, i);
    }
}

/// Reduced model of the C `ScreensPanel` struct (`ScreensPanel.h:27`). Only
/// the fields the ported functions touch are modeled: the embedded `Panel
/// super`, the inline rename [`LineEditor`], and the four rename/move
/// scalars. The `settings` / `columns` / `availableColumns` / `scr`
/// back-pointers and the C `char buffer[]` scratch are omitted — every
/// function that dereferences them stays a stub (see the module docs).
/// `super_` avoids the Rust `super` keyword, matching the `columnspanel.rs`
/// convention.
pub struct ScreensPanel {
    /// C `Panel super` — the embedded panel base.
    pub super_: Panel,
    /// C `Settings* settings` — back-pointer to the config layer owned by
    /// `htop.c`, modeled as a raw pointer (the `MainPanel.state` precedent).
    /// [`rebuildSettingsArray`] / [`ScreensPanel_update`] reach
    /// [`Settings::screens`] through it; the reduced-model tests wire it to
    /// a heap `Settings` and the rename/move helpers that never touch
    /// settings leave it null.
    pub settings: *mut Settings,
    /// C `LineEditor editor` — the inline editor used while renaming.
    pub editor: LineEditor,
    /// C `bool moving` — whether the panel is in row-reorder mode.
    pub moving: bool,
    /// C `char* saved` — the row's original name, restored on cancel.
    pub saved: Option<String>,
    /// C `ListItem* renamingItem` — the row under edit, modeled as its
    /// index (`None` == C `NULL`, i.e. not renaming).
    pub renamingItem: Option<usize>,
    /// C `bool renamingNewItem` — whether the row under edit was just added.
    pub renamingNewItem: bool,
}

#[cfg(test)]
use crate::ported::panel::PanelItem;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::crt::{KEY_ENTER, KEY_UP};
    use crate::ported::panel::Panel_new;
    use crate::ported::settings::HeaderLayout;

    /// A `ScreenListItem` row named `name` whose `ssIndex` is `idx`.
    fn row(name: &str, idx: usize) -> Box<dyn Object> {
        Box::new(ScreenListItem_new(name, idx))
    }

    /// A `ScreensPanel` whose embedded panel holds the named rows (each
    /// row's `ssIndex` is its position) with a NULL `settings` back-pointer
    /// — for the rename/move helpers that never dereference it.
    fn panel_with(names: &[&str]) -> ScreensPanel {
        let mut super_ = Panel_new(1, 1, 20, 10, None);
        for (i, n) in names.iter().enumerate() {
            super_.items.push(PanelItem::Owned(row(n, i)));
        }
        // ScreensPanel_new sets `super->prevSelected = super->selected`.
        super_.prevSelected = super_.selected;
        ScreensPanel {
            super_,
            settings: core::ptr::null_mut(),
            editor: LineEditor::default(),
            moving: false,
            saved: None,
            renamingItem: None,
            renamingNewItem: false,
        }
    }

    /// A heap `Settings` with one screen per name (`heading == name`).
    fn make_settings(names: &[&str]) -> Box<Settings> {
        let screens = names
            .iter()
            .map(|n| ScreenSettings {
                heading: Some((*n).to_string()),
                ..Default::default()
            })
            .collect();
        Box::new(Settings {
            hLayout: HeaderLayout::HF_ONE_100,
            hColumns: Vec::new(),
            screens,
            ssIndex: 0,
            changed: false,
            lastUpdate: 0,
            ..Default::default()
        })
    }

    /// A `ScreensPanel` wired to a live heap `Settings` (one screen per
    /// name, rows' `ssIndex` matching). Returns the boxed `Settings` so the
    /// caller keeps it alive for the panel's raw back-pointer.
    fn wired(names: &[&str]) -> (Box<Settings>, ScreensPanel) {
        let mut settings = make_settings(names);
        let ptr: *mut Settings = settings.as_mut();
        let mut super_ = Panel_new(1, 1, 20, 10, None);
        for (i, n) in names.iter().enumerate() {
            super_.items.push(PanelItem::Owned(row(n, i)));
        }
        super_.prevSelected = super_.selected;
        let sp = ScreensPanel {
            super_,
            settings: ptr,
            editor: LineEditor::default(),
            moving: false,
            saved: None,
            renamingItem: None,
            renamingNewItem: false,
        };
        (settings, sp)
    }

    /// The display value of row `idx`.
    fn value_at(p: &ScreensPanel, idx: usize) -> String {
        let any: &dyn core::any::Any = p.super_.items[idx].object();
        any.downcast_ref::<ScreenListItem>()
            .unwrap()
            .super_
            .value
            .clone()
    }

    /// Row `idx`'s modeled `ss` alias (`ssIndex`).
    fn ss_index_at(p: &ScreensPanel, idx: usize) -> usize {
        let any: &dyn core::any::Any = p.super_.items[idx].object();
        any.downcast_ref::<ScreenListItem>().unwrap().ssIndex
    }

    /// The `heading` of every screen in `settings`, in order.
    fn headings(s: &Settings) -> Vec<String> {
        s.screens
            .iter()
            .map(|ss| ss.heading.clone().unwrap_or_default())
            .collect()
    }

    /// Set the `moving` flag on row `idx` (test helper).
    fn set_moving(p: &mut ScreensPanel, idx: usize, moving: bool) {
        let any: &mut dyn core::any::Any = p.super_.items[idx].object_mut();
        any.downcast_mut::<ScreenListItem>().unwrap().super_.moving = moving;
    }

    // ── ScreenListItem_new ─────────────────────────────────────────────

    #[test]
    fn screen_list_item_new_inits_row_and_stores_ss() {
        let it = ScreenListItem_new("Main", 3);
        assert_eq!(it.super_.value, "Main");
        assert_eq!(it.super_.key, 0); // ListItem_init key
        assert!(!it.super_.moving);
        assert_eq!(it.ssIndex, 3); // the modeled `item->ss` alias
    }

    // ── ScreensPanel_cancelMoving ──────────────────────────────────────

    #[test]
    fn cancel_moving_clears_all_flags_and_restores_color() {
        let mut p = panel_with(&["a", "b", "c"]);
        p.moving = true;
        set_moving(&mut p, 0, true);
        set_moving(&mut p, 2, true);
        Panel_setSelectionColor(&mut p.super_, ColorElements::PANEL_SELECTION_FOLLOW);

        ScreensPanel_cancelMoving(&mut p);

        assert!(!p.moving);
        for i in 0..3 {
            let any: &dyn core::any::Any = p.super_.items[i].object();
            assert!(!any.downcast_ref::<ScreenListItem>().unwrap().super_.moving);
        }
        assert_eq!(
            p.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
    }

    // ── startRenaming ──────────────────────────────────────────────────

    #[test]
    fn start_renaming_enters_edit_state() {
        let mut p = panel_with(&["Main", "IO"]);
        p.super_.selected = 1;
        startRenaming(&mut p);

        assert_eq!(p.renamingItem, Some(1));
        assert_eq!(p.saved.as_deref(), Some("IO"));
        assert!(p.super_.cursorOn);
        assert_eq!(p.super_.selectionColorId, ColorElements::PANEL_EDIT);
        // Editor seeded with the current name; row value points at it.
        assert_eq!(LineEditor_getText(&p.editor), "IO");
        assert_eq!(value_at(&p, 1), "IO");
        assert_eq!(p.super_.selectedLen, 2); // cursor at end of "IO"
        assert!(p.super_.currentBar.is_some());
    }

    #[test]
    fn start_renaming_cancels_in_progress_move() {
        let mut p = panel_with(&["Main"]);
        p.moving = true;
        set_moving(&mut p, 0, true);
        startRenaming(&mut p);
        // cancelMoving ran: panel + row moving cleared.
        assert!(!p.moving);
        let any: &dyn core::any::Any = p.super_.items[0].object();
        assert!(!any.downcast_ref::<ScreenListItem>().unwrap().super_.moving);
        assert_eq!(p.renamingItem, Some(0));
    }

    #[test]
    fn start_renaming_empty_panel_is_noop() {
        let mut p = panel_with(&[]);
        startRenaming(&mut p);
        assert_eq!(p.renamingItem, None);
        assert!(!p.super_.cursorOn);
    }

    // ── ScreensPanel_eventHandlerRenaming (self-contained paths) ───────

    #[test]
    fn renaming_default_key_edits_editor_and_row_value() {
        let mut p = panel_with(&["Main"]);
        startRenaming(&mut p); // editor = "Main", cursor at 4
        let r = ScreensPanel_eventHandlerRenaming(&mut p, b'X' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(LineEditor_getText(&p.editor), "MainX");
        assert_eq!(value_at(&p, 0), "MainX"); // row follows the editor
        assert_eq!(p.super_.selectedLen, 5); // cursor advanced
    }

    #[test]
    fn renaming_equals_is_swallowed_without_editing() {
        let mut p = panel_with(&["Main"]);
        startRenaming(&mut p);
        let r = ScreensPanel_eventHandlerRenaming(&mut p, EQUALS);
        assert_eq!(r, HandlerResult::HANDLED);
        // '=' reserved by the config format: editor + row unchanged.
        assert_eq!(LineEditor_getText(&p.editor), "Main");
        assert_eq!(value_at(&p, 0), "Main");
    }

    #[test]
    fn renaming_esc_cancels_and_restores_original_name() {
        let mut p = panel_with(&["Main"]);
        startRenaming(&mut p);
        // Type an edit, then cancel: the saved original must be restored.
        ScreensPanel_eventHandlerRenaming(&mut p, b'Z' as i32);
        assert_eq!(value_at(&p, 0), "MainZ");
        let r = ScreensPanel_eventHandlerRenaming(&mut p, ESC);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(value_at(&p, 0), "Main"); // restored from `saved`
        assert_eq!(p.renamingItem, None);
        assert!(!p.super_.cursorOn);
        assert!(!p.renamingNewItem);
        assert_eq!(
            p.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
    }

    #[test]
    fn renaming_event_set_selected_same_row_does_not_finish() {
        // Selection unchanged (still the renaming row) => no finish, so the
        // stubbed ScreensPanel_update is NOT reached and this runs clean.
        let mut p = panel_with(&["Main", "IO"]);
        p.super_.selected = 0;
        startRenaming(&mut p); // renamingItem = 0, selected = 0
        let r = ScreensPanel_eventHandlerRenaming(&mut p, EVENT_SET_SELECTED);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(p.renamingItem, Some(0)); // still renaming
    }

    // ── ScreensPanel_eventHandler dispatch routing ─────────────────────

    #[test]
    fn dispatch_routes_to_normal_when_not_renaming() {
        // Not renaming => normal handler. KEY_UP on a single row cannot move
        // (selection stays, focus unchanged), so it runs clean and returns
        // IGNORED — the renaming handler would instead return HANDLED, so
        // the result alone proves the route. NULL settings is never touched.
        let mut p = panel_with(&["Main"]);
        let r = ScreensPanel_eventHandler(&mut p, KEY_UP);
        assert_eq!(r, HandlerResult::IGNORED);
    }

    #[test]
    fn dispatch_routes_to_renaming_when_renaming() {
        // renamingItem set => renaming handler; a default key edits cleanly.
        let mut p = panel_with(&["Main"]);
        startRenaming(&mut p);
        let r = ScreensPanel_eventHandler(&mut p, b'!' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(LineEditor_getText(&p.editor), "Main!");
    }

    #[test]
    fn renaming_enter_finish_syncs_settings() {
        // The finish path now runs the ported ScreensPanel_update: the edited
        // name lands in the row value AND the screen heading, settings dirty.
        let (settings, mut p) = wired(&["Main"]);
        startRenaming(&mut p);
        ScreensPanel_eventHandlerRenaming(&mut p, b'X' as i32); // editor -> "MainX"
        let r = ScreensPanel_eventHandlerRenaming(&mut p, KEY_ENTER);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(value_at(&p, 0), "MainX");
        assert_eq!(p.renamingItem, None);
        assert!(!p.super_.cursorOn);
        assert!(settings.changed);
        assert_eq!(settings.lastUpdate, 1);
        assert_eq!(headings(&settings), vec!["MainX"]);
    }

    // ── rebuildSettingsArray ───────────────────────────────────────────

    #[test]
    fn rebuild_reorders_screens_to_panel_order() {
        let (settings, mut p) = wired(&["A", "B", "C"]);
        // Simulate a user move: swap the first two rows (each Box carries its
        // ssIndex), so panel order is B, A, C with ssIndex 1, 0, 2.
        p.super_.items.swap(0, 1);
        rebuildSettingsArray(&mut p, 0);
        // screens reordered to panel order; rows remapped to their new slots.
        assert_eq!(headings(&settings), vec!["B", "A", "C"]);
        assert_eq!(ss_index_at(&p, 0), 0);
        assert_eq!(ss_index_at(&p, 1), 1);
        assert_eq!(ss_index_at(&p, 2), 2);
        assert_eq!(settings.ssIndex, 0);
    }

    #[test]
    fn rebuild_clamps_selection_into_range() {
        let (settings_hi, mut p_hi) = wired(&["A", "B", "C"]);
        rebuildSettingsArray(&mut p_hi, 9); // > n-1 -> clamp to 2
        assert_eq!(settings_hi.ssIndex, 2);

        let (settings_lo, mut p_lo) = wired(&["A", "B"]);
        rebuildSettingsArray(&mut p_lo, -3); // < 0 -> clamp to 0
        assert_eq!(settings_lo.ssIndex, 0);
    }

    // ── ScreensPanel_update ────────────────────────────────────────────

    #[test]
    fn update_writes_headings_and_marks_changed() {
        let (settings, mut p) = wired(&["A", "B"]);
        // Rename row 0's display value; update copies it into the heading.
        p.set_item_value(0, "Alpha".to_string());
        ScreensPanel_update(&mut p);
        assert!(settings.changed);
        assert_eq!(settings.lastUpdate, 1);
        assert_eq!(headings(&settings), vec!["Alpha", "B"]);
    }

    #[test]
    fn update_reorders_screens_to_panel_order() {
        let (settings, mut p) = wired(&["A", "B", "C"]);
        // Panel order becomes C, B, A (swap rows 0 and 2); ssIndex 2, 1, 0.
        p.super_.items.swap(0, 2);
        ScreensPanel_update(&mut p);
        assert_eq!(headings(&settings), vec!["C", "B", "A"]);
        assert_eq!(ss_index_at(&p, 0), 0);
        assert_eq!(ss_index_at(&p, 2), 2);
    }

    // ── ScreensPanel_eventHandlerNormal (ported arms) ──────────────────

    #[test]
    fn normal_enter_toggles_move_mode_and_updates() {
        let (settings, mut p) = wired(&["Main", "IO"]);
        let r = ScreensPanel_eventHandlerNormal(&mut p, KEY_ENTER);
        assert_eq!(r, HandlerResult::HANDLED);
        assert!(p.moving);
        assert_eq!(
            p.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOLLOW
        );
        let any: &dyn core::any::Any = p.super_.items[0].object();
        assert!(any.downcast_ref::<ScreenListItem>().unwrap().super_.moving);
        // result HANDLED => the ported ScreensPanel_update ran.
        assert!(settings.changed);
    }

    #[test]
    fn normal_move_down_reorders_rebuilds_and_updates() {
        let (settings, mut p) = wired(&["A", "B", "C"]);
        // F8 = unconditional MoveDn of the selected (row 0).
        let r = ScreensPanel_eventHandlerNormal(&mut p, KEY_F8);
        assert_eq!(r, HandlerResult::HANDLED);
        // panel rows now B, A, C; selection followed the moved row to idx 1.
        assert_eq!(value_at(&p, 0), "B");
        assert_eq!(value_at(&p, 1), "A");
        assert_eq!(p.super_.selected, 1);
        // settings.screens reordered to match, ssIndex tracks the selection.
        assert_eq!(headings(&settings), vec!["B", "A", "C"]);
        assert_eq!(settings.ssIndex, 1);
    }

    #[test]
    #[should_panic(expected = "ScreensPanel.c:345")]
    fn normal_delete_reaches_focus_change_stub() {
        // Deleting the focused row changes the focused item => the C
        // focus-change tail (ColumnsPanel_fill on the omitted sub-panels)
        // fires and hits the honest inline stub.
        let (_settings, mut p) = wired(&["A", "B", "C"]);
        p.super_.selected = 1;
        p.super_.prevSelected = 1;
        let _ = ScreensPanel_eventHandlerNormal(&mut p, KEY_F9);
    }

    #[test]
    #[should_panic(expected = "ScreensPanel.c:292")]
    fn normal_f5_new_screen_reaches_stub() {
        // The new-screen arm needs settings->dynamicScreens + addNewScreen.
        let (_settings, mut p) = wired(&["Main"]);
        let _ = ScreensPanel_eventHandlerNormal(&mut p, KEY_F5);
    }
}
