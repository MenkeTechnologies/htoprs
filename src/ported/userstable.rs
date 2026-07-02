//! Stub scaffold for `UsersTable.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `UsersTable.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `UsersTable* UsersTable_new(void` from `UsersTable.c:20`.
pub fn UsersTable_new() {
    todo!("port of UsersTable.c:20")
}

/// TODO: port of `void UsersTable_delete(UsersTable* this` from `UsersTable.c:27`.
pub fn UsersTable_delete() {
    todo!("port of UsersTable.c:27")
}

/// TODO: port of `char* UsersTable_getRef(UsersTable* this, unsigned int uid` from `UsersTable.c:32`.
pub fn UsersTable_getRef() {
    todo!("port of UsersTable.c:32")
}

/// TODO: port of `inline void UsersTable_foreach(UsersTable* this, Hashtable_PairFunction f, void* userData` from `UsersTable.c:49`.
pub fn UsersTable_foreach() {
    todo!("port of UsersTable.c:49")
}
