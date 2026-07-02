//! Port of `Machine.c` — htop's per-host state: the sample timers, CPU
//! counts, memory totals, the users table, and the set of `Table`s
//! (screens) plus the process table.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Ported (platform-independent)
//!
//! - [`Machine_addTable`] (`Machine.c:63`) — dedup-append a table.
//! - [`Machine_populateTablesFromSettings`] (`Machine.c:76`) — default
//!   each screen's table to the process table, record the first as
//!   active, and register each.
//!
//! # Struct model
//!
//! htop's `Machine` (`Machine.h:42`) is a large platform struct. The
//! fields the ported logic (here and in `table.rs`) reads are modeled
//! with their real types; the rest are modeled for layout so the
//! `linux/` scan layer can fill them:
//!
//! - `settings` — `Option<Settings>`; C's `Settings*` back-pointer. The
//!   `Table` port dereferences `host->settings->ss->treeView`,
//!   `->highlightChanges`, and `->highlightDelaySecs`, so [`Settings`]
//!   models exactly those plus the `screens` array
//!   `populateTablesFromSettings` walks.
//! - `monotonicMs` / `prevMonotonicMs` / `realtimeMs` — sample clocks;
//!   `Table_add` / `Table_cleanupRow` read `monotonicMs`.
//! - `tables` / `tableCount` / `activeTable` / `processTable` — the
//!   table set. Tables are opaque [`TableHandle`]s: the ported functions
//!   only compare them by identity (`this->tables[i] == table`) and
//!   never dereference them, so the pointer's identity (`usize`) is all
//!   that is needed. A null `Table*` is `None`. C tracks `tableCount`
//!   separately from the `tables` array; that is mirrored (invariant
//!   `tableCount == tables.len()`) so the dedup loop bound reads exactly
//!   like the C `for`.
//! - CPU counts, memory totals, uid/pid maxima, `usersTable`,
//!   `iterationsRemaining` — modeled for layout; not read by the ported
//!   functions. `usersTable` is an opaque handle (the `UsersTable` is
//!   not ported); `hwloc` topology is omitted.
//!
//! # Not ported (substrate-dependent)
//!
//! - [`Machine_init`] (`Machine.c:22`) — `getuid`, `Platform_getMaxPid`,
//!   `Platform_gettime_realtime`, `Row_setPidColumnWidth`, `hwloc`
//!   topology init: syscalls and unported platform/UI layers.
//! - [`Machine_done`] (`Machine.c:53`) — `hwloc_topology_destroy`,
//!   `Object_delete`, `free`: teardown of unported machinery (`Drop`
//!   releases the owned Rust fields).
//! - [`Machine_setTablesPanel`] (`Machine.c:94`) — delegates to
//!   `Table_setPanel`, wiring an ncurses `Panel` not ported.
//! - [`Machine_scanTables`] (`Machine.c:100`) — `Platform_gettime_monotonic`,
//!   the `Row_*ColumnWidth` helpers, and `Table_scanPrepare`/`_scanIterate`/
//!   `_scanCleanup` dispatch: syscalls plus unported scan machinery.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// A stand-in for htop's `Table*` — an opaque identity handle. The
/// ported functions only compare tables by pointer equality and never
/// dereference them, so the identity (`usize`) is all that is needed.
pub type TableHandle = usize;

/// The subset of htop's `ScreenSettings` (`Settings.h:42`) the ported
/// logic touches: `Table* table` (defaulted by
/// `Machine_populateTablesFromSettings`; a null `Table*` is `None`) and
/// `bool treeView` (read by `Table_updateDisplayList` via
/// `settings->ss`). The full struct carries a sort key, column list,
/// tree flags, etc. — none of which these functions read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenSettings {
    /// C `Table* table`.
    pub table: Option<TableHandle>,
    /// C `bool treeView`.
    pub treeView: bool,
}

/// The subset of htop's `Settings` (`Settings.h`) the ported logic
/// touches: the active screen `ss`, the `highlightChanges` /
/// `highlightDelaySecs` flags read by `Table_cleanupRow`, and the
/// `screens` array walked by `Machine_populateTablesFromSettings` (C's
/// `size_t nScreens` is `screens.len()`). The real `Settings` holds
/// meters, colour scheme, and many more flags, none read here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings {
    /// C `bool highlightChanges`.
    pub highlightChanges: bool,
    /// C `int highlightDelaySecs`.
    pub highlightDelaySecs: i32,
    /// C `bool showCPUFrequency` — gates `LinuxMachine_scanCPUFrequency`
    /// in `Machine_scan`.
    pub showCPUFrequency: bool,
    /// C `ScreenSettings* ss` — the active screen settings.
    pub ss: ScreenSettings,
    /// C `ScreenSettings** screens` (+ `size_t nScreens`).
    pub screens: Vec<ScreenSettings>,
}

