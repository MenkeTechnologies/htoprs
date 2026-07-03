//! Partial port of `CategoriesPanel.c` — the setup-screen category list.
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
//! # Ported
//!
//! - [`CategoriesPanel_eventHandler`] (`CategoriesPanel.c:120`) — the key
//!   dispatch that computes the [`HandlerResult`]. In C the parameter is
//!   literally `Panel* super`; everything reachable from `super` alone (the
//!   `Panel_getSelectedIndex`/`Panel_onKey` navigation arms and the graphic
//!   fall-through to the now-ported [`Panel_selectByTyping`]) ports faithfully.
//!   The `CategoriesPanel* this = (CategoriesPanel*)super` upcast and the
//!   `if (result == HANDLED) { … }` tail it enables — which rebuild the pages
//!   to the right — are the residual blocker; see below.
//!
//! # Stubbed (and the specific substrate blocking each)
//!
//! Two blockers recur across the rest of the file and are noted once here:
//!
//! - **The `scr` back-pointer is self-referential and has no safe-Rust model.**
//!   `struct CategoriesPanel_` (`CategoriesPanel.h:16`) holds `ScreenManager* scr`
//!   — a pointer back to the very `ScreenManager` whose `Vector* panels` owns
//!   this `CategoriesPanel` (it is added by `ScreenManager_add(scr, super, 16)`
//!   in `CategoriesPanel_new`). `screenmanager.rs` models its own
//!   `Header`/`Machine`/`State` back-pointers as owned `Option<T>` because those
//!   are acyclic; `scr` here is a genuine ownership cycle (`scr` owns the panel
//!   that owns `scr`), which an owned field cannot express. The struct is
//!   therefore still not modeled.
//! - **The per-category sibling constructors are all `todo!()` stubs**:
//!   `MetersPanel_new`, `AvailableMetersPanel_new`, `DisplayOptionsPanel_new`,
//!   `ColorsPanel_new`, `ScreensPanel_new`, `HeaderOptionsPanel_new`,
//!   `ScreenTabsPanel_new` (verified in their modules). Each `make*Page` builds
//!   its page by calling one (or several) of these. `ScreenManager_add`
//!   (`screenmanager.rs`) — a former blocker — is now ported, so it no longer
//!   blocks anything here; only the sibling constructors and the `scr`
//!   back-pointer do.
//!
//! With those in mind:
//!
//! - [`CategoriesPanel_delete`] — C body is `Panel_done(&this->super); free(this);`,
//!   released by `Drop` in Rust (same rationale as `Panel_delete`/`Panel_done`),
//!   so there is no algorithm to port.
//! - [`CategoriesPanel_makeMetersPage`] — `MetersPanel_new` +
//!   `AvailableMetersPanel_new` (stubbed) + the `scr`/`host`/`header`
//!   back-pointers.
//! - [`CategoriesPanel_makeDisplayOptionsPage`] — `DisplayOptionsPanel_new`
//!   (stubbed) + the `scr`/`host` back-pointers.
//! - [`CategoriesPanel_makeColorsPage`] — `ColorsPanel_new` (stubbed) + the
//!   `scr`/`host` back-pointers.
//! - [`CategoriesPanel_makeScreenTabsPage`] — `ScreenTabsPanel_new` (stubbed),
//!   PCP-only in C (`#if defined(HTOP_PCP)`) + the `scr`/`host` back-pointers.
//! - [`CategoriesPanel_makeScreensPage`] — `ScreensPanel_new` (stubbed) + the
//!   `scr`/`host` back-pointers.
//! - [`CategoriesPanel_makeHeaderOptionsPage`] — `HeaderOptionsPanel_new`
//!   (stubbed) + the `scr`/`host` back-pointers.
//! - [`CategoriesPanel_new`] — populates the list with the now-ported
//!   `ListItem_new` rows and could register itself via the now-ported
//!   `ScreenManager_add`, but it (a) needs the unmodelable `scr` back-pointer,
//!   and (b) immediately calls `categoriesPanelPages[0].ctor` ==
//!   `CategoriesPanel_makeDisplayOptionsPage` (stubbed). Both block it.
//!
//! # Data model deferred with the constructor
//!
//! htop's `struct CategoriesPanel_` (`CategoriesPanel.h:16`) is a `Panel super`
//! plus three non-owning back-pointers (`ScreenManager* scr`, `Machine* host`,
//! `Header* header`). It is deliberately **not** modeled here: the only ported
//! function ([`CategoriesPanel_eventHandler`]) needs nothing beyond the `Panel*
//! super` it already takes, and the `scr` back-pointer is the self-referential
//! cycle described above. The struct will be modeled together with the first
//! constructor that can be ported (once the sibling constructors land and the
//! `scr` cycle has a home).
//!
//! Also not modeled: the file-static `CategoriesFunctions` function-bar labels
//! (`CategoriesPanel.c:35`) and the `categoriesPanelPages` name/ctor dispatch
//! table (`CategoriesPanel.c:109`). The table's `ctor` column is exactly the set
//! of stubbed `make*Page` functions, so the table has no working entry to point
//! at; it is deferred with the functions that consume it.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::crt::{KEY_CTRL, KEY_DOWN, KEY_END, KEY_HOME, KEY_NPAGE, KEY_PPAGE, KEY_UP};
use crate::ported::panel::{
    HandlerResult, Panel, Panel_getSelectedIndex, Panel_onKey, Panel_selectByTyping,
    EVENT_SET_SELECTED,
};

