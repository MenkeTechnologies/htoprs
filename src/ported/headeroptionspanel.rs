//! Partial port of `HeaderOptionsPanel.c` — the Setup-screen "Header Layout"
//! chooser (the panel of radio-style [`CheckItem`](crate::ported::optionitem::CheckItem)
//! rows that pick how many meter columns the header shows and their widths).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
//!
//! # Ported
//!
//! None yet — every function in this file is blocked on substrate that does not
//! exist in the ported tree. The two blockers are described once here.
//!
//! # Stubbed (and the specific substrate blocking each)
//!
//! - **The `HeaderOptionsPanel` struct holds two non-owning back-pointers with
//!   no safe-Rust model.** `struct HeaderOptionsPanel_` (`HeaderOptionsPanel.h:15`)
//!   is a `Panel super` plus `ScreenManager* scr` and `Settings* settings`. The
//!   `scr` pointer is the same self-referential cycle documented in
//!   `categoriespanel.rs`: the panel is added to `scr` by
//!   `ScreenManager_add(scr, super, 16)` (in `CategoriesPanel_makeHeaderOptionsPage`),
//!   so `scr`'s `Vector* panels` owns the very panel that points back at `scr`.
//!   `screenmanager.rs` models `ScreenManager.panels` as an owned `Vec<Panel>`
//!   of plain [`Panel`](crate::ported::panel::Panel)s carrying no back-pointer,
//!   so the wrapper struct cannot be modeled and neither the `(HeaderOptionsPanel*)
//!   super` upcast (used by the event handler) nor the constructed-and-returned
//!   `HeaderOptionsPanel*` (from `_new`) has a home.
//! - **The `HeaderLayout_layouts[]` description table is not ported.**
//!   `_new` reads `HeaderLayout_layouts[i].description` (`HeaderLayout.c:41`) for
//!   each row label. Only the [`HeaderLayout`](crate::ported::settings::HeaderLayout)
//!   enum and `HeaderLayout_getColumns` are ported (in `settings.rs`); the
//!   `HeaderLayout_layouts` table itself — with its `name`/`description`/`widths`
//!   columns — has no ported analog (see the note at `header.rs`, which defers
//!   the same table's `widths[]`).
//!
//! With those in mind:
//!
//! - [`HeaderOptionsPanel_delete`] — C body is `Panel_done(&this->super);
//!   free(this);`, released by `Drop` in Rust (same rationale as
//!   `Panel_delete`/`Panel_done` and every other `*Panel_delete`), so there is
//!   no algorithm to port.
//! - [`HeaderOptionsPanel_eventHandler`] — the Enter/Space/click arm clears
//!   every [`CheckItem`](crate::ported::optionitem::CheckItem) and sets the
//!   marked one (reachable from `super` alone), but then calls
//!   `Header_setLayout(this->scr->header, mark)` (ported), mutates
//!   `this->settings->changed`/`lastUpdate`, and `ScreenManager_resize(this->scr)`
//!   (ported) — all through the unmodelable `this->scr` / `this->settings`
//!   back-pointers. Unlike `CategoriesPanel_eventHandler` (whose `HandlerResult`
//!   is fully derivable from `super`), the entire *effect* of this handler lives
//!   behind those back-pointers, so it is deferred whole.
//! - [`HeaderOptionsPanel_new`] — needs both blockers: the missing
//!   `HeaderLayout_layouts[].description` row labels and the wrapper struct that
//!   stores the `scr`/`settings` back-pointers (and reads
//!   `scr->header->headerLayout` to pre-check the active layout's row).
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `static void HeaderOptionsPanel_delete(Object* object)` from
/// `HeaderOptionsPanel.c:27`. Body is `Panel_done(&this->super); free(this);` —
/// released by `Drop` in Rust (same rationale as `Panel_delete`/`Panel_done`),
/// so there is no algorithm to port.
pub fn HeaderOptionsPanel_delete() {
    todo!("port of HeaderOptionsPanel.c:27 — Drop releases the panel")
}

/// TODO: port of `static HandlerResult HeaderOptionsPanel_eventHandler(Panel* super,
/// int ch)` from `HeaderOptionsPanel.c:33`. Blocked: the handled arm calls
/// `Header_setLayout(this->scr->header, mark)`, sets `this->settings->changed`/
/// `lastUpdate++`, and `ScreenManager_resize(this->scr)` — all through the
/// `HeaderOptionsPanel` wrapper's `scr`/`settings` back-pointers, which are the
/// self-referential cycle with no safe-Rust model (see the module docs).
/// `Header_setLayout` and `ScreenManager_resize` are themselves ported; the
/// blocker is solely the unmodelable back-pointers.
pub fn HeaderOptionsPanel_eventHandler() {
    todo!("port of HeaderOptionsPanel.c:33 — needs the HeaderOptionsPanel scr/settings back-pointers (self-referential cycle)")
}

/// TODO: port of `HeaderOptionsPanel* HeaderOptionsPanel_new(Settings* settings,
/// ScreenManager* scr)` from `HeaderOptionsPanel.c:74`. Blocked on both module
/// blockers: it labels each `CheckItem` row with `HeaderLayout_layouts[i].description`
/// (the description table is not ported — see the module docs) and it constructs
/// the `HeaderOptionsPanel` wrapper storing the `scr`/`settings` back-pointers,
/// reading `scr->header->headerLayout` to pre-select the active layout's row.
pub fn HeaderOptionsPanel_new() {
    todo!("port of HeaderOptionsPanel.c:74 — needs HeaderLayout_layouts[].description + the HeaderOptionsPanel scr/settings back-pointers")
}
