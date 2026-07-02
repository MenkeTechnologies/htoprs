//! Partial port of `CommandScreen.c` — htop's process full-command-line
//! viewer, a thin `InfoScreen` subclass (`CommandScreen.h:16`:
//! `struct CommandScreen_ { InfoScreen super; }`) that shows a process's
//! command line word-wrapped to the terminal width.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module. A C fn
//! `Foo_bar(InfoScreen* this)` ports to a free fn `Foo_bar(this: &mut
//! InfoScreen)` — the shape the `Vector.c`/`History.c` ports use (free
//! fns, not methods). The embedded `InfoScreen super` becomes `super_:
//! InfoScreen` (the Rust-keyword workaround the ported panels use, e.g.
//! `backtracescreen.rs`/`columnspanel.rs`).
//!
//! # Ported (no unported substrate)
//!
//! - The [`CommandScreen`] struct (`CommandScreen.h:16`).
//! - [`CommandScreen_new`] (`CommandScreen.c:81`) — `AllocThis` the
//!   subclass, then chain-return `InfoScreen_init(&this->super, process,
//!   NULL, LINES - 2, " ")`. `InfoScreen_init` is ported
//!   (`infoscreen.rs`); the C `LINES` global maps to `Ncurses::lines()`,
//!   the same source `InfoScreen_init` reads for `COLS`
//!   (`Ncurses::cols()`). C's `AllocThis` allocates uninitialized storage
//!   that `InfoScreen_init` then overwrites; the faithful analog seeds a
//!   throwaway `InfoScreen` (the same bootstrap `InfoScreen::empty` builds
//!   for the `infoscreen.rs` tests, replicated here because `empty` is
//!   private to that module) and lets `InfoScreen_init` overwrite every
//!   field. The `CommandScreen_class` `InfoScreenClass` vtable
//!   (`CommandScreen.c:72`) is data, not a function; the omitted `Object
//!   super` vtable in the ported `InfoScreen` has no slot for it, and only
//!   the stubbed `InfoScreen_run` would dispatch through it.
//!
//! # Stubbed (cannot be ported faithfully yet), each naming its blocker
//!
//! - [`CommandScreen_scan`] (`CommandScreen.c:22`) — `static` in C; the
//!   vtable `scan` hook. Its word-wrap loop is self-contained, but its
//!   only input is `Process_getCommand(this->process)`, and
//!   `Process_getCommand` (`process.rs:575`, `Process.c:808`) is still a
//!   `todo!()` stub blocked on the unported `Machine`/`Settings`
//!   substrate (it reads `this->super.host->settings->showThreadNames`).
//!   With no command bytes to wrap, the loop has no faithful input, so the
//!   whole function stays a stub. `Panel_getSelectedIndex`/`Panel_prune`/
//!   `Panel_setSelected` (`panel.rs`) and `InfoScreen_addLine`
//!   (`infoscreen.rs`) are all available — the sole blocker is
//!   `Process_getCommand`.
//! - [`CommandScreen_draw`] (`CommandScreen.c:68`) — `static` in C; the
//!   vtable `draw` hook. A single call to `InfoScreen_drawTitled(this,
//!   "Command of process %d - %s", Process_getPid(this->process),
//!   Process_getCommand(this->process))`. Blocked on: `InfoScreen_drawTitled`
//!   (`infoscreen.rs`, `todo!()` — needs `String_stripControlChars`, which
//!   is ABSENT from the port-purity snapshot, plus the unported
//!   `IncSet_drawBar`) and `Process_getCommand` (stub, above).
//!   `Process_getPid` (`process.rs:802`) is available.
//! - [`CommandScreen_delete`] (`CommandScreen.c:86`) — the `Object.delete`
//!   hook: `free(InfoScreen_done((InfoScreen*)this))`. Blocked on
//!   `InfoScreen_done` (`infoscreen.rs`, `todo!()`): it is heap-free only,
//!   and an owned `CommandScreen`/`InfoScreen` releases its fields via
//!   `Drop`, so there is no free-everything algorithm to port (same
//!   precedent as `InfoScreen_done`/`History_delete`/`Panel_delete`).
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::functionbar::Ncurses;
use crate::ported::incset::IncSet_new;
use crate::ported::infoscreen::{InfoScreen, InfoScreen_init};
use crate::ported::listitem::ListItem_new;
use crate::ported::object::Object;
use crate::ported::panel::Panel_new;
use crate::ported::process::Process;
use crate::ported::vector::Vector_new;

