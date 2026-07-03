//! Partial port of `MainPanel.c` — htop's main process-list panel.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. A C fn `Foo_bar(Foo* this)`
//! ports to a free fn `Foo_bar(this: &mut Foo)` (the same shape the
//! `Panel.c`/`ColumnsPanel.c` ports use: free fns, not methods).
//!
//! # Data model
//!
//! htop's `MainPanel` (`MainPanel.h:21`) embeds a `Panel super`, a
//! `State* state` back-pointer, an owned `IncSet* inc`, an owned
//! `Htop_Action* keys` action table, two owned `FunctionBar*`
//! (`processBar`/`readonlyBar`), and `unsigned int idSearch`. The
//! [`MainPanel`] struct here models every field the ported functions
//! touch — the embedded `super` [`Panel`], the `state` pointer, the owned
//! [`IncSet`] and two owned [`FunctionBar`]s, and `idSearch` — following
//! the reduced-struct precedent of `columnspanel.rs`. `super_` avoids the
//! Rust `super` keyword, matching the `columnspanel.rs`/`process.rs`
//! convention.
//!
//! The C `Htop_Action* keys` action table is **omitted**: the only
//! functions that read/allocate it — [`MainPanel_new`],
//! [`MainPanel_eventHandler`], [`MainPanel_delete`] — are all stubbed on
//! other blockers (see below), so no ported function needs it. The
//! `state` back-pointer is an owning-elsewhere `*mut State` (the C field
//! is a `State*` that `htop.c` owns and shares), so [`MainPanel_setState`]
//! stores it verbatim; the ported functions that read `state`
//! (`eventHandler`/`drawFunctionBar`/`printHeader`) are stubbed on
//! `state->host` (`Machine*`) / `state->failedUpdate`, which the minimal
//! `action::State` model omits.
//!
//! # Ported (self-contained, no unported substrate)
//!
//! - [`MainPanel_updateLabels`] (`MainPanel.c:32`) — retargets the F5
//!   (List/Tree) and F4 (Filter/FILTER) labels on the panel's default
//!   bar via the ported [`FunctionBar_setLabel`]. The C
//!   `MainPanel_getFunctionBar(this)` macro (`MainPanel.h:33`) is
//!   `((Panel*)this)->defaultBar`; the `Vec`-model's `defaultBar` is an
//!   `Option<FunctionBar>`, so the guard `if let Some(bar)` replaces the
//!   C null-deref-if-unset (the bar is always set in practice).
//! - [`MainPanel_idSearch`] (`MainPanel.c:38`, `static`) — the digit-key
//!   incremental PID search: builds a running id from typed digits,
//!   selects the first row whose `id` matches, and rolls the accumulator
//!   over at 10000000. Uses the ported `Panel_size`/`Panel_get`/
//!   `Panel_setSelected`; each item is downcast to [`Row`] via the `Any`
//!   supertrait (the safe-Rust analog of the C `(Row*)Panel_get(...)`
//!   cast).
//! - [`MainPanel_selectedRow`] (`MainPanel.c:175`) — the selected row's
//!   `id`, or `-1` when the list is empty (`Panel_getSelected` → `Row`).
//! - [`MainPanel_foreachRow`] (`MainPanel.c:180`) — applies `fn` to every
//!   tagged row (falling back to the selected row when none are tagged),
//!   AND-folding the results and reporting whether any were tagged. The
//!   ported `Panel_get` hands back an immutable `&dyn Object`, so — like
//!   `ColumnsPanel_cancelMoving` — the faithful mutating analog indexes
//!   `super.items` directly and downcasts each `&mut dyn Object` to
//!   `&mut Row`. The C `Arg arg` (a `union`) is `object::Arg`, which is
//!   not `Copy`, so [`MainPanel_foreachRowFn`] passes it by shared
//!   reference (`&Arg`); the callbacks only ever read it, so this is
//!   observationally identical to the C by-value pass.
//! - [`MainPanel_setState`] (`MainPanel.c:250`) — stores the `State*`
//!   back-pointer.
//! - [`MainPanel_setFunctionBar`] (`MainPanel.c:254`) — points the panel's
//!   (and the `IncSet`'s) default bar at the read-only or process bar. C
//!   aliases the one `FunctionBar*`; the `Vec`-model owns each bar via
//!   `Option<FunctionBar>`, so the target bar is cloned into both slots —
//!   the same clone-reproduces-the-shared-pointer mapping `Panel_init`/
//!   `Panel_setDefaultBar` already use.
//!
//! # Stubbed (cannot be ported faithfully yet)
//!
//! - [`MainPanel_getValue`] (`MainPanel.c:54`, `static`) — `Row_sortKeyString`
//!   (`Row.h:104`) dispatches through the `RowClass.sortKeyString` vtable
//!   slot, which `row.rs` does NOT model (only the `.compare` slot is
//!   realized; the `RowClass`-specific slots are the unmodeled NULL/vtable
//!   entries). No faithful body without that slot.
//! - [`MainPanel_eventHandler`] (`MainPanel.c:59`, `static`) — the panel's
//!   key dispatcher. Blocked on nearly everything unported: `state->host`
//!   (`Machine*`, omitted from `action::State`), `IncSet_handleKey` /
//!   `IncSet_filter` (both `incset.rs` stubs), `Action_setSortKey` /
//!   `Action_setScreenTab` (both `action.rs` stubs), `RowField_keyAt`
//!   (`row.rs` stub), the `ScreenSettings` sort-key helpers, the
//!   `HandlerResult` enum, and the `keys[]` `Htop_Action` dispatch.
//! - [`MainPanel_drawFunctionBar`] (`MainPanel.c:198`, `static`) — reads
//!   `state->pauseUpdate`/`state->failedUpdate`; the latter (a `const char*`
//!   failure message) is omitted from the minimal `action::State`, so the
//!   `else if (this->state->failedUpdate)` branch has no faithful body.
//!   (`IncSet_drawBar` is now ported, so it is no longer a blocker.)
//! - [`MainPanel_printHeader`] (`MainPanel.c:213`, `static`) —
//!   `Table_printHeader` (`table.rs` stub) against `state->host->settings`
//!   (`Machine*`, omitted from `action::State`).
//! - [`MainPanel_new`] (`MainPanel.c:229`) — allocates the panel and calls
//!   `Action_setBindings` (`action.rs` stub) + `Platform_setBindings`
//!   (`Platform.c` unported) to fill the `keys` table. The binding setup
//!   is essential and cannot run against those stubs.
//!
//! [`MainPanel_delete`] (`MainPanel.c:253`) is now ported: its `free` chain
//! maps to the by-value drop idiom (`processBar`/`readonlyBar` handed to
//! `FunctionBar_delete`, `inc` to `IncSet_delete`, `super_` dropped in place
//! of `Panel_done`).
//!
//! `gen_port_report.py` counts `todo!()` bodies as *stubbed*, not
//! *ported*, so scaffolding does not inflate coverage.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)] // `MainPanel_foreachRowFn` mirrors the C typedef name
#![allow(dead_code)]

