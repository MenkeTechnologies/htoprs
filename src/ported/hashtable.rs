//! Port of `Hashtable.c` — htop's open-addressing hash table with linear
//! probing and Robin-Hood displacement.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C function takes
//! `Hashtable* this`; the faithful analog is a free fn taking
//! `this: &mut Hashtable` / `this: &Hashtable` (the same shape the
//! `Vector.c` / `History.c` ports use: free fns, not methods).
//!
//! # C model
//!
//! ```c
//! typedef struct HashtableItem_ {
//!    ht_key_t key;       // unsigned int
//!    size_t   probe;     // linear-probe distance from the ideal slot
//!    void*    value;     // NULL == empty bucket
//! } HashtableItem;
//!
//! struct Hashtable_ {
//!    size_t size;                // bucket count (always a prime)
//!    HashtableItem* buckets;     // xCalloc'd array (zeroed == all empty)
//!    size_t items;               // live entries
//!    bool owner;                 // frees values it holds when true
//! };
//! ```
//!
//! The table is open-addressing: `key % size` is the ideal slot, and on
//! collision it probes forward one slot at a time, using Robin-Hood
//! displacement (an incoming item with a longer probe distance evicts a
//! resident with a shorter one) to keep probe distances low.
//!
//! # Rust model
//!
//! - `HashtableItem` is modeled faithfully: `key: u32` (C `ht_key_t` ==
//!   `unsigned int`), `probe: usize` (C `size_t`), and `value:
//!   Option<Box<dyn Object>>`. The `Option` is the C `void* value ==
//!   NULL` empty-bucket test: `None` is an empty bucket, `Some` a filled
//!   one. The payload is `Box<dyn Object>` — the same way `vector.rs` /
//!   `panel.rs` model an owned `Object*`.
//! - `buckets` is a `Vec<HashtableItem>` of exactly `size` slots (C's
//!   `xCalloc(size, …)`), each initialized empty (C's zero-fill).
//! - `key % size`, the load-factor thresholds, the Robin-Hood probe
//!   bookkeeping, and the backward-shift on removal are all ported
//!   verbatim from the C.
//!
//! ## The `owner` flag under owned `Box` values
//!
//! htop's `void* value` is owned-or-borrowed depending on the `owner`
//! flag: an owner table frees the values it holds (`Hashtable_clear`,
//! overwrite in `insert`, `Hashtable_remove`), a non-owner table only
//! references values owned elsewhere and never frees them. The faithful
//! safe-Rust analog of the OWNING case is `Box<dyn Object>`: the table
//! owns the box, and dropping it *is* the C `free`. The `owner` field is
//! retained and drives the one place the two cases differ observably —
//! `Hashtable_remove` returns the value to the caller when `!owner` and
//! drops it (C `free`) when `owner`.
//!
//! In the owned-`Box` model the table is always the sole owner of every
//! value it stores, so the non-owner "reference something owned
//! elsewhere" reading collapses: clearing / overwriting a slot always
//! drops the box regardless of `owner`, because there is no separate
//! owner to leave it to. This differs from C only for a `!owner` table's
//! clear/overwrite (C leaves the borrowed pointer to its real owner; here
//! the box is dropped) — a safe-Rust ownership consequence, not an
//! algorithmic one, and it does not change probing, sizing, or lookup.
//!
//! # Ported
//! - `Hashtable_new` (`Hashtable.c:117`)
//! - `Hashtable_clear` (`Hashtable.c:139`)
//! - `insert` (`Hashtable.c:152`, `static`)
//! - `Hashtable_setSize` (`Hashtable.c:195`)
//! - `Hashtable_put` (`Hashtable.c:226`)
//! - `Hashtable_remove` (`Hashtable.c:247`)
//! - `Hashtable_get` (`Hashtable.c:302`)
//! - `Hashtable_foreach` (`Hashtable.c:330`)
//! - `Hashtable_count` (`Hashtable.c:79`, under `#ifndef NDEBUG`)
//! - `Hashtable_delete` (`Hashtable.c:132`)
//! - `Hashtable_dump` (`Hashtable.c:42`, `static`, under `#ifndef NDEBUG`)
//! - `Hashtable_isConsistent` (`Hashtable.c:67`, `static`, under
//!   `#ifndef NDEBUG`)
//! - `nextPrime` + its `OEISprimes` table (pure prime-table math)
//!
//! ## `Hashtable_delete` under owned fields
//!
//! The C teardown is `Hashtable_clear(this)` + `free(this->buckets)` +
//! `free(this)`. `Hashtable` owns its `Vec<HashtableItem>` (and each
//! `Box<dyn Object>` value), so taking `this` by value is the faithful
//! analog of `free(this)`: the moved-in struct drops at end of scope,
//! which is the two C `free`s. The explicit `Hashtable_clear` call
//! mirrors the C's first line verbatim.
//!
//! ## The debug helpers `Hashtable_dump` / `Hashtable_isConsistent`
//!
//! Both are `static` and compiled only under `#ifndef NDEBUG`: `dump`
//! prints the table to stderr, and `isConsistent` recomputes `items` for
//! the `assert(Hashtable_isConsistent(this))` guards sprinkled through
//! the C. They are ported verbatim (`fprintf(stderr, …)` → `eprintln!`),
//! but — like the `Vector.c` port's dropped asserts (`vector.rs`) — the
//! `isConsistent` assert *call sites* inside the already-ported functions
//! are NOT re-added: they check the port's own bookkeeping, which Rust's
//! owned `Vec`/`Option` maintain structurally, and re-adding them would
//! mean editing ported functions. So `isConsistent` (and, transitively,
//! `dump`, which only `isConsistent` calls) is unused — carried under
//! `#[allow(dead_code)]` as a faithful debug helper, exactly as the
//! public `Hashtable_count` (which recomputes the live count and
//! `assert`s it equals `items`) pins the same invariant from the tests
//! below. `CRT_fatalError` (`Hashtable.c:114`, `:235`) is rendered as
//! `panic!` with the C message, matching the `nextPrime` port's choice —
//! the real `CRT_fatalError` (`crt.rs`) restores the terminal and exits,
//! which is untestable and unwanted for a pure container.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // preserve the C table name `OEISprimes` verbatim

