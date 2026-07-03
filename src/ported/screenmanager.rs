//! Port of `ScreenManager.c` — htop's panel layout manager and main loop.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
//!
//! # Data model
//!
//! htop's `ScreenManager` (`ScreenManager.h:19`) holds a layout rectangle
//! (`x1/y1/x2/y2`), a `Vector* panels`, a `panelCount`, plus `Header*`,
//! `Machine*` and `State*` back-pointers. The [`ScreenManager`] struct here
//! models the layout rectangle, the `panels` as a `Vec<Panel>`, the count,
//! the focus-change flag, the tab `name`, and the three back-pointers as
//! owned `Option<T>` fields — the same convention `machine.rs` uses for
//! C's `Settings*` back-pointer (`settings: Option<Settings>`). `header` is
//! `Option<Header>` because C null-checks it (`if (this->header)`); `state`
//! is dereferenced unconditionally in C, so the layout ops `.unwrap()` it
//! (matching machine.rs's `settings.as_ref().unwrap()`). `host` is stored by
//! [`ScreenManager_new`] but read by none of the ported functions.
//!
//! Now that `State` (`action.rs`), `Header` (`header.rs`) and `Machine`
//! (`machine.rs`) are modeled, and `Ncurses::lines()`/`cols()`
//! (`functionbar.rs`) provide the ncurses `LINES`/`COLS` globals, the whole
//! layout engine (`header_height` + insert/add/resize) ports faithfully.
//!
//! # What is ported
//!
//! - [`ScreenManager_new`] — stores the layout defaults and the
//!   `Header`/`Machine`/`State` back-pointers (the `owner` arg only typed
//!   the C `Vector`; a `Vec<Panel>` always owns, so it is dropped, exactly
//!   as `Panel_new` drops its `type`/`owner`).
//! - [`ScreenManager_size`] — returns `panelCount` (pure).
//! - [`header_height`] — reads `state->hideMeters` and `header->height`.
//! - [`ScreenManager_insert`] / [`ScreenManager_add`] — place a panel and
//!   size it to `LINES`/`COLS` less the layout insets and header band.
//! - [`ScreenManager_remove`] — removes a panel and shifts the panels to
//!   its right leftward by its width (the only layout op that does **not**
//!   call `header_height`).
//! - [`ScreenManager_resize`] — re-lays every panel across the width.
//! - [`drawTab`] — behavioral crossterm port of the single tab primitive
//!   through the [`Ncurses`] shim; its `*x` advancement / return value are
//!   unit tested by driving it with an in-memory geometry.
//!
//! # What stays a stub (and why)
//!
//! - [`ScreenManager_delete`] — `Vector_delete` + `free`; released by `Drop`.
//! - [`checkRecalculation`] — `Platform_gettime_realtime`, `Machine_scan`,
//!   `Machine_scanTables`, `Platform_getFailedState`, `Header_updateData`,
//!   `Table_rebuildPanel`, `Header_draw`, and the `Process_uidDigits`/
//!   `Process_pidDigits` globals are all unported.
//! - [`ScreenManager_drawScreenTabs`] — iterates `settings->screens` at
//!   `settings->ssIndex` and prints each `ScreenSettings->heading`; the
//!   ported `machine::Settings` has no `ssIndex` and `machine::ScreenSettings`
//!   has no `heading`.
//! - [`ScreenManager_drawPanels`] — reads `settings->screenTabs`,
//!   `state->mainPanel` (the ported `State` has no `mainPanel` pointer to
//!   compare against) and `State_hideFunctionBar` (not ported).
//! - [`ScreenManager_run`] — the main loop: `checkRecalculation`,
//!   `drawPanels`, mouse handling (`getmouse`/`MEVENT`), the `Panel`
//!   event-handler vtable, and `settings->enableMouse`/`screenTabs` — all
//!   bound to the unported substrate above.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::io::{self, Write};

