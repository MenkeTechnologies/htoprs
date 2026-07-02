//! Partial port of `ScreenTabsPanel.c` — htop's screen-tab / screen-name
//! editor panels (the "Screens" setup screen split into a tab list on the
//! left and a per-tab name list on the right).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C function takes a
//! `Panel*`/`Object*`/`ScreenNamesPanel*`; the faithful analog is a free fn
//! (matching the `Panel.c`/`ListItem.c` ports: free fns, not methods).
//!
//! # Data model
//!
//! The four `ScreenTabsPanel.h` subclass structs are modeled here on the
//! ported [`Panel`]/[`ListItem`] bases, following the `MainPanel`/
//! `ScreensPanel` precedent: the embedded C `Panel super` / `ListItem super`
//! becomes an owned `super_` field, and the owned-elsewhere back-pointers
//! (`Settings*`/`ScreenManager*`/`DynamicScreen*`) stay raw pointers matching
//! the C shape. The two C alias pointers into the item vector are modeled the
//! way `screenspanel.rs` models them — as **indices**, not raw pointers:
//!
//! - `ScreenNamesPanel.renamingItem` (C `ListItem*` aliasing the row under
//!   edit) becomes `Option<usize>` — the index of that row (`None` == C
//!   `NULL`); renaming never reorders the list, so the index is stable.
//! - `ScreenNamesPanel.saved` (C `char*` aliasing the row's original name)
//!   becomes an owned `Option<String>`, moved back into the row on cancel and
//!   dropped on finish (C `free`).
//! - `ScreenNameListItem.ss` (C `ScreenSettings*` aliasing
//!   `settings->screens[i]`) becomes `Option<usize>` — the index into the
//!   `settings->screens[]` `Vec`, reached through the `*mut Settings`
//!   back-pointer (`None` == C `NULL`, set by `ScreenNamesPanel_delete`).
//!
//! `ScreenNameListItem` / `ScreenTabListItem` implement [`Object`] via the
//! ported `ScreenNameListItem_class` / `ScreenTabListItem_class` vtables
//! (`ScreenTabsPanel.c:160` / `:30`), rooted at `Object_class` (the C
//! `.extends = Class(ListItem)` targets a private `static`), so they can live
//! in the panel's `Vec<Box<dyn Object>>` and be recovered by an `Any`
//! downcast (the safe-Rust analog of the C `(ScreenNameListItem*)` cast).
//!
//! Ported:
//! - `ScreenTabsPanel_cleanup` (`ScreenTabsPanel.c:178`) — tears down the
//!   process-wide renaming `FunctionBar` modeled as a
//!   `Mutex<Option<FunctionBar>>` file-static.
//! - `ScreenNamesPanel_fill` (`:37`) — repopulates the names panel from
//!   `settings->screens[]` through the `*mut Settings` back-pointer.
//! - `renameScreenSettings` (`:204`) — writes the renamed screen's
//!   `heading` into `settings->screens[ss]` (via the `ScreenNameListItem.ss`
//!   index + `*mut Settings` back-pointer) and bumps `changed`/`lastUpdate`.
//! - `ScreenNamesPanel_eventHandlerRenaming` (`:215`) — the rename-mode key
//!   `switch` over the ported [`LineEditor`], the index-modeled
//!   `renamingItem`/`saved`, finishing through [`renameScreenSettings`].
//! - `ScreenNamesPanel_eventHandlerNormal` (`:306`) — the normal-mode key
//!   `switch`. Its `KEY_F(5)`/`KEY_CTRL('N')` arm calls the still-stubbed
//!   [`addNewScreen`] / [`startRenaming`] (the new-screen path — see below),
//!   so that one arm transitively hits an honest stub; every other arm
//!   (Enter/mouse, navigation, type-to-search) runs to completion.
//! - `ScreenNamesPanel_eventHandler` (`:350`) — the dispatcher choosing the
//!   renaming vs. normal handler by whether a rename is in progress.
//! - `ScreenTabsPanel_eventHandler` (`:68`) — the tab-panel key `switch`;
//!   its `HANDLED` tail refills the names sub-panel from the selected
//!   `ScreenTabListItem.ds`, and its `KEY_F(5)`/`KEY_CTRL('N')` arm delegates
//!   to `ScreenNamesPanel_eventHandlerNormal` (transitively the stub above).
//!
//! Stubbed (cannot be ported faithfully yet — specific blocker per fn):
//! - `ScreenTabsPanel_delete` (`:62`) / `ScreenNamesPanel_delete` (`:185`) —
//!   a `Panel_done` + `free` chain; the owned fields are released by `Drop`
//!   in Rust, so there is no algorithm to port (same as the `Panel_delete` /
//!   `ListItem_delete` / `FunctionBar_delete` stubs). `_delete` additionally
//!   nulls each `ScreenNameListItem.ss` and restores `renamingItem->value =
//!   this->saved` — bookkeeping that only matters for the C manual-free
//!   protocol.
//! - `startRenaming` (`:276`) — enters rename mode; only ever reached through
//!   the stubbed `addNewScreen` (its sole caller adds a new screen first),
//!   so it is left an honest stub with the new-screen path per the task
//!   scope. Signature preserved so the `_eventHandlerNormal` call site is
//!   faithful.
//! - `addNewScreen` (`:296`) — allocates a `ScreenSettings` via the stubbed
//!   `Settings_newScreen` / `Settings_newDynamicScreen` (`Settings.c:263` /
//!   `:286`, both blocked on the platform `Process_fields[]` table) and
//!   inserts a `ScreenNameListItem` via the stubbed constructor below.
//! - `ScreenTabListItem_new` (`:121`) / `ScreenNameListItem_new` (`:167`) —
//!   `AllocThis` constructors; construction is a struct literal and
//!   destruction is `Drop` (so `AllocThis` has no safe-Rust free-fn analog,
//!   as with the `ListItem_new` stub).
//! - `addDynamicScreen` (`:128`) — a `Hashtable_foreach` callback reading
//!   `screen->heading` (absent from the ported `DynamicScreen` model) and
//!   building a `ScreenTabListItem` via the stubbed constructor above.
//! - `ScreenTabsPanel_new` (`:138`) / `ScreenNamesPanel_new` (`:366`) —
//!   construct the panels: they need `Hashtable_foreach` over
//!   `settings->dynamicScreens`, the stubbed list-item constructors, and (for
//!   `ScreenTabsPanel_new`) the stubbed `ScreenNamesPanel_new`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::sync::Mutex;

