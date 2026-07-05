//! Partial port of `Action.c` — htop's keybinding action handlers.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake` and
//! `camelCase`), so `non_snake_case` is allowed for the whole module —
//! matching the spec name-for-name is the point of the port.
//!
//! `Action.c` is almost entirely UI glue: nearly every `actionXxx`
//! handler drives a `Panel`/`ScreenManager`, mutates a `Settings`/
//! `Machine`/`Table`, calls ncurses (`clear`, `attrset`, `refresh`,
//! `beep`, `napms`), spawns child screens (lsof/strace/env/command),
//! or issues syscalls (`getpwnam`, signals). Most of that substrate is
//! unported or unreachable from the minimal `State` model, so those
//! handlers stay as their exact `todo!()` stubs.
//!
//! # Ported (self-contained in safe Rust)
//!
//! - The `Htop_Reaction` bit-flag set from `Action.h:21` (pure data).
//! - `State`'s three `bool` fields plus the `host: *mut Machine`
//!   back-pointer (`Action.h:35`). The still-omitted members are the
//!   substrate pointers `mainPanel` (`MainPanel*`), `header` (`Header*`)
//!   and `failedUpdate` (`const char*`), none of which the ported handlers
//!   touch.
//! - `actionQuit` (`Action.c:454`) — `State*` is `ATTR_UNUSED`; the full
//!   behavior is returning the `HTOP_QUIT` constant.
//! - `actionToggleHideMeters` (`Action.c:300`) / `actionTogglePauseUpdate`
//!   (`Action.c:703`) — flip one `State` bool and return a reaction.
//! - **Sort handlers:** `Action_setSortKey` (`Action.c:174`, calls
//!   `ScreenSettings_setSortKey`), and `actionSortByPID`/`actionSortByMemory`/
//!   `actionSortByCPU`/`actionSortByTime` (`Action.c:227-239`) which reach
//!   `st->host->settings`.
//! - **Display toggles reaching `st->host->settings`:**
//!   `actionToggleKernelThreads`/`actionToggleUserlandThreads`
//!   (`Action.c:243/253` — the `Machine_scanTables(st->host)` re-scan maps to
//!   the still-stubbed `Machine_scanTables`, a faithful stub-chain call),
//!   `actionToggleRunningInContainer` (`Action.c:263`),
//!   `actionToggleProgramPath` (`Action.c:271`), `actionToggleMergedCommand`
//!   (`Action.c:279`), `actionToggleTreeView` (`Action.c:287`),
//!   `actionExpandOrCollapseAllBranches` (`Action.c:305`), and
//!   `actionInvertSortOrder` (`Action.c:349`) — the last three also drive the
//!   active table (`Table_expandTree`/`Table_collapseAllBranches`, both ported).
//! - `Action_writeableProcess`/`Action_readableProcess` (`Action.c:181/187`).
//! - `expandCollapse` (`Action.c:148`) / `collapseIntoParent`
//!   (`Action.c:157`) — the two `static` tree helpers that take a bare
//!   `Panel*` (not `State`). They mutate the selected/parent [`Row`](crate::ported::row::Row)'s
//!   `showChildren` via the ported [`Panel`]/[`Row`](crate::ported::row::Row) substrate. The
//!   ported `Panel_get`/`Panel_getSelected` yield only `&dyn Object`, so
//!   the mutating analog indexes `panel.items` and downcasts to `&mut Row`
//!   through the `Any` supertrait — the exact idiom `ColumnsPanel.c`'s
//!   port uses (`columnspanel.rs`), and the safe-Rust analog of the C
//!   `(Row*)` cast.
//! - `tagAllChildren` (`Action.c:137`) — same `panel.items` index +
//!   `Any`-downcast idiom as `expandCollapse`. The C `Row* parent` (a
//!   pointer into `panel`) is modeled as its `panel.items` index so the
//!   recursive walk never needs to alias `&mut Panel`.
//! - `Action_setUserOnly` (`Action.c:127`) — `getpwnam` via the crate's
//!   `libc` dep, mirroring `userstable.rs`'s `getpwuid` FFI idiom.
//!
//! # Stubbed (genuinely blocked; grouped by the missing substrate)
//!
//! - **ncurses / CRT drawing:** `actionHelp`, `addattrstr`, `actionRedraw`
//!   (`clear()` is unported in `crt.rs`). No ported drawing primitives.
//! - **Screen-tab switching needs `st->mainPanel`:**
//!   `setActiveScreen`/`actionNextScreen`/`actionPrevScreen`/
//!   `Action_setScreenTab` call `MainPanel_setFunctionBar(st->mainPanel, …)`,
//!   which the minimal `State` (no `mainPanel`) cannot reach; also blocked on
//!   `Action_follow`.
//! - **Column-sort picker (now ported):** `Action_pickFromVector`
//!   (`Action.c:59`) builds a transient two-panel `ScreenManager` (the picker
//!   `list` + the shared `mainPanel`), runs its modal loop with focus locked to
//!   the picker, and returns the selected index + the reclaimed panel — enabled
//!   now that `ScreenManager_new`/`_add`/`_run` take `*mut` back-pointers. On it,
//!   `actionSetSortColumn` (`Action.c:192`) is ported: it builds the sortable-
//!   column `ListItem` picker (`Process_fields[]` / `DynamicColumn` via
//!   `Hashtable_get`) and applies the pick via `Action_setSortKey`. The single
//!   remaining gap is `beep()` in `Action_pickFromVector`'s `follow` branch
//!   (unported ncurses bell), unreached by the `follow = false` sort path.
//! - **Setup screen (now ported):** `Action_runSetup` (`Action.c:101`) /
//!   `actionSetup` — the `owner = true` setup `ScreenManager` seeded by
//!   `CategoriesPanel_new`, with `CRT_setMouse` / `Header_writeBackToSettings`
//!   write-back, all now-ported substrate.
//! - **`Panel`/`MainPanel` glue (still blocked):**
//!   `changePriority`/`actionHigherPriority`/`actionLowerPriority`
//!   (`MainPanel_foreachRow` callback-type mismatch with `Process_rowChangePriorityBy`
//!   + `beep`), `addUserToVector`/`actionFilterByUser` (`UsersTable_foreach` on
//!   the opaque `usersTable`).
//! - **`actionKill` (`Action.c:524`):** signal delivery is available via
//!   the crate's `nix`/`libc` deps, but the handler reaches `st->mainPanel`
//!   (`Panel_setHeader`/`Panel_draw`/`MainPanel_foreachRow`) which the
//!   minimal `State` does not model, so it stays stubbed on that ground.
//! - **Child screens (each its own unported InfoScreen subclass):**
//!   `actionLsof`, `actionStrace`, `actionShowLocks`, `actionShowEnvScreen`,
//!   `actionShowCommandScreen`, `actionBacktrace`, `actionSetAffinity`,
//!   `actionSetSchedPolicy`.
//! `Action_setBindings` (`Action.c:947`) is now ported: every `actionXxx`
//! handler named in the C binding list shares the
//! [`Htop_Action`] = `fn(&mut State) -> Htop_Reaction` signature (the still
//! genuinely-blocked ones — `actionKill`/`actionLsof`/`actionHelp`/… — keep
//! that signature with a `todo!()` body, a faithful chain-of-stubs), so the
//! dispatch table is filled `keys[code] = Some(actionX)`. The
//! `SCHEDULER_SUPPORT` / `HAVE_BACKTRACE_SCREEN` bindings are gated out to
//! match the darwin-first build.
//!
//! `gen_port_report.py` counts remaining `todo!()` bodies as *stubbed*,
//! not *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)] // `Htop_Reaction` mirrors the C type name verbatim
#![allow(dead_code)]

use crate::ported::categoriespanel::CategoriesPanel_new;
use crate::ported::commandline::{COPYRIGHT, VERSION};
use crate::ported::commandscreen::{CommandScreen_delete, CommandScreen_new};
use crate::ported::crt::{
    CRT_enableDelay, CRT_readKey, CRT_setMouse, ColorElements, ColorScheme, KEY_DOWN, KEY_F,
    KEY_RECLICK, KEY_SHIFT_TAB,
};
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::{Platform_memoryClasses, Platform_numberOfMemoryClasses};
use crate::ported::dynamiccolumn::DynamicColumn;
use crate::ported::envscreen::{EnvScreen_delete, EnvScreen_new};
use crate::ported::functionbar::{FunctionBar_newEnterEsc, Ncurses};
use crate::ported::hashtable::Hashtable_get;
use crate::ported::header::{Header, Header_writeBackToSettings};
use crate::ported::incset::{IncSet_activate, IncSet_filter, IncSet_reset, IncType};
use crate::ported::infoscreen::InfoScreen_run;
use crate::ported::linux::linuxprocess::{Process_fields, LAST_PROCESSFIELD};
#[cfg(not(target_os = "macos"))]
use crate::ported::linux::platform::{Platform_memoryClasses, Platform_numberOfMemoryClasses};
use crate::ported::listitem::{ListItem, ListItem_getRef, ListItem_new};
use crate::ported::machine::{Machine, Machine_scanTables};
use crate::ported::mainpanel::{
    MainPanel, MainPanel_foreachRow, MainPanel_selectedRow, MainPanel_setFunctionBar,
};
use crate::ported::object::{Arg, Object};
use crate::ported::openfilesscreen::{OpenFilesScreen_delete, OpenFilesScreen_new};
use crate::ported::panel::{
    Panel, PanelClass, PanelItem, Panel_add, Panel_draw, Panel_getSelected, Panel_insert,
    Panel_move, Panel_new, Panel_onKey, Panel_resize, Panel_setHeader, Panel_setSelected,
    Panel_setSelectionColor, Panel_size,
};
use crate::ported::process::{
    Process, ProcessField, Process_rowChangePriorityBy, Process_rowSendSignal,
};
use crate::ported::processlocksscreen::{ProcessLocksScreen_delete, ProcessLocksScreen_new};
use crate::ported::row::{Row_getGroupOrParent, Row_isChildOf, Row_toggleTag};
use crate::ported::screenmanager::{
    ScreenManager_add, ScreenManager_delete, ScreenManager_new, ScreenManager_remove,
    ScreenManager_run,
};
use crate::ported::settings::{
    RowField, ScreenSettings_getActiveSortKey, ScreenSettings_invertSortOrder,
    ScreenSettings_setSortKey, Settings, Settings_isReadonly,
};
use crate::ported::signalspanel::{SignalsPanel_new, SIGNALSPANEL_INITSELECTEDSIGNAL};
use crate::ported::table::{Table_collapseAllBranches, Table_expandTree};
use crate::ported::tracescreen::{TraceScreen_delete, TraceScreen_forkTracer, TraceScreen_new};
use crate::ported::userstable::{UsersTable, UsersTable_foreach};
use crate::ported::xutils::String_trim;

/// Port of `#define ROW_DYNAMIC_FIELDS LAST_RESERVED_FIELD` (`RowField.h:53`).
/// `LAST_RESERVED_FIELD == LAST_PROCESSFIELD` (`Process.h:229`), the reserved
/// (non-dynamic) field count; a `RowField` at or above this indexes a
/// runtime-discovered [`DynamicColumn`] instead of `Process_fields[]`.
const ROW_DYNAMIC_FIELDS: i32 = LAST_PROCESSFIELD as i32;

/// Port of `#define SCREEN_TAB_MARGIN_LEFT 2` (`CRT.h:17`). Used by
/// [`Action_setScreenTab`] to skip the left margin before the first tab.
const SCREEN_TAB_MARGIN_LEFT: i32 = 2;
/// Port of `#define SCREEN_TAB_COLUMN_GAP 1` (`CRT.h:18`). Inter-tab gap in
/// [`Action_setScreenTab`]'s hit-test walk.
const SCREEN_TAB_COLUMN_GAP: i32 = 1;

/// Port of the `Htop_Reaction` enum from `Action.h:21`.
///
/// The C enum's members are OR-combined at every `return` site
/// (`return HTOP_RESIZE | HTOP_KEEP_FOLLOWING;`), so it is used as a
/// bit-flag set rather than a discriminant. A C `enum` has type `int`;
/// all defined values are non-negative and fit in a byte, so a `u32`
/// alias reproduces the arithmetic exactly while keeping the OR
/// semantics.
pub type Htop_Reaction = u32;

/// `HTOP_OK = 0x00` — `Action.h:22`.
pub const HTOP_OK: Htop_Reaction = 0x00;
/// `HTOP_REFRESH = 0x01` — `Action.h:23`.
pub const HTOP_REFRESH: Htop_Reaction = 0x01;
/// `HTOP_RECALCULATE = 0x02 | HTOP_REFRESH` — `Action.h:24`.
pub const HTOP_RECALCULATE: Htop_Reaction = 0x02 | HTOP_REFRESH;
/// `HTOP_SAVE_SETTINGS = 0x04` — `Action.h:25`.
pub const HTOP_SAVE_SETTINGS: Htop_Reaction = 0x04;
/// `HTOP_KEEP_FOLLOWING = 0x08` — `Action.h:26`.
pub const HTOP_KEEP_FOLLOWING: Htop_Reaction = 0x08;
/// `HTOP_QUIT = 0x10` — `Action.h:27`.
pub const HTOP_QUIT: Htop_Reaction = 0x10;
/// `HTOP_REDRAW_BAR = 0x20` — `Action.h:28`.
pub const HTOP_REDRAW_BAR: Htop_Reaction = 0x20;
/// `HTOP_UPDATE_PANELHDR = 0x40 | HTOP_REFRESH` — `Action.h:29`.
pub const HTOP_UPDATE_PANELHDR: Htop_Reaction = 0x40 | HTOP_REFRESH;
/// `HTOP_RESIZE = 0x80 | HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR`
/// — `Action.h:30`.
pub const HTOP_RESIZE: Htop_Reaction = 0x80 | HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR;

/// Port of the `typedef Htop_Reaction (*Htop_Action)(State* st)` function
/// pointer from `Action.h:57`. The keypress dispatch table
/// ([`Action_setBindings`]) is an array of these; `MainPanel_eventHandler`
/// invokes `this->keys[ch](this->state)`. C passes `State*` (mutable); the
/// ported analog is `fn(&mut State) -> Htop_Reaction`, so every `actionXxx`
/// handler stored in the table shares this one signature.
pub type Htop_Action = fn(&mut State) -> Htop_Reaction;

/// Model of `State` from `Action.h:35`.
///
/// The three `bool` fields and the `host` back-pointer are modeled — the
/// latter is the `Machine*` the sort/toggle handlers reach for
/// `host->settings` / `host->activeTable`. The `mainPanel`/`header`
/// back-pointers are the `struct MainPanel_*` / `Header*` the C handlers
/// dereference (`IncSet`, `changePriority`, `actionKill`, the child
/// screens, …); they are raw pointers exactly as in C, valid for the
/// lifetime of the main loop that owns them.
pub struct State {
    /// C `Machine* host` — back-pointer to the owning machine. The
    /// host-based handlers read/mutate `host->settings` and
    /// `host->activeTable` through it; a valid non-null `host` is their
    /// precondition (as in C, where `st->host->settings` is dereferenced
    /// unconditionally).
    pub host: *mut Machine,
    /// C `struct MainPanel_* mainPanel` — the process panel the handlers
    /// tag/select/read the current row through.
    pub mainPanel: *mut MainPanel,
    /// C `Header* header` — the meters header (reinit on layout change).
    pub header: *mut Header,
    /// C `const char* failedUpdate` — function-bar diagnostic, or `None`
    /// (C `NULL`) when the last sample succeeded.
    pub failedUpdate: Option<String>,
    pub pauseUpdate: bool,
    pub hideSelection: bool,
    pub hideMeters: bool,
}

