//! Partial port of `MainPanel.c` — htop's main process-list panel.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. A C fn `Foo_bar(Foo* this)`
//! ports to a free fn `Foo_bar(this: &mut Foo)` (the same shape the
//! `Panel.c`/`ColumnsPanel.c` ports use: free fns, not methods).
//!
//! # Data model
//!
//! htop's `MainPanel` (`MainPanel.h:21`) embeds a `Panel super`, a
//! `State* state` back-pointer, an owned `IncSet* inc`, an owned
//! `Htop_Action* keys` action table, two owned `FunctionBar*`
//! (`processBar`/`readonlyBar`), and `unsigned int idSearch`. The
//! [`MainPanel`] struct here models every field the ported functions
//! touch — the embedded `super` [`Panel`], the `state` pointer, the owned
//! [`IncSet`] and two owned [`FunctionBar`]s, and `idSearch` — following
//! the reduced-struct precedent of `columnspanel.rs`. `super_` avoids the
//! Rust `super` keyword, matching the `columnspanel.rs`/`process.rs`
//! convention.
//!
//! The C `Htop_Action* keys` action table is modeled as the `keys` field
//! (`Vec<Option<Htop_Action>>`, length `KEY_MAX`): [`MainPanel_new`] allocates
//! and fills it via [`Action_setBindings`], and [`MainPanel_eventHandler`]
//! dispatches `keys[ch](state)` through it. The `state` back-pointer is an
//! owning-elsewhere `*mut State` (the C field is a `State*` that `htop.c`
//! owns and shares), so [`MainPanel_setState`] stores it verbatim;
//! [`MainPanel_drawFunctionBar`]/[`MainPanel_printHeader`] now read through it
//! (`state->pauseUpdate`/`failedUpdate`, `state->host->settings`) since
//! `action::State` models `host`/`failedUpdate`.
//!
//! # Ported (self-contained, no unported substrate)
//!
//! - [`MainPanel_updateLabels`] (`MainPanel.c:32`) — retargets the F5
//!   (List/Tree) and F4 (Filter/FILTER) labels on the panel's default
//!   bar via the ported [`FunctionBar_setLabel`]. The C
//!   `MainPanel_getFunctionBar(this)` macro (`MainPanel.h:33`) is
//!   `((Panel*)this)->defaultBar`; the `Vec`-model's `defaultBar` is an
//!   `Option<FunctionBar>`, so the guard `if let Some(bar)` replaces the
//!   C null-deref-if-unset (the bar is always set in practice).
//! - `MainPanel_idSearch` (`MainPanel.c:38`, `static`) — the digit-key
//!   incremental PID search: builds a running id from typed digits,
//!   selects the first row whose `id` matches, and rolls the accumulator
//!   over at 10000000. Uses the ported `Panel_size`/`Panel_get`/
//!   `Panel_setSelected`; each item's embedded `Row` is reached via the
//!   `as_row()` vtable accessor (the safe-Rust analog of the C
//!   `(Row*)Panel_get(...)` upcast — panel items are platform `Process`
//!   objects, so an exact-type `Any` downcast to `Row` would miss).
//! - [`MainPanel_selectedRow`] (`MainPanel.c:175`) — the selected row's
//!   `id`, or `-1` when the list is empty (`Panel_getSelected` → `Row`).
//! - [`MainPanel_foreachRow`] (`MainPanel.c:180`) — applies `fn` to every
//!   tagged row (falling back to the selected row when none are tagged),
//!   AND-folding the results and reporting whether any were tagged. The
//!   ported `Panel_get` hands back an immutable `&dyn Object`, so — like
//!   `ColumnsPanel_cancelMoving` — the faithful mutating analog indexes
//!   `super.items` directly and downcasts each `&mut dyn Object` to
//!   `&mut Row`. The C `Arg arg` (a `union`) is `object::Arg`, which is
//!   not `Copy`, so [`MainPanel_foreachRowFn`] passes it by shared
//!   reference (`&Arg`); the callbacks only ever read it, so this is
//!   observationally identical to the C by-value pass.
//! - [`MainPanel_setState`] (`MainPanel.c:250`) — stores the `State*`
//!   back-pointer.
//! - [`MainPanel_setFunctionBar`] (`MainPanel.c:254`) — points the panel's
//!   (and the `IncSet`'s) default bar at the read-only or process bar. C
//!   aliases the one `FunctionBar*`; the `Vec`-model owns each bar via
//!   `Option<FunctionBar>`, so the target bar is cloned into both slots —
//!   the same clone-reproduces-the-shared-pointer mapping `Panel_init`/
//!   `Panel_setDefaultBar` already use.
//! - [`MainPanel_getValue`] (`MainPanel.c:54`, `static`) — returns
//!   `Row_sortKeyString(row)` (`Row.h:104`) for item `i`, dispatching through
//!   the now-modeled `RowClass.sortKeyString` vtable slot via the
//!   [`Object::row_class`] accessor; the slot's `Option<&[u8]>` (`None` = C
//!   `""`) is decoded to the `&str` the incremental-search callback consumes.
//! - [`MainPanel_drawFunctionBar`] (`MainPanel.c:198`, `static`) — draws the
//!   [`IncSet_drawBar`] bar (threading `&mut this.super_` for the panel the C
//!   `IncSet` reaches by back-pointer), then appends the `PAUSED` /
//!   `failedUpdate` marker read through the `*mut State` back-pointer (both
//!   fields now modeled). `CRT_colors[X]` resolves via
//!   `ColorElements::X.packed(ColorScheme::active())`.
//! - [`MainPanel_printHeader`] (`MainPanel.c:213`, `static`) — hands
//!   `state->host->settings` and the panel `header` to the ported
//!   [`Table_printHeader`]; `host`/`settings` are reached through the raw
//!   `*mut State` back-pointer (`action::State` now models `host`).
//!
//! [`MainPanel_eventHandler`] (`MainPanel.c:59`) and [`MainPanel_new`]
//! (`MainPanel.c:229`) are now ported: the `keys` [`Htop_Action`] table is
//! modeled (see above), [`Action_setBindings`] and [`IncSet_handleKey`] are
//! ported, and the constructor fills the table via `Action_setBindings` +
//! [`Platform_setBindings`]. One divergence is surfaced on
//! [`MainPanel_eventHandler`]: C passes `NULL` for `IncSet_handleKey`'s `lines`
//! argument (guarded by `filterChanged && lines`), but the ported
//! `IncSet_handleKey` takes a non-optional `&mut Vector` and dropped that NULL
//! guard, so an empty placeholder `Vector` is passed — restoring the optional
//! guard belongs in `incset.rs`.
//!
//! [`MainPanel_delete`] (`MainPanel.c:253`) is ported: its `free` chain
//! maps to the by-value drop idiom (`processBar`/`readonlyBar` handed to
//! `FunctionBar_delete`, `inc` to `IncSet_delete`, `super_` dropped in place
//! of `Panel_done`).
//!
//! `gen_port_report.py` counts `todo!()` bodies as *stubbed*, not
//! *ported*, so scaffolding does not inflate coverage.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)] // `MainPanel_foreachRowFn` mirrors the C typedef name
#![allow(dead_code)]

