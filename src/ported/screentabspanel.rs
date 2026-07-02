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
//! Ported (self-contained, no unported substrate):
//! - `ScreenTabsPanel_cleanup` (`ScreenTabsPanel.c:178`) — tears down the
//!   process-wide renaming `FunctionBar`. The C file-static
//!   `FunctionBar* ScreenNames_renamingBar` (`:176`) is modeled here as a
//!   `Mutex<Option<FunctionBar>>` (`None` = the C `NULL`); dropping the
//!   `Some` payload runs `FunctionBar`'s `Drop`, the faithful analog of the
//!   C `FunctionBar_delete`, and leaving `None` reproduces the `= NULL`.
//!
//! Stubbed (cannot be ported faithfully yet — specific blocker per fn):
//! - `ScreenTabsPanel_delete` (`:62`) / — a pure
//!   `Panel_done` + `free` chain; the owned fields are released by `Drop` in
//!   Rust, so there is no algorithm to port (same as the `Panel_delete` /
//!   `ListItem_delete` / `FunctionBar_delete` stubs elsewhere in the tree).
//! - The four event handlers `ScreenTabsPanel_eventHandler` (`:68`),
//!   `ScreenNamesPanel_eventHandlerRenaming` (`:215`),
//!   `ScreenNamesPanel_eventHandlerNormal` (`:306`) and
//!   `ScreenNamesPanel_eventHandler` (`:350`). Their historical blockers —
//!   the `HandlerResult` bitmask (`Panel.h:23`) and `Panel_selectByTyping`
//!   (`Panel.c:507`) — are now available in `panel.rs`, but each handler
//!   still cannot be ported faithfully: (a) the `ScreenNamesPanel` /
//!   `ScreenTabsPanel` subclass structs (`ScreenTabsPanel.h:20` / `:36`,
//!   `Panel super` plus the raw-pointer fields `names`/`ds`/`renamingItem`/
//!   `saved`) are not modeled here; (b) their core branches call the
//!   still-stubbed leaf fns `ScreenNamesPanel_fill` (`:37`), `addNewScreen`
//!   (`:296`), `startRenaming` (`:276`) and `renameScreenSettings` (`:204`),
//!   each blocked on the unmodeled `Settings.screens[]`/`nScreens`/
//!   `lastUpdate`, `ScreenSettings.heading`/`dynamic`, `DynamicScreen.heading`
//!   and the stubbed `Settings_newScreen`/`Settings_newDynamicScreen`
//!   (`Settings.c:263`/`:286`); (c) `_eventHandlerRenaming` additionally
//!   needs the unported `LineEditor_getCursor` (`LineEditor.h`) — the
//!   `LineEditor.cursor` field is private to `lineeditor.rs` — and the
//!   `renamingItem`/`saved` `ListItem*`/`char*` aliasing into the item
//!   vector, which the `Vec<Box<dyn Object>>` panel model forbids.
//!   (Precedent: `MainPanel_eventHandler` is likewise still a stub in
//!   `mainpanel.rs` even though the `MainPanel` struct is modeled.)
//! - `ScreenNamesPanel_fill` (`:37`) — iterates `settings->nScreens` /
//!   `settings->screens[]` and reads `ss->dynamic` / `ss->heading`; none of
//!   those `Settings`/`ScreenSettings` fields exist in the ported `Settings`
//!   subset (`settings.rs`), and it builds items via the stubbed
//!   `ListItem_new` (`ListItem.c:47`).
//! - `renameScreenSettings` (`:204`) — writes `ss->heading` and bumps
//!   `settings->lastUpdate`; neither `ScreenSettings.heading` nor
//!   `Settings.lastUpdate` is modeled in the ported subset.
//! - `startRenaming` (`:276`) — stores a `ListItem*` alias (`renamingItem`)
//!   and aliases `item->value` onto the live `LineEditor` buffer; that
//!   pointer aliasing has no safe analog under the `Vec<Box<dyn Object>>`
//!   panel model, and it needs the `ScreenNamesPanel` struct's raw-pointer
//!   fields.
//! - `addNewScreen` (`:296`) — allocates a `ScreenSettings` via the stubbed
//!   `Settings_newScreen` / `Settings_newDynamicScreen` (`Settings.c:263` /
//!   `:286`) and inserts a `ScreenNameListItem`.
//! - `ScreenTabListItem_new` (`:121`) / `ScreenNameListItem_new` (`:167`) —
//!   `AllocThis` constructors that stash a borrowed `DynamicScreen*` /
//!   `ScreenSettings*` pointer into a `ListItem` subclass. Construction is a
//!   struct literal and destruction is `Drop` (so `AllocThis` has no
//!   safe-Rust free-fn analog, as with the `ListItem_new` stub), and the
//!   borrowed back-pointer needs the panel's item-lifetime model that is not
//!   built here.
//! - `addDynamicScreen` (`:128`) — a `Hashtable_foreach` callback reading
//!   `screen->heading` (absent from the ported `DynamicScreen` model) and
//!   building a `ScreenTabListItem` via the stubbed constructor above.
//! - `ScreenTabsPanel_new` (`:138`) / `ScreenNamesPanel_new` (`:366`) —
//!   construct the panels: they need `Hashtable_foreach` over
//!   `settings->dynamicScreens`, the `settings->screens[]` array, the
//!   `ScreenNamesPanel`/`ScreenTabsPanel` structs with their raw-pointer
//!   fields, and the stubbed list-item constructors. Blocked on all of the
//!   above.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::sync::Mutex;