use crate::ported::object::Object;

/// Port of `static const uint64_t OEISprimes[]` from `Hashtable.c:92`
/// (<https://oeis.org/A014234>).
///
/// In the C the table is tiered by `#if SIZE_MAX > UINT16_MAX` and
/// `#if SIZE_MAX > UINT32_MAX`: 16-bit targets get only the first
/// 14 entries, 32-bit targets add through `4294967291`, and 64-bit
/// targets add the final five through `137438953447`. htoprs targets
/// 64-bit platforms only (macOS aarch64, Linux x86_64/aarch64), where
/// `SIZE_MAX > UINT32_MAX` holds, so the FULL table (all three tiers)
/// is included. Typed as `usize` since the C uses these values as
/// `size_t`.
const OEISprimes: [usize; 35] = [
    7,
    13,
    31,
    61,
    127,
    251,
    509,
    1021,
    2039,
    4093,
    8191,
    16381,
    32749,
    65521,
    // #if SIZE_MAX > UINT16_MAX
    131071,
    262139,
    524287,
    1048573,
    2097143,
    4194301,
    8388593,
    16777213,
    33554393,
    67108859,
    134217689,
    268435399,
    536870909,
    1073741789,
    2147483647,
    4294967291,
    // #if SIZE_MAX > UINT32_MAX
    8589934583,
    17179869143,
    34359738337,
    68719476731,
    137438953447,
];

/// Port of `static size_t nextPrime(size_t n)` from `Hashtable.c:106`.
/// Returns the smallest entry in `OEISprimes` that is `>= n` (the C
/// `<=` test is inclusive). The C falls through to
/// `CRT_fatalError("Hashtable: no prime found")` (which aborts) when no
/// table entry is large enough; that is ported as a `panic!` with the
/// same message. No fabricated fallback value is returned.
pub fn nextPrime(n: usize) -> usize {
    for &prime in &OEISprimes {
        if n <= prime {
            return prime;
        }
    }
    panic!("Hashtable: no prime found");
}

