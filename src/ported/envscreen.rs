//! Port of `EnvScreen.c` — htop's "show a process's environment"
//! `InfoScreen` subclass (the `e` key on the main panel).
//!
//! `EnvScreen` is a thin `InfoScreen` subclass: its struct is literally
//! `{ InfoScreen super; }` (`EnvScreen.h:17`), and all four functions are
//! defined purely in terms of the `InfoScreen` base class and its vtable
//! (`InfoScreenClass`). C names are preserved verbatim (htop uses
//! `CamelCase_snake`), so `non_snake_case` is allowed for the whole
//! module. A C fn `EnvScreen_foo(InfoScreen* this)` ports to a free fn
//! `EnvScreen_foo(this: &mut InfoScreen)` (the shape the module header of
//! `infoscreen.rs` prescribes). The embedded `InfoScreen super` becomes
//! `super_` (the Rust-keyword workaround the ported subclasses use, e.g.
//! `commandscreen.rs`/`tracescreen.rs`).
//!
//! # Ported (no unported substrate)
//!
//! - The [`EnvScreen`] struct (`EnvScreen.h:17`): `{ InfoScreen super; }`.
//! - [`EnvScreen_new`] (`EnvScreen.c:25`) — `xMalloc` the wrapper then
//!   chain-return `InfoScreen_init(&this->super, process, NULL, LINES - 2,
//!   " ")`. `InfoScreen_init` is ported (`infoscreen.rs`); `LINES` maps to
//!   `Ncurses::lines()` (the same source `InfoScreen_init` reads for
//!   `COLS`). The C `Object_setClass(this, Class(EnvScreen))` vtable install
//!   is omitted — the ported `InfoScreen` drops the `Object super` vtable
//!   slot (only the stubbed dispatch would read it); see `infoscreen.rs`.
//! - [`EnvScreen_scan`] (`EnvScreen.c:39`) — the `scan` vtable hook. Saves
//!   the panel selection, prunes it, reads the process's environment via
//!   [`Platform_getProcessEnv`] (`Platform.c:519`, ported in
//!   `linux::platform`), adds one [`InfoScreen_addLine`] per NUL-separated
//!   entry (or the "Could not read" message on `None`), then re-sorts
//!   `lines` and the panel items and restores the selection. Every
//!   dependency (`Panel_getSelectedIndex`/`Panel_prune`/`Panel_setSelected`,
//!   `Process_getPid`, `Vector_insertionSort`, `InfoScreen_addLine`) is
//!   available.
//!
//! # Stubbed (cannot be ported faithfully yet), each naming its blocker
//!
//! - [`EnvScreen_delete`] (`EnvScreen.c:31`) — `free(InfoScreen_done(this))`,
//!   heap-free only. `InfoScreen_done` is itself a `todo!()` (an owned
//!   `InfoScreen` releases its fields via `Drop`), and the owned
//!   `EnvScreen` frees itself the same way, so there is no algorithm to
//!   port (the `InfoScreen_done` / `History_delete` / `Panel_delete`
//!   precedent).
//! - [`EnvScreen_draw`] (`EnvScreen.c:35`) — the `draw` vtable hook: a
//!   single call to `InfoScreen_drawTitled(this, "Environment of process
//!   %d - %s", Process_getPid(this->process), Process_getCommand(
//!   this->process))`. `InfoScreen_drawTitled` (`infoscreen.rs`) and
//!   `Process_getPid` are now ported, but the title's `%s` argument
//!   `Process_getCommand` (`process.rs`, `todo!()` — needs
//!   `settings->showThreadNames`, a field the ported `Settings` subset lacks,
//!   reached through the opaque `Row::host` pointer) is still a stub, so the
//!   hook still cannot be drawn faithfully.
#![allow(non_snake_case)]
#![allow(dead_code)]

use core::ffi::c_int;

use crate::ported::functionbar::Ncurses;
use crate::ported::incset::IncSet_new;
use crate::ported::infoscreen::{InfoScreen, InfoScreen_addLine, InfoScreen_init};
use crate::ported::linux::platform::Platform_getProcessEnv;
use crate::ported::listitem::ListItem_new;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::{Panel_getSelectedIndex, Panel_new, Panel_prune, Panel_setSelected};
use crate::ported::process::{Process, Process_getPid};
use crate::ported::vector::{Vector_insertionSort, Vector_new};