// The two Ctrl-key codes `CategoriesPanel_eventHandler` matches in its
// navigation arm (`KEY_CTRL('P')` / `KEY_CTRL('N')`, `CategoriesPanel.c:131`
// and `:133`). `KEY_CTRL` is a `const fn` in crt.rs; binding its results as
// `const`s makes them usable as `match` patterns (a const-fn call is not
// itself a pattern) without adding any top-level `fn` — the same idiom
// `panel.rs` uses for its own `CTRL_*` pattern constants.
const CTRL_P: i32 = KEY_CTRL(b'P' as i32);
const CTRL_N: i32 = KEY_CTRL(b'N' as i32);

/// TODO: port of `static void CategoriesPanel_delete(Object* object)` from
/// `CategoriesPanel.c:37`: `Panel_done(&this->super); free(this);`. Blocked
/// on missing substrate: the `CategoriesPanel` struct (`super` panel plus the
/// `scr`/`host`/`header` back-pointers) is not modeled in this port, so there
/// is no `this` type to consume by value. Left a stub rather than inventing
/// an unused struct.
pub fn CategoriesPanel_delete() {
    todo!("port of CategoriesPanel.c:37 — CategoriesPanel struct is not modeled; no Rust type to consume")
}

/// TODO: port of `static void CategoriesPanel_makeMetersPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:43`. Blocked: builds one `MetersPanel_new` per header
/// column plus an `AvailableMetersPanel_new` (both `todo!()` stubs), and reads
/// the unmodeled `scr`/`host`/`header` back-pointers (the `scr` cycle noted in
/// the module docs). `ScreenManager_add` is now ported and no longer blocks it.
pub fn CategoriesPanel_makeMetersPage() {
    todo!("port of CategoriesPanel.c:43 — needs MetersPanel_new + AvailableMetersPanel_new + the scr/host/header back-pointers")
}

/// TODO: port of `static void CategoriesPanel_makeDisplayOptionsPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:65`. Blocked: `DisplayOptionsPanel_new` is a stub and
/// it reads the unmodeled `scr`/`host` back-pointers.
pub fn CategoriesPanel_makeDisplayOptionsPage() {
    todo!(
        "port of CategoriesPanel.c:65 — needs DisplayOptionsPanel_new + the scr/host back-pointers"
    )
}

/// TODO: port of `static void CategoriesPanel_makeColorsPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:71`. Blocked: `ColorsPanel_new` is a stub and it
/// reads the unmodeled `scr`/`host` back-pointers.
pub fn CategoriesPanel_makeColorsPage() {
    todo!("port of CategoriesPanel.c:71 — needs ColorsPanel_new + the scr/host back-pointers")
}

/// TODO: port of `static void CategoriesPanel_makeScreenTabsPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:78`. PCP-only in C (`#if defined(HTOP_PCP)`).
/// Blocked: `ScreenTabsPanel_new` is a stub and it reads the unmodeled
/// `scr`/`host` back-pointers.
pub fn CategoriesPanel_makeScreenTabsPage() {
    todo!("port of CategoriesPanel.c:78 — needs ScreenTabsPanel_new + the scr/host back-pointers (PCP-only)")
}

/// TODO: port of `static void CategoriesPanel_makeScreensPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:87`. Blocked: `ScreensPanel_new` is a stub and it
/// reads the unmodeled `scr`/`host` back-pointers.
pub fn CategoriesPanel_makeScreensPage() {
    todo!("port of CategoriesPanel.c:87 — needs ScreensPanel_new + the scr/host back-pointers")
}