/// Port of `Object* Action_pickFromVector(State* st, Panel* list, int x,
/// bool follow)` from `Action.c:59`. Builds a transient two-panel
/// `ScreenManager` — the picker `list` on the left (width `x`) and the shared
/// `mainPanel` filling the rest — runs its modal loop with focus locked to the
/// picker (`allowFocusChange = false`), and returns `(selected index, panel)`.
///
/// The C returns `Panel_getSelected(list)` — a **borrowed** pointer into the
/// non-owned picker, leaving the panel intact so the caller can read it and
/// (e.g. `actionSetSchedPolicy`) re-use it across picks. The port models that
/// non-destructively: it returns the selected item's index (or `None`) together
/// with the reclaimed picker `Box`, so the caller reads `panel.items[idx]` and
/// keeps ownership of the panel. The `ScreenManager` back-pointers are raw
/// pointers ([`ScreenManager_new`] takes `*mut Header`/`*mut Machine`/`*mut
/// State`). Two ownership adaptations of the C `owner = false` manager remain:
///
/// - `list` is taken as an owned `Box<dyn PanelClass>` (matching the manager's
///   `Vec<Box<dyn PanelClass>>` element) for the modal run, then reclaimed with
///   [`ScreenManager_remove`] and returned to the caller (which frees it — the
///   C `Object_delete(sortPanel)`), rather than being consumed here.
/// - `mainPanel` is the caller-owned `*mut MainPanel` shared with the main loop's
///   `ScreenManager` (which owns the real `Box<MainPanel>`; the address is
///   move-stable). It is re-boxed via [`Box::from_raw`], added for display, then
///   [`ScreenManager_remove`]d and [`Box::into_raw`]-leaked so the transient
///   manager never drops it — the faithful analog of C's `owner = false` (the
///   manager frees neither panel). The `Vec`-owner [`ScreenManager_delete`] then
///   drops an empty manager.
///
/// The C pointer identity `panelFocus == list` is the picker's fixed panel
/// index (`0`); `ch == 13` is Enter. `COLS`/`LINES` come from
/// `Ncurses::cols()`/`Ncurses::lines()`.
///
/// # Gap: `beep()`
///
/// In the `follow` branch, C `beep()`s when the mainPanel's selection changed
/// during the modal and returns `NULL`. The ncurses audible bell has no facade
/// in `crt.rs`, so that mismatch path returns `None` without the bell — a
/// documented cosmetic gap; the control flow (return `None`) is otherwise
/// faithful. `actionSetSortColumn` passes `follow = false`, so it never reaches
/// this branch.
pub fn Action_pickFromVector(
    st: &mut State,
    list: Box<dyn PanelClass>,
    x: i32,
    follow: bool,
) -> (Option<usize>, Box<dyn PanelClass>) {
    // C: MainPanel* mainPanel = st->mainPanel; Header* header = st->header;
    //    Machine* host = st->host;
    let mainPanel = st.mainPanel;
    let header = st.header;
    let host = st.host;
    let st_ptr: *mut State = st;

    // C: int y = ((Panel*)mainPanel)->y;
    // SAFETY: mainPanel is the caller-owned MainPanel* (main loop precondition).
    let y = unsafe { (*mainPanel).super_.y };

    // C: ScreenManager* scr = ScreenManager_new(header, host, st, false);
    //    scr->allowFocusChange = false;
    let mut scr = ScreenManager_new(header, host, st_ptr);
    scr.allowFocusChange = false;

    // C: ScreenManager_add(scr, list, x);
    let mut list = list;
    ScreenManager_add(&mut scr, list, x);
    // C: ScreenManager_add(scr, (Panel*)mainPanel, -1);
    // Re-box the shared mainPanel WITHOUT taking ownership (C owner = false):
    // SAFETY: mainPanel points into the main loop's Box<MainPanel> (allocated by
    // Box, move-stable). Reclaimed and Box::into_raw-leaked below, so this
    // transient box never drops the MainPanel.
    let mp_box: Box<MainPanel> = unsafe { Box::from_raw(mainPanel) };
    ScreenManager_add(&mut scr, mp_box, -1);

    // C: Panel* panelFocus; int ch; bool unfollow = false;
    let mut panelFocus: usize = 0;
    let mut ch: i32 = 0;
    let mut unfollow = false;
    // C: int row = follow ? MainPanel_selectedRow(mainPanel) : -1;
    // SAFETY: mainPanel valid (main loop precondition); read-only here.
    let row = if follow {
        MainPanel_selectedRow(unsafe { &*mainPanel })
    } else {
        -1
    };
    // C: if (follow && host->activeTable->following == -1) { ...; unfollow = true; }
    if follow {
        // SAFETY: host valid; activeTable is the non-null back-pointer.
        let at = unsafe {
            (*host)
                .activeTable
                .expect("Action_pickFromVector: host->activeTable is NULL")
        };
        if unsafe { (*at).following } == -1 {
            unsafe {
                (*at).following = row;
            }
            unfollow = true;
        }
    }

    // C: ScreenManager_run(scr, &panelFocus, &ch, NULL);
    ScreenManager_run(&mut scr, Some(&mut panelFocus), Some(&mut ch), None);

    // C: if (unfollow) host->activeTable->following = -1;
    if unfollow {
        let at = unsafe {
            (*host)
                .activeTable
                .expect("Action_pickFromVector: host->activeTable is NULL")
        };
        unsafe {
            (*at).following = -1;
        }
    }

    // C: ScreenManager_delete(scr);  (owner = false — frees neither panel)
    // Reclaim both panels first so the Vec-owner drop does not free them.
    // Remove index 1 (mainPanel) before index 0 (list) — removing 0 first would
    // shift the mainPanel down to index 0.
    let mp_reclaimed = ScreenManager_remove(&mut scr, 1);
    // Leak the transient MainPanel box WITHOUT dropping: the main loop's
    // ScreenManager still owns the real Box<MainPanel> (C owner = false).
    let _leaked: *mut dyn PanelClass = Box::into_raw(mp_reclaimed);
    list = ScreenManager_remove(&mut scr, 0);
    ScreenManager_delete(scr);

    // C: Panel_move((Panel*)mainPanel, 0, y);
    //    Panel_resize((Panel*)mainPanel, COLS, LINES - y - 1);
    // SAFETY: mainPanel valid; the main loop is suspended, so no live &mut aliases
    // it (matching C, which writes through the shared pointer here).
    Panel_move(unsafe { &mut (*mainPanel).super_ }, 0, y);
    Panel_resize(
        unsafe { &mut (*mainPanel).super_ },
        Ncurses::cols(),
        Ncurses::lines() - y - 1,
    );

    // C: if (panelFocus == list && ch == 13) { ... }  (picker is panel index 0)
    let selected_idx: Option<usize> = if panelFocus == 0 && ch == 13 {
        let return_selection = if follow {
            // C: const Row* selected = (const Row*)Panel_getSelected((Panel*)mainPanel);
            //    if (selected && selected->id == row) return Panel_getSelected(list);
            // The mainPanel's items are concrete platform `Process` objects, so
            // reach the embedded `Row` through the `as_row()` vtable accessor.
            match Panel_getSelected(unsafe { &(*mainPanel).super_ }) {
                Some(o) => o.as_row().is_some_and(|r| r.id == row),
                // C beep()s here on mismatch; the bell is unported (see the fn
                // docs), so the mismatch path returns `None`.
                None => false,
            }
        } else {
            // C: else return Panel_getSelected(list);
            true
        };
        if return_selection {
            let panel = list.as_panel();
            let sel = panel.selected;
            if sel >= 0 && (sel as usize) < panel.items.len() {
                Some(sel as usize)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // C returns `Panel_getSelected(list)` — a BORROWED pointer into the
    // non-owned picker, leaving the panel (and item) intact. The port returns
    // the selected index plus the reclaimed panel `Box` instead of destructively
    // moving the item out, so the caller reads the item non-destructively and
    // may re-use the panel (the `actionSetSchedPolicy` reset-on-fork re-pick loop
    // depends on this).
    (selected_idx, list)
}

/// Port of `static void Action_runSetup(State* st)` from `Action.c:101`.
/// Builds the setup `ScreenManager` via [`ScreenManager_new`], seeds it with the
/// config-category panel tree ([`CategoriesPanel_new`], which self-registers
/// into `scr`), runs the modal loop, then — if the settings changed — re-applies
/// the mouse mode ([`CRT_setMouse`]) and writes the header layout back
/// ([`Header_writeBackToSettings`]).
///
/// The C `ScreenManager_new(st->header, st->host, st, true)` `owner = true` maps
/// to the `Vec<Box<dyn PanelClass>>` owner default (the manager drops its panels
/// on [`ScreenManager_delete`]), so no panel re-boxing is needed here (unlike
/// [`Action_pickFromVector`], which shares the caller-owned `mainPanel`). C
/// `Header_writeBackToSettings(const Header*)` reaches settings internally; the
/// ported signature takes `settings` explicitly, so `host->settings` is threaded
/// in. `settings->changed`/`enableMouse` are read before the mutable settings
/// borrow so they do not alias.
pub fn Action_runSetup(st: &mut State) {
    // C: const Settings* settings = st->host->settings;  (read per use below)
    let host = st.host;
    let header = st.header;
    let st_ptr: *mut State = st;

    // C: ScreenManager* scr = ScreenManager_new(st->header, st->host, st, true);
    let mut scr = ScreenManager_new(header, host, st_ptr);
    // C: CategoriesPanel_new(scr, st->header, st->host);  (self-registers into scr)
    CategoriesPanel_new(&mut scr, header, host);
    // C: ScreenManager_run(scr, NULL, NULL, "Setup");
    ScreenManager_run(&mut scr, None, None, Some("Setup"));
    // C: ScreenManager_delete(scr);
    ScreenManager_delete(scr);

    // C: if (settings->changed) { CRT_setMouse(...); Header_writeBackToSettings(...); }
    // SAFETY: host valid (C precondition); settings read immutably here.
    let changed = unsafe {
        (*host)
            .settings
            .as_ref()
            .expect("Action_runSetup: host->settings is NULL")
            .changed
    };
    if changed {
        // C: CRT_setMouse(settings->enableMouse);
        let enableMouse = unsafe { (*host).settings.as_ref().unwrap().enableMouse };
        CRT_setMouse(enableMouse);
        // C: Header_writeBackToSettings(st->header);
        // SAFETY: header/settings are distinct allocations (header owned apart
        // from host->settings), so &Header and &mut Settings do not alias.
        let settings = unsafe { (*host).settings.as_mut().unwrap() };
        Header_writeBackToSettings(unsafe { &*header }, settings);
    }
}

/// TODO: port of `static bool changePriority(MainPanel* panel, int delta)` from
/// `Action.c:113`. Applies `Process_rowChangePriorityBy` to every selected/tagged
/// row via `MainPanel_foreachRow`, beeps on partial failure, and reports
/// whether any row was tagged. Blocked on a callback-type mismatch: the ported
/// [`MainPanel_foreachRowFn`](crate::ported::mainpanel::MainPanel_foreachRowFn)
/// is `fn(&mut Row, &Arg) -> bool`, but `process.rs` ports
/// `Process_rowChangePriorityBy` as `fn(&mut dyn Object, Arg) -> bool` — the two
/// signatures are incompatible, and `beep()` is unported. Reconciling the
/// `foreachRow` callback shape belongs in `mainpanel.rs`/`process.rs`.
pub fn changePriority(panel: &mut MainPanel, delta: i32) -> bool {
    // C: bool ok = MainPanel_foreachRow(panel, Process_rowChangePriorityBy,
    //              (Arg){.i = delta}, &anyTagged); if (!ok) beep();
    let mut anyTagged = false;
    let ok = MainPanel_foreachRow(
        panel,
        Process_rowChangePriorityBy,
        Arg::I(delta),
        Some(&mut anyTagged),
    );
    if !ok {
        let mut out = std::io::stdout().lock();
        Ncurses::beep(&mut out);
    }
    anyTagged
}

/// Port of `static void addUserToVector(ht_key_t key, void* userCast,
/// void* panelCast)` from `Action.c:121`. Appends a `ListItem` carrying the
/// user name (`user`) and its uid (`key`) to `panel`. The C `void*` casts are
/// the `UsersTable_foreach` callback ABI (`ht_key_t key`, the `char*` value,
/// the `Panel*` accumulator); the ported analog takes those already-resolved
/// types directly, since the (unported) `UsersTable_foreach` has no
/// `void*`-callback consumer here. `ListItem_new` returns an owned `ListItem`
/// (not a pointer), boxed into the `Box<dyn Object>` the ported
/// [`Panel_add`] expects.
pub fn addUserToVector(key: i32, user: &str, panel: &mut Panel) {
    Panel_add(panel, Box::new(ListItem_new(user, key)));
}

/// Port of `bool Action_setUserOnly(const char* userName, uid_t* userId)`
/// from `Action.c:127`. Resolves `userName` to its uid via `getpwnam`
/// (the same `unsafe { libc::* }` idiom `userstable.rs` uses for
/// `getpwuid`): on a hit it writes `pw_uid` and returns `true`; on a NULL
/// lookup it writes `(uid_t)-1` (`u32::MAX`, matching `process.rs`'s
/// `(uid_t)-1` idiom) and returns `false`. The C `const char*` is taken
/// as `&str` and marshalled through a `CString`; an interior NUL — which
/// a C NUL-terminated string could never carry — is treated as a failed
/// lookup.
pub fn Action_setUserOnly(userName: &str, userId: &mut libc::uid_t) -> bool {
    let c_userName = match std::ffi::CString::new(userName) {
        Ok(s) => s,
        Err(_) => {
            *userId = libc::uid_t::MAX;
            return false;
        }
    };
    // C `const struct passwd* user = getpwnam(userName);`
    let user = unsafe { libc::getpwnam(c_userName.as_ptr()) };
    if !user.is_null() {
        // C `*userId = user->pw_uid; return true;`
        *userId = unsafe { (*user).pw_uid };
        return true;
    }
    // C `*userId = (uid_t)-1; return false;`
    *userId = libc::uid_t::MAX;
    false
}

/// Port of `static void tagAllChildren(Panel* panel, Row* parent)` from
/// `Action.c:137`. Sets the parent row's `tag`, then recursively tags
/// every untagged row that [`Row_isChildOf`] the parent's `id`. In C
/// `parent` is a `Row*` aliasing an element of `panel`; safe Rust cannot
/// hold that `&mut Row` while it also mutably walks `panel`, so — exactly
/// as `expandCollapse`/`collapseIntoParent` model `Panel_getSelected`'s
/// `(Row*)` — the parent is identified by its `panel.items` index and the
/// two `(Row*)` upcasts go through `as_row`/`as_row_mut` (panel items are
/// platform `Process` objects, not bare `Row`s), keeping the borrows
/// non-overlapping while preserving the C recursion order verbatim.
pub fn tagAllChildren(panel: &mut Panel, parent_idx: i32) {
    // C `parent->tag = true; int parent_id = parent->id;`
    let parent_id = {
        let obj: &mut dyn Object = panel.items[parent_idx as usize].object_mut();
        // mainPanel items are platform `Process` objects; reach the embedded
        // `Row` via `as_row_mut()`, not an exact-type `Any` downcast.
        let parent = obj
            .as_row_mut()
            .expect("tagAllChildren operates on the mainPanel, whose items are process rows");
        parent.tag = true;
        parent.id
    };

    let size = Panel_size(panel);
    for i in 0..size {
        // C `Row* row = Panel_get(panel, i);
        //    if (!row->tag && Row_isChildOf(row, parent_id))`
        let recurse = {
            let obj: &dyn Object = panel.items[i as usize].object();
            let row = obj
                .as_row()
                .expect("tagAllChildren operates on the mainPanel, whose items are process rows");
            !row.tag && Row_isChildOf(row, parent_id)
        };
        if recurse {
            tagAllChildren(panel, i);
        }
    }
}

/// Port of `static bool expandCollapse(Panel* panel)` from `Action.c:148`.
/// Flips the selected row's `showChildren` flag and returns `true`;
/// returns `false` when the panel is empty (the C `if (!row) return false`,
/// since ported `Panel_getSelected` yields `NULL`/`None` only for an empty
/// list). The C `(Row*) Panel_getSelected(panel)` casts the base
/// `Object*`; the ported `Panel_getSelected` returns an immutable
/// `&dyn Object`, so the mutating analog indexes `panel.items` at the
/// selected position and reaches the embedded `&mut Row` via `as_row_mut()`
/// (panel items are platform `Process` objects, not bare `Row`s).
pub fn expandCollapse(panel: &mut Panel) -> bool {
    if panel.items.is_empty() {
        return false;
    }

    let idx = panel.selected as usize;
    let obj: &mut dyn Object = panel.items[idx].object_mut();
    let row = obj
        .as_row_mut()
        .expect("expandCollapse operates on the mainPanel, whose items are process rows");
    row.showChildren = !row.showChildren;
    true
}

/// Port of `static bool collapseIntoParent(Panel* panel)` from
/// `Action.c:157`. Reads the selected row's group-or-parent id via
/// [`Row_getGroupOrParent`], then scans the panel for the row whose `id`
/// matches: on a hit it clears that row's `showChildren`, moves the
/// selection there via [`Panel_setSelected`], and returns `true`;
/// otherwise `false` (also `false` when the panel is empty — the C
/// `if (!r) return false`). The two `(Row*)` upcasts go through
/// `as_row`/`as_row_mut` on `panel.items`; the read of the selected row
/// (immutable) is scoped before the mutating scan so the borrows never overlap.
pub fn collapseIntoParent(panel: &mut Panel) -> bool {
    if panel.items.is_empty() {
        return false;
    }

    let parent_id = {
        let obj: &dyn Object = panel.items[panel.selected as usize].object();
        let r = obj
            .as_row()
            .expect("collapseIntoParent operates on the mainPanel, whose items are process rows");
        Row_getGroupOrParent(r)
    };

    let size = Panel_size(panel);
    for i in 0..size {
        let id = {
            let obj: &dyn Object = panel.items[i as usize].object();
            obj.as_row()
                .expect(
                    "collapseIntoParent operates on the mainPanel, whose items are process rows",
                )
                .id
        };
        if id == parent_id {
            let obj: &mut dyn Object = panel.items[i as usize].object_mut();
            obj.as_row_mut()
                .expect(
                    "collapseIntoParent operates on the mainPanel, whose items are process rows",
                )
                .showChildren = false;
            Panel_setSelected(panel, i);
            return true;
        }
    }
    false
}

/// Port of `Htop_Reaction Action_setSortKey(Settings* settings,
/// ProcessField sortKey)` from `Action.c:174`. Delegates to
/// [`ScreenSettings_setSortKey`] on the active screen (`settings->ss`,
/// modeled as `screens[ssIndex]`) and returns the sort-changed reaction.
/// The C `(RowField) sortKey` cast is the identity here — the caller passes
/// a [`RowField`] already.
pub fn Action_setSortKey(settings: &mut Settings, sortKey: RowField) -> Htop_Reaction {
    ScreenSettings_setSortKey(&mut settings.screens[settings.ssIndex as usize], sortKey);
    HTOP_REFRESH | HTOP_SAVE_SETTINGS | HTOP_UPDATE_PANELHDR | HTOP_KEEP_FOLLOWING
}

/// Port of `static bool Action_writeableProcess(State* st)` from
/// `Action.c:181`. A process is writeable unless htop is read-only
/// ([`Settings_isReadonly`]) or the active screen is a dynamic screen
/// (`settings->ss->dynamic`, modeled as `Option<String>` — truthy when
/// `Some`). Reads through `st->host->settings`; a valid non-null `host` is
/// the precondition (as in C).
pub fn Action_writeableProcess(st: &State) -> bool {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_ref()
            .expect("Action_writeableProcess: host->settings is NULL")
    };
    let readonly = Settings_isReadonly()
        || settings.screens[settings.ssIndex as usize]
            .dynamic
            .is_some();
    !readonly
}

/// Port of `static bool Action_readableProcess(State* st)` from
/// `Action.c:187`. A process is readable unless the active screen is a
/// dynamic screen (`settings->ss->dynamic`). Reads through
/// `st->host->settings` (valid non-null `host` is the precondition).
pub fn Action_readableProcess(st: &State) -> bool {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_ref()
            .expect("Action_readableProcess: host->settings is NULL")
    };
    settings.screens[settings.ssIndex as usize]
        .dynamic
        .is_none()
}

/// Port of `static Htop_Reaction actionSetSortColumn(State* st)` from
/// `Action.c:192`. Builds a `ListItem` picker of the active screen's sortable
/// columns, runs it through [`Action_pickFromVector`], and applies the chosen
/// key via [`Action_setSortKey`].
///
/// Each of the active screen's `fields` (`settings->ss->fields`, modeled as
/// `screens[ssIndex].fields`, walked to the C `0` terminator) yields a
/// [`ListItem`]: a dynamic field (`>= ROW_DYNAMIC_FIELDS`) resolves its label
/// through [`Hashtable_get`] on `settings->dynamicColumns` (skipped on a miss —
/// the C `if (!column) continue;`), using `caption` or falling back to `name`;
/// a reserved field uses [`String_trim`]`(Process_fields[f].name)`. The item
/// matching [`ScreenSettings_getActiveSortKey`] is pre-selected at loop index
/// `i` (faithful to the C `Panel_setSelected(sortPanel, i)`, which indexes by
/// the field position). `Class(ListItem)` / `true` (owner) have no analog in the
/// reduced [`Panel_new`]; the picker's owned `ListItem`s free with it.
///
/// `Object_delete(sortPanel)` maps to dropping the picker `Box`
/// [`Action_pickFromVector`] returns after the modal run (owned by the modal for
/// its duration, then handed back). `host->activeTable->needsSort` is set through the
/// `*mut Table` back-pointer.
pub fn actionSetSortColumn(st: &mut State) -> Htop_Reaction {
    // C: Htop_Reaction reaction = HTOP_OK;
    let mut reaction: Htop_Reaction = HTOP_OK;
    // C: Panel* sortPanel = Panel_new(0, 0, 0, 0, Class(ListItem), true,
    //                                 FunctionBar_newEnterEsc("Sort   ", "Cancel "));
    let mut sortPanel = Panel_new(
        0,
        0,
        0,
        0,
        Some(FunctionBar_newEnterEsc("Sort   ", "Cancel ")),
    );
    // C: Panel_setHeader(sortPanel, "Sort by");
    Panel_setHeader(&mut sortPanel, "Sort by");

    // C: Machine* host = st->host; Settings* settings = host->settings;
    let host = st.host;
    // C: const RowField* fields = settings->ss->fields;
    //    Hashtable* dynamicColumns = settings->dynamicColumns;
    // (snapshot the field list, dynamicColumns pointer, and the active sort key
    // up front so the picker-building mutations do not alias the settings borrow.)
    let (fields, dynamicColumns, activeSortKey) = unsafe {
        let settings = (*host)
            .settings
            .as_ref()
            .expect("actionSetSortColumn: host->settings is NULL");
        let ss = &settings.screens[settings.ssIndex as usize];
        (
            ss.fields.clone(),
            settings.dynamicColumns,
            ScreenSettings_getActiveSortKey(ss),
        )
    };

    // C: for (int i = 0; fields[i]; i++) { ... }
    for i in 0..fields.len() {
        let field = fields[i];
        if field == 0 {
            break; // the C `fields[i]` 0-terminator
        }
        // C: char* name = NULL;
        let name: String;
        if field >= ROW_DYNAMIC_FIELDS {
            // C: DynamicColumn* column = Hashtable_get(dynamicColumns, fields[i]);
            //    if (!column) continue;
            let column = match dynamicColumns {
                // SAFETY: dynamicColumns is the Machine-owned Hashtable pointer
                // (settings->dynamicColumns), valid for the run.
                Some(dc) => Hashtable_get(unsafe { &*dc }, field as u32).and_then(|o| {
                    let any: &dyn core::any::Any = o;
                    any.downcast_ref::<DynamicColumn>()
                }),
                None => None,
            };
            let column = match column {
                Some(c) => c,
                None => continue,
            };
            // C: name = xStrdup(column->caption ? column->caption : column->name);
            name = column
                .caption
                .clone()
                .unwrap_or_else(|| column.name.clone());
        } else {
            // C: name = String_trim(Process_fields[fields[i]].name);
            name = String_trim(Process_fields[field as usize].name);
        }
        // C: Panel_add(sortPanel, (Object*) ListItem_new(name, fields[i]));
        Panel_add(&mut sortPanel, Box::new(ListItem_new(&name, field)));
        // C: if (fields[i] == ScreenSettings_getActiveSortKey(settings->ss))
        //       Panel_setSelected(sortPanel, i);
        if field == activeSortKey {
            Panel_setSelected(&mut sortPanel, i as i32);
        }
        // C: free(name);  — the owned `name` String drops at end of iteration.
    }

    // C: const ListItem* field = (const ListItem*) Action_pickFromVector(st, sortPanel, 14, false);
    let (picked, panel) = Action_pickFromVector(st, Box::new(sortPanel), 14, false);
    // C: if (field) reaction |= Action_setSortKey(settings, field->key);
    if let Some(i) = picked {
        let any: &dyn core::any::Any = panel.as_panel().items[i].object();
        if let Some(item) = any.downcast_ref::<ListItem>() {
            // SAFETY: host valid (C precondition); settings borrowed mutably here
            // only (no other live settings borrow).
            let settings = unsafe {
                (*host)
                    .settings
                    .as_mut()
                    .expect("actionSetSortColumn: host->settings is NULL")
            };
            reaction |= Action_setSortKey(settings, item.key);
        }
    }
    // C: Object_delete(sortPanel);  — the `panel` Box returned by
    // Action_pickFromVector drops at the end of this scope.

    // C: host->activeTable->needsSort = true;
    // SAFETY: host valid; activeTable is the non-null back-pointer.
    let at = unsafe {
        (*host)
            .activeTable
            .expect("actionSetSortColumn: host->activeTable is NULL")
    };
    unsafe {
        (*at).needsSort = true;
    }

    // C: return reaction | HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR;
    reaction | HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR
}

/// Port of `static Htop_Reaction actionSortByPID(State* st)` from
/// `Action.c:227`: `Action_setSortKey(st->host->settings, PID)`.
pub fn actionSortByPID(st: &mut State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionSortByPID: host->settings is NULL")
    };
    Action_setSortKey(settings, ProcessField::PID as RowField)
}

