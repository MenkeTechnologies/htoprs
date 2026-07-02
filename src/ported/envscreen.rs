//! Port surface for `EnvScreen.c` — htop's "show a process's environment"
//! `InfoScreen` subclass (the `e` key on the main panel).
//!
//! `EnvScreen` is a thin `InfoScreen` subclass: its struct is literally
//! `{ InfoScreen super; }` (`EnvScreen.h:17`), and all four functions are
//! defined purely in terms of the `InfoScreen` base class and its vtable
//! (`InfoScreenClass`). C names are preserved verbatim (htop uses
//! `CamelCase_snake`), so `non_snake_case` is allowed for the whole
//! module. A C fn `EnvScreen_foo(InfoScreen* this)` would port to a free
//! fn `EnvScreen_foo(this: &mut InfoScreen)` — but the `InfoScreen`
//! substrate it needs does not exist yet (see below).
//!
//! Ported: none. Every function in this file is blocked on the same
//! unported substrate.
//!
//! Stubbed (cannot be ported faithfully yet — shared blocker):
//! The whole `InfoScreen` base class is unported. `src/ported/infoscreen.rs`
//! is still a stub scaffold: there is no `InfoScreen` struct modeled
//! anywhere (it owns the `.display` [`Panel`], `.lines` [`Vector`], and
//! `.process` fields these functions read), and `InfoScreen_init`
//! (`InfoScreen.c:31`), `InfoScreen_done` (`InfoScreen.c:43`),
//! `InfoScreen_drawTitled` (`InfoScreen.c:50`), and `InfoScreen_addLine`
//! (`InfoScreen.c:71`) are all still `todo!()`. Porting any EnvScreen fn
//! now would require inventing the `InfoScreen` struct here — but that
//! struct is owned by `InfoScreen.h`, not `EnvScreen.c`, so modeling it
//! in this module would violate the "only define a struct the C file owns"
//! rule and fork the substrate. Per-function blockers:
//! - `EnvScreen_new` (`EnvScreen.c:25`) — allocates the `EnvScreen`
//!   wrapper (`xMalloc`), sets its `Object` class vtable
//!   (`Object_setClass`/`Class(EnvScreen)` — htop's runtime class system,
//!   also unported), and delegates to `InfoScreen_init` (stubbed) with an
//!   `LINES - 2` height taken from the ncurses global `LINES`. Blocked on
//!   the `InfoScreen`/`Object`-class substrate.
//! - `EnvScreen_delete` (`EnvScreen.c:31`) — `free(InfoScreen_done(this))`:
//!   a C manual teardown built on `InfoScreen_done` (stubbed). No faithful
//!   analog until `InfoScreen` is modeled; in Rust the owning struct's
//!   `Drop` would free the fields, so this cannot be ported in isolation.
//! - `EnvScreen_draw` (`EnvScreen.c:35`) — calls the variadic
//!   `InfoScreen_drawTitled(this, "Environment of process %d - %s",
//!   Process_getPid(this->process), Process_getCommand(this->process))`.
//!   `Process_getPid`/`Process_getCommand` are ported, but it reads the
//!   `InfoScreen.process` field (struct not modeled) and calls the stubbed
//!   variadic ncurses drawer. Blocked on `InfoScreen`.
//! - `EnvScreen_scan` (`EnvScreen.c:39`) — reads `this->display`
//!   (`Panel*`) and `this->lines` (`Vector*`) off the unmodeled
//!   `InfoScreen`, and depends on `Platform_getProcessEnv` (not ported —
//!   returns the process's NUL-separated environment block) and
//!   `InfoScreen_addLine` (stubbed). `Panel_prune`, `Panel_getSelectedIndex`,
//!   `Panel_setSelected`, and `Vector_insertionSort` exist, but the
//!   function cannot be assembled without the `InfoScreen` fields, the
//!   platform env reader, and the line-appender.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `EnvScreen* EnvScreen_new(Process* process` from `EnvScreen.c:25`.
/// Blocked on the unported `InfoScreen`/`Object`-class substrate — see module header.
pub fn EnvScreen_new() {
    todo!("port of EnvScreen.c:25")
}

/// TODO: port of `void EnvScreen_delete(Object* this` from `EnvScreen.c:31`.
/// Blocked on the stubbed `InfoScreen_done` — see module header.
pub fn EnvScreen_delete() {
    todo!("port of EnvScreen.c:31")
}

/// TODO: port of `static void EnvScreen_draw(InfoScreen* this` from `EnvScreen.c:35`.
/// Blocked on the unmodeled `InfoScreen` struct + stubbed `InfoScreen_drawTitled` — see module header.
pub fn EnvScreen_draw() {
    todo!("port of EnvScreen.c:35")
}

/// TODO: port of `static void EnvScreen_scan(InfoScreen* this` from `EnvScreen.c:39`.
/// Blocked on the unmodeled `InfoScreen` struct, unported `Platform_getProcessEnv`,
/// and stubbed `InfoScreen_addLine` — see module header.
pub fn EnvScreen_scan() {
    todo!("port of EnvScreen.c:39")
}
