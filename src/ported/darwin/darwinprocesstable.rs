//! Partial port of `DarwinProcessTable.c` — the Darwin process table.
//!
//! Ported (self-contained: only the base [`ProcessTable`] +
//! [`crate::ported::table::Table`] plumbing):
//! - [`ProcessTable_new`] (`DarwinProcessTable.c:56`) — allocate and init
//!   the `DarwinProcessTable`.
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `ProcessTable_getKInfoProcs` / `ProcessTable_goThroughEntries` need the
//!   `kinfo_proc` struct (absent from `libc`) modeled for the
//!   `sysctl(KERN_PROC_ALL)` scan.
//! - `ProcessTable_getProcess` is still a stub in `processtable.rs`, so the
//!   per-entry `getProcess` → `setFromKInfoProc` → `scanThreads` pipeline in
//!   `goThroughEntries` cannot be wired up.
//! - `ProcessTable_delete` needs `Object_delete` teardown (`Drop` releases
//!   the owned fields).
//!
//! `gen_port_report.py` counts remaining `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;
use crate::ported::processtable::{ProcessTable, ProcessTable_init};

/// Port of htop's `struct DarwinProcessTable_` (`DarwinProcessTable.h:16`).
/// "Extends" [`ProcessTable`] via the embedded `super_` field (htop's
/// `ProcessTable super;` first member); `global_diff` is the Darwin-only
/// per-scan accumulator.
pub struct DarwinProcessTable {
    /// C `ProcessTable super` — the embedded base process table.
    pub super_: ProcessTable,
    /// C `uint64_t global_diff`.
    pub global_diff: u64,
}

/// TODO: port of `static struct kinfo_proc* ProcessTable_getKInfoProcs(size_t* count` from `DarwinProcessTable.c:31`.
pub fn ProcessTable_getKInfoProcs() {
    todo!("port of DarwinProcessTable.c:31")
}

/// Port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` from `DarwinProcessTable.c:56`. C `xCalloc`s a
/// `DarwinProcessTable` (zeroing `global_diff`), sets its class, runs
/// `ProcessTable_init` on the embedded base with the `DarwinProcess`
/// constructor class, and returns `&this->super`.
///
/// The returned `Box<DarwinProcessTable>` is the owner (C's heap
/// allocation); the caller derives the graph pointers `&mut box.super_`
/// (`*mut ProcessTable`) and `&mut box.super_.super_` (`*mut Table`). The
/// `Object_setClass` / `Class(DarwinProcess)` class tags are dropped —
/// class identity is the Rust type (see [`ProcessTable_init`]).
pub fn ProcessTable_new(host: *const Machine, pidMatchList: Option<usize>) -> Box<DarwinProcessTable> {
    let mut this = Box::new(DarwinProcessTable {
        super_: ProcessTable::empty(),
        global_diff: 0,
    });

    ProcessTable_init(&mut this.super_, host, pidMatchList);

    this
}

/// TODO: port of `void ProcessTable_delete(Object* cast` from `DarwinProcessTable.c:66`.
pub fn ProcessTable_delete() {
    todo!("port of DarwinProcessTable.c:66")
}

/// TODO: port of `void ProcessTable_goThroughEntries(ProcessTable* super` from `DarwinProcessTable.c:72`.
pub fn ProcessTable_goThroughEntries() {
    todo!("port of DarwinProcessTable.c:72")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_inits_base_table_with_host_and_filter() {
        // A distinct non-null `*const Machine` stand-in; ProcessTable_new
        // only stores it on the base table (never dereferences it).
        let host = 0xF00D as *const Machine;
        let filter = Some(0xBEEF_usize);

        let pt = ProcessTable_new(host, filter);

        // global_diff zeroed like the C xCalloc.
        assert_eq!(pt.global_diff, 0);
        // ProcessTable_init stored the filter list and Table_init wired the
        // host back-pointer on the embedded base table.
        assert_eq!(pt.super_.pidMatchList, filter);
        assert_eq!(pt.super_.super_.host, host);
        // Base table starts empty (no rows registered yet).
        assert!(pt.super_.super_.rows.is_empty());
        // Counters start at zero.
        assert_eq!(pt.super_.totalTasks, 0);
        assert_eq!(pt.super_.runningTasks, 0);
    }
}