use core::any::Any;

use crate::ported::action::State;
use crate::ported::crt::KEY_F;
use crate::ported::functionbar::{FunctionBar, FunctionBar_delete, FunctionBar_setLabel};
use crate::ported::incset::{IncSet, IncSet_delete};
use crate::ported::object::{Arg, Object};
use crate::ported::panel::{
    Panel, Panel_get, Panel_getSelected, Panel_setSelected, Panel_size,
};
use crate::ported::row::Row;

/// Reduced model of the C `MainPanel` struct (`MainPanel.h:21`). See the
/// module docs for the omitted `Htop_Action* keys` field and the `state`
/// back-pointer mapping. `super_` avoids the Rust `super` keyword.
pub struct MainPanel {
    /// C `Panel super` — the embedded panel base.
    pub super_: Panel,
    /// C `State* state` — back-pointer to the shared UI state, owned by
    /// `htop.c`. Modeled as a raw pointer ([`MainPanel_setState`] stores
    /// it); the functions that dereference it are stubbed.
    pub state: *mut State,
    /// C `IncSet* inc` — owned incremental search/filter set.
    pub inc: IncSet,
    /// C `FunctionBar* processBar` — owned bar with process-specific
    /// actions.
    pub processBar: FunctionBar,
    /// C `FunctionBar* readonlyBar` — owned bar without process actions.
    pub readonlyBar: FunctionBar,
    /// C `unsigned int idSearch` — accumulator for digit-key PID search.
    pub idSearch: u32,
}

