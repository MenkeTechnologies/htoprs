//! Partial port of `IncSet.c` — htop's incremental search / filter set.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake` and
//! lowerCamelCase statics), so `non_snake_case` is allowed for the whole
//! module — matching the spec name-for-name is the point of the port.
//!
//! # What is ported
//!
//! The pure state layer, which depends only on already-ported substrate
//! ([`FunctionBar`]/`FunctionBar_new`, [`LineEditor`] init/reset/setText,
//! [`Panel_get`], `String_contains_i`, `crt::{ERR, KEY_F}`):
//!
//! - the `IncType` enum and the [`IncMode`]/[`IncSet`] structs,
//! - [`IncMode_reset`], [`IncSet_reset`], [`IncSet_setFilter`],
//! - [`IncMode_initSearch`] / [`IncMode_initFilter`] (the exact
//!   function-bar label/key/event tables) and [`IncSet_new`],
//! - [`IncSet_getListItemValue`] — the concrete `IncMode_GetPanelValue`
//!   callback (downcasts each `Panel` item to [`ListItem`] and returns its
//!   `value`, `""` for a non-`ListItem`, exactly like the C ternary).
//!
//! # What stays a `todo!()` stub, and why
//!
//! Every stub below is blocked on substrate that is not yet ported; per
//! the port rules the missing piece is escalated, never papered over with
//! an ad-hoc reimplementation:
//!
//! - `LineEditor_getText` (`LineEditor.h:37`, `return this->buffer`) has
//!   **no** Rust analog in `lineeditor.rs` and the `LineEditor` fields are
//!   module-private, so the editor's current text cannot be read from this
//!   module. This blocks the entire match core: [`search`] (`:124`),
//!   [`IncMode_find`] (`:154`), [`updateWeakPanel`] (`:96`), and
//!   [`IncSet_filter`] (`IncSet.h:40`).
//! - `LineEditor_handleKey` (`lineeditor.rs:205`, itself a `todo!()`) is
//!   the char/backspace edit path; together with the `getText` gap it
//!   blocks [`IncSet_handleKey`] (`:177`).
//! - `Panel_setDefaultBar` (`Panel.c`) is not ported in `panel.rs`, and the
//!   terminal side-effects (`FunctionBar_draw`) block [`IncSet_deactivate`]
//!   (`:147`).
//! - `IncSet_drawBar` (`:302`) is ncurses drawing (`FunctionBar_drawExtra`,
//!   `LineEditor_draw`/`LineEditor_updateScroll`, `curs_set`, `COLS`/`LINES`);
//!   `LineEditor_draw` is also an unported stub. [`IncSet_activate`]
//!   (`:136`) ends in `IncSet_drawBar` and stores a `Panel*` back-pointer
//!   the Rust model omits, so it is stubbed too.
//! - [`IncSet_synthesizeEvent`] (`:327`) writes `this->panel->lastMouseBarClickX`
//!   through the omitted `Panel*` back-pointer.
//! - `History` (`History.c`) is not ported: [`IncSet_delete`] (`:77`),
//!   [`IncSet_setHistoryFile`] (`:85`), [`IncSet_saveHistory`] (`:91`).
//! - [`IncMode_done`] (`:61`) is `FunctionBar_delete(mode->bar)` — the
//!   owned [`FunctionBar`] is released by `Drop`, so there is no algorithm
//!   to port (same as `FunctionBar_delete`/`Panel_done`).
//!
//! # Struct mapping
//!
//! - C `IncMode* active` (points into `modes[]`) → `Option<IncType>` (which
//!   of the two modes is active), avoiding a self-referential borrow.
//! - C `Panel* panel` back-pointer and `History* history` are omitted; the
//!   only functions that use them are stubbed for the reasons above.
//! - C `FunctionBar* defaultBar` → owned `Option<FunctionBar>`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::ported::crt::{ERR, KEY_F};
use crate::ported::functionbar::{FunctionBar, FunctionBar_new};
use crate::ported::history::{History, History_new, History_save};
use crate::ported::lineeditor::{LineEditor, LineEditor_init, LineEditor_reset, LineEditor_setText};
use crate::ported::listitem::ListItem;
use crate::ported::panel::{Panel, Panel_get};

