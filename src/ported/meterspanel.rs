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
//! workaround the sibling panels use), the `bool moving` flag is carried
//! verbatim, and `meters` is now a ported [`crate::ported::vector::Vector`]
//! of `Meter` boxed as `Object` (`Meter` now `impl Object`, so it can live
//! in a `Vector` wherever C stores an `Object*`). In C `meters` is a
//! `Vector*` **shared with and owned by** the `Header`; the owned `Vector`
//! model here takes ownership of that store instead of aliasing it (there is
//! no safe-Rust way to model two owners of one `Vector`). This is faithful
//! for every operation that reads or mutates a MetersPanel's *own* `meters`
//! column — which is all of [`moveToNeighbor`], [`MetersPanel_setMoving`],
//! and [`MetersPanel_new`] — and is the same owned-`Vector`-subsumes-a-shared-
//! `Vector*` decision `panel.rs`/`vector.rs` already took. The remaining
//! three C fields have no faithful owned analog and are omitted (see the stub
//! notes): `leftNeighbor`/`rightNeighbor` are self-referential `MetersPanel*`
//! cross-links between the two columns, and `scr`/`settings` are
//! back-pointers to the owning `ScreenManager`/shared `Settings` — modeling
//! any of them as an owned field would form an ownership cycle (the
//! `ScreenManager` owns the `MetersPanel`s through its `panels` `Vector`, and
//! these fields point back at it).
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
//! - `moveToNeighbor` (`MetersPanel.c:74`) — relocates the selected meter to
//!   a neighbor column. `neighbor` is a *parameter* (not the omitted
//!   `leftNeighbor`/`rightNeighbor` field), so it ports as
//!   `neighbor: Option<&mut MetersPanel>`: takes the `Meter` out of `this`'s
//!   `meters` ([`Vector_take`]) and its panel row ([`Panel_remove`]), inserts
//!   the meter into `neighbor`'s `meters` ([`Vector_insert`]), rebuilds the
//!   row from the now-relocated meter via [`Meter_toListItem`]
//!   ([`Panel_insert`]), moves the selection, and toggles move mode on both
//!   columns. Every step is a pure own-`meters`/own-`super` op.
//! - `MetersPanel_new` (`MetersPanel.c:209`) — the constructor. Builds the
//!   default `FunctionBar` (`MetersFunctions`), lazily builds the shared
//!   `Meters_movingBar` (`MetersMovingFunctions`), `Panel_init`s the embedded
//!   panel, takes ownership of the passed-in `meters` `Vector`, sets the
//!   header, and populates the panel with one [`Meter_toListItem`] row per
//!   meter. The C `Settings*`/`ScreenManager*` params are **dropped** (like
//!   `Panel_new` drops its `type`/`owner`): they are stored only into the
//!   omitted `settings`/`scr` back-pointer fields, and the neighbor
//!   cross-links C nulls here are likewise absent.
//!
//! Stubbed (cannot be ported faithfully yet):
//! - `MetersPanel_delete` (`MetersPanel.c:45`) — `Panel_done(&this->super);
//!   free(this);`. Both the `Panel_done` free-chain and the struct free
//!   are released by `Drop` in Rust; there is no algorithm to port (same
//!   rationale as every other `*_delete` in the ported tree). `Panel_done`
//!   is itself an unported stub in `panel.rs`.
//! - `MetersPanel_eventHandler` (`MetersPanel.c:95`) — the key dispatcher.
//!   The individual arms are now portable (restyle via `Meter_setMode` +
//!   `Meter_toListItem` over the owned `meters`; up/down via `Vector_moveUp`/
//!   `Vector_moveDown` + the panel; delete via `Vector_remove`; the toggle
//!   via `MetersPanel_setMoving`), but the function as a whole cannot port
//!   for two structural reasons: (a) its C signature is
//!   `(Panel* super, int ch)` and immediately downcasts
//!   `this = (MetersPanel*) super` — the owned model has no `Panel`→
//!   `MetersPanel` upcast, so there is no faithful entry point; (b) the
//!   side-move arms read `this->rightNeighbor`/`this->leftNeighbor` (the
//!   omitted self-referential cross-link fields) and the shared tail block
//!   reads `this->scr->header` / `this->settings->changed`+`lastUpdate` (the
//!   omitted `scr`/`settings` back-pointer fields). The tail's *callees*
//!   (`Header_calculateHeight`, `ScreenManager_resize`) and the fields they
//!   touch (`ScreenManager.header`, `Settings.changed`/`lastUpdate`) are all
//!   ported now — only the back-pointer path to reach them is missing. Left
//!   as a stub; returns [`crate::ported::panel::HandlerResult`] (modeled).
//!
//! Not modeled: the `MetersPanel_class` `PanelClass` vtable
//! (`MetersPanel.c:201`) — like the other `PanelClass` initializers, the
//! dispatch table is not represented in this struct-based port.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;
use core::ffi::c_int;
use std::sync::Mutex;