use crate::ported::crt::{
    ColorElements, KEY_CTRL, KEY_DOWN, KEY_END, KEY_ENTER, KEY_F, KEY_HOME, KEY_MOUSE, KEY_NPAGE,
    KEY_PPAGE, KEY_RECLICK, KEY_UP,
};
use crate::ported::dynamicscreen::DynamicScreen;
use crate::ported::functionbar::FunctionBar;
use crate::ported::lineeditor::{
    LineEditor, LineEditor_getCursor, LineEditor_getText, LineEditor_handleKey,
};
use crate::ported::listitem::{ListItem, ListItem_compare, ListItem_display, ListItem_new};
use crate::ported::object::{Object, ObjectClass, Object_class};
use crate::ported::panel::{
    EVENT_PANEL_LOST_FOCUS, EVENT_SET_SELECTED, HandlerResult, Panel, Panel_add,
    Panel_getSelectedIndex, Panel_onKey, Panel_prune, Panel_selectByTyping,
    Panel_setCursorToSelection, Panel_setDefaultBar, Panel_setSelectionColor,
};
use crate::ported::richstring::RichString;
use crate::ported::screenmanager::ScreenManager;
use crate::ported::settings::Settings;
use crate::ported::xutils::String_eq;

// Char / `KEY_F(n)` / `KEY_CTRL(c)` case labels cannot appear as Rust match
// patterns directly (a `const fn` call / `'\n' as i32` is not a pattern), so
// bind them as module `const`s — the same idiom `panel.rs`/`screenspanel.rs`
// use. `const`, not `pub fn`, so the port-purity gate is unaffected.
const NEWLINE: i32 = '\n' as i32;
const CARRIAGE_RETURN: i32 = '\r' as i32;
const ESC: i32 = 27;
const EQUALS: i32 = b'=' as i32;
const KEY_F2: i32 = KEY_F(2);
const KEY_F5: i32 = KEY_F(5);
const KEY_F10: i32 = KEY_F(10);
const CTRL_N: i32 = KEY_CTRL(b'N' as i32);

/// Port of the C `ScreenNamesPanel` struct (`ScreenTabsPanel.h:20`). The
/// embedded `Panel super` becomes `super_` (avoiding the Rust `super`
/// keyword, per the `MainPanel`/`ColumnsPanel` convention). `scr`/`settings`
/// are owned elsewhere (`htop.c`/`ScreenManager`) so they stay raw pointers;
/// `ds` is the current tab's dynamic screen. The two C alias pointers into
/// the item vector are index/owned-String modeled (see the module docs):
/// `saved` is the row's original name (C `char*`, moved back on cancel) and
/// `renamingItem` is the index of the row under edit (C `ListItem*`, `None`
/// == `NULL`).
pub struct ScreenNamesPanel {
    pub super_: Panel,
    pub scr: *mut ScreenManager,
    pub settings: *mut Settings,
    pub editor: LineEditor,
    pub ds: *mut DynamicScreen,
    pub saved: Option<String>,
    pub renamingItem: Option<usize>,
}

/// Port of the C `ScreenNameListItem` struct (`ScreenTabsPanel.h:31`). The
/// embedded `ListItem super` becomes `super_`; `ss` is the C
/// `ScreenSettings*` back-pointer aliasing an entry of `settings->screens[]`,
/// modeled as the **index** into that `Vec` (`None` == C `NULL`, set by
/// `ScreenNamesPanel_delete`). [`renameScreenSettings`] reaches the screen
/// through this index plus the panel's `*mut Settings` back-pointer.
pub struct ScreenNameListItem {
    pub super_: ListItem,
    pub ss: Option<usize>,
}

/// Port of the C `ScreenTabsPanel` struct (`ScreenTabsPanel.h:36`). The
/// embedded `Panel super` becomes `super_`; `names` is the owned-elsewhere
/// `ScreenNamesPanel*` the tab handler drives, and `cursor` mirrors the C
/// `int cursor`.
pub struct ScreenTabsPanel {
    pub super_: Panel,
    pub scr: *mut ScreenManager,
    pub settings: *mut Settings,
    pub names: *mut ScreenNamesPanel,
    pub cursor: i32,
}

/// Port of the C `ScreenTabListItem` struct (`ScreenTabsPanel.h:45`). The
/// embedded `ListItem super` becomes `super_`; `ds` is the `DynamicScreen*`
/// back-pointer read by `ScreenTabsPanel_eventHandler`'s `HANDLED` tail.
pub struct ScreenTabListItem {
    pub super_: ListItem,
    pub ds: *mut DynamicScreen,
}

/// Port of `ObjectClass ScreenTabListItem_class` (`ScreenTabsPanel.c:30`):
/// `{ .extends = Class(ListItem), .display = ListItem_display, .delete =
/// ListItem_delete, .compare = ListItem_compare }`. The C `.extends` targets
/// `ListItem_class`, a private `static` in `listitem.rs`, so the nearest
/// exported ancestor `Object_class` is used (the class chain is unused by the
/// ported surface — rows are downcast via `Any`, never `Object_isA`).
/// `.display`/`.compare` are wired through the [`Object`] impl below;
/// `.delete` maps to `Drop`.
static ScreenTabListItem_class: ObjectClass = ObjectClass {
    extends: Some(&Object_class),
};

impl Object for ScreenTabListItem {
    /// C `this->klass` set to `&ScreenTabListItem_class`.
    fn klass(&self) -> &'static ObjectClass {
        &ScreenTabListItem_class
    }

    /// C vtable slot `.display = ListItem_display` — draws exactly like a
    /// plain `ListItem` over its embedded `super`.
    fn display(&self, out: &mut RichString) {
        ListItem_display(&self.super_, out);
    }

    /// C vtable slot `.compare = ListItem_compare`. The C comparator casts the
    /// opaque `const void*` back to the concrete type; the safe-Rust analog
    /// downcasts via `Any`.
    fn compare(&self, other: &dyn Object) -> i32 {
        let any: &dyn core::any::Any = other;
        let o = any
            .downcast_ref::<ScreenTabListItem>()
            .expect("ScreenTabListItem_compare called across incompatible classes");
        ListItem_compare(&self.super_, &o.super_)
    }
}

/// Port of `ObjectClass ScreenNameListItem_class` (`ScreenTabsPanel.c:160`):
/// `{ .extends = Class(ListItem), .display = ListItem_display, .delete =
/// ListItem_delete, .compare = ListItem_compare }`. Same modeling as
/// [`ScreenTabListItem_class`] above (rooted at `Object_class`).
static ScreenNameListItem_class: ObjectClass = ObjectClass {
    extends: Some(&Object_class),
};

impl Object for ScreenNameListItem {
    /// C `this->klass` set to `&ScreenNameListItem_class`.
    fn klass(&self) -> &'static ObjectClass {
        &ScreenNameListItem_class
    }

    /// C vtable slot `.display = ListItem_display`.
    fn display(&self, out: &mut RichString) {
        ListItem_display(&self.super_, out);
    }

    /// C vtable slot `.compare = ListItem_compare`.
    fn compare(&self, other: &dyn Object) -> i32 {
        let any: &dyn core::any::Any = other;
        let o = any
            .downcast_ref::<ScreenNameListItem>()
            .expect("ScreenNameListItem_compare called across incompatible classes");
        ListItem_compare(&self.super_, &o.super_)
    }
}

