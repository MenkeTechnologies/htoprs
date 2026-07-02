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
//! # Struct mapping (`InfoScreen.h:22`)
//!
//! `Object super` — the `InfoScreenClass` vtable slots
//! (`scan`/`draw`/`onErr`/`onKey`) — is omitted: the only consumer is the
//! stubbed [`InfoScreen_run`] main loop (the `As_InfoScreen` dispatch),
//! matching how `incset.rs` omits its `Panel*` back-pointer because only
//! stubbed functions read it. `const Process* process` is a raw
//! `*const Process` back-pointer (the `MainPanel.state` /
//! `BacktracePanel.processes` precedent — a borrowed handle owned
//! elsewhere, kept raw so the struct stays `'static`). `Panel* display`,
//! `IncSet* inc`, and `Vector* lines` are owned values now that
//! `panel.rs` / `incset.rs` / `vector.rs` all model their types.
//!
//! # Ported
//!
//! - The [`InfoScreen`] struct (`InfoScreen.h:22`).
//! - [`InfoScreen_init`] (`InfoScreen.c:31`) — builds the `Panel`, the
//!   `IncSet`, and the `lines` `Vector`, then installs the panel header.
//! - [`InfoScreen_addLine`] (`InfoScreen.c:73`) — `ListItem_new` +
//!   `Vector_add` + the `IncSet_filter` gate that decides whether the new
//!   line is also shown in the panel.
//!
//! ## Owned-value divergences (documented, per "port what you can")
//!
//! - **Shared `FunctionBar`.** C hands ONE `FunctionBar*` to BOTH
//!   `Panel_new` and `IncSet_new`, so `InfoScreen_run` can later mutate it
//!   in place through `this->display->defaultBar` and have the `IncSet`
//!   observe it. The ported `Panel_new`/`IncSet_new` each take an *owned*
//!   `Option<FunctionBar>`, so [`InfoScreen_init`] gives the panel a clone
//!   and moves the original into the `IncSet`: identical bar *content* in
//!   both, but not one aliased, in-place-mutated object. The only code
//!   that mutates the shared bar is [`InfoScreen_run`] (stubbed), so no
//!   ported behavior observes the difference. Same clone precedent as
//!   `Panel_init` seeding `defaultBar`/`currentBar` from one `fuBar`.
//! - **`COLS`.** C passes the ncurses `COLS` global as the panel width;
//!   the ported analog is `functionbar::Ncurses::cols()` (the terminal
//!   column count), the same source `Panel_draw`/`FunctionBar_draw` read.
//! - **`Vector_type(this->display->items)`.** C types the `lines` vector
//!   with the panel's item class (`Class(ListItem)`, set at `Panel_new`).
//!   The ported `Panel` drops per-item typing (`items` is an untyped
//!   `Vec<Box<dyn Object>>`), so the `ListItem` class is recovered from an
//!   instance's `Object::klass()` — the same class `Vector_type` would
//!   yield.
//! - **`Panel_add(display, Vector_get(lines, last))`** (in
//!   [`InfoScreen_addLine`]). C adds the *same* `Object*` to the panel
//!   that it just put in `lines` (htop's "weak panel": the panel is a
//!   filtered view aliasing the `lines` objects). A `Box<dyn Object>` is a
//!   unique owner and cannot sit in two `Vec`s at once, so the panel
//!   receives an independent `ListItem` with identical `value`/`key`.
//!   That is faithful for the panel's role — `Panel_draw` and
//!   `IncSet_getListItemValue` only read the item's `value` — and the sole
//!   place C relies on the shared *identity* is [`InfoScreen_appendLine`],
//!   which is stubbed for exactly that reason.
//!
//! # Stubbed (cannot be ported faithfully yet), each naming its blocker
//!
//! - [`InfoScreen_done`] (`InfoScreen.c:43`) — `Panel_delete` +
//!   `IncSet_delete` + `Vector_delete` + `free`, i.e. heap-free only. An
//!   owned `InfoScreen` releases its fields via `Drop`, so there is no
//!   algorithm to port (same precedent as `IncSet_delete` /
//!   `History_delete` / `Panel_delete`).
//! - [`InfoScreen_appendLine`] (`InfoScreen.c:81`) — depends on the
//!   weak-panel shared-`Object*` identity that owned `Box`es cannot model.
//!   Its `displayLast != last` test is a pointer-identity compare between
//!   the panel's last item and `lines`' last item ("is `lines`' last line
//!   currently shown as the panel's last?"), and its in-place
//!   `ListItem_append` mutation relies on the panel and `lines` aliasing
//!   the same object. With independent clones (see the `addLine`
//!   divergence above) the two are never pointer-equal and a mutation of
//!   `lines` does not reach the panel copy, so the C dedup/in-place path
//!   cannot be reproduced. C also re-tests the filter against the *newly
//!   appended fragment* (`String_contains_i(line, incFilter, true)` where
//!   `line` is only the appended text), which a value-recompute view would
//!   not match. The `lines`-side growth (`ListItem_append` on the last
//!   item) is itself portable, but the function as a whole is gated on the
//!   panel identity dedup — the same blocker `incset.rs` documents for
//!   `updateWeakPanel` (`IncSet.c:96`).
//! - [`InfoScreen_drawTitled`] (`InfoScreen.c:50`) — a pure draw
//!   side-effect (`attrset`/`mvhline`/`mvaddstr`/`CRT_colors`, `Panel_draw`,
//!   `IncSet_drawBar`) that also calls `String_stripControlChars`
//!   (`XUtils.h:147`, a `static inline`), which is ABSENT from the
//!   port-purity snapshot (`tests/data/htop_c_fn_names.txt`) and so cannot
//!   be added as a `pub fn` yet; `IncSet_drawBar` is itself an unported
//!   `todo!()` (`incset.rs:378`). No splittable pure logic.
//! - [`InfoScreen_run`] (`InfoScreen.c:96`) — the ncurses main loop
//!   (`Panel_getCh`, `getmouse`/`MEVENT`, `clear()`), the
//!   `IncSet_handleKey`/`IncSet_activate`/`IncSet_drawBar` handlers (all
//!   `todo!()` stubs in `incset.rs`), `Vector_prune`, and the
//!   `As_InfoScreen` vtable dispatch
//!   (`InfoScreen_scan`/`InfoScreen_draw`/`InfoScreen_onErr`/
//!   `InfoScreen_onKey`), which the omitted `Object super` does not model.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::ffi::c_int;