use crate::ported::crt::ColorElements;
use crate::ported::functionbar::{FunctionBar, FunctionBar_new};
use crate::ported::listitem::ListItem;
use crate::ported::meter::{Meter, Meter_toListItem};
use crate::ported::panel::{
    Panel, Panel_add, Panel_insert, Panel_new, Panel_remove, Panel_setDefaultBar, Panel_setHeader,
    Panel_setSelected, Panel_setSelectionColor, Panel_size,
};
use crate::ported::vector::{Vector, Vector_get, Vector_insert, Vector_size, Vector_take};

/// Port of `static const char* const MetersFunctions[]` from
/// `MetersPanel.c:28` — the standard (non-moving) F-key layout:
/// `F4=Style F7=MoveUp F8=MoveDn F9=Delete F10=Done`. The trailing `NULL`
/// terminator is dropped (a Rust slice length is the terminator, matching
/// how `functionbar.rs` ports the `FunctionBar_F*` tables).
const MetersFunctions: [&str; 10] = [
    "      ", "      ", "      ", "Style ", "      ", "      ", "MoveUp", "MoveDn", "Delete",
    "Done  ",
];

/// Port of `static const char* const MetersMovingFunctions[]` from
/// `MetersPanel.c:35` — the move-mode F-key layout, adding
/// `F5=MoveLt F6=MoveRt` to the standard set. UTF-8 arrows are avoided
/// upstream (they display full-width on some terminals), so the labels are
/// ASCII verbatim. Trailing `NULL` dropped as above.
const MetersMovingFunctions: [&str; 10] = [
    "      ", "      ", "      ", "Style ", "MoveLt", "MoveRt", "MoveUp", "MoveDn", "Delete",
    "Done  ",
];

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