/// Port of `enum` `IncType` from `IncSet.h:19`. The discriminants (0/1)
/// are load-bearing: they index `IncSet::modes`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum IncType {
    INC_SEARCH = 0,
    INC_FILTER = 1,
}

/// Port of `struct IncMode_` from `IncSet.h:24`: the per-mode line editor,
/// its function bar, and the filter/search discriminator.
pub struct IncMode {
    pub editor: LineEditor,
    pub bar: FunctionBar,
    pub isFilter: bool,
}

impl IncMode {
    /// Zeroed `IncMode` (`memset(mode, 0, sizeof(IncMode))` storage before
    /// `IncMode_initSearch`/`IncMode_initFilter` overwrites it). Gate-skipped
    /// associated fn — not a C function; mirrors `Panel::empty`.
    fn empty() -> IncMode {
        IncMode {
            editor: LineEditor::default(),
            bar: FunctionBar {
                functions: Vec::new(),
                keys: Vec::new(),
                events: Vec::new(),
                staticData: false,
            },
            isFilter: false,
        }
    }
}

/// Port of `struct IncSet_` from `IncSet.h:30`. See the module docs for the
/// `active`/`panel`/`history` field mapping.
pub struct IncSet {
    pub modes: [IncMode; 2],
    pub active: Option<IncType>,
    pub defaultBar: Option<FunctionBar>,
    pub filtering: bool,
    pub found: bool,
    /// C `History* history` (`IncSet.h:37`) — shared history for search and
    /// filter, `NULL` when no history file is set. Modeled as an owned
    /// `Option<History>` (History.c is ported); the C `History_delete` free
    /// is supplied by `Drop`, so reassignment/teardown releases it.
    pub history: Option<History>,
}

/// Port of the `IncMode_GetPanelValue` function-pointer typedef from
/// `IncSet.h:46` (`typedef const char* (*)(Panel*, int)`). Modeled as a
/// borrowing `fn` pointer: the returned `&str` borrows from the `Panel`.
/// [`IncSet_getListItemValue`] is the concrete implementation htop passes.
pub type IncMode_GetPanelValue = for<'a> fn(&'a Panel, i32) -> &'a str;

// Search-mode function bar tables (`IncSet.c:39`). The C arrays carry a
// trailing NULL terminator that `FunctionBar_new` counts against; the
// ported `FunctionBar_new` takes a plain slice, so the NULL is dropped.
const searchFunctions: [&str; 4] = ["Next  ", "Prev   ", "Cancel ", " Search: "];
const searchKeys: [&str; 4] = ["F3", "S-F3", "Esc", "  "];
const searchEvents: [i32; 4] = [KEY_F(3), KEY_F(15), 27, ERR];

// Filter-mode function bar tables (`IncSet.c:50`).
const filterFunctions: [&str; 3] = ["Done  ", "Clear ", " Filter: "];
const filterKeys: [&str; 3] = ["Enter", "Esc", "  "];
const filterEvents: [i32; 3] = [13, 27, ERR];

/// Port of `IncSet.c:24`.
fn IncMode_reset(mode: &mut IncMode) {
    LineEditor_reset(&mut mode.editor);
}

/// Port of `IncSet.c:28`.
pub fn IncSet_reset(this: &mut IncSet, type_: IncType) {
    IncMode_reset(&mut this.modes[type_ as usize]);
    this.found = false;
}

/// Port of `IncSet.c:33`. `filter` (a `&str`) is never null, so the C
/// `(filter && filter[0] != '\0')` reduces to `!filter.is_empty()`.
pub fn IncSet_setFilter(this: &mut IncSet, filter: &str) {
    let mode = &mut this.modes[IncType::INC_FILTER as usize];
    LineEditor_setText(&mut mode.editor, filter);
    this.filtering = !filter.is_empty();
}

/// Port of `IncSet.c:43`. `memset(search, 0, ...)` is the caller-supplied
/// zeroed [`IncMode::empty`]; this fills in the bar, the flag, and inits
/// the editor.
fn IncMode_initSearch(search: &mut IncMode) {
    *search = IncMode::empty();
    search.bar = FunctionBar_new(
        Some(&searchFunctions[..]),
        Some(&searchKeys[..]),
        Some(&searchEvents[..]),
    );
    search.isFilter = false;
    LineEditor_init(&mut search.editor);
}