use crate::ported::functionbar::FunctionBar;

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

/// TODO: port of `static void ScreenNamesPanel_fill(ScreenNamesPanel* this, DynamicScreen* ds` from `ScreenTabsPanel.c:37`.
/// Blocked: reads `settings->nScreens` / `settings->screens[]` and
/// `ss->dynamic` / `ss->heading` (none modeled in the ported `Settings`
/// subset) and builds items via the stubbed `ListItem_new` (`ListItem.c:47`).
pub fn ScreenNamesPanel_fill() {
    todo!("port of ScreenTabsPanel.c:37")
}

/// TODO: port of `static void ScreenTabsPanel_delete(Object* object` from `ScreenTabsPanel.c:62`.
/// `Panel_done` + `free` — the owned fields are released by `Drop` in Rust,
/// so there is no algorithm to port (as with the `Panel_delete` stub).
pub fn ScreenTabsPanel_delete() {
    todo!("port of ScreenTabsPanel.c:62 — Drop releases the panel")
}

/// TODO: port of `static HandlerResult ScreenTabsPanel_eventHandler(Panel* super, int ch` from `ScreenTabsPanel.c:68`.
/// Blocked: needs the unmodeled `ScreenTabsPanel` struct (`ScreenTabsPanel.h:36`,
/// for `this->names`), and its `if (result == HANDLED)` tail casts
/// `Panel_getSelected` to `ScreenTabListItem*` to read `focus->ds` and calls
/// the still-stubbed `ScreenNamesPanel_fill` (`:37`) and
/// `ScreenNamesPanel_eventHandlerNormal` (`:306`). `HandlerResult`
/// (`Panel.h:23`) and `Panel_selectByTyping` (`Panel.c:507`) are now in
/// `panel.rs` and no longer block it.
pub fn ScreenTabsPanel_eventHandler() {
    todo!("port of ScreenTabsPanel.c:68")
}

/// TODO: port of `static ScreenTabListItem* ScreenTabListItem_new(const char* value, DynamicScreen* ds` from `ScreenTabsPanel.c:121`.
/// Blocked: `AllocThis` constructor stashing a borrowed `DynamicScreen*`
/// into a `ListItem` subclass — construction is a struct literal / destruction
/// is `Drop` (no `AllocThis` analog, as with `ListItem_new`), and the borrowed
/// back-pointer needs the panel item-lifetime model not built here.
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
/// `ScreenTabsPanel`/`ScreenNamesPanel` structs with raw-pointer fields, and
/// the stubbed `ScreenTabListItem_new` / `ScreenNamesPanel_new`.
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
/// back-pointer and restores `renamingItem->value = this->saved` — both need
/// the `ScreenNameListItem`/`ScreenNamesPanel` structs with the raw-pointer
/// aliasing the `Vec<Box<dyn Object>>` panel model forbids; the trailing
/// `Panel_done` + `free` is released by `Drop`.
pub fn ScreenNamesPanel_delete() {
    todo!("port of ScreenTabsPanel.c:185")
}

