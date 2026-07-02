//! Partial port of `MetersPanel.c` — htop's meter-arrangement panel
//! (the Setup screen's per-column meter list, with move/restyle/delete
//! and cross-column relocation).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C function takes
//! `MetersPanel* this`; the faithful analog would be a free fn taking
//! `this: &mut MetersPanel`.
//!
//! The [`MetersPanel`] struct (`MetersPanel.h:21`) is *partially* modeled:
//! the embedded `Panel super` becomes `super_` (the `super`-keyword
//! workaround the sibling panels use) and the `bool moving` flag is carried
//! verbatim. Those two fields are all [`MetersPanel_setMoving`] reads, so it
//! ports faithfully. The remaining five C fields have no faithful owned
//! analog and are omitted (see the stub notes): `meters` is a `Vector*` of
//! `Meter` **shared with and owned by** the `Header` (aliasing the header's
//! store — an owned field would break the shared mutation the event handler
//! depends on), and `Meter` does not even `impl Object`, so it cannot be held
//! in a ported [`crate::ported::vector::Vector`]; `leftNeighbor`/
//! `rightNeighbor` are self-referential `MetersPanel*` cross-links; `scr`/
//! `settings` are back-pointers — together a cyclic shared-mutable graph.
//!
//! Ported:
//! - `MetersPanel_cleanup` (`MetersPanel.c:38`) — frees the file-static
//!   moving-mode `FunctionBar` (`Meters_movingBar`) and nulls it. Modeled
//!   against the [`Meters_movingBar`] static below: `FunctionBar_delete`
//!   has no safe-Rust analog (the owned `FunctionBar` is released by
//!   `Drop`), so freeing the bar is setting the `Option` to `None`, and
//!   nulling the pointer is the same assignment — exactly the C guard +
//!   free + null.
//! - `MetersPanel_setMoving` (`MetersPanel.c:51`) — toggles move mode over
//!   the embedded `Panel`. Reads only `this->super`/`this->moving` and the
//!   `Meters_movingBar` static, all present: it clears (or sets) each
//!   `ListItem`'s `moving` flag via a `downcast_mut` (the faithful analog of
//!   the C `(ListItem*) Panel_get(...)` cast-and-mutate), flips the panel's
//!   selection color between `PANEL_SELECTION_FOCUS`/`PANEL_SELECTION_FOLLOW`,
//!   and swaps `currentBar` between the default bar ([`Panel_setDefaultBar`])
//!   and a clone of the shared `Meters_movingBar` (the same owned-clone of a
//!   C shared `FunctionBar*` that `Panel_setDefaultBar` already relies on).
//!
//! Stubbed (cannot be ported faithfully yet):
//! - `MetersPanel_delete` (`MetersPanel.c:45`) — `Panel_done(&this->super);
//!   free(this);`. Both the `Panel_done` free-chain and the struct free
//!   are released by `Drop` in Rust; there is no algorithm to port (same
//!   rationale as every other `*_delete` in the ported tree). `Panel_done`
//!   is itself an unported stub in `panel.rs`.
//! - `moveToNeighbor` (`MetersPanel.c:74`) — relocates the selected meter to
//!   a neighbor column. Blocked by (a) `Meter_toListItem` (`Meter.c:571`),
//!   absent from the ported tree; (b) the `Meter`-holding shared `meters`
//!   `Vector*` on both `this` and `neighbor` (`Meter` is not an `Object`, and
//!   the two columns alias/share the header's store); (c) the
//!   self-referential `neighbor: MetersPanel*` cross-link — none of which the
//!   owned struct models.
//! - `MetersPanel_eventHandler` (`MetersPanel.c:95`) — the key dispatcher.
//!   Some arms are pure `Panel`/`Vector` ops, but the function as a whole
//!   cannot port: the restyle arm needs `Meter_toListItem` (absent) +
//!   `Meter_setMode` (ported) over the `Meter`-holding `meters` `Vector*`;
//!   the side-move arms need the blocked `moveToNeighbor`; and the shared
//!   tail block needs `this->scr->header` (`ScreenManager` models no `header`
//!   field), `this->settings->changed`/`lastUpdate` (`Settings` models
//!   neither), `Header_calculateHeight`, and `ScreenManager_resize`
//!   (`ScreenManager.c:107`, a `todo!()` stub). It also returns
//!   [`crate::ported::panel::HandlerResult`] (now modeled) but stays stubbed
//!   on the above.
//! - `MetersPanel_new` (`MetersPanel.c:209`) — the constructor. Blocked by
//!   `Meter_toListItem` (absent) and by the shared `Vector* meters` /
//!   `scr` / `settings` inputs it stores (the aliasing/back-pointer fields
//!   the struct omits).
//!
//! Not modeled: the `MetersPanel_class` `PanelClass` vtable
//! (`MetersPanel.c:201`) — like the other `PanelClass` initializers, the
//! dispatch table is not represented in this struct-based port.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::sync::Mutex;

