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
//! # Ported (calling through a stubbed command source)
//!
//! - [`CommandScreen_scan`] (`CommandScreen.c:22`) — `static` in C; the
//!   vtable `scan` hook. Its word-wrap loop over
//!   `Process_getCommand(this->process)`, feeding `InfoScreen_addLine` and
//!   restoring the selection across `Panel_prune`, is ported in full.
//!   `Panel_getSelectedIndex`/`Panel_prune`/`Panel_setSelected` (`panel.rs`)
//!   and `InfoScreen_addLine` (`infoscreen.rs`) are all available;
//!   `Process_getCommand` (`process.rs`, `Process.c:831`) still `todo!()`s
//!   on the `Settings` substrate, so a real scan panics through it — the
//!   faithful chain-of-stubs the ported `Process_getSortKey` already uses.
//! - [`CommandScreen_draw`] (`CommandScreen.c:68`) — `static` in C; the
//!   vtable `draw` hook. A single `InfoScreen_drawTitled(this,
//!   "Command of process %d - %s", Process_getPid(this->process),
//!   Process_getCommand(this->process))`, now ported: `InfoScreen_drawTitled`
//!   (`infoscreen.rs`) and `Process_getPid` (`process.rs`) are both
//!   available; `Process_getCommand` is the same stubbed command source.
//!
//! # Stubbed (cannot be ported faithfully yet), each naming its blocker
//!
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
use crate::ported::infoscreen::{
    InfoScreen, InfoScreen_addLine, InfoScreen_drawTitled, InfoScreen_init,
};
use crate::ported::listitem::ListItem_new;
use crate::ported::object::Object;
use crate::ported::panel::{Panel_getSelectedIndex, Panel_new, Panel_prune, Panel_setSelected};
use crate::ported::process::{Process, Process_getCommand, Process_getPid};
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

/// Port of `static void CommandScreen_scan(InfoScreen* this)` from
/// `CommandScreen.c:22` — the vtable `scan` hook. Word-wraps
/// `Process_getCommand(this->process)` to `COLS`-width lines (min 40) fed
/// to [`InfoScreen_addLine`], preserving the selected index across the
/// `Panel_prune`. C's `Panel* panel = this->display` alias becomes a
/// reborrow of `this.display` at each use point (the panel is only touched
/// before and after the wrap loop, so no live borrow spans the
/// `InfoScreen_addLine` calls). The scratch `char line[line_maxlen + 1]`
/// buffer maps to a `Vec<u8>` whose used length is tracked by `line_offset`
/// (C's `memmove(line, line + line_len, line_offset)` shift becomes
/// `Vec::copy_within`). The command bytes come from [`Process_getCommand`]
/// (still a `todo!()` stub pending the `Settings` substrate), so a real
/// scan panics through it — the same faithful chain-of-stubs the ported
/// [`Process_getSortKey`](crate::ported::process::Process_getSortKey) uses;
/// `None` maps to the empty command (nothing to wrap).
pub fn CommandScreen_scan(this: &mut InfoScreen) {
    // C: Panel* panel = this->display;
    //    int idx = MAXIMUM(Panel_getSelectedIndex(panel), 0);
    let idx = Panel_getSelectedIndex(&this.display).max(0);
    // C: Panel_prune(panel);
    Panel_prune(&mut this.display);

    // C: const char* p = Process_getCommand(this->process);
    let p = Process_getCommand(unsafe { &*this.process });
    // C treats `p` as a NUL-terminated char*; iterate the command bytes.
    let bytes: &[u8] = match p {
        Some(b) => b,
        None => &[],
    };

    // C: size_t line_maxlen = COLS < 40 ? 40 : COLS;
    let cols = Ncurses::cols();
    let line_maxlen: usize = if cols < 40 { 40 } else { cols as usize };
    // C: size_t line_offset = 0; size_t last_space = 0;
    let mut line_offset: usize = 0;
    let mut last_space: usize = 0;
    // C: char* line = xCalloc(line_maxlen + 1, sizeof(char));
    let mut line: Vec<u8> = vec![0u8; line_maxlen + 1];

    // C: for (; *p != '\0'; p++) { ... }
    for &ch in bytes {
        if line_offset >= line_maxlen {
            debug_assert!(line_offset <= line_maxlen);
            debug_assert!(last_space <= line_maxlen);

            // C: size_t line_len = last_space <= 0 ? line_offset : last_space;
            //    (last_space is size_t, so `<= 0` means `== 0`.)
            let line_len = if last_space == 0 {
                line_offset
            } else {
                last_space
            };
            // C: char tmp = line[line_len]; line[line_len] = '\0';
            //    InfoScreen_addLine(this, line); line[line_len] = tmp;
            {
                let s = String::from_utf8_lossy(&line[..line_len]);
                InfoScreen_addLine(this, &s);
            }

            debug_assert!(line_len <= line_offset);
            // C: line_offset -= line_len; memmove(line, line + line_len, line_offset);
            let old_offset = line_offset;
            line_offset -= line_len;
            line.copy_within(line_len..old_offset, 0);

            // C: last_space = 0;
            last_space = 0;
        }

        // C: line[line_offset++] = *p;
        line[line_offset] = ch;
        line_offset += 1;
        // C: if (*p == ' ') last_space = line_offset;
        if ch == b' ' {
            last_space = line_offset;
        }
    }

    // C: if (line_offset > 0) { line[line_offset] = '\0'; InfoScreen_addLine(this, line); }
    if line_offset > 0 {
        let s = String::from_utf8_lossy(&line[..line_offset]);
        InfoScreen_addLine(this, &s);
    }

    // C: free(line); — the Vec drops here.

    // C: Panel_setSelected(panel, idx);
    Panel_setSelected(&mut this.display, idx);
}

/// Port of `static void CommandScreen_draw(InfoScreen* this)` from
/// `CommandScreen.c:68` — the vtable `draw` hook. A single
/// [`InfoScreen_drawTitled`] call with the C `printf`-style
/// `"Command of process %d - %s"` pre-formatted (the ported
/// `InfoScreen_drawTitled` takes an already-built `&str`, the standard
/// `xSnprintf`/`vsnprintf` idiom). `%d` is [`Process_getPid`] and `%s` is
/// [`Process_getCommand`] (a `const char*`, rendered lossily from its
/// bytes; `None` -> empty). `Process_getCommand` is still a `todo!()` stub,
/// so a real draw panics through it — the faithful chain-of-stubs wiring.
pub fn CommandScreen_draw(this: &mut InfoScreen) {
    // C: InfoScreen_drawTitled(this, "Command of process %d - %s",
    //        Process_getPid(this->process), Process_getCommand(this->process));
    let pid = Process_getPid(unsafe { &*this.process });
    let cmd = Process_getCommand(unsafe { &*this.process });
    let cmd = match cmd {
        Some(b) => String::from_utf8_lossy(b).into_owned(),
        None => String::new(),
    };
    let title = format!("Command of process {} - {}", pid, cmd);
    InfoScreen_drawTitled(this, &title);
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
