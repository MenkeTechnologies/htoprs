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
//! - `Machine_addTable` (`Machine.c:63`) — dedup-append a table.
//! - [`Machine_populateTablesFromSettings`] (`Machine.c:76`) — default
//!   each screen's table to the process table, record the first as
//!   active, and register each.
//! - [`Machine_setTablesPanel`] (`Machine.c:94`) — point every registered
//!   table at the shared main `Panel`.
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
//!   table set. Following the crate's chosen ownership model, each is a
//!   raw [`TableHandle`] (`*mut Table`) mirroring htop's `Table*` 1:1:
//!   `Machine` borrows tables it does not own. The dedup loop compares by
//!   pointer identity (`this->tables[i] == table`), and
//!   [`Machine_setTablesPanel`] dereferences each to wire its panel. Null
//!   is never stored in `tables`; the nullable `activeTable`/`processTable`
//!   use `Option<TableHandle>` (`None` = C `NULL`). C tracks `tableCount`
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
//! # Ported (Darwin scan seam)
//!
//! - [`Machine_scanTables`] (`Machine.c:100`) — the per-scan clock stamp +
//!   the table scan loop, dispatching through the ported [`TableClass`] scan
//!   vtable (`Table.h:55-57`) whose Darwin slots reach the live process scan.
//!   `macos`-gated (it samples `Platform_gettime_monotonic`).
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::generic::gettime::{Generic_gettime_monotonic, Generic_gettime_realtime};
use crate::ported::panel::Panel;
use crate::ported::row::{Row_resetFieldWidths, Row_setPidColumnWidth, Row_setUidColumnWidth};
use crate::ported::table::{
    Table, Table_scanCleanup, Table_scanIterate, Table_scanPrepare, Table_setPanel,
};

// `Machine_init` calls `Platform_getMaxPid`, resolved per active platform
// module (htop link-resolves the compiled platform's definition; here the
// `#[cfg]` selects it — one platform per build). The clock samplers are the
// generic `Generic_gettime_*` the per-platform `Platform_gettime_*` macros
// alias, so `Machine_init`/`Machine_scanTables` compile on every target.
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_getMaxPid;
#[cfg(target_os = "linux")]
use crate::ported::linux::platform::Platform_getMaxPid;
#[cfg(target_os = "freebsd")]
use crate::ported::freebsd::platform::Platform_getMaxPid;
#[cfg(target_os = "netbsd")]
use crate::ported::netbsd::platform::Platform_getMaxPid;
#[cfg(target_os = "openbsd")]
use crate::ported::openbsd::platform::Platform_getMaxPid;
#[cfg(any(target_os = "solaris", target_os = "illumos"))]
use crate::ported::solaris::platform::Platform_getMaxPid;
#[cfg(target_os = "dragonfly")]
use crate::ported::dragonflybsd::platform::Platform_getMaxPid;
#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "solaris",
    target_os = "illumos",
    target_os = "dragonfly"
)))]
use crate::ported::unsupported::platform::Platform_getMaxPid;

/// htop's `Table*` — a raw pointer to a [`Table`]. The crate mirrors
/// htop's C pointer graph 1:1 (raw-pointer ownership model): `Machine`
/// holds borrowed `Table*`s it does not own. Ported functions compare
/// them by identity and [`Machine_setTablesPanel`] dereferences them;
/// null is never stored in [`Machine::tables`], while the nullable
/// `activeTable`/`processTable`/`ScreenSettings::table` slots use
/// `Option<TableHandle>` with `None` = C `NULL`.
pub type TableHandle = *mut Table;

/// htop's `Settings` and `ScreenSettings` are a single struct each in the C
/// source; the canonical Rust port lives in [`crate::ported::settings`].
/// They are re-exported here (not re-declared) so `machine::Settings` /
/// `machine::ScreenSettings` keep resolving for the existing `Machine`
/// consumers while there is exactly one definition of each — the C's active
/// screen `ss` is `screens[ssIndex]`, so `ss` is not a separate field.
pub use crate::ported::settings::{ScreenSettings, Settings};

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

/// Port of `void Machine_init(Machine* this, UsersTable* usersTable, uid_t
/// userId)` from `Machine.c:22`. Stores the users table and selected user,
/// records the real uid (`getuid`), sets the PID column width from
/// `Platform_getMaxPid`, and samples the realtime clock.
///
/// Deviations: the `#ifdef HAVE_LIBHWLOC` topology init is not built (no
/// `libhwloc`), matching a build without it. `Platform_gettime_realtime`
/// writes both a `timeval` and `realtimeMs`; `Machine` models only
/// `realtimeMs` (the `timeval` reader, `checkRecalculation`, is unported),
/// so the `timeval` is sampled into a throwaway local.
pub fn Machine_init(this: &mut Machine, usersTable: Option<usize>, userId: u32) {
    this.usersTable = usersTable;
    this.userId = userId;

    this.htopUserId = unsafe { libc::getuid() };

    // discover fixed column width limits
    Row_setPidColumnWidth(Platform_getMaxPid());

    // always maintain valid realtime timestamps. C calls
    // `Platform_gettime_realtime` (a per-platform macro aliasing
    // `Generic_gettime_realtime`).
    let mut realtime: libc::timeval = unsafe { core::mem::zeroed() };
    Generic_gettime_realtime(&mut realtime, &mut this.realtimeMs);
}

