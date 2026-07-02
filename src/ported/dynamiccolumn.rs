//! Partial port of `DynamicColumn.c` — htop's dynamic-column registry.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C function taking
//! `DynamicColumn* this` ports to a free fn (the shape `Vector.c`/
//! `History.c` use), not a method.
//!
//! Ported (self-contained, no unported substrate):
//! - `DynamicColumn_compare` (`DynamicColumn.c:52`) — `static` in C; the
//!   `Hashtable_foreach` comparison callback. Its logic is a plain
//!   `String_eq` name match (`strcmp == 0`) plus an iterator update,
//!   needing no unported substrate.
//! - `DynamicColumn_search` (`DynamicColumn.c:61`) — drives
//!   `Hashtable_foreach` over the registry, accumulating the matched key,
//!   then re-reads the value via `Hashtable_get`. Both are ported now
//!   (`hashtable.rs`).
//! - `DynamicColumn_lookup` (`DynamicColumn.c:70`) — thin wrapper over
//!   `Hashtable_get`, ported now.
//!
//! Stubbed (cannot be ported faithfully yet — specific blocker named):
//! - `DynamicColumns_new` (`DynamicColumn.c:22`) — calls
//!   `Platform_dynamicColumns()`, an unported `Platform_*` fn (the
//!   `Hashtable_new` fallback IS ported).
//! - `DynamicColumns_delete` (`DynamicColumn.c:29`) — calls
//!   `Platform_dynamicColumnsDone` (unported `Platform_*`) and
//!   `Hashtable_delete` (still a stub in `hashtable.rs`; `Drop` frees the
//!   owned `Vec`/`Box` fields).
//! - `DynamicColumn_name` (`DynamicColumn.c:36`) — thin wrapper over
//!   `Platform_dynamicColumnName`, an unported `Platform_*` fn.
//! - `DynamicColumn_done` (`DynamicColumn.c:40`) — `free()`s `heading`,
//!   `caption`, `description`. No faithful safe-Rust analog: a
//!   `DynamicColumn` owns its `String` fields, so `Drop` frees them
//!   automatically (same precedent as `History_delete`).
//! - `DynamicColumn_writeField` (`DynamicColumn.c:74`) — thin wrapper
//!   over `Platform_dynamicColumnWriteField`; also needs the `Process`
//!   and `RichString` graph.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // preserve the C-style class name `DynamicColumn_class`
#![allow(dead_code)]

use crate::ported::hashtable::{Hashtable, Hashtable_foreach, Hashtable_get};
use crate::ported::object::{Object, ObjectClass, Object_class};

/// Model of the C `DynamicColumn` struct (`DynamicColumn.h`). Only the
/// `name` field is needed by [`DynamicColumn_compare`]; the C struct's
/// other fields (`heading`, `caption`, `description`, `width`,
/// `enabled`, `table`) are omitted because that callback never reads
/// them.
pub struct DynamicColumn {
    /// C `char name[32]` — unique, internal-only name.
    pub name: String,
}

/// Class descriptor for [`DynamicColumn`], present solely so a
/// `DynamicColumn` can be stored as a `Box<dyn Object>` value in the
/// ported [`Hashtable`] (whose value type is `dyn Object`). htop's
/// `Hashtable` stores raw `void*`, so C's `DynamicColumn` is **not** an
/// `Object` subclass — there is no `DynamicColumn_class` in htop; this
/// exists only as the safe-Rust adapter for the ported table's owned
/// `dyn Object` value model. Rooted at [`Object_class`]; it sets no
/// `display`/`compare` slots (the table never dispatches through them).
static DynamicColumn_class: ObjectClass = ObjectClass {
    extends: Some(&Object_class),
};

impl Object for DynamicColumn {
    fn klass(&self) -> &'static ObjectClass {
        &DynamicColumn_class
    }
}

/// Model of the file-private C `DynamicIterator` struct
/// (`DynamicColumn.c:47`). `data` is a borrow of the matched column
/// (C's `const DynamicColumn*`), tied to the lifetime of the scanned
/// values.
pub struct DynamicIterator<'a> {
    /// Needle name being searched for.
    pub name: &'a str,
    /// Matched column, or `None` (C `NULL`) if unmatched.
    pub data: Option<&'a DynamicColumn>,
    /// Key of the matched column (C `unsigned int`, `0` when unmatched).
    pub key: u32,
}

/// Port of `DynamicColumn.c:52`. `Hashtable_foreach` callback: when the
/// visited column's name equals the iterator's search name (C
/// `String_eq`, i.e. exact `strcmp == 0`), record the column and its
/// key into the iterator. `ht_key_t` is C `unsigned int`.
pub fn DynamicColumn_compare<'a>(
    key: u32,
    value: &'a DynamicColumn,
    iter: &mut DynamicIterator<'a>,
) {
    if iter.name == value.name {
        iter.data = Some(value);
        iter.key = key;
    }
}