/// Port of `static Htop_Reaction actionSortByMemory(State* st)` from
/// `Action.c:231`: `Action_setSortKey(st->host->settings, PERCENT_MEM)`.
pub fn actionSortByMemory(st: &mut State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionSortByMemory: host->settings is NULL")
    };
    Action_setSortKey(settings, ProcessField::PERCENT_MEM as RowField)
}

/// Port of `static Htop_Reaction actionSortByCPU(State* st)` from
/// `Action.c:235`: `Action_setSortKey(st->host->settings, PERCENT_CPU)`.
pub fn actionSortByCPU(st: &mut State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionSortByCPU: host->settings is NULL")
    };
    Action_setSortKey(settings, ProcessField::PERCENT_CPU as RowField)
}

/// Port of `static Htop_Reaction actionSortByTime(State* st)` from
/// `Action.c:239`: `Action_setSortKey(st->host->settings, TIME)`.
pub fn actionSortByTime(st: &mut State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionSortByTime: host->settings is NULL")
    };
    Action_setSortKey(settings, ProcessField::TIME as RowField)
}

/// Port of `static Htop_Reaction actionToggleKernelThreads(State* st)` from
/// `Action.c:243`. Flips `settings->hideKernelThreads`, bumps
/// `settings->lastUpdate`, then re-scans the tables so the display does not
/// lag a tick behind the toggle.
///
/// The `Machine_scanTables(st->host)` call maps to the still-stubbed
/// [`Machine_scanTables`] (the platform scan machinery); the wiring is
/// faithful — the call panics through that `todo!()` until the scan layer
/// lands, matching the `Process_compare`/`Process_compareByParent`
/// stub-chain precedent.
pub fn actionToggleKernelThreads(st: &mut State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionToggleKernelThreads: host->settings is NULL")
    };
    settings.hideKernelThreads = !settings.hideKernelThreads;
    settings.lastUpdate += 1;

    // SAFETY: st->host is a valid, non-null Machine* (C precondition).
    Machine_scanTables(unsafe { &mut *st.host }); // C: Machine_scanTables(st->host)

    HTOP_RECALCULATE | HTOP_SAVE_SETTINGS | HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionToggleUserlandThreads(State* st)`
/// from `Action.c:253`. Flips `settings->hideUserlandThreads`, bumps
/// `settings->lastUpdate`, then re-scans the tables. The
/// `Machine_scanTables(st->host)` call maps to the still-stubbed
/// [`Machine_scanTables`] (see [`actionToggleKernelThreads`]).
pub fn actionToggleUserlandThreads(st: &mut State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionToggleUserlandThreads: host->settings is NULL")
    };
    settings.hideUserlandThreads = !settings.hideUserlandThreads;
    settings.lastUpdate += 1;

    // SAFETY: st->host is a valid, non-null Machine* (C precondition).
    Machine_scanTables(unsafe { &mut *st.host }); // C: Machine_scanTables(st->host)

    HTOP_RECALCULATE | HTOP_SAVE_SETTINGS | HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionToggleRunningInContainer(State* st)`
/// from `Action.c:263`. Flips `settings->hideRunningInContainer` and bumps
/// `settings->lastUpdate`.
pub fn actionToggleRunningInContainer(st: &mut State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionToggleRunningInContainer: host->settings is NULL")
    };
    settings.hideRunningInContainer = !settings.hideRunningInContainer;
    settings.lastUpdate += 1;

    HTOP_RECALCULATE | HTOP_SAVE_SETTINGS | HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionToggleProgramPath(State* st)` from
/// `Action.c:271`. Flips `settings->showProgramPath` and bumps
/// `settings->lastUpdate`.
pub fn actionToggleProgramPath(st: &mut State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionToggleProgramPath: host->settings is NULL")
    };
    settings.showProgramPath = !settings.showProgramPath;
    settings.lastUpdate += 1;

    HTOP_REFRESH | HTOP_SAVE_SETTINGS | HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionToggleMergedCommand(State* st)` from
/// `Action.c:279`. Flips `settings->showMergedCommand` and bumps
/// `settings->lastUpdate`.
pub fn actionToggleMergedCommand(st: &mut State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionToggleMergedCommand: host->settings is NULL")
    };
    settings.showMergedCommand = !settings.showMergedCommand;
    settings.lastUpdate += 1;

    HTOP_REFRESH | HTOP_SAVE_SETTINGS | HTOP_KEEP_FOLLOWING | HTOP_UPDATE_PANELHDR
}

/// Port of `static Htop_Reaction actionToggleTreeView(State* st)` from
/// `Action.c:287`. Flips the active screen's `treeView`; when the tree was
/// not fully collapsed, expands it ([`Table_expandTree`]); marks the active
/// table for re-sort. `settings->ss` is modeled as `screens[ssIndex]`, and
/// `host->activeTable` is the `*mut Table` back-pointer (its non-null
/// validity is the precondition, as in C).
pub fn actionToggleTreeView(st: &mut State) -> Htop_Reaction {
    let host = st.host;
    unsafe {
        let ssidx = (*host)
            .settings
            .as_ref()
            .expect("actionToggleTreeView: host->settings is NULL")
            .ssIndex as usize;
        {
            let ss = &mut (*host).settings.as_mut().unwrap().screens[ssidx];
            ss.treeView = !ss.treeView;
        }
        let all_collapsed = (*host).settings.as_ref().unwrap().screens[ssidx].allBranchesCollapsed;

        let at = (*host)
            .activeTable
            .expect("actionToggleTreeView: host->activeTable is NULL");
        if !all_collapsed {
            Table_expandTree(&mut *at);
        }
        (*at).needsSort = true;
    }

    HTOP_REFRESH | HTOP_SAVE_SETTINGS | HTOP_KEEP_FOLLOWING | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR
}

