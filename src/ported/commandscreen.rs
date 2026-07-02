//! Stub scaffold for `CommandScreen.c` — NOT yet portable.
//!
//! `CommandScreen` is a thin `InfoScreen` subclass (`CommandScreen.h:16`:
//! `struct CommandScreen_ { InfoScreen super; }`) that shows a process's
//! full command line, word-wrapped to the terminal width. Every one of
//! its four C functions is an `InfoScreen*` virtual-table method or ctor
//! whose body dereferences the `InfoScreen` struct and calls into the
//! `InfoScreen` API. That entire substrate is unported, so none of the
//! four can be ported faithfully yet — porting any of them now would
//! require inventing the `InfoScreen` struct here, which
//! `InfoScreen.c`/`.h` owns (out of scope: edit only this file).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module. A C fn
//! `Foo_bar(InfoScreen* this)` ports to a free fn
//! `Foo_bar(this: &mut InfoScreen)`, but the `InfoScreen` type does not
//! exist in the ported tree yet, so the stubs keep their zero-arg
//! placeholder signatures until that substrate lands.
//!
//! Ported (self-contained, no unported substrate):
//! - none. Every function in this file depends on the `InfoScreen`
//!   substrate below.
//!
//! Stubbed (cannot be ported faithfully yet) — the shared blocker is the
//! `InfoScreen` struct (`InfoScreen.h:22`: `Object super; const Process*
//! process; Panel* display; IncSet* inc; Vector* lines;`), which is not
//! modeled anywhere in `src/ported/`, and the `InfoScreen` API in
//! `infoscreen.rs`, which is entirely `todo!()` (`InfoScreen_init`,
//! `InfoScreen_done`, `InfoScreen_drawTitled`, `InfoScreen_addLine`):
//! - `CommandScreen_scan` (`CommandScreen.c:22`) — `static` in C; the
//!   vtable `scan` hook. Reads `this->display` (`Panel*`) and
//!   `this->process` (`const Process*`) off the unmodeled `InfoScreen`
//!   struct, word-wraps `Process_getCommand(this->process)` to
//!   `COLS`-width lines, and feeds each to `InfoScreen_addLine`. Blocked
//!   on: `InfoScreen` struct fields; `InfoScreen_addLine` (stub);
//!   `Process_getCommand` (`Process.c:808`, itself a stub — needs
//!   Machine/Settings substrate); the `COLS` curses global (unmodeled).
//!   `Panel_prune`/`Panel_getSelectedIndex`/`Panel_setSelected` do exist
//!   (`panel.rs`), but there is no `InfoScreen.display` `Panel` to pass
//!   them.
//! - `CommandScreen_draw` (`CommandScreen.c:68`) — `static` in C; the
//!   vtable `draw` hook. A single call to `InfoScreen_drawTitled(this,
//!   "Command of process %d - %s", Process_getPid(this->process),
//!   Process_getCommand(this->process))`. Blocked on: `InfoScreen` struct;
//!   `InfoScreen_drawTitled` (stub); `Process_getCommand` (stub).
//!   `Process_getPid` (`process.rs:793`) is available.
//! - `CommandScreen_new` (`CommandScreen.c:81`) — allocates a
//!   `CommandScreen` (`AllocThis`) and hands it to `InfoScreen_init(&this
//!   ->super, process, NULL, LINES - 2, " ")`. Blocked on: the
//!   `CommandScreen`/`InfoScreen` structs; the `CommandScreen_class`
//!   `InfoScreenClass` vtable (`InfoScreen.h:35`, unmodeled); `AllocThis`;
//!   `InfoScreen_init` (stub); the `LINES` curses global (unmodeled).
//! - `CommandScreen_delete` (`CommandScreen.c:86`) — the `Object.delete`
//!   hook: `free(InfoScreen_done((InfoScreen*)this))`. In Rust `Drop`
//!   frees owned fields, so the free-everything shape has no analog once
//!   `InfoScreen` is modeled; but the call is `InfoScreen_done` (stub),
//!   which tears down the `Panel`/`Vector`/`IncSet` and returns the
//!   struct pointer to free. Blocked on: `InfoScreen` struct;
//!   `InfoScreen_done` (stub).
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `static void CommandScreen_scan(InfoScreen* this` from `CommandScreen.c:22`.
/// Blocked: unmodeled `InfoScreen` struct + `InfoScreen_addLine`/`Process_getCommand` stubs + `COLS`.
pub fn CommandScreen_scan() {
    todo!("port of CommandScreen.c:22 — needs InfoScreen struct, InfoScreen_addLine, Process_getCommand, COLS")
}

/// TODO: port of `static void CommandScreen_draw(InfoScreen* this` from `CommandScreen.c:68`.
/// Blocked: unmodeled `InfoScreen` struct + `InfoScreen_drawTitled`/`Process_getCommand` stubs.
pub fn CommandScreen_draw() {
    todo!("port of CommandScreen.c:68 — needs InfoScreen struct, InfoScreen_drawTitled, Process_getCommand")
}

/// TODO: port of `CommandScreen* CommandScreen_new(Process* process` from `CommandScreen.c:81`.
/// Blocked: unmodeled `CommandScreen`/`InfoScreen` structs + `CommandScreen_class` vtable + `InfoScreen_init` stub + `LINES`.
pub fn CommandScreen_new() {
    todo!("port of CommandScreen.c:81 — needs InfoScreen struct, InfoScreenClass vtable, InfoScreen_init, LINES")
}

/// TODO: port of `void CommandScreen_delete(Object* this` from `CommandScreen.c:86`.
/// Blocked: unmodeled `InfoScreen` struct + `InfoScreen_done` stub.
pub fn CommandScreen_delete() {
    todo!("port of CommandScreen.c:86 — needs InfoScreen struct, InfoScreen_done")
}
