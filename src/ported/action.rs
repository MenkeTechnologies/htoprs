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
//!   `Panel*` (not `State`). They mutate the selected/parent [`Row`]'s
//!   `showChildren` via the ported [`Panel`]/[`Row`] substrate. The
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
//! - **`Panel`/`ScreenManager`/`IncSet`/`MainPanel` glue:**
//!   `Action_pickFromVector`, `Action_runSetup`/`actionSetup`,
//!   `changePriority`/`actionHigherPriority`/`actionLowerPriority`,
//!   `addUserToVector`/`actionFilterByUser`, `actionIncFilter`/
//!   `actionIncSearch`, `actionTag`/`actionUntagAll`/`actionTagAllChildren`,
//!   `actionSetSortColumn` (needs `Action_pickFromVector` + a `Panel` of
//!   `ListItem`s), `actionExpandOrCollapse`/`actionCollapseIntoParent`/
//!   `actionExpandCollapseOrSortColumn`. These reach `st->mainPanel`, which
//!   the minimal `State` does not model, and several need mutable panel
//!   accessors the ported `Panel` API does not expose.
//! - **`actionKill` (`Action.c:524`):** signal delivery is available via
//!   the crate's `nix`/`libc` deps, but the handler reaches `st->mainPanel`
//!   (`Panel_setHeader`/`Panel_draw`/`MainPanel_foreachRow`) which the
//!   minimal `State` does not model, so it stays stubbed on that ground.
//! - **Child screens (each its own unported InfoScreen subclass):**
//!   `actionLsof`, `actionStrace`, `actionShowLocks`, `actionShowEnvScreen`,
//!   `actionShowCommandScreen`, `actionBacktrace`, `actionSetAffinity`,
//!   `actionSetSchedPolicy`.
//! - **`Action_setBindings` (`Action.c:969`):** builds a dispatch table of
//!   `fn(&mut State) -> Htop_Reaction` pointers, which requires every
//!   `actionXxx` to share that signature. Most are still stubs with the
//!   scaffold `pub fn foo()` signature, so the table cannot be built yet.
//!
//! `gen_port_report.py` counts remaining `todo!()` bodies as *stubbed*,
//! not *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)] // `Htop_Reaction` mirrors the C type name verbatim
#![allow(dead_code)]

use crate::ported::header::Header;
use crate::ported::machine::{Machine, Machine_scanTables};
use crate::ported::mainpanel::MainPanel;
use crate::ported::object::Object;
use crate::ported::panel::{Panel, Panel_setSelected, Panel_size};
use crate::ported::process::ProcessField;
use crate::ported::row::{Row, Row_getGroupOrParent, Row_isChildOf};
use crate::ported::settings::{
    RowField, ScreenSettings_invertSortOrder, ScreenSettings_setSortKey, Settings,
    Settings_isReadonly,
};
use crate::ported::table::{Table_collapseAllBranches, Table_expandTree};

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

/// TODO: port of `Object* Action_pickFromVector(State* st, Panel* list, int x, bool follow` from `Action.c:59`.
pub fn Action_pickFromVector() {
    todo!("port of Action.c:59")
}

/// TODO: port of `static void Action_runSetup(State* st` from `Action.c:101`.
pub fn Action_runSetup() {
    todo!("port of Action.c:101")
}

/// TODO: port of `static bool changePriority(MainPanel* panel, int delta` from `Action.c:113`.
pub fn changePriority() {
    todo!("port of Action.c:113")
}

