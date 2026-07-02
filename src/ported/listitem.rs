//! Partial port of `ListItem.c` — htop's plain list row object.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port.
//!
//! [`ListItem_compare`], [`ListItem_init`], [`ListItem_append`],
//! [`ListItem_new`], and [`ListItem_display`] are ported. The comparison,
//! init, and append paths only read/write the `value` string (and the plain
//! `key`/`moving` scalars). [`ListItem_display`] renders both paths: the
//! always-run append of `value` with `CRT_colors[DEFAULT_COLOR]` through the
//! real [`RichString`]/[`ColorElements`] substrate, and the `moving` prefix
//! branch, which selects its glyph (`↕ ` vs `+ `) from the now-available
//! [`CRT_utf8`] (`CRT.c:91`, in `crt.rs`) and writes it via
//! [`RichString_writeWide`]. The [`Object`] vtable is wired via
//! `impl Object for ListItem` (dispatching `display`/`compare` faithfully).
//! [`ListItem_new`] is the `AllocThis` constructor: it returns an owned
//! `ListItem` after [`ListItem_init`], mirroring the `History_new`/
//! `Affinity_new` owned-return idiom. `ListItem_delete` (frees through the
//! `Object` cast) has no safe-Rust free-fn analog — destruction is `Drop` —
//! so it remains a `todo!()` stub.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::cmp::Ordering;
use std::sync::atomic::Ordering as AtomicOrdering;

use crate::ported::crt::{CRT_utf8, ColorElements, ColorScheme};
use crate::ported::object::{Object, ObjectClass};
use crate::ported::richstring::{RichString, RichString_appendWide, RichString_writeWide};

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

/// Port of `ListItem.c:27`. Writes the item into `out`: when `moving`, the
/// C first draws a `RichString_writeWide` prefix (`CRT_utf8 ? "↕ " : "+ "`)
/// in `CRT_colors[DEFAULT_COLOR]`, then always appends `value` in the same
/// color. `CRT_colors[DEFAULT_COLOR]` is `DEFAULT_COLOR.packed(active
/// scheme)`. The moving-glyph selection reads the [`CRT_utf8`] flag
/// (`CRT.c:91`); the non-`HAVE_LIBNCURSESW` build has no `↕ ` alternative and
/// always uses `"+ "`, but this port models the `HAVE_LIBNCURSESW` path.
pub fn ListItem_display(this: &ListItem, out: &mut RichString) {
    let default_color = ColorElements::DEFAULT_COLOR.packed(ColorScheme::active());
    if this.moving {
        // C (HAVE_LIBNCURSESW): RichString_writeWide(out, CRT_colors[DEFAULT_COLOR],
        //                                             CRT_utf8 ? "↕ " : "+ ");
        let glyph: &[u8] = if CRT_utf8.load(AtomicOrdering::Relaxed) {
            "↕ ".as_bytes()
        } else {
            "+ ".as_bytes()
        };
        RichString_writeWide(out, default_color, glyph);
    }
    RichString_appendWide(out, default_color, this.value.as_bytes());
}

/// Port of `const ObjectClass ListItem_class` (`ListItem.c:70`). The C
/// initializer sets `.display`/`.delete`/`.compare` but no `.extends`, so
/// `extends` is `NULL` (zero-initialized) — ported verbatim as `None`.
/// Declared `static` so its address (the type's identity, per [`Object_isA`])
/// is stable.
static ListItem_class: ObjectClass = ObjectClass { extends: None };

impl Object for ListItem {
    /// C `this->klass` set to `&ListItem_class`.
    fn klass(&self) -> &'static ObjectClass {
        &ListItem_class
    }

    /// C vtable slot `.display = ListItem_display`.
    fn display(&self, out: &mut RichString) {
        ListItem_display(self, out);
    }

    /// C vtable slot `.compare = ListItem_compare`. The C comparator casts
    /// the opaque `const void*` back to `ListItem`; the safe-Rust analog
    /// downcasts the trait object via `Any`.
    fn compare(&self, other: &dyn Object) -> i32 {
        let any: &dyn core::any::Any = other;
        let o = any
            .downcast_ref::<ListItem>()
            .expect("ListItem_compare called across incompatible classes");
        ListItem_compare(self, o)
    }
}

/// Port of `ListItem_init(ListItem* this, const char* value, int key)`
/// from `ListItem.c:41`. Sets `value` (C `xStrdup`, an owning copy),
/// `key`, and clears `moving` — no vtable/`Object` field is touched.
pub fn ListItem_init(this: &mut ListItem, value: &str, key: i32) {
    this.value = value.to_string();
    this.key = key;
    this.moving = false;
}

