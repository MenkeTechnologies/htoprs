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
//! models the layout rectangle, the `panels` as a `Vec<Box<dyn PanelClass>>`,
//! the count, the focus-change flag, the tab `name`, and the three
//! back-pointers as raw pointers (`header: *mut Header`, `host: *mut Machine`,
//! `state: *mut State`) — matching C's `Header* Machine* State*`. The three
//! objects are owned by the caller (main.rs / htop.c), which builds the run's
//! shared object graph (the `DarwinMachine` host, the process `MainPanel`, the
//! `State` whose `host`/`mainPanel`/`header` point back into the same objects)
//! and outlives the ScreenManager for the run. `header` is `NULL`-able in C
//! (`if (this->header)`), so the null-guards become `!this.header.is_null()`;
//! `state` is dereferenced unconditionally in C, so the layout ops deref it
//! directly (`unsafe { &*this.state }`). All three are stored by
//! [`ScreenManager_new`].
//!
//! The C `Vector* panels` holds `Panel*` element pointers that are really
//! subclass objects (`MainPanel`, `CategoriesPanel`, …) dispatched through
//! the `PanelClass` vtable. The faithful Rust analog is
//! `Vec<Box<dyn PanelClass>>`: each element is a boxed trait object whose
//! concrete type is the subclass, and every layout op reaches the embedded
//! base [`Panel`] through [`PanelClass::as_panel`] /
//! [`PanelClass::as_panel_mut`] (the C `(Panel*)this` upcast). This is what
//! lets the main loop route keys to the focused panel's subclass handler
//! (`panels[focus].event_handler(ch)`) — the C `Panel_eventHandler` vtable
//! dispatch.
//!
//! # What is ported
//!
//! - [`ScreenManager_new`] / [`ScreenManager_size`] / [`header_height`] /
//!   [`ScreenManager_insert`] / [`ScreenManager_add`] /
//!   [`ScreenManager_remove`] / [`ScreenManager_resize`] — the layout engine,
//!   each panel reached through `.as_panel()` / `.as_panel_mut()`.
//! - [`drawTab`] / [`ScreenManager_drawScreenTabs`] — the screen-tab row.
//! - [`ScreenManager_drawPanels`] — dispatches `Panel_draw` per panel with the
//!   `panel != state->mainPanel` identity test done by raw-pointer identity of
//!   the boxed panel's `as_panel()` against `(Panel*)state->mainPanel`, the
//!   `State_hideFunctionBar` predicate inlined (`Action.h:45`), and the
//!   inter-panel `mvvline` separator.
//! - [`ScreenManager_run`] — the main loop: focus tracking, per-panel key
//!   dispatch through the `PanelClass` trait, the `HANDLED`/`BREAK_LOOP`/
//!   `REFRESH`/`REDRAW`/`RESIZE`/`RESCAN`/`SYNTH_KEY` result handling, the
//!   `EVENT_PANEL_LOST_FOCUS` on focus change, and the navigation switch.
//!
//! # What stays a gap (and why)
//!
//! - [`ScreenManager_delete`] — `Vector_delete` + `free`; released by `Drop`.
//! - [`checkRecalculation`] — its time-sampling + machine-rescan core is
//!   gapped (see the function docs): `Platform_gettime_realtime` has no
//!   cross-target facade and `Machine` models no `realtime` field, so
//!   `newTime` cannot be sampled; `Settings` has no `ss` field, `Machine_scan`
//!   is platform-generic, and `Platform_getFailedState` is absent. The
//!   reachable `if (*redraw)` tail (`Table_rebuildPanel` + `Header_draw`) is
//!   ported.
//! - [`ScreenManager_run`]'s `HAVE_GETMOUSE` mouse-decode block — `getmouse`/
//!   `MEVENT` are not in `src/ported` (`crt::CRT_readKey` collapses a mouse
//!   event to `KEY_MOUSE` with no coordinates), so the loop is ported as the
//!   faithful `#ifndef HAVE_GETMOUSE` configuration (a `KEY_MOUSE` falls
//!   through to the default handler).
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::io::{self, Write};

use crate::ported::action::State;
use crate::ported::crt::{
    ColorElements, ColorScheme, ERR, KEY_ALT, KEY_CTRL, KEY_DOWN, KEY_F, KEY_FOCUS_IN,
    KEY_FOCUS_OUT, KEY_LEFT, KEY_RESIZE, KEY_RIGHT, KEY_UP,
};
use crate::ported::functionbar::Ncurses;
use crate::ported::header::{Header, Header_draw, Header_updateData};
use crate::ported::machine::{Machine, Machine_scanTables};
use crate::ported::panel::{
    HandlerResult, Panel, PanelClass, Panel_draw, Panel_getCh, Panel_move, Panel_onKey,
    Panel_resize, Panel_size, EVENT_PANEL_LOST_FOCUS,
};
use crate::ported::table::Table_rebuildPanel;

