//! Port of `Machine.c` — only the platform-independent table
//! bookkeeping (`Machine_addTable` and
//! `Machine_populateTablesFromSettings`).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port.
//!
//! htop's `Machine` is a large platform struct carrying timers, CPU
//! counts, memory totals, an `hwloc` topology, a `UsersTable`, and the
//! process/screen tables. Only two functions are pure logic over the
//! table set: `Machine_addTable` (dedup-append) and
//! `Machine_populateTablesFromSettings` (default each screen's table to
//! the process table, record the first as active, and register each).
//! Both port faithfully once the fields they touch are modelled as a
//! small Rust struct.
//!
//! The `Table*`/`Settings*`/`ScreenSettings*` pointers in C carry
//! identity, not content, for these two functions: `Machine_addTable`
//! compares tables by pointer equality (`this->tables[i] == table`) and
//! never dereferences them, and `populateTablesFromSettings` only reads
//! `ss->table` and reassigns it. So a `Table` is modelled as an opaque
//! `usize` handle (a stand-in for the pointer's identity), a null
//! `Table*` as `None`, and `xReallocArray`-grow-then-assign-last as a
//! `Vec::push`. `this->tableCount` is kept as an explicit field
//! (mirroring the C, which tracks it separately from the array) so the
//! dedup loop bound reads exactly like the C `for` loop.
//!
//! Not ported (and why) — all substrate-dependent:
//! - `Machine_init` (`Machine.c:22`) — `getuid`, `Platform_getMaxPid`,
//!   `Platform_gettime_realtime`, `Row_setPidColumnWidth`, `hwloc`
//!   topology init: syscalls and unported platform/UI layers.
//! - `Machine_done` (`Machine.c:53`) — `hwloc_topology_destroy`,
//!   `Object_delete`, `free`: teardown of unported machinery.
//! - `Machine_setTablesPanel` (`Machine.c:94`) — pure delegation to
//!   `Table_setPanel`, which wires a `Panel` (ncurses UI) not ported.
//! - `Machine_scanTables` (`Machine.c:100`) — `Platform_gettime_monotonic`,
//!   `Row_resetFieldWidths`/`Row_setUidColumnWidth`/`Row_setPidColumnWidth`,
//!   and `Table_scanPrepare`/`Table_scanIterate`/`Table_scanCleanup`:
//!   syscalls plus unported Row/Table scan machinery.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// A stand-in for htop's `Table*` — an opaque identity handle. These
/// two functions only compare tables by pointer equality and never
/// dereference them, so the identity (`usize`) is all that is needed.
type TableHandle = usize;

/// The subset of htop's `ScreenSettings` that
/// `Machine_populateTablesFromSettings` touches: the `Table* table`
/// field. A null `Table*` is modelled as `None`. (`ScreenSettings`
/// carries many more fields — sort key, tree view, column list, etc. —
/// none of which this function reads.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSettings {
    pub table: Option<TableHandle>,
}

/// The subset of htop's `Settings` that
/// `Machine_populateTablesFromSettings` touches: the `screens` array.
/// C's `size_t nScreens` is the array length, so it is `screens.len()`
/// here. (The real `Settings` holds meters, colour scheme, flags, etc.,
/// none read by this function.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub screens: Vec<ScreenSettings>,
}

/// The subset of htop's `Machine` that the ported functions touch:
/// the settings pointer, the process/active table handles, and the
/// deduplicated table set. C tracks `tableCount` separately from the
/// `tables` array; that is mirrored here (invariant:
/// `tableCount == tables.len()`) so the dedup loop bound matches the C.
/// (The full `Machine` also carries timers, CPU counts, memory totals,
/// an `hwloc` topology, and a `UsersTable` — none touched here.)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Machine {
    pub settings: Option<Settings>,
    pub processTable: Option<TableHandle>,
    pub activeTable: Option<TableHandle>,
    pub tables: Vec<TableHandle>,
    pub tableCount: usize,
}

/// TODO: port of `void Machine_init(Machine* this, UsersTable* usersTable, uid_t userId` from `Machine.c:22`.
pub fn Machine_init() {
    todo!("port of Machine.c:22")
}

/// TODO: port of `void Machine_done(Machine* this` from `Machine.c:53`.
pub fn Machine_done() {
    todo!("port of Machine.c:53")
}

/// Port of `static void Machine_addTable(Machine* this, Table* table)`
/// from `Machine.c:63`. Registers `table` in `this->tables` unless it is
/// already present: the C first scans `[0, tableCount)` for a pointer
/// match and returns early on a hit, otherwise `xReallocArray`-grows the
/// array by one, stores `table` in the new last slot, and bumps
/// `tableCount`. `Vec::push` performs the same grow-and-store; the
/// explicit `tableCount` bump mirrors the C.
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
/// table via `Machine_addTable`. The C mutates `ss->table` through the
/// stored `Settings*`, so ownership of `settings` is moved into
/// `this.settings` and the defaulting mutation is applied there —
/// faithfully mirroring the in-place `ss->table = processTable`.
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

/// TODO: port of `void Machine_setTablesPanel(Machine* this, Panel* panel` from `Machine.c:94`.
pub fn Machine_setTablesPanel() {
    todo!("port of Machine.c:94")
}

/// TODO: port of `void Machine_scanTables(Machine* this` from `Machine.c:100`.
pub fn Machine_scanTables() {
    todo!("port of Machine.c:100")
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

    #[test]
    fn populate_defaults_null_tables_to_processTable() {
        // Two screens, both with no table (null Table*): each must be
        // defaulted to processTable in place, and — since both then
        // equal processTable — only one entry is registered.
        let settings = Settings {
            screens: vec![
                ScreenSettings { table: None },
                ScreenSettings { table: None },
            ],
        };
        let mut m = Machine::default();

        Machine_populateTablesFromSettings(&mut m, settings, 7);

        // ss->table defaulting persisted through the stored Settings.
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
        // First screen has an explicit table (not overwritten); second
        // is null (defaulted to processTable); third repeats the first
        // (deduped away).
        let settings = Settings {
            screens: vec![
                ScreenSettings { table: Some(100) },
                ScreenSettings { table: None },
                ScreenSettings { table: Some(100) },
            ],
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
        let settings = Settings { screens: vec![] };
        let mut m = Machine::default();

        Machine_populateTablesFromSettings(&mut m, settings, 5);

        assert_eq!(m.processTable, Some(5));
        assert_eq!(m.activeTable, None); // loop never runs
        assert!(m.tables.is_empty());
        assert_eq!(m.tableCount, 0);
    }
}
