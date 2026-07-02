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
//! the focus-change flag, and the tab `name`. The `Header`/`Machine`/`State`
//! pointers are **omitted**: those types are not yet ported (`header.rs`,
//! `machine.rs`, `settings.rs` are stubs), so any function that reads them
//! cannot be ported faithfully and stays a `todo!()`.
//!
//! # What is ported
//!
//! - [`ScreenManager_size`] — returns `panelCount` (pure).
//! - [`ScreenManager_remove`] — removes a panel and shifts the panels to
//!   its right leftward by its width (pure geometry over the `Vec<Panel>`;
//!   the only ScreenManager layout op that does **not** call
//!   `header_height`).
//! - [`drawTab`] — behavioral crossterm port of the single tab primitive
//!   through the [`Ncurses`] shim; its `*x` advancement / return value are
//!   unit tested by driving it with an in-memory geometry.
//!
//! # What stays a stub (and why)
//!
//! - [`ScreenManager_new`] / [`ScreenManager_delete`] — need
//!   `Header`/`Machine`/`State` (unported) / are `Drop`.
//! - [`header_height`] — reads `state->hideMeters` and `header->height`
//!   (both unported `State`/`Header` substrate). Every layout op below
//!   funnels through it.
//! - [`ScreenManager_add`] / [`ScreenManager_insert`] /
//!   [`ScreenManager_resize`] — all compute panel height via
//!   `header_height`, so they are blocked on the same substrate.
//! - [`checkRecalculation`] — `Machine`/`Platform`/`Header`/`Table`/
//!   `Settings` sampling.
//! - [`ScreenManager_drawScreenTabs`] / [`ScreenManager_drawPanels`] /
//!   [`ScreenManager_run`] — `Settings` (screens, mouse, tabs),
//!   `State`, and the `Panel` event-handler vtable.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::io::{self, Write};

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::functionbar::Ncurses;
use crate::ported::panel::{Panel, Panel_move};

/// Port of `#define SCREEN_TAB_MARGIN_LEFT 2` (`CRT.h:17`).
const SCREEN_TAB_MARGIN_LEFT: i32 = 2;
/// Port of `#define SCREEN_TAB_COLUMN_GAP 1` (`CRT.h:18`).
const SCREEN_TAB_COLUMN_GAP: i32 = 1;

/// Model of htop's `struct ScreenManager_` (`ScreenManager.h:19`); see the
/// module docs for the field mapping and the omitted `Header`/`Machine`/
/// `State` back-pointers.
pub struct ScreenManager {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub allowFocusChange: bool,
    pub panelCount: u32,
    pub panels: Vec<Panel>,
    pub name: Option<String>,
}

impl ScreenManager {
    /// A ScreenManager with the C `ScreenManager_new` layout defaults
    /// (`x1=y1=x2=0`, `y2=-1`, `allowFocusChange=true`, no panels) but
    /// without the `Header`/`Machine`/`State` wiring `ScreenManager_new`
    /// also does. Gate-skipped associated fn (no C 1:1 analog on its own);
    /// used by the tests to exercise the ported layout ops.
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
        }
    }
}

