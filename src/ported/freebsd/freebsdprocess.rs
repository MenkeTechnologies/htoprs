//! `FreeBSDProcess.c` — NOT yet ported (blocked on substrate).
//!
//! Every `pub fn` below is an honest `todo!()` placeholder named after its
//! C counterpart. The file is blocked on unported substrate:
//! - the `FreeBSDProcess` object struct (`Process super` + `kernel`/`jail`
//!   fields) and the `Object_setClass` / `ProcessClass` vtable wiring for
//!   `FreeBSDProcess_new` / `Process_delete` / `FreeBSDProcess_writeField` /
//!   `_compareByKey`.
//! - the base `Process_writeField` / `Process_compareByKey_Base` field paths.
//!
//! `gen_port_report.py` counts these `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `Process* FreeBSDProcess_new(const Machine* machine` from `FreeBSDProcess.c:62`.
pub fn FreeBSDProcess_new() {
    todo!("port of FreeBSDProcess.c:62")
}

/// TODO: port of `void Process_delete(Object* cast` from `FreeBSDProcess.c:69`.
pub fn Process_delete() {
    todo!("port of FreeBSDProcess.c:69")
}

/// TODO: port of `static void FreeBSDProcess_rowWriteField(const Row* super, RichString* str, ProcessField field` from `FreeBSDProcess.c:85`.
pub fn FreeBSDProcess_rowWriteField() {
    todo!("port of FreeBSDProcess.c:85")
}

/// TODO: port of `static int FreeBSDProcess_compareByKey(const Process* v1, const Process* v2, ProcessField key` from `FreeBSDProcess.c:120`.
pub fn FreeBSDProcess_compareByKey() {
    todo!("port of FreeBSDProcess.c:120")
}