use crate::ported::crt::KEY_F;
use crate::ported::functionbar::{FunctionBar, FunctionBar_new, Ncurses};
use crate::ported::incset::{IncSet, IncSet_filter, IncSet_new};
use crate::ported::listitem::ListItem_new;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::{Panel, Panel_add, Panel_new, Panel_setHeader};
use crate::ported::process::Process;
use crate::ported::vector::{Vector, Vector_add, Vector_new};
use crate::ported::xutils::String_contains_i;

/// Port of `#define VECTOR_DEFAULT_SIZE (10)` from `Vector.h:15` — the
/// initial `lines` vector capacity `InfoScreen_init` passes to `Vector_new`.
const VECTOR_DEFAULT_SIZE: c_int = 10;

/// Port of `static const char* const InfoScreenFunctions[]`
/// (`InfoScreen.c:25`), minus the trailing `NULL` (Rust length terminates).
const InfoScreenFunctions: [&str; 4] = ["Search ", "Filter ", "Refresh", "Done   "];

/// Port of `static const char* const InfoScreenKeys[]` (`InfoScreen.c:27`).
const InfoScreenKeys: [&str; 4] = ["F3", "F4", "F5", "Esc"];

/// Port of `static const int InfoScreenEvents[]` (`InfoScreen.c:29`):
/// `{KEY_F(3), KEY_F(4), KEY_F(5), 27}` (`crt::KEY_F` reproduces the
/// ncurses codes; `27` is `Esc`).
const InfoScreenEvents: [c_int; 4] = [KEY_F(3), KEY_F(4), KEY_F(5), 27];

/// Port of `struct InfoScreen_` (`InfoScreen.h:22`). See the module docs
/// for the full field mapping: `Object super` (the vtable) is omitted (only
/// the stubbed [`InfoScreen_run`] reads it), `process` is a raw back-pointer
/// (owned by the caller), and `display`/`inc`/`lines` are owned values.
pub struct InfoScreen {
    /// C `const Process* process` — the process this screen describes; a
    /// borrowed handle owned elsewhere, kept raw so the struct stays
    /// `'static` (the `MainPanel.state` precedent). Never dereferenced by
    /// the ported functions.
    pub process: *const Process,
    /// C `Panel* display` — the scrollable, filtered list widget.
    pub display: Panel,
    /// C `IncSet* inc` — the incremental search/filter state.
    pub inc: IncSet,
    /// C `Vector* lines` — every line ever added (the full set the filter
    /// narrows `display` against). Owns its `ListItem`s.
    pub lines: Vector,
}