/// Port of `ListItem* ListItem_new(const char* value, int key)` from
/// `ListItem.c:47`. The C body is `AllocThis(ListItem)` followed by
/// `ListItem_init(this, value, key)`, returning the fresh object. The
/// heap allocation becomes an owned return value here — mirroring the
/// `History_new`/`Affinity_new` owned-return idiom — and [`ListItem_init`]
/// fills in `value`/`key` and clears `moving`.
pub fn ListItem_new(value: &str, key: i32) -> ListItem {
    let mut this = ListItem {
        value: String::new(),
        key: 0,
        moving: false,
    };
    ListItem_init(&mut this, value, key);
    this
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

/// Port of `static inline const char* ListItem_getRef(const ListItem* this)`
/// from `ListItem.h:37`. The C body is `return this->value;` — a borrow of
/// the item's owned `value` string, not a copy.
pub fn ListItem_getRef(this: &ListItem) -> &str {
    &this.value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(value: &str) -> ListItem {
        ListItem {
            value: value.to_string(),
            key: 0,
            moving: false,
        }
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
        let mut it = ListItem {
            value: "old".to_string(),
            key: -1,
            moving: true,
        };
        ListItem_init(&mut it, "new", 42);
        assert_eq!(it.value, "new");
        assert_eq!(it.key, 42);
        assert!(!it.moving);
    }

    /// Visible characters of the valid `[0, chlen)` range.
    fn rendered(rs: &RichString) -> String {
        rs.chptr
            .iter()
            .take(rs.chlen as usize)
            .map(|c| c.chars)
            .collect()
    }

    #[test]
    fn display_appends_value_in_default_color() {
        let it = ListItem {
            value: "hello".to_string(),
            key: 0,
            moving: false,
        };
        let mut rs = RichString::new();
        ListItem_display(&it, &mut rs);
        assert_eq!(rendered(&rs), "hello");
        assert_eq!(rs.chlen, 5);
        // CRT_colors[DEFAULT_COLOR], masked as the ASCII/wide write path does.
        let expect = ColorElements::DEFAULT_COLOR.packed(ColorScheme::active()) & 0xffffff;
        for i in 0..5 {
            assert_eq!(rs.chptr[i].attr, expect, "attr at {i}");
        }
    }

    #[test]
    fn object_display_dispatches_to_listitem_display() {
        let it = ListItem {
            value: "abc".to_string(),
            key: 0,
            moving: false,
        };
        let mut rs = RichString::new();
        // Dispatch through the Object vtable, not the free fn directly.
        Object::display(&it, &mut rs);
        assert_eq!(rendered(&rs), "abc");
    }

    #[test]
    fn object_compare_dispatches_to_listitem_compare() {
        let a = ListItem {
            value: "apple".to_string(),
            key: 0,
            moving: false,
        };
        let b = ListItem {
            value: "banana".to_string(),
            key: 0,
            moving: false,
        };
        assert_eq!(a.compare(&b), -1);
        assert_eq!(b.compare(&a), 1);
        assert_eq!(
            a.compare(&ListItem {
                value: "apple".to_string(),
                key: 9,
                moving: true
            }),
            0
        );
    }

    #[test]
    fn new_initializes_value_key_and_clears_moving() {
        let it = ListItem_new("entry", 7);
        assert_eq!(it.value, "entry");
        assert_eq!(it.key, 7);
        assert!(!it.moving);
        // Owned return: an independent object each call (C AllocThis).
        let other = ListItem_new("", -3);
        assert_eq!(other.value, "");
        assert_eq!(other.key, -3);
        assert!(!other.moving);
    }

    #[test]
    fn display_moving_branch_selects_glyph_by_crt_utf8() {
        // Both glyph branches are exercised in one test because they mutate the
        // process-shared CRT_utf8 global; splitting them would race under the
        // parallel test runner. The moving prefix is written via
        // RichString_writeWide (from index 0), then value is appended.
        let it = ListItem {
            value: "x".to_string(),
            key: 0,
            moving: true,
        };

        // Non-UTF-8: ASCII "+ " prefix.
        CRT_utf8.store(false, AtomicOrdering::Relaxed);
        let mut rs = RichString::new();
        ListItem_display(&it, &mut rs);
        assert_eq!(rendered(&rs), "+ x");

        // UTF-8: the "↕ " arrow prefix.
        CRT_utf8.store(true, AtomicOrdering::Relaxed);
        let mut rs = RichString::new();
        ListItem_display(&it, &mut rs);
        assert_eq!(rendered(&rs), "↕ x");

        // Restore the shared global for other tests.
        CRT_utf8.store(false, AtomicOrdering::Relaxed);
    }

    #[test]
    fn get_ref_borrows_value() {
        let it = item("payload");
        assert_eq!(ListItem_getRef(&it), "payload");
        assert_eq!(ListItem_getRef(&item("")), "");
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
