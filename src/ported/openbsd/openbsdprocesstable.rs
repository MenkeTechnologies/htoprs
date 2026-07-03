//! `OpenBSDProcessTable.c` — NOT yet ported (blocked on substrate).
//!
//! Every `pub fn` below is an honest `todo!()` placeholder named after its
//! C counterpart. The file is blocked on `libkvm` (`kvm_getprocs`) plus the `kinfo_proc`
//! struct, the stubbed `ProcessTable_getProcess` (`processtable.rs`), and the
//! `OpenBSDProcess` / `OpenBSDProcessTable` object structs.
//!
//! `gen_port_report.py` counts these `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable* pidMatchList` from `OpenBSDProcessTable.c:38`.
pub fn ProcessTable_new() {
    todo!("port of OpenBSDProcessTable.c:38")
}

/// TODO: port of `void ProcessTable_delete(Object* cast` from `OpenBSDProcessTable.c:48`.
pub fn ProcessTable_delete() {
    todo!("port of OpenBSDProcessTable.c:48")
}

/// TODO: port of `static void OpenBSDProcessTable_updateCwd(const struct kinfo_proc* kproc, Process* proc` from `OpenBSDProcessTable.c:54`.
pub fn OpenBSDProcessTable_updateCwd() {
    todo!("port of OpenBSDProcessTable.c:54")
}

/// TODO: port of `static void OpenBSDProcessTable_updateProcessName(kvm_t* kd, const struct kinfo_proc* kproc, Process* proc` from `OpenBSDProcessTable.c:74`.
pub fn OpenBSDProcessTable_updateProcessName() {
    todo!("port of OpenBSDProcessTable.c:74")
}

/// TODO: port of `static double getpcpu(const OpenBSDMachine* ohost, const struct kinfo_proc* kp` from `OpenBSDProcessTable.c:126`.
pub fn getpcpu() {
    todo!("port of OpenBSDProcessTable.c:126")
}

/// TODO: port of `static void OpenBSDProcessTable_scanProcs(OpenBSDProcessTable* this` from `OpenBSDProcessTable.c:133`.
pub fn OpenBSDProcessTable_scanProcs() {
    todo!("port of OpenBSDProcessTable.c:133")
}

/// TODO: port of `void ProcessTable_goThroughEntries(ProcessTable* super` from `OpenBSDProcessTable.c:242`.
pub fn ProcessTable_goThroughEntries() {
    todo!("port of OpenBSDProcessTable.c:242")
}