/// Port of `typedef struct HashtableItem_` from `Hashtable.c:26`. A
/// single bucket: `key`/`probe` mirror the C fields exactly, and
/// `value: Option<Box<dyn Object>>` is the C `void* value` where `None`
/// is the empty-bucket sentinel (C `NULL`) and `Some` an occupied slot.
struct HashtableItem {
    /// C `ht_key_t key` (== `unsigned int`).
    key: u32,
    /// C `size_t probe`: linear-probe distance from `key % size`.
    probe: usize,
    /// C `void* value`: `None` == C `NULL` == empty bucket.
    value: Option<Box<dyn Object>>,
}

/// Port of `struct Hashtable_` from `Hashtable.c:32`. Open-addressing
/// table over a `Vec<HashtableItem>` of `size` (prime) slots, tracking a
/// live-entry count and the owner flag.
pub struct Hashtable {
    /// C `size_t size`: bucket count, always a prime from `nextPrime`.
    size: usize,
    /// C `HashtableItem* buckets`: exactly `size` slots, zero-filled to
    /// empty at allocation.
    buckets: Vec<HashtableItem>,
    /// C `size_t items`: number of filled buckets.
    items: usize,
    /// C `bool owner`: when true the table frees the values it drops (see
    /// the module-level note on owner semantics under `Box` values).
    owner: bool,
}

/// Port of `static void Hashtable_dump(const Hashtable* this)` from
/// `Hashtable.c:42` (defined under `#ifndef NDEBUG`). Prints a header
/// line, one line per bucket, and a footer to stderr — the faithful
/// analog of the C's three `fprintf(stderr, …)` groups. The C `%p`
/// pointer prints map to Rust `{:p}`: `(const void*)this` becomes
/// `this as *const Hashtable`, and each bucket's `void* value` becomes
/// the underlying object's data pointer (`std::ptr::null()` for an empty
/// bucket, matching C's `NULL`). Only reached from
/// [`Hashtable_isConsistent`] on a bookkeeping mismatch, so it is
/// `#[allow(dead_code)]` (see the module note on the debug helpers).
#[allow(dead_code)]
fn Hashtable_dump(this: &Hashtable) {
    eprintln!(
        "Hashtable {:p}: size={} items={} owner={}",
        this as *const Hashtable,
        this.size,
        this.items,
        if this.owner { "yes" } else { "no" }
    );

    let mut items = 0;
    for i in 0..this.size {
        let value_ptr: *const () = this.buckets[i]
            .value
            .as_deref()
            .map_or(std::ptr::null(), |v| v as *const dyn Object as *const ());
        eprintln!(
            "  item {:5}: key = {:5} probe = {:2} value = {:p}",
            i, this.buckets[i].key, this.buckets[i].probe, value_ptr
        );

        if this.buckets[i].value.is_some() {
            items += 1;
        }
    }

    eprintln!(
        "Hashtable {:p}: items={} counted={}",
        this as *const Hashtable, this.items, items
    );
}

/// Port of `static bool Hashtable_isConsistent(const Hashtable* this)`
/// from `Hashtable.c:67` (defined under `#ifndef NDEBUG`). Recomputes the
/// live-entry count by walking every bucket, returns whether it equals
/// the maintained `items`, and dumps the table via [`Hashtable_dump`] on
/// a mismatch. In the C this backs the `assert(Hashtable_isConsistent(…))`
/// guards; those assert call sites are not re-added to the ported
/// functions (see the module note), so this is `#[allow(dead_code)]`.
#[allow(dead_code)]
fn Hashtable_isConsistent(this: &Hashtable) -> bool {
    let mut items = 0;
    for i in 0..this.size {
        if this.buckets[i].value.is_some() {
            items += 1;
        }
    }
    let res = items == this.items;
    if !res {
        Hashtable_dump(this);
    }
    res
}

/// Port of `size_t Hashtable_count(const Hashtable* this)` from
/// `Hashtable.c:79` (defined under `#ifndef NDEBUG`). Walks every bucket,
/// counts the filled ones, `assert`s the tally equals the maintained
/// `items` (the C `assert(items == this->items)`), and returns it.
pub fn Hashtable_count(this: &Hashtable) -> usize {
    let mut items = 0;
    for bucket in this.buckets.iter() {
        if bucket.value.is_some() {
            items += 1;
        }
    }
    assert!(items == this.items);
    items
}