/// TODO: port of `Hashtable* DynamicColumns_new(void` from `DynamicColumn.c:22`.
pub fn DynamicColumns_new() {
    todo!("port of DynamicColumn.c:22")
}

/// TODO: port of `void DynamicColumns_delete(Hashtable* dynamics` from `DynamicColumn.c:29`.
pub fn DynamicColumns_delete() {
    todo!("port of DynamicColumn.c:29")
}

/// TODO: port of `const char* DynamicColumn_name(unsigned int key` from `DynamicColumn.c:36`.
pub fn DynamicColumn_name() {
    todo!("port of DynamicColumn.c:36")
}

/// TODO: port of `void DynamicColumn_done(DynamicColumn* this` from `DynamicColumn.c:40`.
pub fn DynamicColumn_done() {
    todo!("port of DynamicColumn.c:40")
}

/// Port of `DynamicColumn.c:61`. Scans the registry for a column whose
/// name equals `name`, returning it (C `iter.data`) and writing its key
/// through `key` when the out-param is non-null (C `if (key) *key =
/// iter.key`). A null table is skipped (C `if (dynamics)`), leaving
/// `key` `0` and returning `None`.
///
/// The ported [`Hashtable_foreach`] hands each value to the callback as a
/// `&dyn Object` valid only for that call, so the matched reference
/// cannot escape the scan the way C's `void* value` pointer does. The
/// faithful two-step: drive [`DynamicColumn_compare`] into a scratch
/// [`DynamicIterator`] per bucket (its logic is what C runs), record the
/// `Copy` key of the last match (C's callback overwrites on every match,
/// so the last visited wins), then re-read the value with the same probe
/// order via [`Hashtable_get`]. `Hashtable_get` and `Hashtable_foreach`
/// resolve to the same stored value, so this yields exactly C's result.
pub fn DynamicColumn_search<'a>(
    dynamics: Option<&'a Hashtable>,
    name: &str,
    key: Option<&mut u32>,
) -> Option<&'a DynamicColumn> {
    // C: DynamicIterator iter = { .key = 0, .data = NULL, .name = name };
    let mut matched_key: u32 = 0;
    let mut matched = false;

    if let Some(dynamics) = dynamics {
        Hashtable_foreach(dynamics, &mut |k, value| {
            // C: const DynamicColumn* column = (const DynamicColumn*)value;
            let any: &dyn core::any::Any = value;
            let column = any
                .downcast_ref::<DynamicColumn>()
                .expect("DynamicColumn_search: hashtable value is not a DynamicColumn");

            let mut iter = DynamicIterator {
                name,
                data: None,
                key: 0,
            };
            DynamicColumn_compare(k, column, &mut iter);
            if iter.data.is_some() {
                matched = true;
                matched_key = iter.key;
            }
        });
    }

    // C: if (key) *key = iter.key;
    if let Some(key) = key {
        *key = matched_key;
    }

    // C: return iter.data;
    if matched {
        dynamics
            .and_then(|d| Hashtable_get(d, matched_key))
            .and_then(|o| {
                let any: &dyn core::any::Any = o;
                any.downcast_ref::<DynamicColumn>()
            })
    } else {
        None
    }
}

/// Port of `DynamicColumn.c:70`. Thin wrapper over [`Hashtable_get`]:
/// C casts the returned `void*` straight to `const DynamicColumn*`; the
/// safe-Rust analog downcasts the `&dyn Object` value back to its
/// concrete type via `Any`. A miss returns `None` (C `NULL`).
pub fn DynamicColumn_lookup(dynamics: &Hashtable, key: u32) -> Option<&DynamicColumn> {
    Hashtable_get(dynamics, key).and_then(|o| {
        let any: &dyn core::any::Any = o;
        any.downcast_ref::<DynamicColumn>()
    })
}