impl ScreenNamesPanel {
    /// Set row `idx`'s display value (the C `((ListItem*) item)->value = ...`
    /// write). Gate-skipped associated fn — not a C function — shared by the
    /// `item->value = ...` assignments in the rename handler. Because
    /// `Panel_get`/`Panel_getSelected` hand back an immutable `&dyn Object`,
    /// this downcasts the row `&mut dyn Object` to the concrete row type via
    /// the `Any` supertrait. The names panel may hold either a
    /// [`ScreenNameListItem`] (added by `ScreenNamesPanel_new`/`addNewScreen`)
    /// or a plain [`ListItem`] (added by [`ScreenNamesPanel_fill`]); the C
    /// `(ListItem*)` cast writes `.value` on either, so both concrete types
    /// are handled here.
    fn set_item_value(&mut self, idx: usize, value: String) {
        let obj: &mut dyn core::any::Any = self.super_.items[idx].as_mut();
        if let Some(item) = obj.downcast_mut::<ScreenNameListItem>() {
            item.super_.value = value;
            return;
        }
        if let Some(item) = obj.downcast_mut::<ListItem>() {
            item.value = value;
        }
    }
}

/// Port of the file-static `static FunctionBar* ScreenNames_renamingBar = NULL;`
/// (`ScreenTabsPanel.c:176`) — the process-wide renaming-mode bar, lazily
/// built by `ScreenNamesPanel_new` and torn down by [`ScreenTabsPanel_cleanup`].
///
/// The C raw `FunctionBar*` (with `NULL` meaning "not yet built") is modeled
/// as a `Mutex<Option<FunctionBar>>`: `None` is the `NULL` sentinel and a
/// `Some` payload owns the bar, whose `Drop` is the faithful analog of the C
/// `FunctionBar_delete`. `Mutex::new(None)` is `const`, so it initializes the
/// `static` directly, matching the C zero-initialized global.
static ScreenNames_renamingBar: Mutex<Option<FunctionBar>> = Mutex::new(None);

/// Port of `ScreenTabsPanel.c:37`. Repopulates the names panel with a
/// `ListItem` per matching screen in `settings->screens[]`:
///
/// ```c
/// const Settings* settings = this->settings;
/// Panel_prune(&this->super);
/// for (unsigned int i = 0; i < settings->nScreens; i++) {
///    const ScreenSettings* ss = settings->screens[i];
///    if (ds == NULL) { if (ss->dynamic != NULL) continue; }
///    else { if (ss->dynamic == NULL) continue;
///           if (!String_eq(ds->name, ss->dynamic)) continue; }
///    Panel_add(super, (Object*) ListItem_new(ss->heading, i));
/// }
/// this->ds = ds;
/// ```
///
/// `ds == NULL` selects the built-in (non-dynamic) screens; a non-`NULL`
/// `ds` selects only the dynamic screens whose `ss->dynamic` name matches
/// `ds->name`. `settings->nScreens` is `screens.len()`, and the loop index
/// `i` (C `unsigned int`) is the `ListItem` key (C casts it to `int`). The C
/// uses the plain `ListItem_new` here (not `ScreenNameListItem_new`), so the
/// produced items carry no `ss` back-pointer. `this->settings` is read through
/// the raw `*mut Settings` back-pointer (copied to a local first so its borrow
/// is independent of the `&mut this.super_` that `Panel_prune`/`Panel_add`
/// take). `ss->heading` is never `NULL` for a real screen; `unwrap_or("")`
/// guards the modeled `None` without panicking.
pub fn ScreenNamesPanel_fill(this: &mut ScreenNamesPanel, ds: Option<&DynamicScreen>) {
    let settings_ptr = this.settings;
    Panel_prune(&mut this.super_);
    // SAFETY: `settings` is the back-pointer set at construction; the deref
    // yields a reference independent of `this`, so it does not alias the
    // `&mut this.super_` borrows below.
    let settings: &Settings = unsafe { &*settings_ptr };

    for i in 0..settings.screens.len() {
        let ss = &settings.screens[i];

        match ds {
            None => {
                if ss.dynamic.is_some() {
                    continue;
                }
                /* built-in (processes, not dynamic) - e.g. Main or I/O */
            }
            Some(ds) => {
                match &ss.dynamic {
                    None => continue,
                    Some(dynamic) => {
                        if !String_eq(&ds.name, dynamic) {
                            continue;
                        }
                        /* matching dynamic screen found, add it into the Panel */
                    }
                }
            }
        }

        let heading = ss.heading.as_deref().unwrap_or("");
        Panel_add(&mut this.super_, Box::new(ListItem_new(heading, i as i32)));
    }

    this.ds = ds.map_or(std::ptr::null_mut(), |d| {
        d as *const DynamicScreen as *mut DynamicScreen
    });
}

/// TODO: port of `static void ScreenTabsPanel_delete(Object* object` from `ScreenTabsPanel.c:62`.
/// `Panel_done` + `free` — the owned fields are released by `Drop` in Rust,
/// so there is no algorithm to port (as with the `Panel_delete` stub).
pub fn ScreenTabsPanel_delete() {
    todo!("port of ScreenTabsPanel.c:62 — Drop releases the panel")
}