/// Port of `IncSet.c:54`.
fn IncMode_initFilter(filter: &mut IncMode) {
    *filter = IncMode::empty();
    filter.bar = FunctionBar_new(
        Some(&filterFunctions[..]),
        Some(&filterKeys[..]),
        Some(&filterEvents[..]),
    );
    filter.isFilter = true;
    LineEditor_init(&mut filter.editor);
}

/// TODO: port of `IncSet.c:61`. `FunctionBar_delete(mode->bar)` — the owned
/// [`FunctionBar`] is released by `Drop`, so there is no algorithm to port
/// (same as `FunctionBar_delete`/`Panel_done`).
fn IncMode_done() {
    todo!("port of IncSet.c:61 — Drop releases the owned FunctionBar")
}

/// Port of `IncSet.c:65`. Builds both modes (zeroed [`IncMode::empty`] then
/// `IncMode_initSearch`/`IncMode_initFilter`), stores the panel's default
/// bar, and clears `active`/`filtering`/`found`/`history` (C `history = NULL`
/// → `None`).
pub fn IncSet_new(bar: Option<FunctionBar>) -> IncSet {
    let mut this = IncSet {
        modes: [IncMode::empty(), IncMode::empty()],
        active: None,
        defaultBar: bar,
        filtering: false,
        found: false,
        history: None,
    };
    IncMode_initSearch(&mut this.modes[IncType::INC_SEARCH as usize]);
    IncMode_initFilter(&mut this.modes[IncType::INC_FILTER as usize]);
    this
}

/// TODO: port of `IncSet.c:77`. `IncMode_done` x2 + `History_delete` +
/// `free` — the owned fields are released by `Drop`, and `History` is not
/// ported.
pub fn IncSet_delete() {
    todo!("port of IncSet.c:77 — Drop releases owned fields; History.c unported")
}

/// Port of `IncSet.c:85`. Replaces the history with one loaded from
/// `filename`. The C `if (this->history) History_delete(this->history)`
/// free is supplied by `Drop`: assigning `Some(..)` releases the previous
/// `History`. `filename` is never null at the call site, so the C
/// `const char*` becomes `&str` wrapped as `Some(filename)` for
/// `History_new`.
pub fn IncSet_setHistoryFile(this: &mut IncSet, filename: &str) {
    this.history = Some(History_new(Some(filename)));
}

/// Port of `IncSet.c:91`. Saves the history to disk if one is set
/// (`History_save` is itself a no-op when the history has no filename).
pub fn IncSet_saveHistory(this: &IncSet) {
    if let Some(history) = &this.history {
        History_save(history);
    }
}

/// TODO: port of `IncSet.c:96`. Rebuilds `panel` from the backing `lines`,
/// keeping only items whose value matches the filter via `String_contains_i`.
/// Blocked on `LineEditor_getText` (`LineEditor.h:37`, unported — needed to
/// read the filter text) and the unported `Vector` type (the `lines` source).
fn updateWeakPanel() {
    todo!("port of IncSet.c:96 — needs LineEditor_getText (unported) + Vector (unported)")
}

/// TODO: port of `IncSet.c:124`. Walks the panel front-to-back and selects
/// the first item whose `getPanelValue` matches the active editor text via
/// `String_contains_i`. Blocked on `LineEditor_getText` (`LineEditor.h:37`)
/// which is not ported in `lineeditor.rs` (the editor's text cannot be read
/// from this module).
fn search(_this: &mut IncSet, _panel: &mut Panel, _getPanelValue: IncMode_GetPanelValue) -> bool {
    todo!("port of IncSet.c:124 — needs LineEditor_getText (LineEditor.h:37), unported")
}

/// TODO: port of `IncSet.c:136`. Activates a mode (sets `active`,
/// `panel->currentBar`, `cursorOn`, the `panel` back-pointer, resets history
/// position) and ends in `IncSet_drawBar`. Blocked on the omitted `Panel*`
/// back-pointer, `IncSet_drawBar` (ncurses), and `History_resetPosition`.
pub fn IncSet_activate() {
    todo!("port of IncSet.c:136 — needs IncSet_drawBar (ncurses) + Panel back-pointer + History")
}