/// Port of `#define SCREEN_TAB_MARGIN_LEFT 2` (`CRT.h:17`).
const SCREEN_TAB_MARGIN_LEFT: i32 = 2;
/// Port of `#define SCREEN_TAB_COLUMN_GAP 1` (`CRT.h:18`).
const SCREEN_TAB_COLUMN_GAP: i32 = 1;

// Ctrl/Alt/Fn key codes matched by `ScreenManager_run`'s dispatch switch.
// `KEY_CTRL`/`KEY_ALT`/`KEY_F` are `const fn` in `crt.rs`; binding their
// results as `const`s makes them usable as match patterns (a const-fn call is
// not itself a pattern), the same idiom `panel.rs` uses. `HASH`/`ESC`/`KEY_Q`
// bind the raw `case '#':`/`case 27:`/`case 'q':` codes.
const ALT_H: i32 = KEY_ALT(b'H' as i32);
const ALT_J: i32 = KEY_ALT(b'J' as i32);
const ALT_K: i32 = KEY_ALT(b'K' as i32);
const ALT_L: i32 = KEY_ALT(b'L' as i32);
const CTRL_B: i32 = KEY_CTRL(b'B' as i32);
const CTRL_F: i32 = KEY_CTRL(b'F' as i32);
const F10: i32 = KEY_F(10);
const HASH: i32 = b'#' as i32;
const ESC: i32 = 27;
const KEY_Q: i32 = b'q' as i32;

/// Model of htop's `struct ScreenManager_` (`ScreenManager.h:19`). The C
/// `Header*`/`Machine*`/`State*` back-pointers stay raw pointers (`*mut Header`,
/// `*mut Machine`, `*mut State`), aliasing objects the caller owns for the
/// run (see the module docs); `panels` is the `Vec` analog of the C
/// `Vector* panels`, each element a `Box<dyn PanelClass>` — the boxed subclass
/// panel the C `Vector` stored as a `Panel*`.
pub struct ScreenManager {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub allowFocusChange: bool,
    pub panelCount: u32,
    pub panels: Vec<Box<dyn PanelClass>>,
    pub name: Option<String>,
    pub header: *mut Header,
    pub host: *mut Machine,
    pub state: *mut State,
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
            header: core::ptr::null_mut(),
            host: core::ptr::null_mut(),
            state: core::ptr::null_mut(),
        }
    }
}

/// Port of `ScreenManager* ScreenManager_new(Header* header, Machine* host,
/// State* state, bool owner)` from `ScreenManager.c:31`.
///
/// Sets the layout defaults (`x1=y1=x2=0`, `y2=-1`, `allowFocusChange`) and
/// stores the three back-pointers verbatim. The `owner` arg only typed the C
/// `Vector` (whether it frees its items); a `Vec<Box<dyn PanelClass>>` always
/// owns, so it is dropped, exactly as [`Panel_new`](crate::ported::panel::Panel_new)
/// drops its `type`/`owner`. `header`/`host`/`state` are `Header*`/`Machine*`/
/// `State*` aliasing objects the caller owns for the run (`header` may be NULL).
pub fn ScreenManager_new(
    header: *mut Header,
    host: *mut Machine,
    state: *mut State,
) -> ScreenManager {
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
        host,
        state,
    }
}

/// Port of `void ScreenManager_delete(ScreenManager* this)` from
/// `ScreenManager.c:47`: `Vector_delete(this->panels); free(this);`. Taking
/// `this` by value consumes the manager; the `panels`
/// `Vec<Box<dyn PanelClass>>` owns its boxed panels (the C owner-`Vector_delete`),
/// so dropping it runs each panel's teardown, and the owned `name` drops with
/// the struct free. `header`/`host`/`state` are borrowed raw pointers aliasing
/// caller-owned objects (the C `Header*`/`Machine*`/`State*` the caller frees),
/// so they are not touched here — matching C `ScreenManager_delete`, which
/// frees only `this->panels` and `this`.
pub fn ScreenManager_delete(this: ScreenManager) {
    let _ = this;
}

/// Port of `inline int ScreenManager_size(const ScreenManager* this)` from
/// `ScreenManager.c:52`: returns `panelCount`.
pub fn ScreenManager_size(this: &ScreenManager) -> i32 {
    this.panelCount as i32
}

/// Port of `void ScreenManager_add(ScreenManager* this, Panel* item,
/// int size)` from `ScreenManager.c:56`. Inserts `item` at the end
/// (`Vector_size(this->panels)` == `panelCount`). `item` is a
/// `Box<dyn PanelClass>` — the C `Panel*` element (a boxed subclass panel).
pub fn ScreenManager_add(this: &mut ScreenManager, item: Box<dyn PanelClass>, size: i32) {
    let idx = this.panels.len() as i32;
    ScreenManager_insert(this, item, size, idx);
}