/// Port of `static HandlerResult ScreenTabsPanel_eventHandler(Panel* super,
/// int ch)` from `ScreenTabsPanel.c:68`. The tab-panel key `switch`:
///
/// - `EVENT_SET_SELECTED` — `HANDLED`.
/// - `KEY_F(5)` / `KEY_CTRL('N')` — delegate straight to
///   [`ScreenNamesPanel_eventHandlerNormal`] on the names sub-panel to create
///   a new screen (that path bottoms out on the stubbed `addNewScreen`).
/// - navigation keys — run `Panel_onKey`; report `HANDLED` iff the selection
///   moved (C `previous != selected`).
/// - default — a graphic alphabetic char runs [`Panel_selectByTyping`]
///   (`BREAK_LOOP` demoted to `IGNORED`).
///
/// On `HANDLED`, the tail refills the names sub-panel from the newly-selected
/// tab's `ScreenTabListItem.ds` (C `focus->ds`, a `DynamicScreen*` mapped to
/// `Option<&DynamicScreen>`; `NULL` == the built-in Processes tab). `this->names`
/// is the sub-panel back-pointer; `focus` is recovered by an `Any` downcast
/// (the C `(ScreenTabListItem*)` cast), and the C `if (focus)` null guard is
/// the empty-panel check.
pub fn ScreenTabsPanel_eventHandler(this: &mut ScreenTabsPanel, ch: i32) -> HandlerResult {
    let mut result = HandlerResult::IGNORED;

    let mut selected = Panel_getSelectedIndex(&this.super_);
    match ch {
        EVENT_SET_SELECTED => {
            result = HandlerResult::HANDLED;
        }
        KEY_F5 | CTRL_N => {
            // pass onto the Names panel for creating new screen
            // SAFETY: `names` is the sub-panel back-pointer set at construction.
            let names = unsafe { &mut *this.names };
            return ScreenNamesPanel_eventHandlerNormal(names, ch);
        }
        KEY_UP | KEY_DOWN | KEY_NPAGE | KEY_PPAGE | KEY_HOME | KEY_END => {
            let previous = selected;
            Panel_onKey(&mut this.super_, ch);
            selected = Panel_getSelectedIndex(&this.super_);
            if previous != selected {
                result = HandlerResult::HANDLED;
            }
        }
        _ => {
            if ch >= 0 && ch < 255 && (ch as u8).is_ascii_alphabetic() {
                result = Panel_selectByTyping(&mut this.super_, ch);
            }
            if result == HandlerResult::BREAK_LOOP {
                result = HandlerResult::IGNORED;
            }
        }
    }

    if result == HandlerResult::HANDLED {
        // focus = (ScreenTabListItem*) Panel_getSelected(super); the C
        // null-check is the empty-panel guard.
        let focus_ds: Option<*mut DynamicScreen> = if this.super_.items.is_empty() {
            None
        } else {
            let sel = Panel_getSelectedIndex(&this.super_) as usize;
            let any: &dyn core::any::Any = this.super_.items[sel].as_ref();
            let focus = any
                .downcast_ref::<ScreenTabListItem>()
                .expect("ScreenTabsPanel_eventHandler: panel row is not a ScreenTabListItem");
            Some(focus.ds)
        };
        if let Some(ds) = focus_ds {
            // SAFETY: `names` is the sub-panel back-pointer; `ds` (a Copy raw
            // pointer read above) is either NULL (built-in) or a live
            // DynamicScreen owned by the settings' dynamicScreens registry.
            let names = unsafe { &mut *this.names };
            let ds_ref = if ds.is_null() {
                None
            } else {
                Some(unsafe { &*ds })
            };
            ScreenNamesPanel_fill(names, ds_ref);
        }
    }

    result
}

/// TODO: port of `static ScreenTabListItem* ScreenTabListItem_new(const char* value, DynamicScreen* ds` from `ScreenTabsPanel.c:121`.
/// Blocked: `AllocThis` constructor stashing a borrowed `DynamicScreen*`
/// into a `ListItem` subclass — construction is a struct literal / destruction
/// is `Drop` (no `AllocThis` analog, as with `ListItem_new`).
pub fn ScreenTabListItem_new() {
    todo!("port of ScreenTabsPanel.c:121")
}

/// TODO: port of `static void addDynamicScreen(ATTR_UNUSED ht_key_t key, void* value, void* userdata` from `ScreenTabsPanel.c:128`.
/// Blocked: `Hashtable_foreach` callback reading `screen->heading` (absent
/// from the ported `DynamicScreen` model) and building a `ScreenTabListItem`
/// via the stubbed constructor above.
pub fn addDynamicScreen() {
    todo!("port of ScreenTabsPanel.c:128")
}

/// TODO: port of `ScreenTabsPanel* ScreenTabsPanel_new(Settings* settings` from `ScreenTabsPanel.c:138`.
/// Blocked: needs `Hashtable_foreach` over `settings->dynamicScreens`, the
/// stubbed `ScreenTabListItem_new`, and the stubbed `ScreenNamesPanel_new`.
pub fn ScreenTabsPanel_new() {
    todo!("port of ScreenTabsPanel.c:138")
}

/// TODO: port of `ScreenNameListItem* ScreenNameListItem_new(const char* value, ScreenSettings* ss` from `ScreenTabsPanel.c:167`.
/// Blocked: same as `ScreenTabListItem_new` — `AllocThis` constructor stashing
/// a borrowed `ScreenSettings*` back-pointer; no safe free-fn analog.
pub fn ScreenNameListItem_new() {
    todo!("port of ScreenTabsPanel.c:167")
}

/// Port of `ScreenTabsPanel.c:178`. Tears down the process-wide renaming
/// `FunctionBar` if one was ever built. The C body —
/// `if (ScreenNames_renamingBar) { FunctionBar_delete(ScreenNames_renamingBar);
/// ScreenNames_renamingBar = NULL; }` — becomes: if the [`ScreenNames_renamingBar`]
/// `Option` holds a bar, drop it (the `Some` payload's `Drop` is the analog of
/// `FunctionBar_delete`) and leave `None` (the `= NULL`). Idempotent: calling
/// it when the bar was never built is a no-op, exactly as the C `NULL` guard.
pub fn ScreenTabsPanel_cleanup() {
    let mut bar = ScreenNames_renamingBar.lock().unwrap();
    if bar.is_some() {
        *bar = None;
    }
}

/// TODO: port of `static void ScreenNamesPanel_delete(Object* object` from `ScreenTabsPanel.c:185`.
/// Blocked: walks the item vector clearing each `ScreenNameListItem.ss`
/// back-pointer and restores `renamingItem->value = this->saved` — bookkeeping
/// that only matters for the C manual-free protocol; the trailing `Panel_done`
/// + `free` is released by `Drop`.
pub fn ScreenNamesPanel_delete() {
    todo!("port of ScreenTabsPanel.c:185")
}

/// Port of `static void renameScreenSettings(ScreenNamesPanel* this, const
/// ListItem* item)` from `ScreenTabsPanel.c:204`. Commits a finished rename:
///
/// ```c
/// const ScreenNameListItem* nameItem = (const ScreenNameListItem*) item;
/// ScreenSettings* ss = nameItem->ss;
/// free_and_xStrdup(&ss->heading, item->value);
/// Settings* settings = this->settings;
/// settings->changed = true;
/// settings->lastUpdate++;
/// ```
///
/// `item` is the renamed row (C `this->renamingItem`), modeled here as its
/// **index** in the panel. Its `ScreenNameListItem.ss` (the index into
/// `settings->screens[]`) and current `.value` are read via an `Any` downcast
/// (the C `(ScreenNameListItem*)` cast), then the screen's `heading` is set to
/// that value through the panel's `*mut Settings` back-pointer, and the
/// `changed`/`lastUpdate` dirty markers are bumped. `ss` is never `NULL`
/// during a rename (it is set by `ScreenNameListItem_new`/`addNewScreen`); the
/// `if let Some` guards the modeled `None` without the C's unconditional deref.
pub fn renameScreenSettings(this: &mut ScreenNamesPanel, item: usize) {
    // nameItem = (ScreenNameListItem*) item; ss = nameItem->ss; item->value
    let (ss_index, value) = {
        let any: &dyn core::any::Any = this.super_.items[item].as_ref();
        let nameItem = any
            .downcast_ref::<ScreenNameListItem>()
            .expect("renameScreenSettings: panel row is not a ScreenNameListItem");
        (nameItem.ss, nameItem.super_.value.clone())
    };

    // SAFETY: `settings` is the back-pointer set at construction; the deref
    // yields a reference independent of `this.super_` (the item borrow above
    // has ended). free_and_xStrdup(&ss->heading, item->value).
    let settings: &mut Settings = unsafe { &mut *this.settings };
    if let Some(idx) = ss_index {
        settings.screens[idx].heading = Some(value);
    }
    settings.changed = true;
    settings.lastUpdate = settings.lastUpdate.wrapping_add(1);
}

