//! Stub scaffold for `LibNl.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `LibNl.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]


/// TODO: port of `static void unload_libnl(void` from `LibNl.c:48`.
pub fn unload_libnl() {
    todo!("port of LibNl.c:48")
}

/// TODO: port of `static int load_libnl(void` from `LibNl.c:77`.
pub fn load_libnl() {
    todo!("port of LibNl.c:77")
}

/// TODO: port of `static void initNetlinkSocket(LinuxProcessTable* this` from `LibNl.c:134`.
pub fn initNetlinkSocket() {
    todo!("port of LibNl.c:134")
}

/// TODO: port of `void LibNl_destroyNetlinkSocket(LinuxProcessTable* this` from `LibNl.c:149`.
pub fn LibNl_destroyNetlinkSocket() {
    todo!("port of LibNl.c:149")
}

/// TODO: port of `static int handleNetlinkMsg(struct nl_msg* nlmsg, void* linuxProcess` from `LibNl.c:161`.
pub fn handleNetlinkMsg() {
    todo!("port of LibNl.c:161")
}

/// TODO: port of `void LibNl_readDelayAcctData(LinuxProcessTable* this, LinuxProcess* process` from `LibNl.c:199`.
pub fn LibNl_readDelayAcctData() {
    todo!("port of LibNl.c:199")
}