/// Port of the `MainPanel_foreachRowFn` function-pointer typedef from
/// `MainPanel.h:31` (`typedef bool(*)(Row*, Arg)`). The C `Arg` union is
/// `object::Arg`, which is not `Copy`, so the payload is passed by shared
/// reference (`&Arg`); callbacks only read it, so this matches the C
/// by-value semantics observationally.
pub type MainPanel_foreachRowFn = fn(&mut Row, &Arg) -> bool;

/// Port of `void MainPanel_updateLabels(MainPanel* this, bool list, bool filter)`
/// from `MainPanel.c:32`. Sets the F5 label to `"List  "`/`"Tree  "` and
/// the F4 label to `"FILTER"`/`"Filter"` on the panel's default bar.
///
/// `MainPanel_getFunctionBar(this)` (`MainPanel.h:33`) is
/// `((Panel*)this)->defaultBar`; here that is `this.super_.defaultBar`
/// (an `Option<FunctionBar>`). The `if let Some(bar)` guard replaces the
/// C unconditional deref — the bar is always set once the panel is built.
pub fn MainPanel_updateLabels(this: &mut MainPanel, list: bool, filter: bool) {
    if let Some(bar) = this.super_.defaultBar.as_mut() {
        FunctionBar_setLabel(bar, KEY_F(5), if list { "List  " } else { "Tree  " });
        FunctionBar_setLabel(bar, KEY_F(4), if filter { "FILTER" } else { "Filter" });
    }
}

/// Port of `static void MainPanel_idSearch(MainPanel* this, int ch)` from
/// `MainPanel.c:38`. Builds a running id from the typed digit
/// (`ch - '0' + idSearch`), selects the first row whose `id` equals it,
/// then advances the accumulator to `id * 10`, resetting it to `0` once it
/// exceeds `10000000`.
///
/// The C `pid_t id = ch - 48 + this->idSearch;` mixes `int` and
/// `unsigned int`; for the digit-key range (`idSearch` bounded by the
/// rollover, `ch` an ASCII digit) the values stay small and non-negative,
/// so `i32` arithmetic reproduces it. Each `Panel_get` result is downcast
/// to [`Row`] (the C `(const Row*)` cast); a non-`Row` item is skipped
/// (unreachable — a `MainPanel` holds only rows).
fn MainPanel_idSearch(this: &mut MainPanel, ch: i32) {
    let id: i32 = ch - 48 + this.idSearch as i32;
    let size = Panel_size(&this.super_);
    for i in 0..size {
        let matches = {
            let obj: &dyn Any = Panel_get(&this.super_, i);
            obj.downcast_ref::<Row>().is_some_and(|row| row.id == id)
        };
        if matches {
            Panel_setSelected(&mut this.super_, i);
            break;
        }
    }
    this.idSearch = (id * 10) as u32;
    if this.idSearch > 10000000 {
        this.idSearch = 0;
    }
}

/// TODO: port of `static const char* MainPanel_getValue(Panel* this, int i)`
/// from `MainPanel.c:54`. Returns `Row_sortKeyString(row)` — a dispatch
/// through the `RowClass.sortKeyString` vtable slot (`Row.h:104`), which
/// `row.rs` does not model (only the `.compare` slot is realized). Left as
/// a stub until that vtable slot exists.
pub fn MainPanel_getValue() {
    todo!("port of MainPanel.c:54 — needs the RowClass.sortKeyString vtable slot (unmodeled)")
}

/// TODO: port of `static HandlerResult MainPanel_eventHandler(Panel* super,
/// int ch)` from `MainPanel.c:59`. The panel key dispatcher. Blocked on
/// unported substrate: `state->host` (`Machine*`, omitted from
/// `action::State`), `IncSet_handleKey`/`IncSet_filter` (incset stubs),
/// `Action_setSortKey`/`Action_setScreenTab` (action stubs),
/// `RowField_keyAt` (row stub), the `ScreenSettings` sort helpers, the
/// `HandlerResult` enum, and the `keys[]` `Htop_Action` dispatch.
pub fn MainPanel_eventHandler() {
    todo!("port of MainPanel.c:59 — needs Machine/IncSet/Action/HandlerResult substrate")
}