use crate::ported::action::State;
use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::functionbar::Ncurses;
use crate::ported::header::Header;
use crate::ported::machine::Machine;
use crate::ported::panel::{Panel, Panel_move, Panel_resize};

/// Port of `#define SCREEN_TAB_MARGIN_LEFT 2` (`CRT.h:17`).
const SCREEN_TAB_MARGIN_LEFT: i32 = 2;
/// Port of `#define SCREEN_TAB_COLUMN_GAP 1` (`CRT.h:18`).
const SCREEN_TAB_COLUMN_GAP: i32 = 1;

/// Model of htop's `struct ScreenManager_` (`ScreenManager.h:19`). The C
/// `Header*`/`Machine*`/`State*` back-pointers become owned `Option<T>`
/// fields (see the module docs); `panels` is the `Vec` analog of the C
/// `Vector* panels`.
pub struct ScreenManager {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub allowFocusChange: bool,
    pub panelCount: u32,
    pub panels: Vec<Panel>,
    pub name: Option<String>,
    pub header: Option<Header>,
    pub host: Option<Machine>,
    pub state: Option<State>,
}

impl ScreenManager {
    /// A ScreenManager with the C `ScreenManager_new` layout defaults
    /// (`x1=y1=x2=0`, `y2=-1`, `allowFocusChange=true`, no panels) and no
    /// `Header`/`Machine`/`State` wired. Gate-skipped associated fn (no C
    /// 1:1 analog on its own); used by the tests to exercise the ported
    /// layout ops, which set `state`/`header` as each case requires.
    fn empty() -> ScreenManager {
        ScreenManager {
            x1: 0,
            y1: 0,
            x2: 0,
            y2: -1,
            allowFocusChange: true,
            panelCount: 0,
            panels: Vec::new(),
            name: None,
            header: None,
            host: None,
            state: None,
        }
    }
}

/// Port of `ScreenManager* ScreenManager_new(Header* header, Machine* host,
/// State* state, bool owner)` from `ScreenManager.c:31`.
///
/// Sets the layout defaults (`x1=y1=x2=0`, `y2=-1`, `allowFocusChange`) and
/// stores the three back-pointers. The `owner` arg only typed the C
/// `Vector` (whether it frees its items); a `Vec<Panel>` always owns, so it
/// is dropped, exactly as [`Panel_new`](crate::ported::panel::Panel_new)
/// drops its `type`/`owner`. `header` is `NULL`-able (`Option<Header>`);
/// `host`/`state` are always-present pointers taken by value and wrapped in
/// `Some`.
pub fn ScreenManager_new(header: Option<Header>, host: Machine, state: State) -> ScreenManager {
    ScreenManager {
        x1: 0,
        y1: 0,
        x2: 0,
        y2: -1,
        allowFocusChange: true,
        panelCount: 0,
        panels: Vec::new(),
        name: None,
        header,
        host: Some(host),
        state: Some(state),
    }
}

/// TODO: port of `void ScreenManager_delete(ScreenManager* this)` from
/// `ScreenManager.c:47`. `Vector_delete` + `free` — released by `Drop`.
pub fn ScreenManager_delete() {
    todo!("port of ScreenManager.c:47 — Drop releases the panels")
}

/// Port of `inline int ScreenManager_size(const ScreenManager* this)` from
/// `ScreenManager.c:52`: returns `panelCount`.
pub fn ScreenManager_size(this: &ScreenManager) -> i32 {
    this.panelCount as i32
}

/// Port of `void ScreenManager_add(ScreenManager* this, Panel* item,
/// int size)` from `ScreenManager.c:56`. Inserts `item` at the end
/// (`Vector_size(this->panels)` == `panelCount`).
pub fn ScreenManager_add(this: &mut ScreenManager, item: Panel, size: i32) {
    let idx = this.panels.len() as i32;
    ScreenManager_insert(this, item, size, idx);
}