/// TODO: port of `static void CategoriesPanel_makeHeaderOptionsPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:97`. Blocked: `HeaderOptionsPanel_new` is a stub and
/// it reads the unmodeled `scr`/`host` back-pointers.
pub fn CategoriesPanel_makeHeaderOptionsPage() {
    todo!(
        "port of CategoriesPanel.c:97 — needs HeaderOptionsPanel_new + the scr/host back-pointers"
    )
}

/// Port of `static HandlerResult CategoriesPanel_eventHandler(Panel* super, int ch)`
/// from `CategoriesPanel.c:120`.
///
/// Faithful port of the key dispatch that yields the [`HandlerResult`]. The C
/// parameter is literally `Panel* super`; this port keeps that signature, so
/// everything the switch reaches through `super` alone ports 1:1:
/// - `EVENT_SET_SELECTED` → `HANDLED`.
/// - the navigation keys (`KEY_UP`/`^P`/`KEY_DOWN`/`^N`/`KEY_NPAGE`/`KEY_PPAGE`/
///   `KEY_HOME`/`KEY_END`) call [`Panel_onKey`] and report `HANDLED` only when
///   the selection index actually moved.
/// - any other graphic char (`0 < ch < 255 && isgraph`, here `is_ascii_graphic`,
///   matching `isgraph` in the C locale) falls through to [`Panel_selectByTyping`];
///   its `BREAK_LOOP` result is downgraded to `IGNORED`, exactly as in C.
///
/// The C tail `CategoriesPanel* this = (CategoriesPanel*)super; … if (result ==
/// HANDLED) { … }` — which, on a handled event, removes every panel to the
/// right (`ScreenManager_size`/`ScreenManager_remove`, both ported) and then
/// rebuilds the selected page by calling `categoriesPanelPages[selected].ctor(this)`
/// — is NOT reproduced: it requires the `CategoriesPanel* this` upcast, whose
/// only added field it uses (`this->scr`) is the self-referential back-pointer
/// with no safe-Rust model (see the module docs), and every entry of the
/// `categoriesPanelPages` ctor column is a stubbed `make*Page`. The returned
/// `HandlerResult` — the entire contract an event handler exposes to
/// `ScreenManager_run` — is computed faithfully; only that panel-rebuild
/// side effect is deferred with the two blockers above.
pub fn CategoriesPanel_eventHandler(super_: &mut Panel, ch: i32) -> HandlerResult {
    // CategoriesPanel* this = (CategoriesPanel*) super;   // upcast: no safe-Rust
    //   analog; only `this->scr` is read, and that is the self-referential
    //   back-pointer described in the module docs.

    let mut result = HandlerResult::IGNORED;

    let mut selected = Panel_getSelectedIndex(super_);
    match ch {
        EVENT_SET_SELECTED => {
            result = HandlerResult::HANDLED;
        }
        KEY_UP | CTRL_P | KEY_DOWN | CTRL_N | KEY_NPAGE | KEY_PPAGE | KEY_HOME | KEY_END => {
            let previous = selected;
            Panel_onKey(super_, ch);
            selected = Panel_getSelectedIndex(super_);
            if previous != selected {
                result = HandlerResult::HANDLED;
            }
        }
        _ => {
            if 0 < ch && ch < 255 && (ch as u8).is_ascii_graphic() {
                result = Panel_selectByTyping(super_, ch);
            }
            if result == HandlerResult::BREAK_LOOP {
                result = HandlerResult::IGNORED;
            }
        }
    }

    // C: if (result == HANDLED) {
    //        int size = ScreenManager_size(this->scr);
    //        for (int i = 1; i < size; i++)
    //           ScreenManager_remove(this->scr, 1);
    //        if (selected >= 0 && (size_t)selected < ARRAYSIZE(categoriesPanelPages))
    //           categoriesPanelPages[selected].ctor(this);
    //     }
    // Deferred: needs `this->scr` (the self-referential back-pointer, no safe-Rust
    // model) and the `categoriesPanelPages[selected].ctor`, every entry of which
    // is a stubbed `make*Page` (see the module docs). `selected` is read only by
    // this deferred block; the binding below stands in for that consumption so
    // the faithful navigation arm above still needs to compute it.
    let _ = selected;

    result
}