use crate::ported::action::{
    Action_setBindings, Action_setScreenTab, Action_setSortKey, Htop_Action, Htop_Reaction, State,
    HTOP_KEEP_FOLLOWING, HTOP_OK, HTOP_QUIT, HTOP_RECALCULATE, HTOP_REDRAW_BAR, HTOP_REFRESH,
    HTOP_RESIZE, HTOP_SAVE_SETTINGS, HTOP_UPDATE_PANELHDR,
};
use crate::ported::crt::{
    ColorElements, ColorScheme, ERR, KEY_F, KEY_LEFT, KEY_MAX, KEY_MOUSE, KEY_RESIZE, KEY_RIGHT,
};
use crate::ported::functionbar::{
    FunctionBar, FunctionBar_append, FunctionBar_delete, FunctionBar_new, FunctionBar_setLabel,
};
use crate::ported::incset::{
    IncSet, IncSet_delete, IncSet_drawBar, IncSet_filter, IncSet_handleKey, IncSet_new,
};
use crate::ported::object::{Arg, Object, Object_class};
use crate::ported::panel::{
    HandlerResult, Panel, PanelClass, Panel_get, Panel_getSelected, Panel_new, Panel_setSelected,
    Panel_setSelectionColor, Panel_size,
};
use crate::ported::row::RowField_keyAt;
use crate::ported::settings::{
    ScreenSettings_getActiveSortKey, ScreenSettings_invertSortOrder, Settings_isReadonly,
};
use crate::ported::table::Table_printHeader;
use crate::ported::vector::Vector_new;

// The platform-specific key bindings come from the compiled platform's
// `Platform.c` (htop links exactly one). On the darwin-first target that is
// darwin/Platform.c, whose `Platform_setBindings` is a no-op ((void) keys).
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_setBindings;
#[cfg(target_os = "freebsd")]
use crate::ported::freebsd::platform::Platform_setBindings;
#[cfg(target_os = "linux")]
use crate::ported::linux::platform::Platform_setBindings;
#[cfg(target_os = "netbsd")]
use crate::ported::netbsd::platform::Platform_setBindings;
#[cfg(target_os = "openbsd")]
use crate::ported::openbsd::platform::Platform_setBindings;
#[cfg(any(target_os = "solaris", target_os = "illumos"))]
use crate::ported::solaris::platform::Platform_setBindings;
#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "solaris",
    target_os = "illumos"
)))]
use crate::ported::unsupported::platform::Platform_setBindings;

/// Reduced model of the C `MainPanel` struct (`MainPanel.h:21`). See the
/// module docs for the `Htop_Action* keys` table and the `state`
/// back-pointer mapping. `super_` avoids the Rust `super` keyword.
pub struct MainPanel {
    /// C `Panel super` — the embedded panel base.
    pub super_: Panel,
    /// C `State* state` — back-pointer to the shared UI state, owned by
    /// `htop.c`. Modeled as a raw pointer ([`MainPanel_setState`] stores
    /// it); the functions that dereference it are stubbed.
    pub state: *mut State,
    /// C `IncSet* inc` — owned incremental search/filter set.
    pub inc: IncSet,
    /// C `FunctionBar* processBar` — owned bar with process-specific
    /// actions.
    pub processBar: FunctionBar,
    /// C `FunctionBar* readonlyBar` — owned bar without process actions.
    pub readonlyBar: FunctionBar,
    /// C `Htop_Action* keys` — the keypress → handler dispatch table
    /// (`xCalloc(KEY_MAX, sizeof(Htop_Action))`). Modeled as a
    /// `Vec<Option<Htop_Action>>` of length `KEY_MAX`: index `ch` yields the
    /// bound handler (`Some`) or `None` (the C `NULL` slot).
    /// [`MainPanel_eventHandler`] dispatches `keys[ch](state)`;
    /// [`MainPanel_new`] fills it via [`Action_setBindings`] +
    /// [`Platform_setBindings`].
    pub keys: Vec<Option<Htop_Action>>,
    /// C `unsigned int idSearch` — accumulator for digit-key PID search.
    pub idSearch: u32,
}

/// Port of `MainPanel.c`'s `const PanelClass MainPanel_class` vtable
/// (`MainPanel.c:209`). C sets `.eventHandler = MainPanel_eventHandler`,
/// `.drawFunctionBar = MainPanel_drawFunctionBar`, and
/// `.printHeader = MainPanel_printHeader`. This wires `event_handler`,
/// `draw_function_bar` and `print_header` to the ported
/// [`MainPanel_eventHandler`] / [`MainPanel_drawFunctionBar`] /
/// [`MainPanel_printHeader`].
impl PanelClass for MainPanel {
    fn as_panel(&self) -> &Panel {
        &self.super_
    }
    fn as_panel_mut(&mut self) -> &mut Panel {
        &mut self.super_
    }
    fn event_handler(&mut self, ev: i32) -> HandlerResult {
        MainPanel_eventHandler(self, ev)
    }
    fn draw_function_bar(&mut self, hide_function_bar: bool) {
        MainPanel_drawFunctionBar(self, hide_function_bar)
    }
    fn print_header(&mut self) {
        MainPanel_printHeader(self)
    }
}

/// Port of the `MainPanel_foreachRowFn` function-pointer typedef from
/// `MainPanel.h:31` (`typedef bool(*)(Row*, Arg)`). The C passes a `Row*` that
/// each callback upcasts to its concrete subclass (`(Process*)super`); the
/// faithful Rust analog is `&mut dyn Object` (the row object), which the ported
/// callbacks reach `as_process`/`as_row` through — a `Process` item cannot be
/// `downcast` to a bare `Row`. `Arg` is `Copy` (like the C union), passed by
/// value per call.
pub type MainPanel_foreachRowFn = fn(&mut dyn Object, Arg) -> bool;

/// Port of `static const char* const MainFunctions[]` (`MainPanel.c:28`) — the
/// process-mode function-bar labels (F1..F10). The C trailing `NULL`
/// terminator is dropped in the slice model (the ported [`FunctionBar_new`]
/// takes a plain slice, matching `incset.rs`'s `searchFunctions` mapping).
const MainFunctions: [&str; 10] = [
    "Help  ", "Setup ", "Search", "Filter", "Tree  ", "SortBy", "Nice -", "Nice +", "Kill  ",
    "Quit  ",
];