/// TODO: port of `IncSet.c:147`. Clears `active`, restores the default bar
/// (`Panel_setDefaultBar`), hides the cursor, and redraws. Blocked on
/// `Panel_setDefaultBar` (not ported in `panel.rs`) and `FunctionBar_draw`
/// (ncurses side-effect).
fn IncSet_deactivate() {
    todo!("port of IncSet.c:147 — needs Panel_setDefaultBar (unported) + ncurses draw")
}

/// TODO: port of `IncSet.c:154`. Steps through the panel (wrapping) from the
/// current selection looking for the next/prev `String_contains_i` match.
/// Blocked on `LineEditor_getText` (`LineEditor.h:37`), unported.
fn IncMode_find(
    _mode: &mut IncMode,
    _panel: &mut Panel,
    _getPanelValue: IncMode_GetPanelValue,
    _step: i32,
) -> bool {
    todo!("port of IncSet.c:154 — needs LineEditor_getText (LineEditor.h:37), unported")
}

/// TODO: port of `IncSet.c:177`. The key dispatcher (F3/next-prev, history
/// up/down, Enter/Esc confirm-abort, and the line-editor char/backspace
/// path). Blocked on `LineEditor_getText` (`LineEditor.h:37`) and
/// `LineEditor_handleKey` (`lineeditor.rs:205`, itself a stub), plus
/// `History`, `IncSet_drawBar`, and `Panel_setDefaultBar`.
pub fn IncSet_handleKey() {
    todo!("port of IncSet.c:177 — needs LineEditor_getText + LineEditor_handleKey (both unported)")
}

/// Port of `IncSet.c:297`. The concrete `IncMode_GetPanelValue`: downcast the
/// panel's item at `i` to [`ListItem`] and return its `value`, or `""` when
/// it is not a `ListItem` (the C `l ? l->value : ""` ternary — a failed
/// `(ListItem*)` cast / NULL yields the empty string).
pub fn IncSet_getListItemValue(panel: &Panel, i: i32) -> &str {
    // C: `const ListItem* l = (const ListItem*) Panel_get(panel, i);`
    let obj: &dyn core::any::Any = Panel_get(panel, i);
    match obj.downcast_ref::<ListItem>() {
        Some(l) => &l.value,
        None => "",
    }
}

/// TODO: port of `IncSet.c:302`. Draws the active mode's function bar and
/// line editor. Pure ncurses (`FunctionBar_drawExtra`, `LineEditor_updateScroll`,
/// `LineEditor_draw`, `curs_set`, `COLS`/`LINES`); `LineEditor_draw` is also
/// an unported stub.
pub fn IncSet_drawBar() {
    todo!("port of IncSet.c:302 — ncurses draw; LineEditor_draw also unported")
}

/// TODO: port of `IncSet.c:327`. Turns a bar x-coordinate into a synthesized
/// event via `FunctionBar_synthesizeEvent`, writing
/// `this->panel->lastMouseBarClickX` through the omitted `Panel*` back-pointer.
pub fn IncSet_synthesizeEvent() {
    todo!("port of IncSet.c:327 — needs the omitted Panel back-pointer")
}

