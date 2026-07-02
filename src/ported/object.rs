//! Port of `Object.c` — htop's minimal single-inheritance class runtime.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port.
//!
//! htop's `ObjectClass` is a vtable struct:
//!
//! ```c
//! typedef struct ObjectClass_ {
//!    const void* const extends;
//!    const Object_Display display;
//!    const Object_Delete  delete;
//!    const Object_Compare compare;
//! } ObjectClass;
//! ```
//!
//! and every `Object` carries a `const ObjectClass* klass`. Type
//! identity is *pointer identity* of the statically-allocated class
//! objects, and the class hierarchy is the singly-linked chain formed
//! by each class's `extends` pointer.
//!
//! `Object_isA` (`Object.c:20`) is the only free function in the file,
//! and it is faithfully portable: it never *dispatches* through the
//! `display`/`delete`/`compare` function pointers (that vtable
//! dispatch is exactly what the port rules forbid). It only reads the
//! `klass` field and walks the `extends` chain, comparing each link's
//! pointer against `klass`. So the two struct fields it touches —
//! `Object::klass` and `ObjectClass::extends` — are the only fields
//! modeled here; the three function-pointer fields (`display`,
//! `delete`, `compare`) are omitted because no ported code reads them.
//!
//! To reproduce the C pointer-identity check exactly, `ObjectClass` is
//! compared by *address* (`core::ptr::eq`) rather than by value: two
//! distinct classes with identical `extends` must still compare
//! unequal, just as two distinct `const ObjectClass` globals do in C.
//!
//! Not ported: the `AllocThis` / `Object_delete` / `Object_display` /
//! `Object_compare` macros in `Object.h` — they perform heap
//! allocation and vtable dispatch (through `RichString`, comparators,
//! and destructors) with no faithful safe-Rust analog.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

/// A class descriptor. Faithful subset of C's `ObjectClass_`: only the
/// `extends` link (the base class, or `None` for a root) is modeled,
/// because `Object_isA` reads nothing else. The omitted C fields are
/// the `display`, `delete`, and `compare` function pointers (vtable
/// dispatch, unported).
pub struct ObjectClass<'a> {
    /// C `const void* const extends`: the base class, or `None` at the
    /// root of the hierarchy (C `NULL`).
    pub extends: Option<&'a ObjectClass<'a>>,
}

/// An object header. Faithful subset of C's `struct Object_`: only the
/// `klass` back-pointer is modeled (the sole field the C struct has).
pub struct Object<'a> {
    /// C `const ObjectClass* klass`: the object's concrete class.
    pub klass: &'a ObjectClass<'a>,
}

/// Port of `const ObjectClass Object_class` from `Object.c:16`:
/// `{ .extends = NULL }`. The root of every htop class hierarchy. The
/// three unlisted C fields (`display`, `delete`, `compare`) default to
/// `NULL` in the C initializer and are not modeled here.
pub const Object_class: ObjectClass<'static> = ObjectClass { extends: None };

/// Port of `bool Object_isA(const Object* o, const ObjectClass* klass)`
/// from `Object.c:20`. Returns `false` for a null object (C `if (!o)`),
/// otherwise walks the object's class chain from `o->klass` up through
/// each `extends` link and returns `true` as soon as a link's address
/// equals `klass` (C `type == klass`, a pointer-identity compare),
/// else `false` when the chain ends (C `NULL`).
pub fn Object_isA(o: Option<&Object>, klass: &ObjectClass) -> bool {
    let o = match o {
        Some(o) => o,
        None => return false,
    };

    let mut type_ = Some(o.klass);
    while let Some(t) = type_ {
        if core::ptr::eq(t, klass) {
            return true;
        }
        type_ = t.extends;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // A three-level hierarchy of statically-allocated classes, mirroring
    // htop's `const ObjectClass Foo_class = { .extends = Class(Bar) }`
    // pattern. Identity is by address, so these must be `static`.
    static BASE: ObjectClass = ObjectClass { extends: None };
    static MID: ObjectClass = ObjectClass { extends: Some(&BASE) };
    static LEAF: ObjectClass = ObjectClass { extends: Some(&MID) };
    // An unrelated root class, distinct address, same (empty) shape.
    static OTHER: ObjectClass = ObjectClass { extends: None };

    /// C `if (!o) return false;` — a null object is never an instance.
    #[test]
    fn null_object_is_never_an_instance() {
        assert!(!Object_isA(None, &BASE));
        assert!(!Object_isA(None, &Object_class));
    }

    /// The exact class always matches (first chain link).
    #[test]
    fn matches_exact_class() {
        let obj = Object { klass: &LEAF };
        assert!(Object_isA(Some(&obj), &LEAF));
    }

    /// Every ancestor along the `extends` chain matches — this is the
    /// whole point of the loop walking `type = type->extends`.
    #[test]
    fn matches_every_ancestor() {
        let obj = Object { klass: &LEAF };
        assert!(Object_isA(Some(&obj), &MID));
        assert!(Object_isA(Some(&obj), &BASE));
    }

    /// A class not on the chain does not match, even when its shape is
    /// identical to a class that does — C compares pointers, not values.
    #[test]
    fn does_not_match_unrelated_class() {
        let obj = Object { klass: &LEAF };
        assert!(!Object_isA(Some(&obj), &OTHER));
        // BASE and OTHER are value-identical (both `{ extends: None }`)
        // yet distinct addresses: identity, not structural equality.
        assert!(Object_isA(Some(&obj), &BASE));
        assert!(!Object_isA(Some(&obj), &OTHER));
    }

    /// A descendant class is NOT an ancestor: an object of the middle
    /// class is not an instance of the leaf below it. The walk only
    /// goes up (`extends`), never down.
    #[test]
    fn does_not_match_descendant_class() {
        let obj = Object { klass: &MID };
        assert!(Object_isA(Some(&obj), &MID));
        assert!(Object_isA(Some(&obj), &BASE));
        assert!(!Object_isA(Some(&obj), &LEAF));
    }
}
