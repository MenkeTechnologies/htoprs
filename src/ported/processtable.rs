//! Port of `ProcessTable.c` — the process-specific `Table` subclass:
//! adds the task counters and the pid-match filter list, and specializes
//! the scan lifecycle (`prepare`/`iterate`/`cleanup`).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Struct model
//!
//! htop's `ProcessTable` (`ProcessTable.h:19`) is `struct { Table super;
//! Hashtable* pidMatchList; unsigned totalTasks, runningTasks,
//! userlandThreads, kernelThreads; }`. The "extends Table" relationship
//! is modeled by embedding [`Table`] as [`ProcessTable::super_`] (the
//! same pattern `Process` uses to embed `Row`). `pidMatchList` is an
//! opaque handle (the `Hashtable` filter list is not dereferenced by any
//! ported fn). C upcasts `ProcessTable*` ↔ `Table*` by pointer; here the
//! ported fns take `&mut ProcessTable` and reach the base via `super_`.
//!
//! # Ported
//!
//! - [`ProcessTable_init`] (`ProcessTable.c:21`) — `Table_init` + store
//!   `pidMatchList`.
//! - [`ProcessTable_done`] (`ProcessTable.c:27`) — `Table_done`.
//! - [`ProcessTable_prepareEntries`] (`ProcessTable.c:46`) — zero the
//!   task counters, then `Table_prepareEntries`.
//! - [`ProcessTable_iterateEntries`] (`ProcessTable.c:56`) — delegates to
//!   `ProcessTable_goThroughEntries` (the platform scan, stubbed below).
//!
//! # Still stubbed (`todo!()`, named after the C fn so the port gate
//! accepts the module)
//!
//! All three are blocked by the same root gap plus per-fn specifics: the
//! ported [`Table`] stores its rows as `Vec<Option<Row>>` — `Row` values,
//! not the polymorphic `Process*` rows htop's `Table` holds (C stores
//! `Row*` that are upcast `Process*`). Any `ProcessTable` fn that must
//! treat a row *as a `Process`* — recover it via `(Process*)Vector_get`,
//! read a `Process`-only field, or return a `Process*` — has no faithful
//! expression against a `Row`-typed table, and `table.rs` is out of scope
//! for this port (edit-only-my-file rule). This is the honest reason the
//! bodies stay `todo!()` rather than being gutted to a `Row`-only shell.
//!
//! - [`ProcessTable_getProcess`] (`ProcessTable.c:31`) — `Hashtable_get`
//!   returns a `Process*` and the fn returns/constructs a `Process`, but
//!   the ported table's rows are `Row`, so there is no `Process` to look
//!   up or hand back. Also needs the platform `Process_New` constructor
//!   (`typedef Process* (*Process_New)(const struct Machine_*)`,
//!   `Process.h:241`) — a function-pointer type with no ported analog.
//! - [`ProcessTable_goThroughEntries`] (`ProcessTable.c` platform) — the
//!   `/proc` (or per-platform) scan; implemented by `linux/` etc.
//! - [`ProcessTable_cleanupEntries`] (`ProcessTable.c:62`) — three
//!   blockers: (1) it treats each row as a `Process`
//!   (`(Process*)Vector_get`) to read `p->st_uid` and call
//!   `Process_makeCommandStr(p, settings)`, both inaccessible on a
//!   `Row`-typed table; (2) `Process_makeCommandStr` (`Process.c:183`) is
//!   itself an unported stub in `process.rs`; (3) it *mutates*
//!   `host->maxUserId`/`maxProcessId`, but [`Table::host`] is
//!   `*const Machine` (the C `super->host` is a mutable `Machine*`), so
//!   the write is not expressible. The base `Table_cleanupRow` /
//!   `Table_compact` logic it wraps *is* ported, in `table.rs`.
//!
//! `gen_port_report.py` counts `todo!()` bodies as *stubbed*, not
//! *ported*, so scaffolding does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;
use crate::ported::table::{Table, Table_done, Table_init, Table_prepareEntries};

