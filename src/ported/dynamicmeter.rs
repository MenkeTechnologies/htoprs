//! Port of `DynamicMeter.c` — only the pure `Hashtable_foreach` search
//! callback (`DynamicMeter_compare`) has a body reproducible without
//! unported substrate.
//!
//! C names are preserved verbatim (`CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! Everything else in `DynamicMeter.c` bottoms out in substrate not yet
//! ported and so stays as an exact `todo!()` stub:
//!   - `DynamicMeters_new` / `DynamicMeters_delete` / `DynamicMeter_init`
//!     / `DynamicMeter_updateValues` / `DynamicMeter_display` — call
//!     `Platform_*` (and `Hashtable_delete`).
//!   - `DynamicMeter_search` — drives `Hashtable_foreach`.
//!   - `DynamicMeter_lookup` / `DynamicMeter_getCaption`
//!     / `DynamicMeter_getUiName` — read via `Hashtable_get` off a
//!     `Meter`/`Settings`/`Machine` graph not modelled here.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// Subset of htop's `DynamicMeter` struct (`DynamicMeter.h:17`).
///
/// Only `name` is modelled: it is the single field `DynamicMeter_compare`
/// touches. The C struct also carries `caption`, `description`, `type`,
/// and `maximum`, which this callback never reads and so are omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicMeter {
    /// C: `char name[32]` — unique name, cannot contain spaces.
    pub name: String,
}

/// Port of the file-local `DynamicIterator` struct (`DynamicMeter.c:50`).
///
/// `key` is `ht_key_t`, i.e. `unsigned int` (`Hashtable.h:14`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicIterator {
    pub name: String,
    pub key: u32,
    pub found: bool,
}

/// Port of `DynamicMeter.c:56`.
///
/// `Hashtable_foreach` callback: `value` is the stored `DynamicMeter*`,
/// `data` is the `DynamicIterator*`. `String_eq(a, b)` is
/// `strcmp(a, b) == 0` (`XUtils.h:60`), i.e. exact string equality here.
/// Faithful to the C: on a match it sets `found` and records `key`, with
/// no early-out — a later matching key overwrites an earlier one.
pub fn DynamicMeter_compare(key: u32, meter: &DynamicMeter, iter: &mut DynamicIterator) {
    if iter.name == meter.name {
        iter.found = true;
        iter.key = key;
    }
}

/// TODO: port of `Hashtable* DynamicMeters_new(void` from `DynamicMeter.c:39`.
pub fn DynamicMeters_new() {
    todo!("port of DynamicMeter.c:39")
}

/// TODO: port of `void DynamicMeters_delete(Hashtable* dynamics` from `DynamicMeter.c:43`.
pub fn DynamicMeters_delete() {
    todo!("port of DynamicMeter.c:43")
}

/// TODO: port of `bool DynamicMeter_search(Hashtable* dynamics, const char* name, ht_key_t* key` from `DynamicMeter.c:65`.
pub fn DynamicMeter_search() {
    todo!("port of DynamicMeter.c:65")
}

/// TODO: port of `const char* DynamicMeter_lookup(Hashtable* dynamics, ht_key_t key` from `DynamicMeter.c:74`.
pub fn DynamicMeter_lookup() {
    todo!("port of DynamicMeter.c:74")
}

/// TODO: port of `static void DynamicMeter_init(Meter* meter` from `DynamicMeter.c:79`.
pub fn DynamicMeter_init() {
    todo!("port of DynamicMeter.c:79")
}

/// TODO: port of `static void DynamicMeter_updateValues(Meter* meter` from `DynamicMeter.c:83`.
pub fn DynamicMeter_updateValues() {
    todo!("port of DynamicMeter.c:83")
}

/// TODO: port of `static void DynamicMeter_display(const Object* cast, RichString* out` from `DynamicMeter.c:87`.
pub fn DynamicMeter_display() {
    todo!("port of DynamicMeter.c:87")
}

/// TODO: port of `static const char* DynamicMeter_getCaption(const Meter* this` from `DynamicMeter.c:92`.
pub fn DynamicMeter_getCaption() {
    todo!("port of DynamicMeter.c:92")
}

/// TODO: port of `static void DynamicMeter_getUiName(const Meter* this, char* name, size_t length` from `DynamicMeter.c:100`.
pub fn DynamicMeter_getUiName() {
    todo!("port of DynamicMeter.c:100")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(name: &str) -> DynamicMeter {
        DynamicMeter {
            name: name.to_string(),
        }
    }

    fn iter(name: &str) -> DynamicIterator {
        DynamicIterator {
            name: name.to_string(),
            key: 0,
            found: false,
        }
    }

    #[test]
    fn match_sets_found_and_key() {
        let mut it = iter("cpu");
        DynamicMeter_compare(7, &meter("cpu"), &mut it);
        assert!(it.found);
        assert_eq!(it.key, 7);
    }

    #[test]
    fn no_match_leaves_iterator_untouched() {
        let mut it = iter("cpu");
        DynamicMeter_compare(7, &meter("mem"), &mut it);
        assert!(!it.found);
        assert_eq!(it.key, 0);
    }

    #[test]
    fn string_eq_is_exact_case_sensitive() {
        // String_eq == strcmp==0: case-sensitive, no trimming.
        let mut it = iter("CPU");
        DynamicMeter_compare(3, &meter("cpu"), &mut it);
        assert!(!it.found);
        assert_eq!(it.key, 0);
    }

    #[test]
    fn later_match_overwrites_key_no_early_out() {
        // C has no early return: iterating multiple matching entries
        // leaves `key` at the last one visited.
        let mut it = iter("cpu");
        DynamicMeter_compare(1, &meter("cpu"), &mut it);
        assert_eq!(it.key, 1);
        DynamicMeter_compare(2, &meter("cpu"), &mut it);
        assert!(it.found);
        assert_eq!(it.key, 2);
    }

    #[test]
    fn empty_names_compare_equal() {
        let mut it = iter("");
        DynamicMeter_compare(5, &meter(""), &mut it);
        assert!(it.found);
        assert_eq!(it.key, 5);
    }
}