/// Port of `int MainPanel_selectedRow(MainPanel* this)` from
/// `MainPanel.c:175`. Returns the selected row's `id`, or `-1` when the
/// list is empty (`Panel_getSelected` returns `None`) or the selected item
/// is not a [`Row`] (the C `(const Row*)` cast; a `MainPanel` holds only
/// rows).
pub fn MainPanel_selectedRow(this: &MainPanel) -> i32 {
    match Panel_getSelected(&this.super_) {
        Some(obj) => {
            let any: &dyn Any = obj;
            any.downcast_ref::<Row>().map_or(-1, |row| row.id)
        }
        None => -1,
    }
}

/// Port of `bool MainPanel_foreachRow(MainPanel* this,
/// MainPanel_foreachRowFn fn, Arg arg, bool* wasAnyTagged)` from
/// `MainPanel.c:180`. Applies `fn` to every tagged row, AND-folding the
/// returned `bool`s into `ok`; if no row was tagged, applies `fn` to the
/// selected row instead. Reports whether any row was tagged through the
/// optional `wasAnyTagged` out-param (C `bool*`).
///
/// The ported `Panel_get` returns an immutable `&dyn Object`, so — like
/// `ColumnsPanel_cancelMoving` — the mutating analog indexes
/// `super.items` directly and downcasts each `&mut dyn Object` to
/// `&mut Row`. A `Vec` element is never null, so the C `if (row)` guard in
/// the tagged loop (which the C omits anyway) is not needed; the selected
/// fallback keeps the C `if (row)` guard via `Panel_getSelected`'s
/// bounds/emptiness check.
pub fn MainPanel_foreachRow(
    this: &mut MainPanel,
    fn_: MainPanel_foreachRowFn,
    arg: Arg,
    wasAnyTagged: Option<&mut bool>,
) -> bool {
    let mut ok = true;
    let mut anyTagged = false;
    let size = Panel_size(&this.super_);
    for i in 0..size {
        let obj: &mut dyn Object = this.super_.items[i as usize].object_mut();
        let any: &mut dyn Any = obj;
        if let Some(row) = any.downcast_mut::<Row>() {
            if row.tag {
                ok &= fn_(row, &arg);
                anyTagged = true;
            }
        }
    }
    if !anyTagged {
        // C: Row* row = (Row*) Panel_getSelected(super); if (row) ...
        let sel = this.super_.selected;
        if sel >= 0 && (sel as usize) < this.super_.items.len() {
            let obj: &mut dyn Object = this.super_.items[sel as usize].object_mut();
            let any: &mut dyn Any = obj;
            if let Some(row) = any.downcast_mut::<Row>() {
                ok &= fn_(row, &arg);
            }
        }
    }

    if let Some(w) = wasAnyTagged {
        *w = anyTagged;
    }

    ok
}

/// TODO: port of `static void MainPanel_drawFunctionBar(Panel* super,
/// bool hideFunctionBar)` from `MainPanel.c:198`. Draws the incremental
/// bar and appends the PAUSED/failed-read markers. `IncSet_drawBar` is now
/// ported, but the `else if (this->state->failedUpdate)` branch reads
/// `State.failedUpdate` (a `const char*`), which the minimal `action::State`
/// omits — so there is no faithful body yet.
pub fn MainPanel_drawFunctionBar() {
    todo!("port of MainPanel.c:198 — needs State.failedUpdate (omitted from action::State)")
}

/// TODO: port of `static void MainPanel_printHeader(Panel* super)` from
/// `MainPanel.c:213`. Calls `Table_printHeader(host->settings, &super->header)`.
/// Blocked on `Table_printHeader` (`table.rs` stub) and `state->host`
/// (`Machine*`, omitted from `action::State`).
pub fn MainPanel_printHeader() {
    todo!("port of MainPanel.c:213 — needs Table_printHeader (table stub) + State.host (Machine)")
}

/// TODO: port of `MainPanel* MainPanel_new(void)` from `MainPanel.c:229`.
/// Allocates the panel and fills the `keys` action table via
/// `Action_setBindings` (`action.rs` stub) + `Platform_setBindings`
/// (`Platform.c` unported). The binding setup is essential to the
/// constructor and cannot run against those stubs. Left as a stub.
pub fn MainPanel_new() {
    todo!("port of MainPanel.c:229 — needs Action_setBindings (stub) + Platform_setBindings (unported)")
}

/// Port of `void MainPanel_setState(MainPanel* this, State* state)` from
/// `MainPanel.c:250`. Stores the shared-state back-pointer.
pub fn MainPanel_setState(this: &mut MainPanel, state: *mut State) {
    this.state = state;
}