/// TODO: port of `CategoriesPanel* CategoriesPanel_new(ScreenManager* scr,
/// Header* header, Machine* host)` from `CategoriesPanel.c:172`. Blocked: the
/// list-population (`ListItem_new`) and self-registration (`ScreenManager_add`)
/// are both now ported, but the constructor still (a) stores and immediately
/// depends on the `scr` back-pointer, which is the self-referential cycle with
/// no safe-Rust model (see the module docs), and (b) tail-calls the first
/// `categoriesPanelPages` ctor `CategoriesPanel_makeDisplayOptionsPage`, still a
/// stub.
pub fn CategoriesPanel_new() {
    todo!("port of CategoriesPanel.c:172 — needs the scr self-referential back-pointer + makeDisplayOptionsPage")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::listitem::ListItem;
    use crate::ported::object::Object;
    use crate::ported::panel::{Panel_add, Panel_new};

    /// A `Panel` populated with the five (non-PCP) category rows htop lists in
    /// `categoriesPanelPages` (`CategoriesPanel.c:109`), so the graphic-typing
    /// fall-through has real `ListItem` values to search.
    fn categories_panel() -> Panel {
        let mut p = Panel_new(1, 1, 20, 10, None);
        for name in [
            "Display options",
            "Header layout",
            "Meters",
            "Screens",
            "Colors",
        ] {
            let li: Box<dyn Object> = Box::new(ListItem::new_row(name));
            Panel_add(&mut p, li);
        }
        p
    }

    // Local test helper: build a ListItem via its public fields (its `_new`
    // is a free fn returning an owned value; a tiny constructor keeps the
    // test terse without touching production code).
    impl ListItem {
        fn new_row(value: &str) -> ListItem {
            ListItem {
                value: value.to_string(),
                key: 0,
                moving: false,
            }
        }
    }

    #[test]
    fn event_set_selected_is_handled() {
        let mut p = categories_panel();
        let r = CategoriesPanel_eventHandler(&mut p, EVENT_SET_SELECTED);
        assert_eq!(r, HandlerResult::HANDLED);
    }

    #[test]
    fn navigation_that_moves_selection_is_handled() {
        let mut p = categories_panel();
        assert_eq!(Panel_getSelectedIndex(&p), 0);
        let r = CategoriesPanel_eventHandler(&mut p, KEY_DOWN);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&p), 1);
    }

    #[test]
    fn navigation_ctrl_n_moves_selection_like_key_down() {
        let mut p = categories_panel();
        let r = CategoriesPanel_eventHandler(&mut p, CTRL_N);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&p), 1);
    }

    #[test]
    fn navigation_that_does_not_move_selection_is_ignored() {
        let mut p = categories_panel();
        // Already at the top row: KEY_UP clamps to 0, so the index is
        // unchanged and the handler reports IGNORED.
        let r = CategoriesPanel_eventHandler(&mut p, KEY_UP);
        assert_eq!(r, HandlerResult::IGNORED);
        assert_eq!(Panel_getSelectedIndex(&p), 0);
    }

    #[test]
    fn key_end_and_home_toggle_selection_and_report_handled() {
        let mut p = categories_panel();
        let r = CategoriesPanel_eventHandler(&mut p, KEY_END);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&p), 4); // last of five rows
        let r = CategoriesPanel_eventHandler(&mut p, KEY_HOME);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&p), 0);
    }

    #[test]
    fn graphic_char_falls_through_to_select_by_typing() {
        let mut p = categories_panel();
        // 'M' matches "Meters" (index 2) via Panel_selectByTyping, which
        // returns HANDLED and moves the selection.
        let r = CategoriesPanel_eventHandler(&mut p, 'M' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&p), 2);
    }

    #[test]
    fn graphic_char_with_no_match_is_still_handled() {
        let mut p = categories_panel();
        // 'z' matches no row; Panel_selectByTyping still returns HANDLED and
        // leaves the selection where it was (index 0).
        let r = CategoriesPanel_eventHandler(&mut p, 'z' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&p), 0);
    }

    #[test]
    fn q_on_empty_buffer_break_loop_is_downgraded_to_ignored() {
        let mut p = categories_panel();
        // 'q' on the empty type-to-search buffer makes Panel_selectByTyping
        // return BREAK_LOOP; CategoriesPanel_eventHandler downgrades it to
        // IGNORED (CategoriesPanel.c:148-149).
        let r = CategoriesPanel_eventHandler(&mut p, 'q' as i32);
        assert_eq!(r, HandlerResult::IGNORED);
    }

    #[test]
    fn nongraphic_non_navigation_char_is_ignored() {
        let mut p = categories_panel();
        // Ctrl-B (0x02) is not a CategoriesPanel navigation key and is not
        // graphic, so it never leaves the IGNORED default.
        let r = CategoriesPanel_eventHandler(&mut p, KEY_CTRL(b'B' as i32));
        assert_eq!(r, HandlerResult::IGNORED);
        assert_eq!(Panel_getSelectedIndex(&p), 0);
    }
}
