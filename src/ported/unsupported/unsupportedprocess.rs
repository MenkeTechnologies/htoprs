//! Stub scaffold for `UnsupportedProcess.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `UnsupportedProcess.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]


/// TODO: port of `Process* UnsupportedProcess_new(const Machine* host` from `UnsupportedProcess.c:47`.
pub fn UnsupportedProcess_new() {
    todo!("port of UnsupportedProcess.c:47")
}

/// TODO: port of `void Process_delete(Object* cast` from `UnsupportedProcess.c:54`.
pub fn Process_delete() {
    todo!("port of UnsupportedProcess.c:54")
}

/// TODO: port of `static void UnsupportedProcess_rowWriteField(const Row* super, RichString* str, ProcessField field` from `UnsupportedProcess.c:61`.
pub fn UnsupportedProcess_rowWriteField() {
    todo!("port of UnsupportedProcess.c:61")
}

/// TODO: port of `static int UnsupportedProcess_compareByKey(const Process* v1, const Process* v2, ProcessField key` from `UnsupportedProcess.c:82`.
pub fn UnsupportedProcess_compareByKey() {
    todo!("port of UnsupportedProcess.c:82")
}
