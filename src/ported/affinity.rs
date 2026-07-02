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
//! - `Affinity_delete` (`Affinity.c:40`) — `free(this->cpus); free(this)`
//!   heap-teardown, ported as a by-value drop (the moved-in `Affinity` and
//!   its `Vec<u32>` `cpus` drop at end of scope, which *is* the two
//!   `free`s); `host` is a borrowed pointer C's `free` never touches, and a
//!   raw pointer is not dropped, matching the `Hashtable_delete` precedent.
//!
//! Ported, Linux-only (`#[cfg(target_os = "linux")]`): htop compiles the
//! affinity read/set path only under `HAVE_LIBHWLOC || HAVE_AFFINITY`
//! (`Affinity.c:54`/`88`/`118`). This port takes the `HAVE_AFFINITY`
//! branch (`Affinity.c:88`) — the Linux `sched_*` variant — via direct
//! `libc` FFI (`sched_getaffinity` / `sched_setaffinity` plus the
//! `CPU_ZERO` / `CPU_SET` / `CPU_ISSET` helpers), preserving htop's exact
//! `sizeof(cpu_set_t)` (get) and `sizeof(unsigned long)` (set) size
//! arguments — the latter a deliberate htop quirk kept verbatim. Those
//! syscalls exist only on Linux, so — exactly like the C preprocessor that
//! omits these functions entirely on platforms without affinity support —
//! the four functions are cfg-gated to Linux and are simply absent on the
//! darwin dev host. That absence is the faithful analog of the C `#if`
//! omission, not a fake stub:
//! - `Affinity_get` (`Affinity.c:90`), `Affinity_set` (`Affinity.c:105`),
//!   `Affinity_rowGet` (`Affinity.c:126`), `Affinity_rowSet`
//!   (`Affinity.c:120`). The `HAVE_LIBHWLOC` variants (`Affinity.c:56`/`77`)
//!   are the mutually-exclusive alternative build and are not ported.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::machine::Machine;

// Linux-only imports for the `sched_*` affinity path. Gated so the darwin
// build (where these functions do not exist, matching the C `#if`) sees no
// unused imports.
#[cfg(target_os = "linux")]
use crate::ported::object::{Arg, Object, Object_isA};
#[cfg(target_os = "linux")]
use crate::ported::process::{Process, Process_class, Process_getPid};
#[cfg(target_os = "linux")]
use core::any::Any;

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