/// Port of `static int header_height(const ScreenManager* this)` from
/// `ScreenManager.c:60`. Returns `0` when `state->hideMeters` is set, else
/// `header->height` when a header is present, else `0`. C dereferences
/// `state` unconditionally, so `state` is `.unwrap()`ed here (matching
/// `machine.rs`'s always-present `settings.as_ref().unwrap()` convention).
pub fn header_height(this: &ScreenManager) -> i32 {
    if this.state.as_ref().unwrap().hideMeters {
        return 0;
    }

    if let Some(header) = &this.header {
        return header.height;
    }

    0
}

/// Port of `void ScreenManager_insert(ScreenManager* this, Panel* item,
/// int size, int idx)` from `ScreenManager.c:70`.
///
/// Positions the new panel to the right of its predecessor, sizes it to the
/// available height (`LINES - y1 - header_height + y2`) and — when `size <= 0`
/// — to the remaining width (`COLS - x1 + x2 - lastX`), then inserts it and
/// bumps `panelCount`. `LINES`/`COLS` come from `Ncurses::lines()`/`cols()`.
pub fn ScreenManager_insert(this: &mut ScreenManager, mut item: Panel, mut size: i32, idx: i32) {
    let mut lastX = 0;
    if idx > 0 {
        let last = &this.panels[(idx - 1) as usize];
        lastX = last.x + last.w + 1;
    }
    let hh = header_height(this);
    let height = Ncurses::lines() - this.y1 - hh + this.y2;
    if size <= 0 {
        size = Ncurses::cols() - this.x1 + this.x2 - lastX;
    }
    Panel_resize(&mut item, size, height);
    Panel_move(&mut item, lastX, this.y1 + hh);
    if (idx as u32) < this.panelCount {
        // Faithful to the C loop `for (i = idx + 1; i <= panelCount; i++)`:
        // it shifts the existing panels right of `idx`. The `<= panelCount`
        // bound reads one past the last panel (C `Vector_get` would assert);
        // this path is unreachable in htop because `ScreenManager_add` always
        // inserts at `idx == panelCount`, so the guard above is false.
        for i in (idx + 1)..=(this.panelCount as i32) {
            let (px, py) = (this.panels[i as usize].x, this.panels[i as usize].y);
            Panel_move(&mut this.panels[i as usize], px + size, py);
        }
    }
    item.needsRedraw = true;
    this.panels.insert(idx as usize, item);
    this.panelCount += 1;
}

/// Port of `Panel* ScreenManager_remove(ScreenManager* this, int idx)` from
/// `ScreenManager.c:93`. Removes the panel at `idx` and shifts every panel
/// to its right leftward by the removed panel's width. This is the only
/// ScreenManager layout op that does not call `header_height`.
pub fn ScreenManager_remove(this: &mut ScreenManager, idx: i32) -> Panel {
    debug_assert!((idx as u32) < this.panelCount);
    let w = this.panels[idx as usize].w;
    let panel = this.panels.remove(idx as usize);
    this.panelCount -= 1;
    if (idx as u32) < this.panelCount {
        for i in (idx as usize)..(this.panelCount as usize) {
            let (px, py) = (this.panels[i].x, this.panels[i].y);
            Panel_move(&mut this.panels[i], px - w, py);
        }
    }
    panel
}

