//! Partial port of `IncSet.c` — htop's incremental search / filter set.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake` and
//! lowerCamelCase statics), so `non_snake_case` is allowed for the whole
//! module — matching the spec name-for-name is the point of the port.
//!
//! # What is ported
//!
//! The state layer plus the pure match core, which now depend only on
//! already-ported substrate ([`FunctionBar`]/`FunctionBar_new`/
//! `FunctionBar_draw`, [`LineEditor`] init/reset/setText/`getText`,
//! [`Panel_get`]/`Panel_size`/`Panel_setSelected`/`Panel_getSelectedIndex`/
//! `Panel_setDefaultBar`, [`History`], `String_contains_i`, `crt::{ERR, KEY_F}`):
//!
//! - the `IncType` enum and the [`IncMode`]/[`IncSet`] structs,
//! - [`IncMode_reset`], [`IncSet_reset`], [`IncSet_setFilter`],
//! - [`IncMode_initSearch`] / [`IncMode_initFilter`] (the exact
//!   function-bar label/key/event tables) and [`IncSet_new`],
//! - [`IncSet_setHistoryFile`], [`IncSet_saveHistory`],
//! - [`IncSet_getListItemValue`] — the concrete `IncMode_GetPanelValue`
//!   callback (downcasts each `Panel` item to [`ListItem`] and returns its
//!   `value`, `""` for a non-`ListItem`, exactly like the C ternary),
//! - the match core: [`search`] (`:124`) and [`IncMode_find`] (`:154`),
//!   now that `LineEditor_getText` reads the active editor text,
//! - [`IncSet_deactivate`] (`:147`) — `Panel_setDefaultBar` + hide cursor +
//!   `FunctionBar_draw`, all ported,
//! - [`IncSet_filter`] (`IncSet.h:40`) — the filter-text accessor.
//!
//! # What stays a `todo!()` stub, and why
//!
//! Every stub below is blocked on substrate that is not yet ported; per
//! the port rules the missing piece is escalated, never papered over with
//! an ad-hoc reimplementation:
//!
//! - `LineEditor_draw` (`lineeditor.rs`, still a `todo!()` ncurses blit)
//!   blocks [`IncSet_drawBar`] (`:302`), which also writes
//!   `this->panel->cursorY`/`cursorX` through the omitted `Panel*`
//!   back-pointer. [`IncSet_activate`] (`:136`) ends in `IncSet_drawBar`
//!   and stores that same `this->panel = panel` back-pointer, so it is
//!   stubbed too.
//! - [`IncSet_synthesizeEvent`] (`:327`) writes `this->panel->lastMouseBarClickX`
//!   through the omitted `Panel*` back-pointer.
//! - [`IncSet_handleKey`] (`:177`) drives ported line-editor/`search`/
//!   `IncMode_find`/`IncSet_deactivate` code, but also calls the three
//!   stubs above plus `History_navigate` (`history.rs`, still `todo!()` —
//!   the KEY_UP/KEY_DOWN arms), `LineEditor_click` (`lineeditor.rs`, still
//!   `todo!()` — the KEY_MOUSE_BAR_CLICK arm), and [`updateWeakPanel`], so
//!   the whole body cannot be assembled without gutting behavior.
//! - [`updateWeakPanel`] (`:96`) takes a `Vector* lines`; `vector.rs` ports
//!   only the sort core (no `Vector` container), and htop's "weak panel"
//!   shares one `Object*` between `lines` and `panel` while `Panel_add`
//!   takes an owned, non-`Clone` `Box<dyn Object>` — the shared-pointer
//!   model has no analog.
//! - [`IncSet_delete`] (`:77`) is a pure teardown chain; the owned
//!   `FunctionBar`s and `Option<History>` are released by `Drop` (and
//!   `History_delete` is itself a `Drop` no-op), so there is no algorithm.
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
use crate::ported::functionbar::{FunctionBar, FunctionBar_draw, FunctionBar_new};
use crate::ported::history::{History, History_new, History_save};
use crate::ported::lineeditor::{
    LineEditor, LineEditor_getText, LineEditor_init, LineEditor_reset, LineEditor_setText,
};
use crate::ported::listitem::ListItem;
use crate::ported::panel::{
    Panel, Panel_get, Panel_getSelectedIndex, Panel_setDefaultBar, Panel_setSelected, Panel_size,
};
use crate::ported::xutils::String_contains_i;

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
/// `free` — a pure teardown chain. Every owned field (the two `FunctionBar`s,
/// the `Option<History>`) is released by `Drop`, and `History_delete` itself
/// is a `Drop` no-op in `history.rs`, so there is no algorithm to port.
pub fn IncSet_delete() {
    todo!("port of IncSet.c:77 — Drop releases owned fields (FunctionBars + History)")
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
/// Blocked on two things: the `lines` parameter is a `Vector*` and
/// `vector.rs` ports only the sort core (no `Vector` container / `Vector_get`
/// / `Vector_size`); and htop's "weak panel" shares the same `Object*` between
/// `lines` and `panel`, whereas `Panel_add` takes an *owned* `Box<dyn Object>`
/// (not `Clone`), so items cannot be shared/duplicated from `lines` into the
/// panel. (`LineEditor_getText` is now available, so the filter text itself is
/// no longer a blocker.)
fn updateWeakPanel() {
    todo!("port of IncSet.c:96 — needs Vector container (unported) + weak-panel shared Object* model")
}

/// Port of `IncSet.c:124`. Walks the panel front-to-back and selects the
/// first item whose `getPanelValue` matches the active editor text via
/// `String_contains_i`. The C `this->active->editor` (a pointer into
/// `modes[]`) resolves through `active: Option<IncType>` — the mode is
/// non-`None` whenever `search` runs (the caller only searches with an
/// active mode), so `unwrap()` reproduces the C non-NULL dereference.
fn search(this: &mut IncSet, panel: &mut Panel, getPanelValue: IncMode_GetPanelValue) -> bool {
    let active = this.active.unwrap();
    let size = Panel_size(panel);
    for i in 0..size {
        if String_contains_i(
            getPanelValue(&*panel, i),
            LineEditor_getText(&this.modes[active as usize].editor),
            true,
        ) {
            Panel_setSelected(panel, i);
            return true;
        }
    }

    false
}

/// TODO: port of `IncSet.c:136`. Activates a mode (sets `active`,
/// `panel->currentBar`, `cursorOn`, the `panel` back-pointer, resets history
/// position) and ends in `IncSet_drawBar`. Blocked on the omitted `Panel*`
/// back-pointer (`this->panel = panel`, which the Rust model drops) and on
/// [`IncSet_drawBar`] being an unported stub (see its blockers). `History`
/// and `History_resetPosition` are now ported, so those are no longer the gap.
pub fn IncSet_activate() {
    todo!("port of IncSet.c:136 — needs IncSet_drawBar (stub) + the omitted Panel back-pointer")
}

/// Port of `IncSet.c:147`. Clears `active` (`this->active = NULL` → `None`),
/// restores the panel's default bar (`Panel_setDefaultBar`), hides the
/// cursor, and redraws the default bar. C dereferences `this->defaultBar`
/// unconditionally; the `Option<FunctionBar>` model draws it when present.
fn IncSet_deactivate(this: &mut IncSet, panel: &mut Panel) {
    this.active = None;
    Panel_setDefaultBar(panel);
    panel.cursorOn = false;
    if let Some(bar) = &this.defaultBar {
        FunctionBar_draw(bar);
    }
}

/// Port of `IncSet.c:154`. Steps through the panel (wrapping at both ends)
/// from the current selection looking for the next/prev `String_contains_i`
/// match; returns to `here` after a full loop with no match. The C
/// `for (;;)` becomes `loop {}`; every index stays `i32` so the `i == -1`
/// wrap check is faithful.
fn IncMode_find(
    mode: &mut IncMode,
    panel: &mut Panel,
    getPanelValue: IncMode_GetPanelValue,
    step: i32,
) -> bool {
    let size = Panel_size(panel);
    let here = Panel_getSelectedIndex(panel);
    let mut i = here;
    loop {
        i += step;
        if i == size {
            i = 0;
        }
        if i == -1 {
            i = size - 1;
        }
        if i == here {
            return false;
        }

        if String_contains_i(
            getPanelValue(&*panel, i),
            LineEditor_getText(&mode.editor),
            true,
        ) {
            Panel_setSelected(panel, i);
            return true;
        }
    }
}

/// TODO: port of `IncSet.c:177`. The key dispatcher (F3/next-prev, history
/// up/down, Enter/Esc confirm-abort, mouse bar-click, and the line-editor
/// char/backspace path). The line-editor primitives it drives are now ported
/// (`LineEditor_getText`, `LineEditor_handleKey`), and so are `search` /
/// `IncMode_find` / `IncSet_deactivate` above, but the whole body still cannot
/// be assembled because it also calls: [`IncSet_drawBar`] (a stub — see its
/// blockers), `History_navigate` (`history.rs`, still a `todo!()` — the
/// KEY_UP/KEY_DOWN arms), `LineEditor_click` (`lineeditor.rs`, still a
/// `todo!()` — the KEY_MOUSE_BAR_CLICK arm), and [`updateWeakPanel`] (Vector /
/// weak-panel model). Porting a subset that skips those arms would gut
/// behavior, so it stays a whole honest stub.
pub fn IncSet_handleKey() {
    todo!("port of IncSet.c:177 — needs IncSet_drawBar + History_navigate + LineEditor_click + updateWeakPanel (all stubs)")
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
/// line editor. `FunctionBar_drawExtra` and `LineEditor_updateScroll` are now
/// ported, but this still cannot: it calls `LineEditor_draw` (`lineeditor.rs`,
/// still a `todo!()` ncurses blit) and writes `this->panel->cursorY`/`cursorX`
/// through the omitted `Panel*` back-pointer, neither of which exists here.
pub fn IncSet_drawBar() {
    todo!("port of IncSet.c:302 — needs LineEditor_draw (stub) + the omitted Panel back-pointer")
}

/// TODO: port of `IncSet.c:327`. Turns a bar x-coordinate into a synthesized
/// event via `FunctionBar_synthesizeEvent`, writing
/// `this->panel->lastMouseBarClickX` through the omitted `Panel*` back-pointer.
pub fn IncSet_synthesizeEvent() {
    todo!("port of IncSet.c:327 — needs the omitted Panel back-pointer")
}

/// Port of `IncSet.h:40` (`static inline IncSet_filter`). Returns the filter
/// text when `filtering`, else `NULL` (`None`). The C `char*` into the
/// filter mode's editor buffer becomes an `&str` borrowing `this`.
pub fn IncSet_filter(this: &IncSet) -> Option<&str> {
    if this.filtering {
        Some(LineEditor_getText(
            &this.modes[IncType::INC_FILTER as usize].editor,
        ))
    } else {
        None
    }
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

    // ── search (IncSet.c:124) ─────────────────────────────────────────

    /// Build a panel with the given item values.
    fn panel_of(values: &[&str]) -> Panel {
        let mut p = Panel_new(0, 0, 10, 10, None);
        for v in values {
            p.items.push(li(v));
        }
        p
    }

    #[test]
    fn search_selects_first_forward_match() {
        let mut set = IncSet_new(None);
        set.active = Some(IncType::INC_SEARCH);
        LineEditor_setText(&mut set.modes[IncType::INC_SEARCH as usize].editor, "sh");
        // "systemd" has no "sh"; "bash" and "sshd" do -> first match is idx 1.
        let mut p = panel_of(&["systemd", "bash", "sshd"]);
        assert!(search(&mut set, &mut p, IncSet_getListItemValue));
        assert_eq!(p.selected, 1);
    }

    #[test]
    fn search_no_match_returns_false_and_keeps_selection() {
        let mut set = IncSet_new(None);
        set.active = Some(IncType::INC_SEARCH);
        LineEditor_setText(&mut set.modes[IncType::INC_SEARCH as usize].editor, "zzz");
        let mut p = panel_of(&["systemd", "bash", "sshd"]);
        p.selected = 2;
        assert!(!search(&mut set, &mut p, IncSet_getListItemValue));
        assert_eq!(p.selected, 2); // unchanged on no match
    }

    // ── IncMode_find (IncSet.c:154) ───────────────────────────────────

    #[test]
    fn find_forward_advances_to_next_match() {
        let mut set = IncSet_new(None);
        // Every item contains "sh".
        let mut p = panel_of(&["bash", "zsh", "fish", "dash"]);
        LineEditor_setText(&mut set.modes[IncType::INC_SEARCH as usize].editor, "sh");
        p.selected = 0; // on "bash"
        assert!(IncMode_find(
            &mut set.modes[IncType::INC_SEARCH as usize],
            &mut p,
            IncSet_getListItemValue,
            1,
        ));
        assert_eq!(p.selected, 1); // "zsh"
    }

    #[test]
    fn find_forward_wraps_past_end() {
        let mut set = IncSet_new(None);
        let mut p = panel_of(&["bash", "zsh", "fish", "dash"]);
        // Only "fish" contains the needle "fish".
        LineEditor_setText(&mut set.modes[IncType::INC_SEARCH as usize].editor, "fish");
        p.selected = 3; // on "dash": +1 wraps to 0, scans forward to idx 2
        assert!(IncMode_find(
            &mut set.modes[IncType::INC_SEARCH as usize],
            &mut p,
            IncSet_getListItemValue,
            1,
        ));
        assert_eq!(p.selected, 2);
    }

    #[test]
    fn find_backward_steps_to_prev_match() {
        let mut set = IncSet_new(None);
        let mut p = panel_of(&["bash", "zsh", "fish", "dash"]);
        LineEditor_setText(&mut set.modes[IncType::INC_SEARCH as usize].editor, "sh");
        p.selected = 2; // on "fish"; step -1 -> "zsh"
        assert!(IncMode_find(
            &mut set.modes[IncType::INC_SEARCH as usize],
            &mut p,
            IncSet_getListItemValue,
            -1,
        ));
        assert_eq!(p.selected, 1);
    }

    #[test]
    fn find_no_match_full_loop_returns_false() {
        let mut set = IncSet_new(None);
        let mut p = panel_of(&["bash", "zsh", "fish", "dash"]);
        LineEditor_setText(&mut set.modes[IncType::INC_SEARCH as usize].editor, "nomatch");
        p.selected = 1;
        assert!(!IncMode_find(
            &mut set.modes[IncType::INC_SEARCH as usize],
            &mut p,
            IncSet_getListItemValue,
            1,
        ));
        assert_eq!(p.selected, 1); // returns to `here`, selection untouched
    }

    // ── IncSet_deactivate (IncSet.c:147) ──────────────────────────────

    #[test]
    fn deactivate_clears_active_restores_bar_and_hides_cursor() {
        let default_bar = FunctionBar {
            functions: vec!["DEFAULT".into()],
            keys: vec!["F1".into()],
            events: vec![1],
            staticData: false,
        };
        let mut set = IncSet_new(Some(default_bar));
        set.active = Some(IncType::INC_SEARCH);
        let panel_bar = FunctionBar {
            functions: vec!["DEF".into()],
            keys: vec!["F1".into()],
            events: vec![1],
            staticData: false,
        };
        let mut p = Panel_new(0, 0, 10, 10, Some(panel_bar));
        p.cursorOn = true;
        // Emulate an active search: currentBar swapped to the search bar.
        p.currentBar = Some(set.modes[IncType::INC_SEARCH as usize].bar.clone());

        IncSet_deactivate(&mut set, &mut p);
        assert!(set.active.is_none());
        assert!(!p.cursorOn);
        // Panel_setDefaultBar restored currentBar from the panel's defaultBar.
        assert_eq!(
            p.currentBar.as_ref().unwrap().functions,
            vec!["DEF".to_string()]
        );
    }

    // ── IncSet_filter (IncSet.h:40) ───────────────────────────────────

    #[test]
    fn filter_returns_text_only_when_filtering() {
        let mut set = IncSet_new(None);
        // Not filtering -> None (C NULL).
        assert!(IncSet_filter(&set).is_none());
        // Filtering with text -> the filter mode's editor text.
        IncSet_setFilter(&mut set, "bash");
        assert_eq!(IncSet_filter(&set), Some("bash"));
        // Empty filter clears `filtering` -> None again.
        IncSet_setFilter(&mut set, "");
        assert!(IncSet_filter(&set).is_none());
    }
}
