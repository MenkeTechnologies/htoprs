//! `NetBSDProcess.c` — NOT yet ported (blocked on substrate).
//!
//! Every `pub fn` below is an honest `todo!()` placeholder named after its
//! C counterpart. The file is blocked on the `NetBSDProcess` object struct and the
//! `Object_setClass` / `ProcessClass` vtable wiring plus the base
//! `Process_writeField` / `Process_compareByKey_Base` field paths.
//!
//! `gen_port_report.py` counts these `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `Process* NetBSDProcess_new(const Machine* host` from `NetBSDProcess.c:218`.
pub fn NetBSDProcess_new() {
    todo!("port of NetBSDProcess.c:218")
}

/// TODO: port of `void Process_delete(Object* cast` from `NetBSDProcess.c:225`.
pub fn Process_delete() {
    todo!("port of NetBSDProcess.c:225")
}

/// TODO: port of `static void NetBSDProcess_rowWriteField(const Row* super, RichString* str, ProcessField field` from `NetBSDProcess.c:231`.
pub fn NetBSDProcess_rowWriteField() {
    todo!("port of NetBSDProcess.c:231")
}

/// TODO: port of `static int NetBSDProcess_compareByKey(const Process* v1, const Process* v2, ProcessField key` from `NetBSDProcess.c:248`.
pub fn NetBSDProcess_compareByKey() {
    todo!("port of NetBSDProcess.c:248")
}
