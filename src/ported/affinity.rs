//! Port of `Affinity.c` — a growable CPU-id set used to read and set a
//! process's CPU affinity mask.
//!
//! C names are preserved verbatim (`Affinity_add`, …), so
//! `non_snake_case` is allowed for the whole module.
//!
//! Only the pure data-structure operation `Affinity_add` is ported. The
//! rest of the file is intentionally left as stubs:
//!
//! * `Affinity_new` (`Affinity.c:32`) takes a `Machine* host` and stores
//!   it on the struct; `Machine` is not modeled yet, so the constructor
//!   is stubbed. Tests build the struct literal directly, mirroring the
//!   `size = 8` initial allocation the C constructor performs.
//! * `Affinity_delete` (`Affinity.c:40`) is a `free(this->cpus); free(this)`
//!   heap-teardown with no faithful safe-Rust analog — Rust owns the
//!   allocation and drops it automatically (`Vec` + `Drop`). Left stubbed.
//! * `Affinity_get`, `Affinity_set`, `Affinity_rowGet`, `Affinity_rowSet`
//!   call `sched_getaffinity`/`sched_setaffinity` (or hwloc) and cast
//!   `Row`/`Process` pointers. Those touch platform syscalls and types
//!   not yet ported, so they remain stubs.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// A growable set of CPU ids. Faithful to `struct Affinity_` in
/// `Affinity.h:26` (the `Machine* host` field is omitted — `Machine` is
/// not modeled and the fns that use it are stubbed). `size` is the
/// capacity in slots, `used` the number of filled slots, and `cpus` the
/// backing array (length always equals `size`, matching the C heap
/// buffer sized `sizeof(unsigned int) * size`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Affinity {
    pub size: u32,
    pub used: u32,
    pub cpus: Vec<u32>,
}

/// TODO: port of `Affinity* Affinity_new(Machine* host` from `Affinity.c:32`.
pub fn Affinity_new() {
    todo!("port of Affinity.c:32")
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
    fn fresh() -> Affinity {
        Affinity {
            size: 8,
            used: 0,
            cpus: vec![0; 8],
        }
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
