//! Partial port of `DynamicColumn.c` — htop's dynamic-column registry.
//!
//! C names are preserved verbatim (`non_snake_case` allowed). Only the
//! `Hashtable_foreach` comparison callback `DynamicColumn_compare` has a
//! faithful safe-Rust body — its logic is a plain name match plus an
//! iterator update, needing no unported substrate.
//!
//! Every other function here depends on substrate that is not ported:
//! `DynamicColumns_new`/`DynamicColumns_delete`/`DynamicColumn_name`/
//! `DynamicColumn_writeField` call `Platform_*`; `DynamicColumns_new`,
//! `DynamicColumns_delete`, `DynamicColumn_search`, and
//! `DynamicColumn_lookup` need the `Hashtable` heap wrapper and its
//! `foreach`/`get` dispatch; `DynamicColumn_done` is a `free()` wrapper
//! (Rust owns its allocations); `DynamicColumn_writeField` also needs
//! `RichString`. Those keep their exact `todo!()` stubs.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// Model of the C `DynamicColumn` struct (`DynamicColumn.h`). Only the
/// `name` field is needed by [`DynamicColumn_compare`]; the C struct's
/// other fields (`heading`, `caption`, `description`, `width`,
/// `enabled`, `table`) are omitted because that callback never reads
/// them.
pub struct DynamicColumn {
    /// C `char name[32]` — unique, internal-only name.
    pub name: String,
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

/// TODO: port of `const DynamicColumn* DynamicColumn_search(Hashtable* dynamics, const char* name, unsigned int* key` from `DynamicColumn.c:61`.
pub fn DynamicColumn_search() {
    todo!("port of DynamicColumn.c:61")
}

/// TODO: port of `const DynamicColumn* DynamicColumn_lookup(Hashtable* dynamics, unsigned int key` from `DynamicColumn.c:70`.
pub fn DynamicColumn_lookup() {
    todo!("port of DynamicColumn.c:70")
}

/// TODO: port of `bool DynamicColumn_writeField(const Process* proc, RichString* str, unsigned int key` from `DynamicColumn.c:74`.
pub fn DynamicColumn_writeField() {
    todo!("port of DynamicColumn.c:74")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str) -> DynamicColumn {
        DynamicColumn {
            name: name.to_string(),
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
