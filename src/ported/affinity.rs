//! Stub scaffold for `Affinity.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `Affinity.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]


/// TODO: port of `Affinity* Affinity_new(Machine* host` from `Affinity.c:32`.
pub fn Affinity_new() {
    todo!("port of Affinity.c:32")
}

/// TODO: port of `void Affinity_delete(Affinity* this` from `Affinity.c:40`.
pub fn Affinity_delete() {
    todo!("port of Affinity.c:40")
}

/// TODO: port of `void Affinity_add(Affinity* this, unsigned int id` from `Affinity.c:45`.
pub fn Affinity_add() {
    todo!("port of Affinity.c:45")
}

/// TODO: port of `static Affinity* Affinity_get(const Process* p, Machine* host` from `Affinity.c:56`.
pub fn Affinity_get() {
    todo!("port of Affinity.c:56")
}

/// TODO: port of `static bool Affinity_set(Process* p, Arg arg` from `Affinity.c:77`.
pub fn Affinity_set() {
    todo!("port of Affinity.c:77")
}

/// TODO: port of `bool Affinity_rowSet(Row* row, Arg arg` from `Affinity.c:120`.
pub fn Affinity_rowSet() {
    todo!("port of Affinity.c:120")
}

/// TODO: port of `Affinity* Affinity_rowGet(const Row* row, Machine* host` from `Affinity.c:126`.
pub fn Affinity_rowGet() {
    todo!("port of Affinity.c:126")
}
