//! Port of `InfoScreen.c` — htop's abstract scrollable info panel (the
//! base class for the Command / Env / OpenFiles / ProcessLocks / Trace /
//! Backtrace screens).
//!
//! An `InfoScreen` wraps a scrollable `Panel` of `ListItem` lines, an
//! `IncSet` (incremental search/filter), and a backing `Vector` of every
//! line (the filter narrows the visible `Panel` against this full set).
//! Concrete screens plug in via the `InfoScreenClass` vtable
//! (`scan`/`draw`/`onErr`/`onKey`) which `InfoScreen_run` dispatches
//! through `As_InfoScreen(this)`.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module. Each C function
//! takes `InfoScreen* this`; the faithful analog is a free fn taking
//! `this: &mut InfoScreen` (the shape the `Vector.c`/`History.c` ports
//! use: free fns, not methods).
//!
//! Ported (self-contained, no unported substrate):
//! - (none) — every function in this file bottoms out on substrate that
//!   is not yet modeled in the ported tree. See "Stubbed" for the
//!   specific blocker behind each.
//!
//! The `InfoScreen` struct itself is deliberately NOT modeled here: its
//! `Vector* lines` field is a `Vector` (unported — `vector.rs` holds only
//! the sort/search helpers, no `Vector` struct/`Vector_new`), and its
//! `Object super` is the `InfoScreenClass` vtable
//! (`scan`/`draw`/`onErr`/`onKey`), which the ported tree does not model.
//! Defining the struct now would require inventing both, so it waits on
//! that substrate — matching the `incset.rs` precedent that treats
//! `Vector` as an unported blocker rather than papering over it.
//!
//! Stubbed (cannot be ported faithfully yet), each naming its blocker:
//! - `InfoScreen_init` (`InfoScreen.c:31`) — builds `this->lines` via
//!   `Vector_new` (the `Vector` type is unported), and passes ONE
//!   `FunctionBar*` to BOTH `Panel_new` and `IncSet_new` so they share it
//!   (`InfoScreen_run` later mutates it in place through
//!   `this->display->defaultBar`). The ported `Panel_new`/`IncSet_new`
//!   each take an owned `Option<FunctionBar>`, so the owned model cannot
//!   faithfully reproduce that shared, mutated bar.
//! - `InfoScreen_done` (`InfoScreen.c:43`) — `Panel_delete` +
//!   `IncSet_delete` + `Vector_delete` + `free`, i.e. heap-free only. An
//!   owned `InfoScreen` would release its fields via `Drop`, so there is
//!   no algorithm to port (same precedent as `IncSet_delete` /
//!   `History_delete` / `Panel_delete`).
//! - `InfoScreen_drawTitled` (`InfoScreen.c:50`) — a pure draw
//!   side-effect: `attrset`/`mvhline`/`mvaddstr`/`CRT_colors`,
//!   `Panel_draw`, and `IncSet_drawBar` (itself an unported `todo!()`
//!   stub, `incset.rs:305`), plus `String_stripControlChars` (unported in
//!   `xutils.rs`). No splittable pure logic.
//! - `InfoScreen_addLine` (`InfoScreen.c:73`) — needs `ListItem_new`
//!   (`todo!()` stub, `listitem.rs:107`), `Vector_add`/`Vector_get`/
//!   `Vector_size` (`Vector` unported), and `IncSet_filter` (`todo!()`
//!   stub, `incset.rs:319`, blocked on `LineEditor_getText`).
//! - `InfoScreen_appendLine` (`InfoScreen.c:81`) — needs `Vector_size`/
//!   `Vector_get` (`Vector` unported), `InfoScreen_addLine` (stub above),
//!   and `IncSet_filter` (`todo!()` stub, `incset.rs:319`). `String_contains_i`
//!   and `ListItem_append`/`Panel_add`/`Panel_get`/`Panel_size` are
//!   ported, but the Vector and filter reads are not.
//! - `InfoScreen_run` (`InfoScreen.c:96`) — the ncurses main loop:
//!   `Panel_getCh`, `getmouse`/`MEVENT`, `clear()`, and the
//!   `IncSet_handleKey`/`IncSet_activate`/`IncSet_drawBar` incremental-set
//!   handlers (all `todo!()` stubs in `incset.rs`), plus `Vector_prune`
//!   (`Vector` unported) and the `As_InfoScreen` vtable dispatch
//!   (`InfoScreen_scan`/`InfoScreen_draw`/`InfoScreen_onErr`/
//!   `InfoScreen_onKey`) which is not modeled.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `InfoScreen* InfoScreen_init(InfoScreen* this, const
/// Process* process, FunctionBar* bar, int height, const char*
/// panelHeader)` from `InfoScreen.c:31`. Blocked on the unported `Vector`
/// type (`this->lines = Vector_new(...)`) and on the shared `FunctionBar*`
/// that C hands to BOTH `Panel_new` and `IncSet_new` — the owned
/// `Option<FunctionBar>` model cannot reproduce a single bar aliased by
/// panel and inc that `InfoScreen_run` later mutates in place.
pub fn InfoScreen_init() {
    todo!("port of InfoScreen.c:31 — needs Vector (unported) + shared FunctionBar aliasing")
}

