//! Port of `Object.c` / `Object.h` — htop's hand-rolled single-inheritance
//! class runtime, the base every displayable/comparable object (Row,
//! Process, ListItem, Meter, Panel items, …) inherits from.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # C model
//!
//! htop's `ObjectClass` is a statically-allocated vtable struct, and
//! every `Object` carries a pointer to its class:
//!
//! ```c
//! typedef void(*Object_Display)(const Object*, RichString*);
//! typedef int (*Object_Compare)(const void*, const void*);
//! typedef void(*Object_Delete)(Object*);
//!
//! typedef struct ObjectClass_ {
//!    const void* const    extends;   // base class, or NULL at the root
//!    const Object_Display display;
//!    const Object_Delete  delete;
//!    const Object_Compare compare;
//! } ObjectClass;
//!
//! struct Object_ { const ObjectClass* klass; };
//! ```
//!
//! Type identity is *pointer identity* of the `const ObjectClass`
//! globals; the class hierarchy is the singly-linked chain formed by
//! each class's `extends` pointer. `Object_isA` (`Object.c:20`) walks
//! that chain.
//!
//! # Rust model
//!
//! The three function-pointer slots (`display`, `delete`, `compare`)
//! and the `klass` back-pointer are folded into a single Rust
//! [`Object`] **trait** — the faithful safe-Rust analog of a C vtable:
//!
//! | C vtable slot                       | Rust trait mapping                    |
//! |-------------------------------------|---------------------------------------|
//! | `Object_Display display`            | [`Object::display`]                   |
//! | `Object_Compare compare`            | [`Object::compare`]                   |
//! | `Object_Delete  delete`             | `Drop` (Rust's destructor mechanism)  |
//! | `struct Object_ { klass }`          | [`Object::klass`] (class-identity)    |
//! | `ObjectClass::extends`              | [`ObjectClass::extends`]              |
//!
//! Class identity is retained exactly as in C: each concrete type owns
//! a `static X_class: ObjectClass = ObjectClass { extends: Some(&Base_class) }`
//! (mirroring htop's `const ObjectClass X_class = { .extends = Class(Base) }`)
//! and returns `&X_class` from [`Object::klass`] (mirroring
//! `Object_setClass` / `o->klass`). [`ObjectClass`] carries only the
//! `extends` link because [`Object_isA`] reads nothing else; the three
//! function pointers live on the trait, not on this struct.
//!
//! [`Object_isA`] stays a **free fn** (the only free function in
//! `Object.c`). It never dispatches through `display`/`compare` — it
//! only reads the class chain — so it ports faithfully. Classes are
//! compared by *address* (`core::ptr::eq`) rather than by value: two
//! distinct classes with identical `extends` must still compare
//! unequal, exactly as two distinct `const ObjectClass` globals do in
//! C. That is why [`Object_class`] and every concrete class are
//! `static` (stable address), not `const` (may be duplicated per use
//! site, breaking identity).
//!
//! # Not ported
//!
//! The `Object.h` macros — `Object_getClass`, `Object_setClass`,
//! `Object_delete`, `Object_displayFn`, `Object_display`,
//! `Object_compare`, `Class`, and `AllocThis` — are C text-substitution
//! sugar over `xMalloc` heap allocation and raw function-pointer
//! dispatch. Their safe-Rust equivalents are, respectively: struct
//! construction, the trait methods, `Drop`, and the trait itself. None
//! has a faithful free-function analog, so none is ported as a `fn`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::richstring::RichString;

/// A class descriptor: the faithful subset of C's `ObjectClass_` that
/// [`Object_isA`] reads. Only the `extends` link (the base class, or
/// `None` at a root) is modeled here; the C `display`/`delete`/`compare`
/// function pointers are represented as methods on the [`Object`] trait
/// instead (see the module docs). Instances must be `static` so their
/// address — the type's identity — is stable across the whole program,
/// matching C's `const ObjectClass X_class` globals.
pub struct ObjectClass {
    /// C `const void* const extends`: the base class, or `None` at the
    /// root of the hierarchy (C `NULL`).
    pub extends: Option<&'static ObjectClass>,
}