use crate::ported::crt::ColorElements;
use crate::ported::functionbar::FunctionBar;
use crate::ported::listitem::ListItem;
use crate::ported::panel::{Panel, Panel_setDefaultBar, Panel_setSelectionColor, Panel_size};

/// Port of `static FunctionBar* Meters_movingBar = NULL` from
/// `MetersPanel.c:36`. The file-static moving-mode function bar, lazily
/// built by `MetersPanel_new` and torn down by [`MetersPanel_cleanup`].
/// C shares one raw `FunctionBar*`; the owned analog is an
/// `Option<FunctionBar>` behind a `Mutex` (the C global is a
/// translation-unit static, matching the `AtomicI32 currentLen` precedent
/// in `functionbar.rs`). `NULL` maps to `None`.
static Meters_movingBar: Mutex<Option<FunctionBar>> = Mutex::new(None);

/// Port of `void MetersPanel_cleanup(void)` from `MetersPanel.c:38`.
///
/// ```c
/// if (Meters_movingBar) {
///    FunctionBar_delete(Meters_movingBar);
///    Meters_movingBar = NULL;
/// }
/// ```
///
/// `FunctionBar_delete` frees the bar; the owned `FunctionBar` is released
/// by `Drop`, so both the free and the `NULL` assignment collapse to
/// setting the `Option` to `None`. The `if (Meters_movingBar)` guard is
/// preserved as `is_some()` — replacing `None` with `None` is a no-op, so
/// the guard is behaviorally redundant but kept faithful to the C.
pub fn MetersPanel_cleanup() {
    let mut bar = Meters_movingBar.lock().unwrap();
    if bar.is_some() {
        // Drop frees the bar (C `FunctionBar_delete`); `None` is the NULL.
        *bar = None;
    }
}

/// TODO: port of `static void MetersPanel_delete(Object* object)` from
/// `MetersPanel.c:45`. `Panel_done(&this->super); free(this);` — both the
/// `Panel_done` free-chain and the struct free are released by `Drop` in
/// Rust, so there is no algorithm to port. Left as a stub.
pub fn MetersPanel_delete() {
    todo!("port of MetersPanel.c:45 — Drop releases the panel")
}

/// Partial model of the C `MetersPanel` struct (`MetersPanel.h:21`). Only
/// the two fields [`MetersPanel_setMoving`] touches are modeled: `super_`
/// (the embedded `Panel super`; `super` is a Rust keyword — same rename the
/// sibling panels use) and `moving`. The other five C fields have no
/// faithful owned analog and are omitted — see the module docs: `meters` is
/// a `Vector*` of `Meter` shared with (and owned by) the `Header` (and
/// `Meter` is not a ported `Object`), `leftNeighbor`/`rightNeighbor` are
/// self-referential `MetersPanel*` cross-links, and `scr`/`settings` are
/// back-pointers.
pub struct MetersPanel {
    /// C `Panel super`.
    pub super_: Panel,
    /// C `bool moving`.
    pub moving: bool,
}