/// Port of `Hashtable* Hashtable_new(size_t size, bool owner)` from
/// `Hashtable.c:117`. Rounds `size` up to a prime via [`nextPrime`] (or
/// defaults to `13` when `size == 0`, exactly as the C
/// `size ? nextPrime(size) : 13`), then allocates that many empty
/// buckets. The C `xCalloc` zero-fill — every `value == NULL` — is the
/// `value: None` each slot is built with.
pub fn Hashtable_new(size: usize, owner: bool) -> Hashtable {
    let size = if size != 0 { nextPrime(size) } else { 13 };

    let buckets = (0..size)
        .map(|_| HashtableItem {
            key: 0,
            probe: 0,
            value: None,
        })
        .collect();

    Hashtable {
        items: 0,
        size,
        buckets,
        owner,
    }
}

/// Port of `void Hashtable_delete(Hashtable* this)` from `Hashtable.c:132`.
/// Clears the table (C `Hashtable_clear(this)`) then frees the bucket
/// array and the struct (C `free(this->buckets)` + `free(this)`). Taking
/// `this` by value is the faithful analog of `free(this)`: the moved-in
/// `Hashtable` — and its `Vec<HashtableItem>` / each `Box<dyn Object>` —
/// drops at end of scope, which *is* the two C `free`s. The explicit
/// `Hashtable_clear` call mirrors the C's first line.
pub fn Hashtable_delete(mut this: Hashtable) {
    Hashtable_clear(&mut this);
}

/// Port of `void Hashtable_clear(Hashtable* this)` from `Hashtable.c:139`.
/// Empties every bucket and resets `items` to `0`. The C frees each value
/// only when `owner`, then `memset`s the bucket array to zero; in the
/// owned-`Box` model setting each `value` to `None` drops the box (the
/// C `free`), so both cases funnel through the same clear — see the
/// module-level note on `owner` under owned values.
pub fn Hashtable_clear(this: &mut Hashtable) {
    for bucket in this.buckets.iter_mut() {
        bucket.key = 0;
        bucket.probe = 0;
        bucket.value = None;
    }

    this.items = 0;
}

/// Port of `static void insert(Hashtable* this, ht_key_t key, void* value)`
/// from `Hashtable.c:152`. Linear-probe insert with Robin-Hood
/// displacement: probe forward from `key % size`; drop into the first
/// empty slot, overwrite on a key match, and whenever the carried item's
/// probe distance exceeds the resident's, swap the two and keep probing
/// with the evicted item. The `origIndex` full-table `assert` is omitted
/// (debug-only); the load factor kept below 1 by `Hashtable_put`
/// guarantees an empty slot is always found.
fn insert(this: &mut Hashtable, mut key: u32, mut value: Box<dyn Object>) {
    let mut index = (key as usize) % this.size;
    let mut probe: usize = 0;

    loop {
        if this.buckets[index].value.is_none() {
            this.items += 1;
            this.buckets[index].key = key;
            this.buckets[index].probe = probe;
            this.buckets[index].value = Some(value);
            return;
        }

        if this.buckets[index].key == key {
            // C: `if (owner && buckets[index].value != value) free(...)`
            // then overwrite. The incoming `value` is a distinct owned
            // box, so replacing the `Some` drops the previous box (the
            // owner free); the `!= value` self-overwrite guard is vacuous
            // in the `Box` model — you cannot hold the slot's own box to
            // pass it back in.
            this.buckets[index].value = Some(value);
            return;
        }

        /* Robin Hood swap */
        if probe > this.buckets[index].probe {
            // C: `HashtableItem tmp = buckets[index];` then write the
            // carried item into the slot and continue with `tmp`.
            let tmp_key = this.buckets[index].key;
            let tmp_probe = this.buckets[index].probe;
            let tmp_value = this.buckets[index].value.take().unwrap();

            this.buckets[index].key = key;
            this.buckets[index].probe = probe;
            this.buckets[index].value = Some(value);

            key = tmp_key;
            probe = tmp_probe;
            value = tmp_value;
        }

        index = (index + 1) % this.size;
        probe += 1;
    }
}

