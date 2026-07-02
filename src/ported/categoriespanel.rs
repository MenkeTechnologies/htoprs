//! Stub scaffold for `CategoriesPanel.c` — the setup-screen category list.
//!
//! `CategoriesPanel` is the left-hand list of the Setup screen ("Display
//! options", "Header layout", "Meters", "Screens", "Colors", …). Selecting a
//! row tears down every panel to its right in the [`ScreenManager`] and rebuilds
//! the page for that category by calling the matching sibling-panel constructor.
//! The whole file is therefore *glue*: it wires together the `ScreenManager`,
//! the `Panel` base widget, and the per-category sub-panels. It owns no
//! algorithm of its own beyond that dispatch.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
//!
//! # Nothing here can be ported faithfully yet
//!
//! Every function in `CategoriesPanel.c` bottoms out in substrate that is still
//! a `todo!()` stub elsewhere in the ported tree, so porting any of them now
//! would mean inventing behavior the C code does not have. Per the port rules,
//! each stays an honest `todo!()` naming its specific blocker:
//!
//! - **`ScreenManager_add`** (`screenmanager.rs`) is a stub — it computes panel
//!   height through `header_height`, which needs the unported `State`/`Header`
//!   substrate. Six of the nine functions (`make*Page` + `CategoriesPanel_new`)
//!   call it.
//! - **The per-category sibling constructors are all stubs**: `MetersPanel_new`,
//!   `AvailableMetersPanel_new`, `DisplayOptionsPanel_new`, `ColorsPanel_new`,
//!   `ScreensPanel_new`, `HeaderOptionsPanel_new`, `ScreenTabsPanel_new`. Each
//!   `make*Page` builds its page by calling one (or several) of these.
//! - **`HandlerResult`** (the `Object.h` enum `HANDLED`/`IGNORED`/`BREAK_LOOP`)
//!   is not modeled anywhere in the ported tree, and `Panel_selectByTyping`
//!   (`panel.rs`) — the fall-through of the event handler — is itself a stub.
//!   `CategoriesPanel_eventHandler` returns a `HandlerResult` and dispatches to
//!   the (stubbed) page constructors, so it is doubly blocked.
//! - **`ListItem_new`** (`listitem.rs`) is a stub; `CategoriesPanel_new`
//!   populates the list with `ListItem_new(page.name, 0)` rows.
//!
//! # Data model deferred with the constructor
//!
//! htop's `struct CategoriesPanel_` (`CategoriesPanel.h:16`) is a `Panel super`
//! plus three non-owning back-pointers (`ScreenManager* scr`, `Machine* host`,
//! `Header* header`). It is deliberately **not** modeled here: no function in
//! this file is portable yet, so nothing would read it, and — as
//! `screenmanager.rs` does for its own `Header`/`Machine`/`State` back-pointers —
//! the non-owning aliases have no faithful safe-Rust analog absent a ported
//! consumer. The struct will be modeled together with the first constructor that
//! can be ported (once `ScreenManager_add` and the sibling constructors land).
//!
//! Also not modeled: the file-static `CategoriesFunctions` function-bar labels
//! (`CategoriesPanel.c:35`) and the `categoriesPanelPages` name/ctor dispatch
//! table (`CategoriesPanel.c:109`). The table's `ctor` column is exactly the set
//! of stubbed `make*Page` functions, so the table has no working entry to point
//! at; it is deferred with the functions that consume it.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `static void CategoriesPanel_delete(Object* object)` from
/// `CategoriesPanel.c:37`. Body is `Panel_done(&this->super); free(this);` —
/// released by `Drop` in Rust (same rationale as `Panel_delete`/`Panel_done`),
/// so there is no algorithm to port.
pub fn CategoriesPanel_delete() {
    todo!("port of CategoriesPanel.c:37 — Drop releases the panel")
}

/// TODO: port of `static void CategoriesPanel_makeMetersPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:43`. Blocked: builds one `MetersPanel_new` per header
/// column plus an `AvailableMetersPanel_new`, and registers each via
/// `ScreenManager_add` — all three are `todo!()` stubs.
pub fn CategoriesPanel_makeMetersPage() {
    todo!("port of CategoriesPanel.c:43 — needs MetersPanel_new + AvailableMetersPanel_new + ScreenManager_add")
}

/// TODO: port of `static void CategoriesPanel_makeDisplayOptionsPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:65`. Blocked: `DisplayOptionsPanel_new` and
/// `ScreenManager_add` are both stubs.
pub fn CategoriesPanel_makeDisplayOptionsPage() {
    todo!("port of CategoriesPanel.c:65 — needs DisplayOptionsPanel_new + ScreenManager_add")
}

/// TODO: port of `static void CategoriesPanel_makeColorsPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:71`. Blocked: `ColorsPanel_new` and
/// `ScreenManager_add` are both stubs.
pub fn CategoriesPanel_makeColorsPage() {
    todo!("port of CategoriesPanel.c:71 — needs ColorsPanel_new + ScreenManager_add")
}

/// TODO: port of `static void CategoriesPanel_makeScreenTabsPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:78`. PCP-only in C (`#if defined(HTOP_PCP)`).
/// Blocked: `ScreenTabsPanel_new` and `ScreenManager_add` are both stubs.
pub fn CategoriesPanel_makeScreenTabsPage() {
    todo!("port of CategoriesPanel.c:78 — needs ScreenTabsPanel_new + ScreenManager_add (PCP-only)")
}

/// TODO: port of `static void CategoriesPanel_makeScreensPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:87`. Blocked: `ScreensPanel_new` and
/// `ScreenManager_add` are both stubs.
pub fn CategoriesPanel_makeScreensPage() {
    todo!("port of CategoriesPanel.c:87 — needs ScreensPanel_new + ScreenManager_add")
}

/// TODO: port of `static void CategoriesPanel_makeHeaderOptionsPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:97`. Blocked: `HeaderOptionsPanel_new` and
/// `ScreenManager_add` are both stubs.
pub fn CategoriesPanel_makeHeaderOptionsPage() {
    todo!("port of CategoriesPanel.c:97 — needs HeaderOptionsPanel_new + ScreenManager_add")
}

/// TODO: port of `static HandlerResult CategoriesPanel_eventHandler(Panel* super, int ch)`
/// from `CategoriesPanel.c:120`. Blocked: returns the unmodeled `HandlerResult`
/// enum (`Object.h`), falls through to the stubbed `Panel_selectByTyping`, and
/// its `HANDLED` branch rebuilds the page by calling the `categoriesPanelPages`
/// ctor — every entry of which is a stubbed `make*Page`.
pub fn CategoriesPanel_eventHandler() {
    todo!("port of CategoriesPanel.c:120 — needs HandlerResult enum + Panel_selectByTyping + make*Page ctors")
}

/// TODO: port of `CategoriesPanel* CategoriesPanel_new(ScreenManager* scr,
/// Header* header, Machine* host)` from `CategoriesPanel.c:172`. Blocked:
/// populates the list with `ListItem_new` rows (stub), registers itself and its
/// first page via `ScreenManager_add` (stub), and immediately calls the first
/// `categoriesPanelPages` ctor `CategoriesPanel_makeDisplayOptionsPage` (stub).
pub fn CategoriesPanel_new() {
    todo!("port of CategoriesPanel.c:172 — needs ListItem_new + ScreenManager_add + makeDisplayOptionsPage")
}