/// Port of `const ObjectClass Object_class` from `Object.c:16`:
/// `{ .extends = NULL }`. The root of every htop class hierarchy.
///
/// Declared `static` (not `const`) so `&Object_class` denotes one fixed
/// address — the sentinel every root subclass points its `extends` at,
/// and the identity [`Object_isA`] compares against. The three unlisted
/// C fields (`display`, `delete`, `compare`) are `NULL` in the C
/// initializer; their Rust analogs are the (defaulted) trait methods.
pub static Object_class: ObjectClass = ObjectClass { extends: None };

/// The Rust analog of htop's `ObjectClass` vtable and `struct Object_`
/// combined: the base every displayable/comparable htop object
/// implements.
///
/// A concrete type (Row, ListItem, Meter, Process, …) implements this
/// by:
///  1. declaring `static Foo_class: ObjectClass = ObjectClass { extends: Some(&Bar_class) }`
///     (mirrors C `const ObjectClass Foo_class = { .extends = Class(Bar), … }`),
///  2. returning `&Foo_class` from [`klass`](Object::klass) (mirrors C
///     `Object_setClass` setting `this->klass`), and
///  3. overriding [`display`](Object::display) / [`compare`](Object::compare)
///     for the vtable slots its C class sets.
///
/// The `Any` supertrait models the `const void*` type-erasure in
/// `Object_Compare`'s C signature: a comparator receives an opaque
/// pointer and casts it back to the concrete type. In safe Rust that
/// cast is `(&dyn Object as &dyn Any).downcast_ref::<Concrete>()`, which
/// requires `Any`. It costs implementors nothing — `Any` is
/// auto-implemented for every `'static` type.
pub trait Object: core::any::Any {
    /// C `o->klass`: the object's concrete class descriptor. This is the
    /// class-identity mechanism [`Object_isA`] walks from.
    fn klass(&self) -> &'static ObjectClass;

    /// C `Object_Display display` (`void(*)(const Object*, RichString*)`),
    /// dispatched by the `Object_display` macro. Renders the object into
    /// `out`. The default models a `NULL` display slot: the C
    /// `Object_display` macro `assert`s the pointer is non-`NULL` before
    /// calling, so a class that never sets it aborts — here, a panic.
    fn display(&self, out: &mut RichString) {
        let _ = out;
        unimplemented!("Object::display: class has no display method (C NULL vtable slot)")
    }

    /// C `Object_Compare compare` (`int(*)(const void*, const void*)`),
    /// dispatched by the `Object_compare(self, other)` macro. Returns an
    /// ordering as a signed int (negative / zero / positive), like the C
    /// comparator. The default models a `NULL` compare slot: the C macro
    /// `assert`s non-`NULL`, so an unset slot aborts — here, a panic.
    fn compare(&self, other: &dyn Object) -> i32 {
        let _ = other;
        unimplemented!("Object::compare: class has no compare method (C NULL vtable slot)")
    }