/// TODO: port of `ScreenManager* ScreenManager_new(Header* header,
/// Machine* host, State* state, bool owner)` from `ScreenManager.c:31`.
/// Stores `Header`/`Machine`/`State` back-pointers — all unported.
pub fn ScreenManager_new() {
    todo!("port of ScreenManager.c:31 — needs Header/Machine/State substrate")
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

/// TODO: port of `void ScreenManager_add(ScreenManager* this, Panel* item,
/// int size)` from `ScreenManager.c:56`. Calls `ScreenManager_insert`,
/// which computes panel height via `header_height` (unported State/Header).
pub fn ScreenManager_add() {
    todo!("port of ScreenManager.c:56 — insert path needs header_height (State/Header)")
}

/// TODO: port of `static int header_height(const ScreenManager* this)` from
/// `ScreenManager.c:60`. Reads `state->hideMeters` and `header->height`,
/// both unported (`settings.rs` has no `State`, `header.rs` is stubbed).
pub fn header_height() {
    todo!("port of ScreenManager.c:60 — needs State.hideMeters and Header.height")
}

/// TODO: port of `void ScreenManager_insert(ScreenManager* this,
/// Panel* item, int size, int idx)` from `ScreenManager.c:70`. Panel
/// height is `LINES - y1 - header_height(this) + y2`; `header_height`
/// needs unported State/Header substrate.
pub fn ScreenManager_insert() {
    todo!("port of ScreenManager.c:70 — needs header_height (State/Header)")
}

/// Port of `Panel* ScreenManager_remove(ScreenManager* this, int idx)` from
/// `ScreenManager.c:93`. Removes the panel at `idx` and shifts every panel
/// to its right leftward by the removed panel's width. This is the only
/// ScreenManager layout op that does not call `header_height`, so it ports
/// faithfully against the `Vec<Panel>`.
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

/// TODO: port of `void ScreenManager_resize(ScreenManager* this)` from
/// `ScreenManager.c:107`. Every panel's height is `LINES - y1_header + y2`
/// where `y1_header = y1 + header_height(this)`; `header_height` needs
/// unported State/Header substrate.
pub fn ScreenManager_resize() {
    todo!("port of ScreenManager.c:107 — needs header_height (State/Header)")
}

/// TODO: port of `static void checkRecalculation(...)` from
/// `ScreenManager.c:122`. Samples `Machine`/`Platform` metrics, rebuilds
/// the active `Table`, and redraws the `Header` — all unported substrate.
pub fn checkRecalculation() {
    todo!("port of ScreenManager.c:122 — needs Machine/Platform/Header/Table/Settings")
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
    *x += 1 + SCREEN_TAB_COLUMN_GAP;
    let _ = out.flush();
    if *x >= l {
        return false;
    }
    true
}

/// TODO: port of `static void ScreenManager_drawScreenTabs(ScreenManager* this)`
/// from `ScreenManager.c:194`. Iterates `settings->screens` at
/// `settings->ssIndex` — unported `Settings` substrate.
pub fn ScreenManager_drawScreenTabs() {
    todo!("port of ScreenManager.c:194 — needs Settings.screens/ssIndex")
}

/// TODO: port of `static void ScreenManager_drawPanels(ScreenManager* this,
/// size_t focus, bool force_redraw)` from `ScreenManager.c:222`. Reads
/// `settings->screenTabs`, `state->mainPanel`, `state->hideSelection`, and
/// `State_hideFunctionBar` — unported Settings/State substrate.
pub fn ScreenManager_drawPanels() {
    todo!("port of ScreenManager.c:222 — needs Settings/State substrate")
}

/// TODO: port of `void ScreenManager_run(ScreenManager* this,
/// Panel** lastFocus, int* lastKey, const char* name)` from
/// `ScreenManager.c:239`. The main loop: `checkRecalculation`, mouse
/// handling, the `Panel` event-handler vtable, and focus/resize/quit
/// dispatch — all bound to unported Settings/Machine/Header substrate and
/// the Panel vtable.
pub fn ScreenManager_run() {
    todo!("port of ScreenManager.c:239 — needs Settings/Machine/Header + Panel vtable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::panel::Panel_new;

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

    #[test]
    fn size_returns_panel_count() {
        let mut sm = ScreenManager::empty();
        assert_eq!(ScreenManager_size(&sm), 0);
        sm.panelCount = 3;
        assert_eq!(ScreenManager_size(&sm), 3);
    }

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

    #[test]
    fn draw_tab_advances_x_when_it_fits() {
        // wide line: "[main]" + gap fits, x advances by 1 + 4 + (1+1) = 7
        let mut x = SCREEN_TAB_MARGIN_LEFT;
        let ok = drawTab(0, &mut x, 80, "main", true);
        assert!(ok);
        assert_eq!(x, SCREEN_TAB_MARGIN_LEFT + 1 + 4 + 1 + SCREEN_TAB_COLUMN_GAP);
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