/// Port of htop's `struct Machine_` (`Machine.h:42`). See the module
/// docs for which fields are read by the ported logic and which are
/// modeled for layout only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Machine {
    /// C `struct Settings_* settings`.
    pub settings: Option<Settings>,

    /// C `uint64_t realtimeMs` — current sample time (ms).
    pub realtimeMs: u64,
    /// C `uint64_t monotonicMs` — current sample time from the monotonic
    /// clock; read by `Table_add` / `Table_cleanupRow`.
    pub monotonicMs: u64,
    /// C `uint64_t prevMonotonicMs` — previous scan's monotonic time.
    pub prevMonotonicMs: u64,

    /// C `int64_t iterationsRemaining`.
    pub iterationsRemaining: i64,

    /// C `memory_t totalMem` (memory_t == unsigned long long).
    pub totalMem: u64,
    /// C `memory_t totalSwap`.
    pub totalSwap: u64,
    /// C `memory_t usedSwap`.
    pub usedSwap: u64,
    /// C `memory_t cachedSwap`.
    pub cachedSwap: u64,

    /// C `unsigned int activeCPUs`.
    pub activeCPUs: u32,
    /// C `unsigned int existingCPUs`.
    pub existingCPUs: u32,

    /// C `UsersTable* usersTable` — opaque handle (the `UsersTable` is
    /// not ported).
    pub usersTable: Option<usize>,
    /// C `uid_t htopUserId`.
    pub htopUserId: u32,
    /// C `uid_t maxUserId` — recently observed maximum.
    pub maxUserId: u32,
    /// C `uid_t userId` — selected row user id.
    pub userId: u32,

    /// C `pid_t maxProcessId` — largest PID seen at runtime.
    pub maxProcessId: i32,

    /// C `size_t tableCount` — mirrored explicitly (invariant
    /// `tableCount == tables.len()`) so the dedup loop bound matches C.
    pub tableCount: usize,
    /// C `Table** tables` — the registered table set (opaque handles).
    pub tables: Vec<TableHandle>,
    /// C `Table* activeTable`.
    pub activeTable: Option<TableHandle>,
    /// C `Table* processTable`.
    pub processTable: Option<TableHandle>,
}

/// TODO: port of `void Machine_init(Machine* this, UsersTable*
/// usersTable, uid_t userId)` from `Machine.c:22`. Needs `getuid`,
/// `Platform_getMaxPid`, `Platform_gettime_realtime`,
/// `Row_setPidColumnWidth`, and `hwloc` topology init.
pub fn Machine_init() {
    todo!("port of Machine.c:22 — needs getuid/Platform_*/hwloc")
}

/// TODO: port of `void Machine_done(Machine* this)` from `Machine.c:53`.
/// Needs `hwloc_topology_destroy` / `Object_delete` / `free` (Rust `Drop`
/// releases the owned fields).
pub fn Machine_done() {
    todo!("port of Machine.c:53 — teardown handled by Drop")
}

/// Port of `static void Machine_addTable(Machine* this, Table* table)`
/// from `Machine.c:63`. Registers `table` in `this->tables` unless it is
/// already present: the C scans `[0, tableCount)` for a pointer match and
/// returns early on a hit, otherwise `xReallocArray`-grows the array by
/// one, stores `table` in the new last slot, and bumps `tableCount`.
/// `Vec::push` performs the same grow-and-store; the explicit
/// `tableCount` bump mirrors the C.
fn Machine_addTable(this: &mut Machine, table: TableHandle) {
    /* check that this table has not been seen previously */
    for i in 0..this.tableCount {
        if this.tables[i] == table {
            return;
        }
    }

    // nmemb == this.tableCount + 1; tables[nmemb - 1] = table
    this.tables.push(table);
    this.tableCount += 1;
}

/// Port of `void Machine_populateTablesFromSettings(Machine* this,
/// Settings* settings, Table* processTable)` from `Machine.c:76`.
/// Stores `settings`/`processTable` on the machine, then for each
/// screen: if the screen has no table, default it to `processTable`;
/// record the first screen's table as `activeTable`; and register the
/// table via [`Machine_addTable`]. The C mutates `ss->table` through the
/// stored `Settings*`, so `settings` is moved into `this.settings` and
/// the defaulting mutation is applied there — mirroring the in-place
/// `ss->table = processTable`.
pub fn Machine_populateTablesFromSettings(
    this: &mut Machine,
    settings: Settings,
    processTable: TableHandle,
) {
    this.settings = Some(settings);
    this.processTable = Some(processTable);

    let nScreens = this.settings.as_ref().unwrap().screens.len();
    for i in 0..nScreens {
        // ScreenSettings* ss = settings->screens[i];
        if this.settings.as_ref().unwrap().screens[i].table.is_none() {
            this.settings.as_mut().unwrap().screens[i].table = Some(processTable);
        }

        let table = this.settings.as_ref().unwrap().screens[i].table.unwrap();
        if i == 0 {
            this.activeTable = Some(table);
        }

        Machine_addTable(this, table);
    }
}