/// TODO: port of `static void renameScreenSettings(ScreenNamesPanel* this, const ListItem* item` from `ScreenTabsPanel.c:204`.
/// Blocked: writes `ss->heading` and bumps `settings->lastUpdate`; neither
/// `ScreenSettings.heading` nor `Settings.lastUpdate` exists in the ported
/// `Settings` subset (`settings.rs`).
pub fn renameScreenSettings() {
    todo!("port of ScreenTabsPanel.c:204")
}

/// TODO: port of `static HandlerResult ScreenNamesPanel_eventHandlerRenaming(Panel* super, int ch` from `ScreenTabsPanel.c:215`.
/// Blocked: needs the unmodeled `ScreenNamesPanel` struct
/// (`ScreenTabsPanel.h:20`) with the `renamingItem` (`ListItem*`) / `saved`
/// (`char*`) aliases into the item vector; its default branch reads the
/// unported `LineEditor_getCursor` (`LineEditor.h`; `LineEditor.cursor` is
/// private to `lineeditor.rs`) and its finish branch calls the stubbed
/// `renameScreenSettings` (`:204`). `HandlerResult` (`Panel.h:23`) is now
/// modeled in `panel.rs` and no longer blocks it.
pub fn ScreenNamesPanel_eventHandlerRenaming() {
    todo!("port of ScreenTabsPanel.c:215")
}

/// TODO: port of `static void startRenaming(Panel* super` from `ScreenTabsPanel.c:276`.
/// Blocked: stores a `ListItem*` alias (`renamingItem`) and aliases
/// `item->value` onto the `LineEditor` buffer — no safe analog under the
/// `Vec<Box<dyn Object>>` panel model; also needs the `ScreenNamesPanel`
/// struct and the `ScreenNames_renamingBar` assignment.
pub fn startRenaming() {
    todo!("port of ScreenTabsPanel.c:276")
}

/// TODO: port of `static void addNewScreen(Panel* super, DynamicScreen* ds` from `ScreenTabsPanel.c:296`.
/// Blocked: allocates a `ScreenSettings` via the stubbed `Settings_newScreen`
/// / `Settings_newDynamicScreen` (`Settings.c:263` / `:286`) and inserts a
/// `ScreenNameListItem` built by the stubbed constructor above.
pub fn addNewScreen() {
    todo!("port of ScreenTabsPanel.c:296")
}

/// TODO: port of `static HandlerResult ScreenNamesPanel_eventHandlerNormal(Panel* super, int ch` from `ScreenTabsPanel.c:306`.
/// Blocked: needs the unmodeled `ScreenNamesPanel` struct
/// (`ScreenTabsPanel.h:20`, for `this->ds`); its `KEY_F(5)`/`KEY_CTRL('N')`
/// arm calls the stubbed `addNewScreen` (`:296`) and `startRenaming`
/// (`:276`) — the whole new-screen path — so the port would gut its core.
/// `HandlerResult` (`Panel.h:23`) and `Panel_selectByTyping` (`Panel.c:507`)
/// are now in `panel.rs` and no longer block it.
pub fn ScreenNamesPanel_eventHandlerNormal() {
    todo!("port of ScreenTabsPanel.c:306")
}

/// TODO: port of `static HandlerResult ScreenNamesPanel_eventHandler(Panel* super, int ch` from `ScreenTabsPanel.c:350`.
/// Blocked: needs the unmodeled `ScreenNamesPanel` struct
/// (`ScreenTabsPanel.h:20`) to read `this->renamingItem`, and routes to the
/// still-stubbed `ScreenNamesPanel_eventHandlerNormal` (`:306`) /
/// `ScreenNamesPanel_eventHandlerRenaming` (`:215`) — so both dispatch
/// targets are unavailable. `HandlerResult` (`Panel.h:23`) is now modeled in
/// `panel.rs` and no longer blocks it.
pub fn ScreenNamesPanel_eventHandler() {
    todo!("port of ScreenTabsPanel.c:350")
}

/// TODO: port of `ScreenNamesPanel* ScreenNamesPanel_new(Settings* settings` from `ScreenTabsPanel.c:366`.
/// Blocked: constructs the `ScreenNamesPanel` struct (raw-pointer fields),
/// lazily builds `ScreenNames_renamingBar`, and iterates `settings->nScreens`
/// / `settings->screens[]` (absent from the ported `Settings` subset) building
/// items via the stubbed `ScreenNameListItem_new`.
pub fn ScreenNamesPanel_new() {
    todo!("port of ScreenTabsPanel.c:366")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