/// Port of `void Hashtable_setSize(Hashtable* this, size_t size)` from
/// `Hashtable.c:195`. Grows (or, from `Hashtable_remove`, shrinks) the
/// bucket array to the next prime `>= size` and rehashes every live entry
/// into it via `insert`. No-ops when `size <= items` (would not fit) or
/// when the prime target equals the current `size`. The old buckets are
/// swapped out (C `oldBuckets = this->buckets`), the new empty array
/// installed and `items` reset, then each old filled bucket is
/// re-inserted; the old `Vec` frees at scope end (C `free(oldBuckets)`).
pub fn Hashtable_setSize(this: &mut Hashtable, size: usize) {
    if size <= this.items {
        return;
    }

    let newSize = nextPrime(size);
    if newSize == this.size {
        return;
    }

    let newBuckets = (0..newSize)
        .map(|_| HashtableItem {
            key: 0,
            probe: 0,
            value: None,
        })
        .collect();

    let oldBuckets = std::mem::replace(&mut this.buckets, newBuckets);
    this.size = newSize;
    this.items = 0;

    /* rehash */
    for mut bucket in oldBuckets {
        if let Some(value) = bucket.value.take() {
            insert(this, bucket.key, value);
        }
    }
}

/// Port of `void Hashtable_put(Hashtable* this, ht_key_t key, void* value)`
/// from `Hashtable.c:226`. Grows the table before inserting whenever the
/// load factor would exceed 0.7 (the C `10 * items > 7 * size`),
/// doubling `size` — with a `usize::MAX / 2 < size` overflow guard
/// (C `SIZE_MAX / 2 < size` → `CRT_fatalError`, here `panic!`) — then
/// delegates to `insert`. The trailing consistency / `size > items`
/// asserts are omitted (debug-only).
pub fn Hashtable_put(this: &mut Hashtable, key: u32, value: Box<dyn Object>) {
    /* grow on load-factor > 0.7 */
    if 10 * this.items > 7 * this.size {
        if usize::MAX / 2 < this.size {
            panic!("Hashtable: size overflow");
        }

        Hashtable_setSize(this, 2 * this.size);
    }

    insert(this, key, value);
}

/// Port of `void* Hashtable_remove(Hashtable* this, ht_key_t key)` from
/// `Hashtable.c:247`. Probes for `key`; on a hit it removes the entry and
/// backward-shifts the following run of displaced items (each with
/// `probe > 0`) back one slot, decrementing their probe distances, then
/// empties the tail slot and decrements `items`. Returns the removed
/// value when `!owner` (C `res = value`) and `None` when `owner` (C
/// `free(value)`, here the box is dropped). Finally shrinks the table
/// when the load factor drops below 0.125 (C `8 * items < size` →
/// `Hashtable_setSize(this, size / 3)`). The `origIndex` asserts are
/// omitted (debug-only).
pub fn Hashtable_remove(this: &mut Hashtable, key: u32) -> Option<Box<dyn Object>> {
    let mut index = (key as usize) % this.size;
    let mut probe: usize = 0;

    let mut res: Option<Box<dyn Object>> = None;

    while this.buckets[index].value.is_some() {
        if this.buckets[index].key == key {
            let removed = this.buckets[index].value.take();
            if this.owner {
                // C `free(buckets[index].value)`: drop the box.
                drop(removed);
            } else {
                res = removed;
            }

            let mut next = (index + 1) % this.size;

            while this.buckets[next].value.is_some() && this.buckets[next].probe > 0 {
                // C `buckets[index] = buckets[next]; buckets[index].probe -= 1;`
                let key_n = this.buckets[next].key;
                let probe_n = this.buckets[next].probe;
                let value_n = this.buckets[next].value.take();
                this.buckets[index].key = key_n;
                this.buckets[index].probe = probe_n - 1;
                this.buckets[index].value = value_n;

                index = next;
                next = (index + 1) % this.size;
            }

            /* set empty after backward shifting */
            this.buckets[index].value = None;
            this.items -= 1;

            break;
        }

        if this.buckets[index].probe < probe {
            break;
        }

        index = (index + 1) % this.size;
        probe += 1;
    }

    /* shrink on load-factor < 0.125 */
    if 8 * this.items < this.size {
        Hashtable_setSize(this, this.size / 3); /* account for nextPrime rounding up */
    }

    res
}