/// Port of `void MainPanel_setFunctionBar(MainPanel* this, bool readonly)`
/// from `MainPanel.c:254`. Points the panel's default bar (and the
/// `IncSet`'s default bar) at the read-only or process bar.
///
/// C aliases the one `FunctionBar*` into both `super.defaultBar` and
/// `inc->defaultBar`; the `Vec`-model owns each bar via
/// `Option<FunctionBar>`, so the chosen bar is cloned into both slots —
/// the same clone-reproduces-the-shared-pointer mapping `Panel_init` and
/// `Panel_setDefaultBar` use.
pub fn MainPanel_setFunctionBar(this: &mut MainPanel, readonly: bool) {
    let bar = if readonly {
        this.readonlyBar.clone()
    } else {
        this.processBar.clone()
    };
    this.super_.defaultBar = Some(bar);
    this.inc.defaultBar = this.super_.defaultBar.clone();
}

/// Port of `void MainPanel_delete(Object* object)` from `MainPanel.c:253`:
/// `FunctionBar_delete(processBar); FunctionBar_delete(readonlyBar);
/// IncSet_delete(inc); free(keys); Panel_done(&super); free(this);`.
///
/// Taking `this` by value reproduces `free(this)`. The two owned
/// [`FunctionBar`]s and the owned [`IncSet`] are handed to
/// [`FunctionBar_delete`]/[`IncSet_delete`] (mirroring the C call graph); the
/// `keys` action table is omitted from the struct (`free(keys)` has no analog);
/// and the embedded `super_` [`Panel`] plus the non-owning `state` pointer drop
/// with the remaining fields — the faithful analog of `Panel_done(&super)` (a
/// `Drop` no-op, so the panicking `Panel_done` stub is avoided) and the struct
/// free.
pub fn MainPanel_delete(this: MainPanel) {
    let MainPanel {
        super_,
        inc,
        processBar,
        readonlyBar,
        ..
    } = this;
    FunctionBar_delete(processBar);
    FunctionBar_delete(readonlyBar);
    IncSet_delete(inc);
    let _ = super_;
}