/// Port of `static const char* const MainFunctions_ro[]` (`MainPanel.c:29`) —
/// the read-only function-bar labels (the Nice/Kill slots blanked).
const MainFunctions_ro: [&str; 10] = [
    "Help  ", "Setup ", "Search", "Filter", "Tree  ", "SortBy", "      ", "      ", "      ",
    "Quit  ",
];

/// Port of `#define EVENT_IS_HEADER_CLICK(ev_)` (`Panel.h:38`).
fn EVENT_IS_HEADER_CLICK(ev: i32) -> bool {
    (-10000..=-9000).contains(&ev)
}
/// Port of `#define EVENT_HEADER_CLICK_GET_X(ev_)` (`Panel.h:39`).
fn EVENT_HEADER_CLICK_GET_X(ev: i32) -> i32 {
    ev + 10000
}
/// Port of `#define EVENT_IS_SCREEN_TAB_CLICK(ev_)` (`Panel.h:42`).
fn EVENT_IS_SCREEN_TAB_CLICK(ev: i32) -> bool {
    (-20000..-10000).contains(&ev)
}
/// Port of `#define EVENT_SCREEN_TAB_GET_X(ev_)` (`Panel.h:43`).
fn EVENT_SCREEN_TAB_GET_X(ev: i32) -> i32 {
    ev + 20000
}

/// Port of `void MainPanel_updateLabels(MainPanel* this, bool list, bool filter)`
/// from `MainPanel.c:32`. Sets the F5 label to `"List  "`/`"Tree  "` and
/// the F4 label to `"FILTER"`/`"Filter"` on the panel's default bar.
///
/// `MainPanel_getFunctionBar(this)` (`MainPanel.h:33`) is
/// `((Panel*)this)->defaultBar`; here that is `this.super_.defaultBar`
/// (an `Option<FunctionBar>`). The `if let Some(bar)` guard replaces the
/// C unconditional deref — the bar is always set once the panel is built.
pub fn MainPanel_updateLabels(this: &mut MainPanel, list: bool, filter: bool) {
    if let Some(bar) = this.super_.defaultBar.as_mut() {
        FunctionBar_setLabel(bar, KEY_F(5), if list { "List  " } else { "Tree  " });
        FunctionBar_setLabel(bar, KEY_F(4), if filter { "FILTER" } else { "Filter" });
    }
}

/// Port of `static void MainPanel_idSearch(MainPanel* this, int ch)` from
/// `MainPanel.c:38`. Builds a running id from the typed digit
/// (`ch - '0' + idSearch`), selects the first row whose `id` equals it,
/// then advances the accumulator to `id * 10`, resetting it to `0` once it
/// exceeds `10000000`.
///
/// The C `pid_t id = ch - 48 + this->idSearch;` mixes `int` and
/// `unsigned int`; for the digit-key range (`idSearch` bounded by the
/// rollover, `ch` an ASCII digit) the values stay small and non-negative,
/// so `i32` arithmetic reproduces it. Each `Panel_get` result's embedded
/// `Row` is reached via `as_row()` (the C `(const Row*)` upcast); a non-row
/// item is skipped (unreachable — a `MainPanel` holds only process rows).
fn MainPanel_idSearch(this: &mut MainPanel, ch: i32) {
    let id: i32 = ch - 48 + this.idSearch as i32;
    let size = Panel_size(&this.super_);
    for i in 0..size {
        let matches = {
            // mainPanel items are platform `Process` objects; reach the embedded
            // `Row` via `as_row()`, not an exact-type `Any` downcast (which misses).
            let obj: &dyn Object = Panel_get(&this.super_, i);
            obj.as_row().is_some_and(|row| row.id == id)
        };
        if matches {
            Panel_setSelected(&mut this.super_, i);
            break;
        }
    }
    this.idSearch = (id * 10) as u32;
    if this.idSearch > 10000000 {
        this.idSearch = 0;
    }
}

/// Port of `static const char* MainPanel_getValue(Panel* this, int i)` from
/// `MainPanel.c:54`. Returns `Row_sortKeyString(row)` for the item at index
/// `i` — the incremental-search key of that row.
///
/// `Row_sortKeyString(r_)` is the macro `As_Row(r_)->sortKeyString ?
/// As_Row(r_)->sortKeyString(r_) : ""` (`Row.h:104`); here that is the
/// [`RowClass::sortKeyString`](crate::ported::row::RowClass) vtable slot
/// reached via the [`Object::row_class`] accessor (the safe-Rust analog of the
/// C `As_Row(r_)` cast). The slot's `Row_SortKeyString` yields `Option<&[u8]>`
/// (`None`/`NULL` and the empty slot both map to the C `""`); the bytes are
/// the row's sort key, decoded as UTF-8 to match the `&str` the
/// incremental-search callback consumes (invalid bytes then `""`, never a
/// panic). The C `const char*` return type is that same borrowed string.
pub fn MainPanel_getValue(this: &Panel, i: i32) -> &str {
    // C: Row* row = (Row*) Panel_get(this, i);
    let row = Panel_get(this, i);
    // C: return Row_sortKeyString(row);  (Row.h:104 macro)
    match row
        .row_class()
        .and_then(|klass| klass.sortKeyString)
        .and_then(|slot| slot(row))
    {
        Some(bytes) => core::str::from_utf8(bytes).unwrap_or(""),
        None => "",
    }
}