    /// C `As_Row(this)` — the concrete [`RowClass`](crate::ported::row::RowClass)
    /// vtable for a `Row`-derived object, or `None` for objects that are not
    /// `Row`s (meters, panels, list items). The `Row` display/dispatch path
    /// ([`Row_display`](crate::ported::row::Row_display)) reads the
    /// `writeField` / `isHighlighted` slots through this. The default models
    /// a non-`Row` object.
    fn row_class(&self) -> Option<&'static crate::ported::row::RowClass> {
        None
    }

    /// C `(const Row*)cast` — a view of this object's embedded `Row` base,
    /// or `None` for objects that are not `Row`s. `Row`-derived types return
    /// their embedded `Row` (`Process` → `super_`, `LinuxProcess` →
    /// `super_.super_`); the shared display path reads `host`/`tag`/tomb/new
    /// state through it. The default models a non-`Row` object.
    fn as_row(&self) -> Option<&crate::ported::row::Row> {
        None
    }

    /// C `(const Process*)super` — a view of this object's embedded
    /// [`Process`](crate::ported::process::Process) base, or `None` for
    /// non-`Process` objects. Both `Process` (itself) and `LinuxProcess`
    /// (`super_`) return their embedded `Process`, so a `Process`-level vtable
    /// slot works on either concrete type (C's pointer cast up the embed chain
    /// has no `Any`-downcast analog). The default models a non-`Process`.
    fn as_process(&self) -> Option<&crate::ported::process::Process> {
        None
    }

    /// C `As_Process(this)` — the concrete
    /// [`ProcessClass`](crate::ported::process::ProcessClass) vtable for a
    /// `Process`-derived object, or `None` otherwise. `Process_compare` reads
    /// the `compareByKey` slot through this to dispatch a platform's
    /// key comparator (`LinuxProcess`/`DragonFlyBSDProcess`). The default
    /// models a non-`Process` object.
    fn process_class(&self) -> Option<&'static crate::ported::process::ProcessClass> {
        None
    }

    /// Mutable counterpart of [`as_row`](Object::as_row) — a `&mut` view of
    /// this object's embedded [`Row`](crate::ported::row::Row) base, or
    /// `None` for non-`Row`s. Needed when a `Table` owns its rows as
    /// `Box<dyn Object>` and mutates them in place (scan updates, tree
    /// rebuild). The default models a non-`Row` object.
    fn as_row_mut(&mut self) -> Option<&mut crate::ported::row::Row> {
        None
    }

    /// Mutable counterpart of [`as_process`](Object::as_process) — a `&mut`
    /// view of this object's embedded
    /// [`Process`](crate::ported::process::Process) base, or `None` for
    /// non-`Process`es. Used by the process scan to fill a row in place.
    /// The default models a non-`Process` object.
    fn as_process_mut(&mut self) -> Option<&mut crate::ported::process::Process> {
        None
    }
}

/// Port of `bool Object_isA(const Object* o, const ObjectClass* klass)`
/// from `Object.c:20`. Returns `false` for a null object (C `if (!o)`,
/// modeled as `None`), otherwise walks the object's class chain from
/// `o->klass` up through each `extends` link and returns `true` as soon
/// as a link's address equals `klass` (C `type == klass`, a
/// pointer-identity compare), else `false` when the chain ends (C
/// `NULL`).
pub fn Object_isA(o: Option<&dyn Object>, klass: &ObjectClass) -> bool {
    let o = match o {
        Some(o) => o,
        None => return false,
    };

    let mut type_ = Some(o.klass());
    while let Some(t) = type_ {
        if core::ptr::eq(t, klass) {
            return true;
        }
        type_ = t.extends;
    }

    false
}

/// Port of the `Arg` union from `Object.h:48`:
///
/// ```c
/// typedef union { int i; void* v; } Arg;
/// ```
///
/// A two-way tagged value used by htop as generic callback payload
/// (e.g. `FunctionBar`/`Panel` actions). C's untagged `union` is
/// modeled as a Rust tagged `enum` — the faithful safe-Rust analog. No
/// currently-ported code consumes it, so it is defined minimally; the
/// `void* v` arm keeps the raw pointer (storing one needs no `unsafe`;
/// only dereferencing would).
pub enum Arg {
    /// C `int i`.
    I(i32),
    /// C `void* v`.
    V(*mut core::ffi::c_void),
}

#[cfg(test)]
mod tests {
    use super::*;

    // A three-level hierarchy of statically-allocated classes, mirroring
    // htop's `const ObjectClass Foo_class = { .extends = Class(Bar) }`
    // pattern. Identity is by address, so these must be `static`.
    static BASE: ObjectClass = ObjectClass { extends: None };
    static MID: ObjectClass = ObjectClass {
        extends: Some(&BASE),
    };
    static LEAF: ObjectClass = ObjectClass {
        extends: Some(&MID),
    };
    // An unrelated root class: distinct address, same (empty) shape.
    static OTHER: ObjectClass = ObjectClass { extends: None };