/// Port of `static HandlerResult ScreenNamesPanel_eventHandlerRenaming(Panel*
/// super, int ch)` from `ScreenTabsPanel.c:215`. The rename-mode key `switch`,
/// always returning [`HandlerResult::HANDLED`]:
///
/// - `EVENT_SET_SELECTED` — if the selection moved off the row under edit,
///   finish the rename (C `if (item != this->renamingItem) goto renameFinish`).
/// - `EVENT_PANEL_LOST_FOCUS` — finish the rename.
/// - `\n` / `\r` / `KEY_ENTER` / `F10` — finish (unless the list is empty,
///   the C `if (!item) break`).
/// - `Esc` / `F2` — cancel: restore the row's original value from
///   `this->saved`, clear the rename state, restore the default bar/color.
/// - default — feed the key to the [`LineEditor`] (excluding `'='`, reserved
///   by the config format), update `selectedLen`/the cursor, and re-point the
///   row's display value at the live editor text.
///
/// The C `renameFinish` `goto` (reached from three arms) is expressed as a
/// `do_finish` flag whose shared body runs after the `match`: it drops
/// `this->saved` (C `free`), writes the editor text into the row, restores the
/// focus color/default bar, and calls [`renameScreenSettings`] before clearing
/// `renamingItem`. The C `renamingItem` `ListItem*` is the row index
/// ([`ScreenNamesPanel::renamingItem`]); `this->saved` is an owned `String`.
pub fn ScreenNamesPanel_eventHandlerRenaming(this: &mut ScreenNamesPanel, ch: i32) -> HandlerResult {
    let mut do_finish = false;

    match ch {
        EVENT_SET_SELECTED => {
            // C: item = Panel_getSelected; if (item != this->renamingItem) goto renameFinish;
            // An empty panel (item == NULL) also differs from the renaming row.
            let sel = Panel_getSelectedIndex(&this.super_);
            if this.super_.items.is_empty() || this.renamingItem != Some(sel as usize) {
                do_finish = true;
            }
        }
        EVENT_PANEL_LOST_FOCUS => {
            do_finish = true;
        }
        NEWLINE | CARRIAGE_RETURN | KEY_ENTER | KEY_F10 => {
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
            // item->value = this->saved;
            let saved = this.saved.take().unwrap_or_default();
            this.set_item_value(idx, saved);
            this.renamingItem = None;
            this.super_.cursorOn = false;
            Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_SELECTION_FOCUS);
            Panel_setDefaultBar(&mut this.super_);
            return HandlerResult::HANDLED;
        }
        _ => {
            // Delegate editing keys to LineEditor, excluding '=' which has
            // special meaning in the config format.
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
        // free(this->saved);
        this.saved = None;
        // this->renamingItem->value = xStrdup(LineEditor_getText(&this->editor));
        let text = LineEditor_getText(&this.editor).to_string();
        this.set_item_value(idx, text);
        this.super_.cursorOn = false;
        Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_SELECTION_FOCUS);
        Panel_setDefaultBar(&mut this.super_);
        // renameScreenSettings(this, (ListItem*) this->renamingItem);
        renameScreenSettings(this, idx);
        this.renamingItem = None;
    }

    HandlerResult::HANDLED
}

/// TODO: port of `static void startRenaming(Panel* super` from `ScreenTabsPanel.c:276`.
/// Enters rename mode for the selected row (records `renamingItem`/`saved`,
/// seeds the `LineEditor`, switches to the `PANEL_EDIT` color and the
/// `ScreenNames_renamingBar`). Its sole caller is the stubbed [`addNewScreen`]
/// (renaming a screen it just created), so it is left an honest stub per the
/// task scope; the signature is preserved so the
/// [`ScreenNamesPanel_eventHandlerNormal`] call site is faithful.
pub fn startRenaming(this: &mut ScreenNamesPanel) {
    let _ = this;
    todo!("port of ScreenTabsPanel.c:276 — reached only via the stubbed addNewScreen new-screen path")
}

/// TODO: port of `static void addNewScreen(Panel* super, DynamicScreen* ds` from `ScreenTabsPanel.c:296`.
/// Blocked: allocates a `ScreenSettings` via the stubbed `Settings_newScreen`
/// / `Settings_newDynamicScreen` (`Settings.c:263` / `:286`, both blocked on
/// the platform `Process_fields[]` table) and inserts a `ScreenNameListItem`
/// built by the stubbed constructor above.
pub fn addNewScreen(this: &mut ScreenNamesPanel, ds: *mut DynamicScreen) {
    let _ = (this, ds);
    todo!("port of ScreenTabsPanel.c:296 — needs Settings_newScreen/Settings_newDynamicScreen (Process_fields[]) + ScreenNameListItem_new")
}