/// Port of `#define VECTOR_DEFAULT_SIZE (10)` from `Vector.h:15` — the
/// initial `lines` vector capacity for the throwaway `InfoScreen` that
/// [`EnvScreen_new`] seeds before `InfoScreen_init` overwrites it (mirrors
/// the same local const in `infoscreen.rs`/`commandscreen.rs`).
const VECTOR_DEFAULT_SIZE: c_int = 10;

/// Port of `struct EnvScreen_` (`EnvScreen.h:17`): `{ InfoScreen super; }`.
/// The embedded base is exposed as `super_` (the Rust-keyword workaround the
/// ported subclasses use).
pub struct EnvScreen {
    /// C `InfoScreen super` — the scrollable info-panel base class.
    pub super_: InfoScreen,
}

/// Port of `EnvScreen* EnvScreen_new(Process* process)` from
/// `EnvScreen.c:25`. `xMalloc(sizeof(EnvScreen))`, install the
/// `Class(EnvScreen)` vtable, then chain-return `InfoScreen_init(&this->super,
/// process, NULL, LINES - 2, " ")`.
///
/// C's `xMalloc` hands `InfoScreen_init` uninitialized storage which it then
/// overwrites field-for-field; the faithful analog seeds a throwaway
/// `InfoScreen` (same bootstrap as the private `InfoScreen::empty`: null
/// `process`, empty `Panel`/`IncSet`, a `ListItem`-typed `lines` vector) and
/// lets `InfoScreen_init` overwrite every field. `LINES` maps to
/// `Ncurses::lines()` (the same terminal-metric source `InfoScreen_init`
/// reads for `COLS`). `NULL` is passed for the function bar so
/// `InfoScreen_init` builds the default `InfoScreen` bar. The
/// `Object_setClass(this, Class(EnvScreen))` vtable install is omitted (the
/// vtable is not modelled; see the module docs). C returns
/// `(EnvScreen*) InfoScreen_init(&this->super, ...)`; since `super` is at
/// offset 0 the cast is identity, so the port returns `this`.
pub fn EnvScreen_new(process: &Process) -> EnvScreen {
    // C: EnvScreen* this = xMalloc(sizeof(EnvScreen));
    // The xMalloc storage is uninitialized; seed a valid throwaway
    // InfoScreen (InfoScreen_init overwrites process/display/inc/lines).
    let list_item_class: &'static ObjectClass = ListItem_new("", 0).klass();
    let mut this = EnvScreen {
        super_: InfoScreen {
            process: core::ptr::null(),
            display: Panel_new(0, 0, 0, 0, None),
            inc: IncSet_new(None),
            lines: Vector_new(list_item_class, true, VECTOR_DEFAULT_SIZE),
        },
    };

    // C: return (EnvScreen*) InfoScreen_init(&this->super, process, NULL, LINES - 2, " ");
    InfoScreen_init(
        &mut this.super_,
        process as *const Process,
        None,
        Ncurses::lines() - 2,
        " ",
    );

    this
}

/// TODO: port of `void EnvScreen_delete(Object* this)` from `EnvScreen.c:31`:
/// `free(InfoScreen_done((InfoScreen*)this))`. Blocked on `InfoScreen_done`
/// (`infoscreen.rs`, `todo!()`) — heap-free only; an owned `EnvScreen`
/// releases its fields via `Drop`, so there is no free-everything algorithm
/// to port (the `InfoScreen_done` / `History_delete` / `Panel_delete`
/// precedent).
pub fn EnvScreen_delete() {
    todo!("port of EnvScreen.c:31 — InfoScreen_done is heap-free only (Drop releases owned fields)")
}