/// Port of `void ScreenManager_resize(ScreenManager* this)` from
/// `ScreenManager.c:107`.
///
/// Re-lays every panel: each non-last panel keeps its width and gets the
/// full available height, the last panel takes the remaining width. `y1_header`
/// is `y1 + header_height`; heights are `LINES - y1_header + y2`, the last
/// width is `COLS - x1 + x2 - lastX`. Reads `panels[panelCount - 1]`
/// unconditionally, so requires `panelCount >= 1` (as in C, where an empty
/// vector would assert).
pub fn ScreenManager_resize(this: &mut ScreenManager) {
    let y1_header = this.y1 + header_height(this);
    let panels = this.panelCount as i32;
    let lines = Ncurses::lines();
    let cols = Ncurses::cols();
    let mut lastX = 0;
    for i in 0..(panels - 1) {
        let w = this.panels[i as usize].w;
        Panel_resize(&mut this.panels[i as usize], w, lines - y1_header + this.y2);
        Panel_move(&mut this.panels[i as usize], lastX, y1_header);
        let panel = &this.panels[i as usize];
        lastX = panel.x + panel.w + 1;
    }
    let last = (panels - 1) as usize;
    Panel_resize(
        &mut this.panels[last],
        cols - this.x1 + this.x2 - lastX,
        lines - y1_header + this.y2,
    );
    Panel_move(&mut this.panels[last], lastX, y1_header);
}

/// TODO: port of `static void checkRecalculation(...)` from
/// `ScreenManager.c:122`. Samples `Machine`/`Platform` metrics
/// (`Platform_gettime_realtime`, `Machine_scan`, `Machine_scanTables`,
/// `Platform_getFailedState`), rebuilds the active `Table`
/// (`Table_rebuildPanel`), and redraws the `Header` (`Header_updateData`,
/// `Header_draw`) — all unported, plus the `Process_uidDigits`/
/// `Process_pidDigits` globals.
pub fn checkRecalculation() {
    todo!("port of ScreenManager.c:122 — needs Machine/Platform/Header/Table sampling")
}

/// Port of `static inline bool drawTab(const int* y, int* x, int l,
/// const char* name, bool cur)` from `ScreenManager.c:171`.
///
/// Behavioral crossterm port through the [`Ncurses`] shim: draws
/// `[name]` at column `*x` on row `y` (borders in `SCREENS_{CUR,OTH}_BORDER`,
/// the name in `SCREENS_{CUR,OTH}_TEXT`), advancing `*x` and returning
/// `false` as soon as the tab would overflow the line width `l`. The `*x`
/// advancement and the boolean result are pure and unit tested (a
/// `Vec<u8>` sink stands in for the terminal).
pub fn drawTab(y: i32, x: &mut i32, l: i32, name: &str, cur: bool) -> bool {
    debug_assert!(*x >= 0);
    debug_assert!(*x < l);

    let scheme = ColorScheme::active();
    let border = if cur {
        ColorElements::SCREENS_CUR_BORDER
    } else {
        ColorElements::SCREENS_OTH_BORDER
    }
    .packed(scheme);
    let text = if cur {
        ColorElements::SCREENS_CUR_TEXT
    } else {
        ColorElements::SCREENS_OTH_TEXT
    }
    .packed(scheme);

    let mut out = io::stdout().lock();

    Ncurses::attrset(&mut out, border);
    Ncurses::mvaddch(&mut out, y, *x, '[');
    *x += 1;
    if *x >= l {
        let _ = out.flush();
        return false;
    }

    // int nameWidth = (int)strnlen(name, l - *x);
    let name_width = name.len().min((l - *x) as usize) as i32;
    Ncurses::attrset(&mut out, text);
    Ncurses::mvaddnstr(&mut out, y, *x, name, name_width);
    *x += name_width;
    if *x >= l {
        let _ = out.flush();
        return false;
    }

    Ncurses::attrset(&mut out, border);
    Ncurses::mvaddch(&mut out, y, *x, ']');
    let _ = out.flush();
    if *x >= l - (1 + SCREEN_TAB_COLUMN_GAP) {
        *x = l;
        return false;
    }
    *x += 1 + SCREEN_TAB_COLUMN_GAP;
    true
}

