//! Port of `Affinity.c` — a growable CPU-id set used to read and set a
//! process's CPU affinity mask.
//!
//! C names are preserved verbatim (`Affinity_add`, …), so
//! `non_snake_case` is allowed for the whole module.
//!
//! Ported (self-contained, no unported substrate):
//! - `Affinity_new` (`Affinity.c:32`) — constructor: `size = 8`,
//!   `used = 0`, an 8-slot `cpus` buffer, and the borrowed `host`. C heap
//!   allocates and returns a pointer; the faithful analog returns an owned
//!   `Affinity` by value (the same idiom `History_new` uses). `Machine` is
//!   now modeled, so `host` is stored as a raw `*mut Machine` borrowed
//!   pointer — the `Arg` `void* v` precedent (`Object.c`): keeping a raw
//!   pointer needs no `unsafe`; only dereferencing it would.
//! - `Affinity_add` (`Affinity.c:45`) — append, doubling capacity when
//!   the array is full.
//!
//! Stubbed (cannot be ported faithfully yet):
//! - `Affinity_delete` (`Affinity.c:40`) is a `free(this->cpus); free(this)`
//!   heap-teardown with no faithful safe-Rust analog — Rust owns the
//!   `cpus` allocation and drops it automatically (`Vec` + `Drop`); `host`
//!   is a borrowed pointer that C's `free` does not touch either. Left
//!   stubbed, matching the `History_delete` precedent.
//! - `Affinity_get` (`Affinity.c:56`/`90`) calls `sched_getaffinity`
//!   (`HAVE_AFFINITY`) or `hwloc_get_proc_cpubind` (`HAVE_LIBHWLOC`). Both
//!   need a raw-syscall/FFI layer (`libc`/`nix`); the crate depends on
//!   neither, and `sched_getaffinity` is Linux-only. Blocked on the
//!   platform affinity syscall layer. `Process_getPid` and
//!   `Machine::existingCPUs` are modeled, but the syscall itself is not
//!   reachable.
//! - `Affinity_set` (`Affinity.c:77`/`105`) — `sched_setaffinity` /
//!   `hwloc_set_proc_cpubind`. Same syscall blocker as `Affinity_get`.
//! - `Affinity_rowGet` (`Affinity.c:126`) casts `Row*`→`Process*`, asserts
//!   `Object_isA(&Process_class)`, and delegates to `Affinity_get`.
//!   Blocked transitively on `Affinity_get`.
//! - `Affinity_rowSet` (`Affinity.c:120`) casts `Row*`→`Process*`, asserts,
//!   and delegates to `Affinity_set`. Blocked transitively on
//!   `Affinity_set`.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;

/// A growable set of CPU ids. Faithful to `struct Affinity_` in
/// `Affinity.h:26`. `host` is the borrowed `Machine*` (raw pointer — the
/// `Arg` `void* v` precedent; never dereferenced by ported code), `size`
/// is the capacity in slots, `used` the number of filled slots, and
/// `cpus` the backing array (length always equals `size`, matching the C
/// heap buffer sized `sizeof(unsigned int) * size`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Affinity {
    pub host: *mut Machine,
    pub size: u32,
    pub used: u32,
    pub cpus: Vec<u32>,
}

/// Port of `Affinity* Affinity_new(Machine* host)` from `Affinity.c:32`.
/// C `xCalloc`s the struct (so `used == 0`), sets `size = 8`, allocates
/// an 8-slot `cpus` buffer, and stores the borrowed `host`. The C heap
/// pointer is modeled as an owned value returned by move (same idiom as
/// `History_new`); the zero-initialized `cpus` matches `xCalloc`.
pub fn Affinity_new(host: *mut Machine) -> Affinity {
    Affinity {
        host,
        size: 8,
        used: 0,
        cpus: vec![0; 8],
    }
}

/// TODO: port of `void Affinity_delete(Affinity* this` from `Affinity.c:40`.
pub fn Affinity_delete() {
    todo!("port of Affinity.c:40")
}

/// Port of `Affinity_add(Affinity* this, unsigned int id)` from
/// `Affinity.c:45`. Appends `id`, doubling capacity when the array is
/// full. The C code reallocs `cpus` to `sizeof(unsigned int) * size`;
/// here the backing `Vec` is resized to the new `size` (new slots
/// zero-filled — they are always written before read), keeping its
/// length in lock-step with `size` as the C buffer does.
pub fn Affinity_add(this: &mut Affinity, id: u32) {
    if this.used == this.size {
        this.size *= 2;
        this.cpus.resize(this.size as usize, 0);
    }
    this.cpus[this.used as usize] = id;
    this.used += 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the initial state `Affinity_new` (`Affinity.c:32-37`)
    /// produces: `size = 8`, `used = 0`, `cpus` a `size`-length buffer.
    /// A null `host` stands in for the borrowed `Machine*` — ported code
    /// never dereferences it.
    fn fresh() -> Affinity {
        Affinity_new(core::ptr::null_mut())
    }

    #[test]
    fn affinity_new_initial_state() {
        let a = fresh();
        assert_eq!(a.size, 8);
        assert_eq!(a.used, 0);
        assert_eq!(a.cpus, vec![0; 8]);
        assert!(a.host.is_null());
    }

    #[test]
    fn affinity_add_appends_within_capacity() {
        let mut a = fresh();
        for id in 0..8u32 {
            Affinity_add(&mut a, id * 10);
        }
        assert_eq!(a.used, 8);
        assert_eq!(a.size, 8); // no growth yet — used reached but never exceeded size mid-append
        assert_eq!(&a.cpus[..8], &[0, 10, 20, 30, 40, 50, 60, 70]);
    }

    #[test]
    fn affinity_add_doubles_capacity_on_overflow() {
        let mut a = fresh();
        // Fill the initial 8 slots.
        for id in 0..8u32 {
            Affinity_add(&mut a, id);
        }
        assert_eq!(a.size, 8);
        assert_eq!(a.used, 8);
        // The 9th append triggers `size *= 2` -> 16 before storing.
        Affinity_add(&mut a, 100);
        assert_eq!(a.size, 16);
        assert_eq!(a.used, 9);
        assert_eq!(a.cpus.len(), 16);
        assert_eq!(a.cpus[8], 100);
    }

    #[test]
    fn affinity_add_doubles_repeatedly() {
        let mut a = fresh();
        // Append 17 ids: capacity grows 8 -> 16 (at the 9th) -> 32 (at the 17th).
        for id in 0..17u32 {
            Affinity_add(&mut a, id);
        }
        assert_eq!(a.used, 17);
        assert_eq!(a.size, 32);
        assert_eq!(a.cpus.len(), 32);
        // Every appended id is preserved in order.
        for id in 0..17u32 {
            assert_eq!(a.cpus[id as usize], id);
        }
    }

    #[test]
    fn affinity_add_preserves_existing_on_growth() {
        let mut a = fresh();
        for id in 0..8u32 {
            Affinity_add(&mut a, 1000 + id);
        }
        Affinity_add(&mut a, 9999); // forces realloc/resize
                                    // Prior contents survive the growth.
        for id in 0..8u32 {
            assert_eq!(a.cpus[id as usize], 1000 + id);
        }
        assert_eq!(a.cpus[8], 9999);
    }
}
