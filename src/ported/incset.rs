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
//! - the draw/activate pair now that `LineEditor_draw`/`LineEditor_updateScroll`
//!   and `FunctionBar_drawExtra` are ported: [`IncSet_drawBar`] (`:302`) and
//!   [`IncSet_activate`] (`:136`). The C `this->panel` back-pointer has no safe
//!   analog, so both thread the `Panel` as a parameter instead (see struct
//!   mapping); `IncSet_drawBar` also takes `&mut IncSet` because
//!   `LineEditor_updateScroll` mutates the active editor,
//! - [`IncSet_filter`] (`IncSet.h:40`) — the filter-text accessor,
//! - [`updateWeakPanel`] (`:96`) and [`IncSet_handleKey`] (`:177`) — the
//!   "weak panel" filter/key path. htop shares one `Object*` between the
//!   `Vector* lines` and the panel (`Panel_add(panel, (Object*)line)` aliases a
//!   `Vector`-owned pointer, `selected == line` is a raw-pointer identity test).
//!   This is modeled faithfully with [`PanelItem::Borrowed`] raw pointers into
//!   the `lines`-owned `Box`es (the same weak-panel idiom `infoscreen.rs` uses)
//!   and a data-pointer identity compare — see [`updateWeakPanel`]'s SAFETY
//!   note. `IncSet_handleKey` threads `lines: &mut Vector` through to it.
//!
//! [`IncSet_delete`] (`:77`) and [`IncMode_done`] (`:61`) are now ported: the
//! C `free` chain maps to the by-value drop idiom [`FunctionBar_delete`] uses
//! (each mode's owned `FunctionBar` is handed to `FunctionBar_delete`; the
//! `Option<History>`/`defaultBar` drop with the struct).
//!
//! # Struct mapping
//!
//! - C `IncMode* active` (points into `modes[]`) → `Option<IncType>` (which
//!   of the two modes is active), avoiding a self-referential borrow.
//! - C `Panel* panel` back-pointer (`IncSet.h:33`) and `History* history` are
//!   omitted: the back-pointer would alias a `&mut Panel` owned elsewhere, so
//!   the functions that use it ([`IncSet_activate`], [`IncSet_drawBar`], and
//!   [`IncSet_synthesizeEvent`]) thread the panel as a parameter instead; all
//!   their call sites have it in scope.
//! - C `FunctionBar* defaultBar` → owned `Option<FunctionBar>`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::io::{self, Write};

use crate::ported::crt::{
    ColorElements, ColorScheme, ERR, KEY_DOWN, KEY_ENTER, KEY_F, KEY_MOUSE, KEY_RECLICK,
    KEY_RESIZE, KEY_UP,
};
use crate::ported::functionbar::{
    FunctionBar, FunctionBar_delete, FunctionBar_draw, FunctionBar_drawExtra, FunctionBar_getWidth,
    FunctionBar_new, FunctionBar_synthesizeEvent, Ncurses,
};
use crate::ported::history::{
    History, History_add, History_navigate, History_new, History_resetPosition, History_save,
};
use crate::ported::lineeditor::{
    LineEditor, LineEditor_click, LineEditor_draw, LineEditor_getText, LineEditor_handleKey,
    LineEditor_init, LineEditor_reset, LineEditor_setText, LineEditor_updateScroll,
};
use crate::ported::listitem::ListItem;
use crate::ported::object::Object;
use crate::ported::panel::{
    Panel, PanelItem, Panel_get, Panel_getSelected, Panel_getSelectedIndex, Panel_prune,
    Panel_setDefaultBar, Panel_setSelected, Panel_size, KEY_MOUSE_BAR_CLICK,
};
use crate::ported::vector::{Vector, Vector_get, Vector_size};
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

