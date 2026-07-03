//! Stub scaffold for `UnsupportedProcessTable.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `UnsupportedProcessTable.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable* pidMatchList` from `UnsupportedProcessTable.c:19`.
pub fn ProcessTable_new() {
    todo!("port of UnsupportedProcessTable.c:19")
}

/// TODO: port of `void ProcessTable_delete(Object* cast` from `UnsupportedProcessTable.c:29`.
pub fn ProcessTable_delete() {
    todo!("port of UnsupportedProcessTable.c:29")
}

/// TODO: port of `void ProcessTable_goThroughEntries(ProcessTable* super` from `UnsupportedProcessTable.c:35`.
pub fn ProcessTable_goThroughEntries() {
    todo!("port of UnsupportedProcessTable.c:35")
}
