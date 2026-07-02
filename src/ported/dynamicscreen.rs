//! Partial port of `DynamicScreen.c` — htop's dynamic-screen registry.
//!
//! C names are preserved verbatim (`non_snake_case` allowed). The two
//! lookup paths over the registry — `DynamicScreen_search`
//! (`Hashtable_foreach`) and `DynamicScreen_lookup` (`Hashtable_get`) —
//! are now portable because the `Hashtable` container is ported in
//! `hashtable.rs`. This mirrors the sibling `DynamicColumn.c` registry,
//! whose structure is the same.
//!
//! The ported `Hashtable` models the C `void* value` as
//! `Box<dyn Object>`, so any value stored in the screens table must
//! implement [`Object`]. htop's C `DynamicScreen` is a plain struct, not
//! an `Object` subclass — the [`Object`] impl below (and its private
//! `DynamicScreen_class` descriptor) is purely the adapter that lets a
//! `DynamicScreen` live in the ported `Hashtable`; it is not a C symbol.
//! The C `(const DynamicScreen*)value` cast in `DynamicScreen_compare`
//! and the `Hashtable_get` return are rendered as an `Any` downcast, the
//! same technique `hashtable.rs`'s tests use to recover a concrete value
//! from a `&dyn Object`.
//!
//! Ported:
//! - `DynamicScreen_compare` (`DynamicScreen.c:47`) — `static` in C; the
//!   `Hashtable_foreach` visitor that flags a match when the visited
//!   screen's `name` equals the iterator's search `name` (C `String_eq`,
//!   i.e. exact `strcmp == 0`) and records the key.
//! - `DynamicScreen_search` (`DynamicScreen.c:56`) — drives
//!   `Hashtable_foreach` with `DynamicScreen_compare` over the registry,
//!   writing the matched key through the out-param and returning whether
//!   a match was found.
//! - `DynamicScreen_lookup` (`DynamicScreen.c:65`) — `Hashtable_get`
//!   lookup returning the matched screen's `name` (C `const char*`), or
//!   `None` (C `NULL`) when absent.
//!
//! Stubbed (cannot be ported faithfully yet):
//! - `DynamicScreens_new` (`DynamicScreen.c:22`) — returns
//!   `Platform_dynamicScreens()`; depends on the unported `Platform`
//!   layer.
//! - `DynamicScreens_delete` (`DynamicScreen.c:26`) — calls
//!   `Platform_dynamicScreensDone` then `Hashtable_delete`; depends on
//!   the unported `Platform` layer (`Hashtable_delete` is itself a
//!   `Drop`-based stub in `hashtable.rs`).
//! - `DynamicScreen_done` (`DynamicScreen.c:33`) — `free()`s the struct's
//!   `caption`/`fields`/`heading`/`sortKey`/`columnKeys` heap strings.
//!   Rust owns those allocations (`Drop` frees them), so a hand-written
//!   free-everything routine has no faithful analog (same precedent as
//!   `History_delete`).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::hashtable::{Hashtable, Hashtable_foreach, Hashtable_get};
use crate::ported::object::{Object, ObjectClass, Object_class};

/// Model of the C `DynamicScreen` struct (`DynamicScreen.h`). Only the
/// `name` field is read by [`DynamicScreen_compare`] /
/// [`DynamicScreen_lookup`]; the C struct's other fields (`heading`,
/// `caption`, `fields`, `sortKey`, `columnKeys`, `direction`) are omitted
/// because the ported code paths never read them.
pub struct DynamicScreen {
    /// C `char name[32]` — unique name, cannot contain spaces.
    pub name: String,
}

/// Adapter class descriptor that lets a [`DynamicScreen`] be stored in
/// the ported [`Hashtable`] (whose `void* value` is `Box<dyn Object>`).
/// This is NOT a C symbol — htop's `DynamicScreen` is a plain struct, not
/// an `Object` subclass. It roots directly at `Object_class` and sets no
/// vtable slots (`DynamicScreen` is never displayed or compared through
/// the `Object` machinery — only downcast back to its concrete type).
static DynamicScreen_class: ObjectClass = ObjectClass {
    extends: Some(&Object_class),
};

impl Object for DynamicScreen {
    fn klass(&self) -> &'static ObjectClass {
        &DynamicScreen_class
    }
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