/// Port of `static Htop_Reaction actionToggleHideMeters(State* st)` from
/// `Action.c:300`. Flips the `State.hideMeters` flag and returns the
/// resize reaction. The C reads/writes only `st->hideMeters`, so the
/// minimal `State` model suffices; the returned value is the verbatim
/// `HTOP_RESIZE | HTOP_KEEP_FOLLOWING` bit-or.
pub fn actionToggleHideMeters(st: &mut State) -> Htop_Reaction {
    st.hideMeters = !st.hideMeters;
    HTOP_RESIZE | HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionExpandOrCollapseAllBranches(State*
/// st)` from `Action.c:305`. A no-op outside tree view; otherwise flips the
/// active screen's `allBranchesCollapsed` and either collapses
/// ([`Table_collapseAllBranches`]) or expands ([`Table_expandTree`]) the
/// active table accordingly. `settings->ss` is `screens[ssIndex]`;
/// `host->activeTable` is the `*mut Table` back-pointer.
pub fn actionExpandOrCollapseAllBranches(st: &mut State) -> Htop_Reaction {
    let host = st.host;
    unsafe {
        let ssidx = (*host)
            .settings
            .as_ref()
            .expect("actionExpandOrCollapseAllBranches: host->settings is NULL")
            .ssIndex as usize;
        if !(*host).settings.as_ref().unwrap().screens[ssidx].treeView {
            return HTOP_OK;
        }
        let collapsed = {
            let ss = &mut (*host).settings.as_mut().unwrap().screens[ssidx];
            ss.allBranchesCollapsed = !ss.allBranchesCollapsed;
            ss.allBranchesCollapsed
        };
        let at = (*host)
            .activeTable
            .expect("actionExpandOrCollapseAllBranches: host->activeTable is NULL");
        if collapsed {
            Table_collapseAllBranches(&mut *at);
        } else {
            Table_expandTree(&mut *at);
        }
    }
    HTOP_REFRESH | HTOP_SAVE_SETTINGS
}

/// Port of `static Htop_Reaction actionIncFilter(State* st)` from
/// `Action.c:319`. Activates the incremental filter on the main panel and
/// copies the resulting filter text into `host->activeTable->incFilter`.
///
/// C aliases one `IncSet*` as `st->mainPanel->inc` and passes the same panel
/// as `(Panel*)st->mainPanel`; the split-field borrow `&mut mp.inc` /
/// `&mut mp.super_` reproduces that (disjoint fields of the one struct).
/// `host->activeTable->incFilter = IncSet_filter(inc)` stores a pointer into
/// `inc`'s editor buffer in C; the owned model copies the `&str` into the
/// `Option<String>` `incFilter` slot (the module's clone-for-shared-pointer
/// convention). `host->activeTable` non-null is the precondition (as in C).
pub fn actionIncFilter(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->mainPanel is a valid, non-null MainPanel* for the lifetime
    // of the main loop that owns it (C precondition: st->mainPanel->inc is
    // dereferenced unconditionally).
    let mp = unsafe { &mut *st.mainPanel };
    IncSet_activate(&mut mp.inc, IncType::INC_FILTER, &mut mp.super_);
    let filter = IncSet_filter(&mp.inc).map(|s| s.to_string());
    // SAFETY: st->host is a valid, non-null Machine* (C precondition:
    // st->host->activeTable is dereferenced unconditionally).
    let host = unsafe { &mut *st.host };
    let at = host
        .activeTable
        .expect("actionIncFilter: host->activeTable is NULL");
    unsafe {
        (*at).incFilter = filter;
    }
    HTOP_REFRESH | HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionIncSearch(State* st)` from
/// `Action.c:327`. Resets and activates the incremental search on the main
/// panel. As in [`actionIncFilter`], `st->mainPanel->inc` and the panel
/// `(Panel*)st->mainPanel` are the same struct's disjoint `inc` / `super_`
/// fields.
pub fn actionIncSearch(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition:
    // st->mainPanel->inc is dereferenced unconditionally).
    let mp = unsafe { &mut *st.mainPanel };
    IncSet_reset(&mut mp.inc, IncType::INC_SEARCH);
    IncSet_activate(&mut mp.inc, IncType::INC_SEARCH, &mut mp.super_);
    HTOP_REFRESH | HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionHigherPriority(State* st)` from
/// `Action.c:333`. A no-op unless the process is writeable
/// ([`Action_writeableProcess`]); otherwise raises priority by one nice step
/// via [`changePriority`] and refreshes if anything changed. `changePriority`
/// remains a `todo!()` (callback-type mismatch); this handler calls it by name
/// (faithful stub-chain, matching the `Machine_scanTables` precedent).
pub fn actionHigherPriority(st: &mut State) -> Htop_Reaction {
    if !Action_writeableProcess(st) {
        return HTOP_OK;
    }

    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition).
    let changed = changePriority(unsafe { &mut *st.mainPanel }, -1);
    if changed {
        HTOP_REFRESH
    } else {
        HTOP_OK
    }
}

/// Port of `static Htop_Reaction actionLowerPriority(State* st)` from
/// `Action.c:341`. As [`actionHigherPriority`] but lowers priority by one nice
/// step (`delta = 1`).
pub fn actionLowerPriority(st: &mut State) -> Htop_Reaction {
    if !Action_writeableProcess(st) {
        return HTOP_OK;
    }

    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition).
    let changed = changePriority(unsafe { &mut *st.mainPanel }, 1);
    if changed {
        HTOP_REFRESH
    } else {
        HTOP_OK
    }
}

/// Port of `static Htop_Reaction actionInvertSortOrder(State* st)` from
/// `Action.c:349`. Inverts the active screen's sort direction
/// ([`ScreenSettings_invertSortOrder`]) and marks the active table for
/// re-sort. `settings->ss` is `screens[ssIndex]`; `host->activeTable` is
/// the `*mut Table` back-pointer.
pub fn actionInvertSortOrder(st: &mut State) -> Htop_Reaction {
    let host = st.host;
    unsafe {
        let ssidx = (*host)
            .settings
            .as_ref()
            .expect("actionInvertSortOrder: host->settings is NULL")
            .ssIndex as usize;
        ScreenSettings_invertSortOrder(&mut (*host).settings.as_mut().unwrap().screens[ssidx]);
        let at = (*host)
            .activeTable
            .expect("actionInvertSortOrder: host->activeTable is NULL");
        (*at).needsSort = true;
    }
    HTOP_REFRESH | HTOP_SAVE_SETTINGS | HTOP_KEEP_FOLLOWING | HTOP_UPDATE_PANELHDR
}

/// Port of `static Htop_Reaction actionExpandOrCollapse(State* st)` from
/// `Action.c:356`. A no-op outside tree view; otherwise flips the selected
/// row's `showChildren` via [`expandCollapse`] and recalculates if it changed.
/// `settings->ss->treeView` is `screens[ssIndex].treeView`; the panel
/// `(Panel*)st->mainPanel` is the main panel's embedded `super_`.
pub fn actionExpandOrCollapse(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->host is a valid, non-null Machine* (C precondition:
    // st->host->settings->ss is dereferenced unconditionally).
    let treeView = unsafe {
        let s = (*st.host)
            .settings
            .as_ref()
            .expect("actionExpandOrCollapse: host->settings is NULL");
        s.screens[s.ssIndex as usize].treeView
    };
    if !treeView {
        return HTOP_OK;
    }

    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition).
    let changed = expandCollapse(unsafe { &mut (*st.mainPanel).super_ });
    if changed {
        HTOP_RECALCULATE
    } else {
        HTOP_OK
    }
}

/// Port of `static Htop_Reaction actionCollapseIntoParent(State* st)` from
/// `Action.c:364`. A no-op outside tree view; otherwise collapses the selection
/// into its parent via [`collapseIntoParent`] and recalculates if it changed.
pub fn actionCollapseIntoParent(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->host is a valid, non-null Machine* (C precondition).
    let treeView = unsafe {
        let s = (*st.host)
            .settings
            .as_ref()
            .expect("actionCollapseIntoParent: host->settings is NULL");
        s.screens[s.ssIndex as usize].treeView
    };
    if !treeView {
        return HTOP_OK;
    }

    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition).
    let changed = collapseIntoParent(unsafe { &mut (*st.mainPanel).super_ });
    if changed {
        HTOP_RECALCULATE
    } else {
        HTOP_OK
    }
}

/// Port of `static Htop_Reaction actionExpandCollapseOrSortColumn(State* st)`
/// from `Action.c:372`. In tree view, dispatches to [`actionExpandOrCollapse`];
/// otherwise to [`actionSetSortColumn`] (the now-ported sort-column picker).
pub fn actionExpandCollapseOrSortColumn(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->host is a valid, non-null Machine* (C precondition).
    let treeView = unsafe {
        let s = (*st.host)
            .settings
            .as_ref()
            .expect("actionExpandCollapseOrSortColumn: host->settings is NULL");
        s.screens[s.ssIndex as usize].treeView
    };
    if treeView {
        actionExpandOrCollapse(st)
    } else {
        actionSetSortColumn(st)
    }
}

/// Port of `static inline void setActiveScreen(Settings* settings, State* st,
/// unsigned int ssIdx)` from `Action.c:376`. Points the active screen at
/// `screens[ssIdx]`, defaulting its table to the process table, updates
/// `host->activeTable`, and retargets the main panel's function bar
/// (read-only when the active table is not the process table).
///
/// The C `settings->ss = settings->screens[ssIdx]` assignment has no analog:
/// the ported model derives `ss` as `screens[ssIndex]` (the caller has already
/// set `ssIndex`), so setting the pointer is implicit. `settings` is reached
/// through `st->host->settings` (C passes it explicitly, but it is always
/// `st->host->settings`). The process table is read before the mutable
/// `settings` borrow to avoid aliasing `host`.
pub fn setActiveScreen(st: &State, ssIdx: u32) {
    // SAFETY: st->host is a valid, non-null Machine* (C precondition).
    let host = unsafe { &mut *st.host };
    let settings = host
        .settings
        .as_mut()
        .expect("setActiveScreen: host->settings is NULL");
    debug_assert_eq!(settings.ssIndex, ssIdx);
    let idx = ssIdx as usize;

    // host->processTable read up front (the mutable `settings` borrow below
    // aliases `host`, so it cannot also read `host.processTable`).
    let processTable = host.processTable;
    let settings = host.settings.as_mut().unwrap();
    if settings.screens[idx].table.is_none() {
        settings.screens[idx].table = processTable;
    }
    let active = settings.screens[idx].table;
    host.activeTable = active;

    // set correct functionBar - readonly if requested, and/or non-process screens
    let readonly = Settings_isReadonly() || (active != processTable);
    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition).
    let mp = unsafe { &mut *st.mainPanel };
    MainPanel_setFunctionBar(mp, readonly);
}

/// Port of `static Htop_Reaction actionNextScreen(State* st)` from
/// `Action.c:390`. Advances `settings->ssIndex` (wrapping at the screen count)
/// and activates it via [`setActiveScreen`]. The C `settings->nScreens` is the
/// ported model's `screens.len()`.
pub fn actionNextScreen(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->host is a valid, non-null Machine* (C precondition).
    let idx = unsafe {
        let settings = (*st.host)
            .settings
            .as_mut()
            .expect("actionNextScreen: host->settings is NULL");
        let nScreens = settings.screens.len() as u32;
        settings.ssIndex += 1;
        if settings.ssIndex == nScreens {
            settings.ssIndex = 0;
        }
        settings.ssIndex
    };
    setActiveScreen(st, idx);
    HTOP_UPDATE_PANELHDR | HTOP_REFRESH | HTOP_REDRAW_BAR
}

/// Port of `static Htop_Reaction actionPrevScreen(State* st)` from
/// `Action.c:400`. Steps `settings->ssIndex` back one (wrapping to
/// `nScreens - 1` at zero) and activates it. `settings->nScreens` maps to
/// `screens.len()`.
pub fn actionPrevScreen(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->host is a valid, non-null Machine* (C precondition).
    let idx = unsafe {
        let settings = (*st.host)
            .settings
            .as_mut()
            .expect("actionPrevScreen: host->settings is NULL");
        if settings.ssIndex == 0 {
            settings.ssIndex = settings.screens.len() as u32 - 1;
        } else {
            settings.ssIndex -= 1;
        }
        settings.ssIndex
    };
    setActiveScreen(st, idx);
    HTOP_UPDATE_PANELHDR | HTOP_REFRESH | HTOP_REDRAW_BAR
}

/// Port of `Htop_Reaction Action_setScreenTab(State* st, int x)` from
/// `Action.c:411`. Hit-tests the click column `x` against the drawn screen-tab
/// row (`[heading]` cells separated by `SCREEN_TAB_COLUMN_GAP`, after
/// `SCREEN_TAB_MARGIN_LEFT`); on a hit it selects that screen via
/// [`setActiveScreen`], else returns `HTOP_OK` (C `0`).
///
/// `settings->nScreens` maps to `screens.len()`; `screens[i]->heading` is an
/// `Option<String>` (a NULL heading — never produced for a real screen — maps
/// to `""`). The C `strnlen(tab, n)` is `heading.len().min(n)` (a heading has
/// no interior NUL). `bracketWidth = strlen("[]") = 2`.
pub fn Action_setScreenTab(st: &State, x: i32) -> Htop_Reaction {
    let host = st.host;
    let bracketWidth: i32 = 2;

    if x < SCREEN_TAB_MARGIN_LEFT {
        return HTOP_OK;
    }

    // SAFETY: st->host is a valid, non-null Machine* (C precondition).
    let nScreens = unsafe {
        (*host)
            .settings
            .as_ref()
            .expect("Action_setScreenTab: host->settings is NULL")
            .screens
            .len()
    };

    let mut rem = x - SCREEN_TAB_MARGIN_LEFT;
    for i in 0..nScreens {
        // int width = rem >= bracketWidth ? strnlen(tab, rem - bracketWidth + 1) : 0;
        let width = if rem >= bracketWidth {
            let n = (rem - bracketWidth + 1) as usize;
            // SAFETY: st->host valid (C precondition); the &str borrow is
            // contained in this block.
            unsafe {
                let heading = (*host).settings.as_ref().unwrap().screens[i]
                    .heading
                    .as_deref()
                    .unwrap_or("");
                heading.len().min(n) as i32
            }
        } else {
            0
        };
        if width >= rem - bracketWidth + 1 {
            // SAFETY: st->host valid (C precondition).
            unsafe {
                (*host).settings.as_mut().unwrap().ssIndex = i as u32;
            }
            setActiveScreen(st, i as u32);
            return HTOP_UPDATE_PANELHDR | HTOP_REFRESH | HTOP_REDRAW_BAR;
        }

        rem -= bracketWidth + width;
        if rem < SCREEN_TAB_COLUMN_GAP {
            return HTOP_OK;
        }

        rem -= SCREEN_TAB_COLUMN_GAP;
    }
    HTOP_OK
}

/// Port of `static Htop_Reaction actionQuit(ATTR_UNUSED State* st)` from
/// `Action.c:439`. The `State*` argument is `ATTR_UNUSED`; the full
/// behavior is returning the `HTOP_QUIT` constant. The parameter is
/// kept (prefixed `_`) to mirror the C signature.
pub fn actionQuit(_st: &mut State) -> Htop_Reaction {
    HTOP_QUIT
}

/// Port of `static Htop_Reaction actionSetAffinity(State* st)` from
/// `Action.c:443`, for the darwin build where neither `HAVE_LIBHWLOC` nor
/// `HAVE_AFFINITY` is defined. The `#if (HWLOC || AFFINITY)` block (the
/// `AffinityPanel` picker) is excluded on macOS, leaving the `#else` body: the
/// writeable-process and single-CPU guards run, then `return HTOP_OK`. htoprs
/// is darwin-first, so this is the faithful compiled form for the target
/// platform. `host->activeCPUs` is read but leads to `HTOP_OK` either way.
pub fn actionSetAffinity(st: &mut State) -> Htop_Reaction {
    if !Action_writeableProcess(st) {
        return HTOP_OK;
    }

    // SAFETY: st->host is a valid, non-null Machine* (C precondition).
    let host = unsafe { &*st.host };
    if host.activeCPUs == 1 {
        return HTOP_OK;
    }

    HTOP_OK
}