impl InfoScreen {
    /// A zeroed `InfoScreen`: null `process`, an empty `Panel`
    /// (`Panel_new(0, 0, 0, 0, None)`), an empty `IncSet` (`IncSet_new(None)`),
    /// and an empty `ListItem`-typed `lines` `Vector`. Gate-skipped
    /// associated fn — not a C function; the C analog is the `AllocThis`
    /// uninitialized storage that `InfoScreen_init` then overwrites (the
    /// same `Panel::empty` / `IncMode::empty` bootstrap idiom).
    fn empty() -> InfoScreen {
        let list_item_class: &'static ObjectClass = ListItem_new("", 0).klass();
        InfoScreen {
            process: core::ptr::null(),
            display: Panel_new(0, 0, 0, 0, None),
            inc: IncSet_new(None),
            lines: Vector_new(list_item_class, true, VECTOR_DEFAULT_SIZE),
        }
    }
}

/// Port of `InfoScreen* InfoScreen_init(InfoScreen* this, const Process*
/// process, FunctionBar* bar, int height, const char* panelHeader)` from
/// `InfoScreen.c:31`.
///
/// Stores the `process` back-pointer, builds a default function bar when
/// none is supplied (C `if (!bar) bar = FunctionBar_new(...)`), creates the
/// `display` panel and the `inc` set from that bar, allocates the `lines`
/// vector, and installs the panel header. See the module docs for the four
/// owned-value divergences (shared `FunctionBar` -> clone + move; `COLS` ->
/// `Ncurses::cols()`; `Vector_type` -> `ListItem` class from an instance).
/// Returns `this`, mirroring the C `return this` identity chain-return.
pub fn InfoScreen_init<'a>(
    this: &'a mut InfoScreen,
    process: *const Process,
    bar: Option<FunctionBar>,
    height: c_int,
    panelHeader: &str,
) -> &'a mut InfoScreen {
    this.process = process;

    // C: if (!bar) bar = FunctionBar_new(InfoScreenFunctions, InfoScreenKeys, InfoScreenEvents);
    let bar = bar.unwrap_or_else(|| {
        FunctionBar_new(
            Some(&InfoScreenFunctions[..]),
            Some(&InfoScreenKeys[..]),
            Some(&InfoScreenEvents[..]),
        )
    });

    // C: this->display = Panel_new(0, 1, COLS, height, Class(ListItem), false, bar);
    // COLS -> Ncurses::cols(); the shared bar is cloned into the panel and
    // moved into the IncSet below (see module docs).
    this.display = Panel_new(0, 1, Ncurses::cols(), height, Some(bar.clone()));

    // C: this->inc = IncSet_new(bar);   // same bar pointer as the panel in C
    this.inc = IncSet_new(Some(bar));

    // C: this->lines = Vector_new(Vector_type(this->display->items), true, VECTOR_DEFAULT_SIZE);
    // The panel's item class is Class(ListItem); recover it from an instance.
    let list_item_class: &'static ObjectClass = ListItem_new("", 0).klass();
    this.lines = Vector_new(list_item_class, true, VECTOR_DEFAULT_SIZE);

    // C: Panel_setHeader(this->display, panelHeader);
    Panel_setHeader(&mut this.display, panelHeader);

    this
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
/// `IncSet_drawBar`) plus `String_stripControlChars` (`XUtils.h:147`), which
/// is ABSENT from the port-purity snapshot and so cannot be added as a
/// `pub fn` yet; `IncSet_drawBar` is itself a `todo!()` (`incset.rs:378`).
pub fn InfoScreen_drawTitled() {
    todo!("port of InfoScreen.c:50 — ncurses draw; String_stripControlChars (absent from snapshot) + IncSet_drawBar unported")
}