/// Port of `static int header_height(const ScreenManager* this)` from
/// `ScreenManager.c:60`. Returns `0` when `state->hideMeters` is set, else
/// `header->height` when a header is present, else `0`. C dereferences
/// `state` unconditionally, so `state` is dereferenced directly here; `header`
/// is `NULL`-checked (`if (this->header)`) via `!this.header.is_null()`.
pub fn header_height(this: &ScreenManager) -> i32 {
    // SAFETY: `state`/`header` alias objects the caller owns for the run
    // (see the module docs); `state` is non-null whenever the layout ops run
    // (C dereferences it unconditionally), and `header` is null-guarded below.
    if unsafe { &*this.state }.hideMeters {
        return 0;
    }

    if !this.header.is_null() {
        return unsafe { &*this.header }.height;
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
/// The C `Panel*` element is the boxed panel `item`; every `Panel` field
/// access goes through `.as_panel()` / `.as_panel_mut()`.
pub fn ScreenManager_insert(
    this: &mut ScreenManager,
    mut item: Box<dyn PanelClass>,
    mut size: i32,
    idx: i32,
) {
    let mut lastX = 0;
    if idx > 0 {
        let last = this.panels[(idx - 1) as usize].as_panel();
        lastX = last.x + last.w + 1;
    }
    let hh = header_height(this);
    let height = Ncurses::lines() - this.y1 - hh + this.y2;
    if size <= 0 {
        size = Ncurses::cols() - this.x1 + this.x2 - lastX;
    }
    Panel_resize(item.as_panel_mut(), size, height);
    Panel_move(item.as_panel_mut(), lastX, this.y1 + hh);
    if (idx as u32) < this.panelCount {
        // Faithful to the C loop `for (i = idx + 1; i <= panelCount; i++)`:
        // it shifts the existing panels right of `idx`. The `<= panelCount`
        // bound reads one past the last panel (C `Vector_get` would assert);
        // this path is unreachable in htop because `ScreenManager_add` always
        // inserts at `idx == panelCount`, so the guard above is false.
        for i in (idx + 1)..=(this.panelCount as i32) {
            let (px, py) = {
                let p = this.panels[i as usize].as_panel();
                (p.x, p.y)
            };
            Panel_move(this.panels[i as usize].as_panel_mut(), px + size, py);
        }
    }
    item.as_panel_mut().needsRedraw = true;
    this.panels.insert(idx as usize, item);
    this.panelCount += 1;
}

/// Port of `Panel* ScreenManager_remove(ScreenManager* this, int idx)` from
/// `ScreenManager.c:93`. Removes the panel at `idx` and shifts every panel
/// to its right leftward by the removed panel's width. This is the only
/// ScreenManager layout op that does not call `header_height`. Returns the
/// boxed panel (the C `Panel*` the caller reclaims ownership of).
pub fn ScreenManager_remove(this: &mut ScreenManager, idx: i32) -> Box<dyn PanelClass> {
    debug_assert!((idx as u32) < this.panelCount);
    let w = this.panels[idx as usize].as_panel().w;
    let panel = this.panels.remove(idx as usize);
    this.panelCount -= 1;
    if (idx as u32) < this.panelCount {
        for i in (idx as usize)..(this.panelCount as usize) {
            let (px, py) = {
                let p = this.panels[i].as_panel();
                (p.x, p.y)
            };
            Panel_move(this.panels[i].as_panel_mut(), px - w, py);
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
/// vector would assert). Each `Panel*` is reached through `.as_panel()` /
/// `.as_panel_mut()`.
pub fn ScreenManager_resize(this: &mut ScreenManager) {
    let y1_header = this.y1 + header_height(this);
    let panels = this.panelCount as i32;
    let lines = Ncurses::lines();
    let cols = Ncurses::cols();
    let mut lastX = 0;
    for i in 0..(panels - 1) {
        let w = this.panels[i as usize].as_panel().w;
        Panel_resize(
            this.panels[i as usize].as_panel_mut(),
            w,
            lines - y1_header + this.y2,
        );
        Panel_move(this.panels[i as usize].as_panel_mut(), lastX, y1_header);
        let p = this.panels[i as usize].as_panel();
        lastX = p.x + p.w + 1;
    }
    let last = (panels - 1) as usize;
    Panel_resize(
        this.panels[last].as_panel_mut(),
        cols - this.x1 + this.x2 - lastX,
        lines - y1_header + this.y2,
    );
    Panel_move(this.panels[last].as_panel_mut(), lastX, y1_header);
}

/// Port of `static void checkRecalculation(ScreenManager* this,
/// double* oldTime, int* sortTimeout, bool* redraw, bool* rescan,
/// bool* timedOut, bool* force_redraw)` from `ScreenManager.c:122`.
///
/// # Gapped: the time-sampling + machine-rescan core (`ScreenManager.c:125-160`)
///
/// `Platform_gettime_realtime(&host->realtime, &host->realtimeMs)` cannot be
/// ported: `Machine` models no `realtime` (`struct timeval`) field, and
/// `src/ported` exposes no cross-target `Platform_gettime_realtime` facade
/// (only per-OS impls under `darwin/`, `linux/`, …). Without a sampled
/// `newTime`, the `*timedOut`/`*rescan`/`*oldTime` clock logic and the entire
/// `if (*rescan)` block are unreachable. That block additionally needs:
/// `host->settings->ss->treeView` (`Settings` models no `ss`/active
/// `ScreenSettings*` field — only `ssIndex`/`screens`); `Machine_scan(host)`
/// (the ported per-OS `Machine_scan` takes a concrete `LinuxMachine`/
/// `DarwinMachine`, not the generic `Machine` this manager owns); and
/// `Platform_getFailedState()` (absent). `Machine_scanTables`,
/// `Header_updateData`, and the `Row_pidDigits`/`Row_uidDigits` globals do
/// exist, but sit behind the un-samplable `*rescan`, so they stay unreached.
///
/// The reachable `if (*redraw)` tail (`Table_rebuildPanel` + `Header_draw`)
/// and the trailing `*rescan = false` are ported.
pub fn checkRecalculation(
    this: &mut ScreenManager,
    oldTime: &mut f64,
    sortTimeout: &mut i32,
    redraw: &mut bool,
    rescan: &mut bool,
    timedOut: &mut bool,
    force_redraw: &mut bool,
) {
    let _ = &force_redraw; // C bumps this on a UID/PID-digit change (unmodeled)

    // Platform_gettime_realtime(&host->realtime, &host->realtimeMs): resample
    // the wall clock into host->realtimeMs so the delay gate advances.
    #[cfg(target_os = "macos")]
    {
        let mut tv = libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        // SAFETY: host aliases the caller-owned Machine for the run.
        let ms = unsafe { &mut (*this.host).realtimeMs };
        crate::ported::darwin::platform::Platform_gettime_realtime(&mut tv, ms);
    }

    // newTime = tv_sec*10 + tv_usec/100000 == realtimeMs / 100 (tenths of a
    // second); host->settings->delay is also in tenths.
    // SAFETY: host aliases the caller-owned Machine.
    let newTime = unsafe { (*this.host).realtimeMs } as f64 / 100.0;
    let delay = unsafe {
        (*this.host)
            .settings
            .as_ref()
            .map(|s| s.delay)
            .unwrap_or(15)
    } as f64;

    *timedOut = (newTime - *oldTime) > delay;
    *rescan |= *timedOut;
    if newTime < *oldTime {
        *rescan = true; // clock was adjusted?
    }

    if *rescan {
        *oldTime = newTime;

        // SAFETY: state aliases the caller-owned State.
        let pauseUpdate = unsafe { (*this.state).pauseUpdate };
        let treeView = unsafe {
            (*this.host)
                .settings
                .as_ref()
                .and_then(|s| s.screens.get(s.ssIndex as usize))
                .map(|ss| ss.treeView)
                .unwrap_or(false)
        };
        if !pauseUpdate && (*sortTimeout == 0 || treeView) {
            // host->activeTable->needsSort = true; *sortTimeout = 1;
            if let Some(table) = unsafe { (*this.host).activeTable } {
                // SAFETY: activeTable is the caller-owned Table for the run.
                unsafe {
                    (*table).needsSort = true;
                }
            }
            *sortTimeout = 1;
        }

        // Machine_scan(host): resample the system-wide metrics (CPU/mem/swap/
        // load/…). The generic loop dispatches to the per-OS scanner through
        // the offset-0 `Machine` super downcast — the established darwin idiom
        // (darwinprocess.rs `host as *const DarwinMachine`); both XMachine
        // structs are `#[repr(C)]` with `super_: Machine` first.
        #[cfg(target_os = "macos")]
        {
            let dhost = this.host as *mut crate::ported::darwin::darwinmachine::DarwinMachine;
            // SAFETY: base Machine* round-trips to *mut DarwinMachine (offset 0).
            crate::ported::darwin::darwinmachine::Machine_scan(unsafe { &mut *dhost });
        }
        #[cfg(all(not(target_os = "macos"), target_os = "linux"))]
        {
            let lhost = this.host as *mut crate::ported::linux::linuxmachine::LinuxMachine;
            // SAFETY: base Machine* round-trips to *mut LinuxMachine (offset 0).
            crate::ported::linux::linuxmachine::Machine_scan(unsafe { &mut *lhost });
        }

        // if (!pauseUpdate) Machine_scanTables(host): refresh the process table.
        if !pauseUpdate {
            // SAFETY: host aliases the caller-owned Machine; the Machine_scan
            // &mut above has already ended.
            Machine_scanTables(unsafe { &mut *this.host });
        }

        // this->state->failedUpdate = Platform_getFailedState(): the failed-
        // state reader is unported; leave failedUpdate untouched.

        // "always update header, especially to avoid gaps in graph meters"
        // (ScreenManager.c:152-153). Runs each meter's updateValues slot so
        // curItems/curAttributes reflect the freshly sampled data before draw.
        if !this.header.is_null() {
            // SAFETY: header non-null, aliases the caller-owned Header.
            Header_updateData(unsafe { &mut *this.header });
        }

        *redraw = true;
    }

    if *redraw {
        // Table_rebuildPanel(host->activeTable)
        // SAFETY: host aliases the caller-owned Machine for the run.
        if let Some(table) = unsafe { (*this.host).activeTable } {
            unsafe {
                Table_rebuildPanel(&mut *table);
            }
        }
        // if (!this->state->hideMeters) Header_draw(this->header)
        // SAFETY: state aliases the caller-owned State.
        if !unsafe { (*this.state).hideMeters } && !this.header.is_null() {
            let header = unsafe { &mut *this.header };
            let mut out = io::stdout().lock();
            Header_draw(header, &mut out);
            let _ = out.flush();
        }
    }

    *rescan = false;
}

/// Port of `static inline bool drawTab(const int* y, int* x, int l,
/// const char* name, bool cur)` from `ScreenManager.c:171`.
///
/// Behavioral crossterm port through the `Ncurses` shim: draws
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
    // SAFETY: `host` aliases the caller-owned `Machine` for the run (see the
    // module docs); C dereferences `this->host->settings` unconditionally.
    let host = unsafe { &*this.host };
    let settings = host.settings.as_ref().unwrap();
    let screens = &settings.screens;
    let cur = settings.ssIndex as i32;
    let l = Ncurses::cols();
    let panel = this.panels[0].as_panel();
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

/// Port of `static void ScreenManager_drawPanels(ScreenManager* this,
/// size_t focus, bool force_redraw)` from `ScreenManager.c:222`.
///
/// Optionally draws the screen-tab row (`settings->screenTabs`), then for each
/// panel calls [`Panel_draw`] with `i == focus`, the highlight predicate
/// `panel != (Panel*)this->state->mainPanel || !this->state->hideSelection`,
/// and the `State_hideFunctionBar` flag, and paints the inter-panel `mvvline`
/// separator.
///
/// - `State_hideFunctionBar` (`Action.h:45`, a `static inline` not present in
///   `src/ported`) is reproduced inline: `settings->hideFunctionBar == 2 ||
///   (settings->hideFunctionBar == 1 && st->hideSelection)`.
/// - The `panel != (Panel*)this->state->mainPanel` identity test is done by
///   raw-pointer identity: the boxed panel's `as_panel() as *const Panel`
///   against the base `Panel` of `state->mainPanel` (`(*mainPanel).as_panel()`,
///   the C `(Panel*)mainPanel` upcast), a NULL `mainPanel` mapping to a null
///   pointer.
pub fn ScreenManager_drawPanels(this: &mut ScreenManager, focus: usize, force_redraw: bool) {
    // Settings* settings = this->host->settings;
    let (screen_tabs, hide_function_bar) = {
        // SAFETY: `host`/`state` alias caller-owned objects for the run (see
        // the module docs); C dereferences both unconditionally here.
        let settings = unsafe { &*this.host }.settings.as_ref().unwrap();
        // Port of `State_hideFunctionBar` (Action.h:45), inlined at the call
        // site (the `static inline` is not a ported symbol).
        let hfb = settings.hideFunctionBar;
        let hide_selection = unsafe { &*this.state }.hideSelection;
        (
            settings.screenTabs,
            hfb == 2 || (hfb == 1 && hide_selection),
        )
    };

    if screen_tabs {
        ScreenManager_drawScreenTabs(this);
    }

    // SAFETY: `state` aliases the caller-owned `State` for the run.
    let hide_selection = unsafe { &*this.state }.hideSelection;
    // (Panel*)this->state->mainPanel — the base panel of the MainPanel, or a
    // null pointer when `mainPanel` is NULL.
    let main_panel_base: *const Panel = {
        // SAFETY: `state` aliases the caller-owned `State` for the run.
        let mp = unsafe { &*this.state }.mainPanel;
        if mp.is_null() {
            core::ptr::null()
        } else {
            // SAFETY: `mainPanel` is the process panel owned by the main loop
            // for the manager's lifetime (as in C, where it is dereferenced as
            // `(Panel*)this->state->mainPanel`).
            unsafe { (*mp).as_panel() as *const Panel }
        }
    };

    let n_panels = this.panelCount as usize;
    let mut out = io::stdout().lock();
    for i in 0..n_panels {
        let highlight_selected = {
            let panel_base = this.panels[i].as_panel() as *const Panel;
            panel_base != main_panel_base || !hide_selection
        };
        Panel_draw(
            this.panels[i].as_panel_mut(),
            force_redraw,
            i == focus,
            highlight_selected,
            hide_function_bar,
        );
        let (py, px, pw, ph) = {
            let p = this.panels[i].as_panel();
            (p.y, p.x, p.w, p.h)
        };
        Ncurses::mvvline(
            &mut out,
            py,
            px + pw,
            ' ',
            ph + if hide_function_bar { 1 } else { 0 },
        );
    }
    let _ = out.flush();
}

/// Port of `void ScreenManager_run(ScreenManager* this, Panel** lastFocus,
/// int* lastKey, const char* name)` from `ScreenManager.c:239` — the main loop.
///
/// Tracks the focused panel by index (`panelFocus` in C is always
/// `panels[focus]`, so the index alone suffices), draws the panels, reads a
/// key via [`Panel_getCh`] (C `getch`), routes it to the focused panel's
/// subclass handler through the `PanelClass` trait
/// (`panels[focus].event_handler(ch)` — the C `Panel_eventHandler` vtable
/// dispatch; the base handler returns `IGNORED`, the C NULL-slot no-op),
/// applies the `HANDLED`/`BREAK_LOOP`/`REFRESH`/`REDRAW`/`RESIZE`/`RESCAN`/
/// `SYNTH_KEY` result flags, then runs the navigation switch (focus movement
/// firing `EVENT_PANEL_LOST_FOCUS`, `#` meter toggle, quit keys, default
/// `Panel_onKey`).
///
/// `lastFocus`/`lastKey` are the C out-params written on exit (the focused
/// panel index and the last key). `name` sets `this->name`. The
/// `HAVE_GETMOUSE` mouse-decode block is gapped (see the module docs): with no
/// ported `getmouse`/`MEVENT`, this is the faithful `#ifndef HAVE_GETMOUSE`
/// build, where a `KEY_MOUSE` falls through to the default handler.
pub fn ScreenManager_run(
    this: &mut ScreenManager,
    lastFocus: Option<&mut usize>,
    lastKey: Option<&mut i32>,
    name: Option<&str>,
) {
    let mut quit = false;
    let mut focus: usize = 0;

    let mut oldTime = 0.0f64;

    let mut ch = ERR;
    let mut closeTimeout = 0;

    let mut timedOut = true;
    let mut redraw = true;
    let mut force_redraw = true;
    let mut rescan = false;
    let mut sortTimeout = 0;
    let resetSortTimeout = 5;

    this.name = name.map(|s| s.to_string());

    'main: while !quit {
        if !this.header.is_null() {
            checkRecalculation(
                this,
                &mut oldTime,
                &mut sortTimeout,
                &mut redraw,
                &mut rescan,
                &mut timedOut,
                &mut force_redraw,
            );
        }

        if redraw || force_redraw {
            ScreenManager_drawPanels(this, focus, force_redraw);
            // htoprs extension: paint the themed help/chooser/editor overlay
            // over the freshly-drawn panels (no-op when no overlay is open).
            {
                let mut out = io::stdout().lock();
                crate::extensions::overlay::draw_active(&mut out);
            }
            force_redraw = false;
            // SAFETY: `host` aliases the caller-owned `Machine` for the run
            // (see the module docs); C dereferences it unconditionally here.
            if unsafe { &*this.host }.iterationsRemaining != -1 {
                let host = unsafe { &mut *this.host };
                host.iterationsRemaining -= 1;
                if host.iterationsRemaining == 0 {
                    quit = true;
                    continue;
                }
            }
        }

        let prevCh = ch;
        ch = Panel_getCh(this.panels[focus].as_panel());

        // HAVE_GETMOUSE mouse-decode block (ScreenManager.c:280-336) is gapped:
        // `getmouse`/`MEVENT` are not in `src/ported` (`crt::CRT_readKey`
        // collapses a mouse event to `KEY_MOUSE` with no coordinates). This is
        // the faithful `#ifndef HAVE_GETMOUSE` build — a `KEY_MOUSE` falls
        // through the switch to the default `Panel_onKey`.

        if ch == ERR {
            if sortTimeout > 0 {
                sortTimeout -= 1;
            }
            if prevCh == ch && !timedOut {
                closeTimeout += 1;
                if closeTimeout == 100 {
                    break;
                }
            } else {
                closeTimeout = 0;
            }
            redraw = false;
            continue;
        }

        // htoprs extension: give the theme/help overlay first refusal on the
        // key. It consumes its hotkeys (h/? c C x g) and, while open, every
        // key — repainting panels + overlay in the (possibly new) theme.
        if crate::extensions::overlay::dispatch_key(ch) {
            redraw = true;
            force_redraw = true;
            continue;
        }

        match ch {
            ALT_H => ch = KEY_LEFT,
            ALT_J => ch = KEY_DOWN,
            ALT_K => ch = KEY_UP,
            ALT_L => ch = KEY_RIGHT,
            _ => {}
        }

        redraw = true;
        // C: if (Panel_eventHandlerFn(panelFocus)) result = Panel_eventHandler(...).
        // Every `Box<dyn PanelClass>` has an `event_handler` (the base returns
        // IGNORED, the C NULL-slot no-op), so the guard is always taken.
        let result = this.panels[focus].event_handler(ch);
        if result.contains(HandlerResult::SYNTH_KEY) {
            ch = (result.0 >> 16) as i32;
        }
        if result.contains(HandlerResult::REFRESH) {
            sortTimeout = 0;
        }
        if result.contains(HandlerResult::REDRAW) {
            force_redraw = true;
        }
        if result.contains(HandlerResult::RESIZE) {
            ScreenManager_resize(this);
            force_redraw = true;
        }
        if result.contains(HandlerResult::RESCAN) {
            rescan = true;
            sortTimeout = 0;
        }
        if result.contains(HandlerResult::HANDLED) {
            continue;
        } else if result.contains(HandlerResult::BREAK_LOOP) {
            quit = true;
            continue;
        }

        // The C `switch (ch)` with its `goto defaultHandler` fall-throughs.
        // A match arm that ends without `break 'sw`/`continue` falls out of
        // the labeled block into the `defaultHandler:` tail (the C
        // `goto defaultHandler` / `default:`); `break 'sw` is the C `break;`.
        'sw: {
            match ch {
                KEY_RESIZE => {
                    ScreenManager_resize(this);
                    continue 'main;
                }
                KEY_FOCUS_IN | KEY_FOCUS_OUT => break 'sw,
                KEY_LEFT | CTRL_B => {
                    if this.panelCount >= 2 {
                        if !this.allowFocusChange {
                            break 'sw;
                        }
                        if focus > 0 {
                            this.panels[focus].event_handler(EVENT_PANEL_LOST_FOCUS);
                        }
                        // tryLeft:
                        loop {
                            if focus > 0 {
                                focus -= 1;
                            }
                            if Panel_size(this.panels[focus].as_panel()) == 0 && focus > 0 {
                                continue;
                            }
                            break;
                        }
                        break 'sw;
                    }
                    // panelCount < 2 -> goto defaultHandler (fall out of match)
                }
                KEY_RIGHT | CTRL_F | 9 => {
                    if this.panelCount >= 2 {
                        if !this.allowFocusChange {
                            break 'sw;
                        }
                        if (focus as u32) < this.panelCount - 1 {
                            this.panels[focus].event_handler(EVENT_PANEL_LOST_FOCUS);
                        }
                        // tryRight:
                        loop {
                            if (focus as u32) < this.panelCount - 1 {
                                focus += 1;
                            }
                            if Panel_size(this.panels[focus].as_panel()) == 0
                                && (focus as u32) < this.panelCount - 1
                            {
                                continue;
                            }
                            break;
                        }
                        break 'sw;
                    }
                    // panelCount < 2 -> goto defaultHandler (fall out of match)
                }
                HASH => {
                    {
                        // SAFETY: `state` aliases the caller-owned `State` for
                        // the run (see the module docs).
                        let st = unsafe { &mut *this.state };
                        st.hideMeters = !st.hideMeters;
                    }
                    ScreenManager_resize(this);
                    force_redraw = true;
                    break 'sw;
                }
                ESC | KEY_Q | F10 => {
                    quit = true;
                    continue 'main;
                }
                _ => {
                    // default: -> defaultHandler (fall out of match)
                }
            }
            // defaultHandler:
            sortTimeout = resetSortTimeout;
            Panel_onKey(this.panels[focus].as_panel_mut(), ch);
        }
    }

    if let Some(lf) = lastFocus {
        *lf = focus;
    }

    if let Some(lk) = lastKey {
        *lk = ch;
    }
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
            sm.panels.push(Box::new(p));
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
            mainPanel: core::ptr::null_mut(),
            header: core::ptr::null_mut(),
            failedUpdate: None,
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
        // The three pointed-to objects are owned in the test scope and outlive
        // `sm`, exactly as the caller (main.rs) owns them for the run.
        let mut hdr = header(4);
        let mut host = Machine::default();
        let mut st = state(false);
        let sm = ScreenManager_new(&mut hdr, &mut host, &mut st);
        assert_eq!((sm.x1, sm.y1, sm.x2, sm.y2), (0, 0, 0, -1));
        assert!(sm.allowFocusChange);
        assert_eq!(sm.panelCount, 0);
        assert!(sm.panels.is_empty());
        assert!(sm.name.is_none());
        assert!(!sm.host.is_null());
        assert!(!sm.state.is_null());
        // SAFETY: `sm.header` aliases `hdr`, alive for the test body.
        assert_eq!(unsafe { &*sm.header }.height, 4);
    }

    // ── header_height ─────────────────────────────────────────────────

    #[test]
    fn header_height_zero_when_meters_hidden() {
        let mut st = state(true); // hideMeters
        let mut hdr = header(7);
        let mut sm = ScreenManager::empty();
        sm.state = &mut st;
        sm.header = &mut hdr;
        assert_eq!(header_height(&sm), 0);
    }

    #[test]
    fn header_height_returns_header_height_when_present() {
        let mut st = state(false);
        let mut hdr = header(7);
        let mut sm = ScreenManager::empty();
        sm.state = &mut st;
        sm.header = &mut hdr;
        assert_eq!(header_height(&sm), 7);
    }

    #[test]
    fn header_height_zero_when_no_header() {
        let mut st = state(false);
        let mut sm = ScreenManager::empty();
        sm.state = &mut st;
        // header stays null (C NULL) — header_height returns 0.
        assert_eq!(header_height(&sm), 0);
    }

    // ── insert / add ──────────────────────────────────────────────────

    #[test]
    fn insert_first_panel_sizes_to_available_height() {
        let mut st = state(false); // header_height 0 (no header)
        let mut sm = ScreenManager::empty();
        sm.state = &mut st;
        let p = Panel_new(0, 0, 10, 5, None);
        ScreenManager_insert(&mut sm, Box::new(p), 10, 0);
        assert_eq!(sm.panelCount, 1);
        assert_eq!(sm.panels[0].as_panel().w, 10); // explicit positive size kept
                                                   // height = LINES - y1 - header_height + y2 = LINES - 0 - 0 + (-1)
        assert_eq!(sm.panels[0].as_panel().h, Ncurses::lines() - 1);
        assert_eq!(
            (sm.panels[0].as_panel().x, sm.panels[0].as_panel().y),
            (0, 0)
        );
        assert!(sm.panels[0].as_panel().needsRedraw);
    }

    #[test]
    fn insert_negative_size_fills_remaining_width() {
        let mut st = state(false);
        let mut sm = ScreenManager::empty();
        sm.state = &mut st;
        let p = Panel_new(0, 0, 3, 5, None);
        ScreenManager_insert(&mut sm, Box::new(p), 0, 0); // size <= 0 -> COLS - x1 + x2 - lastX
                                                          // lastX 0 (idx 0), so width = COLS.
        assert_eq!(sm.panels[0].as_panel().w, Ncurses::cols());
    }

    #[test]
    fn add_appends_and_places_right_of_predecessor() {
        let mut st = state(false);
        let mut sm = ScreenManager::empty();
        sm.state = &mut st;
        ScreenManager_add(&mut sm, Box::new(Panel_new(0, 0, 5, 5, None)), 5);
        // second panel: lastX = panels[0].x + panels[0].w + 1 = 0 + 5 + 1
        ScreenManager_add(&mut sm, Box::new(Panel_new(0, 0, 8, 5, None)), 8);
        assert_eq!(sm.panelCount, 2);
        assert_eq!(sm.panels[1].as_panel().x, 6);
        assert_eq!(sm.panels[1].as_panel().w, 8);
        assert_eq!(sm.panels[1].as_panel().y, 0);
    }

    // ── resize ────────────────────────────────────────────────────────

    #[test]
    fn resize_relays_panels_across_the_width() {
        let mut st = state(false); // header_height 0
        let mut sm = sm_with_panels(&[10, 20]);
        sm.state = &mut st;
        ScreenManager_resize(&mut sm);
        let lines = Ncurses::lines();
        let cols = Ncurses::cols();
        // y1_header = 0; first panel keeps width 10, gets full height.
        assert_eq!(sm.panels[0].as_panel().w, 10);
        assert_eq!(sm.panels[0].as_panel().h, lines - 1); // LINES - 0 + (-1)
        assert_eq!(
            (sm.panels[0].as_panel().x, sm.panels[0].as_panel().y),
            (0, 0)
        );
        // lastX after first = 0 + 10 + 1 = 11; last panel takes the rest.
        assert_eq!(sm.panels[1].as_panel().x, 11);
        assert_eq!(sm.panels[1].as_panel().w, cols - 11); // COLS - x1 + x2 - lastX
        assert_eq!(sm.panels[1].as_panel().h, lines - 1);
    }

    #[test]
    fn resize_single_panel_takes_full_width() {
        let mut st = state(false);
        let mut sm = sm_with_panels(&[10]);
        sm.state = &mut st;
        ScreenManager_resize(&mut sm);
        // no non-last panels; lastX stays 0, single panel takes full COLS.
        assert_eq!(sm.panels[0].as_panel().w, Ncurses::cols());
        assert_eq!(sm.panels[0].as_panel().x, 0);
    }

    // ── remove ────────────────────────────────────────────────────────

    #[test]
    fn remove_returns_panel_and_updates_count() {
        let mut sm = sm_with_panels(&[10, 20, 5]);
        assert_eq!(sm.panelCount, 3);
        let removed = ScreenManager_remove(&mut sm, 1);
        assert_eq!(removed.as_panel().w, 20);
        assert_eq!(sm.panelCount, 2);
        assert_eq!(sm.panels.len(), 2);
    }

    #[test]
    fn remove_shifts_right_panels_left_by_width() {
        // panels at x=0(w10), x=11(w20), x=32(w5)
        let mut sm = sm_with_panels(&[10, 20, 5]);
        let x_third_before = sm.panels[2].as_panel().x; // 32
        ScreenManager_remove(&mut sm, 0); // removes w=10 panel
                                          // remaining panels each shift left by 10
        assert_eq!(sm.panels[0].as_panel().x, 11 - 10); // old second panel
        assert_eq!(sm.panels[1].as_panel().x, x_third_before - 10);
    }

    #[test]
    fn remove_last_panel_no_shift() {
        let mut sm = sm_with_panels(&[10, 20]);
        let first_x = sm.panels[0].as_panel().x;
        ScreenManager_remove(&mut sm, 1);
        assert_eq!(sm.panelCount, 1);
        assert_eq!(sm.panels[0].as_panel().x, first_x); // unchanged
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
