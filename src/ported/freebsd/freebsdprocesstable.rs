//! `FreeBSDProcessTable.c` — NOT yet ported (blocked on substrate).
//!
//! Every `pub fn` below is an honest `todo!()` placeholder named after its
//! C counterpart. The file is blocked on unported substrate:
//! - `ProcessTable_goThroughEntries` and the `FreeBSDProcessTable_*` scan
//!   helpers need `libkvm` (`kvm_getprocs`) plus the `kinfo_proc` struct
//!   (absent from `libc`) modeled.
//! - `ProcessTable_getProcess` (stub in `processtable.rs`) and the
//!   `FreeBSDProcess` object struct are required to build each row.
//! - `ProcessTable_new` / `_delete` additionally need the `FreeBSDProcessTable`
//!   object struct and `Object_setClass` machinery.
//!
//! `gen_port_report.py` counts these `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable* pidMatchList` from `FreeBSDProcessTable.c:45`.
pub fn ProcessTable_new() {
    todo!("port of FreeBSDProcessTable.c:45")
}

/// TODO: port of `void ProcessTable_delete(Object* cast` from `FreeBSDProcessTable.c:56`.
pub fn ProcessTable_delete() {
    todo!("port of FreeBSDProcessTable.c:56")
}

/// TODO: port of `static void FreeBSDProcessTable_updateExe(const struct kinfo_proc* kproc, Process* proc` from `FreeBSDProcessTable.c:62`.
pub fn FreeBSDProcessTable_updateExe() {
    todo!("port of FreeBSDProcessTable.c:62")
}

/// TODO: port of `static void FreeBSDProcessTable_updateCwd(const struct kinfo_proc* kproc, Process* proc` from `FreeBSDProcessTable.c:79`.
pub fn FreeBSDProcessTable_updateCwd() {
    todo!("port of FreeBSDProcessTable.c:79")
}

/// TODO: port of `static void FreeBSDProcessTable_updateProcessName(kvm_t* kd, const struct kinfo_proc* kproc, Process* proc` from `FreeBSDProcessTable.c:103`.
pub fn FreeBSDProcessTable_updateProcessName() {
    todo!("port of FreeBSDProcessTable.c:103")
}

/// TODO: port of `static char* FreeBSDProcessTable_readJailName(const struct kinfo_proc* kproc` from `FreeBSDProcessTable.c:135`.
pub fn FreeBSDProcessTable_readJailName() {
    todo!("port of FreeBSDProcessTable.c:135")
}

/// TODO: port of `void ProcessTable_goThroughEntries(ProcessTable* super` from `FreeBSDProcessTable.c:160`.
pub fn ProcessTable_goThroughEntries() {
    todo!("port of FreeBSDProcessTable.c:160")
}