/// TODO: port of `static void addUserToVector(ht_key_t key, void* userCast, void* panelCast` from `Action.c:121`.
pub fn addUserToVector() {
    todo!("port of Action.c:121")
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
/// two `(Row*)` casts become scoped `Any` downcasts, keeping the borrows
/// non-overlapping while preserving the C recursion order verbatim.
pub fn tagAllChildren(panel: &mut Panel, parent_idx: i32) {
    // C `parent->tag = true; int parent_id = parent->id;`
    let parent_id = {
        let obj: &mut dyn Object = panel.items[parent_idx as usize].object_mut();
        let any: &mut dyn core::any::Any = obj;
        let parent = any
            .downcast_mut::<Row>()
            .expect("tagAllChildren operates on the mainPanel, whose items are Rows");
        parent.tag = true;
        parent.id
    };

    let size = Panel_size(panel);
    for i in 0..size {
        // C `Row* row = Panel_get(panel, i);
        //    if (!row->tag && Row_isChildOf(row, parent_id))`
        let recurse = {
            let obj: &dyn Object = panel.items[i as usize].object();
            let any: &dyn core::any::Any = obj;
            let row = any
                .downcast_ref::<Row>()
                .expect("tagAllChildren operates on the mainPanel, whose items are Rows");
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
/// selected position and downcasts to `&mut Row` through the `Any`
/// supertrait (same idiom as `columnspanel.rs`).
pub fn expandCollapse(panel: &mut Panel) -> bool {
    if panel.items.is_empty() {
        return false;
    }

    let idx = panel.selected as usize;
    let obj: &mut dyn Object = panel.items[idx].object_mut();
    let any: &mut dyn core::any::Any = obj;
    let row = any
        .downcast_mut::<Row>()
        .expect("expandCollapse operates on the mainPanel, whose items are Rows");
    row.showChildren = !row.showChildren;
    true
}

/// Port of `static bool collapseIntoParent(Panel* panel)` from
/// `Action.c:157`. Reads the selected row's group-or-parent id via
/// [`Row_getGroupOrParent`], then scans the panel for the row whose `id`
/// matches: on a hit it clears that row's `showChildren`, moves the
/// selection there via [`Panel_setSelected`], and returns `true`;
/// otherwise `false` (also `false` when the panel is empty — the C
/// `if (!r) return false`). The two `(Row*)` casts become `Any`
/// downcasts of `panel.items`; the read of the selected row (immutable)
/// is scoped before the mutating scan so the borrows never overlap.
pub fn collapseIntoParent(panel: &mut Panel) -> bool {
    if panel.items.is_empty() {
        return false;
    }

    let parent_id = {
        let obj: &dyn Object = panel.items[panel.selected as usize].object();
        let any: &dyn core::any::Any = obj;
        let r = any
            .downcast_ref::<Row>()
            .expect("collapseIntoParent operates on the mainPanel, whose items are Rows");
        Row_getGroupOrParent(r)
    };

    let size = Panel_size(panel);
    for i in 0..size {
        let id = {
            let obj: &dyn Object = panel.items[i as usize].object();
            let any: &dyn core::any::Any = obj;
            any.downcast_ref::<Row>()
                .expect("collapseIntoParent operates on the mainPanel, whose items are Rows")
                .id
        };
        if id == parent_id {
            let obj: &mut dyn Object = panel.items[i as usize].object_mut();
            let any: &mut dyn core::any::Any = obj;
            any.downcast_mut::<Row>()
                .expect("collapseIntoParent operates on the mainPanel, whose items are Rows")
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
    let readonly =
        Settings_isReadonly() || settings.screens[settings.ssIndex as usize].dynamic.is_some();
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
    settings.screens[settings.ssIndex as usize].dynamic.is_none()
}

/// TODO: port of `static Htop_Reaction actionSetSortColumn(State* st` from `Action.c:192`.
pub fn actionSetSortColumn() {
    todo!("port of Action.c:192")
}

/// Port of `static Htop_Reaction actionSortByPID(State* st)` from
/// `Action.c:227`: `Action_setSortKey(st->host->settings, PID)`.
pub fn actionSortByPID(st: &State) -> Htop_Reaction {
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
pub fn actionSortByMemory(st: &State) -> Htop_Reaction {
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
pub fn actionSortByCPU(st: &State) -> Htop_Reaction {
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
pub fn actionSortByTime(st: &State) -> Htop_Reaction {
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
pub fn actionToggleKernelThreads(st: &State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionToggleKernelThreads: host->settings is NULL")
    };
    settings.hideKernelThreads = !settings.hideKernelThreads;
    settings.lastUpdate += 1;

    Machine_scanTables(); // C: Machine_scanTables(st->host)

    HTOP_RECALCULATE | HTOP_SAVE_SETTINGS | HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionToggleUserlandThreads(State* st)`
/// from `Action.c:253`. Flips `settings->hideUserlandThreads`, bumps
/// `settings->lastUpdate`, then re-scans the tables. The
/// `Machine_scanTables(st->host)` call maps to the still-stubbed
/// [`Machine_scanTables`] (see [`actionToggleKernelThreads`]).
pub fn actionToggleUserlandThreads(st: &State) -> Htop_Reaction {
    let settings = unsafe {
        (*st.host)
            .settings
            .as_mut()
            .expect("actionToggleUserlandThreads: host->settings is NULL")
    };
    settings.hideUserlandThreads = !settings.hideUserlandThreads;
    settings.lastUpdate += 1;

    Machine_scanTables(); // C: Machine_scanTables(st->host)

    HTOP_RECALCULATE | HTOP_SAVE_SETTINGS | HTOP_KEEP_FOLLOWING
}

/// Port of `static Htop_Reaction actionToggleRunningInContainer(State* st)`
/// from `Action.c:263`. Flips `settings->hideRunningInContainer` and bumps
/// `settings->lastUpdate`.
pub fn actionToggleRunningInContainer(st: &State) -> Htop_Reaction {
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
pub fn actionToggleProgramPath(st: &State) -> Htop_Reaction {
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
pub fn actionToggleMergedCommand(st: &State) -> Htop_Reaction {
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
pub fn actionToggleTreeView(st: &State) -> Htop_Reaction {
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
        let all_collapsed =
            (*host).settings.as_ref().unwrap().screens[ssidx].allBranchesCollapsed;

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
pub fn actionExpandOrCollapseAllBranches(st: &State) -> Htop_Reaction {
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

/// TODO: port of `static Htop_Reaction actionIncFilter(State* st` from `Action.c:319`.
pub fn actionIncFilter() {
    todo!("port of Action.c:319")
}

/// TODO: port of `static Htop_Reaction actionIncSearch(State* st` from `Action.c:327`.
pub fn actionIncSearch() {
    todo!("port of Action.c:327")
}

/// TODO: port of `static Htop_Reaction actionHigherPriority(State* st` from `Action.c:333`.
pub fn actionHigherPriority() {
    todo!("port of Action.c:333")
}

/// TODO: port of `static Htop_Reaction actionLowerPriority(State* st` from `Action.c:341`.
pub fn actionLowerPriority() {
    todo!("port of Action.c:341")
}

/// Port of `static Htop_Reaction actionInvertSortOrder(State* st)` from
/// `Action.c:349`. Inverts the active screen's sort direction
/// ([`ScreenSettings_invertSortOrder`]) and marks the active table for
/// re-sort. `settings->ss` is `screens[ssIndex]`; `host->activeTable` is
/// the `*mut Table` back-pointer.
pub fn actionInvertSortOrder(st: &State) -> Htop_Reaction {
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

/// TODO: port of `static Htop_Reaction actionExpandOrCollapse(State* st` from `Action.c:356`.
pub fn actionExpandOrCollapse() {
    todo!("port of Action.c:356")
}

/// TODO: port of `static Htop_Reaction actionCollapseIntoParent(State* st` from `Action.c:364`.
pub fn actionCollapseIntoParent() {
    todo!("port of Action.c:364")
}

/// TODO: port of `static Htop_Reaction actionExpandCollapseOrSortColumn(State* st` from `Action.c:372`.
pub fn actionExpandCollapseOrSortColumn() {
    todo!("port of Action.c:372")
}

/// TODO: port of `static inline void setActiveScreen(Settings* settings, State* st, unsigned int ssIdx` from `Action.c:376`.
pub fn setActiveScreen() {
    todo!("port of Action.c:376")
}

/// TODO: port of `static Htop_Reaction actionNextScreen(State* st` from `Action.c:390`.
pub fn actionNextScreen() {
    todo!("port of Action.c:390")
}

/// TODO: port of `static Htop_Reaction actionPrevScreen(State* st` from `Action.c:400`.
pub fn actionPrevScreen() {
    todo!("port of Action.c:400")
}

/// TODO: port of `Htop_Reaction Action_setScreenTab(State* st, int x` from `Action.c:411`.
pub fn Action_setScreenTab() {
    todo!("port of Action.c:411")
}

/// Port of `static Htop_Reaction actionQuit(ATTR_UNUSED State* st)` from
/// `Action.c:439`. The `State*` argument is `ATTR_UNUSED`; the full
/// behavior is returning the `HTOP_QUIT` constant. The parameter is
/// kept (prefixed `_`) to mirror the C signature.
pub fn actionQuit(_st: &State) -> Htop_Reaction {
    HTOP_QUIT
}

/// TODO: port of `static Htop_Reaction actionSetAffinity(State* st` from `Action.c:443`.
pub fn actionSetAffinity() {
    todo!("port of Action.c:443")
}

/// TODO: port of `static Htop_Reaction actionSetSchedPolicy(State* st` from `Action.c:480`.
pub fn actionSetSchedPolicy() {
    todo!("port of Action.c:480")
}

/// TODO: port of `static Htop_Reaction actionKill(State* st` from `Action.c:524`.
pub fn actionKill() {
    todo!("port of Action.c:524")
}

/// TODO: port of `static Htop_Reaction actionFilterByUser(State* st` from `Action.c:548`.
pub fn actionFilterByUser() {
    todo!("port of Action.c:548")
}

/// TODO: port of `Htop_Reaction Action_follow(State* st` from `Action.c:568`.
pub fn Action_follow() {
    todo!("port of Action.c:568")
}

/// TODO: port of `static Htop_Reaction actionSetup(State* st` from `Action.c:574`.
pub fn actionSetup() {
    todo!("port of Action.c:574")
}

/// TODO: port of `static Htop_Reaction actionLsof(State* st` from `Action.c:579`.
pub fn actionLsof() {
    todo!("port of Action.c:579")
}

/// TODO: port of `static Htop_Reaction actionShowLocks(State* st` from `Action.c:597`.
pub fn actionShowLocks() {
    todo!("port of Action.c:597")
}

/// TODO: port of `static Htop_Reaction actionBacktrace(State *st` from `Action.c:616`.
pub fn actionBacktrace() {
    todo!("port of Action.c:616")
}

/// TODO: port of `static Htop_Reaction actionStrace(State* st` from `Action.c:644`.
pub fn actionStrace() {
    todo!("port of Action.c:644")
}

/// TODO: port of `static Htop_Reaction actionTag(State* st` from `Action.c:665`.
pub fn actionTag() {
    todo!("port of Action.c:665")
}

/// TODO: port of `static Htop_Reaction actionRedraw(ATTR_UNUSED State* st` from `Action.c:675`.
pub fn actionRedraw() {
    todo!("port of Action.c:675")
}

/// Port of `static Htop_Reaction actionTogglePauseUpdate(State* st)` from
/// `Action.c:681`. Flips the `State.pauseUpdate` flag and returns the
/// verbatim `HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_KEEP_FOLLOWING`
/// bit-or. The C touches only `st->pauseUpdate`.
pub fn actionTogglePauseUpdate(st: &mut State) -> Htop_Reaction {
    st.pauseUpdate = !st.pauseUpdate;
    HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_KEEP_FOLLOWING
}

/// TODO: port of `static inline void addattrstr( int attr, const char* str` from `Action.c:746`.
pub fn addattrstr() {
    todo!("port of Action.c:746")
}

/// TODO: port of `static Htop_Reaction actionHelp(State* st` from `Action.c:751`.
pub fn actionHelp() {
    todo!("port of Action.c:751")
}

/// TODO: port of `static Htop_Reaction actionUntagAll(State* st` from `Action.c:894`.
pub fn actionUntagAll() {
    todo!("port of Action.c:894")
}

/// TODO: port of `static Htop_Reaction actionTagAllChildren(State* st` from `Action.c:902`.
pub fn actionTagAllChildren() {
    todo!("port of Action.c:902")
}

/// TODO: port of `static Htop_Reaction actionShowEnvScreen(State* st` from `Action.c:911`.
pub fn actionShowEnvScreen() {
    todo!("port of Action.c:911")
}

/// TODO: port of `static Htop_Reaction actionShowCommandScreen(State* st` from `Action.c:929`.
pub fn actionShowCommandScreen() {
    todo!("port of Action.c:929")
}

/// TODO: port of `void Action_setBindings(Htop_Action* keys` from `Action.c:947`.
pub fn Action_setBindings() {
    todo!("port of Action.c:947")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::panel::{Panel_add, Panel_get, Panel_getSelectedIndex, Panel_new};

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
        let st = State {
            host: core::ptr::null_mut(),
            mainPanel: core::ptr::null_mut(),
            header: core::ptr::null_mut(),
            failedUpdate: None,
            pauseUpdate: false,
            hideSelection: false,
            hideMeters: false,
        };
        assert_eq!(actionQuit(&st), HTOP_QUIT);
        assert_eq!(actionQuit(&st), 0x10);
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