/// Port of `void InfoScreen_addLine(InfoScreen* this, const char* line)`
/// from `InfoScreen.c:73`. Appends a fresh `ListItem` for `line` to the
/// `lines` vector, then — when there is no active filter or `line` matches
/// the current `IncSet_filter` (`String_contains_i`, case-insensitive) —
/// also shows it in the panel. Per the module docs, C adds the *same*
/// `Object*` to the panel that it put in `lines`; the owned-`Box` model
/// instead gives the panel an identical independent `ListItem` (faithful
/// for display; identity matters only to the stubbed `InfoScreen_appendLine`).
pub fn InfoScreen_addLine(this: &mut InfoScreen, line: &str) {
    // C: Vector_add(this->lines, (Object*) ListItem_new(line, 0));
    Vector_add(&mut this.lines, Box::new(ListItem_new(line, 0)));

    // C: const char* incFilter = IncSet_filter(this->inc);
    //    if (!incFilter || String_contains_i(line, incFilter, true)) { ... }
    let show = match IncSet_filter(&this.inc) {
        None => true,
        Some(incFilter) => String_contains_i(line, incFilter, true),
    };
    if show {
        // C: Panel_add(this->display, Vector_get(this->lines, Vector_size(this->lines) - 1));
        Panel_add(&mut this.display, Box::new(ListItem_new(line, 0)));
    }
}

/// TODO: port of `void InfoScreen_appendLine(InfoScreen* this, const char*
/// line)` from `InfoScreen.c:81`. Blocked on the weak-panel shared-`Object*`
/// identity that owned `Box`es cannot model: its `displayLast != last`
/// pointer-identity compare (panel's last item vs `lines`' last item) and
/// its in-place `ListItem_append` mutation both rely on the panel and
/// `lines` aliasing one object, and its filter re-test runs against the
/// newly appended fragment (`String_contains_i(line, incFilter, true)`, with
/// `line` only the appended text). With the independent clones the port uses
/// (see `InfoScreen_addLine`) the two are never pointer-equal, so the C
/// dedup / in-place path cannot be reproduced. Same blocker class as
/// `updateWeakPanel` (`incset.rs`, `IncSet.c:96`).
pub fn InfoScreen_appendLine() {
    todo!("port of InfoScreen.c:81 — needs weak-panel shared Object* identity (displayLast != last); owned Box can't alias panel + lines")
}

