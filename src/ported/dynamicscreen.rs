//! Partial port of `DynamicScreen.c` — htop's dynamic-screen registry.
//!
//! C names are preserved verbatim (`non_snake_case` allowed). Only the
//! `Hashtable_foreach` comparison callback `DynamicScreen_compare` has a
//! faithful safe-Rust body — its logic is a plain name match plus an
//! iterator update, needing no unported substrate. This mirrors the
//! sibling `DynamicColumn.c` port, whose registry is structurally the
//! same.
//!
//! Ported (self-contained, no unported substrate):
//! - `DynamicScreen_compare` (`DynamicScreen.c:47`) — `static` in C; the
//!   `Hashtable_foreach` visitor that flags a match when the visited
//!   screen's `name` equals the iterator's search `name` (C `String_eq`,
//!   i.e. exact `strcmp == 0`) and records the key.
//!
//! Stubbed (cannot be ported faithfully yet):
//! - `DynamicScreens_new` (`DynamicScreen.c:22`) — returns
//!   `Platform_dynamicScreens()`; depends on the unported `Platform`
//!   layer.
//! - `DynamicScreens_delete` (`DynamicScreen.c:26`) — calls
//!   `Platform_dynamicScreensDone` then `Hashtable_delete`; depends on
//!   unported `Platform` and the `Hashtable` heap wrapper (only
//!   `nextPrime` is ported in `hashtable.rs`).
//! - `DynamicScreen_done` (`DynamicScreen.c:33`) — `free()`s the struct's
//!   `caption`/`fields`/`heading`/`sortKey`/`columnKeys` heap strings.
//!   Rust owns those allocations (`Drop` frees them), so a hand-written
//!   free-everything routine has no faithful analog.
//! - `DynamicScreen_search` (`DynamicScreen.c:56`) — drives
//!   `Hashtable_foreach` with `DynamicScreen_compare`; needs the unported
//!   `Hashtable` dispatch.
//! - `DynamicScreen_lookup` (`DynamicScreen.c:65`) — `Hashtable_get`
//!   lookup; needs the unported `Hashtable` dispatch.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// Model of the C `DynamicScreen` struct (`DynamicScreen.h`). Only the
/// `name` field is needed by [`DynamicScreen_compare`]; the C struct's
/// other fields (`heading`, `caption`, `fields`, `sortKey`,
/// `columnKeys`, `direction`) are omitted because that callback never
/// reads them.
pub struct DynamicScreen {
    /// C `char name[32]` — unique name, cannot contain spaces.
    pub name: String,
}

/// Model of the file-private C `DynamicIterator` struct
/// (`DynamicScreen.c:41`). Unlike `DynamicColumn.c`'s iterator, this one
/// carries no matched-value pointer — only the needle `name`, the
/// matched `key`, and a `found` flag.
pub struct DynamicIterator<'a> {
    /// Needle name being searched for.
    pub name: &'a str,
    /// Key of the matched screen (C `ht_key_t`, i.e. `unsigned int`;
    /// `0` when unmatched).
    pub key: u32,
    /// Whether a match was found.
    pub found: bool,
}

/// Port of `DynamicScreen.c:47`. `static` in C. `Hashtable_foreach`
/// callback: when the visited screen's name equals the iterator's search
/// name (C `String_eq`, i.e. exact `strcmp == 0`), set `found` and record
/// the key into the iterator. `ht_key_t` is C `unsigned int`.
pub fn DynamicScreen_compare(key: u32, value: &DynamicScreen, iter: &mut DynamicIterator) {
    if iter.name == value.name {
        iter.found = true;
        iter.key = key;
    }
}

/// TODO: port of `Hashtable* DynamicScreens_new(void` from `DynamicScreen.c:22`.
pub fn DynamicScreens_new() {
    todo!("port of DynamicScreen.c:22")
}

/// TODO: port of `void DynamicScreens_delete(Hashtable* screens` from `DynamicScreen.c:26`.
pub fn DynamicScreens_delete() {
    todo!("port of DynamicScreen.c:26")
}

/// TODO: port of `void DynamicScreen_done(DynamicScreen* this` from `DynamicScreen.c:33`.
pub fn DynamicScreen_done() {
    todo!("port of DynamicScreen.c:33")
}

/// TODO: port of `bool DynamicScreen_search(Hashtable* screens, const char* name, ht_key_t* key` from `DynamicScreen.c:56`.
pub fn DynamicScreen_search() {
    todo!("port of DynamicScreen.c:56")
}

/// TODO: port of `const char* DynamicScreen_lookup(Hashtable* screens, ht_key_t key` from `DynamicScreen.c:65`.
pub fn DynamicScreen_lookup() {
    todo!("port of DynamicScreen.c:65")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(name: &str) -> DynamicScreen {
        DynamicScreen {
            name: name.to_string(),
        }
    }

    #[test]
    fn compare_records_match_and_key() {
        let cpu = screen("cpu");
        let mut iter = DynamicIterator {
            name: "cpu",
            key: 0,
            found: false,
        };
        DynamicScreen_compare(7, &cpu, &mut iter);
        assert!(iter.found);
        assert_eq!(iter.key, 7);
    }

    #[test]
    fn compare_ignores_non_match() {
        let mem = screen("mem");
        let mut iter = DynamicIterator {
            name: "cpu",
            key: 0,
            found: false,
        };
        DynamicScreen_compare(3, &mem, &mut iter);
        // no match: iterator left untouched (C leaves .found=false, .key=0)
        assert!(!iter.found);
        assert_eq!(iter.key, 0);
    }

    #[test]
    fn compare_is_exact_case_sensitive_strcmp() {
        // String_eq is strcmp==0: case-sensitive, no trimming
        let upper = screen("CPU");
        let mut iter = DynamicIterator {
            name: "cpu",
            key: 0,
            found: false,
        };
        DynamicScreen_compare(9, &upper, &mut iter);
        assert!(!iter.found);
        assert_eq!(iter.key, 0);
    }

    #[test]
    fn compare_last_match_wins() {
        // C callback overwrites on every match; a later duplicate wins
        let a = screen("dup");
        let b = screen("dup");
        let mut iter = DynamicIterator {
            name: "dup",
            key: 0,
            found: false,
        };
        DynamicScreen_compare(1, &a, &mut iter);
        DynamicScreen_compare(2, &b, &mut iter);
        assert!(iter.found);
        assert_eq!(iter.key, 2);
    }
}
