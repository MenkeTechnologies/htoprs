//! `NetBSDProcessTable.c` — NOT yet ported (blocked on substrate).
//!
//! Every `pub fn` below is an honest `todo!()` placeholder named after its
//! C counterpart. The file is blocked on `libkvm` (`kvm_getproc2`) plus the `kinfo_proc2`
//! struct, the stubbed `ProcessTable_getProcess` (`processtable.rs`), and the
//! `NetBSDProcess` / `NetBSDProcessTable` object structs.
//!
//! `gen_port_report.py` counts these `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable* pidMatchList` from `NetBSDProcessTable.c:40`.
pub fn ProcessTable_new() {
    todo!("port of NetBSDProcessTable.c:40")
}

/// TODO: port of `void ProcessTable_delete(Object* cast` from `NetBSDProcessTable.c:50`.
pub fn ProcessTable_delete() {
    todo!("port of NetBSDProcessTable.c:50")
}

/// TODO: port of `static void NetBSDProcessTable_updateExe(const struct kinfo_proc2* kproc, Process* proc` from `NetBSDProcessTable.c:56`.
pub fn NetBSDProcessTable_updateExe() {
    todo!("port of NetBSDProcessTable.c:56")
}

/// TODO: port of `static void NetBSDProcessTable_updateCwd(const struct kinfo_proc2* kproc, Process* proc` from `NetBSDProcessTable.c:74`.
pub fn NetBSDProcessTable_updateCwd() {
    todo!("port of NetBSDProcessTable.c:74")
}

/// TODO: port of `static void NetBSDProcessTable_updateProcessName(kvm_t* kd, const struct kinfo_proc2* kproc, Process* proc` from `NetBSDProcessTable.c:94`.
pub fn NetBSDProcessTable_updateProcessName() {
    todo!("port of NetBSDProcessTable.c:94")
}

/// TODO: port of `static double getpcpu(const NetBSDMachine* nhost, const struct kinfo_proc2* kp` from `NetBSDProcessTable.c:146`.
pub fn getpcpu() {
    todo!("port of NetBSDProcessTable.c:146")
}

/// TODO: port of `static ProcessState get_active_status(const NetBSDMachine* nhost, const struct kinfo_proc2* kproc` from `NetBSDProcessTable.c:153`.
pub fn get_active_status() {
    todo!("port of NetBSDProcessTable.c:153")
}

/// TODO: port of `void ProcessTable_goThroughEntries(ProcessTable* super` from `NetBSDProcessTable.c:171`.
pub fn ProcessTable_goThroughEntries() {
    todo!("port of NetBSDProcessTable.c:171")
}