#[cfg(test)]
use crate::ported::panel::PanelItem;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::functionbar::FunctionBar_new;
    use crate::ported::incset::IncSet_new;
    use crate::ported::panel::Panel_new;
    use crate::ported::row::Row;

    /// A zeroed `MainPanel` for tests — not a C function; `MainPanel_new`
    /// (the real constructor) is stubbed on `Action_setBindings`/
    /// `Platform_setBindings`, so tests assemble the struct directly (the
    /// same way `Panel::empty`/`IncMode::empty` back their sibling tests).
    fn blank() -> MainPanel {
        MainPanel {
            super_: Panel_new(1, 1, 1, 1, None),
            state: core::ptr::null_mut(),
            inc: IncSet_new(None),
            processBar: FunctionBar_new(None, None, None),
            readonlyBar: FunctionBar_new(None, None, None),
            idSearch: 0,
        }
    }

    fn row(id: i32) -> Box<dyn Object> {
        Box::new(Row {
            id,
            ..Row::default()
        })
    }

    // ── updateLabels ──────────────────────────────────────────────────

    fn f5f4_bar() -> FunctionBar {
        // A bar carrying the F5 and F4 events MainPanel_updateLabels edits.
        FunctionBar_new(
            Some(&["Tree  ", "Filter"]),
            Some(&["F5", "F4"]),
            Some(&[KEY_F(5), KEY_F(4)]),
        )
    }

    #[test]
    fn update_labels_list_and_filter_active() {
        let mut mp = blank();
        mp.super_.defaultBar = Some(f5f4_bar());
        MainPanel_updateLabels(&mut mp, true, true);
        let bar = mp.super_.defaultBar.as_ref().unwrap();
        assert_eq!(bar.functions[0], "List  "); // F5, list mode
        assert_eq!(bar.functions[1], "FILTER"); // F4, filter active
    }

    #[test]
    fn update_labels_tree_and_filter_inactive() {
        let mut mp = blank();
        mp.super_.defaultBar = Some(f5f4_bar());
        MainPanel_updateLabels(&mut mp, false, false);
        let bar = mp.super_.defaultBar.as_ref().unwrap();
        assert_eq!(bar.functions[0], "Tree  "); // F5, tree mode
        assert_eq!(bar.functions[1], "Filter"); // F4, filter inactive
    }

    #[test]
    fn update_labels_no_bar_is_noop() {
        // defaultBar None -> the Some-guard skips (no C null deref).
        let mut mp = blank();
        mp.super_.defaultBar = None;
        MainPanel_updateLabels(&mut mp, true, true);
        assert!(mp.super_.defaultBar.is_none());
    }

    // ── selectedRow ───────────────────────────────────────────────────

    #[test]
    fn selected_row_empty_is_minus_one() {
        let mp = blank();
        assert_eq!(MainPanel_selectedRow(&mp), -1);
    }

    #[test]
    fn selected_row_returns_selected_id() {
        let mut mp = blank();
        mp.super_.items.push(PanelItem::Owned(row(100)));
        mp.super_.items.push(PanelItem::Owned(row(200)));
        mp.super_.items.push(PanelItem::Owned(row(300)));
        mp.super_.selected = 1;
        assert_eq!(MainPanel_selectedRow(&mp), 200);
        mp.super_.selected = 2;
        assert_eq!(MainPanel_selectedRow(&mp), 300);
    }

    // ── idSearch ──────────────────────────────────────────────────────

    #[test]
    fn id_search_selects_matching_row_and_accumulates() {
        let mut mp = blank();
        // ids 1, 12, 123 so successive digit keys narrow the match.
        mp.super_.items.push(PanelItem::Owned(row(1)));
        mp.super_.items.push(PanelItem::Owned(row(12)));
        mp.super_.items.push(PanelItem::Owned(row(123)));
        // Type '1' -> id = 1 -> selects row 0; idSearch becomes 10.
        MainPanel_idSearch(&mut mp, b'1' as i32);
        assert_eq!(mp.super_.selected, 0);
        assert_eq!(mp.idSearch, 10);
        // Type '2' -> id = 2 + 10 = 12 -> selects row 1; idSearch 120.
        MainPanel_idSearch(&mut mp, b'2' as i32);
        assert_eq!(mp.super_.selected, 1);
        assert_eq!(mp.idSearch, 120);
        // Type '3' -> id = 3 + 120 = 123 -> selects row 2; idSearch 1230.
        MainPanel_idSearch(&mut mp, b'3' as i32);
        assert_eq!(mp.super_.selected, 2);
        assert_eq!(mp.idSearch, 1230);
    }

    #[test]
    fn id_search_no_match_keeps_selection_but_advances() {
        let mut mp = blank();
        mp.super_.items.push(PanelItem::Owned(row(1)));
        mp.super_.items.push(PanelItem::Owned(row(2)));
        mp.super_.selected = 1;
        // Type '9' -> id 9, no row has id 9: selection unchanged, acc 90.
        MainPanel_idSearch(&mut mp, b'9' as i32);
        assert_eq!(mp.super_.selected, 1);
        assert_eq!(mp.idSearch, 90);
    }

    #[test]
    fn id_search_rolls_over_past_ten_million() {
        let mut mp = blank();
        mp.super_.items.push(PanelItem::Owned(row(1)));
        mp.idSearch = 1_000_001; // id = 0 + 1_000_001 -> acc 10_000_010 > 1e7
        MainPanel_idSearch(&mut mp, b'0' as i32);
        assert_eq!(mp.idSearch, 0);
    }

    // ── foreachRow ────────────────────────────────────────────────────

    // Callback: bumps a counter (carried in Arg::V) and stamps the row's
    // indent so we can see which rows it visited. Exercises the &Arg pass.
    fn visit_cb(row: &mut Row, arg: &Arg) -> bool {
        if let Arg::V(p) = arg {
            unsafe {
                *(*p as *mut i32) += 1;
            }
        }
        row.indent = 99;
        true
    }

    // Callback returning false, to check the AND-fold of `ok`.
    fn fail_cb(_row: &mut Row, _arg: &Arg) -> bool {
        false
    }

    #[test]
    fn foreach_row_applies_to_tagged_only() {
        let mut mp = blank();
        for id in [10, 20, 30] {
            mp.super_.items.push(PanelItem::Owned(row(id)));
        }
        // Tag rows 0 and 2.
        {
            let a: &mut dyn Any = mp.super_.items[0].object_mut();
            a.downcast_mut::<Row>().unwrap().tag = true;
        }
        {
            let a: &mut dyn Any = mp.super_.items[2].object_mut();
            a.downcast_mut::<Row>().unwrap().tag = true;
        }
        let mut count: i32 = 0;
        let mut any_tagged = false;
        let arg = Arg::V(&mut count as *mut i32 as *mut core::ffi::c_void);
        let ok = MainPanel_foreachRow(&mut mp, visit_cb, arg, Some(&mut any_tagged));
        assert!(ok);
        assert!(any_tagged);
        assert_eq!(count, 2); // rows 0 and 2 visited
                              // Visited rows stamped; the untagged middle row was not.
        let indent_of = |i: usize, mp: &mut MainPanel| -> i32 {
            let a: &mut dyn Any = mp.super_.items[i].object_mut();
            a.downcast_mut::<Row>().unwrap().indent
        };
        assert_eq!(indent_of(0, &mut mp), 99);
        assert_eq!(indent_of(1, &mut mp), 0);
        assert_eq!(indent_of(2, &mut mp), 99);
    }

    #[test]
    fn foreach_row_falls_back_to_selected_when_none_tagged() {
        let mut mp = blank();
        for id in [10, 20, 30] {
            mp.super_.items.push(PanelItem::Owned(row(id)));
        }
        mp.super_.selected = 1;
        let mut count: i32 = 0;
        let mut any_tagged = true; // must be overwritten to false
        let arg = Arg::V(&mut count as *mut i32 as *mut core::ffi::c_void);
        let ok = MainPanel_foreachRow(&mut mp, visit_cb, arg, Some(&mut any_tagged));
        assert!(ok);
        assert!(!any_tagged);
        assert_eq!(count, 1); // only the selected row
        let a: &mut dyn Any = mp.super_.items[1].object_mut();
        assert_eq!(a.downcast_mut::<Row>().unwrap().indent, 99);
    }

    #[test]
    fn foreach_row_ands_the_callback_results() {
        let mut mp = blank();
        mp.super_.items.push(PanelItem::Owned(row(10)));
        {
            let a: &mut dyn Any = mp.super_.items[0].object_mut();
            a.downcast_mut::<Row>().unwrap().tag = true;
        }
        let ok = MainPanel_foreachRow(&mut mp, fail_cb, Arg::I(0), None);
        assert!(!ok); // fail_cb returned false
    }

    #[test]
    fn foreach_row_wastagged_out_param_is_optional() {
        // Passing None for wasAnyTagged must not panic.
        let mut mp = blank();
        mp.super_.items.push(PanelItem::Owned(row(10)));
        mp.super_.selected = 0;
        let ok = MainPanel_foreachRow(&mut mp, visit_cb, Arg::I(0), None);
        assert!(ok);
    }

    // ── setState / setFunctionBar ─────────────────────────────────────

    #[test]
    fn set_state_stores_pointer() {
        let mut mp = blank();
        let mut st = State {
            host: core::ptr::null_mut(),
            mainPanel: core::ptr::null_mut(),
            header: core::ptr::null_mut(),
            failedUpdate: None,
            pauseUpdate: false,
            hideSelection: false,
            hideMeters: false,
        };
        MainPanel_setState(&mut mp, &mut st as *mut State);
        assert_eq!(mp.state, &mut st as *mut State);
    }

    #[test]
    fn set_function_bar_selects_readonly_or_process() {
        let mut mp = blank();
        mp.processBar = FunctionBar_new(Some(&["PROC"]), Some(&["F1"]), Some(&[1]));
        mp.readonlyBar = FunctionBar_new(Some(&["RO"]), Some(&["F1"]), Some(&[1]));

        MainPanel_setFunctionBar(&mut mp, true);
        assert_eq!(
            mp.super_.defaultBar.as_ref().unwrap().functions,
            vec!["RO".to_string()]
        );
        // The IncSet's default bar tracks the panel's.
        assert_eq!(
            mp.inc.defaultBar.as_ref().unwrap().functions,
            vec!["RO".to_string()]
        );

        MainPanel_setFunctionBar(&mut mp, false);
        assert_eq!(
            mp.super_.defaultBar.as_ref().unwrap().functions,
            vec!["PROC".to_string()]
        );
        assert_eq!(
            mp.inc.defaultBar.as_ref().unwrap().functions,
            vec!["PROC".to_string()]
        );
    }
}