/// TODO: port of `static void EnvScreen_draw(InfoScreen* this)` from
/// `EnvScreen.c:35`. Single call to `InfoScreen_drawTitled(this,
/// "Environment of process %d - %s", Process_getPid(this->process),
/// Process_getCommand(this->process))`. `InfoScreen_drawTitled`
/// (`infoscreen.rs`) and `Process_getPid` (`process.rs`) are ported; blocked
/// only on the title's `%s` argument `Process_getCommand` (`process.rs` stub —
/// needs `settings->showThreadNames`, absent from the ported `Settings`
/// subset, reached via the opaque `Row::host` pointer).
pub fn EnvScreen_draw() {
    todo!("port of EnvScreen.c:35 — needs Process_getCommand (process.rs stub: settings->showThreadNames)")
}

/// Port of `static void EnvScreen_scan(InfoScreen* this)` from
/// `EnvScreen.c:39`. The vtable `scan` hook. C accesses only base
/// `InfoScreen` fields (`this->display`/`this->process`/`this->lines`, no
/// downcast to `EnvScreen`), so the port takes `this: &mut InfoScreen`
/// directly — the shape the `infoscreen.rs` module header prescribes for a
/// `Foo_bar(InfoScreen* this)` C fn.
///
/// Saves the selection (`MAXIMUM(Panel_getSelectedIndex(panel), 0)` ->
/// `.max(0)`), prunes the panel, reads the process's NUL-separated
/// environment block via [`Platform_getProcessEnv`], and — on success —
/// walks each NUL-terminated entry (C's `for (p = env; *p; p = strrchr(p, 0)
/// + 1)`; `str::split('\0')` yields the same entries, and the C loop's `*p`
/// stop condition — halting at the first empty entry / the double-NUL
/// terminator — becomes the `break` on an empty split fragment). On `None`
/// (C `NULL`) it adds the single "Could not read" line. Finally re-sorts the
/// `lines` `Vector` and the panel's items and restores the selection.
///
/// Divergences: C `free(env)` is the owned `String` drop at end of scope.
/// `Vector_insertionSort(panel->items)` has no direct call because the ported
/// `Panel.items` is a plain `Vec<Box<dyn Object>>` (not a `Vector`); it is
/// sorted in place with the same `Object::compare` comparator
/// `Vector_insertionSort` uses (the `openfilesscreen.rs` precedent). The C
/// `Process_getPid(this->process)` derefs the raw `process` back-pointer —
/// an `unsafe { &*this.process }` (the `tracescreen.rs` precedent).
pub fn EnvScreen_scan(this: &mut InfoScreen) {
    // C: Panel* panel = this->display;
    //    int idx = MAXIMUM(Panel_getSelectedIndex(panel), 0);
    let idx = Panel_getSelectedIndex(&this.display).max(0);

    // C: Panel_prune(panel);
    Panel_prune(&mut this.display);

    // C: char* env = Platform_getProcessEnv(Process_getPid(this->process));
    let pid = unsafe { Process_getPid(&*this.process) };
    match Platform_getProcessEnv(pid as libc::pid_t) {
        Some(env) => {
            // C: for (const char* p = env; *p; p = strrchr(p, 0) + 1)
            //        InfoScreen_addLine(this, p);
            // env is a NUL-separated block (double-NUL terminated). Each
            // split fragment is one entry; the first empty fragment is the
            // C loop's `*p == 0` stop (the terminator).
            for entry in env.split('\0') {
                if entry.is_empty() {
                    break;
                }
                InfoScreen_addLine(this, entry);
            }
            // C: free(env); — owned String dropped at end of scope.
        }
        None => {
            // C: InfoScreen_addLine(this, "Could not read process environment.");
            InfoScreen_addLine(this, "Could not read process environment.");
        }
    }

    // C: Vector_insertionSort(this->lines);
    Vector_insertionSort(&mut this.lines);
    // C: Vector_insertionSort(panel->items);  (see the divergence note above)
    this.display.items.sort_by(|a, b| a.compare(&**b).cmp(&0));
    // C: Panel_setSelected(panel, idx);
    Panel_setSelected(&mut this.display, idx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::incset::IncSet_filter;
    use crate::ported::listitem::ListItem;
    use crate::ported::panel::{Panel_get, Panel_headerHeight, Panel_size};
    use crate::ported::process::Process_setPid;
    use crate::ported::vector::Vector_size;

    /// The InfoScreen function-bar labels `InfoScreen_init` installs when
    /// `EnvScreen_new` passes `NULL` for the bar (`InfoScreen.c:25`).
    const INFO_FUNCTIONS: [&str; 4] = ["Search ", "Filter ", "Refresh", "Done   "];

    /// Read the `value` of the `ListItem` shown at panel index `i`.
    fn panel_value(p: &crate::ported::panel::Panel, i: i32) -> String {
        let any: &dyn std::any::Any = Panel_get(p, i);
        any.downcast_ref::<ListItem>().unwrap().value.clone()
    }

    #[test]
    fn new_initializes_infoscreen_base() {
        let p = Process::default();
        let es = EnvScreen_new(&p);
        // process back-pointer stored (points at the passed Process).
        assert_eq!(es.super_.process, &p as *const Process);
        // Fresh screen: no lines scanned yet, panel empty.
        assert_eq!(Vector_size(&es.super_.lines), 0);
        assert_eq!(Panel_size(&es.super_.display), 0);
        // No filter active on a fresh IncSet.
        assert!(IncSet_filter(&es.super_.inc).is_none());
    }

    #[test]
    fn new_geometry_matches_c_panel_new_args() {
        // C: Panel_new(0, 1, COLS, LINES - 2, ...) inside InfoScreen_init,
        // height == LINES - 2 passed by EnvScreen_new.
        let p = Process::default();
        let es = EnvScreen_new(&p);
        assert_eq!(es.super_.display.x, 0);
        assert_eq!(es.super_.display.y, 1);
        assert_eq!(es.super_.display.w, Ncurses::cols());
        assert_eq!(es.super_.display.h, Ncurses::lines() - 2);
        // Header " " installed -> headerHeight 1.
        assert_eq!(Panel_headerHeight(&es.super_.display), 1);
    }

    #[test]
    fn new_builds_default_infoscreen_bar() {
        // NULL bar -> InfoScreen_init synthesizes the InfoScreen default bar.
        let p = Process::default();
        let es = EnvScreen_new(&p);
        let bar = es
            .super_
            .display
            .defaultBar
            .as_ref()
            .expect("default bar built");
        assert_eq!(bar.functions, INFO_FUNCTIONS.to_vec());
        // The IncSet received the same bar content (cloned + moved).
        let inc_bar = es.super_.inc.defaultBar.as_ref().expect("inc default bar");
        assert_eq!(inc_bar.functions, INFO_FUNCTIONS.to_vec());
    }

    #[test]
    fn scan_missing_pid_adds_error_line() {
        // An impossible pid -> Platform_getProcessEnv opens no environ file
        // and returns None on any host, so the C `else` branch runs and adds
        // exactly the single "Could not read" line (deterministic anywhere).
        let mut p = Process::default();
        Process_setPid(&mut p, 2147483646);
        let mut es = EnvScreen_new(&p);

        EnvScreen_scan(&mut es.super_);

        assert_eq!(Vector_size(&es.super_.lines), 1);
        assert_eq!(Panel_size(&es.super_.display), 1);
        assert_eq!(
            panel_value(&es.super_.display, 0),
            "Could not read process environment."
        );
    }

    /// On Linux the current process always has a readable `environ`, so the
    /// scan populates one sorted line per environment entry.
    #[cfg(target_os = "linux")]
    #[test]
    fn scan_self_populates_sorted_env_lines() {
        let mut p = Process::default();
        Process_setPid(&mut p, std::process::id() as i32);
        let mut es = EnvScreen_new(&p);

        EnvScreen_scan(&mut es.super_);

        // At least one env entry recorded, and no trailing empty line from
        // the double-NUL terminator.
        let n = Vector_size(&es.super_.lines);
        assert!(n > 0);
        // Panel items are sorted (Vector_insertionSort(panel->items)).
        let mut prev = panel_value(&es.super_.display, 0);
        for i in 1..Panel_size(&es.super_.display) {
            let cur = panel_value(&es.super_.display, i);
            assert!(prev <= cur, "panel items must be sorted");
            prev = cur;
        }
    }
}