/// Port of `#define VECTOR_DEFAULT_SIZE (10)` from `Vector.h:15` — the
/// initial `lines` vector capacity for the throwaway `InfoScreen` that
/// [`CommandScreen_new`] seeds before `InfoScreen_init` overwrites it
/// (mirrors the same local const in `infoscreen.rs`).
const VECTOR_DEFAULT_SIZE: core::ffi::c_int = 10;

/// Port of `struct CommandScreen_` (`CommandScreen.h:16`):
/// `{ InfoScreen super; }`. The embedded base is exposed as `super_`
/// (the Rust-keyword workaround the ported subclasses use).
pub struct CommandScreen {
    /// C `InfoScreen super` — the scrollable info-panel base class.
    pub super_: InfoScreen,
}

/// TODO: port of `static void CommandScreen_scan(InfoScreen* this)` from
/// `CommandScreen.c:22`. Word-wraps `Process_getCommand(this->process)` to
/// `COLS`-width lines fed to `InfoScreen_addLine`. Blocked: its only input
/// is `Process_getCommand` (`process.rs:575`, `Process.c:808`), still a
/// `todo!()` stub needing the `Machine`/`Settings` substrate. The wrap
/// loop, `Panel_getSelectedIndex`/`Panel_prune`/`Panel_setSelected`, and
/// `InfoScreen_addLine` are all available; only the command source is not.
pub fn CommandScreen_scan() {
    todo!("port of CommandScreen.c:22 — needs Process_getCommand (Process.c:808 stub: Machine/Settings substrate)")
}

/// TODO: port of `static void CommandScreen_draw(InfoScreen* this)` from
/// `CommandScreen.c:68`. Single call to `InfoScreen_drawTitled(this,
/// "Command of process %d - %s", Process_getPid(this->process),
/// Process_getCommand(this->process))`. Blocked: `InfoScreen_drawTitled`
/// (`infoscreen.rs` stub — `String_stripControlChars` absent from the
/// snapshot + unported `IncSet_drawBar`) and `Process_getCommand` (stub).
/// `Process_getPid` (`process.rs:802`) is available.
pub fn CommandScreen_draw() {
    todo!("port of CommandScreen.c:68 — needs InfoScreen_drawTitled + Process_getCommand stubs")
}

/// Port of `CommandScreen* CommandScreen_new(Process* process)` from
/// `CommandScreen.c:81`. `AllocThis(CommandScreen)` then chain-returns
/// `InfoScreen_init(&this->super, process, NULL, LINES - 2, " ")`.
///
/// C's `AllocThis` hands `InfoScreen_init` uninitialized storage which it
/// then overwrites field-for-field; the faithful analog seeds a throwaway
/// `InfoScreen` (same bootstrap as the private `InfoScreen::empty`: null
/// `process`, empty `Panel`/`IncSet`, a `ListItem`-typed `lines` vector)
/// and lets `InfoScreen_init` overwrite every field. `LINES` maps to
/// `Ncurses::lines()` (the same terminal-metric source `InfoScreen_init`
/// reads for `COLS`). No default `FunctionBar` is supplied (`NULL`), so
/// `InfoScreen_init` builds the `InfoScreen`-default bar.
pub fn CommandScreen_new(process: *const Process) -> CommandScreen {
    // C: CommandScreen* this = AllocThis(CommandScreen);
    // The AllocThis storage is uninitialized; seed a valid throwaway
    // InfoScreen (InfoScreen_init overwrites process/display/inc/lines).
    let list_item_class = ListItem_new("", 0).klass();
    let mut this = CommandScreen {
        super_: InfoScreen {
            process: core::ptr::null(),
            display: Panel_new(0, 0, 0, 0, None),
            inc: IncSet_new(None),
            lines: Vector_new(list_item_class, true, VECTOR_DEFAULT_SIZE),
        },
    };

    // C: return (CommandScreen*) InfoScreen_init(&this->super, process, NULL, LINES - 2, " ");
    InfoScreen_init(&mut this.super_, process, None, Ncurses::lines() - 2, " ");

    this
}