/// TODO: port of `void InfoScreen_run(InfoScreen* this)` from
/// `InfoScreen.c:96`. The ncurses main loop: `Panel_getCh`,
/// `getmouse`/`MEVENT`, `clear()`, the
/// `IncSet_handleKey`/`IncSet_activate`/`IncSet_drawBar` handlers (all
/// `todo!()` stubs in `incset.rs`), `Vector_prune`, and the
/// `As_InfoScreen` vtable dispatch
/// (`InfoScreen_scan`/`InfoScreen_draw`/`InfoScreen_onErr`/
/// `InfoScreen_onKey`), which the omitted `Object super` does not model.
pub fn InfoScreen_run() {
    todo!("port of InfoScreen.c:96 — ncurses loop; IncSet handlers + InfoScreenClass vtable unported")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::incset::IncSet_setFilter;
    use crate::ported::listitem::ListItem;
    use crate::ported::panel::{Panel_get, Panel_headerHeight, Panel_size};
    use crate::ported::vector::{Vector_get, Vector_size};

    /// Read the `value` of the `ListItem` stored at `idx` in a `Vector`.
    fn line_value(lines: &Vector, idx: usize) -> String {
        let any: &dyn std::any::Any = Vector_get(lines, idx);
        any.downcast_ref::<ListItem>().unwrap().value.clone()
    }

    /// Read the `value` of the `ListItem` shown at panel index `i`.
    fn panel_value(p: &Panel, i: i32) -> String {
        let any: &dyn std::any::Any = Panel_get(p, i);
        any.downcast_ref::<ListItem>().unwrap().value.clone()
    }

    fn fresh(height: c_int, header: &str) -> InfoScreen {
        let mut this = InfoScreen::empty();
        InfoScreen_init(&mut this, core::ptr::null(), None, height, header);
        this
    }

    #[test]
    fn init_sets_fields_and_geometry() {
        let mut this = InfoScreen::empty();
        InfoScreen_init(&mut this, core::ptr::null(), None, 22, "HEADER");
        // process back-pointer stored (null here).
        assert!(this.process.is_null());
        // lines starts empty; panel starts empty.
        assert_eq!(Vector_size(&this.lines), 0);
        assert_eq!(Panel_size(&this.display), 0);
        // Panel geometry: Panel_new(0, 1, COLS, height, ...).
        assert_eq!(this.display.x, 0);
        assert_eq!(this.display.y, 1);
        assert_eq!(this.display.h, 22);
        assert_eq!(this.display.w, Ncurses::cols());
        // Header installed -> headerHeight 1.
        assert_eq!(Panel_headerHeight(&this.display), 1);
        // No filter active on a fresh IncSet.
        assert!(IncSet_filter(&this.inc).is_none());
    }

    #[test]
    fn init_builds_default_bar_when_none() {
        let this = fresh(10, " ");
        // C: bar = FunctionBar_new(InfoScreenFunctions, InfoScreenKeys, InfoScreenEvents).
        // The panel's default bar carries the InfoScreen labels/keys/events.
        let bar = this.display.defaultBar.as_ref().expect("default bar built");
        assert_eq!(bar.functions, InfoScreenFunctions.to_vec());
        assert_eq!(bar.keys, InfoScreenKeys.to_vec());
        assert_eq!(bar.events, InfoScreenEvents.to_vec());
        // The IncSet received the same bar content (cloned + moved).
        let inc_bar = this.inc.defaultBar.as_ref().expect("inc default bar");
        assert_eq!(inc_bar.functions, InfoScreenFunctions.to_vec());
    }

    #[test]
    fn init_uses_supplied_bar() {
        let mut this = InfoScreen::empty();
        let custom = FunctionBar_new(
            Some(&["Only "][..]),
            Some(&["F1"][..]),
            Some(&[1][..]),
        );
        InfoScreen_init(&mut this, core::ptr::null(), Some(custom), 5, "H");
        assert_eq!(
            this.display.defaultBar.as_ref().unwrap().functions,
            vec!["Only ".to_string()]
        );
        assert_eq!(
            this.inc.defaultBar.as_ref().unwrap().functions,
            vec!["Only ".to_string()]
        );
    }

    #[test]
    fn add_line_grows_lines_and_panel_without_filter() {
        let mut this = fresh(10, "H");
        InfoScreen_addLine(&mut this, "alpha");
        InfoScreen_addLine(&mut this, "beta");
        InfoScreen_addLine(&mut this, "gamma");
        // Every line is recorded in `lines`.
        assert_eq!(Vector_size(&this.lines), 3);
        assert_eq!(line_value(&this.lines, 0), "alpha");
        assert_eq!(line_value(&this.lines, 1), "beta");
        assert_eq!(line_value(&this.lines, 2), "gamma");
        // With no filter, every line is also shown in the panel.
        assert_eq!(Panel_size(&this.display), 3);
        assert_eq!(panel_value(&this.display, 0), "alpha");
        assert_eq!(panel_value(&this.display, 2), "gamma");
    }

    #[test]
    fn add_line_filter_gates_panel_but_not_lines() {
        let mut this = fresh(10, "H");
        // Activate a filter: only lines containing "sh" are shown.
        IncSet_setFilter(&mut this.inc, "sh");
        assert_eq!(IncSet_filter(&this.inc), Some("sh"));

        InfoScreen_addLine(&mut this, "bash"); // matches
        InfoScreen_addLine(&mut this, "xyz"); //  no match
        InfoScreen_addLine(&mut this, "zsh"); //  matches

        // All three are recorded in `lines` regardless of the filter.
        assert_eq!(Vector_size(&this.lines), 3);
        assert_eq!(line_value(&this.lines, 1), "xyz");

        // Only the two matching lines reach the panel, in order.
        assert_eq!(Panel_size(&this.display), 2);
        assert_eq!(panel_value(&this.display, 0), "bash");
        assert_eq!(panel_value(&this.display, 1), "zsh");
    }

    #[test]
    fn add_line_filter_is_case_insensitive() {
        let mut this = fresh(10, "H");
        IncSet_setFilter(&mut this.inc, "SH"); // uppercase needle
        InfoScreen_addLine(&mut this, "bash"); // lowercase haystack -> matches
        assert_eq!(Vector_size(&this.lines), 1);
        assert_eq!(Panel_size(&this.display), 1);
        assert_eq!(panel_value(&this.display, 0), "bash");
    }

    #[test]
    fn add_line_empty_string_is_recorded() {
        let mut this = fresh(10, "H");
        InfoScreen_addLine(&mut this, "");
        assert_eq!(Vector_size(&this.lines), 1);
        assert_eq!(line_value(&this.lines, 0), "");
        // No filter -> shown in the panel too.
        assert_eq!(Panel_size(&this.display), 1);
    }
}