/// Partial model of the C `MetersPanel` struct (`MetersPanel.h:21`). Three
/// of the six C fields are modeled: `super_` (the embedded `Panel super`;
/// `super` is a Rust keyword — same rename the sibling panels use), `meters`
/// (a ported [`Vector`] of `Meter` boxed as `Object`, owning the column's
/// meter store — C aliases the `Header`'s `Vector*`, but the owned model
/// takes ownership; see the module docs), and `moving`. The other three C
/// fields have no faithful owned analog and are omitted: `leftNeighbor`/
/// `rightNeighbor` are self-referential `MetersPanel*` cross-links and
/// `scr`/`settings` are back-pointers to the owning `ScreenManager`/shared
/// `Settings` — any owned analog would form an ownership cycle.
pub struct MetersPanel {
    /// C `Panel super`.
    pub super_: Panel,
    /// C `Vector* meters` (a `Vector` of `Meter*`), owned here rather than
    /// aliased.
    pub meters: Vector,
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

/// Port of `static inline bool moveToNeighbor(MetersPanel* this,
/// MetersPanel* neighbor, int selected)` from `MetersPanel.c:74`.
///
/// ```c
/// Panel* super = &this->super;
/// if (this->moving) {
///    if (neighbor) {
///       if (selected < Vector_size(this->meters)) {
///          MetersPanel_setMoving(this, false);
///          Meter* meter = (Meter*) Vector_take(this->meters, selected);
///          Panel_remove(super, selected);
///          Vector_insert(neighbor->meters, selected, meter);
///          Panel_insert(&(neighbor->super), selected, (Object*) Meter_toListItem(meter, false));
///          Panel_setSelected(&(neighbor->super), selected);
///          MetersPanel_setMoving(neighbor, true);
///          return true;
///       }
///    }
/// }
/// return false;
/// ```
///
/// The C `MetersPanel* neighbor` parameter maps to `Option<&mut MetersPanel>`
/// (`NULL` ⇒ `None`) — it is a caller-supplied column, not the omitted
/// `leftNeighbor`/`rightNeighbor` field, so nothing about the aliasing gap
/// blocks it. `this` and `neighbor` are disjoint `&mut MetersPanel`s, so
/// each `this->meters`/`neighbor->meters` and `this->super`/`neighbor->super`
/// access is a clean borrow.
///
/// The one ordering nuance: C keeps the `Meter*` after `Vector_insert`
/// (the pointer is copied into `neighbor->meters`) and reads it via
/// `Meter_toListItem(meter, …)`. An owned `Box<dyn Object>` is *moved* into
/// `Vector_insert`, so the list row is rebuilt by reading the meter back out
/// of `neighbor->meters` at `selected` ([`Vector_get`] + `downcast_ref`) —
/// the same object, now living in the neighbor column. `Panel_remove`'s
/// returned `Box` is dropped (freeing the old `ListItem`, exactly as the C
/// owner-`Vector` frees it).
pub fn moveToNeighbor(
    this: &mut MetersPanel,
    neighbor: Option<&mut MetersPanel>,
    selected: c_int,
) -> bool {
    if this.moving {
        if let Some(neighbor) = neighbor {
            if selected < Vector_size(&this.meters) {
                MetersPanel_setMoving(this, false);

                // Meter* meter = (Meter*) Vector_take(this->meters, selected);
                let meter = Vector_take(&mut this.meters, selected);
                // Panel_remove(super, selected); (returned ListItem freed by Drop)
                let _ = Panel_remove(&mut this.super_, selected);
                // Vector_insert(neighbor->meters, selected, meter);
                Vector_insert(&mut neighbor.meters, selected, meter);
                // (Object*) Meter_toListItem(meter, false) — meter now lives in
                // neighbor->meters at `selected`; read it back to build the row.
                let item = {
                    let obj = Vector_get(&neighbor.meters, selected as usize);
                    let any: &dyn Any = obj;
                    let m = any
                        .downcast_ref::<Meter>()
                        .expect("moveToNeighbor: meters element is not a Meter");
                    Meter_toListItem(m, false)
                };
                // Panel_insert(&(neighbor->super), selected, ...);
                Panel_insert(&mut neighbor.super_, selected, Box::new(item));
                Panel_setSelected(&mut neighbor.super_, selected);

                MetersPanel_setMoving(neighbor, true);
                return true;
            }
        }
    }
    false
}

/// TODO: port of `static HandlerResult MetersPanel_eventHandler(Panel* super,
/// int ch)` from `MetersPanel.c:95`. The individual switch arms are now
/// portable ([`moveToNeighbor`] is ported, `Meter_setMode`/`Meter_toListItem`
/// exist, the `Vector`/`Panel` list ops exist), but the function cannot port
/// as a unit: (a) its C signature is `(Panel* super, int ch)` and downcasts
/// `this = (MetersPanel*) super` — the owned model has no `Panel`→
/// `MetersPanel` upcast, so there is no faithful entry point; (b) the
/// side-move arms read the omitted `this->rightNeighbor`/`this->leftNeighbor`
/// cross-link fields, and the shared tail block reads the omitted
/// `this->scr`/`this->settings` back-pointer fields (`this->scr->header`,
/// `this->settings->changed`+`lastUpdate`). The tail callees
/// (`Header_calculateHeight`, `ScreenManager_resize`) and the fields they
/// touch are all ported — only the back-pointer path is missing. Returns
/// [`crate::ported::panel::HandlerResult`] (modeled). Left as a stub.
pub fn MetersPanel_eventHandler() {
    todo!("port of MetersPanel.c:95 — needs Panel->MetersPanel upcast + rightNeighbor/leftNeighbor + scr/settings back-pointers")
}

/// Port of `MetersPanel* MetersPanel_new(Settings* settings,
/// const char* header, Vector* meters, ScreenManager* scr)` from
/// `MetersPanel.c:209`.
///
/// ```c
/// MetersPanel* this = AllocThis(MetersPanel);
/// Panel* super = &this->super;
/// FunctionBar* fuBar = FunctionBar_new(MetersFunctions, NULL, NULL);
/// if (!Meters_movingBar)
///    Meters_movingBar = FunctionBar_new(MetersMovingFunctions, NULL, NULL);
/// Panel_init(super, 1, 1, 1, 1, Class(ListItem), true, fuBar);
/// this->settings = settings; this->meters = meters; this->scr = scr;
/// this->moving = false; this->rightNeighbor = NULL; this->leftNeighbor = NULL;
/// Panel_setHeader(super, header);
/// for (int i = 0; i < Vector_size(meters); i++)
///    Panel_add(super, (Object*) Meter_toListItem(Vector_get(meters, i), false));
/// return this;
/// ```
///
/// The `Settings*`/`ScreenManager*` params are dropped: they are stored only
/// into the omitted `settings`/`scr` back-pointer fields (the same reason
/// `Panel_new` drops its `type`/`owner`), and the `rightNeighbor`/
/// `leftNeighbor` NULL-init is the absence of those omitted cross-link
/// fields. `meters` is taken by value — the owned `Vector` model owns the
/// column store rather than aliasing the `Header`'s. `Panel_init(super, 1,
/// 1, 1, 1, Class(ListItem), true, fuBar)` is [`Panel_new`] at those coords
/// (which drops the `Class(ListItem)`/`true` `Vector`-typing args). Each row
/// is built by reading a meter out of the now-owned `meters` and running it
/// through [`Meter_toListItem`].
pub fn MetersPanel_new(header: &str, meters: Vector) -> MetersPanel {
    // FunctionBar* fuBar = FunctionBar_new(MetersFunctions, NULL, NULL);
    let fu_bar = FunctionBar_new(Some(&MetersFunctions), None, None);
    // if (!Meters_movingBar) Meters_movingBar = FunctionBar_new(MetersMovingFunctions, NULL, NULL);
    {
        let mut bar = Meters_movingBar.lock().unwrap();
        if bar.is_none() {
            *bar = Some(FunctionBar_new(Some(&MetersMovingFunctions), None, None));
        }
    }
    // Panel_init(super, 1, 1, 1, 1, Class(ListItem), true, fuBar);
    let super_ = Panel_new(1, 1, 1, 1, Some(fu_bar));

    // this->meters = meters; this->moving = false; (neighbors NULL == omitted)
    let mut this = MetersPanel {
        super_,
        meters,
        moving: false,
    };

    // Panel_setHeader(super, header);
    Panel_setHeader(&mut this.super_, header);

    // for (int i = 0; i < Vector_size(meters); i++)
    //    Panel_add(super, (Object*) Meter_toListItem(Vector_get(meters, i), false));
    for i in 0..Vector_size(&this.meters) {
        let item = {
            let obj = Vector_get(&this.meters, i as usize);
            let any: &dyn Any = obj;
            let m = any
                .downcast_ref::<Meter>()
                .expect("MetersPanel_new: meters element is not a Meter");
            Meter_toListItem(m, false)
        };
        Panel_add(&mut this.super_, Box::new(item));
    }

    this
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::meter::Meter_class;
    use crate::ported::object::Object;
    use crate::ported::panel::{Panel_get, Panel_new, Panel_size};
    use crate::ported::richstring::RichString_sizeVal;
    use crate::ported::vector::{Vector_add, Vector_new};

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

    // An empty owned `Vector` typed for `Meter` elements (C
    // `Vector_new(Class(Meter), true, size)`).
    fn empty_meters() -> Vector {
        Vector_new(&Meter_class.super_, true, 4)
    }

    // A `Meter` boxed as `Object`, with a fixed setup-menu ui-name and
    // reserved mode 0 (so `Meter_toListItem`'s label is exactly `name`).
    fn meter(name: &'static str) -> Box<dyn Object> {
        Box::new(Meter {
            uiName: name,
            mode: 0,
            ..Meter::empty()
        })
    }

    // A MetersPanel whose embedded Panel holds `n` non-moving ListItems and a
    // known default FunctionBar (so Panel_setDefaultBar's restore is visible).
    // Its `meters` Vector is left empty (setMoving reads only super_/moving).
    fn mp(n: usize) -> MetersPanel {
        let mut super_ = Panel_new(1, 1, 1, 1, Some(default_bar()));
        for i in 0..n {
            super_.items.push(li(&format!("meter{i}"), false));
        }
        MetersPanel {
            super_,
            meters: empty_meters(),
            moving: false,
        }
    }

    // Read the value string of the panel row at index `i`.
    fn row_value(m: &MetersPanel, i: usize) -> String {
        let any: &dyn Any = Panel_get(&m.super_, i as i32);
        any.downcast_ref::<ListItem>().unwrap().value.clone()
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

    // ── MetersPanel_new ───────────────────────────────────────────────

    #[test]
    fn new_owns_meters_populates_rows_and_sets_header() {
        // MetersPanel_new lazily inits the shared Meters_movingBar static.
        let _guard = BAR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *Meters_movingBar.lock().unwrap() = None;

        let mut meters = empty_meters();
        Vector_add(&mut meters, meter("CPU"));
        Vector_add(&mut meters, meter("Mem"));

        let m = MetersPanel_new("Meters", meters);

        assert!(!m.moving);
        // Owns the passed-in meters store.
        assert_eq!(Vector_size(&m.meters), 2);
        // One panel row per meter, labelled by Meter_toListItem (mode 0 => name).
        assert_eq!(Panel_size(&m.super_), 2);
        assert_eq!(row_value(&m, 0), "CPU");
        assert_eq!(row_value(&m, 1), "Mem");
        // Header was set.
        assert_eq!(RichString_sizeVal(&m.super_.header), "Meters".len() as i32);
        // currentBar is the default MetersFunctions bar.
        assert_eq!(
            m.super_.currentBar.as_ref().unwrap().functions,
            MetersFunctions
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        // The moving-mode bar static was lazily built.
        assert!(Meters_movingBar.lock().unwrap().is_some());

        *Meters_movingBar.lock().unwrap() = None;
    }

    #[test]
    fn new_with_empty_meters_has_no_rows() {
        let _guard = BAR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let m = MetersPanel_new("Empty", empty_meters());
        assert_eq!(Vector_size(&m.meters), 0);
        assert_eq!(Panel_size(&m.super_), 0);
        *Meters_movingBar.lock().unwrap() = None;
    }

    // ── moveToNeighbor ────────────────────────────────────────────────

    #[test]
    fn move_to_neighbor_relocates_meter_and_row_when_moving() {
        let _guard = BAR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *Meters_movingBar.lock().unwrap() = Some(moving_bar());

        // `this` owns one meter "CPU"; `neighbor` starts empty.
        let mut meters = empty_meters();
        Vector_add(&mut meters, meter("CPU"));
        let mut this = MetersPanel_new("Left", meters);
        let mut neighbor = MetersPanel_new("Right", empty_meters());

        // Enter move mode on `this` with the CPU row selected.
        this.super_.selected = 0;
        MetersPanel_setMoving(&mut this, true);
        assert!(this.moving);

        let moved = moveToNeighbor(&mut this, Some(&mut neighbor), 0);

        assert!(moved);
        // Meter + row left `this`.
        assert_eq!(Vector_size(&this.meters), 0);
        assert_eq!(Panel_size(&this.super_), 0);
        assert!(!this.moving); // MetersPanel_setMoving(this, false)
                               // Meter + row arrived in `neighbor`.
        assert_eq!(Vector_size(&neighbor.meters), 1);
        assert_eq!(Panel_size(&neighbor.super_), 1);
        assert_eq!(row_value(&neighbor, 0), "CPU");
        assert_eq!(neighbor.super_.selected, 0);
        assert!(neighbor.moving); // MetersPanel_setMoving(neighbor, true)

        *Meters_movingBar.lock().unwrap() = None;
    }

    #[test]
    fn move_to_neighbor_is_noop_when_not_moving() {
        let _guard = BAR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut meters = empty_meters();
        Vector_add(&mut meters, meter("CPU"));
        let mut this = MetersPanel_new("Left", meters);
        let mut neighbor = MetersPanel_new("Right", empty_meters());

        // this.moving is false: the guard fails, nothing moves.
        assert!(!moveToNeighbor(&mut this, Some(&mut neighbor), 0));
        assert_eq!(Vector_size(&this.meters), 1);
        assert_eq!(Vector_size(&neighbor.meters), 0);

        *Meters_movingBar.lock().unwrap() = None;
    }

    #[test]
    fn move_to_neighbor_is_noop_when_neighbor_is_none() {
        let _guard = BAR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *Meters_movingBar.lock().unwrap() = Some(moving_bar());

        let mut meters = empty_meters();
        Vector_add(&mut meters, meter("CPU"));
        let mut this = MetersPanel_new("Left", meters);
        this.super_.selected = 0;
        MetersPanel_setMoving(&mut this, true);

        // No neighbor column: nothing moves, but `this` stays in move mode.
        assert!(!moveToNeighbor(&mut this, None, 0));
        assert_eq!(Vector_size(&this.meters), 1);
        assert!(this.moving);

        *Meters_movingBar.lock().unwrap() = None;
    }

    #[test]
    fn move_to_neighbor_is_noop_when_selected_out_of_range() {
        let _guard = BAR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *Meters_movingBar.lock().unwrap() = Some(moving_bar());

        let mut meters = empty_meters();
        Vector_add(&mut meters, meter("CPU"));
        let mut this = MetersPanel_new("Left", meters);
        let mut neighbor = MetersPanel_new("Right", empty_meters());
        MetersPanel_setMoving(&mut this, true);

        // selected == size (1) is not < Vector_size -> false, nothing moves.
        assert!(!moveToNeighbor(&mut this, Some(&mut neighbor), 1));
        assert_eq!(Vector_size(&this.meters), 1);
        assert_eq!(Vector_size(&neighbor.meters), 0);

        *Meters_movingBar.lock().unwrap() = None;
    }
}