/// TODO: port of `void CommandScreen_delete(Object* this)` from
/// `CommandScreen.c:86`: `free(InfoScreen_done((InfoScreen*)this))`.
/// Blocked on `InfoScreen_done` (`infoscreen.rs`, `todo!()`) — heap-free
/// only; an owned `CommandScreen` releases its fields via `Drop`, so there
/// is no free-everything algorithm to port.
pub fn CommandScreen_delete() {
    todo!("port of CommandScreen.c:86 — InfoScreen_done is heap-free only (Drop releases owned fields)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::functionbar::FunctionBar_new;
    use crate::ported::incset::IncSet_filter;
    use crate::ported::panel::{Panel_headerHeight, Panel_size};
    use crate::ported::vector::Vector_size;

    /// The InfoScreen function-bar labels `InfoScreen_init` installs when
    /// `CommandScreen_new` passes `NULL` for the bar (`InfoScreen.c:25`).
    const INFO_FUNCTIONS: [&str; 4] = ["Search ", "Filter ", "Refresh", "Done   "];

    #[test]
    fn new_initializes_infoscreen_base() {
        let cs = CommandScreen_new(core::ptr::null());
        // process back-pointer stored (null here).
        assert!(cs.super_.process.is_null());
        // Fresh screen: no lines scanned yet, panel empty.
        assert_eq!(Vector_size(&cs.super_.lines), 0);
        assert_eq!(Panel_size(&cs.super_.display), 0);
        // No filter active on a fresh IncSet.
        assert!(IncSet_filter(&cs.super_.inc).is_none());
    }

    #[test]
    fn new_geometry_matches_c_panel_new_args() {
        // C: Panel_new(0, 1, COLS, LINES - 2, ...) inside InfoScreen_init,
        // height == LINES - 2 passed by CommandScreen_new.
        let cs = CommandScreen_new(core::ptr::null());
        assert_eq!(cs.super_.display.x, 0);
        assert_eq!(cs.super_.display.y, 1);
        assert_eq!(cs.super_.display.w, Ncurses::cols());
        assert_eq!(cs.super_.display.h, Ncurses::lines() - 2);
        // Header " " installed -> headerHeight 1.
        assert_eq!(Panel_headerHeight(&cs.super_.display), 1);
    }

    #[test]
    fn new_builds_default_infoscreen_bar() {
        // NULL bar -> InfoScreen_init synthesizes the InfoScreen default bar.
        let cs = CommandScreen_new(core::ptr::null());
        let bar = cs
            .super_
            .display
            .defaultBar
            .as_ref()
            .expect("default bar built");
        assert_eq!(bar.functions, INFO_FUNCTIONS.to_vec());
        // The IncSet received the same bar content (cloned + moved).
        let inc_bar = cs.super_.inc.defaultBar.as_ref().expect("inc default bar");
        assert_eq!(inc_bar.functions, INFO_FUNCTIONS.to_vec());
    }

    #[test]
    fn new_stores_nonnull_process_backpointer() {
        // A non-null process handle is stored verbatim as a raw pointer.
        // (Never dereferenced by any ported CommandScreen function.)
        let sentinel = 0xdead_beef_usize as *const Process;
        let cs = CommandScreen_new(sentinel);
        assert_eq!(cs.super_.process, sentinel);
    }

    /// The `FunctionBar_new` import stays exercised so the default-bar
    /// comparison above is meaningful (labels originate there).
    #[test]
    fn info_functions_match_functionbar_labels() {
        let bar = FunctionBar_new(Some(&INFO_FUNCTIONS[..]), None, None);
        assert_eq!(bar.functions, INFO_FUNCTIONS.to_vec());
    }
}
