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
//! Ported (self-contained, no unported substrate):
//! - `MetersPanel_cleanup` (`MetersPanel.c:38`) — frees the file-static
//!   moving-mode `FunctionBar` (`Meters_movingBar`) and nulls it. Modeled
//!   against the [`Meters_movingBar`] static below: `FunctionBar_delete`
//!   has no safe-Rust analog (the owned `FunctionBar` is released by
//!   `Drop`), so freeing the bar is setting the `Option` to `None`, and
//!   nulling the pointer is the same assignment — exactly the C guard +
//!   free + null.
//!
//! Stubbed (cannot be ported faithfully yet):
//! - `MetersPanel_delete` (`MetersPanel.c:45`) — `Panel_done(&this->super);
//!   free(this);`. Both the `Panel_done` free-chain and the struct free
//!   are released by `Drop` in Rust; there is no algorithm to port (same
//!   rationale as every other `*_delete` in the ported tree). `Panel_done`
//!   is itself an unported stub in `panel.rs`.
//! - `MetersPanel_setMoving` (`MetersPanel.c:51`), `moveToNeighbor`
//!   (`MetersPanel.c:74`), `MetersPanel_eventHandler` (`MetersPanel.c:95`),
//!   `MetersPanel_new` (`MetersPanel.c:209`) — all four take/construct a
//!   `MetersPanel`, whose struct (`MetersPanel.h:21`) cannot be modeled
//!   faithfully yet. Its `meters` field is a `Vector*` of `Meter` shared
//!   with (and owned by) the `Header`; there is no ported `Vector` type to
//!   point at, and an owned `Vec<Meter>` would break the shared-mutation
//!   semantics the event handler and `moveToNeighbor` depend on (they
//!   mutate the *same* vector the header draws from). Its `leftNeighbor`/
//!   `rightNeighbor` are self-referential `MetersPanel*` and `scr`/
//!   `settings` are back-pointers, forming a cyclic shared-mutable graph
//!   with no owned-struct analog. Beyond the struct, these functions also
//!   need substrate that is absent or stubbed:
//!   * `Meter_toListItem` — not present anywhere in the ported tree
//!     (required by `moveToNeighbor`, `eventHandler`, and `new`).
//!   * `Meter_setMode` (`Meter.c:525`) and `ScreenManager_resize`
//!     (`ScreenManager.c:107`) — both `todo!()` stubs.
//!   * `this->scr->header` — `ScreenManager` (`screenmanager.rs`) models no
//!     `header` field; `this->settings->lastUpdate` — `Settings`
//!     (`settings.rs`) models no `lastUpdate` field.
//!   * `Vector_take`/`Vector_insert`/`Vector_size`/`Vector_get`/
//!     `Vector_remove`/`Vector_moveUp`/`Vector_moveDown` on `this->meters`
//!     — the dynamic-array `Vector` machinery is not ported (`vector.rs`
//!     ports only the sort/search core).
//!   Porting any of the four now would require inventing that substrate, so
//!   they stay `todo!()` stubs.
//!
//! Not modeled: the `MetersPanel_class` `PanelClass` vtable
//! (`MetersPanel.c:201`) — like the other `PanelClass` initializers, the
//! dispatch table is not represented in this struct-based port.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::sync::Mutex;

use crate::ported::functionbar::FunctionBar;

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

/// TODO: port of `void MetersPanel_setMoving(MetersPanel* this, bool moving)`
/// from `MetersPanel.c:51`. Needs the `MetersPanel` struct
/// (`MetersPanel.h:21`), which cannot be modeled faithfully yet: its
/// `meters: Vector*`, self-referential `leftNeighbor`/`rightNeighbor`, and
/// `scr`/`settings` back-pointers form a cyclic shared-mutable graph with
/// no ported `Vector` type and no owned-struct analog. Left as a stub.
pub fn MetersPanel_setMoving() {
    todo!("port of MetersPanel.c:51 — needs the unmodelable MetersPanel struct")
}

/// TODO: port of `static inline bool moveToNeighbor(MetersPanel* this,
/// MetersPanel* neighbor, int selected)` from `MetersPanel.c:74`. Needs the
/// `MetersPanel` struct, `Meter_toListItem` (absent from the ported tree),
/// and `Vector_take`/`Vector_insert` on `this->meters` (unported `Vector`
/// machinery). Left as a stub.
pub fn moveToNeighbor() {
    todo!("port of MetersPanel.c:74 — needs Meter_toListItem + Vector + MetersPanel struct")
}

/// TODO: port of `static HandlerResult MetersPanel_eventHandler(Panel* super,
/// int ch)` from `MetersPanel.c:95`. Needs the `MetersPanel` struct,
/// `Meter_toListItem` (absent), `Meter_setMode` (`Meter.c:525`, stub),
/// `ScreenManager_resize` (`ScreenManager.c:107`, stub), `this->scr->header`
/// (`ScreenManager` models no `header` field), `this->settings->lastUpdate`
/// (`Settings` models no `lastUpdate` field), and the unported `Vector`
/// machinery on `this->meters`. Left as a stub.
pub fn MetersPanel_eventHandler() {
    todo!("port of MetersPanel.c:95 — needs Meter_toListItem/Meter_setMode/ScreenManager_resize + Vector + MetersPanel struct")
}

/// TODO: port of `MetersPanel* MetersPanel_new(Settings* settings,
/// const char* header, Vector* meters, ScreenManager* scr)` from
/// `MetersPanel.c:209`. Constructs the `MetersPanel` struct (unmodelable —
/// see the module header) and needs `Meter_toListItem` (absent from the
/// ported tree) plus the `Vector*` of meters. Left as a stub.
pub fn MetersPanel_new() {
    todo!("port of MetersPanel.c:209 — needs Meter_toListItem + Vector + MetersPanel struct")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moving_bar() -> FunctionBar {
        // A minimal moving-mode bar (shape mirrors MetersMovingFunctions).
        FunctionBar {
            functions: vec!["Style ".into(), "MoveLt".into()],
            keys: vec!["F4".into(), "F5".into()],
            events: vec![4, 5],
            staticData: false,
        }
    }

    // Single test: both cases mutate the shared file-static, so they are
    // exercised sequentially here rather than as parallel tests that race.
    #[test]
    fn cleanup_clears_bar_and_is_noop_on_null() {
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
}
