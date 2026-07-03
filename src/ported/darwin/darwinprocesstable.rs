//! `DarwinProcessTable.c` — NOT yet ported (blocked on substrate).
//!
//! Every `pub fn` below is an honest `todo!()` placeholder named after its
//! C counterpart. The whole file is blocked on unported substrate:
//! - `ProcessTable_getKInfoProcs` / `ProcessTable_goThroughEntries` need the
//!   `kinfo_proc` struct (absent from `libc`) modeled for the
//!   `sysctl(KERN_PROC_ALL)` scan.
//! - `ProcessTable_getProcess` is still a stub in `processtable.rs`, so the
//!   per-entry `getProcess` → `setFromKInfoProc` → `scanThreads` pipeline in
//!   `goThroughEntries` cannot be wired up.
//! - `ProcessTable_new` / `_delete` additionally need the `DarwinProcessTable`
//!   object struct and the `Object_setClass` / `DarwinProcess` machinery.
//!
//! `gen_port_report.py` counts these `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `static struct kinfo_proc* ProcessTable_getKInfoProcs(size_t* count` from `DarwinProcessTable.c:31`.
pub fn ProcessTable_getKInfoProcs() {
    todo!("port of DarwinProcessTable.c:31")
}

/// TODO: port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable* pidMatchList` from `DarwinProcessTable.c:56`.
pub fn ProcessTable_new() {
    todo!("port of DarwinProcessTable.c:56")
}

/// TODO: port of `void ProcessTable_delete(Object* cast` from `DarwinProcessTable.c:66`.
pub fn ProcessTable_delete() {
    todo!("port of DarwinProcessTable.c:66")
}

/// TODO: port of `void ProcessTable_goThroughEntries(ProcessTable* super` from `DarwinProcessTable.c:72`.
pub fn ProcessTable_goThroughEntries() {
    todo!("port of DarwinProcessTable.c:72")
}
