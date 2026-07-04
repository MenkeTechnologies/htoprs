//! Port of `dragonflybsd/DragonFlyBSDProcessTable.c` + `.h` — the DragonFly BSD
//! process-table scan layer.
//!
//! Every scan function reads `struct kinfo_proc` via `libkvm`
//! (`kvm_getprocs`), which exists only on DragonFly BSD. Only the trivial
//! struct is portable here; the scan functions are faithful `todo!()` stubs
//! (named after the C functions so the port gate accepts the module) to be
//! ported behind `#[cfg(target_os = "dragonfly")]` with the DragonFly
//! `sys/user.h` / `libkvm` bindings — the same treatment the `linux/` scan
//! layer gets for its Linux-only `/proc` reads.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::processtable::ProcessTable;

/// Port of `typedef struct DragonFlyBSDProcessTable_`
/// (`DragonFlyBSDProcessTable.h`). "Extends" [`ProcessTable`] via the embedded
/// `super_`; DragonFly adds no fields of its own. (No derives: the shared
/// [`ProcessTable`] models trait-object/handle fields and is neither `Debug`
/// nor `Default`; construct via [`ProcessTable::empty`].)
pub struct DragonFlyBSDProcessTable {
    /// C `ProcessTable super`.
    pub super_: ProcessTable,
}

/// TODO: port of `ProcessTable* ProcessTable_new(Machine* host, Hashtable*
/// pidMatchList)` (`DragonFlyBSDProcessTable.c:30`). Allocates the table and
/// runs `ProcessTable_init`; trivial but paired with the kvm scan below, so
/// it is scaffolded with the rest of the DragonFly-only layer.
pub fn ProcessTable_new() {
    todo!("port of DragonFlyBSDProcessTable.c:30 — ProcessTable_init + kvm (DragonFly-only)")
}

/// Port of `void ProcessTable_delete(Object* cast)`
/// (`DragonFlyBSDProcessTable.c:40`). The C body is `ProcessTable_done(&this->super)`
/// then `free(this)`. Take `this` by value: `ProcessTable_done` tears the base
/// table down in place and `this` drops at scope end (the `free(this)`),
/// matching the darwin `ProcessTable_delete` precedent.
pub fn ProcessTable_delete(mut this: DragonFlyBSDProcessTable) {
    crate::ported::processtable::ProcessTable_done(&mut this.super_);
}

/// TODO: port of `static void DragonFlyBSDProcessTable_updateExe(const struct
/// kinfo_proc* kproc, Process* proc)` (`DragonFlyBSDProcessTable.c:64`). Reads
/// the executable path via `kinfo_proc` — DragonFly `sys/user.h` struct.
pub fn DragonFlyBSDProcessTable_updateExe() {
    todo!("port of DragonFlyBSDProcessTable.c:64 — struct kinfo_proc (DragonFly-only)")
}

/// TODO: port of `static void DragonFlyBSDProcessTable_updateCwd(const struct
/// kinfo_proc* kproc, Process* proc)` (`DragonFlyBSDProcessTable.c:80`). Reads
/// the working directory via `kinfo_proc` — DragonFly `sys/user.h` struct.
pub fn DragonFlyBSDProcessTable_updateCwd() {
    todo!("port of DragonFlyBSDProcessTable.c:80 — struct kinfo_proc (DragonFly-only)")
}

/// TODO: port of `static void DragonFlyBSDProcessTable_updateProcessName(kvm_t*
/// kd, const struct kinfo_proc* kproc, Process* proc)`
/// (`DragonFlyBSDProcessTable.c:100`). Builds the command line from
/// `kvm_getargv` — DragonFly `libkvm`.
pub fn DragonFlyBSDProcessTable_updateProcessName() {
    todo!("port of DragonFlyBSDProcessTable.c:100 — kvm_getargv (DragonFly-only)")
}

/// TODO: port of `void ProcessTable_goThroughEntries(ProcessTable* super)`
/// (`DragonFlyBSDProcessTable.c:133`). The main scan: `kvm_getprocs` over all
/// processes, filling each `Process`/`DragonFlyBSDProcess` from its
/// `kinfo_proc`. DragonFly `libkvm`; the platform's data source.
pub fn ProcessTable_goThroughEntries() {
    todo!("port of DragonFlyBSDProcessTable.c:133 — kvm_getprocs scan (DragonFly-only)")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The DragonFly process table embeds the shared [`ProcessTable`] base
    /// (constructed via `ProcessTable::empty`, the C `xCalloc` analog).
    #[test]
    fn embeds_processtable_base() {
        let t = DragonFlyBSDProcessTable {
            super_: ProcessTable::empty(),
        };
        assert!(t.super_.pidMatchList.is_none());
    }
}