/// TODO: port of `InfoScreen* InfoScreen_done(InfoScreen* this)` from
/// `InfoScreen.c:43`. `Panel_delete` + `IncSet_delete` + `Vector_delete` +
/// `free` — heap-free only. An owned `InfoScreen` releases its fields via
/// `Drop`, so there is no algorithm to port (same as `IncSet_delete` /
/// `History_delete`).
pub fn InfoScreen_done() {
    todo!("port of InfoScreen.c:43 — Drop releases owned fields")
}

/// TODO: port of `void InfoScreen_drawTitled(InfoScreen* this, const char*
/// fmt, ...)` from `InfoScreen.c:50`. Pure ncurses draw
/// (`attrset`/`mvhline`/`mvaddstr`/`CRT_colors`, `Panel_draw`,
/// `IncSet_drawBar`) plus `String_stripControlChars` (unported).
/// `IncSet_drawBar` is itself a `todo!()` stub (`incset.rs:305`).
pub fn InfoScreen_drawTitled() {
    todo!("port of InfoScreen.c:50 — ncurses draw; IncSet_drawBar + String_stripControlChars unported")
}

/// TODO: port of `void InfoScreen_addLine(InfoScreen* this, const char*
/// line)` from `InfoScreen.c:73`. Blocked on `ListItem_new` (`todo!()`
/// stub, `listitem.rs:107`), the unported `Vector` type
/// (`Vector_add`/`Vector_get`/`Vector_size`), and `IncSet_filter`
/// (`todo!()` stub, `incset.rs:319`).
pub fn InfoScreen_addLine() {
    todo!("port of InfoScreen.c:73 — needs ListItem_new + Vector + IncSet_filter (all unported)")
}

/// TODO: port of `void InfoScreen_appendLine(InfoScreen* this, const char*
/// line)` from `InfoScreen.c:81`. Blocked on the unported `Vector` type
/// (`Vector_size`/`Vector_get`), `InfoScreen_addLine` (stub above), and
/// `IncSet_filter` (`todo!()` stub, `incset.rs:319`). `ListItem_append`
/// and `Panel_add`/`Panel_get`/`Panel_size`/`String_contains_i` are
/// ported, but the Vector and filter reads are not.
pub fn InfoScreen_appendLine() {
    todo!("port of InfoScreen.c:81 — needs Vector + IncSet_filter (both unported)")
}

/// TODO: port of `void InfoScreen_run(InfoScreen* this)` from
/// `InfoScreen.c:96`. The ncurses main loop: `Panel_getCh`,
/// `getmouse`/`MEVENT`, `clear()`, the
/// `IncSet_handleKey`/`IncSet_activate`/`IncSet_drawBar` handlers (all
/// `todo!()` stubs in `incset.rs`), `Vector_prune` (`Vector` unported),
/// and the `As_InfoScreen` vtable dispatch
/// (`InfoScreen_scan`/`InfoScreen_draw`/`InfoScreen_onErr`/
/// `InfoScreen_onKey`), which is not modeled.
pub fn InfoScreen_run() {
    todo!("port of InfoScreen.c:96 — ncurses loop; IncSet handlers + InfoScreenClass vtable unported")
}
