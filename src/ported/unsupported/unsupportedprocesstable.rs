//! Port of `UnsupportedProcessTable.c` — the fallback process table.
//!
//! Ported (self-contained: only the base [`ProcessTable`] +
//! [`crate::ported::table::Table`] plumbing and the fallback
//! [`UnsupportedProcess`]):
//! - [`ProcessTable_new`] (`UnsupportedProcessTable.c:19`)
//! - [`ProcessTable_goThroughEntries`] (`UnsupportedProcessTable.c:35`)
//! - [`ProcessTable_delete`] (`UnsupportedProcessTable.c:29`) — pure teardown:
//!   [`ProcessTable_done`](crate::ported::processtable::ProcessTable_done) on `&mut this.super_`, then `Drop` releases the owned
//!   fields (the darwin `ProcessTable_delete` precedent).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // faithful C global name (UnsupportedProcessTable_class)
#![allow(dead_code)]

use core::any::Any;

use crate::ported::machine::Machine;
use crate::ported::object::Object;
use crate::ported::process::{
    Process, ProcessState, Process_fillStarttimeBuffer, Process_setParent, Process_setPid,
    Process_setThreadGroup, Process_updateCPUFieldWidths, Process_updateCmdline,
    Process_updateComm, Process_updateExe, PROCESS_FLAG_CWD,
};
use crate::ported::processtable::{
    ProcessTable, ProcessTable_cleanupEntries, ProcessTable_getProcess, ProcessTable_init,
    ProcessTable_prepareEntries,
};
use crate::ported::table::{Table, TableClass};
use crate::ported::unsupported::unsupportedprocess::{UnsupportedProcess, UnsupportedProcess_new};

/// Port of `typedef struct UnsupportedProcessTable_` (`UnsupportedProcessTable.h`).
/// The fallback table embeds the base [`ProcessTable`] and adds no fields.
/// `#[repr(C)]` keeps `super_` at offset 0 so the C `(UnsupportedProcessTable*)`
/// downcast (and the `*mut Table` → concrete round-trip through the scan
/// vtable) is sound.
#[repr(C)]
pub struct UnsupportedProcessTable {
    /// C `ProcessTable super` — the embedded base process table.
    pub super_: ProcessTable,
}

/// The scan-vtable glue for [`UnsupportedProcessTable`]. Each slot downcasts
/// the base `*mut Table` to the concrete table and delegates to the
/// corresponding base `ProcessTable_class` slot — the same structural pattern
/// as the darwin `DarwinProcessTable` scan vtable (`super_: ProcessTable` at
/// offset 0, whose `super_: Table` is likewise at offset 0).
impl UnsupportedProcessTable {
    /// C `ProcessTable_class.prepare` (`ProcessTable_prepareEntries(Table*)`).
    ///
    /// # Safety precondition
    /// `super_` is the base of a live `UnsupportedProcessTable`.
    fn scan_prepare(super_: *mut Table) {
        let this = super_ as *mut UnsupportedProcessTable;
        // SAFETY: `super_` is the base of a live `UnsupportedProcessTable`.
        ProcessTable_prepareEntries(unsafe { &mut (*this).super_ });
    }

    /// C `ProcessTable_class.iterate` (`ProcessTable_iterateEntries(Table*)`,
    /// which calls `ProcessTable_goThroughEntries`). Dispatches straight to the
    /// fallback [`ProcessTable_goThroughEntries`] — the platform symbol C
    /// link-resolves — since the common `ProcessTable_iterateEntries` routes to
    /// the *stubbed* base `ProcessTable_goThroughEntries`.
    ///
    /// # Safety precondition
    /// `super_` is the base of a live `UnsupportedProcessTable`.
    fn scan_iterate(super_: *mut Table) {
        let this = super_ as *mut UnsupportedProcessTable;
        // SAFETY: `super_` is the base of a live `UnsupportedProcessTable`.
        ProcessTable_goThroughEntries(unsafe { &mut *this });
    }

    /// C `ProcessTable_class.cleanup` (`ProcessTable_cleanupEntries(Table*)`).
    ///
    /// # Safety precondition
    /// `super_` is the base of a live `UnsupportedProcessTable`.
    fn scan_cleanup(super_: *mut Table) {
        let this = super_ as *mut UnsupportedProcessTable;
        // SAFETY: `super_` is the base of a live `UnsupportedProcessTable`.
        ProcessTable_cleanupEntries(unsafe { &mut (*this).super_ });
    }
}

/// Port of `const TableClass ProcessTable_class` (`ProcessTable.c:94`), the
/// class the fallback `UnsupportedProcessTable` runs under (htop's fallback
/// table uses the common `ProcessTable_class`, whose `iterate` link-resolves to
/// the fallback `ProcessTable_goThroughEntries`). Only the scan-vtable half is
/// modeled (see [`TableClass`]); the `ObjectClass super` is class identity in
/// Rust.
pub static UnsupportedProcessTable_class: TableClass = TableClass {
    prepare: Some(UnsupportedProcessTable::scan_prepare),
    iterate: Some(UnsupportedProcessTable::scan_iterate),
    cleanup: Some(UnsupportedProcessTable::scan_cleanup),
};