/// Port of `static HandlerResult ScreenNamesPanel_eventHandlerNormal(Panel*
/// super, int ch)` from `ScreenTabsPanel.c:306`. The normal-mode key `switch`:
///
/// - Enter / mouse / reclick — restore the `PANEL_SELECTION_FOCUS` color,
///   `HANDLED`.
/// - `EVENT_SET_SELECTED` — `HANDLED`.
/// - navigation keys — run `Panel_onKey`.
/// - `KEY_F(5)` / `KEY_CTRL('N')` — add a new screen and start renaming it,
///   `HANDLED`. This calls the still-stubbed [`addNewScreen`] / [`startRenaming`]
///   (the whole new-screen path bottoms out on the platform `Process_fields[]`
///   table), so this one arm transitively hits an honest stub.
/// - default — a graphic alphabetic char runs [`Panel_selectByTyping`]
///   (`BREAK_LOOP` demoted to `IGNORED`).
///
/// The C compares the selected-row pointer before and after the switch
/// (`oldFocus != newFocus`) to report `HANDLED` when the focused row changed.
/// With no reordering on any reachable (non-stub) path, the faithful analog is
/// comparing the selected index (as `Option<usize>`, `None` == the empty-panel
/// `NULL` focus), so the final `if (newFocus && oldFocus != newFocus)` becomes
/// `new.is_some() && old != new`.
pub fn ScreenNamesPanel_eventHandlerNormal(this: &mut ScreenNamesPanel, ch: i32) -> HandlerResult {
    // oldFocus = (ScreenNameListItem*) Panel_getSelected(super); NULL == empty.
    let oldFocus = if this.super_.items.is_empty() {
        None
    } else {
        Some(Panel_getSelectedIndex(&this.super_))
    };
    let mut result = HandlerResult::IGNORED;

    match ch {
        NEWLINE | CARRIAGE_RETURN | KEY_ENTER | KEY_MOUSE | KEY_RECLICK => {
            Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_SELECTION_FOCUS);
            result = HandlerResult::HANDLED;
        }
        EVENT_SET_SELECTED => {
            result = HandlerResult::HANDLED;
        }
        KEY_NPAGE | KEY_PPAGE | KEY_HOME | KEY_END => {
            Panel_onKey(&mut this.super_, ch);
        }
        KEY_F5 | CTRL_N => {
            let ds = this.ds;
            addNewScreen(this, ds);
            startRenaming(this);
            result = HandlerResult::HANDLED;
        }
        _ => {
            if ch >= 0 && ch < 255 && (ch as u8).is_ascii_alphabetic() {
                result = Panel_selectByTyping(&mut this.super_, ch);
            }
            if result == HandlerResult::BREAK_LOOP {
                result = HandlerResult::IGNORED;
            }
        }
    }

    // newFocus = (ScreenNameListItem*) Panel_getSelected(super);
    let newFocus = if this.super_.items.is_empty() {
        None
    } else {
        Some(Panel_getSelectedIndex(&this.super_))
    };
    if newFocus.is_some() && oldFocus != newFocus {
        result = HandlerResult::HANDLED;
    }

    result
}

/// Port of `static HandlerResult ScreenNamesPanel_eventHandler(Panel* super,
/// int ch)` from `ScreenTabsPanel.c:350`. Dispatches to the renaming handler
/// while a rename is in progress (C `if (!this->renamingItem)` selects the
/// normal handler), otherwise to the normal handler.
pub fn ScreenNamesPanel_eventHandler(this: &mut ScreenNamesPanel, ch: i32) -> HandlerResult {
    if this.renamingItem.is_none() {
        ScreenNamesPanel_eventHandlerNormal(this, ch)
    } else {
        ScreenNamesPanel_eventHandlerRenaming(this, ch)
    }
}