/// Port of `void MetersPanel_setMoving(MetersPanel* this, bool moving)` from
/// `MetersPanel.c:51`.
///
/// ```c
/// Panel* super = &this->super;
/// this->moving = moving;
/// if (!moving) {
///    for (int i = 0; i < Panel_size(super); i++) {
///       ListItem* item = (ListItem*) Panel_get(super, i);
///       if (item) item->moving = false;
///    }
///    Panel_setSelectionColor(super, PANEL_SELECTION_FOCUS);
///    Panel_setDefaultBar(super);
/// } else {
///    ListItem* selected = (ListItem*)Panel_getSelected(super);
///    if (selected) selected->moving = true;
///    Panel_setSelectionColor(super, PANEL_SELECTION_FOLLOW);
///    super->currentBar = Meters_movingBar;
/// }
/// super->needsRedraw = true;
/// ```
///
/// The C `(ListItem*) Panel_get(super, i)` / `(ListItem*)
/// Panel_getSelected(super)` casts followed by a write to `->moving` are
/// reproduced by taking the boxed panel item mutably and `downcast_mut`-ing
/// it to [`ListItem`] (the safe-Rust analog of the hard C pointer cast — a
/// wrong class panics here where C would invoke UB). The `if (item)` /
/// `if (selected)` null guards map to the `downcast_mut`'s `Some` (and, for
/// the selected item, to the non-empty check that `Panel_getSelected`
/// itself makes). `super->currentBar = Meters_movingBar` aliases the shared
/// `FunctionBar*` in C; the owned `Option<FunctionBar>` model takes a clone
/// of the [`Meters_movingBar`] static — the exact owned-clone-of-a-shared-bar
/// idiom [`Panel_setDefaultBar`] already uses (a `None` static ⇒ a `None`
/// `currentBar`, matching a `NULL` bar).
pub fn MetersPanel_setMoving(this: &mut MetersPanel, moving: bool) {
    this.moving = moving;
    let super_ = &mut this.super_;
    if !moving {
        // Reset all items' moving flags when canceling move mode.
        let n = Panel_size(super_);
        for i in 0..n {
            // C: ListItem* item = (ListItem*) Panel_get(super, i);
            //    if (item) item->moving = false;
            let any: &mut dyn core::any::Any = super_.items[i as usize].as_mut();
            if let Some(item) = any.downcast_mut::<ListItem>() {
                item.moving = false;
            }
        }
        Panel_setSelectionColor(super_, ColorElements::PANEL_SELECTION_FOCUS);
        Panel_setDefaultBar(super_);
    } else {
        // C: ListItem* selected = (ListItem*) Panel_getSelected(super);
        //    if (selected) selected->moving = true;
        // Panel_getSelected returns NULL on an empty list, so the write only
        // happens when there is a selected item.
        if !super_.items.is_empty() {
            let sel = super_.selected as usize;
            let any: &mut dyn core::any::Any = super_.items[sel].as_mut();
            if let Some(selected) = any.downcast_mut::<ListItem>() {
                selected.moving = true;
            }
        }
        Panel_setSelectionColor(super_, ColorElements::PANEL_SELECTION_FOLLOW);
        // C: super->currentBar = Meters_movingBar; (shared FunctionBar* ->
        // owned clone; a None static reproduces a NULL bar).
        super_.currentBar = Meters_movingBar.lock().unwrap().clone();
    }
    super_.needsRedraw = true;
}

/// TODO: port of `static inline bool moveToNeighbor(MetersPanel* this,
/// MetersPanel* neighbor, int selected)` from `MetersPanel.c:74`. Blocked by
/// `Meter_toListItem` (`Meter.c:571`), absent from the ported tree; by the
/// `Meter`-holding shared `meters` `Vector*` on both `this` and `neighbor`
/// (`Meter` is not a ported `Object`, and the two columns alias the header's
/// store); and by the self-referential `neighbor: MetersPanel*` cross-link
/// the owned struct omits. Left as a stub.
pub fn moveToNeighbor() {
    todo!("port of MetersPanel.c:74 — needs Meter_toListItem + Meter-in-Vector + neighbor cross-link")
}

/// TODO: port of `static HandlerResult MetersPanel_eventHandler(Panel* super,
/// int ch)` from `MetersPanel.c:95`. The restyle arm needs `Meter_toListItem`
/// (absent) over the `Meter`-holding `meters` `Vector*`; the side-move arms
/// need the blocked [`moveToNeighbor`]; and the shared tail block needs
/// `this->scr->header` (`ScreenManager` models no `header` field),
/// `this->settings->changed`/`lastUpdate` (`Settings` models neither),
/// `Header_calculateHeight`, and `ScreenManager_resize` (`ScreenManager.c:107`,
/// a `todo!()` stub). Returns [`crate::ported::panel::HandlerResult`] (now
/// modeled) but stays stubbed on the above. Left as a stub.
pub fn MetersPanel_eventHandler() {
    todo!("port of MetersPanel.c:95 — needs Meter_toListItem + moveToNeighbor + scr->header/settings/ScreenManager_resize")
}