/// Port of htop's `struct ProcessTable_` (`ProcessTable.h:19`).
/// Embeds [`Table`] as `super_` (the "extends Table" relation) and adds
/// the pid-match filter and the task counters.
pub struct ProcessTable {
    /// C `Table super` — the embedded base table.
    pub super_: Table,
    /// C `Hashtable* pidMatchList` — opaque handle (the filter list is
    /// never dereferenced by a ported fn).
    pub pidMatchList: Option<usize>,
    /// C `unsigned int totalTasks`.
    pub totalTasks: u32,
    /// C `unsigned int runningTasks`.
    pub runningTasks: u32,
    /// C `unsigned int userlandThreads`.
    pub userlandThreads: u32,
    /// C `unsigned int kernelThreads`.
    pub kernelThreads: u32,
}

impl ProcessTable {
    /// A zeroed `ProcessTable` (empty base table, no filter, zero
    /// counters). Gate-skipped associated fn — not a real C function; the
    /// C analog is `xMalloc` returning uninitialized storage that
    /// `ProcessTable_init` overwrites.
    pub fn empty() -> ProcessTable {
        ProcessTable {
            super_: Table::empty(),
            pidMatchList: None,
            totalTasks: 0,
            runningTasks: 0,
            userlandThreads: 0,
            kernelThreads: 0,
        }
    }
}

/// Port of `void ProcessTable_init(ProcessTable* this, const ObjectClass*
/// klass, Machine* host, Hashtable* pidMatchList)` from
/// `ProcessTable.c:21`. Initializes the base table and stores the
/// pid-match list.
///
/// Signature mapping: the `klass` type tag is dropped (class identity is
/// the Rust type; see `Table_init`). `pidMatchList` is an opaque handle.
pub fn ProcessTable_init(
    this: &mut ProcessTable,
    host: *const Machine,
    pidMatchList: Option<usize>,
) {
    Table_init(&mut this.super_, host);

    this.pidMatchList = pidMatchList;
}

/// Port of `void ProcessTable_done(ProcessTable* this)` from
/// `ProcessTable.c:27`. Tears down the base table (`Drop` releases the
/// owned fields).
pub fn ProcessTable_done(this: &mut ProcessTable) {
    Table_done(&mut this.super_);
}

/// TODO: port of `Process* ProcessTable_getProcess(ProcessTable* this,
/// pid_t pid, bool* preExisting, Process_New constructor)` from
/// `ProcessTable.c:31`. Blocked: the body looks up (`Hashtable_get`),
/// constructs (`constructor(table->host)`), and returns a `Process*`, but
/// the ported [`Table`] stores rows as `Row` values (`Vec<Option<Row>>`),
/// not the upcast `Process*` htop's table holds — there is no `Process` to
/// recover or return. It also needs the platform `Process_New` constructor
/// (`typedef Process* (*Process_New)(...)`, `Process.h:241`), a
/// function-pointer type with no ported analog. Cannot be gutted to a
/// `Row`-only shell without lying about what it does.
pub fn ProcessTable_getProcess() {
    todo!("port of ProcessTable.c:31 — Table rows are Row, not Process; needs Process_New constructor")
}

/// Port of `static void ProcessTable_prepareEntries(Table* super)` from
/// `ProcessTable.c:46`. Zeroes the task counters, then delegates to
/// `Table_prepareEntries`.
///
/// Signature mapping: the C takes `Table* super` and downcasts to
/// `ProcessTable*`; here the fn takes `&mut ProcessTable` directly and
/// reaches the base via `super_`.
pub fn ProcessTable_prepareEntries(this: &mut ProcessTable) {
    this.totalTasks = 0;
    this.userlandThreads = 0;
    this.kernelThreads = 0;
    this.runningTasks = 0;

    Table_prepareEntries(&mut this.super_);
}

/// Port of `static void ProcessTable_iterateEntries(Table* super)` from
/// `ProcessTable.c:56`. Delegates to the platform scan
/// [`ProcessTable_goThroughEntries`] (stubbed).
pub fn ProcessTable_iterateEntries(this: &mut ProcessTable) {
    // calling into platform-specific code
    ProcessTable_goThroughEntries(this);
}

