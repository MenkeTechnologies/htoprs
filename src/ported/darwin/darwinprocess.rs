//! `DarwinProcess.c` — NOT yet ported (blocked on substrate).
//!
//! Every `pub fn` below is an honest `todo!()` placeholder named after its
//! C counterpart. The file is blocked on unported substrate:
//! - the `DarwinProcess` object struct (`Process super` + `utime`/`stime`/
//!   `taskAccess`/`translated`) and the `Object_setClass` / `ProcessClass`
//!   vtable wiring needed by `DarwinProcess_new` / `Process_delete` /
//!   `DarwinProcess_rowWriteField` / `_compareByKey`.
//! - the `kinfo_proc` struct (absent from `libc`), required by
//!   `DarwinProcess_setFromKInfoProc` / `_updateCmdLine`.
//! - `Process_fillStarttimeBuffer` (stub in `process.rs`) and
//!   `ProcessTable_getProcess` (stub in `processtable.rs`), required by
//!   `setFromKInfoProc` and `scanThreads`.
//! - the `DarwinMachine` struct (`darwinmachine.rs`), read by
//!   `setFromLibprocPidinfo` for `host_info.max_mem`.
//!
//! The `Process_fields[]` field-descriptor table (`DarwinProcess.c:24`) is
//! data, not a function, and is deferred until the Darwin `ProcessField`
//! layer is modeled. `gen_port_report.py` counts these `todo!()` bodies as
//! *stubbed*, not *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `Process* DarwinProcess_new(const Machine* host` from `DarwinProcess.c:57`.
pub fn DarwinProcess_new() {
    todo!("port of DarwinProcess.c:57")
}

/// TODO: port of `void Process_delete(Object* cast` from `DarwinProcess.c:71`.
pub fn Process_delete() {
    todo!("port of DarwinProcess.c:71")
}

/// TODO: port of `static void DarwinProcess_rowWriteField(const Row* super, RichString* str, ProcessField field` from `DarwinProcess.c:78`.
pub fn DarwinProcess_rowWriteField() {
    todo!("port of DarwinProcess.c:78")
}

/// TODO: port of `static int DarwinProcess_compareByKey(const Process* v1, const Process* v2, ProcessField key` from `DarwinProcess.c:96`.
pub fn DarwinProcess_compareByKey() {
    todo!("port of DarwinProcess.c:96")
}

/// TODO: port of `static void DarwinProcess_updateExe(pid_t pid, Process* proc` from `DarwinProcess.c:109`.
pub fn DarwinProcess_updateExe() {
    todo!("port of DarwinProcess.c:109")
}

/// TODO: port of `static void DarwinProcess_updateCwd(pid_t pid, Process* proc` from `DarwinProcess.c:119`.
pub fn DarwinProcess_updateCwd() {
    todo!("port of DarwinProcess.c:119")
}

/// TODO: port of `static void DarwinProcess_updateCmdLine(const struct kinfo_proc* k, Process* proc` from `DarwinProcess.c:138`.
pub fn DarwinProcess_updateCmdLine() {
    todo!("port of DarwinProcess.c:138")
}

/// TODO: port of `static char* DarwinProcess_getDevname(dev_t dev` from `DarwinProcess.c:280`.
pub fn DarwinProcess_getDevname() {
    todo!("port of DarwinProcess.c:280")
}

/// TODO: port of `void DarwinProcess_setFromKInfoProc(Process* proc, const struct kinfo_proc* ps, bool exists` from `DarwinProcess.c:292`.
pub fn DarwinProcess_setFromKInfoProc() {
    todo!("port of DarwinProcess.c:292")
}

/// TODO: port of `void DarwinProcess_setFromLibprocPidinfo(DarwinProcess* proc, DarwinProcessTable* dpt, double timeIntervalNS` from `DarwinProcess.c:364`.
pub fn DarwinProcess_setFromLibprocPidinfo() {
    todo!("port of DarwinProcess.c:364")
}

/// TODO: port of `void DarwinProcess_scanThreads(DarwinProcess* dp, DarwinProcessTable* dpt` from `DarwinProcess.c:410`.
pub fn DarwinProcess_scanThreads() {
    todo!("port of DarwinProcess.c:410")
}
