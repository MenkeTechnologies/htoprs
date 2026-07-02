//! Port of `Hashtable.c` — only the pure prime-table math is ported.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port.
//!
//! The hashtable proper — `Hashtable_new`, `Hashtable_delete`,
//! `Hashtable_clear`, `insert`, `Hashtable_setSize`, `Hashtable_put`,
//! `Hashtable_get`, `Hashtable_remove`, `Hashtable_foreach`,
//! `Hashtable_count`, `Hashtable_dump`, and `Hashtable_isConsistent` —
//! is intentionally not ported. Those implement an open-addressing
//! table over a heap-allocated `HashtableItem*` buckets array with
//! owner semantics (probing, allocation, and item lifetimes managed by
//! hand). Like the XUtils allocation wrappers, they have no faithful
//! safe-Rust analog — Rust's `std::collections::HashMap` owns
//! allocation, probing, and lifetimes itself, so a line-for-line port
//! would be reimplementing `unsafe` pointer machinery rather than
//! translating it. `Hashtable_count` is a trivial `return this->items`
//! accessor that requires the struct to exist. Only `nextPrime` and
//! its `OEISprimes` table are pure, struct-free math and are ported
//! here.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // preserve the C table name `OEISprimes` verbatim

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
/// Returns the smallest entry in [`OEISprimes`] that is `>= n` (the C
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