/// TODO: port of `void Machine_setTablesPanel(Machine* this, Panel*
/// panel)` from `Machine.c:94`. Delegates to `Table_setPanel`, wiring an
/// ncurses `Panel` not ported.
pub fn Machine_setTablesPanel() {
    todo!("port of Machine.c:94 — needs ncurses Panel")
}

/// TODO: port of `void Machine_scanTables(Machine* this)` from
/// `Machine.c:100`. Needs `Platform_gettime_monotonic`, the
/// `Row_*ColumnWidth` helpers, and the `Table_scanPrepare`/`_scanIterate`/
/// `_scanCleanup` dispatch (platform scan machinery).
pub fn Machine_scanTables() {
    todo!("port of Machine.c:100 — needs Platform scan + Table scan dispatch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addTable_dedup_keeps_first_occurrence_and_ignores_repeats() {
        let mut m = Machine::default();

        Machine_addTable(&mut m, 10);
        Machine_addTable(&mut m, 20);
        // Re-adding an already-registered table is a no-op (C returns
        // early on pointer match).
        Machine_addTable(&mut m, 10);
        Machine_addTable(&mut m, 20);
        Machine_addTable(&mut m, 30);

        assert_eq!(m.tables, vec![10, 20, 30]);
        assert_eq!(m.tableCount, 3);
        // Invariant the C maintains: tableCount tracks the array length.
        assert_eq!(m.tableCount, m.tables.len());
    }

    #[test]
    fn addTable_first_insert_grows_from_empty() {
        let mut m = Machine::default();
        assert_eq!(m.tableCount, 0);
        assert!(m.tables.is_empty());

        Machine_addTable(&mut m, 42);

        assert_eq!(m.tables, vec![42]);
        assert_eq!(m.tableCount, 1);
    }

    /// Helper: a `Settings` with `nScreens` empty (null-table) screens.
    fn settings_with_screens(n: usize) -> Settings {
        Settings {
            screens: vec![ScreenSettings::default(); n],
            ..Default::default()
        }
    }

    #[test]
    fn populate_defaults_null_tables_to_processTable() {
        // Two screens, both with no table (null Table*): each must be
        // defaulted to processTable in place, and — since both then equal
        // processTable — only one entry is registered.
        let mut m = Machine::default();

        Machine_populateTablesFromSettings(&mut m, settings_with_screens(2), 7);

        let s = m.settings.as_ref().unwrap();
        assert_eq!(s.screens[0].table, Some(7));
        assert_eq!(s.screens[1].table, Some(7));

        assert_eq!(m.processTable, Some(7));
        assert_eq!(m.activeTable, Some(7)); // first screen's table
        assert_eq!(m.tables, vec![7]); // deduped
        assert_eq!(m.tableCount, 1);
    }

    #[test]
    fn populate_first_screen_becomes_active_and_explicit_tables_are_kept() {
        // First screen has an explicit table (not overwritten); second is
        // null (defaulted to processTable); third repeats the first
        // (deduped away).
        let settings = Settings {
            screens: vec![
                ScreenSettings {
                    table: Some(100),
                    treeView: false,
                },
                ScreenSettings::default(),
                ScreenSettings {
                    table: Some(100),
                    treeView: false,
                },
            ],
            ..Default::default()
        };
        let mut m = Machine::default();

        Machine_populateTablesFromSettings(&mut m, settings, 9);

        let s = m.settings.as_ref().unwrap();
        assert_eq!(s.screens[0].table, Some(100)); // untouched
        assert_eq!(s.screens[1].table, Some(9)); // defaulted
        assert_eq!(s.screens[2].table, Some(100)); // untouched

        assert_eq!(m.activeTable, Some(100)); // first screen wins
        assert_eq!(m.tables, vec![100, 9]); // 100 registered once, 9 next
        assert_eq!(m.tableCount, 2);
    }

    #[test]
    fn populate_with_no_screens_registers_nothing() {
        let mut m = Machine::default();

        Machine_populateTablesFromSettings(&mut m, settings_with_screens(0), 5);

        assert_eq!(m.processTable, Some(5));
        assert_eq!(m.activeTable, None); // loop never runs
        assert!(m.tables.is_empty());
        assert_eq!(m.tableCount, 0);
    }
}