/// Port of `bool DynamicScreen_search(Hashtable* screens, const char*
/// name, ht_key_t* key)` from `DynamicScreen.c:56`. Seeds a
/// [`DynamicIterator`] with the needle `name`, walks the registry with
/// `Hashtable_foreach` + [`DynamicScreen_compare`] (only when `screens`
/// is present — the C `if (screens)` guard, modeled as `Option`), writes
/// the matched key through the `key` out-param when present (C
/// `if (key) *key = iter.key`, modeled as `Option<&mut u32>`), and
/// returns whether a match was found. Each visited `&dyn Object` is
/// downcast back to `&DynamicScreen` — the safe-Rust analog of the C
/// `(const DynamicScreen*)value` cast in the callback.
pub fn DynamicScreen_search(
    screens: Option<&Hashtable>,
    name: &str,
    key: Option<&mut u32>,
) -> bool {
    let mut iter = DynamicIterator {
        key: 0,
        name,
        found: false,
    };
    if let Some(screens) = screens {
        Hashtable_foreach(screens, &mut |k, value| {
            let any: &dyn core::any::Any = value;
            let screen = any
                .downcast_ref::<DynamicScreen>()
                .expect("value is a DynamicScreen");
            DynamicScreen_compare(k, screen, &mut iter);
        });
    }
    if let Some(key) = key {
        *key = iter.key;
    }
    iter.found
}

/// Port of `const char* DynamicScreen_lookup(Hashtable* screens, ht_key_t
/// key)` from `DynamicScreen.c:65`. Fetches the value stored under `key`
/// with `Hashtable_get` and returns its `name` (C `const char*`), or
/// `None` (C `NULL`) when no entry exists. The retrieved `&dyn Object` is
/// downcast back to `&DynamicScreen` — the safe-Rust analog of the C
/// `Hashtable_get` return being assigned to a `const DynamicScreen*`.
pub fn DynamicScreen_lookup(screens: &Hashtable, key: u32) -> Option<&str> {
    Hashtable_get(screens, key).map(|value| {
        let any: &dyn core::any::Any = value;
        any.downcast_ref::<DynamicScreen>()
            .expect("value is a DynamicScreen")
            .name
            .as_str()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::hashtable::{Hashtable_new, Hashtable_put};

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

    // Build a registry of named screens keyed exactly as put.
    fn registry(entries: &[(u32, &str)]) -> Hashtable {
        let mut ht = Hashtable_new(0, true);
        for &(k, name) in entries {
            Hashtable_put(&mut ht, k, Box::new(screen(name)));
        }
        ht
    }

    #[test]
    fn search_finds_entry_and_writes_key() {
        let ht = registry(&[(11, "cpu"), (22, "mem"), (33, "disk")]);

        let mut key = 0u32;
        let found = DynamicScreen_search(Some(&ht), "mem", Some(&mut key));
        assert!(found);
        assert_eq!(key, 22);
    }

    #[test]
    fn search_absent_name_returns_false_and_zero_key() {
        let ht = registry(&[(11, "cpu"), (22, "mem")]);

        // C seeds iter.key = 0; on no match *key is written 0.
        let mut key = 999u32;
        let found = DynamicScreen_search(Some(&ht), "gpu", Some(&mut key));
        assert!(!found);
        assert_eq!(key, 0);
    }

    #[test]
    fn search_null_key_out_param_is_ignored() {
        // C `if (key)` guard: a NULL key out-param is skipped, but the
        // boolean result still reflects the match.
        let ht = registry(&[(5, "cpu")]);
        assert!(DynamicScreen_search(Some(&ht), "cpu", None));
        assert!(!DynamicScreen_search(Some(&ht), "nope", None));
    }

    #[test]
    fn search_none_registry_returns_false() {
        // C `if (screens)` guard: a NULL table never runs the foreach, so
        // the result is false and *key stays the seeded 0.
        let mut key = 42u32;
        let found = DynamicScreen_search(None, "cpu", Some(&mut key));
        assert!(!found);
        assert_eq!(key, 0);
    }

    #[test]
    fn search_is_case_sensitive() {
        let ht = registry(&[(7, "CPU")]);
        assert!(!DynamicScreen_search(Some(&ht), "cpu", None));
        assert!(DynamicScreen_search(Some(&ht), "CPU", None));
    }

    #[test]
    fn lookup_returns_name_for_key() {
        let ht = registry(&[(11, "cpu"), (22, "mem"), (33, "disk")]);
        assert_eq!(DynamicScreen_lookup(&ht, 11), Some("cpu"));
        assert_eq!(DynamicScreen_lookup(&ht, 22), Some("mem"));
        assert_eq!(DynamicScreen_lookup(&ht, 33), Some("disk"));
    }

    #[test]
    fn lookup_absent_key_returns_none() {
        let ht = registry(&[(11, "cpu")]);
        // C `Hashtable_get` returns NULL → the ternary yields NULL.
        assert_eq!(DynamicScreen_lookup(&ht, 999), None);
    }

    #[test]
    fn search_then_lookup_roundtrip() {
        // The key search reports for a name must resolve back to that name.
        let ht = registry(&[(100, "cpu"), (200, "mem"), (300, "net")]);
        let mut key = 0u32;
        assert!(DynamicScreen_search(Some(&ht), "net", Some(&mut key)));
        assert_eq!(key, 300);
        assert_eq!(DynamicScreen_lookup(&ht, key), Some("net"));
    }
}