/// Port of `void Affinity_delete(Affinity* this)` from `Affinity.c:40`.
/// C frees the backing `cpus` array then the struct (`free(this->cpus)` +
/// `free(this)`). Taking `this` by value is the faithful analog of
/// `free(this)`: the moved-in [`Affinity`] — and its `Vec<u32>` `cpus` —
/// drops at end of scope, which *is* the two C `free`s. `host` is a
/// borrowed raw pointer C's `free` never touches, and a raw pointer is not
/// dropped, so it is left alone here too. Same idiom as `Hashtable_delete`.
pub fn Affinity_delete(this: Affinity) {
    let _ = this;
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

/// Port of `static Affinity* Affinity_get(const Process* p, Machine* host)`
/// from `Affinity.c:90` (the `HAVE_AFFINITY` / Linux `sched_*` branch).
/// Reads the process's CPU affinity mask via `sched_getaffinity` (passing
/// htop's `sizeof(cpu_set_t)`); on failure returns `None` (C `NULL`),
/// otherwise builds a fresh [`Affinity`] and appends every existing CPU id
/// whose bit is set (`CPU_ISSET`), iterating `[0, host->existingCPUs)`.
///
/// Signature mapping: C `const Process* p` → `&Process`; C `Machine* host`
/// → `*mut Machine` (the borrowed back-pointer [`Affinity_new`] stores);
/// the C `Affinity*` / `NULL` return → `Option<Affinity>`. `host` is
/// dereferenced for `existingCPUs` (the only field read), which needs
/// `unsafe`; the `cpu_set_t` is zero-initialized before the syscall fills
/// it (the C leaves it uninitialized, relying on the kernel write).
#[cfg(target_os = "linux")]
pub fn Affinity_get(p: &Process, host: *mut Machine) -> Option<Affinity> {
    let mut cpuset: libc::cpu_set_t = unsafe { core::mem::zeroed() };
    let ok = unsafe {
        libc::sched_getaffinity(
            Process_getPid(p),
            core::mem::size_of::<libc::cpu_set_t>(),
            &mut cpuset,
        )
    } == 0;
    if !ok {
        return None;
    }

    let mut affinity = Affinity_new(host);
    let existingCPUs = unsafe { (*host).existingCPUs };
    for i in 0..existingCPUs {
        if unsafe { libc::CPU_ISSET(i as usize, &cpuset) } {
            Affinity_add(&mut affinity, i);
        }
    }
    Some(affinity)
}

/// Port of `static bool Affinity_set(Process* p, Arg arg)` from
/// `Affinity.c:105` (the `HAVE_AFFINITY` / Linux `sched_*` branch). Reads
/// the [`Affinity`] out of `arg.v`, builds a `cpu_set_t` with `CPU_ZERO` +
/// one `CPU_SET` per used CPU id, and applies it via `sched_setaffinity`,
/// returning whether the call succeeded.
///
/// Signature mapping: C `Process* p` → `&Process`; C `Arg arg` → the
/// ported [`Arg`] union (`arg.v` is the `Affinity*`, so the `Arg::V` arm
/// carries it — the `Arg::I` arm is impossible here, matching the C's
/// unconditional `arg.v` read). Dereferencing the type-erased pointer
/// needs `unsafe`, as does the FFI. The `sizeof(unsigned long)` size
/// argument is a deliberate htop quirk, kept verbatim as
/// `size_of::<c_ulong>()`.
#[cfg(target_os = "linux")]
pub fn Affinity_set(p: &Process, arg: Arg) -> bool {
    // Affinity* this = arg.v;
    let this: &Affinity = match arg {
        Arg::V(v) => unsafe { &*(v as *const Affinity) },
        Arg::I(_) => panic!("Affinity_set: Arg must carry the Affinity* in arg.v"),
    };

    let mut cpuset: libc::cpu_set_t = unsafe { core::mem::zeroed() };
    unsafe { libc::CPU_ZERO(&mut cpuset) };
    for i in 0..this.used {
        unsafe { libc::CPU_SET(this.cpus[i as usize] as usize, &mut cpuset) };
    }
    let ok = unsafe {
        libc::sched_setaffinity(
            Process_getPid(p),
            core::mem::size_of::<core::ffi::c_ulong>(),
            &cpuset,
        )
    } == 0;
    ok
}

/// Port of `bool Affinity_rowSet(Row* row, Arg arg)` from `Affinity.c:120`
/// (`HAVE_LIBHWLOC || HAVE_AFFINITY`). Casts the `Row*` to a `Process*`,
/// asserts the object really is a [`Process`], and delegates to
/// [`Affinity_set`].
///
/// Signature mapping: the C downcast `(Process*) row` + the
/// `Object_isA(..., &Process_class)` assert becomes `row: &dyn Object`
/// (the actual object implements [`Object`]), the ported [`Object_isA`]
/// guard, and an `Any` downcast to `Process` — the safe-Rust analog of the
/// C pointer cast validated by the same assert.
#[cfg(target_os = "linux")]
pub fn Affinity_rowSet(row: &dyn Object, arg: Arg) -> bool {
    // Process* p = (Process*) row;
    assert!(Object_isA(Some(row), &Process_class));
    let p = (row as &dyn Any)
        .downcast_ref::<Process>()
        .expect("Affinity_rowSet: row is not a Process");
    Affinity_set(p, arg)
}

/// Port of `Affinity* Affinity_rowGet(const Row* row, Machine* host)` from
/// `Affinity.c:126` (`HAVE_LIBHWLOC || HAVE_AFFINITY`). Casts the `Row*` to
/// a `Process*`, asserts the object really is a [`Process`], and delegates
/// to [`Affinity_get`]. Same `Row*`→`Process*` mapping as
/// [`Affinity_rowSet`]; returns `Option<Affinity>` (C `Affinity*` / `NULL`).
#[cfg(target_os = "linux")]
pub fn Affinity_rowGet(row: &dyn Object, host: *mut Machine) -> Option<Affinity> {
    // const Process* p = (const Process*) row;
    assert!(Object_isA(Some(row), &Process_class));
    let p = (row as &dyn Any)
        .downcast_ref::<Process>()
        .expect("Affinity_rowGet: row is not a Process");
    Affinity_get(p, host)
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

    // ── Linux-only `sched_*` affinity path ────────────────────────────
    //
    // These exercise the real `sched_getaffinity` / `sched_setaffinity`
    // syscalls, so they only compile (and run) on Linux — mirroring htop's
    // `#if defined(HAVE_AFFINITY)` gate. On darwin the four functions do
    // not exist, so the tests are cfg'd out and the module's `cargo test`
    // reduces to the platform-independent `Affinity_new`/`Affinity_add`
    // cases above.
    #[cfg(target_os = "linux")]
    use crate::ported::machine::Machine;
    #[cfg(target_os = "linux")]
    use crate::ported::object::{Arg, Object};
    #[cfg(target_os = "linux")]
    use crate::ported::process::{Process, Process_setPid};

    /// A `Machine` whose `existingCPUs` is the online CPU count — the
    /// `[0, existingCPUs)` bound `Affinity_get` iterates. Returns the
    /// machine plus a raw pointer to it (what `Affinity_*` take as `host`).
    #[cfg(target_os = "linux")]
    fn online_host() -> Machine {
        let ncpu = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
        let existing = if ncpu > 0 { ncpu as u32 } else { 1 };
        let mut host = Machine::default();
        host.existingCPUs = existing;
        host
    }

    /// A `Process` targeting the calling thread: pid 0 makes
    /// `sched_getaffinity` / `sched_setaffinity` operate on the current
    /// thread, so the syscalls always have a valid target.
    #[cfg(target_os = "linux")]
    fn current_thread_process() -> Process {
        let mut p = Process::default();
        Process_setPid(&mut p, 0);
        p
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn affinity_get_reports_current_thread_cpus() {
        let mut host = online_host();
        let existing = host.existingCPUs;
        let hp: *mut Machine = &mut host;
        let p = current_thread_process();

        let aff = Affinity_get(&p, hp).expect("sched_getaffinity(0) must succeed");
        // The calling thread runs on at least one allowed CPU.
        assert!(aff.used >= 1);
        // `host` is stored verbatim by `Affinity_new`.
        assert_eq!(aff.host, hp);
        // Every reported id is within the existing-CPU range the loop scans.
        for k in 0..aff.used as usize {
            assert!(aff.cpus[k] < existing);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn affinity_set_roundtrips_current_mask() {
        let mut host = online_host();
        let hp: *mut Machine = &mut host;
        let p = current_thread_process();

        // Read the current mask, then write the same set back: a subset of
        // the allowed CPUs, so `sched_setaffinity` must succeed.
        let mut aff = Affinity_get(&p, hp).expect("get must succeed");
        let arg = Arg::V(&mut aff as *mut Affinity as *mut core::ffi::c_void);
        assert!(Affinity_set(&p, arg));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn affinity_rowget_and_rowset_delegate_through_object_isa() {
        let mut host = online_host();
        let hp: *mut Machine = &mut host;
        let p = current_thread_process();

        // rowGet: the `Row*`→`Process*` cast + Object_isA guard + delegation.
        let mut aff = Affinity_rowGet(&p as &dyn Object, hp).expect("rowGet must succeed");
        assert!(aff.used >= 1);

        // rowSet: same guard, delegating to Affinity_set with the mask read back.
        let arg = Arg::V(&mut aff as *mut Affinity as *mut core::ffi::c_void);
        assert!(Affinity_rowSet(&p as &dyn Object, arg));
    }

    /// The `assert(Object_isA(..., &Process_class))` guard must reject an
    /// object that is a bare `Row` (its class chain is Row → Object, never
    /// Process), mirroring the C assert firing on a bad `(Process*)` cast.
    #[cfg(target_os = "linux")]
    #[test]
    #[should_panic]
    fn affinity_rowget_rejects_non_process_row() {
        let row = crate::ported::row::Row::default();
        let mut host = Machine::default();
        let hp: *mut Machine = &mut host;
        let _ = Affinity_rowGet(&row as &dyn Object, hp);
    }
}