/// TODO: port of `bool DynamicColumn_writeField(const Process* proc, RichString* str, unsigned int key` from `DynamicColumn.c:74`.
pub fn DynamicColumn_writeField() {
    todo!("port of DynamicColumn.c:74")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::hashtable::{Hashtable_new, Hashtable_put};

    fn col(name: &str) -> DynamicColumn {
        DynamicColumn {
            name: name.to_string(),
        }
    }

    // Build a registry the way DynamicColumns_new would: a non-owner
    // table (values live in the caller's world) keyed by column index.
    fn registry(names: &[(u32, &str)]) -> crate::ported::hashtable::Hashtable {
        let mut ht = Hashtable_new(0, false);
        for &(k, n) in names {
            Hashtable_put(&mut ht, k, Box::new(col(n)));
        }
        ht
    }

    #[test]
    fn lookup_returns_column_for_present_key() {
        let ht = registry(&[(1, "cpu"), (2, "mem"), (100, "io")]);
        assert_eq!(DynamicColumn_lookup(&ht, 1).unwrap().name, "cpu");
        assert_eq!(DynamicColumn_lookup(&ht, 2).unwrap().name, "mem");
        assert_eq!(DynamicColumn_lookup(&ht, 100).unwrap().name, "io");
    }

    #[test]
    fn lookup_returns_none_for_absent_key() {
        let ht = registry(&[(1, "cpu")]);
        assert!(DynamicColumn_lookup(&ht, 999).is_none());
    }

    #[test]
    fn search_finds_by_name_and_writes_key() {
        let ht = registry(&[(10, "cpu"), (20, "mem"), (30, "io")]);
        let mut key: u32 = 0;
        let found = DynamicColumn_search(Some(&ht), "mem", Some(&mut key));
        assert_eq!(found.unwrap().name, "mem");
        assert_eq!(key, 20);
    }

    #[test]
    fn search_key_out_param_is_optional() {
        // C: `if (key)` — passing None (C NULL) must not fault.
        let ht = registry(&[(5, "cpu")]);
        let found = DynamicColumn_search(Some(&ht), "cpu", None);
        assert_eq!(found.unwrap().name, "cpu");
    }

    #[test]
    fn search_miss_returns_none_and_zeroes_key() {
        let ht = registry(&[(1, "cpu"), (2, "mem")]);
        let mut key: u32 = 12345;
        let found = DynamicColumn_search(Some(&ht), "nonexistent", Some(&mut key));
        assert!(found.is_none());
        // C leaves iter.key at its 0 init when nothing matches.
        assert_eq!(key, 0);
    }

    #[test]
    fn search_null_table_returns_none() {
        // C: `if (dynamics)` guards the foreach; a null table yields
        // iter.data == NULL and iter.key == 0.
        let mut key: u32 = 7;
        let found = DynamicColumn_search(None, "cpu", Some(&mut key));
        assert!(found.is_none());
        assert_eq!(key, 0);
    }

    #[test]
    fn search_is_case_sensitive_like_string_eq() {
        // String_eq is strcmp==0: "CPU" != "cpu".
        let ht = registry(&[(1, "CPU")]);
        let mut key: u32 = 0;
        assert!(DynamicColumn_search(Some(&ht), "cpu", Some(&mut key)).is_none());
        assert_eq!(key, 0);
        assert_eq!(
            DynamicColumn_search(Some(&ht), "CPU", None).unwrap().name,
            "CPU"
        );
    }

    #[test]
    fn search_result_matches_lookup_of_returned_key() {
        // The key written by search must round-trip through lookup to the
        // same column (the two-phase foreach/get must agree).
        let ht = registry(&[(3, "alpha"), (17, "beta"), (42, "gamma")]);
        for name in ["alpha", "beta", "gamma"] {
            let mut key: u32 = 0;
            let s = DynamicColumn_search(Some(&ht), name, Some(&mut key)).unwrap();
            let l = DynamicColumn_lookup(&ht, key).unwrap();
            assert_eq!(s.name, name);
            assert_eq!(l.name, name);
            assert!(std::ptr::eq(s, l));
        }
    }

    #[test]
    fn compare_records_match_and_key() {
        let cpu = col("cpu");
        let mut iter = DynamicIterator {
            name: "cpu",
            data: None,
            key: 0,
        };
        DynamicColumn_compare(7, &cpu, &mut iter);
        assert_eq!(iter.key, 7);
        assert!(matches!(iter.data, Some(c) if c.name == "cpu"));
    }

    #[test]
    fn compare_ignores_non_match() {
        let mem = col("mem");
        let mut iter = DynamicIterator {
            name: "cpu",
            data: None,
            key: 0,
        };
        DynamicColumn_compare(3, &mem, &mut iter);
        // no match: iterator left untouched (C leaves .data=NULL, .key=0)
        assert_eq!(iter.key, 0);
        assert!(iter.data.is_none());
    }

    #[test]
    fn compare_is_exact_case_sensitive_strcmp() {
        // String_eq is strcmp==0: case-sensitive, no trimming
        let upper = col("CPU");
        let mut iter = DynamicIterator {
            name: "cpu",
            data: None,
            key: 0,
        };
        DynamicColumn_compare(9, &upper, &mut iter);
        assert_eq!(iter.key, 0);
        assert!(iter.data.is_none());
    }

    #[test]
    fn compare_last_match_wins() {
        // C callback overwrites on every match; a later duplicate wins
        let a = col("dup");
        let b = col("dup");
        let mut iter = DynamicIterator {
            name: "dup",
            data: None,
            key: 0,
        };
        DynamicColumn_compare(1, &a, &mut iter);
        DynamicColumn_compare(2, &b, &mut iter);
        assert_eq!(iter.key, 2);
        assert!(std::ptr::eq(iter.data.unwrap(), &b));
    }
}