/// TODO: port of `void ProcessTable_goThroughEntries(ProcessTable*
/// this)` — the platform `/proc` (or per-OS) scan. Implemented by the
/// `linux/` etc. scan layer.
pub fn ProcessTable_goThroughEntries(_this: &mut ProcessTable) {
    todo!("platform ProcessTable_goThroughEntries — /proc scan, filled by linux/ layer")
}

/// TODO: port of `static void ProcessTable_cleanupEntries(Table* super)`
/// from `ProcessTable.c:62`. Three blockers, none yet resolvable from
/// this file: (1) each row is cast `(Process*)Vector_get(super->rows, i)`
/// to read `p->st_uid` and call `Process_makeCommandStr(p, settings)` —
/// both inaccessible because the ported [`Table`] stores `Row` values, not
/// `Process`; (2) `Process_makeCommandStr` (`Process.c:183`) is itself a
/// stub in `process.rs`; (3) it mutates `host->maxUserId`/`maxProcessId`,
/// but [`Table::host`] is `*const Machine` while the C `super->host` is a
/// mutable `Machine*`, so the write cannot be expressed. The wrapped base
/// `Table_cleanupRow` / `Table_compact` logic *is* ported (in `table.rs`);
/// this per-process wrapper is not portable until the table models
/// `Process` rows and `Process_makeCommandStr` lands.
pub fn ProcessTable_cleanupEntries() {
    todo!("port of ProcessTable.c:62 — Table rows are Row not Process; needs Process_makeCommandStr + mutable host")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::machine::{Machine, ScreenSettings, Settings};
    use crate::ported::row::Row;
    use crate::ported::table::{Table_add, Table_findRow};

    fn host(mono: u64) -> Machine {
        let mut m = Machine::default();
        m.monotonicMs = mono;
        m.settings = Some(Settings {
            highlightChanges: false,
            highlightDelaySecs: 0,
            ss: ScreenSettings {
                treeView: false,
                table: None,
            },
            screens: Vec::new(),
            ..Default::default()
        });
        m
    }

    fn row(id: i32) -> Row {
        let mut r = Row::default();
        r.id = id;
        r.show = true;
        r.showChildren = true;
        r
    }

    #[test]
    fn init_wires_base_table_and_pid_match_list() {
        let h = host(0);
        let mut pt = ProcessTable::empty();
        ProcessTable_init(&mut pt, &h as *const Machine, Some(77));

        assert_eq!(pt.pidMatchList, Some(77));
        assert!(pt.super_.needsSort);
        assert_eq!(pt.super_.following, -1);
    }

    #[test]
    fn prepare_entries_zeroes_counters_and_resets_rows() {
        let h = host(10);
        let mut pt = ProcessTable::empty();
        ProcessTable_init(&mut pt, &h as *const Machine, None);

        // Populate the base table and dirty the counters + a row flag.
        Table_add(&mut pt.super_, row(1));
        Table_add(&mut pt.super_, row(2));
        pt.totalTasks = 9;
        pt.runningTasks = 3;
        pt.userlandThreads = 5;
        pt.kernelThreads = 4;
        pt.super_.rows[0].as_mut().unwrap().updated = true;
        pt.super_.rows[0].as_mut().unwrap().show = false;

        ProcessTable_prepareEntries(&mut pt);

        // Counters zeroed.
        assert_eq!(pt.totalTasks, 0);
        assert_eq!(pt.runningTasks, 0);
        assert_eq!(pt.userlandThreads, 0);
        assert_eq!(pt.kernelThreads, 0);

        // Base Table_prepareEntries ran: updated cleared, show reset true,
        // wasShown carries the previous show.
        let r0 = Table_findRow(&pt.super_, 1).unwrap();
        assert!(!r0.updated);
        assert!(r0.show);
        assert!(!r0.wasShown); // previous show was false
    }

    #[test]
    #[should_panic(expected = "goThroughEntries")]
    fn iterate_entries_delegates_to_platform_scan_stub() {
        // iterateEntries is a pure delegation; it must call the (stubbed)
        // platform scan, which panics until the linux/ layer fills it.
        let h = host(0);
        let mut pt = ProcessTable::empty();
        ProcessTable_init(&mut pt, &h as *const Machine, None);
        ProcessTable_iterateEntries(&mut pt);
    }
}