/// Port of `static Htop_Reaction actionSetSchedPolicy(State* st)` from
/// `Action.c:480` (`#ifdef SCHEDULER_SUPPORT`). Modally picks a scheduling
/// policy (re-picking while the reset-on-fork toggle, `key == -1`, is chosen),
/// then a priority, and applies both to every tagged/selected row via
/// `MainPanel_foreachRow(Scheduling_rowSetPolicy)`. The re-pick loop relies on
/// the non-destructive [`Action_pickFromVector`] (returns the selected index +
/// the reclaimed panel, so `schedPanel` survives each pick). The modal body is
/// verified by primary-source reading (the `actionKill`/`actionStrace`
/// precedent — `ScreenManager_run` isn't headless-drivable); `Scheduling_setPolicy`
/// is a no-op off Linux, so on the darwin build the `foreachRow` pass changes
/// nothing.
pub fn actionSetSchedPolicy(st: &mut State) -> Htop_Reaction {
    use crate::ported::scheduling::{
        SchedulingArg, Scheduling_newPolicyPanel, Scheduling_newPriorityPanel,
        Scheduling_rowSetPolicy, Scheduling_togglePolicyPanelResetOnFork,
    };
    use std::sync::atomic::Ordering;

    // if (!Action_writeableProcess(st)) return HTOP_KEEP_FOLLOWING;
    if !Action_writeableProcess(st) {
        return HTOP_KEEP_FOLLOWING;
    }

    // static int preSelectedPolicy = SCHED_OTHER; static int preSelectedPriority = 50;
    static PRE_SELECTED_POLICY: std::sync::atomic::AtomicI32 =
        std::sync::atomic::AtomicI32::new(libc::SCHED_OTHER);
    static PRE_SELECTED_PRIORITY: std::sync::atomic::AtomicI32 =
        std::sync::atomic::AtomicI32::new(50);

    // Reads the `key` of the panel item at `idx` (all these panels hold ListItems).
    let item_key = |panel: &dyn PanelClass, idx: usize| -> i32 {
        (panel.as_panel().items[idx].object() as &dyn core::any::Any)
            .downcast_ref::<ListItem>()
            .expect("actionSetSchedPolicy: panel item is not a ListItem")
            .key
    };

    // Panel* schedPanel = Scheduling_newPolicyPanel(preSelectedPolicy);
    let mut sched_panel: Box<dyn PanelClass> = Box::new(Scheduling_newPolicyPanel(
        PRE_SELECTED_POLICY.load(Ordering::Relaxed),
    ));

    // for (;;) { policy = pickFromVector(schedPanel, 18, true);
    //   if (!policy || policy->key != -1) break;
    //   Scheduling_togglePolicyPanelResetOnFork(schedPanel); }
    let mut policy_key: Option<i32> = None;
    loop {
        let (sel, panel) = Action_pickFromVector(st, sched_panel, 18, true);
        sched_panel = panel;
        match sel {
            None => break,
            Some(i) => {
                let key = item_key(sched_panel.as_ref(), i);
                if key != -1 {
                    policy_key = Some(key);
                    break;
                }
                Scheduling_togglePolicyPanelResetOnFork(sched_panel.as_panel_mut());
            }
        }
    }

    if let Some(policy) = policy_key {
        // preSelectedPolicy = policy->key;
        PRE_SELECTED_POLICY.store(policy, Ordering::Relaxed);

        // Panel* prioPanel = Scheduling_newPriorityPanel(policy->key, preSelectedPriority);
        if let Some(prio_panel) =
            Scheduling_newPriorityPanel(policy, PRE_SELECTED_PRIORITY.load(Ordering::Relaxed))
        {
            // const ListItem* prio = pickFromVector(prioPanel, 14, true);
            let (prio_sel, prio_panel_back) =
                Action_pickFromVector(st, Box::new(prio_panel), 14, true);
            // if (prio) preSelectedPriority = prio->key;
            if let Some(pi) = prio_sel {
                PRE_SELECTED_PRIORITY
                    .store(item_key(prio_panel_back.as_ref(), pi), Ordering::Relaxed);
            }
            // Panel_delete(prioPanel) — prio_panel_back drops here.
        }

        // SchedulingArg v = { .policy = …, .priority = … };
        let mut v = SchedulingArg {
            policy: PRE_SELECTED_POLICY.load(Ordering::Relaxed),
            priority: PRE_SELECTED_PRIORITY.load(Ordering::Relaxed),
        };
        // bool ok = MainPanel_foreachRow(mainPanel, Scheduling_rowSetPolicy, (Arg){.v=&v}, NULL);
        // SAFETY: st->mainPanel is the caller-owned MainPanel*.
        let ok = MainPanel_foreachRow(
            unsafe { &mut *st.mainPanel },
            Scheduling_rowSetPolicy,
            Arg::V(&mut v as *mut SchedulingArg as *mut core::ffi::c_void),
            None,
        );
        // if (!ok) beep();
        if !ok {
            let mut out = std::io::stdout().lock();
            Ncurses::beep(&mut out);
        }
    }

    // Panel_delete(schedPanel) — sched_panel drops here.
    HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_KEEP_FOLLOWING
}

/// TODO: port of `static Htop_Reaction actionKill(State* st)` from
/// `Action.c:524`. Presents the `SignalsPanel` picker via
/// [`Action_pickFromVector`], then delivers the chosen signal to every
/// tagged/selected row through `MainPanel_foreachRow`. The picker itself is now
/// portable ([`Action_pickFromVector`]); the remaining blockers are the
/// `Process_rowSendSignal` callback (`fn(&dyn Object, Arg) -> bool`), incompatible
/// with the ported `MainPanel_foreachRowFn` (`fn(&mut Row, &Arg)`), and the
/// unported ncurses `Panel_draw` / `refresh()` / `beep()` / `napms()` epilogue.
/// C `static int preSelectedSignal = SIGNALSPANEL_INITSELECTEDSIGNAL;` — the
/// signal remembered between opens of the kill picker (a function static).
static PRE_SELECTED_SIGNAL: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(SIGNALSPANEL_INITSELECTEDSIGNAL);

pub fn actionKill(st: &mut State) -> Htop_Reaction {
    use std::sync::atomic::Ordering;

    // C: if (!Action_writeableProcess(st)) return HTOP_OK;
    if !Action_writeableProcess(st) {
        return HTOP_OK;
    }

    let pre = PRE_SELECTED_SIGNAL.load(Ordering::Relaxed);

    // C: Panel* signalsPanel = SignalsPanel_new(preSelectedSignal);
    // The signal table is per-OS (htop links each Platform.c). Darwin's is
    // ported; the TUI only runs on darwin, so other targets compile against an
    // empty table (linux's Platform_signals is not ported yet).
    #[cfg(target_os = "macos")]
    let signals = crate::ported::darwin::platform::Platform_signals;
    #[cfg(not(target_os = "macos"))]
    let signals: &[crate::ported::signalspanel::SignalItem] = &[];
    let signalsPanel = SignalsPanel_new(pre, signals);

    // C: const ListItem* sgn = (ListItem*) Action_pickFromVector(st, signalsPanel, 14, true);
    let (picked, panel) = Action_pickFromVector(st, Box::new(signalsPanel), 14, true);

    // C: if (sgn && sgn->key != 0) { ... }
    if let Some(i) = picked {
        let any: &dyn core::any::Any = panel.as_panel().items[i].object();
        if let Some(sgn) = any.downcast_ref::<ListItem>() {
            if sgn.key != 0 {
                PRE_SELECTED_SIGNAL.store(sgn.key, Ordering::Relaxed);

                // C: Panel_setHeader((Panel*)mainPanel, "Sending...");
                //    Panel_draw(...); refresh();
                // SAFETY: st->mainPanel is the caller-owned MainPanel* for the run.
                let mp = unsafe { &mut (*st.mainPanel).super_ };
                Panel_setHeader(mp, "Sending...");
                Panel_draw(mp, false, true, true, false);
                {
                    let mut out = std::io::stdout().lock();
                    Ncurses::refresh(&mut out);
                }

                // C: bool ok = MainPanel_foreachRow(mainPanel, Process_rowSendSignal,
                //             (Arg){.i = sgn->key}, NULL); if (!ok) beep();
                let ok = MainPanel_foreachRow(
                    unsafe { &mut *st.mainPanel },
                    Process_rowSendSignal,
                    Arg::I(sgn.key),
                    None,
                );
                if !ok {
                    let mut out = std::io::stdout().lock();
                    Ncurses::beep(&mut out);
                }

                // C: napms(500);
                Ncurses::napms(500);
            }
        }
    }

    // C: Panel_delete((Object*)signalsPanel); — `Action_pickFromVector` reclaims
    //    and drops the picker box, so no explicit delete is needed here.
    // C: return HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR;
    HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR
}

/// Port of `static Htop_Reaction actionFilterByUser(State* st)` from
/// `Action.c:548`. Builds a users picker (populated by [`addUserToVector`] via
/// `UsersTable_foreach` over the machine's uid->name cache), runs it through
/// [`Action_pickFromVector`], and sets `host->userId` from the pick — "All
/// users" (`key == -1`) resets to `(uid_t)-1`, otherwise the chosen name maps
/// to its uid via [`Action_setUserOnly`]. The `Vector_insertionSort` becomes a
/// direct sort of the `Vec<PanelItem>` by each `ListItem`'s name.
pub fn actionFilterByUser(st: &mut State) -> Htop_Reaction {
    // C: Panel* usersPanel = Panel_new(0,0,0,0, Class(ListItem), true,
    //       FunctionBar_newEnterEsc("Show   ", "Cancel "));
    let mut usersPanel = Panel_new(
        0,
        0,
        0,
        0,
        Some(FunctionBar_newEnterEsc("Show   ", "Cancel ")),
    );
    Panel_setHeader(&mut usersPanel, "Show processes of:");

    // C: UsersTable_foreach(host->usersTable, addUserToVector, usersPanel);
    // The machine's `usersTable` (an opaque pointer) is the uid->name cache the
    // process scan populates; each cached user becomes a picker row. An unset
    // table (`None`) yields just the "All users" entry inserted below.
    // SAFETY: host aliases the caller-owned Machine for the run.
    let usersTable = unsafe { (*st.host).usersTable };
    if let Some(ptr) = usersTable {
        // SAFETY: the handle is the Machine-owned UsersTable pointer for the run.
        let ut = unsafe { &*(ptr as *const UsersTable) };
        UsersTable_foreach(ut, &mut |uid, name| {
            addUserToVector(uid as i32, name, &mut usersPanel);
        });
    }

    // C: Vector_insertionSort(usersPanel->items); — sort the ListItems by name.
    // The port models items as `Vec<PanelItem>`, so sort it directly by each
    // ListItem's value (the same order the C Object compare produces).
    usersPanel.items.sort_by(|a, b| {
        let name = |it: &PanelItem| -> String {
            match it {
                PanelItem::Owned(o) => (o.as_ref() as &dyn core::any::Any)
                    .downcast_ref::<ListItem>()
                    .map(|li| ListItem_getRef(li).to_string())
                    .unwrap_or_default(),
                PanelItem::Borrowed(_) => String::new(),
            }
        };
        name(a).cmp(&name(b))
    });

    // C: ListItem* allUsers = ListItem_new("All users", -1);
    //    Panel_insert(usersPanel, 0, (Object*) allUsers);
    Panel_insert(&mut usersPanel, 0, Box::new(ListItem_new("All users", -1)));

    // C: const ListItem* picked = (ListItem*) Action_pickFromVector(st, usersPanel, 19, false);
    let (picked, panel) = Action_pickFromVector(st, Box::new(usersPanel), 19, false);
    if let Some(i) = picked {
        if let Some(li) =
            (panel.as_panel().items[i].object() as &dyn core::any::Any).downcast_ref::<ListItem>()
        {
            // C: if (picked == allUsers) host->userId = (uid_t)-1;
            //    else Action_setUserOnly(ListItem_getRef(picked), &host->userId);
            if li.key == -1 {
                // SAFETY: host aliases the caller-owned Machine.
                unsafe {
                    (*st.host).userId = u32::MAX; // (uid_t)-1 == all users
                }
            } else {
                let name = ListItem_getRef(li).to_string();
                // SAFETY: host aliases the caller-owned Machine.
                Action_setUserOnly(&name, unsafe { &mut (*st.host).userId });
            }
        }
    }

    HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR
}