/// Port of `static void ScreenManager_drawScreenTabs(ScreenManager* this)`
/// from `ScreenManager.c:194`.
///
/// Draws the row of screen tabs one line above the first panel
/// (`y = panels[0].y - 1`) starting at `SCREEN_TAB_MARGIN_LEFT`. When the
/// manager carries an override `name`, a single current tab is drawn; else it
/// iterates `settings->screens`, marking the tab at `settings->ssIndex` as
/// current and stopping the first time [`drawTab`] reports the line is full.
/// The `end:` label restores `CRT_colors[RESET_COLOR]`.
///
/// `settings` is reached through `this->host->settings` (both always-present
/// pointers in C, so `.unwrap()`ed here, matching the layout ops). The C
/// NULL-terminated `screens[]` walk becomes a `Vec` iteration; a NULL
/// `heading` (never produced for a real screen) maps to the empty string.
pub fn ScreenManager_drawScreenTabs(this: &ScreenManager) {
    let host = this.host.as_ref().unwrap();
    let settings = host.settings.as_ref().unwrap();
    let screens = &settings.screens;
    let cur = settings.ssIndex as i32;
    let l = Ncurses::cols();
    let panel = &this.panels[0];
    let y = panel.y - 1;
    let mut x = SCREEN_TAB_MARGIN_LEFT;

    // C: if (x >= l) goto end;
    if x < l {
        if let Some(name) = &this.name {
            drawTab(y, &mut x, l, name, true);
        } else {
            for (s, screen) in screens.iter().enumerate() {
                let heading = screen.heading.as_deref().unwrap_or("");
                let ok = drawTab(y, &mut x, l, heading, s as i32 == cur);
                if !ok {
                    break;
                }
            }
        }
    }

    // end: attrset(CRT_colors[RESET_COLOR]);
    let scheme = ColorScheme::active();
    let mut out = io::stdout().lock();
    Ncurses::attrset(&mut out, ColorElements::RESET_COLOR.packed(scheme));
    let _ = out.flush();
}

/// TODO: port of `static void ScreenManager_drawPanels(ScreenManager* this,
/// size_t focus, bool force_redraw)` from `ScreenManager.c:222`. Reads
/// `settings->screenTabs`, compares each panel to `state->mainPanel` (the
/// ported `State` has no `mainPanel` pointer), reads `state->hideSelection`,
/// and calls `State_hideFunctionBar` (not ported).
pub fn ScreenManager_drawPanels() {
    todo!("port of ScreenManager.c:222 — needs State.mainPanel + State_hideFunctionBar")
}