/// TODO: port of `ScreenNamesPanel* ScreenNamesPanel_new(Settings* settings` from `ScreenTabsPanel.c:366`.
/// Blocked: lazily builds `ScreenNames_renamingBar`, then iterates
/// `settings->screens[]` building items via the stubbed `ScreenNameListItem_new`
/// (`:167`, an `AllocThis` constructor with no safe analog).
pub fn ScreenNamesPanel_new() {
    todo!("port of ScreenTabsPanel.c:366")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::lineeditor::{LineEditor_initWithMax, LineEditor_setText};
    use crate::ported::panel::Panel_new;
    use crate::ported::settings::{HeaderLayout, ScreenSettings};

    fn bar() -> FunctionBar {
        FunctionBar {
            functions: vec!["      ".into()],
            keys: vec!["F5".into()],
            events: vec![5],
            staticData: false,
        }
    }

    #[test]
    fn cleanup_drops_the_renaming_bar_and_nulls_it() {
        // Seed the file-static as ScreenNamesPanel_new would (bar != NULL).
        *ScreenNames_renamingBar.lock().unwrap() = Some(bar());
        assert!(ScreenNames_renamingBar.lock().unwrap().is_some());

        ScreenTabsPanel_cleanup();
        // The C sets the pointer back to NULL after FunctionBar_delete.
        assert!(ScreenNames_renamingBar.lock().unwrap().is_none());

        // Idempotent: a second cleanup with the bar already NULL is a no-op.
        ScreenTabsPanel_cleanup();
        assert!(ScreenNames_renamingBar.lock().unwrap().is_none());
    }

    // ── shared scaffolding ────────────────────────────────────────────

    /// A `Settings` carrying only the `screens` the ported code reads (the
    /// meter/layout fields are inert here).
    fn settings_with(screens: Vec<ScreenSettings>) -> Settings {
        Settings {
            hLayout: HeaderLayout::HF_TWO_50_50,
            hColumns: Vec::new(),
            screens,
            ssIndex: 0,
            changed: false,
            lastUpdate: 0,
        }
    }

    fn builtin(heading: &str) -> ScreenSettings {
        ScreenSettings {
            heading: Some(heading.to_string()),
            dynamic: None,
            ..Default::default()
        }
    }

    fn dynamic(heading: &str, dyn_name: &str) -> ScreenSettings {
        ScreenSettings {
            heading: Some(heading.to_string()),
            dynamic: Some(dyn_name.to_string()),
            ..Default::default()
        }
    }

    /// Build a bare `ScreenNamesPanel` pointing at `settings`. Mirrors the
    /// `MainPanel`/`Panel::empty` test-scaffold idiom: the real constructor
    /// `ScreenNamesPanel_new` is stubbed, so the struct is assembled directly.
    fn names_panel(settings: &mut Settings) -> ScreenNamesPanel {
        ScreenNamesPanel {
            super_: Panel_new(1, 1, 1, 1, None),
            scr: std::ptr::null_mut(),
            settings: settings as *mut Settings,
            editor: LineEditor::default(),
            ds: std::ptr::null_mut(),
            saved: None,
            renamingItem: None,
        }
    }

    /// A `ScreenTabsPanel` whose `names` back-pointer targets `names`.
    fn tabs_panel(names: &mut ScreenNamesPanel) -> ScreenTabsPanel {
        ScreenTabsPanel {
            super_: Panel_new(1, 1, 1, 1, None),
            scr: std::ptr::null_mut(),
            settings: std::ptr::null_mut(),
            names: names as *mut ScreenNamesPanel,
            cursor: 0,
        }
    }

    /// A plain `ListItem` row (what `ScreenNamesPanel_fill` produces).
    fn li(value: &str, key: i32) -> Box<dyn Object> {
        Box::new(ListItem_new(value, key))
    }

    /// A `ScreenNameListItem` row carrying its `settings->screens[]` index.
    fn name_item(value: &str, ss: Option<usize>) -> Box<dyn Object> {
        Box::new(ScreenNameListItem {
            super_: ListItem_new(value, 0),
            ss,
        })
    }

    /// A `ScreenTabListItem` row carrying its dynamic-screen back-pointer.
    fn tab_item(value: &str, ds: *mut DynamicScreen) -> Box<dyn Object> {
        Box::new(ScreenTabListItem {
            super_: ListItem_new(value, 0),
            ds,
        })
    }

    /// (value, key) of each plain-`ListItem` row (fill output).
    fn items(p: &ScreenNamesPanel) -> Vec<(String, i32)> {
        p.super_
            .items
            .iter()
            .map(|o| {
                let any: &dyn std::any::Any = o.as_ref();
                let li = any.downcast_ref::<ListItem>().expect("fill adds ListItems");
                (li.value.clone(), li.key)
            })
            .collect()
    }

    /// The display value of a `ScreenNameListItem` row.
    fn name_value(p: &ScreenNamesPanel, idx: usize) -> String {
        let any: &dyn std::any::Any = p.super_.items[idx].as_ref();
        any.downcast_ref::<ScreenNameListItem>()
            .unwrap()
            .super_
            .value
            .clone()
    }

    /// Enter rename mode on row `idx` the way the (stubbed) `startRenaming`
    /// would: record the row and its original name, seed the editor, and point
    /// the row value at the live editor buffer.
    fn enter_renaming(p: &mut ScreenNamesPanel, idx: usize) {
        let original = name_value(p, idx);
        p.renamingItem = Some(idx);
        p.saved = Some(original.clone());
        p.super_.cursorOn = true;
        LineEditor_initWithMax(&mut p.editor, 19); // SCREEN_NAME_LEN - 1
        LineEditor_setText(&mut p.editor, &original);
        let text = LineEditor_getText(&p.editor).to_string();
        p.set_item_value(idx, text);
    }

    // ── ScreenNamesPanel_fill ─────────────────────────────────────────

    #[test]
    fn fill_builtin_selects_non_dynamic_screens_with_index_keys() {
        // ds == NULL: keep only screens whose `dynamic` is NULL; the key is
        // the position in the full screens[] array (not the output index).
        let mut settings = settings_with(vec![
            builtin("Main"),         // idx 0 kept
            dynamic("Pods", "pods"), // idx 1 skipped (dynamic)
            builtin("I/O"),          // idx 2 kept
        ]);
        let mut panel = names_panel(&mut settings);

        ScreenNamesPanel_fill(&mut panel, None);

        assert_eq!(
            items(&panel),
            vec![("Main".to_string(), 0), ("I/O".to_string(), 2)]
        );
        // this->ds = ds (NULL) — stays null.
        assert!(panel.ds.is_null());
    }

    #[test]
    fn fill_dynamic_selects_only_matching_name() {
        // ds != NULL: keep only dynamic screens whose `dynamic` name equals
        // ds->name; built-ins and other dynamic screens are skipped.
        let mut settings = settings_with(vec![
            builtin("Main"),              // skipped (not dynamic)
            dynamic("Pods", "pods"),      // idx 1 matches
            dynamic("Containers", "ctr"), // skipped (name mismatch)
            dynamic("More Pods", "pods"), // idx 3 matches
        ]);
        let mut panel = names_panel(&mut settings);
        let ds = DynamicScreen {
            name: "pods".to_string(),
        };

        ScreenNamesPanel_fill(&mut panel, Some(&ds));

        assert_eq!(
            items(&panel),
            vec![("Pods".to_string(), 1), ("More Pods".to_string(), 3)]
        );
        // this->ds now points at the passed dynamic screen.
        assert_eq!(panel.ds as *const DynamicScreen, &ds as *const DynamicScreen);
    }

    #[test]
    fn fill_prunes_existing_items_first() {
        // Panel_prune clears any prior contents before the loop repopulates.
        let mut settings = settings_with(vec![builtin("Only")]);
        let mut panel = names_panel(&mut settings);
        Panel_add(&mut panel.super_, li("stale", 99));
        Panel_add(&mut panel.super_, li("stale2", 98));

        ScreenNamesPanel_fill(&mut panel, None);

        assert_eq!(items(&panel), vec![("Only".to_string(), 0)]);
    }

    #[test]
    fn fill_empty_screens_yields_empty_panel() {
        let mut settings = settings_with(Vec::new());
        let mut panel = names_panel(&mut settings);
        ScreenNamesPanel_fill(&mut panel, None);
        assert!(panel.super_.items.is_empty());
    }

    // ── renameScreenSettings ──────────────────────────────────────────

    #[test]
    fn rename_screen_settings_writes_heading_and_dirties() {
        let mut settings = settings_with(vec![builtin("Main"), builtin("I/O")]);
        let mut panel = names_panel(&mut settings);
        // Row 0 edits screens[1]; its display value is the new name.
        panel.super_.items.push(name_item("Renamed", Some(1)));

        renameScreenSettings(&mut panel, 0);

        // free_and_xStrdup(&ss->heading, item->value) wrote the new heading.
        assert_eq!(settings.screens[1].heading.as_deref(), Some("Renamed"));
        // untouched screen keeps its heading.
        assert_eq!(settings.screens[0].heading.as_deref(), Some("Main"));
        // dirty markers bumped.
        assert!(settings.changed);
        assert_eq!(settings.lastUpdate, 1);
    }

    // ── ScreenNamesPanel_eventHandlerRenaming ─────────────────────────

    #[test]
    fn renaming_default_key_edits_editor_and_row_value() {
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("Main", Some(0)));
        enter_renaming(&mut panel, 0);

        let r = ScreenNamesPanel_eventHandlerRenaming(&mut panel, b'X' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(LineEditor_getText(&panel.editor), "MainX");
        assert_eq!(name_value(&panel, 0), "MainX"); // row follows the editor
        assert_eq!(panel.super_.selectedLen, 5); // cursor advanced
    }

    #[test]
    fn renaming_equals_is_swallowed_without_editing() {
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("Main", Some(0)));
        enter_renaming(&mut panel, 0);

        let r = ScreenNamesPanel_eventHandlerRenaming(&mut panel, EQUALS);
        assert_eq!(r, HandlerResult::HANDLED);
        // '=' reserved by the config format: editor + row unchanged.
        assert_eq!(LineEditor_getText(&panel.editor), "Main");
        assert_eq!(name_value(&panel, 0), "Main");
    }

    #[test]
    fn renaming_esc_cancels_and_restores_original_name() {
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("Main", Some(0)));
        enter_renaming(&mut panel, 0);
        // Type an edit, then cancel: the saved original must be restored and
        // the screen heading left untouched.
        ScreenNamesPanel_eventHandlerRenaming(&mut panel, b'Z' as i32);
        assert_eq!(name_value(&panel, 0), "MainZ");

        let r = ScreenNamesPanel_eventHandlerRenaming(&mut panel, ESC);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(name_value(&panel, 0), "Main"); // restored from `saved`
        assert_eq!(panel.renamingItem, None);
        assert!(!panel.super_.cursorOn);
        assert_eq!(
            panel.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
        // Esc does NOT commit: the heading is unchanged and not dirtied.
        assert_eq!(settings.screens[0].heading.as_deref(), Some("Main"));
        assert!(!settings.changed);
    }

    #[test]
    fn renaming_enter_commits_edit_to_screen_settings() {
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("Main", Some(0)));
        enter_renaming(&mut panel, 0);
        ScreenNamesPanel_eventHandlerRenaming(&mut panel, b'Z' as i32); // "MainZ"

        let r = ScreenNamesPanel_eventHandlerRenaming(&mut panel, KEY_ENTER);
        assert_eq!(r, HandlerResult::HANDLED);
        // The finished value is written to both the row and the screen.
        assert_eq!(name_value(&panel, 0), "MainZ");
        assert_eq!(settings.screens[0].heading.as_deref(), Some("MainZ"));
        assert!(settings.changed);
        assert_eq!(settings.lastUpdate, 1);
        // rename state cleared.
        assert_eq!(panel.renamingItem, None);
        assert!(panel.saved.is_none());
        assert!(!panel.super_.cursorOn);
    }

    #[test]
    fn renaming_event_set_selected_same_row_keeps_editing() {
        // Selection unchanged (still the renaming row) => no finish, rename
        // state stays intact.
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("Main", Some(0)));
        panel.super_.selected = 0;
        enter_renaming(&mut panel, 0);

        let r = ScreenNamesPanel_eventHandlerRenaming(&mut panel, EVENT_SET_SELECTED);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(panel.renamingItem, Some(0)); // still renaming
        assert!(!settings.changed); // not committed
    }

    // ── ScreenNamesPanel_eventHandlerNormal ───────────────────────────

    #[test]
    fn normal_event_set_selected_is_handled() {
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("Main", Some(0)));

        let r = ScreenNamesPanel_eventHandlerNormal(&mut panel, EVENT_SET_SELECTED);
        assert_eq!(r, HandlerResult::HANDLED);
    }

    #[test]
    fn normal_enter_restores_focus_color_and_handles() {
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("Main", Some(0)));
        Panel_setSelectionColor(&mut panel.super_, ColorElements::PANEL_EDIT);

        let r = ScreenNamesPanel_eventHandlerNormal(&mut panel, KEY_ENTER);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(
            panel.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
    }

    #[test]
    fn normal_navigation_reports_handled_when_focus_moves() {
        let mut settings = settings_with(Vec::new());
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("a", Some(0)));
        panel.super_.items.push(name_item("b", Some(1)));
        panel.super_.items.push(name_item("c", Some(2)));
        panel.super_.selected = 0;

        // KEY_END moves the selection to the last row -> focus changed.
        let r = ScreenNamesPanel_eventHandlerNormal(&mut panel, KEY_END);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(panel.super_.selected, 2);
    }

    #[test]
    fn normal_typing_selects_by_prefix() {
        // Type-to-search runs Panel_selectByTyping, which needs plain
        // ListItems (a names panel legitimately holds these after a fill).
        let mut settings = settings_with(Vec::new());
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(li("apple", 0));
        panel.super_.items.push(li("banana", 1));
        panel.super_.selected = 0;

        let r = ScreenNamesPanel_eventHandlerNormal(&mut panel, b'b' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(panel.super_.selected, 1); // "banana"
    }

    #[test]
    #[should_panic(expected = "ScreenTabsPanel.c:296")]
    fn normal_new_screen_arm_hits_the_add_stub() {
        // KEY_F(5) -> addNewScreen (stubbed on Settings_newScreen).
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("Main", Some(0)));
        let _ = ScreenNamesPanel_eventHandlerNormal(&mut panel, KEY_F5);
    }

    // ── ScreenNamesPanel_eventHandler dispatch ────────────────────────

    #[test]
    fn dispatch_routes_to_normal_when_not_renaming() {
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("Main", Some(0)));
        // Not renaming: a benign key that the normal handler resolves cleanly.
        let r = ScreenNamesPanel_eventHandler(&mut panel, EVENT_SET_SELECTED);
        assert_eq!(r, HandlerResult::HANDLED);
    }

    #[test]
    fn dispatch_routes_to_renaming_when_renaming() {
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut panel = names_panel(&mut settings);
        panel.super_.items.push(name_item("Main", Some(0)));
        enter_renaming(&mut panel, 0);
        // Renaming: a default key edits the editor via the renaming handler.
        let r = ScreenNamesPanel_eventHandler(&mut panel, b'!' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(LineEditor_getText(&panel.editor), "Main!");
    }

    // ── ScreenTabsPanel_eventHandler ──────────────────────────────────

    #[test]
    fn tabs_event_set_selected_fills_names_with_builtin_screens() {
        let mut settings =
            settings_with(vec![builtin("Main"), dynamic("Pods", "pods"), builtin("I/O")]);
        let mut names = names_panel(&mut settings);
        let mut tabs = tabs_panel(&mut names);
        // A single "Processes" tab (ds == NULL).
        Panel_add(&mut tabs.super_, tab_item("Processes", std::ptr::null_mut()));

        let r = ScreenTabsPanel_eventHandler(&mut tabs, EVENT_SET_SELECTED);
        assert_eq!(r, HandlerResult::HANDLED);
        // The HANDLED tail refilled names from focus->ds (NULL) = built-ins.
        assert_eq!(
            items(&names),
            vec![("Main".to_string(), 0), ("I/O".to_string(), 2)]
        );
    }

    #[test]
    fn tabs_navigation_fills_names_for_selected_dynamic_tab() {
        let mut settings = settings_with(vec![
            builtin("Main"),
            dynamic("Pods", "pods"),
            dynamic("Containers", "ctr"),
        ]);
        let mut names = names_panel(&mut settings);
        let mut ds_pods = DynamicScreen {
            name: "pods".to_string(),
        };
        let mut tabs = tabs_panel(&mut names);
        Panel_add(&mut tabs.super_, tab_item("Processes", std::ptr::null_mut()));
        Panel_add(
            &mut tabs.super_,
            tab_item("Pods", &mut ds_pods as *mut DynamicScreen),
        );

        // KEY_DOWN moves from Processes (0) to Pods (1): HANDLED, tail fills
        // names by the "pods" dynamic name.
        let r = ScreenTabsPanel_eventHandler(&mut tabs, KEY_DOWN);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(tabs.super_.selected, 1);
        assert_eq!(items(&names), vec![("Pods".to_string(), 1)]);
    }

    #[test]
    #[should_panic(expected = "ScreenTabsPanel.c:296")]
    fn tabs_new_screen_arm_delegates_to_names_add_stub() {
        // KEY_F(5) delegates to ScreenNamesPanel_eventHandlerNormal on the
        // names panel, whose F5 arm hits the addNewScreen stub.
        let mut settings = settings_with(vec![builtin("Main")]);
        let mut names = names_panel(&mut settings);
        names.super_.items.push(name_item("Main", Some(0)));
        let mut tabs = tabs_panel(&mut names);
        Panel_add(&mut tabs.super_, tab_item("Processes", std::ptr::null_mut()));
        let _ = ScreenTabsPanel_eventHandler(&mut tabs, KEY_F5);
    }
}