/// Port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` (`UnsupportedProcessTable.c:19`). C `xCalloc`s an
/// `UnsupportedProcessTable`, sets its class, runs `ProcessTable_init` on the
/// embedded base with the `UnsupportedProcess` constructor class, and returns
/// `&this->super`.
///
/// The returned `Box<UnsupportedProcessTable>` is the owner (C's heap
/// allocation); the caller derives the graph pointers `&mut box.super_`
/// (`*mut ProcessTable`) and `&mut box.super_.super_` (`*mut Table`). The
/// `Class(UnsupportedProcess)` row-constructor tag is dropped (class identity
/// is the Rust type), but the *table's* scan class is wired here: C's
/// `Object_setClass(this, Class(ProcessTable))` sets `super.klass`, which the
/// scan macros dispatch through, so the base [`Table::klass`] is pointed at
/// [`UnsupportedProcessTable_class`].
pub fn ProcessTable_new(
    host: *const Machine,
    pidMatchList: Option<usize>,
) -> Box<UnsupportedProcessTable> {
    let mut this = Box::new(UnsupportedProcessTable {
        super_: ProcessTable::empty(),
    });

    ProcessTable_init(&mut this.super_, host, pidMatchList);

    // Object_setClass(this, Class(...)) — wire the scan vtable so
    // Machine_scanTables can dispatch prepare/iterate/cleanup through it.
    this.super_.super_.klass = &UnsupportedProcessTable_class as *const TableClass;

    this
}

/// Port of `void ProcessTable_delete(Object* cast)` from
/// `UnsupportedProcessTable.c:29`. The C body runs
/// `ProcessTable_done(&this->super)` then `free(this)` (no platform-specific
/// heap fields). Taking `this` by value hands the [`UnsupportedProcessTable`]
/// to `Drop` for the allocation; the base teardown is [`ProcessTable_done`](crate::ported::processtable::ProcessTable_done) on
/// `&mut this.super_` (the darwin `ProcessTable_delete` precedent).
pub fn ProcessTable_delete(mut this: UnsupportedProcessTable) {
    crate::ported::processtable::ProcessTable_done(&mut this.super_);
}

/// Port of `void ProcessTable_goThroughEntries(ProcessTable* super)`
/// (`UnsupportedProcessTable.c:35`). The fallback scan emits a single synthetic
/// process (pid 1) filled with fixed placeholder values.
///
/// Deviation (documented, not silent): per [`ProcessTable_getProcess`], a
/// newly-seen process is added inside `getProcess`, so the C's trailing
/// `if (!preExisting) ProcessTable_add(super, proc)` is not repeated.
pub fn ProcessTable_goThroughEntries(this: &mut UnsupportedProcessTable) {
    // const Settings* settings = super->super.host->settings; -> ss->flags,
    // read once through the raw host pointer (absent settings => 0 flags).
    let host = this.super_.super_.host;
    let flags: u32 = unsafe {
        host.as_ref()
            .and_then(|m| m.settings.as_ref())
            .and_then(|s| s.screens.get(s.ssIndex as usize))
            .map_or(0, |ss| ss.flags)
    };

    // proc = ProcessTable_getProcess(super, 1, &preExisting, UnsupportedProcess_new);
    let (_pre_existing, idx) = ProcessTable_getProcess(&mut this.super_, 1, |h| {
        UnsupportedProcess_new(h) as Box<dyn Object>
    });

    // Recover a raw `*mut UnsupportedProcess` for the row via a checked borrow
    // (which ends here). `Object: Any`, so upcast to `dyn Any` and downcast to
    // the concrete type the row was built as.
    let up: *mut UnsupportedProcess = {
        let obj: &mut dyn Object = this.super_.super_.rows[idx].as_mut().unwrap().as_mut();
        let any: &mut dyn Any = obj;
        any.downcast_mut::<UnsupportedProcess>().unwrap()
    };

    // SAFETY: `up` aliases the row just added/fetched in
    // `this.super_.super_.rows`; no further `getProcess` runs this iteration, so
    // `rows` is not reallocated and the pointer stays valid.
    let proc: &mut Process = unsafe { &mut (*up).super_ };

    /* Empty values */
    proc.time += 10;
    Process_setPid(proc, 1);
    Process_setParent(proc, 1);
    Process_setThreadGroup(proc, 0);

    Process_updateComm(proc, Some("commof16char"));
    Process_updateCmdline(proc, Some("<unsupported architecture>"), 0, 0);
    Process_updateExe(proc, Some("/path/to/executable"));

    if flags & PROCESS_FLAG_CWD != 0 {
        proc.procCwd = Some("/current/working/directory".to_string());
    }

    proc.super_.updated = true;

    proc.state = ProcessState::RUNNING;
    proc.isKernelThread = false;
    proc.isUserlandThread = false;
    proc.super_.show = true; /* Reflected in settings-> "hideXXX" really */
    proc.pgrp = 0;
    proc.session = 0;
    proc.tty_nr = 0;
    proc.tty_name = None;
    proc.tpgid = 0;
    proc.processor = 0;

    proc.percent_cpu = 2.5;
    proc.percent_mem = 2.5;
    Process_updateCPUFieldWidths(proc.percent_cpu);

    proc.st_uid = 0;
    proc.user = Some("nobody".to_string()); /* Update whenever proc->st_uid is changed */

    proc.priority = 0;
    proc.nice = 0;
    proc.nlwp = 1;
    proc.starttime_ctime = 1433116800; // Jun 01, 2015
    Process_fillStarttimeBuffer(proc);

    proc.m_virt = 100;
    proc.m_resident = 100;

    proc.minflt = 20;
    proc.majflt = 20;

    // if (!preExisting) ProcessTable_add(super, proc);
    // ProcessTable_getProcess already added a newly-seen process; not repeated.
}