/// Port of `Htop_Reaction Action_follow(State* st)` from `Action.c:568`. Pins
/// the active table's `following` field to the selected row's id
/// ([`MainPanel_selectedRow`]) and switches the panel's selection color to
/// `PANEL_SELECTION_FOLLOW`. `host->activeTable` non-null is the precondition
/// (as in C).
pub fn Action_follow(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition).
    let sel = MainPanel_selectedRow(unsafe { &*st.mainPanel });
    // SAFETY: st->host is a valid, non-null Machine* (C precondition:
    // st->host->activeTable is dereferenced unconditionally).
    let host = unsafe { &mut *st.host };
    let at = host
        .activeTable
        .expect("Action_follow: host->activeTable is NULL");
    unsafe {
        (*at).following = sel;
    }
    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition).
    Panel_setSelectionColor(
        unsafe { &mut (*st.mainPanel).super_ },
        ColorElements::PANEL_SELECTION_FOLLOW,
    );
    HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionSetup(State* st)` from `Action.c:574`.
/// Runs the setup screen via the now-ported [`Action_runSetup`] and returns the
/// refresh/redraw/resize reaction.
pub fn actionSetup(st: &mut State) -> Htop_Reaction {
    Action_runSetup(st);
    HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR | HTOP_RESIZE
}

/// TODO: port of `static Htop_Reaction actionLsof(State* st)` from
/// `Action.c:579`. Opens the selected process's `OpenFilesScreen` as a modal
/// `InfoScreen`. Blocked on ncurses: the epilogue calls `clear()` (unported
/// in `crt.rs`), and `InfoScreen_run` takes `&mut dyn InfoScreenClass` while
/// `OpenFilesScreen` does not implement that trait, so it cannot be driven yet.
pub fn actionLsof(st: &mut State) -> Htop_Reaction {
    // C: if (!Action_writeableProcess(st)) return HTOP_OK;
    if !Action_writeableProcess(st) {
        return HTOP_OK;
    }
    // C: const Process* p = (Process*)Panel_getSelected((Panel*)st->mainPanel);
    let mainpanel = st.mainPanel;
    if mainpanel.is_null() {
        return HTOP_OK;
    }
    // C: OpenFilesScreen* ofs = OpenFilesScreen_new(p);
    let mut ofs = {
        // SAFETY: `mainPanel` is the process panel wired at startup.
        let panel = unsafe { &(*mainpanel).super_ };
        let p = match Panel_getSelected(panel).and_then(|o| o.as_process()) {
            Some(p) => p,
            None => return HTOP_OK,
        };
        OpenFilesScreen_new(p)
    };
    // C: InfoScreen_run((InfoScreen*)ofs);
    InfoScreen_run(&mut ofs);
    // C: OpenFilesScreen_delete((Object*)ofs);
    OpenFilesScreen_delete(ofs);
    // C: clear(); CRT_enableDelay();
    let mut out = std::io::stdout().lock();
    Ncurses::clear(&mut out);
    Ncurses::refresh(&mut out);
    CRT_enableDelay();
    HTOP_REFRESH | HTOP_REDRAW_BAR
}

/// TODO: port of `static Htop_Reaction actionShowLocks(State* st)` from
/// `Action.c:597`. Opens the selected process's `ProcessLocksScreen` modally.
/// Blocked on the same ncurses substrate as [`actionLsof`]: `clear()` is
/// unported and `ProcessLocksScreen` does not implement `InfoScreenClass`.
pub fn actionShowLocks(st: &mut State) -> Htop_Reaction {
    // C: const Process* p = (Process*) Panel_getSelected((Panel*)st->mainPanel);
    //    if (!p) return HTOP_OK;
    // SAFETY: mainPanel is the caller-owned MainPanel* for the run.
    let panel = unsafe { &(*st.mainPanel).super_ };
    let p: *const Process = match Panel_getSelected(panel).and_then(|o| o.as_process()) {
        Some(pr) => pr as *const Process,
        None => return HTOP_OK,
    };

    // C: ProcessLocksScreen* pls = ProcessLocksScreen_new(p);
    //    InfoScreen_run((InfoScreen*)pls);
    // SAFETY: `p` points at the selected Process in the main panel, which is not
    // mutated while the modal locks screen runs (InfoScreen_run drives its own
    // display panel).
    let mut pls = ProcessLocksScreen_new(unsafe { &*p });
    InfoScreen_run(&mut pls);

    // C: ProcessLocksScreen_delete((Object*)pls);
    ProcessLocksScreen_delete(pls);

    // C: clear(); CRT_enableDelay();
    {
        let mut out = std::io::stdout().lock();
        Ncurses::clear(&mut out);
    }
    CRT_enableDelay();

    HTOP_REFRESH | HTOP_REDRAW_BAR
}

/// Port of `static Htop_Reaction actionBacktrace(State *st)` from
/// `Action.c:616` (`#if defined(HAVE_BACKTRACE_SCREEN)`). Collects the selected
/// process (or, for a main-thread process, its whole thread group) into a
/// non-owning `Vec<*const Process>`, builds a `BacktracePanel` from it, and runs
/// it modally in a `ScreenManager` — the [`actionStrace`] modal precedent.
/// Deviations: the ported `ScreenManager` owns its panels, so the C's separate
/// `BacktracePanel_delete` is folded into `ScreenManager_delete` (freed once); a
/// NULL selection is skipped (empty list) rather than pushed. The modal body is
/// verified by primary-source reading (`ScreenManager_run` can't be driven
/// headlessly); the null-`mainPanel` guard has a unit test.
pub fn actionBacktrace(st: &mut State) -> Htop_Reaction {
    use crate::ported::backtracescreen::BacktracePanel_new;
    use crate::ported::process::{Process_getThreadGroup, Process_isUserlandThread};
    use crate::ported::screenmanager::ScreenManager_run;
    use crate::ported::settings::Settings;

    // Process* selectedProcess = (Process*) Panel_getSelected(st->mainPanel);
    // const Vector* allProcesses = st->mainPanel->super.items;
    // SAFETY: `mainPanel` is the caller-owned process panel wired at startup.
    let mainpanel = st.mainPanel;
    if mainpanel.is_null() {
        return HTOP_OK;
    }
    let panel = unsafe { &(*mainpanel).super_ };

    let selected: Option<*const Process> = Panel_getSelected(panel)
        .and_then(|o| o.as_process())
        .map(|p| p as *const Process);

    // Vector* processes = Vector_new(Class(Process), false, …) — a non-owning
    // list of borrowed `Process*`, modeled as `Vec<*const Process>` (the type
    // BacktracePanel_new takes).
    let mut processes: Vec<*const Process> = Vec::new();
    match selected {
        // if (selected && !Process_isUserlandThread(selected)): the whole
        // thread group of the selected main-thread process.
        Some(sel) if !Process_isUserlandThread(unsafe { &*sel }) => {
            let tg = Process_getThreadGroup(unsafe { &*sel });
            for item in &panel.items {
                if let Some(p) = item.object().as_process() {
                    if Process_getThreadGroup(p) == tg {
                        processes.push(p as *const Process);
                    }
                }
            }
        }
        // else Vector_add(processes, selectedProcess): a thread adds itself.
        Some(sel) => processes.push(sel),
        // The C adds the (here NULL) selectedProcess; the port skips a NULL
        // selection (empty list) to avoid a null deref when the panel populates
        // its frames — the degenerate empty-panel case.
        None => {}
    }

    // BacktracePanel* panel = BacktracePanel_new(processes, st->host->settings);
    let settings = unsafe { (*st.host).settings.as_ref() }
        .map(|s| s as *const Settings)
        .unwrap_or(core::ptr::null());
    let bt_panel = BacktracePanel_new(processes, settings);

    // ScreenManager* sm = ScreenManager_new(NULL, st->host, st, false);
    let host = st.host;
    let st_ptr: *mut State = st;
    let mut sm = ScreenManager_new(core::ptr::null_mut(), host, st_ptr);
    // ScreenManager_add(sm, (Panel*)panel, 0);
    ScreenManager_add(&mut sm, bt_panel, 0);
    // ScreenManager_run(sm, NULL, NULL, NULL);
    ScreenManager_run(&mut sm, None, None, None);
    // C: BacktracePanel_delete(panel); ScreenManager_delete(sm). The ported
    // ScreenManager owns its panels (`Vec<Box<dyn PanelClass>>`), unlike the
    // C's `owner=false` manager, so the panel is freed once — by
    // ScreenManager_delete — not separately.
    ScreenManager_delete(sm);

    HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_UPDATE_PANELHDR
}

/// TODO: port of `static Htop_Reaction actionStrace(State* st)` from
/// `Action.c:644`. Forks a tracer and shows its `TraceScreen` modally. Blocked
/// on ncurses (`clear()` unported) and the `InfoScreen_run` trait gap
/// (`TraceScreen` does not implement `InfoScreenClass`).
pub fn actionStrace(st: &mut State) -> Htop_Reaction {
    // C: if (!Action_writeableProcess(st)) return HTOP_OK;
    if !Action_writeableProcess(st) {
        return HTOP_OK;
    }
    // C: const Process* p = (Process*)Panel_getSelected((Panel*)st->mainPanel);
    let mainpanel = st.mainPanel;
    if mainpanel.is_null() {
        return HTOP_OK;
    }
    // C: TraceScreen* ts = TraceScreen_new(p);
    let mut ts = {
        // SAFETY: `mainPanel` is the process panel wired at startup.
        let panel = unsafe { &(*mainpanel).super_ };
        let p = match Panel_getSelected(panel).and_then(|o| o.as_process()) {
            Some(p) => p,
            None => return HTOP_OK,
        };
        TraceScreen_new(p)
    };
    // C: bool ok = TraceScreen_forkTracer(ts); if (ok) InfoScreen_run(...);
    if TraceScreen_forkTracer(&mut ts) {
        InfoScreen_run(&mut ts);
    }
    // C: TraceScreen_delete((Object*)ts);  — kills the tracer child + closes the
    // pipe; the owned InfoScreen fields then release when `ts` drops at scope end.
    TraceScreen_delete(&mut ts);
    // C: clear(); CRT_enableDelay();
    let mut out = std::io::stdout().lock();
    Ncurses::clear(&mut out);
    Ncurses::refresh(&mut out);
    CRT_enableDelay();
    HTOP_REFRESH | HTOP_REDRAW_BAR
}

/// Port of `static Htop_Reaction actionTag(State* st)` from `Action.c:665`.
/// Toggles the selected row's tag ([`Row_toggleTag`]) and advances the
/// selection one row down ([`Panel_onKey`] with [`KEY_DOWN`]); a no-op when the
/// panel has no selectable row (the C `if (!r) return HTOP_OK`). The C
/// `(Row*)Panel_getSelected` cast becomes an index into the panel's items with
/// an `as_row_mut()` upcast to `&mut Row` (the `expandCollapse` idiom).
pub fn actionTag(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition).
    let panel = unsafe { &mut (*st.mainPanel).super_ };
    let sel = panel.selected;
    // C: Row* r = (Row*) Panel_getSelected(...); if (!r) return HTOP_OK;
    if sel < 0 || sel as usize >= panel.items.len() {
        return HTOP_OK;
    }
    {
        let obj: &mut dyn Object = panel.items[sel as usize].object_mut();
        // C: `(Row*) Panel_getSelected(...)` is a superclass upcast, not a
        // concrete-type cast. Panel items are `Process` objects, so reach the
        // embedded `Row` via `as_row_mut()`; `downcast_mut::<Row>()` matches
        // only a *bare* `Row` and silently missed every `Process` — which is
        // why Space (tag) appeared to do nothing.
        if let Some(r) = obj.as_row_mut() {
            Row_toggleTag(r);
        }
    }
    Panel_onKey(panel, KEY_DOWN);
    HTOP_OK
}

/// TODO: port of `static Htop_Reaction actionRedraw(ATTR_UNUSED State* st)` from
/// `Action.c:675`. The body is `clear(); return HTOP_RECALCULATE | HTOP_REFRESH
/// | HTOP_REDRAW_BAR;`. Blocked on the ncurses `clear()` primitive, which is
/// not ported in `crt.rs` (no `clear`/`refresh` drawing primitives exist yet).
pub fn actionRedraw(_st: &mut State) -> Htop_Reaction {
    // C `clear();` — wipe the screen so the next draw is from a clean slate.
    let mut out = std::io::stdout().lock();
    Ncurses::clear(&mut out);
    Ncurses::refresh(&mut out);
    // HTOP_RECALCULATE here makes Ctrl-L also refresh the data, not just redraw.
    HTOP_RECALCULATE | HTOP_REFRESH | HTOP_REDRAW_BAR
}

/// Port of `static Htop_Reaction actionTogglePauseUpdate(State* st)` from
/// `Action.c:681`. Flips the `State.pauseUpdate` flag and returns the
/// verbatim `HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_KEEP_FOLLOWING`
/// bit-or. The C touches only `st->pauseUpdate`.
pub fn actionTogglePauseUpdate(st: &mut State) -> Htop_Reaction {
    st.pauseUpdate = !st.pauseUpdate;
    HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_KEEP_FOLLOWING
}

/// Port of `static inline void addattrstr(int attr, const char* str)` from
/// `Action.c:746`: set the attribute, then write the string at the current
/// cursor. Takes the crossterm output sink (htoprs's terminal backend) in
/// place of the C global `stdscr`. Used by [`actionHelp`] to lay out the
/// colored help legend.
pub fn addattrstr<W: std::io::Write>(out: &mut W, attr: i32, str_: &str) {
    Ncurses::attrset(out, attr);
    Ncurses::addstr(out, str_);
}

/// One row of the help-screen key legend. Models the anonymous file-static
/// `struct { const char* key; bool roInactive; const char* info; }` used by
/// `helpLeft`/`helpRight` (`Action.c:686`/`Action.c:713`). The C arrays are
/// NUL-`key` terminated; the Rust slices carry their own length, so the
/// terminator entry is dropped.
struct HelpItem {
    key: &'static str,
    roInactive: bool,
    info: &'static str,
}

/// Port of `static const struct { ... } helpLeft[]` from `Action.c:686`.
#[allow(non_upper_case_globals)]
const helpLeft: &[HelpItem] = &[
    HelpItem {
        key: "      #: ",
        roInactive: false,
        info: "hide/show header meters",
    },
    HelpItem {
        key: "    Tab: ",
        roInactive: false,
        info: "switch to next screen tab",
    },
    HelpItem {
        key: " Arrows: ",
        roInactive: false,
        info: "scroll process list",
    },
    HelpItem {
        key: " Digits: ",
        roInactive: false,
        info: "incremental PID search",
    },
    HelpItem {
        key: "   F3 /: ",
        roInactive: false,
        info: "incremental name search",
    },
    HelpItem {
        key: "   F4 \\: ",
        roInactive: false,
        info: "incremental name filtering",
    },
    HelpItem {
        key: "   F5 t: ",
        roInactive: false,
        info: "tree view",
    },
    HelpItem {
        key: "      p: ",
        roInactive: false,
        info: "toggle program path",
    },
    HelpItem {
        key: "      m: ",
        roInactive: false,
        info: "toggle merged command",
    },
    HelpItem {
        key: "      Z: ",
        roInactive: false,
        info: "pause/resume process updates",
    },
    HelpItem {
        key: "      u: ",
        roInactive: false,
        info: "show processes of a single user",
    },
    HelpItem {
        key: "      H: ",
        roInactive: false,
        info: "hide/show user process threads",
    },
    HelpItem {
        key: "      K: ",
        roInactive: false,
        info: "hide/show kernel threads",
    },
    HelpItem {
        key: "      O: ",
        roInactive: false,
        info: "hide/show processes in containers",
    },
    HelpItem {
        key: "      F: ",
        roInactive: false,
        info: "cursor follows process",
    },
    HelpItem {
        key: "  + - *: ",
        roInactive: false,
        info: "expand/collapse tree/toggle all",
    },
    HelpItem {
        key: "N P M T: ",
        roInactive: false,
        info: "sort by PID, CPU%, MEM% or TIME",
    },
    HelpItem {
        key: "      I: ",
        roInactive: false,
        info: "invert sort order",
    },
    HelpItem {
        key: " F6 > .: ",
        roInactive: false,
        info: "select sort column",
    },
];

/// Port of `static const struct { ... } helpRight[]` from `Action.c:713`.
/// The `a` (`HAVE_LIBHWLOC || HAVE_AFFINITY`), `b` (`HAVE_BACKTRACE_SCREEN`)
/// and `Y` (`SCHEDULER_SUPPORT`) entries are Linux-only in htop; the darwin
/// build compiles them out (matching the `#if`/`#ifdef` guards in the C).
#[allow(non_upper_case_globals)]
const helpRight: &[HelpItem] = &[
    HelpItem {
        key: "  S-Tab: ",
        roInactive: false,
        info: "switch to previous screen tab",
    },
    HelpItem {
        key: "  Space: ",
        roInactive: false,
        info: "tag process",
    },
    HelpItem {
        key: "      c: ",
        roInactive: false,
        info: "tag process and its children",
    },
    HelpItem {
        key: "      U: ",
        roInactive: false,
        info: "untag all processes",
    },
    HelpItem {
        key: "   F9 k: ",
        roInactive: true,
        info: "kill process/tagged processes",
    },
    HelpItem {
        key: "   F7 ]: ",
        roInactive: true,
        info: "higher priority (root only)",
    },
    HelpItem {
        key: "   F8 [: ",
        roInactive: true,
        info: "lower priority (+ nice)",
    },
    #[cfg(target_os = "linux")]
    HelpItem {
        key: "      a: ",
        roInactive: true,
        info: "set CPU affinity",
    },
    #[cfg(target_os = "linux")]
    HelpItem {
        key: "      b: ",
        roInactive: false,
        info: "show process backtrace",
    },
    HelpItem {
        key: "      e: ",
        roInactive: false,
        info: "show process environment",
    },
    HelpItem {
        key: "      i: ",
        roInactive: true,
        info: "set IO priority",
    },
    HelpItem {
        key: "      l: ",
        roInactive: true,
        info: "list open files with lsof",
    },
    HelpItem {
        key: "      x: ",
        roInactive: false,
        info: "list file locks of process",
    },
    HelpItem {
        key: "      s: ",
        roInactive: true,
        info: "trace syscalls with strace",
    },
    HelpItem {
        key: "      w: ",
        roInactive: false,
        info: "wrap process command in multiple lines",
    },
    #[cfg(target_os = "linux")]
    HelpItem {
        key: "      Y: ",
        roInactive: true,
        info: "set scheduling policy",
    },
    HelpItem {
        key: " F2 C S: ",
        roInactive: false,
        info: "setup",
    },
    HelpItem {
        key: " F1 h ?: ",
        roInactive: false,
        info: "show this help screen",
    },
    HelpItem {
        key: "  F10 q: ",
        roInactive: false,
        info: "quit",
    },
];