/// TODO: port of `IncSet.h:40` (`static inline IncSet_filter`). Returns the
/// filter text when `filtering`, else `NULL`. Blocked on `LineEditor_getText`
/// (`LineEditor.h:37`), unported in `lineeditor.rs`.
pub fn IncSet_filter() {
    todo!("port of IncSet.h:40 — needs LineEditor_getText (LineEditor.h:37), unported")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::object::Object;
    use crate::ported::panel::Panel_new;

    fn li(value: &str) -> Box<dyn Object> {
        Box::new(ListItem {
            value: value.to_string(),
            key: 0,
            moving: false,
        })
    }

    // ── IncSet_new / mode init ────────────────────────────────────────

    #[test]
    fn new_builds_two_modes_with_flags_and_defaults() {
        let set = IncSet_new(None);
        assert!(set.active.is_none());
        assert!(!set.filtering);
        assert!(!set.found);
        assert!(set.defaultBar.is_none());
        // INC_SEARCH is a search mode, INC_FILTER is a filter mode.
        assert!(!set.modes[IncType::INC_SEARCH as usize].isFilter);
        assert!(set.modes[IncType::INC_FILTER as usize].isFilter);
    }

    #[test]
    fn new_stores_default_bar() {
        let bar = FunctionBar {
            functions: vec!["Help".into()],
            keys: vec!["F1".into()],
            events: vec![1],
            staticData: false,
        };
        let set = IncSet_new(Some(bar));
        assert!(set.defaultBar.is_some());
        assert_eq!(set.defaultBar.unwrap().functions, vec!["Help".to_string()]);
    }

    #[test]
    fn init_search_bar_matches_c_tables() {
        let set = IncSet_new(None);
        let bar = &set.modes[IncType::INC_SEARCH as usize].bar;
        assert_eq!(bar.functions, vec!["Next  ", "Prev   ", "Cancel ", " Search: "]);
        assert_eq!(bar.keys, vec!["F3", "S-F3", "Esc", "  "]);
        assert_eq!(bar.events, vec![KEY_F(3), KEY_F(15), 27, ERR]);
        // functions+keys+events supplied -> owns per-slot copies.
        assert!(!bar.staticData);
    }

    #[test]
    fn init_filter_bar_matches_c_tables() {
        let set = IncSet_new(None);
        let bar = &set.modes[IncType::INC_FILTER as usize].bar;
        assert_eq!(bar.functions, vec!["Done  ", "Clear ", " Filter: "]);
        assert_eq!(bar.keys, vec!["Enter", "Esc", "  "]);
        assert_eq!(bar.events, vec![13, 27, ERR]);
    }

    // ── IncSet_setFilter ──────────────────────────────────────────────

    #[test]
    fn set_filter_nonempty_turns_filtering_on() {
        let mut set = IncSet_new(None);
        IncSet_setFilter(&mut set, "bash");
        assert!(set.filtering);
    }

    #[test]
    fn set_filter_empty_turns_filtering_off() {
        let mut set = IncSet_new(None);
        IncSet_setFilter(&mut set, "bash");
        assert!(set.filtering);
        // Empty filter clears the flag (C: filter[0] == '\0').
        IncSet_setFilter(&mut set, "");
        assert!(!set.filtering);
    }

    // ── IncSet_reset / IncMode_reset ──────────────────────────────────

    #[test]
    fn reset_clears_found() {
        let mut set = IncSet_new(None);
        set.found = true;
        IncSet_reset(&mut set, IncType::INC_SEARCH);
        assert!(!set.found);
    }

    #[test]
    fn reset_does_not_touch_filtering_flag() {
        // IncSet_reset only clears `found` + resets the mode's editor; it
        // leaves `filtering` alone (matching the C body).
        let mut set = IncSet_new(None);
        IncSet_setFilter(&mut set, "x");
        assert!(set.filtering);
        IncSet_reset(&mut set, IncType::INC_FILTER);
        assert!(set.filtering);
        assert!(!set.found);
    }

    // ── IncSet_getListItemValue (the concrete GetPanelValue) ──────────

    #[test]
    fn get_list_item_value_returns_item_strings() {
        let mut p = Panel_new(0, 0, 10, 10, None);
        p.items.push(li("systemd"));
        p.items.push(li("bash"));
        p.items.push(li("htop"));
        assert_eq!(IncSet_getListItemValue(&p, 0), "systemd");
        assert_eq!(IncSet_getListItemValue(&p, 1), "bash");
        assert_eq!(IncSet_getListItemValue(&p, 2), "htop");
    }

    #[test]
    fn get_list_item_value_usable_as_fn_pointer() {
        // It must satisfy the IncMode_GetPanelValue callback type.
        let f: IncMode_GetPanelValue = IncSet_getListItemValue;
        let mut p = Panel_new(0, 0, 10, 10, None);
        p.items.push(li("firefox"));
        assert_eq!(f(&p, 0), "firefox");
    }

    #[test]
    fn get_list_item_value_composes_with_string_contains_i() {
        // Demonstrates the two ported primitives the (stubbed) search would
        // use: getPanelValue + String_contains_i, case-insensitive.
        use crate::ported::xutils::String_contains_i;
        let mut p = Panel_new(0, 0, 10, 10, None);
        for v in ["systemd", "bash", "htop", "sshd"] {
            p.items.push(li(v));
        }
        let needle = "SH"; // matches "bash" and "sshd" case-insensitively
        let hits: Vec<i32> = (0..p.items.len() as i32)
            .filter(|&i| String_contains_i(IncSet_getListItemValue(&p, i), needle, true))
            .collect();
        assert_eq!(hits, vec![1, 3]);
    }
}
