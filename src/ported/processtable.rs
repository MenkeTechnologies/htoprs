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
//! - [`ProcessTable_cleanupEntries`] (`ProcessTable.c:62`) — per-process
//!   `Process_makeCommandStr` + max-UID/PID tracking, wrapping the base
//!   `Table_cleanupRow` / `Table_compact` cull.
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
//!   `/proc` (or per-platform) scan. There is no generic C body: the
//!   header (`ProcessTable.h:34`) only declares it and each platform
//!   (`darwin/`, `linux/`, `freebsd/`, …) defines its own. It is therefore
//!   out of scope for this generic module and filled by the platform scan
//!   layer; the stub is the seam.
//!
//! `gen_port_report.py` counts `todo!()` bodies as *stubbed*, not
//! *ported*, so scaffolding does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;
use crate::ported::object::Object;
use crate::ported::process::{Process_getPid, Process_makeCommandStr, Process_setPid};
use crate::ported::settings::Settings;
use crate::ported::table::{
    Table, Table_add, Table_cleanupRow, Table_compact, Table_done, Table_init,
    Table_prepareEntries,
};

/// Port of htop's `struct ProcessTable_` (`ProcessTable.h:19`).
/// Embeds [`Table`] as `super_` (the "extends Table" relation) and adds
/// the pid-match filter and the task counters.
///
/// `#[repr(C)]` keeps `super_` at offset 0 so htop's `(ProcessTable*)table`
/// downcast — a `*Table` (e.g. `Machine::processTable`) cast back — is sound
/// (used by `TasksMeter` to read the task counters through the machine).
#[repr(C)]
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
/// Port of `Process* ProcessTable_getProcess(ProcessTable* this, pid_t pid,
/// bool* preExisting, Process_New constructor)` from `ProcessTable.c:31`.
/// Finds the process with `pid` in the table, or constructs a fresh one via
/// `constructor(host)` (e.g. `DarwinProcess_new`) and sets its pid.
///
/// Returns `(preExisting, idx)`: `idx` is the slot in `this.super_.rows`
/// holding the process (as a `Box<dyn Object>`), which the caller reads back
/// as a `&mut Process` via [`Object::as_process_mut`]. `preExisting` is the C
/// out-parameter.
///
/// Deviation from the C: htop's `getProcess` returns the (not-yet-added) new
/// process and the platform's `goThroughEntries` calls `ProcessTable_add`
/// later; here the new process is added immediately (via [`Table_add`], which
/// also stamps `seenStampMs`) so a stable `rows` index can be returned within
/// Rust's borrow model. The net effect is identical — a new process is always
/// added, and nothing observes the pre-add gap — and the caller skips its own
/// add when `!preExisting`.
pub fn ProcessTable_getProcess(
    this: &mut ProcessTable,
    pid: i32,
    constructor: fn(*const Machine) -> Box<dyn Object>,
) -> (bool, usize) {
    if let Some(&idx) = this.super_.table.get(&pid) {
        // Process* proc = Hashtable_get(...); *preExisting = true.
        debug_assert_eq!(
            Process_getPid(this.super_.rows[idx].as_ref().unwrap().as_process().unwrap()),
            pid
        );
        return (true, idx);
    }

    // proc = constructor(table->host); Process_setPid(proc, pid);
    let host = this.super_.host;
    let mut obj = constructor(host);
    debug_assert!(
        obj.as_process().unwrap().cmdline.is_none(),
        "getProcess: fresh process must have no cmdline"
    );
    Process_setPid(obj.as_process_mut().unwrap(), pid);

    let idx = this.super_.rows.len();
    Table_add(&mut this.super_, obj);
    (false, idx)
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

/// Port of `static void ProcessTable_cleanupEntries(Table* super)` from
/// `ProcessTable.c:62`. Walks `super->rows` back-to-front: refreshes each
/// process's merged command string ([`Process_makeCommandStr`]), tracks the
/// highest UID and PID seen for column scaling, then applies the base
/// [`Table_cleanupRow`] cull, remembering the lowest removed index. Finally
/// [`Table_compact`]s from there.
///
/// Signature mapping: the C takes `Table* super` and downcasts to
/// `ProcessTable*`; here the fn takes `&mut ProcessTable` directly and reaches
/// the base via `super_`. Each row is recovered as a `&mut Process` /
/// `&Process` via [`Object::as_process_mut`] / [`Object::as_process`] — the
/// `(Process*)Vector_get` cast — since the ported [`Table`] stores its rows
/// polymorphically (`Box<dyn Object>`, really `Process`es for a
/// `ProcessTable`).
///
/// The C `super->host` is a mutable `Machine*` and the body writes
/// `host->maxUserId`/`maxProcessId`; [`Table::host`] is `*const Machine`, so
/// the write goes through a `*mut Machine` cast of that pointer (its non-null
/// validity is the precondition, as in C). `settings` is read from the same
/// `Machine` as a `*const Settings`, so the read-of-settings / write-of-host
/// aliasing matches C's `const Settings* settings = host->settings;` beside
/// `host->maxUserId = …` (distinct fields of one object).
pub fn ProcessTable_cleanupEntries(this: &mut ProcessTable) {
    // Machine* host = super->host; const Settings* settings = host->settings;
    let host: *mut Machine = this.super_.host as *mut Machine;
    let settings: *const Settings = unsafe {
        (*host)
            .settings
            .as_ref()
            .expect("ProcessTable_cleanupEntries: host->settings is NULL") as *const Settings
    };

    // Lowest index of the row that is soft-removed. Used to speed up compaction.
    let mut dirty_index = this.super_.rows.len();

    // Finish process table update, culling any exit'd processes
    for i in (0..this.super_.rows.len()).rev() {
        // Process* p = (Process*) Vector_get(super->rows, i);
        // tidy up Process state after refreshing the ProcessTable table
        {
            let p = this.super_.rows[i]
                .as_mut()
                .expect("ProcessTable_cleanupEntries: NULL row slot")
                .as_process_mut()
                .expect("ProcessTable_cleanupEntries: row is not a Process");
            Process_makeCommandStr(p, unsafe { &*settings });
        }

        // keep track of the highest UID and PID for column scaling
        let (st_uid, pid) = {
            let p = this.super_.rows[i]
                .as_ref()
                .unwrap()
                .as_process()
                .expect("ProcessTable_cleanupEntries: row is not a Process");
            (p.st_uid, Process_getPid(p))
        };
        unsafe {
            if st_uid > (*host).maxUserId {
                (*host).maxUserId = st_uid;
            }
            if pid > (*host).maxProcessId {
                (*host).maxProcessId = pid;
            }
        }

        if !Table_cleanupRow(&mut this.super_, i) {
            dirty_index = i;
        }
    }

    // compact the table in case of deletions
    Table_compact(&mut this.super_, dirty_index);
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
            screens: vec![ScreenSettings {
                treeView: false,
                ..Default::default()
            }],
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

    /// A `Process_New`-shaped constructor for the tests: a fresh base
    /// `Process` (no cmdline), boxed as `dyn Object`.
    fn make_proc(_host: *const Machine) -> Box<dyn Object> {
        Box::new(crate::ported::process::Process::default())
    }

    #[test]
    fn getProcess_creates_new_then_finds_existing() {
        let h = host(1000);
        let mut pt = ProcessTable::empty();
        ProcessTable_init(&mut pt, &h as *const Machine, None);

        // First call for pid 42 constructs, sets the pid, and adds it.
        let (pre1, idx1) = ProcessTable_getProcess(&mut pt, 42, make_proc);
        assert!(!pre1);
        assert_eq!(pt.super_.rows.len(), 1);
        let p = pt.super_.rows[idx1].as_ref().unwrap().as_process().unwrap();
        assert_eq!(Process_getPid(p), 42);

        // Second call for the same pid finds the existing slot (no dup).
        let (pre2, idx2) = ProcessTable_getProcess(&mut pt, 42, make_proc);
        assert!(pre2);
        assert_eq!(idx2, idx1);
        assert_eq!(pt.super_.rows.len(), 1);
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
        Table_add(&mut pt.super_, Box::new(row(1)));
        Table_add(&mut pt.super_, Box::new(row(2)));
        pt.totalTasks = 9;
        pt.runningTasks = 3;
        pt.userlandThreads = 5;
        pt.kernelThreads = 4;
        pt.super_.rows[0].as_mut().unwrap().as_row_mut().unwrap().updated = true;
        pt.super_.rows[0].as_mut().unwrap().as_row_mut().unwrap().show = false;

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