/// Port of `static Htop_Reaction actionHelp(State* st)` from `Action.c:751`.
///
/// Clears the screen and paints the full help page: the version/copyright
/// banner, the colored CPU/memory/swap-bar legends, the process-state key,
/// then the two-column key-binding table (`helpLeft`/`helpRight`), and finally
/// waits for a keypress before wiping the screen. The C `#define addbartext`
/// and `#define addattrstatestr` macros become nested `fn`s; `CRT_colors[X]`
/// is `ColorElements::X.packed(scheme)` (the active scheme read once, exactly
/// as C's `CRT_colors` already points at `CRT_colorSchemes[CRT_colorScheme]`);
/// `LINES`/`COLS` are `Ncurses::lines()`/`Ncurses::cols()`.
pub fn actionHelp(st: &mut State) -> Htop_Reaction {
    use crate::ported::crt::ColorElements::{
        BAR_BORDER, BAR_SHADOW, CPU_GUEST, CPU_IOWAIT, CPU_IRQ, CPU_NICE_TEXT, CPU_NORMAL,
        CPU_SOFTIRQ, CPU_STEAL, CPU_SYSTEM, DEFAULT_COLOR, HELP_BOLD, HELP_SHADOW, PROCESS_D_STATE,
        PROCESS_RUN_STATE, PROCESS_SHADOW, PROCESS_THREAD, SWAP,
    };
    use std::io::Write;

    // C `#define addbartext(attr, prefix, text)`: default-colored prefix, then
    // the attributed text.
    fn addbartext<W: Write>(out: &mut W, scheme: ColorScheme, attr: i32, prefix: &str, text: &str) {
        addattrstr(out, ColorElements::DEFAULT_COLOR.packed(scheme), prefix);
        addattrstr(out, attr, text);
    }

    // C `#define addattrstatestr(attr, state, desc)`: attributed state glyph,
    // then default-colored `": " desc`.
    fn addattrstatestr<W: Write>(
        out: &mut W,
        scheme: ColorScheme,
        attr: i32,
        state: &str,
        desc: &str,
    ) {
        addattrstr(out, attr, state);
        addattrstr(
            out,
            ColorElements::DEFAULT_COLOR.packed(scheme),
            &format!(": {desc}"),
        );
    }

    let scheme = ColorScheme::active();

    // C reads st->host->settings->detailedCPUTime / showCachedMemory.
    // SAFETY: st->host is a valid, non-null Machine* (C precondition).
    let (detailedCPUTime, showCachedMemory) = {
        let settings = unsafe {
            (*st.host)
                .settings
                .as_ref()
                .expect("actionHelp: host->settings is NULL")
        };
        (settings.detailedCPUTime, settings.showCachedMemory)
    };

    let mut out = std::io::stdout().lock();

    // C: clear(); attrset(CRT_colors[HELP_BOLD]);
    Ncurses::clear(&mut out);
    Ncurses::attrset(&mut out, HELP_BOLD.packed(scheme));

    // C: for (int i = 0; i < LINES - 1; i++) mvhline(i, 0, ' ', COLS);
    let lines = Ncurses::lines();
    let cols = Ncurses::cols();
    for i in 0..(lines - 1) {
        Ncurses::mvhline(&mut out, i, 0, ' ', cols);
    }

    let mut line: i32 = 0;

    // C: mvaddstr(line++, 0, "htop " VERSION " - " COPYRIGHT);
    Ncurses::mvaddstr(&mut out, line, 0, &format!("htop {VERSION} - {COPYRIGHT}"));
    line += 1;
    // C: mvaddstr(line++, 0, "Released under the GNU GPLv2+. ...");
    Ncurses::mvaddstr(
        &mut out,
        line,
        0,
        "Released under the GNU GPLv2+. See 'man' page for more info.",
    );
    line += 1;

    // C: attrset(CRT_colors[DEFAULT_COLOR]); line++;
    Ncurses::attrset(&mut out, DEFAULT_COLOR.packed(scheme));
    line += 1;
    // C: mvaddstr(line++, 0, "CPU usage bar: ");
    Ncurses::mvaddstr(&mut out, line, 0, "CPU usage bar: ");
    line += 1;

    addattrstr(&mut out, BAR_BORDER.packed(scheme), "[");
    addbartext(&mut out, scheme, CPU_NICE_TEXT.packed(scheme), "", "low");
    addbartext(&mut out, scheme, CPU_NORMAL.packed(scheme), "/", "normal");
    addbartext(&mut out, scheme, CPU_SYSTEM.packed(scheme), "/", "kernel");
    if detailedCPUTime {
        addbartext(&mut out, scheme, CPU_IRQ.packed(scheme), "/", "irq");
        addbartext(
            &mut out,
            scheme,
            CPU_SOFTIRQ.packed(scheme),
            "/",
            "soft-irq",
        );
        addbartext(&mut out, scheme, CPU_STEAL.packed(scheme), "/", "steal");
        addbartext(&mut out, scheme, CPU_GUEST.packed(scheme), "/", "guest");
        addbartext(&mut out, scheme, CPU_IOWAIT.packed(scheme), "/", "io-wait");
        addbartext(&mut out, scheme, BAR_SHADOW.packed(scheme), " ", "used%");
    } else {
        addbartext(&mut out, scheme, CPU_GUEST.packed(scheme), "/", "virt");
        addbartext(
            &mut out,
            scheme,
            BAR_SHADOW.packed(scheme),
            "                             ",
            "used%",
        );
    }
    addattrstr(&mut out, BAR_BORDER.packed(scheme), "]");

    // C: attrset(CRT_colors[DEFAULT_COLOR]); mvaddstr(line++, 0, "Memory bar:    ");
    Ncurses::attrset(&mut out, DEFAULT_COLOR.packed(scheme));
    Ncurses::mvaddstr(&mut out, line, 0, "Memory bar:    ");
    line += 1;
    addattrstr(&mut out, BAR_BORDER.packed(scheme), "[");
    // memory classes are OS-specific; ideal bar length 56 chars, pad to 45.
    let mut barTxtLen: i32 = 0;
    for i in 0..Platform_numberOfMemoryClasses {
        // skip reclaimable cache classes if "show cached memory" is off
        if !showCachedMemory && Platform_memoryClasses[i].countsAsCache {
            continue;
        }
        // skip the available-memory class (special case for the Linux platform)
        if !Platform_memoryClasses[i].countsAsUsed && !Platform_memoryClasses[i].countsAsCache {
            continue;
        }
        addbartext(
            &mut out,
            scheme,
            Platform_memoryClasses[i].color.packed(scheme),
            if i == 0 { "" } else { "/" },
            Platform_memoryClasses[i].label,
        );
        barTxtLen += (if i == 0 { 0 } else { 1 }) + Platform_memoryClasses[i].label.len() as i32;
    }
    for _ in barTxtLen..45 {
        addattrstr(&mut out, BAR_SHADOW.packed(scheme), " "); // pad to 45 chars if necessary
    }
    addbartext(&mut out, scheme, BAR_SHADOW.packed(scheme), " ", "used");
    addbartext(&mut out, scheme, BAR_SHADOW.packed(scheme), "/", "total");
    addattrstr(&mut out, BAR_BORDER.packed(scheme), "]");

    // C: attrset(CRT_colors[DEFAULT_COLOR]); mvaddstr(line++, 0, "Swap bar:      ");
    Ncurses::attrset(&mut out, DEFAULT_COLOR.packed(scheme));
    Ncurses::mvaddstr(&mut out, line, 0, "Swap bar:      ");
    line += 1;
    addattrstr(&mut out, BAR_BORDER.packed(scheme), "[");
    addbartext(&mut out, scheme, SWAP.packed(scheme), "", "used");
    #[cfg(target_os = "linux")]
    {
        use crate::ported::crt::ColorElements::{SWAP_CACHE, SWAP_FRONTSWAP};
        addbartext(&mut out, scheme, SWAP_CACHE.packed(scheme), "/", "cache");
        addbartext(
            &mut out,
            scheme,
            SWAP_FRONTSWAP.packed(scheme),
            "/",
            "frontswap",
        );
    }
    #[cfg(not(target_os = "linux"))]
    {
        addbartext(
            &mut out,
            scheme,
            BAR_SHADOW.packed(scheme),
            "                ",
            "",
        );
    }
    addbartext(
        &mut out,
        scheme,
        BAR_SHADOW.packed(scheme),
        "                          ",
        "used",
    );
    addbartext(&mut out, scheme, BAR_SHADOW.packed(scheme), "/", "total");
    addattrstr(&mut out, BAR_BORDER.packed(scheme), "]");

    // C: line++;
    line += 1;

    // C: attrset(CRT_colors[DEFAULT_COLOR]);
    //    mvaddstr(line++, 0, "Type and layout of header meters ...");
    Ncurses::attrset(&mut out, DEFAULT_COLOR.packed(scheme));
    Ncurses::mvaddstr(
        &mut out,
        line,
        0,
        "Type and layout of header meters are configurable in the setup screen.",
    );
    line += 1;
    // C: if (CRT_colorScheme == COLORSCHEME_MONOCHROME) { mvaddstr(line, 0, ...); }
    if scheme == ColorScheme::COLORSCHEME_MONOCHROME {
        Ncurses::mvaddstr(
            &mut out,
            line,
            0,
            "In monochrome, meters display as different chars, in order: |#*@$%&.",
        );
    }
    // C: line++;
    line += 1;

    // C: mvaddstr(line, 0, "Process state: ");  (note: no line++ here)
    Ncurses::mvaddstr(&mut out, line, 0, "Process state: ");
    addattrstatestr(
        &mut out,
        scheme,
        PROCESS_RUN_STATE.packed(scheme),
        "R",
        "running; ",
    );
    addattrstatestr(
        &mut out,
        scheme,
        PROCESS_SHADOW.packed(scheme),
        "S",
        "sleeping; ",
    );
    addattrstatestr(
        &mut out,
        scheme,
        PROCESS_RUN_STATE.packed(scheme),
        "t",
        "traced/stopped; ",
    );
    addattrstatestr(
        &mut out,
        scheme,
        PROCESS_D_STATE.packed(scheme),
        "Z",
        "zombie; ",
    );
    addattrstatestr(
        &mut out,
        scheme,
        PROCESS_D_STATE.packed(scheme),
        "D",
        "disk sleep",
    );
    Ncurses::attrset(&mut out, DEFAULT_COLOR.packed(scheme));

    // C: line += 2;
    line += 2;

    // C: const bool readonly = Settings_isReadonly();
    let readonly = Settings_isReadonly();

    // C: for (item = 0; helpLeft[item].key; item++) { ... }
    for item in 0..helpLeft.len() {
        let entry = &helpLeft[item];
        let shadowed = entry.roInactive && readonly;
        let y = line + item as i32;
        Ncurses::attrset(
            &mut out,
            if shadowed { HELP_SHADOW } else { DEFAULT_COLOR }.packed(scheme),
        );
        Ncurses::mvaddstr(&mut out, y, 10, entry.info);
        Ncurses::attrset(
            &mut out,
            if shadowed { HELP_SHADOW } else { HELP_BOLD }.packed(scheme),
        );
        Ncurses::mvaddstr(&mut out, y, 1, entry.key);
        if entry.key == "      H: " {
            Ncurses::attrset(
                &mut out,
                if shadowed {
                    HELP_SHADOW
                } else {
                    PROCESS_THREAD
                }
                .packed(scheme),
            );
            Ncurses::mvaddstr(&mut out, y, 33, "threads");
        } else if entry.key == "      K: " {
            Ncurses::attrset(
                &mut out,
                if shadowed {
                    HELP_SHADOW
                } else {
                    PROCESS_THREAD
                }
                .packed(scheme),
            );
            Ncurses::mvaddstr(&mut out, y, 27, "threads");
        }
    }
    let leftHelpItems = helpLeft.len() as i32;

    // C: for (item = 0; helpRight[item].key; item++) { ... }
    for item in 0..helpRight.len() {
        let entry = &helpRight[item];
        let shadowed = entry.roInactive && readonly;
        let y = line + item as i32;
        Ncurses::attrset(
            &mut out,
            if shadowed { HELP_SHADOW } else { HELP_BOLD }.packed(scheme),
        );
        Ncurses::mvaddstr(&mut out, y, 43, entry.key);
        Ncurses::attrset(
            &mut out,
            if shadowed { HELP_SHADOW } else { DEFAULT_COLOR }.packed(scheme),
        );
        Ncurses::mvaddstr(&mut out, y, 52, entry.info);
    }
    // C: line += MAXIMUM(leftHelpItems, item);
    line += leftHelpItems.max(helpRight.len() as i32);
    // C: line++;
    line += 1;

    // htoprs extension: htop's `helpLeft`/`helpRight` cover only the ported
    // bindings, so append the htoprs-original hotkeys (theme overlay + bar
    // style + monitoring layer) here — otherwise this legend hides features the
    // shell actually supports.
    Ncurses::attrset(&mut out, HELP_BOLD.packed(scheme));
    Ncurses::mvaddstr(&mut out, line, 0, "htoprs extras (not in upstream htop):");
    line += 1;
    Ncurses::attrset(&mut out, DEFAULT_COLOR.packed(scheme));
    Ncurses::mvaddstr(
        &mut out,
        line,
        2,
        "b bar style   B border   g header   c/C theme chooser/editor",
    );
    line += 1;
    Ncurses::mvaddstr(
        &mut out,
        line,
        2,
        "f find   r filter   d snapshot   o export   A alerts   G graph   v sparkline",
    );
    line += 1;
    line += 1;

    // C: attrset(CRT_colors[HELP_BOLD]); mvaddstr(line++, 0, "Press any key to return.");
    Ncurses::attrset(&mut out, HELP_BOLD.packed(scheme));
    Ncurses::mvaddstr(&mut out, line, 0, "Press any key to return.");
    line += 1;
    let _ = line; // final post-increment value is unused (matches C)
                  // C: attrset(CRT_colors[DEFAULT_COLOR]); refresh();
    Ncurses::attrset(&mut out, DEFAULT_COLOR.packed(scheme));
    Ncurses::refresh(&mut out);
    // C: CRT_readKey(); clear();
    CRT_readKey();
    Ncurses::clear(&mut out);
    Ncurses::refresh(&mut out);

    // C: return HTOP_RECALCULATE | HTOP_REDRAW_BAR | HTOP_KEEP_FOLLOWING;
    HTOP_RECALCULATE | HTOP_REDRAW_BAR | HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionUntagAll(State* st)` from
/// `Action.c:894`. Clears the `tag` flag on every row of the main panel and
/// requests a refresh. The C `(Row*)Panel_get(...)` cast becomes a per-index
/// `as_row_mut()` upcast of the panel's items (panel items are platform
/// `Process` objects, so an exact-type `Any` downcast to `Row` would miss).
pub fn actionUntagAll(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition).
    let panel = unsafe { &mut (*st.mainPanel).super_ };
    let size = Panel_size(panel);
    for i in 0..size {
        let obj: &mut dyn Object = panel.items[i as usize].object_mut();
        // mainPanel items are platform `Process` objects; clear the tag on the
        // embedded `Row` via `as_row_mut()`. An `Any` downcast to `Row` would
        // miss every item (exact-type check) and untag nothing.
        if let Some(row) = obj.as_row_mut() {
            row.tag = false;
        }
    }
    HTOP_REFRESH
}

/// Port of `static Htop_Reaction actionTagAllChildren(State* st)` from
/// `Action.c:902`. Tags the selected row and all its descendants via
/// [`tagAllChildren`]; a no-op when the panel has no selectable row (the C
/// `if (!row) return HTOP_OK`). The C `Row* row` (the selected element) is
/// identified by its panel index, matching [`tagAllChildren`]'s ported shape.
pub fn actionTagAllChildren(st: &mut State) -> Htop_Reaction {
    // SAFETY: st->mainPanel is a valid, non-null MainPanel* (C precondition).
    let panel = unsafe { &mut (*st.mainPanel).super_ };
    let sel = panel.selected;
    if sel < 0 || sel as usize >= panel.items.len() {
        return HTOP_OK;
    }
    tagAllChildren(panel, sel);
    HTOP_OK
}

/// Port of `static Htop_Reaction actionShowEnvScreen(State* st)` from
/// `Action.c:911`. Opens the selected process's `EnvScreen` modally: build it
/// from the selected `Process`, run the [`InfoScreen_run`] loop, tear it down,
/// wipe the screen, and re-enable the input delay. Returns
/// `HTOP_REFRESH | HTOP_REDRAW_BAR`.
pub fn actionShowEnvScreen(st: &mut State) -> Htop_Reaction {
    // C: if (!Action_readableProcess(st)) return HTOP_OK;
    if !Action_readableProcess(st) {
        return HTOP_OK;
    }
    // C: Process* p = (Process*)Panel_getSelected((Panel*)st->mainPanel);
    //    if (!p) return HTOP_OK;
    let mainpanel = st.mainPanel;
    if mainpanel.is_null() {
        return HTOP_OK;
    }
    // C: EnvScreen* es = EnvScreen_new(p);
    // The `&Process` borrow of the panel ends with this block; `es` keeps only
    // the raw back-pointer into the table-owned process (valid for the modal
    // run, which does not rescan the table — the same lifetime C relies on).
    let mut es = {
        // SAFETY: `mainPanel` is the process panel wired at startup.
        let panel = unsafe { &(*mainpanel).super_ };
        let p = match Panel_getSelected(panel).and_then(|o| o.as_process()) {
            Some(p) => p,
            None => return HTOP_OK,
        };
        EnvScreen_new(p)
    };
    // C: InfoScreen_run((InfoScreen*)es);
    InfoScreen_run(&mut es);
    // C: EnvScreen_delete((Object*)es);
    EnvScreen_delete(es);
    // C: clear(); CRT_enableDelay();
    let mut out = std::io::stdout().lock();
    Ncurses::clear(&mut out);
    Ncurses::refresh(&mut out);
    CRT_enableDelay();
    HTOP_REFRESH | HTOP_REDRAW_BAR
}

/// TODO: port of `static Htop_Reaction actionShowCommandScreen(State* st)` from
/// `Action.c:929`. Opens the selected process's `CommandScreen` modally.
/// Blocked on the same ncurses substrate as [`actionLsof`]: `clear()` is
/// unported and `CommandScreen` does not implement `InfoScreenClass`.
pub fn actionShowCommandScreen(st: &mut State) -> Htop_Reaction {
    // C: if (!Action_readableProcess(st)) return HTOP_OK;
    if !Action_readableProcess(st) {
        return HTOP_OK;
    }
    // C: Process* p = (Process*)Panel_getSelected((Panel*)st->mainPanel);
    let mainpanel = st.mainPanel;
    if mainpanel.is_null() {
        return HTOP_OK;
    }
    // C: CommandScreen* cmdScr = CommandScreen_new(p);  (takes `const Process*`)
    let mut cmd_scr = {
        // SAFETY: `mainPanel` is the process panel wired at startup.
        let panel = unsafe { &(*mainpanel).super_ };
        let p = match Panel_getSelected(panel).and_then(|o| o.as_process()) {
            Some(p) => p as *const Process,
            None => return HTOP_OK,
        };
        CommandScreen_new(p)
    };
    // C: InfoScreen_run((InfoScreen*)cmdScr);
    InfoScreen_run(&mut cmd_scr);
    // C: CommandScreen_delete((Object*)cmdScr);
    CommandScreen_delete(cmd_scr);
    // C: clear(); CRT_enableDelay();
    let mut out = std::io::stdout().lock();
    Ncurses::clear(&mut out);
    Ncurses::refresh(&mut out);
    CRT_enableDelay();
    HTOP_REFRESH | HTOP_REDRAW_BAR
}

/// Port of `void Action_setBindings(Htop_Action* keys)` from `Action.c:947`.
/// Fills the keypress → handler dispatch table: `keys[code] = actionX`. The C
/// `Htop_Action* keys` is a `KEY_MAX`-length array, modeled as the borrowed
/// `&mut [Option<Htop_Action>]` slice (`MainPanel_new` allocates it via
/// `vec![None; KEY_MAX]`, the analog of `xCalloc(KEY_MAX, sizeof(Htop_Action))`
/// whose zeroed entries are `NULL`/`None`).
///
/// Char codes use `b'x' as usize` (the C `keys['x']`); `'\014'` (Ctrl+L) is
/// `0o14`, `'\177'` (DEL) is `0o177`, `'\t'` is `b'\t'`. `KEY_F(n)` /
/// `KEY_RECLICK` / `KEY_SHIFT_TAB` come from `crt.rs`.
///
/// Two bindings are gated out to match the darwin-first (non-Linux) build:
/// `keys['Y'] = actionSetSchedPolicy` (`#ifdef SCHEDULER_SUPPORT`) and
/// `keys['b'] = actionBacktrace` (`#if defined(HAVE_BACKTRACE_SCREEN)`), neither
/// defined on the target platform.
pub fn Action_setBindings(keys: &mut [Option<Htop_Action>]) {
    keys[b' ' as usize] = Some(actionTag);
    keys[b'#' as usize] = Some(actionToggleHideMeters);
    keys[b'*' as usize] = Some(actionExpandOrCollapseAllBranches);
    keys[b'+' as usize] = Some(actionExpandOrCollapse);
    keys[b',' as usize] = Some(actionSetSortColumn);
    keys[b'-' as usize] = Some(actionExpandOrCollapse);
    keys[b'.' as usize] = Some(actionSetSortColumn);
    keys[b'/' as usize] = Some(actionIncSearch);
    keys[b'<' as usize] = Some(actionSetSortColumn);
    keys[b'=' as usize] = Some(actionExpandOrCollapse);
    keys[b'>' as usize] = Some(actionSetSortColumn);
    keys[b'?' as usize] = Some(actionHelp);
    keys[b'C' as usize] = Some(actionSetup);
    keys[b'F' as usize] = Some(Action_follow);
    keys[b'H' as usize] = Some(actionToggleUserlandThreads);
    keys[b'I' as usize] = Some(actionInvertSortOrder);
    keys[b'K' as usize] = Some(actionToggleKernelThreads);
    keys[b'M' as usize] = Some(actionSortByMemory);
    keys[b'N' as usize] = Some(actionSortByPID);
    keys[b'O' as usize] = Some(actionToggleRunningInContainer);
    keys[b'P' as usize] = Some(actionSortByCPU);
    keys[b'S' as usize] = Some(actionSetup);
    keys[b'T' as usize] = Some(actionSortByTime);
    keys[b'U' as usize] = Some(actionUntagAll);
    // #ifdef SCHEDULER_SUPPORT  — not defined on the darwin-first target:
    //    keys['Y'] = actionSetSchedPolicy;
    keys[b'Z' as usize] = Some(actionTogglePauseUpdate);
    keys[b'[' as usize] = Some(actionLowerPriority);
    keys[0o14] = Some(actionRedraw); // '\014' — Ctrl+L
    keys[0o177] = Some(actionCollapseIntoParent); // '\177' — DEL
    keys[b'\\' as usize] = Some(actionIncFilter);
    keys[b']' as usize] = Some(actionHigherPriority);
    keys[b'a' as usize] = Some(actionSetAffinity);
    // Upstream `#if defined(HAVE_BACKTRACE_SCREEN)` binds keys['b'] =
    // actionBacktrace, but that is not defined on the darwin-first target, so
    // htoprs repurposes the free 'b' slot for the bar fill-glyph cycler (ported
    // from storageshower) — an htoprs-original handler living in src/extensions
    // (same extension-hook pattern the ScreenManager uses for the theme system).
    keys[b'b' as usize] = Some(crate::extensions::barstyle::cycle_bar_style);
    keys[b'c' as usize] = Some(actionTagAllChildren);
    keys[b'e' as usize] = Some(actionShowEnvScreen);
    keys[b'h' as usize] = Some(actionHelp);
    keys[b'k' as usize] = Some(actionKill);
    keys[b'l' as usize] = Some(actionLsof);
    keys[b'm' as usize] = Some(actionToggleMergedCommand);
    keys[b'p' as usize] = Some(actionToggleProgramPath);
    keys[b'q' as usize] = Some(actionQuit);
    keys[b's' as usize] = Some(actionStrace);
    keys[b't' as usize] = Some(actionToggleTreeView);
    keys[b'u' as usize] = Some(actionFilterByUser);
    keys[b'w' as usize] = Some(actionShowCommandScreen);
    keys[b'x' as usize] = Some(actionShowLocks);
    keys[KEY_F(1) as usize] = Some(actionHelp);
    keys[KEY_F(2) as usize] = Some(actionSetup);
    keys[KEY_F(3) as usize] = Some(actionIncSearch);
    keys[KEY_F(4) as usize] = Some(actionIncFilter);
    keys[KEY_F(5) as usize] = Some(actionToggleTreeView);
    keys[KEY_F(6) as usize] = Some(actionSetSortColumn);
    keys[KEY_F(7) as usize] = Some(actionHigherPriority);
    keys[KEY_F(8) as usize] = Some(actionLowerPriority);
    keys[KEY_F(9) as usize] = Some(actionKill);
    keys[KEY_F(10) as usize] = Some(actionQuit);
    keys[KEY_F(18) as usize] = Some(actionExpandCollapseOrSortColumn);
    keys[KEY_RECLICK as usize] = Some(actionExpandOrCollapse);
    keys[KEY_SHIFT_TAB as usize] = Some(actionPrevScreen);
    keys[b'\t' as usize] = Some(actionNextScreen);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::panel::{Panel_add, Panel_get, Panel_getSelectedIndex, Panel_new};
    use crate::ported::row::Row;

    /// A `Row` with the given `id`/`group`/`parent`, all else defaulted.
    /// Mirrors the fields `expandCollapse`/`collapseIntoParent` read.
    fn row(id: i32, group: i32, parent: i32) -> Row {
        Row {
            id,
            group,
            parent,
            ..Row::default()
        }
    }

    /// Read a panel item's `showChildren` via the ported `Panel_get`
    /// (`&dyn Object`) upcast to `&dyn Any` (the proven `object.rs`
    /// coercion) then downcast to `Row`.
    fn show_children_at(p: &Panel, i: i32) -> bool {
        let obj: &dyn Object = Panel_get(p, i);
        let any: &dyn core::any::Any = obj;
        any.downcast_ref::<Row>().unwrap().showChildren
    }

    /// `expandCollapse` flips the selected row's `showChildren` and
    /// returns `true` (`Action.c:148-155`).
    #[test]
    fn expand_collapse_toggles_selected_show_children() {
        let mut p = Panel_new(0, 0, 0, 0, None);
        Panel_add(&mut p, Box::new(row(1, 1, 0)));
        Panel_add(&mut p, Box::new(row(2, 2, 0)));
        Panel_setSelected(&mut p, 1); // select the second row

        // showChildren starts false (Row::default); first call sets it true.
        assert!(expandCollapse(&mut p));
        assert!(show_children_at(&p, 1));
        // The unselected row is untouched.
        assert!(!show_children_at(&p, 0));

        // Second call flips it back to false.
        assert!(expandCollapse(&mut p));
        assert!(!show_children_at(&p, 1));
    }

    /// `expandCollapse` on an empty panel returns `false` (the C
    /// `if (!row) return false`).
    #[test]
    fn expand_collapse_empty_panel_returns_false() {
        let mut p = Panel_new(0, 0, 0, 0, None);
        assert!(!expandCollapse(&mut p));
    }

    /// `collapseIntoParent` finds the selected row's parent, clears its
    /// `showChildren`, moves the selection there, and returns `true`
    /// (`Action.c:157-172`).
    #[test]
    fn collapse_into_parent_finds_parent_and_moves_selection() {
        let mut p = Panel_new(0, 0, 0, 0, None);
        // Parent (id 10, its own group => getGroupOrParent yields parent 0),
        // then a child whose group points at 10.
        let mut parent = row(10, 10, 0);
        parent.showChildren = true;
        Panel_add(&mut p, Box::new(parent)); // index 0
        Panel_add(&mut p, Box::new(row(11, 10, 10))); // index 1: child of 10
        Panel_setSelected(&mut p, 1); // select the child

        assert!(collapseIntoParent(&mut p));
        // Selection moved to the parent (index 0) and its children collapsed.
        assert_eq!(Panel_getSelectedIndex(&p), 0);
        assert!(!show_children_at(&p, 0));
    }

    /// `collapseIntoParent` returns `false` when no row matches the
    /// group-or-parent id (the C loop falls through).
    #[test]
    fn collapse_into_parent_no_match_returns_false() {
        let mut p = Panel_new(0, 0, 0, 0, None);
        // Selected row's group (99) matches no row's id in the panel.
        Panel_add(&mut p, Box::new(row(1, 99, 0)));
        Panel_add(&mut p, Box::new(row(2, 2, 0)));
        Panel_setSelected(&mut p, 0);
        assert!(!collapseIntoParent(&mut p));
    }

    /// `collapseIntoParent` on an empty panel returns `false`.
    #[test]
    fn collapse_into_parent_empty_panel_returns_false() {
        let mut p = Panel_new(0, 0, 0, 0, None);
        assert!(!collapseIntoParent(&mut p));
    }

    /// Pins the `Htop_Reaction` composite values from `Action.h:24-30`.
    /// These composites are OR-of-other-members, so a wrong base value
    /// silently changes every handler's return — pin them explicitly.
    #[test]
    fn reaction_flag_composites_match_c() {
        assert_eq!(HTOP_OK, 0x00);
        assert_eq!(HTOP_REFRESH, 0x01);
        assert_eq!(HTOP_RECALCULATE, 0x03); // 0x02 | 0x01
        assert_eq!(HTOP_QUIT, 0x10);
        assert_eq!(HTOP_UPDATE_PANELHDR, 0x41); // 0x40 | 0x01
                                                // 0x80 | 0x01 | 0x20 | (0x40 | 0x01)
        assert_eq!(HTOP_RESIZE, 0xE1);
    }

    /// `actionQuit` ignores its `ATTR_UNUSED` argument and returns
    /// `HTOP_QUIT` (`Action.c:439-441`).
    #[test]
    fn action_quit_returns_htop_quit() {
        let mut st = State {
            host: core::ptr::null_mut(),
            mainPanel: core::ptr::null_mut(),
            header: core::ptr::null_mut(),
            failedUpdate: None,
            pauseUpdate: false,
            hideSelection: false,
            hideMeters: false,
        };
        assert_eq!(actionQuit(&mut st), HTOP_QUIT);
        assert_eq!(actionQuit(&mut st), 0x10);
    }

    /// `actionBacktrace` returns early (`HTOP_OK`) on a null `mainPanel` before
    /// building the process list or entering the modal `ScreenManager_run` — the
    /// only headless-drivable path (the modal body follows the `actionStrace`
    /// precedent and is verified by primary-source reading).
    #[test]
    fn action_backtrace_null_mainpanel_is_a_noop() {
        let mut st = State {
            host: core::ptr::null_mut(),
            mainPanel: core::ptr::null_mut(),
            header: core::ptr::null_mut(),
            failedUpdate: None,
            pauseUpdate: false,
            hideSelection: false,
            hideMeters: false,
        };
        assert_eq!(actionBacktrace(&mut st), HTOP_OK);
    }

    /// `actionToggleHideMeters` flips `hideMeters` and returns
    /// `HTOP_RESIZE | HTOP_KEEP_FOLLOWING` (`Action.c:300-303`).
    #[test]
    fn action_toggle_hide_meters_flips_and_returns_resize() {
        let mut st = State {
            host: core::ptr::null_mut(),
            mainPanel: core::ptr::null_mut(),
            header: core::ptr::null_mut(),
            failedUpdate: None,
            pauseUpdate: false,
            hideSelection: false,
            hideMeters: false,
        };
        let r = actionToggleHideMeters(&mut st);
        assert!(st.hideMeters);
        assert_eq!(r, 0xE1 | 0x08); // HTOP_RESIZE | HTOP_KEEP_FOLLOWING
                                    // Second toggle returns to the original state (pure boolean flip).
        let r2 = actionToggleHideMeters(&mut st);
        assert!(!st.hideMeters);
        assert_eq!(r2, r);
        // Only hideMeters is touched; the other fields are untouched.
        assert!(!st.pauseUpdate);
        assert!(!st.hideSelection);
    }

    /// `actionTogglePauseUpdate` flips `pauseUpdate` and returns
    /// `HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_KEEP_FOLLOWING`
    /// (`Action.c:681-684`).
    #[test]
    fn action_toggle_pause_update_flips_and_returns_refresh() {
        let mut st = State {
            host: core::ptr::null_mut(),
            mainPanel: core::ptr::null_mut(),
            header: core::ptr::null_mut(),
            failedUpdate: None,
            pauseUpdate: false,
            hideSelection: false,
            hideMeters: false,
        };
        let r = actionTogglePauseUpdate(&mut st);
        assert!(st.pauseUpdate);
        assert_eq!(r, 0x01 | 0x20 | 0x08);
        let r2 = actionTogglePauseUpdate(&mut st);
        assert!(!st.pauseUpdate);
        assert_eq!(r2, r);
        // hideMeters must not be affected by the pause toggle.
        assert!(!st.hideMeters);
    }
}