/// TODO: port of `void ScreenManager_run(ScreenManager* this,
/// Panel** lastFocus, int* lastKey, const char* name)` from
/// `ScreenManager.c:239`. The main loop: [`checkRecalculation`],
/// [`ScreenManager_drawPanels`], mouse handling (`getmouse`/`MEVENT`), the
/// `Panel` event-handler vtable, and `settings->enableMouse`/`screenTabs`
/// dispatch — all bound to unported substrate and the Panel vtable.
pub fn ScreenManager_run() {
    todo!("port of ScreenManager.c:239 — needs checkRecalculation/drawPanels + Panel vtable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::panel::Panel_new;
    use crate::ported::settings::HeaderLayout;

    fn sm_with_panels(widths: &[i32]) -> ScreenManager {
        let mut sm = ScreenManager::empty();
        let mut x = 0;
        for &w in widths {
            let mut p = Panel_new(x, 0, w, 10, None);
            p.x = x;
            sm.panels.push(p);
            sm.panelCount += 1;
            x += w + 1;
        }
        sm
    }

    /// A `State` with all display toggles off (the only field the ported
    /// layout ops read is `hideMeters`).
    fn state(hideMeters: bool) -> State {
        State {
            pauseUpdate: false,
            hideSelection: false,
            hideMeters,
            host: core::ptr::null_mut(),
        }
    }

    /// A `Header` whose only field the ports read is `height`.
    fn header(height: i32) -> Header {
        Header {
            host: core::ptr::null(),
            columns: Vec::new(),
            headerLayout: HeaderLayout::HF_ONE_100,
            pad: 0,
            height,
            headerMargin: false,
            screenTabs: false,
        }
    }

    #[test]
    fn size_returns_panel_count() {
        let mut sm = ScreenManager::empty();
        assert_eq!(ScreenManager_size(&sm), 0);
        sm.panelCount = 3;
        assert_eq!(ScreenManager_size(&sm), 3);
    }

    #[test]
    fn new_sets_layout_defaults_and_stores_pointers() {
        let sm = ScreenManager_new(Some(header(4)), Machine::default(), state(false));
        assert_eq!((sm.x1, sm.y1, sm.x2, sm.y2), (0, 0, 0, -1));
        assert!(sm.allowFocusChange);
        assert_eq!(sm.panelCount, 0);
        assert!(sm.panels.is_empty());
        assert!(sm.name.is_none());
        assert!(sm.host.is_some());
        assert!(sm.state.is_some());
        assert_eq!(sm.header.as_ref().unwrap().height, 4);
    }

    // ── header_height ─────────────────────────────────────────────────

    #[test]
    fn header_height_zero_when_meters_hidden() {
        let mut sm = ScreenManager::empty();
        sm.state = Some(state(true)); // hideMeters
        sm.header = Some(header(7));
        assert_eq!(header_height(&sm), 0);
    }

    #[test]
    fn header_height_returns_header_height_when_present() {
        let mut sm = ScreenManager::empty();
        sm.state = Some(state(false));
        sm.header = Some(header(7));
        assert_eq!(header_height(&sm), 7);
    }

    #[test]
    fn header_height_zero_when_no_header() {
        let mut sm = ScreenManager::empty();
        sm.state = Some(state(false));
        sm.header = None;
        assert_eq!(header_height(&sm), 0);
    }

    // ── insert / add ──────────────────────────────────────────────────

    #[test]
    fn insert_first_panel_sizes_to_available_height() {
        let mut sm = ScreenManager::empty();
        sm.state = Some(state(false)); // header_height 0 (no header)
        let p = Panel_new(0, 0, 10, 5, None);
        ScreenManager_insert(&mut sm, p, 10, 0);
        assert_eq!(sm.panelCount, 1);
        assert_eq!(sm.panels[0].w, 10); // explicit positive size kept
                                        // height = LINES - y1 - header_height + y2 = LINES - 0 - 0 + (-1)
        assert_eq!(sm.panels[0].h, Ncurses::lines() - 1);
        assert_eq!((sm.panels[0].x, sm.panels[0].y), (0, 0));
        assert!(sm.panels[0].needsRedraw);
    }

    #[test]
    fn insert_negative_size_fills_remaining_width() {
        let mut sm = ScreenManager::empty();
        sm.state = Some(state(false));
        let p = Panel_new(0, 0, 3, 5, None);
        ScreenManager_insert(&mut sm, p, 0, 0); // size <= 0 -> COLS - x1 + x2 - lastX
                                                // lastX 0 (idx 0), so width = COLS.
        assert_eq!(sm.panels[0].w, Ncurses::cols());
    }

    #[test]
    fn add_appends_and_places_right_of_predecessor() {
        let mut sm = ScreenManager::empty();
        sm.state = Some(state(false));
        ScreenManager_add(&mut sm, Panel_new(0, 0, 5, 5, None), 5);
        // second panel: lastX = panels[0].x + panels[0].w + 1 = 0 + 5 + 1
        ScreenManager_add(&mut sm, Panel_new(0, 0, 8, 5, None), 8);
        assert_eq!(sm.panelCount, 2);
        assert_eq!(sm.panels[1].x, 6);
        assert_eq!(sm.panels[1].w, 8);
        assert_eq!(sm.panels[1].y, 0);
    }

    // ── resize ────────────────────────────────────────────────────────

    #[test]
    fn resize_relays_panels_across_the_width() {
        let mut sm = sm_with_panels(&[10, 20]);
        sm.state = Some(state(false)); // header_height 0
        ScreenManager_resize(&mut sm);
        let lines = Ncurses::lines();
        let cols = Ncurses::cols();
        // y1_header = 0; first panel keeps width 10, gets full height.
        assert_eq!(sm.panels[0].w, 10);
        assert_eq!(sm.panels[0].h, lines - 1); // LINES - 0 + (-1)
        assert_eq!((sm.panels[0].x, sm.panels[0].y), (0, 0));
        // lastX after first = 0 + 10 + 1 = 11; last panel takes the rest.
        assert_eq!(sm.panels[1].x, 11);
        assert_eq!(sm.panels[1].w, cols - 11); // COLS - x1 + x2 - lastX
        assert_eq!(sm.panels[1].h, lines - 1);
    }

    #[test]
    fn resize_single_panel_takes_full_width() {
        let mut sm = sm_with_panels(&[10]);
        sm.state = Some(state(false));
        ScreenManager_resize(&mut sm);
        // no non-last panels; lastX stays 0, single panel takes full COLS.
        assert_eq!(sm.panels[0].w, Ncurses::cols());
        assert_eq!(sm.panels[0].x, 0);
    }

    // ── remove ────────────────────────────────────────────────────────

    #[test]
    fn remove_returns_panel_and_updates_count() {
        let mut sm = sm_with_panels(&[10, 20, 5]);
        assert_eq!(sm.panelCount, 3);
        let removed = ScreenManager_remove(&mut sm, 1);
        assert_eq!(removed.w, 20);
        assert_eq!(sm.panelCount, 2);
        assert_eq!(sm.panels.len(), 2);
    }

    #[test]
    fn remove_shifts_right_panels_left_by_width() {
        // panels at x=0(w10), x=11(w20), x=32(w5)
        let mut sm = sm_with_panels(&[10, 20, 5]);
        let x_third_before = sm.panels[2].x; // 32
        ScreenManager_remove(&mut sm, 0); // removes w=10 panel
                                          // remaining panels each shift left by 10
        assert_eq!(sm.panels[0].x, 11 - 10); // old second panel
        assert_eq!(sm.panels[1].x, x_third_before - 10);
    }

    #[test]
    fn remove_last_panel_no_shift() {
        let mut sm = sm_with_panels(&[10, 20]);
        let first_x = sm.panels[0].x;
        ScreenManager_remove(&mut sm, 1);
        assert_eq!(sm.panelCount, 1);
        assert_eq!(sm.panels[0].x, first_x); // unchanged
    }

    // ── drawTab ───────────────────────────────────────────────────────

    #[test]
    fn draw_tab_advances_x_when_it_fits() {
        // wide line: "[main]" + gap fits, x advances by 1 + 4 + (1+1) = 7
        let mut x = SCREEN_TAB_MARGIN_LEFT;
        let ok = drawTab(0, &mut x, 80, "main", true);
        assert!(ok);
        assert_eq!(
            x,
            SCREEN_TAB_MARGIN_LEFT + 1 + 4 + 1 + SCREEN_TAB_COLUMN_GAP
        );
    }

    #[test]
    fn draw_tab_truncates_name_to_line_width() {
        // l small: after '[', remaining width limits nameWidth via strnlen.
        // x starts at 2, l=5 -> after '[' x=3, l-x=2 -> nameWidth=2, x=5 >= l
        let mut x = 2;
        let ok = drawTab(0, &mut x, 5, "abcdef", false);
        assert!(!ok);
        assert_eq!(x, 5); // 3 + 2
    }

    #[test]
    fn draw_tab_returns_false_when_bracket_overflows() {
        // x one before l: '[' pushes x to l -> returns false immediately.
        let mut x = 4;
        let ok = drawTab(0, &mut x, 5, "name", true);
        assert!(!ok);
        assert_eq!(x, 5);
    }
}