/// Port of `static HandlerResult MainPanel_eventHandler(Panel* super, int ch)`
/// from `MainPanel.c:59`. The panel key dispatcher: it resolves header-tab /
/// screen-tab clicks, feeds the incremental search/filter, handles ESC, then
/// dispatches the keypress through the `keys[]` [`Htop_Action`] table, falling
/// back to digit id-search / arrow follow-mode; finally it maps the accumulated
/// [`Htop_Reaction`] bits to the [`HandlerResult`] flags the `ScreenManager`
/// consumes.
///
/// The C `Panel* super` upcast to `MainPanel*` is the reduced-struct receiver
/// `this: &mut MainPanel`. `Machine* host = this->state->host` and the
/// `settings`/`ss`/`activeTable` it reaches are threaded through the raw
/// `*mut State` back-pointer stored by [`MainPanel_setState`] (as in C, valid
/// for the main loop's lifetime). The `keys[ch](this->state)` dispatch copies
/// the `fn` pointer out of `keys` (fn pointers are `Copy`) and calls it with
/// `&mut *state` — the handler may reach back into `*this` via
/// `state->mainPanel` exactly as the C raw-pointer aliasing does.
///
/// # Divergence (surfaced for the coordinator)
///
/// C passes `NULL` for the `Vector* lines` argument of `IncSet_handleKey`
/// (the weak-panel backing is an `InfoScreen`-only concept; the main panel has
/// none), and `IncSet.c` guards `if (filterChanged && lines)` before touching
/// the weak panel. The ported [`IncSet_handleKey`] takes `lines: &mut Vector`
/// (non-optional) and dropped that `&& lines` NULL guard, so it would run
/// `updateWeakPanel` (which `Panel_prune`s the panel) against the main panel.
/// An empty placeholder `Vector` is passed here as the closest analog to
/// `NULL`; restoring the optional-`lines` guard belongs in `incset.rs`.
pub fn MainPanel_eventHandler(this: &mut MainPanel, ch: i32) -> HandlerResult {
    // C: MainPanel* this = (MainPanel*) super;
    //    Machine* host = this->state->host;
    let state = this.state;
    // SAFETY: `state`/`host` are the non-owning back-pointers wired at startup
    // (C precondition: `this->state->host` is dereferenced unconditionally).
    let host = unsafe { (*state).host };
    let mut reaction: Htop_Reaction = HTOP_OK;
    let mut result: HandlerResult = HandlerResult::IGNORED;

    // C: /* Let supervising ScreenManager handle resize */
    //    if (ch == KEY_RESIZE) return IGNORED;
    if ch == KEY_RESIZE {
        return HandlerResult::IGNORED;
    }

    // C: /* reset on every normal key */  bool needReset = ch != ERR;
    let mut needReset = ch != ERR;
    // C: #ifdef HAVE_GETMOUSE
    //    /* except mouse events while mouse support is disabled */
    //    if (!(ch != KEY_MOUSE || host->settings->enableMouse)) needReset = false;
    // SAFETY: host is a valid Machine* (C precondition).
    let enableMouse = unsafe {
        (*host)
            .settings
            .as_ref()
            .expect("MainPanel_eventHandler: host->settings is NULL")
            .enableMouse
    };
    if !(ch != KEY_MOUSE || enableMouse) {
        needReset = false;
    }
    // C: if (needReset) this->state->hideSelection = false;
    if needReset {
        // SAFETY: state is a valid State* (C precondition).
        unsafe {
            (*state).hideSelection = false;
        }
    }

    // C: Settings* settings = host->settings;  ScreenSettings* ss = settings->ss;
    // (modeled as host->settings and settings.screens[ssIndex] — reached per
    // branch through the raw host pointer to keep borrows non-overlapping.)

    if EVENT_IS_HEADER_CLICK(ch) {
        // C: int x = EVENT_HEADER_CLICK_GET_X(ch);
        //    int hx = super->scrollH + x + 1;
        //    RowField field = RowField_keyAt(settings, hx);
        let x = EVENT_HEADER_CLICK_GET_X(ch);
        let hx = this.super_.scrollH + x + 1;
        // SAFETY: host valid (C precondition).
        let ssidx = unsafe { (*host).settings.as_ref().unwrap().ssIndex as usize };
        let field = unsafe { RowField_keyAt((*host).settings.as_ref().unwrap(), hx) };
        let (treeView, alwaysByPID) = unsafe {
            let s = &(*host).settings.as_ref().unwrap().screens[ssidx];
            (s.treeView, s.treeViewAlwaysByPID)
        };
        if treeView && alwaysByPID {
            // C: ss->treeView = false; ss->direction = 1;
            //    reaction |= Action_setSortKey(settings, field);
            unsafe {
                let s = &mut (*host).settings.as_mut().unwrap().screens[ssidx];
                s.treeView = false;
                s.direction = 1;
            }
            reaction |= unsafe { Action_setSortKey((*host).settings.as_mut().unwrap(), field) };
        } else if field
            == unsafe {
                ScreenSettings_getActiveSortKey(&(*host).settings.as_ref().unwrap().screens[ssidx])
            }
        {
            // C: ScreenSettings_invertSortOrder(ss);
            unsafe {
                ScreenSettings_invertSortOrder(
                    &mut (*host).settings.as_mut().unwrap().screens[ssidx],
                );
            }
        } else {
            // C: reaction |= Action_setSortKey(settings, field);
            reaction |= unsafe { Action_setSortKey((*host).settings.as_mut().unwrap(), field) };
        }
        // C: reaction |= HTOP_RECALCULATE | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR | HTOP_SAVE_SETTINGS;
        reaction |= HTOP_RECALCULATE | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR | HTOP_SAVE_SETTINGS;
        result = HandlerResult::HANDLED;
    } else if EVENT_IS_SCREEN_TAB_CLICK(ch) {
        // C: int x = EVENT_SCREEN_TAB_GET_X(ch);
        //    reaction |= Action_setScreenTab(this->state, x);
        let x = EVENT_SCREEN_TAB_GET_X(ch);
        // SAFETY: state valid (C precondition).
        reaction |= Action_setScreenTab(unsafe { &*state }, x);
        result = HandlerResult::HANDLED;
    } else if ch != ERR && this.inc.active.is_some() {
        // C: bool filterChanged = IncSet_handleKey(this->inc, ch, super, MainPanel_getValue, NULL);
        // See the divergence note: NULL lines -> empty placeholder Vector.
        let mut lines = Vector_new(&Object_class, false, 10);
        let filterChanged = IncSet_handleKey(
            &mut this.inc,
            ch,
            &mut this.super_,
            MainPanel_getValue,
            &mut lines,
        );
        if filterChanged {
            // C: host->activeTable->incFilter = IncSet_filter(this->inc);
            let filter = IncSet_filter(&this.inc).map(|s| s.to_string());
            // SAFETY: host valid; activeTable is the non-null back-pointer.
            let at = unsafe {
                (*host)
                    .activeTable
                    .expect("MainPanel_eventHandler: host->activeTable is NULL")
            };
            unsafe {
                (*at).incFilter = filter;
            }
            // C: reaction = HTOP_REFRESH | HTOP_REDRAW_BAR;
            reaction = HTOP_REFRESH | HTOP_REDRAW_BAR;
        }
        // C: if (this->inc->found && this->inc->active && !this->inc->active->isFilter)
        let followFound = this.inc.found
            && this
                .inc
                .active
                .is_some_and(|a| !this.inc.modes[a as usize].isFilter);
        if followFound {
            // C: host->activeTable->following = MainPanel_selectedRow(this);
            let sel = MainPanel_selectedRow(this);
            let at = unsafe {
                (*host)
                    .activeTable
                    .expect("MainPanel_eventHandler: host->activeTable is NULL")
            };
            unsafe {
                (*at).following = sel;
            }
            // C: Panel_setSelectionColor(super, PANEL_SELECTION_FOLLOW);
            Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_SELECTION_FOLLOW);
            // C: reaction |= HTOP_KEEP_FOLLOWING;
            reaction |= HTOP_KEEP_FOLLOWING;
        }
        result = HandlerResult::HANDLED;
    } else if ch == 27 {
        // C: this->state->hideSelection = true; return HANDLED;
        unsafe {
            (*state).hideSelection = true;
        }
        return HandlerResult::HANDLED;
    } else if ch != ERR && ch > 0 && ch < KEY_MAX && this.keys[ch as usize].is_some() {
        // C: reaction |= (this->keys[ch])(this->state);
        let handler = this.keys[ch as usize].unwrap();
        // SAFETY: state valid (C precondition); the handler may alias *this via
        // state->mainPanel, matching the C raw-pointer dispatch.
        reaction |= handler(unsafe { &mut *state });
        result = HandlerResult::HANDLED;
    } else if 0 < ch && ch < 255 && (ch as u8).is_ascii_digit() {
        // C: MainPanel_idSearch(this, ch);
        MainPanel_idSearch(this, ch);
    } else if ch == KEY_LEFT || ch == KEY_RIGHT {
        // C: reaction |= HTOP_KEEP_FOLLOWING;
        reaction |= HTOP_KEEP_FOLLOWING;
    } else {
        // C: if (ch != ERR) this->idSearch = 0; else reaction |= HTOP_KEEP_FOLLOWING;
        if ch != ERR {
            this.idSearch = 0;
        } else {
            reaction |= HTOP_KEEP_FOLLOWING;
        }
    }

    // C: if ((reaction & HTOP_REDRAW_BAR) == HTOP_REDRAW_BAR)
    //       MainPanel_updateLabels(this, settings->ss->treeView, host->activeTable->incFilter);
    if reaction & HTOP_REDRAW_BAR == HTOP_REDRAW_BAR {
        // SAFETY: host valid (C precondition).
        let treeView = unsafe {
            let s = (*host).settings.as_ref().unwrap();
            s.screens[s.ssIndex as usize].treeView
        };
        let filter = unsafe {
            let at = (*host)
                .activeTable
                .expect("MainPanel_eventHandler: host->activeTable is NULL");
            (*at).incFilter.is_some()
        };
        MainPanel_updateLabels(this, treeView, filter);
    }
    // C: if ((reaction & HTOP_RESIZE) == HTOP_RESIZE) result |= RESIZE;
    if reaction & HTOP_RESIZE == HTOP_RESIZE {
        result |= HandlerResult::RESIZE;
    }
    // C: if ((reaction & HTOP_UPDATE_PANELHDR) == HTOP_UPDATE_PANELHDR) result |= REDRAW;
    if reaction & HTOP_UPDATE_PANELHDR == HTOP_UPDATE_PANELHDR {
        result |= HandlerResult::REDRAW;
    }
    // C: if ((reaction & HTOP_REFRESH) == HTOP_REFRESH) result |= REFRESH;
    if reaction & HTOP_REFRESH == HTOP_REFRESH {
        result |= HandlerResult::REFRESH;
    }
    // C: if ((reaction & HTOP_RECALCULATE) == HTOP_RECALCULATE) result |= RESCAN;
    if reaction & HTOP_RECALCULATE == HTOP_RECALCULATE {
        result |= HandlerResult::RESCAN;
    }
    // C: if ((reaction & HTOP_SAVE_SETTINGS) == HTOP_SAVE_SETTINGS) host->settings->changed = true;
    if reaction & HTOP_SAVE_SETTINGS == HTOP_SAVE_SETTINGS {
        unsafe {
            (*host).settings.as_mut().unwrap().changed = true;
        }
    }
    // C: if ((reaction & HTOP_QUIT) == HTOP_QUIT) return BREAK_LOOP;
    if reaction & HTOP_QUIT == HTOP_QUIT {
        return HandlerResult::BREAK_LOOP;
    }
    // C: if ((reaction & HTOP_KEEP_FOLLOWING) != HTOP_KEEP_FOLLOWING) {
    //       host->activeTable->following = -1;
    //       Panel_setSelectionColor(super, PANEL_SELECTION_FOCUS);
    //    }
    if reaction & HTOP_KEEP_FOLLOWING != HTOP_KEEP_FOLLOWING {
        let at = unsafe {
            (*host)
                .activeTable
                .expect("MainPanel_eventHandler: host->activeTable is NULL")
        };
        unsafe {
            (*at).following = -1;
        }
        Panel_setSelectionColor(&mut this.super_, ColorElements::PANEL_SELECTION_FOCUS);
    }
    // C: return result;
    result
}

