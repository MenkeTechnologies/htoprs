//! `SolarisProcessTable.c` — NOT yet ported (blocked on substrate).
//!
//! Every `pub fn` below is an honest `todo!()` placeholder named after its
//! C counterpart. The file is blocked on `libproc` (`proc_walk`) plus the `psinfo`/`pstatus`/
//! `lwpsinfo` `/proc` readers, the stubbed `ProcessTable_getProcess`
//! (`processtable.rs`), and the `SolarisProcess` / `SolarisProcessTable`
//! object structs.
//!
//! `gen_port_report.py` counts these `todo!()` bodies as *stubbed*, not
//! *ported*, so the scaffold does not inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `static char* SolarisProcessTable_readZoneName(kstat_ctl_t* kd, SolarisProcess* sproc` from `SolarisProcessTable.c:34`.
pub fn SolarisProcessTable_readZoneName() {
    todo!("port of SolarisProcessTable.c:34")
}

/// TODO: port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable* pidMatchList` from `SolarisProcessTable.c:49`.
pub fn ProcessTable_new() {
    todo!("port of SolarisProcessTable.c:49")
}

/// TODO: port of `void ProcessTable_delete(Object* cast` from `SolarisProcessTable.c:59`.
pub fn ProcessTable_delete() {
    todo!("port of SolarisProcessTable.c:59")
}

/// TODO: port of `static void SolarisProcessTable_updateExe(pid_t pid, Process* proc` from `SolarisProcessTable.c:65`.
pub fn SolarisProcessTable_updateExe() {
    todo!("port of SolarisProcessTable.c:65")
}

/// TODO: port of `static void SolarisProcessTable_updateCwd(pid_t pid, Process* proc` from `SolarisProcessTable.c:78`.
pub fn SolarisProcessTable_updateCwd() {
    todo!("port of SolarisProcessTable.c:78")
}

/// TODO: port of `static inline ProcessState SolarisProcessTable_getProcessState(char state` from `SolarisProcessTable.c:92`.
pub fn SolarisProcessTable_getProcessState() {
    todo!("port of SolarisProcessTable.c:92")
}

/// TODO: port of `static int SolarisProcessTable_walkproc(psinfo_t* _psinfo, lwpsinfo_t* _lwpsinfo, void* listptr` from `SolarisProcessTable.c:110`.
pub fn SolarisProcessTable_walkproc() {
    todo!("port of SolarisProcessTable.c:110")
}

/// TODO: port of `void ProcessTable_goThroughEntries(ProcessTable* super` from `SolarisProcessTable.c:266`.
pub fn ProcessTable_goThroughEntries() {
    todo!("port of SolarisProcessTable.c:266")
}