/// Port of `static inline void IncMode_done(IncMode* mode)` from
/// `IncSet.c:61`. C `FunctionBar_delete(mode->bar)`. The [`IncMode`] owns its
/// `bar` by value, so taking `mode` by value and handing `mode.bar` to
/// [`FunctionBar_delete`] frees the bar exactly as the C does; the remaining
/// fields (`editor`, `isFilter`) drop at end of scope, which is the caller's
/// `free` of the enclosing struct.
fn IncMode_done(mode: IncMode) {
    FunctionBar_delete(mode.bar);
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

/// Port of `void IncSet_delete(IncSet* this)` from `IncSet.c:77`:
/// `IncMode_done(&modes[0]); IncMode_done(&modes[1]); if (history)
/// History_delete(history); free(this);`.
///
/// Taking `this` by value reproduces `free(this)`. The `modes` array is moved
/// out and each [`IncMode`] handed to [`IncMode_done`] (mirroring the C call
/// graph, which frees each mode's `FunctionBar`). The `Option<History>`
/// `history` and the owned `Option<FunctionBar>` `defaultBar` drop with the
/// remaining fields — the faithful analog of `History_delete(history)` (a
/// `Drop` no-op in `history.rs`, so calling the stub is avoided) and the
/// struct free.
pub fn IncSet_delete(this: IncSet) {
    let IncSet { modes, .. } = this;
    let [search, filter] = modes;
    IncMode_done(search);
    IncMode_done(filter);
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

/// Port of `static void updateWeakPanel(IncSet* this, Panel* panel, Vector*
/// lines)` from `IncSet.c:96`. Rebuilds the weak `panel` from the backing
/// `lines`: prunes the panel, then (when filtering) re-adds only the items whose
/// value matches the filter, else every item — preserving the selection by
/// raw-pointer identity.
///
/// **Weak panel (raw-pointer alias).** htop's `Panel_add(panel,
/// (Object*)line)` aliases a `Vector`-owned pointer into the non-owning panel;
/// `selected == (Object*)line` is a raw-pointer identity test. Modeled
/// faithfully: each re-added item is a [`PanelItem::Borrowed`] raw pointer into
/// the `lines`-owned `Box` (the same weak-panel idiom `infoscreen.rs`'s
/// `InfoScreen_addLine` uses), and `selected == line` is a data-pointer compare
/// (`*const dyn Object as *const ()`).
///
/// SAFETY: `lines` owns the `ListItem` boxes (heap-stable across `Vector`
/// growth); the panel only borrows them and is pruned/rebuilt whenever `lines`
/// changes, so the borrowed pointers are never dereferenced after their owner
/// is freed. This mirrors htop's non-owning `Panel` exactly.
fn updateWeakPanel(this: &IncSet, panel: &mut Panel, lines: &mut Vector) {
    // C: const Object* selected = Panel_getSelected(panel);
    let selected: *const () = match Panel_getSelected(panel) {
        Some(o) => o as *const dyn Object as *const (),
        None => core::ptr::null(),
    };
    // C: Panel_prune(panel);
    Panel_prune(panel);

    if this.filtering {
        // C: int n = 0;
        //    const char* incFilter = LineEditor_getText(&this->modes[INC_FILTER].editor);
        let mut n: i32 = 0;
        let incFilter =
            LineEditor_getText(&this.modes[IncType::INC_FILTER as usize].editor).to_string();
        for i in 0..Vector_size(lines) as usize {
            // C: ListItem* line = (ListItem*)Vector_get(lines, i);
            //    if (String_contains_i(line->value, incFilter, true)) { ... }
            let matches = {
                let obj: &dyn core::any::Any = Vector_get(lines, i);
                let value = &obj
                    .downcast_ref::<ListItem>()
                    .expect("weak panel lines are ListItem")
                    .value;
                String_contains_i(value, &incFilter, true)
            };
            if matches {
                // C: Panel_add(panel, (Object*)line); — weak (borrowed) add.
                let ptr: *mut dyn Object = &mut **lines.array[i].as_mut().unwrap();
                let line_id = ptr as *const dyn Object as *const ();
                panel.items.push(PanelItem::Borrowed(ptr));
                panel.prevSelected = -1;
                panel.needsRedraw = true;
                // C: if (selected == (Object*)line) Panel_setSelected(panel, n);
                if selected == line_id {
                    Panel_setSelected(panel, n);
                }
                n += 1;
            }
        }
    } else {
        for i in 0..Vector_size(lines) as usize {
            // C: Object* line = Vector_get(lines, i); Panel_add(panel, line);
            let ptr: *mut dyn Object = &mut **lines.array[i].as_mut().unwrap();
            let line_id = ptr as *const dyn Object as *const ();
            panel.items.push(PanelItem::Borrowed(ptr));
            panel.prevSelected = -1;
            panel.needsRedraw = true;
            // C: if (selected == line) Panel_setSelected(panel, i);
            if selected == line_id {
                Panel_setSelected(panel, i as i32);
            }
        }
    }
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

/// Port of `IncSet.c:136`. Activates a mode: sets `active`, swaps the
/// panel's `currentBar` to the mode's bar, turns the cursor on, resets the
/// history browse position, and redraws the bar via [`IncSet_drawBar`].
///
/// The C `this->panel = panel` back-pointer (`IncSet.h:33`) has no safe-Rust
/// analog (it would alias a `&mut Panel` owned elsewhere), so the panel is
/// threaded as a parameter and forwarded to [`IncSet_drawBar`] instead — the
/// only two call sites (this fn and `IncSet_handleKey`) both have the panel in
/// scope, so the back-pointer is never needed. `panel->currentBar =
/// this->active->bar` shares one `FunctionBar*` in C; the owned-`FunctionBar`
/// model stores a clone (as `Panel_setDefaultBar`/`Panel_init` already do).
pub fn IncSet_activate(this: &mut IncSet, type_: IncType, panel: &mut Panel) {
    this.active = Some(type_);
    panel.currentBar = Some(this.modes[type_ as usize].bar.clone());
    panel.cursorOn = true;
    /* Reset history browse position when starting a new search/filter */
    if let Some(history) = &mut this.history {
        History_resetPosition(history);
    }
    IncSet_drawBar(
        this,
        panel,
        ColorElements::FUNCTION_BAR.packed(ColorScheme::active()),
    );
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

/// Port of `bool IncSet_handleKey(IncSet* this, int ch, Panel* panel,
/// IncMode_GetPanelValue getPanelValue, Vector* lines)` from `IncSet.c:177`.
/// The key dispatcher: F3/Shift-F3 next/prev, history up/down, Enter/Esc
/// confirm/abort, mouse bar-click cursor placement, and the line-editor
/// char/backspace path; it then runs the search (when `doSearch`), refreshes the
/// weak panel (when `filterChanged`) via [`updateWeakPanel`], and redraws the
/// bar. Returns whether the filter changed (C `bool`).
///
/// The C `IncMode* mode = this->active` (a pointer into `modes[]`) resolves
/// through `active: Option<IncType>` — the caller only dispatches keys with an
/// active mode, so `active` is non-`None` here; the mode is reached as
/// `this.modes[active as usize]`. `Vector* lines` is threaded as `&mut Vector`
/// so [`updateWeakPanel`] can alias its boxes into the panel (see its note).
pub fn IncSet_handleKey(
    this: &mut IncSet,
    ch: i32,
    panel: &mut Panel,
    getPanelValue: IncMode_GetPanelValue,
    lines: &mut Vector,
) -> bool {
    // C: if (ch == ERR) return true;
    if ch == ERR {
        return true;
    }

    // C: IncMode* mode = this->active;
    let mode = this
        .active
        .expect("IncSet_handleKey called with no active mode");
    let midx = mode as usize;
    // C: int size = Panel_size(panel);
    let size = Panel_size(panel);
    let mut filterChanged = false;
    let mut doSearch = true;

    let functionBar = ColorElements::FUNCTION_BAR.packed(ColorScheme::active());

    // C: if (ch == KEY_MOUSE_BAR_CLICK) { ... IncSet_drawBar; return false; }
    if ch == KEY_MOUSE_BAR_CLICK {
        let fieldStartX = FunctionBar_getWidth(&this.modes[midx].bar);
        LineEditor_click(
            &mut this.modes[midx].editor,
            panel.lastMouseBarClickX,
            fieldStartX,
        );
        IncSet_drawBar(this, panel, functionBar);
        return false;
    }

    if ch == KEY_F(3) || ch == KEY_F(15) {
        // C: if (size == 0) return true;
        if size == 0 {
            return true;
        }
        // C: IncMode_find(mode, panel, getPanelValue, ch == KEY_F(3) ? 1 : -1);
        IncMode_find(
            &mut this.modes[midx],
            panel,
            getPanelValue,
            if ch == KEY_F(3) { 1 } else { -1 },
        );
        doSearch = false;
    } else if ch == KEY_UP {
        // C: History navigation: older entry
        if this.history.is_some() {
            let entry = History_navigate(
                this.history.as_mut().unwrap(),
                &this.modes[midx].editor,
                true,
            )
            .map(str::to_string);
            if let Some(entry) = entry {
                LineEditor_setText(&mut this.modes[midx].editor, &entry);
                if this.modes[midx].isFilter {
                    filterChanged = true;
                    this.filtering = !LineEditor_getText(&this.modes[midx].editor).is_empty();
                }
            }
            doSearch = !this.modes[midx].isFilter;
        } else {
            doSearch = false;
        }
    } else if ch == KEY_DOWN {
        // C: History navigation: newer entry
        if this.history.is_some() {
            let entry = History_navigate(
                this.history.as_mut().unwrap(),
                &this.modes[midx].editor,
                false,
            )
            .map(str::to_string);
            if let Some(entry) = entry {
                LineEditor_setText(&mut this.modes[midx].editor, &entry);
                if this.modes[midx].isFilter {
                    filterChanged = true;
                    this.filtering = !LineEditor_getText(&this.modes[midx].editor).is_empty();
                }
            }
            doSearch = !this.modes[midx].isFilter;
        } else {
            doSearch = false;
        }
    } else if ch == KEY_RESIZE {
        // C: doSearch = (LineEditor_getText(&mode->editor)[0] != '\0');
        doSearch = !LineEditor_getText(&this.modes[midx].editor).is_empty();
    } else if ch == 13 || ch == b'\r' as i32 || ch == KEY_ENTER {
        // C: Enter confirms: add to history and deactivate
        if this.history.is_some() {
            let text = LineEditor_getText(&this.modes[midx].editor).to_string();
            if !text.is_empty() {
                History_add(this.history.as_mut().unwrap(), &text);
                History_save(this.history.as_ref().unwrap());
            }
            History_resetPosition(this.history.as_mut().unwrap());
        }
        if !this.modes[midx].isFilter {
            // C: For search: reset buffer on Enter
            IncMode_reset(&mut this.modes[midx]);
        }
        IncSet_deactivate(this, panel);
        doSearch = false;
        filterChanged = this.modes[midx].isFilter;
    } else if ch == 27 || ch == KEY_MOUSE || ch == KEY_RECLICK {
        // C: Esc or panel click aborts
        if this.history.is_some() {
            History_resetPosition(this.history.as_mut().unwrap());
        }
        if this.modes[midx].isFilter {
            filterChanged = true;
            this.filtering = false;
            IncMode_reset(&mut this.modes[midx]);
        } else {
            this.found = false;
            IncMode_reset(&mut this.modes[midx]);
        }
        IncSet_deactivate(this, panel);
        doSearch = false;
    } else {
        // C: Try line editor first
        let textChanged = LineEditor_handleKey(&mut this.modes[midx].editor, ch);
        if textChanged {
            let empty = LineEditor_getText(&this.modes[midx].editor).is_empty();
            if this.modes[midx].isFilter {
                filterChanged = true;
                this.filtering = !empty;
            } else if empty {
                // C: Buffer emptied in search mode: clear stale found state
                this.found = false;
                doSearch = false;
            }
        } else {
            // C: Key was a movement key (no text change) or unrecognized
            doSearch = false;
        }
    }

    // C: if (doSearch && LineEditor_getText(&mode->editor)[0] != '\0')
    //        this->found = search(this, panel, getPanelValue);
    if doSearch && !LineEditor_getText(&this.modes[midx].editor).is_empty() {
        this.found = search(this, panel, getPanelValue);
    }
    // C: if (filterChanged && lines) updateWeakPanel(this, panel, lines);
    if filterChanged {
        updateWeakPanel(this, panel, lines);
    }

    // C: if (this->active) IncSet_drawBar(this, CRT_colors[FUNCTION_BAR]);
    if this.active.is_some() {
        IncSet_drawBar(this, panel, functionBar);
    }

    filterChanged
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

/// Port of `IncSet.c:302`. Draws the active mode's function bar and line
/// editor, or the default bar when no mode is active. When searching with a
/// non-empty, not-yet-found buffer the bar is drawn in `FAILED_SEARCH`.
///
/// Two faithful adaptations: the C `this->panel->cursorY`/`cursorX` writes go
/// through the panel threaded as a parameter (the `Panel*` back-pointer is not
/// modeled — see [`IncSet_activate`]); and `this` is `&mut` (not the C `const
/// IncSet*`) because `LineEditor_updateScroll` mutates `this->active->editor`,
/// which in C is reached through the non-const `IncMode* active` pointer but
/// here through the `Option<IncType>` index, requiring `&mut self`. The C
/// `this->active->editor.len > 0` test uses the (private) editor length; the
/// equivalent `!LineEditor_getText(..).is_empty()` reads the same buffer.
pub fn IncSet_drawBar(this: &mut IncSet, panel: &mut Panel, attr: i32) {
    if let Some(active) = this.active {
        let idx = active as usize;
        let mut attr = attr;
        if !this.modes[idx].isFilter
            && !this.found
            && !LineEditor_getText(&this.modes[idx].editor).is_empty()
        {
            attr = ColorElements::FAILED_SEARCH.packed(ColorScheme::active());
        }

        /* Draw the function keys and get the start of the input field */
        let fieldStartX = FunctionBar_drawExtra(&this.modes[idx].bar, None, -1, false);

        /* Update scroll so the cursor remains visible */
        let mut fieldWidth = Ncurses::cols() - fieldStartX;
        if fieldWidth < 1 {
            fieldWidth = 1;
        }
        LineEditor_updateScroll(&mut this.modes[idx].editor, fieldWidth);

        /* Draw the visible portion of the input text */
        let cursorX = LineEditor_draw(&this.modes[idx].editor, fieldStartX, fieldWidth, attr);

        {
            let mut out = io::stdout().lock();
            Ncurses::curs_set(&mut out, true);
            let _ = out.flush();
        }

        panel.cursorY = Ncurses::lines() - 1;
        panel.cursorX = cursorX;
    } else if let Some(bar) = &this.defaultBar {
        FunctionBar_draw(bar);
    }
}

/// Port of `IncSet.c:327`. Turns a bar x-coordinate into a synthesized
/// event via `FunctionBar_synthesizeEvent`, returning `KEY_MOUSE_BAR_CLICK`
/// (and stashing `x` in `panel->lastMouseBarClickX`) for a click in the input
/// area, else the slot event (or the default bar's event when no mode is
/// active).
///
/// The C `this->panel->lastMouseBarClickX = x` write goes through the panel
/// threaded as a parameter (the `Panel*` back-pointer is not modeled — see
/// [`IncSet_activate`]/[`IncSet_drawBar`]); the only field mutated is the
/// panel's, so `this` stays `&IncSet`. `this->active->bar` resolves through
/// `active: Option<IncType>` into `modes[..].bar`. The C else branch
/// dereferences `this->defaultBar` unconditionally; `as_ref().unwrap()`
/// reproduces that non-NULL dereference (same idiom as `search`).
pub fn IncSet_synthesizeEvent(this: &IncSet, panel: &mut Panel, x: i32) -> i32 {
    if let Some(active) = this.active {
        let bar = &this.modes[active as usize].bar;
        let ev = FunctionBar_synthesizeEvent(bar, x);
        /* Click in the input area: synthesize a bar-click event */
        if ev == ERR && x >= FunctionBar_getWidth(bar) {
            panel.lastMouseBarClickX = x;
            return KEY_MOUSE_BAR_CLICK;
        }
        ev
    } else {
        FunctionBar_synthesizeEvent(this.defaultBar.as_ref().unwrap(), x)
    }
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
        assert_eq!(
            bar.functions,
            vec!["Next  ", "Prev   ", "Cancel ", " Search: "]
        );
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
        p.items.push(PanelItem::Owned(li("systemd")));
        p.items.push(PanelItem::Owned(li("bash")));
        p.items.push(PanelItem::Owned(li("htop")));
        assert_eq!(IncSet_getListItemValue(&p, 0), "systemd");
        assert_eq!(IncSet_getListItemValue(&p, 1), "bash");
        assert_eq!(IncSet_getListItemValue(&p, 2), "htop");
    }

    #[test]
    fn get_list_item_value_usable_as_fn_pointer() {
        // It must satisfy the IncMode_GetPanelValue callback type.
        let f: IncMode_GetPanelValue = IncSet_getListItemValue;
        let mut p = Panel_new(0, 0, 10, 10, None);
        p.items.push(PanelItem::Owned(li("firefox")));
        assert_eq!(f(&p, 0), "firefox");
    }

    #[test]
    fn get_list_item_value_composes_with_string_contains_i() {
        // Demonstrates the two ported primitives the (stubbed) search would
        // use: getPanelValue + String_contains_i, case-insensitive.
        use crate::ported::xutils::String_contains_i;
        let mut p = Panel_new(0, 0, 10, 10, None);
        for v in ["systemd", "bash", "htop", "sshd"] {
            p.items.push(PanelItem::Owned(li(v)));
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
            p.items.push(PanelItem::Owned(li(v)));
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
        LineEditor_setText(
            &mut set.modes[IncType::INC_SEARCH as usize].editor,
            "nomatch",
        );
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

    // ── updateWeakPanel (IncSet.c:96) ─────────────────────────────────

    use crate::ported::listitem::ListItem_new;
    use crate::ported::vector::{Vector_add, Vector_new};

    /// A `lines` Vector of ListItems (owner=true, as InfoScreen builds it).
    fn lines_of(values: &[&str]) -> Vector {
        let klass = ListItem_new("", 0).klass();
        let mut lines = Vector_new(klass, true, 10);
        for v in values {
            Vector_add(&mut lines, Box::new(ListItem_new(v, 0)));
        }
        lines
    }

    #[test]
    fn update_weak_panel_non_filtering_re_adds_all() {
        let set = IncSet_new(None); // filtering == false
        let mut lines = lines_of(&["systemd", "bash", "sshd", "htop"]);
        let mut panel = Panel_new(0, 0, 10, 10, None);

        updateWeakPanel(&set, &mut panel, &mut lines);

        assert_eq!(Panel_size(&panel), 4);
        assert_eq!(IncSet_getListItemValue(&panel, 0), "systemd");
        assert_eq!(IncSet_getListItemValue(&panel, 1), "bash");
        assert_eq!(IncSet_getListItemValue(&panel, 3), "htop");
    }

    #[test]
    fn update_weak_panel_filtering_keeps_only_matches() {
        let mut set = IncSet_new(None);
        IncSet_setFilter(&mut set, "sh"); // filtering == true
        let mut lines = lines_of(&["systemd", "bash", "sshd", "htop"]);
        let mut panel = Panel_new(0, 0, 10, 10, None);

        updateWeakPanel(&set, &mut panel, &mut lines);

        // Only "bash" and "sshd" contain "sh".
        assert_eq!(Panel_size(&panel), 2);
        assert_eq!(IncSet_getListItemValue(&panel, 0), "bash");
        assert_eq!(IncSet_getListItemValue(&panel, 1), "sshd");
    }

    #[test]
    fn update_weak_panel_preserves_selection_by_pointer_identity() {
        // First (non-filtering) build aliases the lines boxes into the panel;
        // selecting index 2 then rebuilding must keep the selection on the same
        // *object* (sshd), via the `selected == line` raw-pointer identity test.
        let set = IncSet_new(None);
        let mut lines = lines_of(&["systemd", "bash", "sshd", "htop"]);
        let mut panel = Panel_new(0, 0, 10, 10, None);
        updateWeakPanel(&set, &mut panel, &mut lines);
        panel.selected = 2; // "sshd" (aliases lines box 2)

        updateWeakPanel(&set, &mut panel, &mut lines);
        assert_eq!(panel.selected, 2);
        assert_eq!(IncSet_getListItemValue(&panel, panel.selected), "sshd");
    }

    #[test]
    fn update_weak_panel_selection_follows_object_across_filter() {
        // Select "bash" in the full view, then filter "sh": bash moves to the
        // new index 0 and the selection follows the object by identity.
        let mut set = IncSet_new(None);
        let mut lines = lines_of(&["systemd", "bash", "sshd", "htop"]);
        let mut panel = Panel_new(0, 0, 10, 10, None);
        updateWeakPanel(&set, &mut panel, &mut lines);
        panel.selected = 1; // "bash"

        IncSet_setFilter(&mut set, "sh");
        updateWeakPanel(&set, &mut panel, &mut lines);
        assert_eq!(Panel_size(&panel), 2);
        assert_eq!(panel.selected, 0); // bash is now first
        assert_eq!(IncSet_getListItemValue(&panel, panel.selected), "bash");
    }
}