/// Port of `int MainPanel_selectedRow(MainPanel* this)` from
/// `MainPanel.c:175`. Returns the selected row's `id`, or `-1` when the
/// list is empty (`Panel_getSelected` returns `None`) or the selected item
/// has no embedded `Row` (reached via `as_row()`, the C `(const Row*)`
/// upcast; a `MainPanel` holds only process rows).
pub fn MainPanel_selectedRow(this: &MainPanel) -> i32 {
    match Panel_getSelected(&this.super_) {
        // mainPanel items are platform `Process` objects; reach the embedded
        // `Row` via `as_row()`, not an exact-type `Any` downcast (which misses,
        // returning -1 for every real row).
        Some(obj) => obj.as_row().map_or(-1, |row| row.id),
        None => -1,
    }
}

/// Port of `bool MainPanel_foreachRow(MainPanel* this,
/// MainPanel_foreachRowFn fn, Arg arg, bool* wasAnyTagged)` from
/// `MainPanel.c:180`. Applies `fn` to every tagged row, AND-folding the
/// returned `bool`s into `ok`; if no row was tagged, applies `fn` to the
/// selected row instead. Reports whether any row was tagged through the
/// optional `wasAnyTagged` out-param (C `bool*`).
///
/// The ported `Panel_get` returns an immutable `&dyn Object`, so — like
/// `ColumnsPanel_cancelMoving` — the mutating analog indexes
/// `super.items` directly and downcasts each `&mut dyn Object` to
/// `&mut Row`. A `Vec` element is never null, so the C `if (row)` guard in
/// the tagged loop (which the C omits anyway) is not needed; the selected
/// fallback keeps the C `if (row)` guard via `Panel_getSelected`'s
/// bounds/emptiness check.
pub fn MainPanel_foreachRow(
    this: &mut MainPanel,
    fn_: MainPanel_foreachRowFn,
    arg: Arg,
    wasAnyTagged: Option<&mut bool>,
) -> bool {
    let mut ok = true;
    let mut anyTagged = false;
    let size = Panel_size(&this.super_);
    for i in 0..size {
        let obj: &mut dyn Object = this.super_.items[i as usize].object_mut();
        // C: `if (row->tag)` on the `Row*`. Panel items are `Process` objects,
        // so read the tag through the embedded `Row` (`as_row`); the callback
        // receives the object and upcasts it itself, mirroring the C `Row*`.
        let tagged = obj.as_row().is_some_and(|r| r.tag);
        if tagged {
            ok &= fn_(obj, arg);
            anyTagged = true;
        }
    }
    if !anyTagged {
        // C: Row* row = (Row*) Panel_getSelected(super); if (row) ...
        let sel = this.super_.selected;
        if sel >= 0 && (sel as usize) < this.super_.items.len() {
            let obj: &mut dyn Object = this.super_.items[sel as usize].object_mut();
            ok &= fn_(obj, arg);
        }
    }

    if let Some(w) = wasAnyTagged {
        *w = anyTagged;
    }

    ok
}