/// Port of `void Machine_done(Machine* this)` from `Machine.c:53`. C body:
/// `Object_delete(this->processTable); free(this->tables);` (the
/// `hwloc_topology_destroy` block is `#ifdef HAVE_LIBHWLOC`, compiled out on a
/// no-libhwloc build). `free(this->tables)` maps to clearing the
/// `Vec<TableHandle>` (the C frees the pointer *array*, not the tables it
/// aliases). `Object_delete(this->processTable)` frees the process `Table`
/// through the pointer: `main.rs` `Box::into_raw`s the concrete platform
/// `ProcessTable` and stores the base `*mut Table` (offset 0 via the repr(C)
/// `super_` chain — the same base↔concrete round-trip
/// [`DarwinProcessTable::scan_iterate`](crate::ported::darwin::darwinprocesstable::DarwinProcessTable)
/// relies on), so the base pointer downcasts back to the concrete `Box` for a
/// correct free. `Machine` is the sole deleter of that allocation (`main.rs`
/// hands it over and never frees it), so the reclaim is not a double free.
///
/// The reclaim is darwin-only: `DarwinProcessTable` is `#[repr(C)]` (so the
/// cast is sound), whereas `LinuxProcessTable` is not, and only the darwin TUI
/// wires a real process table. On other targets the borrowed pointer is left
/// for the OS to reclaim at exit (the faithful net effect, since C only calls
/// `Machine_done` at process teardown).
pub fn Machine_done(this: &mut Machine) {
    // Object_delete(this->processTable);
    #[cfg(target_os = "macos")]
    if let Some(pt) = this.processTable.take() {
        // SAFETY: `pt` is the darwin process table `Box::into_raw`'d by
        // `main.rs`; the repr(C) `super_`-at-offset-0 chain makes the base
        // `*mut Table` round-trip to `*mut DarwinProcessTable`, and `Machine`
        // is its sole owner (no other code frees it), so reconstructing and
        // dropping the `Box` is a correct, non-double free.
        drop(unsafe {
            Box::from_raw(pt as *mut crate::ported::darwin::darwinprocesstable::DarwinProcessTable)
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = this.processTable.take();

    // free(this->tables);
    this.tables.clear();
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
/// table via `Machine_addTable`. The C mutates `ss->table` through the
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

/// Port of `void Machine_setTablesPanel(Machine* this, Panel* panel)` from
/// `Machine.c:94`. Points every registered table at the shared main
/// `Panel` by calling [`Table_setPanel`] on each `this->tables[i]`. The C
/// walks `[0, tableCount)` and passes the one `Panel*` through unchanged;
/// each `this.tables[i]` is a raw `*mut Table` that is dereferenced here
/// (the panel it stores is later read by `Table_rebuildPanel`).
pub fn Machine_setTablesPanel(this: &mut Machine, panel: *mut Panel) {
    for i in 0..this.tableCount {
        // C: `Table_setPanel(this->tables[i], panel)`.
        unsafe {
            Table_setPanel(&mut *this.tables[i], panel);
        }
    }
}

/// Port of `void Machine_scanTables(Machine* this)` from `Machine.c:100`.
/// Stamps the scan clocks (first scan seeds `prevMonotonicMs = 0`,
/// `monotonicMs = 1`; later scans roll `prev = curr` and re-sample the
/// monotonic clock), early-returns if the clock did not advance, resets
/// `maxUserId` and the per-field column widths, then walks every registered
/// table dispatching the scan vtable
/// (`Table_scanPrepare`/`_scanIterate`/`_scanCleanup`, `Table.h:55-57`),
/// and finally publishes the observed uid/pid column widths.
///
/// The C `static bool firstScanDone` (function-local, persists across calls)
/// maps to a function-local [`AtomicBool`]: htop is single-threaded here, so
/// `Relaxed` matches the C's plain read/write.
///
/// The scan loop dispatches through the ported [`Table`] scan vtable — the
/// concrete Darwin table wires its slots in `ProcessTable_new`, so `iterate`
/// reaches `ProcessTable_goThroughEntries` (the live process scan).
pub fn Machine_scanTables(this: &mut Machine) {
    use std::sync::atomic::{AtomicBool, Ordering};

    // set scan timestamp
    static FIRST_SCAN_DONE: AtomicBool = AtomicBool::new(false);

    if FIRST_SCAN_DONE.load(Ordering::Relaxed) {
        this.prevMonotonicMs = this.monotonicMs;
        // C `Platform_gettime_monotonic` (a per-platform macro aliasing
        // `Generic_gettime_monotonic`).
        Generic_gettime_monotonic(&mut this.monotonicMs);
    } else {
        this.prevMonotonicMs = 0;
        this.monotonicMs = 1;
        FIRST_SCAN_DONE.store(true, Ordering::Relaxed);
    }
    if this.monotonicMs <= this.prevMonotonicMs {
        return;
    }

    this.maxUserId = 0;
    Row_resetFieldWidths();

    for i in 0..this.tableCount {
        // Table* table = this->tables[i];
        let table = this.tables[i];

        // pre-processing of each row
        Table_scanPrepare(table);

        // scan values for this table
        Table_scanIterate(table);

        // post-process after scanning
        Table_scanCleanup(table);
    }

    Row_setUidColumnWidth(this.maxUserId);
    Row_setPidColumnWidth(this.maxProcessId);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Distinct non-null `*mut Table` stand-ins. The ported functions only
    // store and identity-compare these (never dereference), exactly as the
    // C tests would use distinct `Table*` values.
    const T10: TableHandle = 10 as TableHandle;
    const T20: TableHandle = 20 as TableHandle;
    const T30: TableHandle = 30 as TableHandle;
    const T42: TableHandle = 42 as TableHandle;

    #[test]
    fn addTable_dedup_keeps_first_occurrence_and_ignores_repeats() {
        let mut m = Machine::default();

        Machine_addTable(&mut m, T10);
        Machine_addTable(&mut m, T20);
        // Re-adding an already-registered table is a no-op (C returns
        // early on pointer match).
        Machine_addTable(&mut m, T10);
        Machine_addTable(&mut m, T20);
        Machine_addTable(&mut m, T30);

        assert_eq!(m.tables, vec![T10, T20, T30]);
        assert_eq!(m.tableCount, 3);
        // Invariant the C maintains: tableCount tracks the array length.
        assert_eq!(m.tableCount, m.tables.len());
    }

    #[test]
    fn addTable_first_insert_grows_from_empty() {
        let mut m = Machine::default();
        assert_eq!(m.tableCount, 0);
        assert!(m.tables.is_empty());

        Machine_addTable(&mut m, T42);

        assert_eq!(m.tables, vec![T42]);
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
        let pt: TableHandle = 7 as TableHandle;

        Machine_populateTablesFromSettings(&mut m, settings_with_screens(2), pt);

        let s = m.settings.as_ref().unwrap();
        assert_eq!(s.screens[0].table, Some(pt));
        assert_eq!(s.screens[1].table, Some(pt));

        assert_eq!(m.processTable, Some(pt));
        assert_eq!(m.activeTable, Some(pt)); // first screen's table
        assert_eq!(m.tables, vec![pt]); // deduped
        assert_eq!(m.tableCount, 1);
    }

    #[test]
    fn populate_first_screen_becomes_active_and_explicit_tables_are_kept() {
        // First screen has an explicit table (not overwritten); second is
        // null (defaulted to processTable); third repeats the first
        // (deduped away).
        let explicit: TableHandle = 100 as TableHandle;
        let pt: TableHandle = 9 as TableHandle;
        let settings = Settings {
            screens: vec![
                ScreenSettings {
                    table: Some(explicit),
                    treeView: false,
                    ..Default::default()
                },
                ScreenSettings::default(),
                ScreenSettings {
                    table: Some(explicit),
                    treeView: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut m = Machine::default();

        Machine_populateTablesFromSettings(&mut m, settings, pt);

        let s = m.settings.as_ref().unwrap();
        assert_eq!(s.screens[0].table, Some(explicit)); // untouched
        assert_eq!(s.screens[1].table, Some(pt)); // defaulted
        assert_eq!(s.screens[2].table, Some(explicit)); // untouched

        assert_eq!(m.activeTable, Some(explicit)); // first screen wins
        assert_eq!(m.tables, vec![explicit, pt]); // 100 registered once, 9 next
        assert_eq!(m.tableCount, 2);
    }

    #[test]
    fn populate_with_no_screens_registers_nothing() {
        let mut m = Machine::default();
        let pt: TableHandle = 5 as TableHandle;

        Machine_populateTablesFromSettings(&mut m, settings_with_screens(0), pt);

        assert_eq!(m.processTable, Some(pt));
        assert_eq!(m.activeTable, None); // loop never runs
        assert!(m.tables.is_empty());
        assert_eq!(m.tableCount, 0);
    }

    #[test]
    fn setTablesPanel_points_every_table_at_the_panel() {
        // Real, address-stable Tables so setTablesPanel can dereference
        // them and store the panel; a distinct non-null `*mut Panel`
        // stand-in is written into each and read back.
        use crate::ported::panel::Panel;
        let mut t0 = Box::new(Table::empty());
        let mut t1 = Box::new(Table::empty());
        let panel = 0xABCD as *mut Panel;

        let mut m = Machine::default();
        Machine_addTable(&mut m, &mut *t0 as *mut Table);
        Machine_addTable(&mut m, &mut *t1 as *mut Table);

        Machine_setTablesPanel(&mut m, panel);

        // C `Table_setPanel` stores the pointer verbatim on each table.
        assert_eq!(t0.panel, panel);
        assert_eq!(t1.panel, panel);
    }
}
