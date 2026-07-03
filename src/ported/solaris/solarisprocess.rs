//! `SolarisProcess.c` — NOT yet ported (blocked on substrate).
//!
//! Every `pub fn` below is an honest `todo!()` placeholder named after its
//! C counterpart. The file is blocked on the `SolarisProcess` object struct (`Process super`
//! + zone/contract/`lwpid`/`taskid`/`poolid` fields), the `Object_setClass` /
//! `ProcessClass` vtable wiring, and the base `Process_writeField` /
//! `Process_compareByKey_Base` field paths.
//!
//! `gen_port_report.py` counts these `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `Process* SolarisProcess_new(const Machine* host` from `SolarisProcess.c:63`.
pub fn SolarisProcess_new() {
    todo!("port of SolarisProcess.c:63")
}

/// TODO: port of `void Process_delete(Object* cast` from `SolarisProcess.c:70`.
pub fn Process_delete() {
    todo!("port of SolarisProcess.c:70")
}

/// TODO: port of `static void SolarisProcess_rowWriteField(const Row* super, RichString* str, ProcessField field` from `SolarisProcess.c:77`.
pub fn SolarisProcess_rowWriteField() {
    todo!("port of SolarisProcess.c:77")
}

/// TODO: port of `static int SolarisProcess_compareByKey(const Process* v1, const Process* v2, ProcessField key` from `SolarisProcess.c:104`.
pub fn SolarisProcess_compareByKey() {
    todo!("port of SolarisProcess.c:104")
}