/// Port of `static void MainPanel_drawFunctionBar(Panel* super,
/// bool hideFunctionBar)` from `MainPanel.c:198`. Draws the incremental
/// search/filter bar, then appends the `PAUSED` marker (when the UI is
/// paused) or the failed-read message (`state->failedUpdate`) to it.
///
/// The C `Panel* super` upcast to `MainPanel*` becomes the reduced-struct
/// receiver `this: &mut MainPanel` (the sibling-panel convention). The C
/// `IncSet_drawBar(this->inc, CRT_colors[FUNCTION_BAR])` becomes the ported
/// [`IncSet_drawBar`], which threads the panel (`&mut this.super_`) that the
/// C `IncSet` reaches through its own back-pointer, plus the resolved
/// `FUNCTION_BAR` color. `state` is the raw `*mut State` back-pointer stored by
/// [`MainPanel_setState`]; `pauseUpdate`/`failedUpdate` are read through it as
/// C reads `this->state->...`. `CRT_colors[X]` resolves via
/// `ColorElements::X.packed(ColorScheme::active())`, the same lookup
/// [`crate::ported::panel::Panel_draw`] uses.
pub fn MainPanel_drawFunctionBar(this: &mut MainPanel, hideFunctionBar: bool) {
    // C: if (hideFunctionBar && !this->inc->active) return;  (keep active bar)
    if hideFunctionBar && this.inc.active.is_none() {
        return;
    }

    // C: IncSet_drawBar(this->inc, CRT_colors[FUNCTION_BAR]);
    IncSet_drawBar(
        &mut this.inc,
        &mut this.super_,
        ColorElements::FUNCTION_BAR.packed(ColorScheme::active()),
    );

    // C: this->state->pauseUpdate / this->state->failedUpdate
    // SAFETY: `state` is the non-owning back-pointer stored by MainPanel_setState.
    let state = unsafe { &*this.state };
    if state.pauseUpdate {
        // C: FunctionBar_append("PAUSED", CRT_colors[PAUSED]);
        FunctionBar_append(
            "PAUSED",
            ColorElements::PAUSED.packed(ColorScheme::active()),
        );
    } else if let Some(msg) = &state.failedUpdate {
        // C: FunctionBar_append(this->state->failedUpdate, CRT_colors[FAILED_READ]);
        FunctionBar_append(
            msg,
            ColorElements::FAILED_READ.packed(ColorScheme::active()),
        );
    }
}

/// Port of `static void MainPanel_printHeader(Panel* super)` from
/// `MainPanel.c:213`. Calls `Table_printHeader(host->settings, &super->header)`
/// to render the process-list column header.
///
/// The C `Panel* super` upcast to `MainPanel*` becomes `this: &mut MainPanel`.
/// `this->state->host` (a `Machine*`) is reached through the raw `*mut State`
/// back-pointer; its owned [`Settings`](crate::ported::settings::Settings)
/// (`host->settings`, an `Option<Settings>`) is handed with the panel's own
/// `header` [`RichString`](crate::ported::richstring::RichString) to the ported
/// [`Table_printHeader`]. The `host`/`settings` borrows derive from the raw
/// pointer (independent of the `&mut this` borrow of the header), so they
/// coexist.
///
/// # Safety
///
/// `this.state` must be the valid non-owning `State` pointer stored by
/// [`MainPanel_setState`], and its `host` a valid `Machine` with settings.
pub fn MainPanel_printHeader(this: &mut MainPanel) {
    // C: Machine* host = this->state->host;
    // SAFETY: `state`/`host` are the non-owning back-pointers wired at startup.
    let host = unsafe { &*(*this.state).host };
    let settings = host
        .settings
        .as_ref()
        .expect("MainPanel_printHeader: host->settings is NULL");
    // C: Table_printHeader(host->settings, &super->header);
    Table_printHeader(settings, &mut this.super_.header);
}

/// Port of `MainPanel* MainPanel_new(void)` from `MainPanel.c:229`. Builds the
/// process/read-only function bars, initializes the embedded panel with the
/// active bar, allocates the `keys` [`Htop_Action`] table, creates the
/// [`IncSet`], and fills the table via [`Action_setBindings`] +
/// [`Platform_setBindings`].
///
/// The C `AllocThis(MainPanel)` (zeroed) + `Panel_init((Panel*)this, …)` maps to
/// building the struct with an initialized [`Panel_new`]. `FunctionBar_new(…,
/// NULL, NULL)` becomes `FunctionBar_new(Some(&MainFunctions), None, None)`.
/// C aliases one `FunctionBar*` as `activeBar` into `Panel_init`/`IncSet_new`;
/// the `Vec`-owned bar model clones the chosen bar into those slots (the same
/// clone-reproduces-the-shared-pointer mapping `Panel_init`/`MainPanel_setFunctionBar`
/// use). `keys = xCalloc(KEY_MAX, sizeof(Htop_Action))` is `vec![None; KEY_MAX]`.
/// The `state` back-pointer is `NULL` until [`MainPanel_setState`] (as in C,
/// where `AllocThis` zeroes it).
pub fn MainPanel_new() -> MainPanel {
    // C: this->processBar  = FunctionBar_new(MainFunctions, NULL, NULL);
    //    this->readonlyBar = FunctionBar_new(MainFunctions_ro, NULL, NULL);
    let processBar = FunctionBar_new(Some(&MainFunctions), None, None);
    let readonlyBar = FunctionBar_new(Some(&MainFunctions_ro), None, None);
    // C: FunctionBar* activeBar = Settings_isReadonly() ? this->readonlyBar : this->processBar;
    let activeBar = if Settings_isReadonly() {
        readonlyBar.clone()
    } else {
        processBar.clone()
    };
    // C: Panel_init((Panel*) this, 1, 1, 1, 1, Class(Row), false, activeBar);
    // (Panel_new = AllocThis + Panel_init; the Class(Row)/owner args have no
    // analog in the reduced Panel model.)
    let super_ = Panel_new(1, 1, 1, 1, Some(activeBar.clone()));
    // C: this->keys = xCalloc(KEY_MAX, sizeof(Htop_Action));
    let mut keys: Vec<Option<Htop_Action>> = vec![None; KEY_MAX as usize];
    // C: this->inc = IncSet_new(activeBar);
    let inc = IncSet_new(Some(activeBar));

    // C: Action_setBindings(this->keys);  Platform_setBindings(this->keys);
    Action_setBindings(&mut keys);
    Platform_setBindings(&mut keys);

    MainPanel {
        super_,
        state: core::ptr::null_mut(),
        inc,
        processBar,
        readonlyBar,
        keys,
        idSearch: 0,
    }
}

