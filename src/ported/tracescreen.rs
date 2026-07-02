//! Stub scaffold for `TraceScreen.c` — htop's live strace/truss viewer.
//!
//! `TraceScreen` forks an external tracer (`strace -T -tt -s 512 -p PID`
//! on Linux, `truss` on the BSDs), reads its output through a non-blocking
//! pipe, and streams the lines into an [`InfoScreen`] panel with
//! follow/pause toggles. The C struct is
//! `struct { InfoScreen super; FILE* strace; pid_t child; bool tracing;
//! bool contLine; bool follow; bool strace_alive; }` (`TraceScreen.h:19`);
//! every function is dispatched on `TraceScreen*` or on the downcast
//! `InfoScreen* super` and dereferences `this->super.process`,
//! `super->display`, or a sibling `InfoScreen_*` method.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
//!
//! Ported: none. Two hard blockers cover the whole file.
//!
//! Stubbed — blocker A (no `InfoScreen` struct):
//! `InfoScreen` is not modeled yet — `src/ported/infoscreen.rs` is a pure
//! stub scaffold with no `InfoScreen` type and `todo!()` bodies for
//! `InfoScreen_init` / `InfoScreen_done` / `InfoScreen_drawTitled` /
//! `InfoScreen_addLine` / `InfoScreen_appendLine`. Because the C
//! `TraceScreen` struct embeds `InfoScreen super`, the `TraceScreen`
//! struct cannot be faithfully declared, and no function can take a
//! faithful `this: &mut TraceScreen`. Modeling a partial `InfoScreen`
//! here would be inventing substrate that belongs to `infoscreen.rs`, so
//! the struct is documented above rather than defined.
//!
//! Stubbed — blocker B (no libc/nix for raw POSIX syscalls):
//! The crate depends only on `crossterm` + `unicode-width`; there is no
//! `libc`/`nix` dependency and Rust `std` exposes none of the primitives
//! this file is built from — `pipe(2)`, `fork(2)`, `fcntl(2)` with
//! `O_NONBLOCK`, `dup2(2)`, `execlp(3)`, `fdopen(3)`, `select(2)`,
//! `fread(3)`, `kill(2)`, `waitpid(2)`. `std::process::Command` spawns and
//! execs but cannot reproduce the fork/dup2/execlp/shared-nonblocking-pipe
//! sequence, so using it would be an adhoc reimplementation, not a port.
//!
//! Per-function blockers:
//! - `TraceScreen_new` (`TraceScreen.c:37`) — builds the object via
//!   `InfoScreen_init` (stubbed), `Object_setClass` (unported), and
//!   returns an `InfoScreen*`; needs the unmodeled `InfoScreen` struct
//!   (blocker A). `FunctionBar_new` / `CRT_disableDelay` are available,
//!   but the enclosing constructor is not portable without the base.
//! - `TraceScreen_delete` (`TraceScreen.c:48`) — `kill(this->child,
//!   SIGTERM)` + `xWaitpid` (unported) + `fclose` (blocker B) and
//!   `InfoScreen_done` (stubbed, blocker A).
//! - `TraceScreen_draw` (`TraceScreen.c:63`) — `InfoScreen_drawTitled`
//!   (stubbed, blocker A) and `Process_getCommand` (itself still a stub:
//!   needs `Machine`/`Settings` substrate, `Process.c:808`).
//! - `TraceScreen_forkTracer` (`TraceScreen.c:67`) — the raw
//!   pipe/fcntl/fork/dup2/execlp/fdopen tracer launch (blocker B); also
//!   reads `this->super.process` for the pid (blocker A).
//! - `TraceScreen_updateTrace` (`TraceScreen.c:134`) — `select`/`fread`
//!   drain loop (blocker B) feeding `InfoScreen_addLine` /
//!   `InfoScreen_appendLine` (stubbed) and `Panel_setSelected` via
//!   `super->display` (blocker A).
//! - `TraceScreen_onKey` (`TraceScreen.c:185`) — toggles `follow`/
//!   `tracing`, but drives `Panel_setSelected` / `FunctionBar_setLabel`
//!   through `super->display` / `super->display->defaultBar` and calls
//!   `InfoScreen_draw` (blocker A). `Panel_setSelected`,
//!   `FunctionBar_setLabel`, `Panel_size` are available, but they are
//!   reached through the unmodeled `InfoScreen super`.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `TraceScreen* TraceScreen_new(const Process* process)`
/// from `TraceScreen.c:37`. Blocked: returns an `InfoScreen*` built by the
/// stubbed `InfoScreen_init` and needs the unmodeled `InfoScreen` struct
/// (`super`) plus `Object_setClass` (unported).
pub fn TraceScreen_new() {
    todo!("port of TraceScreen.c:37")
}

/// TODO: port of `void TraceScreen_delete(Object* cast)` from
/// `TraceScreen.c:48`. Blocked: `kill`/`xWaitpid`/`fclose` (no libc/nix)
/// and `InfoScreen_done` (stubbed).
pub fn TraceScreen_delete() {
    todo!("port of TraceScreen.c:48")
}

/// TODO: port of `static void TraceScreen_draw(InfoScreen* this)` from
/// `TraceScreen.c:63`. Blocked: `InfoScreen_drawTitled` (stubbed) and
/// `Process_getCommand` (stubbed — needs Machine/Settings substrate).
pub fn TraceScreen_draw() {
    todo!("port of TraceScreen.c:63")
}

/// TODO: port of `bool TraceScreen_forkTracer(TraceScreen* this)` from
/// `TraceScreen.c:67`. Blocked: the raw
/// pipe/fcntl/fork/dup2/execlp/fdopen tracer launch has no faithful analog
/// without a libc/nix dependency; also reads `this->super.process`
/// (unmodeled `InfoScreen`).
pub fn TraceScreen_forkTracer() {
    todo!("port of TraceScreen.c:67")
}

/// TODO: port of `static void TraceScreen_updateTrace(InfoScreen* super)`
/// from `TraceScreen.c:134`. Blocked: `select`/`fread` (no libc/nix)
/// feeding the stubbed `InfoScreen_addLine`/`InfoScreen_appendLine` and
/// `Panel_setSelected` via the unmodeled `super->display`.
pub fn TraceScreen_updateTrace() {
    todo!("port of TraceScreen.c:134")
}

/// TODO: port of `static bool TraceScreen_onKey(InfoScreen* super, int
/// ch)` from `TraceScreen.c:185`. Blocked: drives `Panel_setSelected` /
/// `FunctionBar_setLabel` through `super->display` and calls
/// `InfoScreen_draw` — all require the unmodeled `InfoScreen` struct.
pub fn TraceScreen_onKey() {
    todo!("port of TraceScreen.c:185")
}