/// TODO: port of `MetersPanel* MetersPanel_new(Settings* settings,
/// const char* header, Vector* meters, ScreenManager* scr)` from
/// `MetersPanel.c:209`. Blocked by `Meter_toListItem` (`Meter.c:571`, absent)
/// and by the shared `Vector* meters` / `scr` / `settings` inputs it stores —
/// the aliasing/back-pointer fields the [`MetersPanel`] struct omits (see the
/// module header). Left as a stub.
pub fn MetersPanel_new() {
    todo!("port of MetersPanel.c:209 — needs Meter_toListItem + shared meters Vector + scr/settings")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::object::Object;
    use crate::ported::panel::Panel_new;

    // Every test that reads or writes the process-shared `Meters_movingBar`
    // static acquires this first, so they run sequentially rather than racing
    // under the parallel test runner (the same hazard `ListItem`'s CRT_utf8
    // tests avoid by folding both cases into one test). `into_inner` recovers
    // from a poisoned lock left by a panicking test.
    static BAR_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn moving_bar() -> FunctionBar {
        // A minimal moving-mode bar (shape mirrors MetersMovingFunctions).
        FunctionBar {
            functions: vec!["Style ".into(), "MoveLt".into()],
            keys: vec!["F4".into(), "F5".into()],
            events: vec![4, 5],
            staticData: false,
        }
    }

    fn default_bar() -> FunctionBar {
        FunctionBar {
            functions: vec!["Style ".into()],
            keys: vec!["F4".into()],
            events: vec![4],
            staticData: false,
        }
    }

    fn li(value: &str, moving: bool) -> Box<dyn Object> {
        Box::new(ListItem {
            value: value.to_string(),
            key: 0,
            moving,
        })
    }

    // A MetersPanel whose embedded Panel holds `n` non-moving ListItems and a
    // known default FunctionBar (so Panel_setDefaultBar's restore is visible).
    fn mp(n: usize) -> MetersPanel {
        let mut super_ = Panel_new(1, 1, 1, 1, Some(default_bar()));
        for i in 0..n {
            super_.items.push(li(&format!("meter{i}"), false));
        }
        MetersPanel {
            super_,
            moving: false,
        }
    }

    fn item_moving(m: &MetersPanel, i: usize) -> bool {
        let any: &dyn core::any::Any = m.super_.items[i].as_ref();
        any.downcast_ref::<ListItem>().unwrap().moving
    }

    // Single test: both cases mutate the shared file-static, so they are
    // exercised sequentially here rather than as parallel tests that race.
    #[test]
    fn cleanup_clears_bar_and_is_noop_on_null() {
        let _guard = BAR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Seed the static as MetersPanel_new would, then tear it down.
        *Meters_movingBar.lock().unwrap() = Some(moving_bar());
        assert!(Meters_movingBar.lock().unwrap().is_some());
        MetersPanel_cleanup();
        assert!(Meters_movingBar.lock().unwrap().is_none());

        // Second cleanup on the now-NULL bar is a no-op (the
        // `if (Meters_movingBar)` guard: replacing None with None).
        MetersPanel_cleanup();
        assert!(Meters_movingBar.lock().unwrap().is_none());
    }

    #[test]
    fn set_moving_true_marks_selected_and_swaps_to_moving_bar() {
        let _guard = BAR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *Meters_movingBar.lock().unwrap() = Some(moving_bar());

        let mut m = mp(3);
        m.super_.selected = 1;
        m.super_.needsRedraw = false;

        MetersPanel_setMoving(&mut m, true);

        assert!(m.moving);
        // Only the selected item's moving flag is set.
        assert!(!item_moving(&m, 0));
        assert!(item_moving(&m, 1));
        assert!(!item_moving(&m, 2));
        // Follow-mode selection color while moving.
        assert_eq!(
            m.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOLLOW
        );
        // currentBar is a clone of the shared moving bar.
        assert_eq!(m.super_.currentBar, Some(moving_bar()));
        assert!(m.super_.needsRedraw);

        // Leave the static clean for other tests.
        *Meters_movingBar.lock().unwrap() = None;
    }

    #[test]
    fn set_moving_false_clears_all_flags_and_restores_default_bar() {
        // Does not read the shared static (the !moving path uses
        // Panel_setDefaultBar), so no BAR_TEST_LOCK is needed.
        let mut m = mp(3);
        // Simulate an in-progress move: every item flagged, follow color,
        // a foreign currentBar swapped in.
        for i in 0..3 {
            let any: &mut dyn core::any::Any = m.super_.items[i].as_mut();
            any.downcast_mut::<ListItem>().unwrap().moving = true;
        }
        m.moving = true;
        m.super_.selectionColorId = ColorElements::PANEL_SELECTION_FOLLOW;
        m.super_.currentBar = Some(moving_bar());
        m.super_.needsRedraw = false;

        MetersPanel_setMoving(&mut m, false);

        assert!(!m.moving);
        // All items' moving flags cleared.
        for i in 0..3 {
            assert!(!item_moving(&m, i), "item {i} still moving");
        }
        // Focus-mode selection color restored.
        assert_eq!(
            m.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOCUS
        );
        // currentBar restored to the panel's default bar (Panel_setDefaultBar).
        assert_eq!(m.super_.currentBar, Some(default_bar()));
        assert!(m.super_.needsRedraw);
    }

    #[test]
    fn set_moving_true_on_empty_panel_does_not_panic() {
        let _guard = BAR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *Meters_movingBar.lock().unwrap() = Some(moving_bar());

        // Panel_getSelected returns NULL on an empty list; the write is
        // skipped and nothing indexes out of bounds.
        let mut m = mp(0);
        MetersPanel_setMoving(&mut m, true);
        assert!(m.moving);
        assert_eq!(
            m.super_.selectionColorId,
            ColorElements::PANEL_SELECTION_FOLLOW
        );
        assert_eq!(m.super_.currentBar, Some(moving_bar()));

        *Meters_movingBar.lock().unwrap() = None;
    }
}