/// Port of `void MainPanel_setState(MainPanel* this, State* state)` from
/// `MainPanel.c:250`. Stores the shared-state back-pointer.
pub fn MainPanel_setState(this: &mut MainPanel, state: *mut State) {
    this.state = state;
}

/// Port of `void MainPanel_setFunctionBar(MainPanel* this, bool readonly)`
/// from `MainPanel.c:254`. Points the panel's default bar (and the
/// `IncSet`'s default bar) at the read-only or process bar.
///
/// C aliases the one `FunctionBar*` into both `super.defaultBar` and
/// `inc->defaultBar`; the `Vec`-model owns each bar via
/// `Option<FunctionBar>`, so the chosen bar is cloned into both slots —
/// the same clone-reproduces-the-shared-pointer mapping `Panel_init` and
/// `Panel_setDefaultBar` use.
pub fn MainPanel_setFunctionBar(this: &mut MainPanel, readonly: bool) {
    let bar = if readonly {
        this.readonlyBar.clone()
    } else {
        this.processBar.clone()
    };
    this.super_.defaultBar = Some(bar);
    this.inc.defaultBar = this.super_.defaultBar.clone();
}

/// Port of `void MainPanel_delete(Object* object)` from `MainPanel.c:253`:
/// `FunctionBar_delete(processBar); FunctionBar_delete(readonlyBar);
/// IncSet_delete(inc); free(keys); Panel_done(&super); free(this);`.
///
/// Taking `this` by value reproduces `free(this)`. The two owned
/// [`FunctionBar`]s and the owned [`IncSet`] are handed to
/// [`FunctionBar_delete`]/[`IncSet_delete`] (mirroring the C call graph); the
/// `keys` action table (a `Vec` of `fn` pointers) drops in place — the analog
/// of `free(keys)`, which frees only the array, not the (static) handlers; and
/// the embedded `super_` [`Panel`] plus the non-owning `state` pointer drop
/// with the remaining fields — the faithful analog of `Panel_done(&super)` (a
/// `Drop` no-op, so the panicking `Panel_done` stub is avoided) and the struct
/// free.
pub fn MainPanel_delete(this: MainPanel) {
    let MainPanel {
        super_,
        inc,
        processBar,
        readonlyBar,
        ..
    } = this;
    FunctionBar_delete(processBar);
    FunctionBar_delete(readonlyBar);
    IncSet_delete(inc);
    let _ = super_;
}