/// Port of `void* Hashtable_get(Hashtable* this, ht_key_t key)` from
/// `Hashtable.c:302`. Probes forward from `key % size` returning the
/// matching value, or `None` (C `NULL`) once an empty slot is reached or
/// the resident probe distance falls below the search distance
/// (`buckets[index].probe < probe`, the Robin-Hood early-out). The C
/// increment is the literal `(index + 1) != size ? (index + 1) : 0`
/// (kept verbatim, distinct from the `% size` the other functions use).
/// The returned reference borrows the table, mirroring C's non-owning
/// read of `void* value`.
pub fn Hashtable_get(this: &Hashtable, key: u32) -> Option<&dyn Object> {
    let mut index = (key as usize) % this.size;
    let mut probe: usize = 0;
    let mut res: Option<&dyn Object> = None;

    while this.buckets[index].value.is_some() {
        if this.buckets[index].key == key {
            res = this.buckets[index].value.as_deref();
            break;
        }

        if this.buckets[index].probe < probe {
            break;
        }

        index = if index + 1 != this.size { index + 1 } else { 0 };
        probe += 1;
    }

    res
}

/// Port of `void Hashtable_foreach(Hashtable* this, Hashtable_PairFunction
/// f, void* userData)` from `Hashtable.c:330`. Walks every bucket in
/// storage order and calls `f(key, value)` for each filled one. The C
/// `Hashtable_PairFunction (ht_key_t, void*, void*)` callback plus its
/// `userData` argument are modeled as a single `&mut dyn FnMut(u32,
/// &dyn Object)` closure — any user data the C threads through `userData`
/// is captured by the closure instead, the faithful safe-Rust analog of
/// the callback+context pair.
pub fn Hashtable_foreach(this: &Hashtable, f: &mut dyn FnMut(u32, &dyn Object)) {
    for walk in this.buckets.iter() {
        if let Some(value) = walk.value.as_deref() {
            f(walk.key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::object::{Object, ObjectClass};

    #[test]
    fn next_prime_matches_table_entries() {
        // 0 rounds up to the first entry
        assert_eq!(nextPrime(0), 7);
        // exact hit: `<=` is inclusive
        assert_eq!(nextPrime(7), 7);
        // 8 rounds up past 7 to the next entry
        assert_eq!(nextPrime(8), 13);
        assert_eq!(nextPrime(13), 13);
        // 14 rounds up to 31 (skipping nothing between 13 and 31)
        assert_eq!(nextPrime(14), 31);
        // mid-table value, exact hit
        assert_eq!(nextPrime(1048573), 1048573);
        // one past a mid-table entry rounds to the next
        assert_eq!(nextPrime(1048574), 2097143);
        // the exact largest entry returns itself
        assert_eq!(nextPrime(137438953447), 137438953447);
    }

    #[test]
    fn next_prime_is_identity_on_every_table_entry() {
        for &prime in &OEISprimes {
            assert_eq!(nextPrime(prime), prime, "prime={prime}");
        }
    }

    #[test]
    #[should_panic(expected = "Hashtable: no prime found")]
    fn next_prime_panics_when_no_prime_large_enough() {
        // usize::MAX exceeds the largest table entry, so the C
        // fall-through (CRT_fatalError) fires — ported as panic.
        nextPrime(usize::MAX);
    }

    // A tiny concrete Object for the container tests — mirrors the `Num`
    // test type at the bottom of `object.rs`. It carries a payload `u32`
    // so a retrieved value can be checked against the key it was put
    // under.
    static VAL_class: ObjectClass = ObjectClass { extends: None };

    struct Val {
        n: u32,
    }

    impl Object for Val {
        fn klass(&self) -> &'static ObjectClass {
            &VAL_class
        }
    }

    // Downcast a `&dyn Object` back to the concrete `Val` (the safe-Rust
    // analog of C casting the `void*` payload back to its real type) and
    // read its payload.
    fn val_of(o: &dyn Object) -> u32 {
        let any: &dyn core::any::Any = o;
        any.downcast_ref::<Val>().expect("value is a Val").n
    }

    #[test]
    fn new_rounds_size_to_prime_and_defaults_to_13() {
        // size 0 → the C `: 13` default branch
        let ht = Hashtable_new(0, true);
        assert_eq!(ht.size, 13);
        assert_eq!(ht.items, 0);
        assert_eq!(Hashtable_count(&ht), 0);

        // non-zero size rounds up through nextPrime: 10 → 13, 20 → 31
        assert_eq!(Hashtable_new(10, false).size, 13);
        assert_eq!(Hashtable_new(20, false).size, 31);
        assert_eq!(Hashtable_new(31, false).size, 31);
    }

    #[test]
    fn put_get_roundtrip() {
        let mut ht = Hashtable_new(0, true);
        Hashtable_put(&mut ht, 1, Box::new(Val { n: 1 }));
        Hashtable_put(&mut ht, 2, Box::new(Val { n: 2 }));
        Hashtable_put(&mut ht, 100, Box::new(Val { n: 100 }));

        assert_eq!(val_of(Hashtable_get(&ht, 1).unwrap()), 1);
        assert_eq!(val_of(Hashtable_get(&ht, 2).unwrap()), 2);
        assert_eq!(val_of(Hashtable_get(&ht, 100).unwrap()), 100);
        // absent key
        assert!(Hashtable_get(&ht, 999).is_none());

        assert_eq!(ht.items, 3);
        assert_eq!(Hashtable_count(&ht), 3);
    }

    #[test]
    fn put_same_key_overwrites_without_growing_items() {
        let mut ht = Hashtable_new(0, true);
        Hashtable_put(&mut ht, 7, Box::new(Val { n: 70 }));
        assert_eq!(val_of(Hashtable_get(&ht, 7).unwrap()), 70);
        assert_eq!(ht.items, 1);

        // overwrite: same key, new value — items stays 1 (C insert's
        // key-match branch replaces the value in place)
        Hashtable_put(&mut ht, 7, Box::new(Val { n: 71 }));
        assert_eq!(val_of(Hashtable_get(&ht, 7).unwrap()), 71);
        assert_eq!(ht.items, 1);
        assert_eq!(Hashtable_count(&ht), 1);
    }

    #[test]
    fn remove_then_get_returns_none() {
        // owner=false so remove hands the value back for inspection
        let mut ht = Hashtable_new(0, false);
        Hashtable_put(&mut ht, 3, Box::new(Val { n: 33 }));
        Hashtable_put(&mut ht, 4, Box::new(Val { n: 44 }));

        let removed = Hashtable_remove(&mut ht, 3);
        assert_eq!(val_of(removed.as_deref().unwrap()), 33);
        assert!(Hashtable_get(&ht, 3).is_none());
        // the untouched key still resolves
        assert_eq!(val_of(Hashtable_get(&ht, 4).unwrap()), 44);
        assert_eq!(ht.items, 1);
        assert_eq!(Hashtable_count(&ht), 1);

        // removing an absent key returns None and changes nothing
        assert!(Hashtable_remove(&mut ht, 999).is_none());
        assert_eq!(ht.items, 1);

        // owner=true remove drops the value and returns None
        let mut owned = Hashtable_new(0, true);
        Hashtable_put(&mut owned, 5, Box::new(Val { n: 55 }));
        assert!(Hashtable_remove(&mut owned, 5).is_none());
        assert!(Hashtable_get(&owned, 5).is_none());
        assert_eq!(owned.items, 0);
    }

    #[test]
    fn colliding_keys_probe_and_all_resolve() {
        // size 13: keys 5, 18, 31, 44 all hash to slot 5 (mod 13),
        // forcing linear probing / Robin-Hood placement into later slots.
        let mut ht = Hashtable_new(0, true);
        assert_eq!(ht.size, 13);
        let keys = [5u32, 18, 31, 44];
        for &k in &keys {
            assert_eq!((k as usize) % ht.size, 5, "key {k} must collide at 5");
            Hashtable_put(&mut ht, k, Box::new(Val { n: k }));
        }

        // every colliding key still resolves to its own value
        for &k in &keys {
            assert_eq!(val_of(Hashtable_get(&ht, k).unwrap()), k);
        }
        assert_eq!(ht.items, 4);
        assert_eq!(Hashtable_count(&ht), 4);

        // remove a middle colliding key, triggering the backward-shift of
        // the probe run; the rest must still resolve
        let mut ht2 = Hashtable_new(0, false);
        for &k in &keys {
            Hashtable_put(&mut ht2, k, Box::new(Val { n: k }));
        }
        let removed = Hashtable_remove(&mut ht2, 18);
        assert_eq!(val_of(removed.as_deref().unwrap()), 18);
        assert!(Hashtable_get(&ht2, 18).is_none());
        for &k in &[5u32, 31, 44] {
            assert_eq!(
                val_of(Hashtable_get(&ht2, k).unwrap()),
                k,
                "key {k} after shift"
            );
        }
        assert_eq!(Hashtable_count(&ht2), 3);
    }

    #[test]
    fn foreach_visits_every_filled_bucket() {
        let mut ht = Hashtable_new(0, true);
        let keys = [1u32, 14, 27, 100, 250];
        for &k in &keys {
            Hashtable_put(&mut ht, k, Box::new(Val { n: k }));
        }

        let mut seen: Vec<(u32, u32)> = Vec::new();
        Hashtable_foreach(&ht, &mut |k, v| seen.push((k, val_of(v))));

        // every entry visited exactly once, key paired with its payload;
        // storage order is arbitrary so compare as sorted sets
        assert_eq!(seen.len(), keys.len());
        seen.sort_unstable();
        let mut expected: Vec<(u32, u32)> = keys.iter().map(|&k| (k, k)).collect();
        expected.sort_unstable();
        assert_eq!(seen, expected);
    }

    #[test]
    fn many_puts_trigger_resize_and_preserve_all_entries() {
        // Start at the default 13 buckets; insert enough that the
        // load-factor grow (10*items > 7*size) fires at least once and
        // rehashes. 13 → grows to nextPrime(26)=31 → grows to
        // nextPrime(62)=127 as items climb.
        let mut ht = Hashtable_new(0, true);
        assert_eq!(ht.size, 13);

        let count: u32 = 50;
        for k in 0..count {
            Hashtable_put(&mut ht, k, Box::new(Val { n: k }));
        }

        // the table must have grown past its initial 13
        assert!(ht.size > 13, "expected a resize, size still {}", ht.size);
        assert_eq!(ht.items, count as usize);
        assert_eq!(Hashtable_count(&ht), count as usize);

        // every key survived the rehash(es)
        for k in 0..count {
            assert_eq!(
                val_of(Hashtable_get(&ht, k).unwrap()),
                k,
                "key {k} lost in resize"
            );
        }
        // load factor stays below 1: there is always an empty slot
        assert!(ht.size > ht.items);
    }

    #[test]
    fn clear_empties_the_table() {
        let mut ht = Hashtable_new(0, true);
        for k in 0..10u32 {
            Hashtable_put(&mut ht, k, Box::new(Val { n: k }));
        }
        assert_eq!(ht.items, 10);

        Hashtable_clear(&mut ht);
        assert_eq!(ht.items, 0);
        assert_eq!(Hashtable_count(&ht), 0);
        for k in 0..10u32 {
            assert!(Hashtable_get(&ht, k).is_none());
        }
    }

    #[test]
    fn remove_shrinks_on_low_load_factor() {
        // Grow the table, then remove almost everything so the shrink
        // branch (8*items < size) fires.
        let mut ht = Hashtable_new(0, false);
        for k in 0..40u32 {
            Hashtable_put(&mut ht, k, Box::new(Val { n: k }));
        }
        let grown = ht.size;
        assert!(grown > 13);

        for k in 0..38u32 {
            Hashtable_remove(&mut ht, k);
        }
        // now items is tiny relative to the grown size → shrunk
        assert!(
            ht.size < grown,
            "expected shrink from {grown}, still {}",
            ht.size
        );
        assert_eq!(ht.items, 2);
        assert_eq!(Hashtable_count(&ht), 2);
        // survivors still resolve
        assert_eq!(val_of(Hashtable_get(&ht, 38).unwrap()), 38);
        assert_eq!(val_of(Hashtable_get(&ht, 39).unwrap()), 39);
    }
}
