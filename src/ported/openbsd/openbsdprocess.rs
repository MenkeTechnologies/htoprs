//! `OpenBSDProcess.c` — NOT yet ported (blocked on substrate).
//!
//! Every `pub fn` below is an honest `todo!()` placeholder named after its
//! C counterpart. The file is blocked on the `OpenBSDProcess` object struct, the
//! `Object_setClass` / `ProcessClass` vtable wiring, and the base
//! `Process_writeField` / `Process_compareByKey_Base` field paths.
//!
//! `gen_port_report.py` counts these `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `Process* OpenBSDProcess_new(const Machine* host` from `OpenBSDProcess.c:210`.
pub fn OpenBSDProcess_new() {
    todo!("port of OpenBSDProcess.c:210")
}

/// TODO: port of `void Process_delete(Object* cast` from `OpenBSDProcess.c:217`.
pub fn Process_delete() {
    todo!("port of OpenBSDProcess.c:217")
}

/// TODO: port of `static void OpenBSDProcess_rowWriteField(const Row* super, RichString* str, ProcessField field` from `OpenBSDProcess.c:223`.
pub fn OpenBSDProcess_rowWriteField() {
    todo!("port of OpenBSDProcess.c:223")
}

/// TODO: port of `static int OpenBSDProcess_compareByKey(const Process* v1, const Process* v2, ProcessField key` from `OpenBSDProcess.c:240`.
pub fn OpenBSDProcess_compareByKey() {
    todo!("port of OpenBSDProcess.c:240")
}