#[cfg(test)]
use crate::ported::panel::PanelItem;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::functionbar::FunctionBar_new;
    use crate::ported::incset::IncSet_new;
    use crate::ported::panel::Panel_new;
    use crate::ported::row::Row;
    use core::any::Any;

    /// A zeroed `MainPanel` for tests — not a C function; `MainPanel_new`
    /// (the real constructor) is stubbed on `Action_setBindings`/
    /// `Platform_setBindings`, so tests assemble the struct directly (the
    /// same way `Panel::empty`/`IncMode::empty` back their sibling tests).
    fn blank() -> MainPanel {
        MainPanel {
            super_: Panel_new(1, 1, 1, 1, None),
            state: core::ptr::null_mut(),
            inc: IncSet_new(None),
            processBar: FunctionBar_new(None, None, None),
            readonlyBar: FunctionBar_new(None, None, None),
            keys: vec![None; KEY_MAX as usize],
            idSearch: 0,
        }
    }

    fn row(id: i32) -> Box<dyn Object> {
        Box::new(Row {
            id,
            ..Row::default()
        })
    }

    // ── updateLabels ──────────────────────────────────────────────────

    fn f5f4_bar() -> FunctionBar {
        // A bar carrying the F5 and F4 events MainPanel_updateLabels edits.
        FunctionBar_new(
            Some(&["Tree  ", "Filter"]),
            Some(&["F5", "F4"]),
            Some(&[KEY_F(5), KEY_F(4)]),
        )
    }

    #[test]
    fn update_labels_list_and_filter_active() {
        let mut mp = blank();
        mp.super_.defaultBar = Some(f5f4_bar());
        MainPanel_updateLabels(&mut mp, true, true);
        let bar = mp.super_.defaultBar.as_ref().unwrap();
        assert_eq!(bar.functions[0], "List  "); // F5, list mode
        assert_eq!(bar.functions[1], "FILTER"); // F4, filter active
    }

    #[test]
    fn update_labels_tree_and_filter_inactive() {
        let mut mp = blank();
        mp.super_.defaultBar = Some(f5f4_bar());
        MainPanel_updateLabels(&mut mp, false, false);
        let bar = mp.super_.defaultBar.as_ref().unwrap();
        assert_eq!(bar.functions[0], "Tree  "); // F5, tree mode
        assert_eq!(bar.functions[1], "Filter"); // F4, filter inactive
    }

    #[test]
    fn update_labels_no_bar_is_noop() {
        // defaultBar None -> the Some-guard skips (no C null deref).
        let mut mp = blank();
        mp.super_.defaultBar = None;
        MainPanel_updateLabels(&mut mp, true, true);
        assert!(mp.super_.defaultBar.is_none());
    }

    // ── selectedRow ───────────────────────────────────────────────────

    #[test]
    fn selected_row_empty_is_minus_one() {
        let mp = blank();
        assert_eq!(MainPanel_selectedRow(&mp), -1);
    }

    #[test]
    fn selected_row_returns_selected_id() {
        let mut mp = blank();
        mp.super_.items.push(PanelItem::Owned(row(100)));
        mp.super_.items.push(PanelItem::Owned(row(200)));
        mp.super_.items.push(PanelItem::Owned(row(300)));
        mp.super_.selected = 1;
        assert_eq!(MainPanel_selectedRow(&mp), 200);
        mp.super_.selected = 2;
        assert_eq!(MainPanel_selectedRow(&mp), 300);
    }

    // ── idSearch ──────────────────────────────────────────────────────

    #[test]
    fn id_search_selects_matching_row_and_accumulates() {
        let mut mp = blank();
        // ids 1, 12, 123 so successive digit keys narrow the match.
        mp.super_.items.push(PanelItem::Owned(row(1)));
        mp.super_.items.push(PanelItem::Owned(row(12)));
        mp.super_.items.push(PanelItem::Owned(row(123)));
        // Type '1' -> id = 1 -> selects row 0; idSearch becomes 10.
        MainPanel_idSearch(&mut mp, b'1' as i32);
        assert_eq!(mp.super_.selected, 0);
        assert_eq!(mp.idSearch, 10);
        // Type '2' -> id = 2 + 10 = 12 -> selects row 1; idSearch 120.
        MainPanel_idSearch(&mut mp, b'2' as i32);
        assert_eq!(mp.super_.selected, 1);
        assert_eq!(mp.idSearch, 120);
        // Type '3' -> id = 3 + 120 = 123 -> selects row 2; idSearch 1230.
        MainPanel_idSearch(&mut mp, b'3' as i32);
        assert_eq!(mp.super_.selected, 2);
        assert_eq!(mp.idSearch, 1230);
    }

    #[test]
    fn id_search_no_match_keeps_selection_but_advances() {
        let mut mp = blank();
        mp.super_.items.push(PanelItem::Owned(row(1)));
        mp.super_.items.push(PanelItem::Owned(row(2)));
        mp.super_.selected = 1;
        // Type '9' -> id 9, no row has id 9: selection unchanged, acc 90.
        MainPanel_idSearch(&mut mp, b'9' as i32);
        assert_eq!(mp.super_.selected, 1);
        assert_eq!(mp.idSearch, 90);
    }

    #[test]
    fn id_search_rolls_over_past_ten_million() {
        let mut mp = blank();
        mp.super_.items.push(PanelItem::Owned(row(1)));
        mp.idSearch = 1_000_001; // id = 0 + 1_000_001 -> acc 10_000_010 > 1e7
        MainPanel_idSearch(&mut mp, b'0' as i32);
        assert_eq!(mp.idSearch, 0);
    }

    // ── foreachRow ────────────────────────────────────────────────────

    // Callback: bumps a counter (carried in Arg::V) and stamps the row's
    // indent so we can see which rows it visited. Matches the C typedef
    // `bool(Row*, Arg)` — takes the row object and reaches its Row via as_row_mut.
    fn visit_cb(obj: &mut dyn Object, arg: Arg) -> bool {
        if let Arg::V(p) = arg {
            unsafe {
                *(p as *mut i32) += 1;
            }
        }
        if let Some(row) = obj.as_row_mut() {
            row.indent = 99;
        }
        true
    }

    // Callback returning false, to check the AND-fold of `ok`.
    fn fail_cb(_obj: &mut dyn Object, _arg: Arg) -> bool {
        false
    }

    #[test]
    fn foreach_row_applies_to_tagged_only() {
        let mut mp = blank();
        for id in [10, 20, 30] {
            mp.super_.items.push(PanelItem::Owned(row(id)));
        }
        // Tag rows 0 and 2.
        {
            let a: &mut dyn Any = mp.super_.items[0].object_mut();
            a.downcast_mut::<Row>().unwrap().tag = true;
        }
        {
            let a: &mut dyn Any = mp.super_.items[2].object_mut();
            a.downcast_mut::<Row>().unwrap().tag = true;
        }
        let mut count: i32 = 0;
        let mut any_tagged = false;
        let arg = Arg::V(&mut count as *mut i32 as *mut core::ffi::c_void);
        let ok = MainPanel_foreachRow(&mut mp, visit_cb, arg, Some(&mut any_tagged));
        assert!(ok);
        assert!(any_tagged);
        assert_eq!(count, 2); // rows 0 and 2 visited
                              // Visited rows stamped; the untagged middle row was not.
        let indent_of = |i: usize, mp: &mut MainPanel| -> i32 {
            let a: &mut dyn Any = mp.super_.items[i].object_mut();
            a.downcast_mut::<Row>().unwrap().indent
        };
        assert_eq!(indent_of(0, &mut mp), 99);
        assert_eq!(indent_of(1, &mut mp), 0);
        assert_eq!(indent_of(2, &mut mp), 99);
    }

    #[test]
    fn foreach_row_falls_back_to_selected_when_none_tagged() {
        let mut mp = blank();
        for id in [10, 20, 30] {
            mp.super_.items.push(PanelItem::Owned(row(id)));
        }
        mp.super_.selected = 1;
        let mut count: i32 = 0;
        let mut any_tagged = true; // must be overwritten to false
        let arg = Arg::V(&mut count as *mut i32 as *mut core::ffi::c_void);
        let ok = MainPanel_foreachRow(&mut mp, visit_cb, arg, Some(&mut any_tagged));
        assert!(ok);
        assert!(!any_tagged);
        assert_eq!(count, 1); // only the selected row
        let a: &mut dyn Any = mp.super_.items[1].object_mut();
        assert_eq!(a.downcast_mut::<Row>().unwrap().indent, 99);
    }

    #[test]
    fn foreach_row_ands_the_callback_results() {
        let mut mp = blank();
        mp.super_.items.push(PanelItem::Owned(row(10)));
        {
            let a: &mut dyn Any = mp.super_.items[0].object_mut();
            a.downcast_mut::<Row>().unwrap().tag = true;
        }
        let ok = MainPanel_foreachRow(&mut mp, fail_cb, Arg::I(0), None);
        assert!(!ok); // fail_cb returned false
    }

    #[test]
    fn foreach_row_wastagged_out_param_is_optional() {
        // Passing None for wasAnyTagged must not panic.
        let mut mp = blank();
        mp.super_.items.push(PanelItem::Owned(row(10)));
        mp.super_.selected = 0;
        let ok = MainPanel_foreachRow(&mut mp, visit_cb, Arg::I(0), None);
        assert!(ok);
    }

    // ── setState / setFunctionBar ─────────────────────────────────────

    #[test]
    fn set_state_stores_pointer() {
        let mut mp = blank();
        let mut st = State {
            host: core::ptr::null_mut(),
            mainPanel: core::ptr::null_mut(),
            header: core::ptr::null_mut(),
            failedUpdate: None,
            pauseUpdate: false,
            hideSelection: false,
            hideMeters: false,
        };
        MainPanel_setState(&mut mp, &mut st as *mut State);
        assert_eq!(mp.state, &mut st as *mut State);
    }

    #[test]
    fn set_function_bar_selects_readonly_or_process() {
        let mut mp = blank();
        mp.processBar = FunctionBar_new(Some(&["PROC"]), Some(&["F1"]), Some(&[1]));
        mp.readonlyBar = FunctionBar_new(Some(&["RO"]), Some(&["F1"]), Some(&[1]));

        MainPanel_setFunctionBar(&mut mp, true);
        assert_eq!(
            mp.super_.defaultBar.as_ref().unwrap().functions,
            vec!["RO".to_string()]
        );
        // The IncSet's default bar tracks the panel's.
        assert_eq!(
            mp.inc.defaultBar.as_ref().unwrap().functions,
            vec!["RO".to_string()]
        );

        MainPanel_setFunctionBar(&mut mp, false);
        assert_eq!(
            mp.super_.defaultBar.as_ref().unwrap().functions,
            vec!["PROC".to_string()]
        );
        assert_eq!(
            mp.inc.defaultBar.as_ref().unwrap().functions,
            vec!["PROC".to_string()]
        );
    }
}
