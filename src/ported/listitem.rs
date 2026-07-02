//! Partial port of `ListItem.c` — htop's plain list row object.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port.
//!
//! [`ListItem_compare`], [`ListItem_init`], and [`ListItem_append`] are
//! ported: they only read/write the `value` string (and the plain
//! `key`/`moving` scalars) and have no `Object`, `CRT`, or `RichString`
//! dependency. `ListItem_new` (allocates via `AllocThis` + wires the
//! `Object` vtable), `ListItem_display` (draws through `RichString`/
//! `CRT_colors`), and `ListItem_delete` (frees through the `Object`
//! cast) depend on substrate not yet ported, so they remain `todo!()`
//! stubs — `gen_port_report.py` counts those as *stubbed*, not
//! *ported*.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::cmp::Ordering;

/// Minimal model of the C `ListItem` struct (`ListItem.h`). The C type
/// embeds an `Object super` (vtable) as its first field; that is only
/// needed by the vtable-dispatched `display`/`delete`/`new` paths, which
/// are still stubbed, so it is omitted here. `value` is the compared
/// string; `key` and `moving` mirror the remaining C fields.
pub struct ListItem {
    pub value: String,
    pub key: i32,
    pub moving: bool,
}

/// TODO: port of `void ListItem_delete(Object* cast` from `ListItem.c:21`.
pub fn ListItem_delete() {
    todo!("port of ListItem.c:21")
}

/// TODO: port of `void ListItem_display(const Object* cast, RichString* out` from `ListItem.c:27`.
pub fn ListItem_display() {
    todo!("port of ListItem.c:27")
}

/// Port of `ListItem_init(ListItem* this, const char* value, int key)`
/// from `ListItem.c:41`. Sets `value` (C `xStrdup`, an owning copy),
/// `key`, and clears `moving` — no vtable/`Object` field is touched.
pub fn ListItem_init(this: &mut ListItem, value: &str, key: i32) {
    this.value = value.to_string();
    this.key = key;
    this.moving = false;
}

/// TODO: port of `ListItem* ListItem_new(const char* value, int key` from `ListItem.c:47`.
pub fn ListItem_new() {
    todo!("port of ListItem.c:47")
}

/// Port of `ListItem_append(ListItem* this, const char* text)` from
/// `ListItem.c:53`. Appends `text` onto `value`. The C code does a
/// manual `xRealloc` + `memcpy` + NUL terminate; Rust's growable
/// `String` expresses the same concatenation directly.
pub fn ListItem_append(this: &mut ListItem, text: &str) {
    this.value.push_str(text);
}

/// Port of `ListItem_compare(const void* cast1, const void* cast2)`
/// from `ListItem.c:62`. The C body is `return strcmp(obj1->value,
/// obj2->value);` — a case-sensitive, byte-wise lexicographic compare.
/// Rust `str` ordering compares the UTF-8 bytes unsigned, matching
/// `strcmp`'s `unsigned char` ordering; the result is normalized to
/// the sign convention `strcmp` guarantees (`< 0`, `0`, `> 0`).
pub fn ListItem_compare(cast1: &ListItem, cast2: &ListItem) -> i32 {
    match cast1.value.as_bytes().cmp(cast2.value.as_bytes()) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(value: &str) -> ListItem {
        ListItem { value: value.to_string(), key: 0, moving: false }
    }

    #[test]
    fn compare_equal_returns_zero() {
        assert_eq!(ListItem_compare(&item("alpha"), &item("alpha")), 0);
        assert_eq!(ListItem_compare(&item(""), &item("")), 0);
    }

    #[test]
    fn compare_orders_lexicographically() {
        // "apple" < "banana"
        assert_eq!(ListItem_compare(&item("apple"), &item("banana")), -1);
        assert_eq!(ListItem_compare(&item("banana"), &item("apple")), 1);
        // prefix is less than the longer string (strcmp: NUL < any byte)
        assert_eq!(ListItem_compare(&item("app"), &item("apple")), -1);
        assert_eq!(ListItem_compare(&item("apple"), &item("app")), 1);
    }

    #[test]
    fn compare_is_case_sensitive() {
        // strcmp is byte-wise: 'A' (0x41) < 'a' (0x61), so uppercase sorts first
        assert_eq!(ListItem_compare(&item("Apple"), &item("apple")), -1);
        assert_eq!(ListItem_compare(&item("apple"), &item("Apple")), 1);
        // differs from a case-insensitive compare, which would be 0 here
        assert_ne!(ListItem_compare(&item("ABC"), &item("abc")), 0);
    }

    #[test]
    fn init_sets_value_key_and_clears_moving() {
        let mut it = ListItem { value: "old".to_string(), key: -1, moving: true };
        ListItem_init(&mut it, "new", 42);
        assert_eq!(it.value, "new");
        assert_eq!(it.key, 42);
        assert!(!it.moving);
    }

    #[test]
    fn append_concatenates_onto_value() {
        let mut it = item("foo");
        ListItem_append(&mut it, "bar");
        assert_eq!(it.value, "foobar");
        // appending empty text leaves value unchanged
        ListItem_append(&mut it, "");
        assert_eq!(it.value, "foobar");
        // append onto an empty value
        let mut empty = item("");
        ListItem_append(&mut empty, "x");
        assert_eq!(empty.value, "x");
    }
}