    // A concrete leaf object, the way a real htop type embeds Object and
    // points its klass at its own _class. Overrides display + compare.
    struct Num {
        n: i32,
    }
    impl Object for Num {
        fn klass(&self) -> &'static ObjectClass {
            &LEAF
        }
        fn display(&self, out: &mut RichString) {
            // Emit the number's decimal text through the real ported
            // RichString ASCII-append path (attr 0).
            crate::ported::richstring::RichString_appendAscii(
                out,
                0,
                self.n.to_string().as_bytes(),
            );
        }
        fn compare(&self, other: &dyn Object) -> i32 {
            // C casts the `const void*` back to the concrete type; the
            // safe-Rust analog downcasts the trait object via `Any`.
            let any: &dyn core::any::Any = other;
            let o = any
                .downcast_ref::<Num>()
                .expect("compare called across incompatible classes");
            (self.n > o.n) as i32 - (self.n < o.n) as i32
        }
    }

    // A concrete object at the middle class, for the descendant test.
    struct MidObj;
    impl Object for MidObj {
        fn klass(&self) -> &'static ObjectClass {
            &MID
        }
    }

    // A concrete object rooted at an unrelated class that never sets its
    // display/compare slots (models a C class with NULL vtable entries).
    struct Widget;
    impl Object for Widget {
        fn klass(&self) -> &'static ObjectClass {
            &OTHER
        }
    }

    fn empty_richstring() -> RichString {
        RichString::new()
    }

    /// C `if (!o) return false;` — a null object is never an instance.
    #[test]
    fn null_object_is_never_an_instance() {
        assert!(!Object_isA(None, &BASE));
        assert!(!Object_isA(None, &Object_class));
    }

    /// The exact class always matches (first chain link).
    #[test]
    fn matches_exact_class() {
        let obj = Num { n: 0 };
        assert!(Object_isA(Some(&obj), &LEAF));
    }

    /// Every ancestor along the `extends` chain matches — this is the
    /// whole point of the loop walking `type = type->extends`.
    #[test]
    fn matches_every_ancestor() {
        let obj = Num { n: 0 };
        assert!(Object_isA(Some(&obj), &MID));
        assert!(Object_isA(Some(&obj), &BASE));
    }

    /// A class not on the chain does not match, even when its shape is
    /// identical to a class that does — C compares pointers, not values.
    #[test]
    fn does_not_match_unrelated_class() {
        let obj = Num { n: 0 };
        assert!(!Object_isA(Some(&obj), &OTHER));
        // BASE and OTHER are value-identical (both `{ extends: None }`)
        // yet distinct addresses: identity, not structural equality.
        assert!(Object_isA(Some(&obj), &BASE));
        assert!(!Object_isA(Some(&obj), &OTHER));
    }

    /// A descendant class is NOT an ancestor: an object of the middle
    /// class is not an instance of the leaf below it. The walk only goes
    /// up (`extends`), never down.
    #[test]
    fn does_not_match_descendant_class() {
        let obj = MidObj;
        assert!(Object_isA(Some(&obj), &MID));
        assert!(Object_isA(Some(&obj), &BASE));
        assert!(!Object_isA(Some(&obj), &LEAF));
    }

    /// `display` dispatches to the concrete type's override and writes
    /// its per-type content into the RichString.
    #[test]
    fn display_dispatches_to_concrete_impl() {
        let obj = Num { n: 42 };
        let mut out = empty_richstring();
        obj.display(&mut out);
        let got: String = out
            .chptr
            .iter()
            .take(out.chlen as usize)
            .map(|c| c.chars)
            .collect();
        assert_eq!(got, "42");
        assert_eq!(out.chlen, 2);
    }

    /// `compare` dispatches to the concrete override and orders like a C
    /// comparator (negative / zero / positive).
    #[test]
    fn compare_dispatches_and_orders() {
        let a = Num { n: 3 };
        let b = Num { n: 5 };
        assert!(a.compare(&b) < 0);
        assert!(b.compare(&a) > 0);
        assert_eq!(a.compare(&Num { n: 3 }), 0);
    }

    /// A class that leaves its display slot unset aborts on dispatch,
    /// modeling the C `assert(Object_getClass(obj)->display)`.
    #[test]
    #[should_panic(expected = "no display method")]
    fn unset_display_slot_panics() {
        let mut out = empty_richstring();
        Widget.display(&mut out);
    }

    /// Likewise for an unset compare slot.
    #[test]
    #[should_panic(expected = "no compare method")]
    fn unset_compare_slot_panics() {
        let _ = Widget.compare(&Widget);
    }
}
