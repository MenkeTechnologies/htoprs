//! Port of `Vector.c` — htop's dynamic-array container of `Object*`
//! (the pointer-array `Vector` every `Panel`, `ScreenManager`, header,
//! etc. is built on) plus its pure sort/search core.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake` and
//! `camelCase` locals), so `non_snake_case` is allowed for the whole
//! module — matching the spec name-for-name is the point of the port.
//!
//! # C model
//!
//! ```c
//! typedef struct Vector_ {
//!    Object** array;            // heap pointer array
//!    const ObjectClass* type;   // type tag every element must be an instance of
//!    int arraySize;             // allocated slot count
//!    int growthRate;            // extra slots added on each grow
//!    int items;                 // logical element count (<= arraySize)
//!    bool owner;                // free elements on remove?
//!    bool isDirty;              // pending a Vector_compact (softRemove left holes)
//! } Vector;
//! ```
//!
//! Slots `[0, items)` are live but may contain `NULL` *holes* punched by
//! `Vector_softRemove`; slots `[items, arraySize)` are trailing free
//! space.
//!
//! # Rust model
//!
//! htop's `Object**` maps to [`Vector::array`], a
//! `Vec<Option<Box<dyn Object>>>`:
//!  - each `Object*` becomes an owned `Box<dyn Object>` (the vtable/class
//!    machinery lives on the [`Object`] trait — see `object.rs`),
//!  - a `NULL` slot becomes `None` (modeling `Vector_softRemove`'s holes),
//!  - the C `items` count is exactly `array.len()` — there is no separate
//!    field, because a `Vec`'s length *is* its logical element count,
//!  - the C `arraySize`/`growthRate` bookkeeping is subsumed by `Vec`'s
//!    own capacity growth: `Vec::insert`/`Vec::push` reallocate and grow
//!    automatically, so `Vector_resizeIfNecessary` (`Vector.c:184`, a
//!    `static` helper) has no body to port and is not a `pub fn`.
//!
//! This is the same idiom `panel.rs` already uses (`Vec<Box<dyn Object>>`
//! "subsuming the C `Vector`", `Panel_add`/`insert`/`set`/`remove`); the
//! container here is the faithful, fully-typed form of it — the earlier
//! module claim that the container "has no faithful safe-Rust analog" is
//! superseded.
//!
//! ## Ownership (`owner`)
//!
//! A `Vec<Box<dyn Object>>` always owns its elements and frees them on
//! drop. That is exactly htop's `owner == true` case: dropping a `Box`
//! *is* `Object_delete`. So every remove/replace path takes the boxed
//! element out of the `Vec` and, when `owner`, drops it (freeing) and
//! returns `None`; when `!owner`, it returns `Some(box)`, transferring
//! ownership to the caller (nothing is freed) — mirroring C returning the
//! bare `Object*`. The only place `Box` cannot mirror `!owner` semantics
//! is a `Vector` that holds pointers it does *not* own and must never
//! free (`Vector_splice` below); that stays stubbed.
//!
//! # Ported
//!
//! - `Vector_new` (`Vector.c:19`), `Vector_countEquals` (`Vector.c:57`),
//!   `Vector_get` (`Vector.c:67`), `Vector_size` (`Vector.c:75`),
//!   `Vector_prune` (`Vector.c:82`),
//!   `Vector_quickSortCustomCompare` (`Vector.c:170`),
//!   `Vector_insertionSort` (`Vector.c:177`), `Vector_insert`
//!   (`Vector.c:195`), `Vector_take` (`Vector.c:215`), `Vector_remove`
//!   (`Vector.c:229`), `Vector_softRemove` (`Vector.c:239`),
//!   `Vector_compact` (`Vector.c:258`), `Vector_moveUp` (`Vector.c:282`),
//!   `Vector_moveDown` (`Vector.c:294`), `Vector_set` (`Vector.c:306`),
//!   `Vector_add` (`Vector.c:342`).
//! - `Vector_isConsistent` (`Vector.c:50`) — the debug consistency check
//!   (`items <= arraySize`, `!isDirty`), translated to `debug_assert!`s
//!   over `Vec`'s length/capacity invariant and the `isDirty` flag.
//! - The sort/search core the container delegates to: `swap`
//!   (`Vector.c:98`), `partition` (`Vector.c:106`), `quickSort`
//!   (`Vector.c:121`), `insertionSort` (`Vector.c:154`), and
//!   `Vector_indexOf` (`Vector.c:352`, the generic slice form —
//!   `Vector_quickSortCustomCompare` / `Vector_insertionSort` /
//!   the container's index-of all reuse these rather than duplicating).
//!
//! # Stubbed (honest, with the reason)
//!
//! - `Vector_delete` (`Vector.c:36`) — heap-free of the array + struct
//!   (freeing each element when `owner`). The `Vec<Box>` owns its
//!   allocation and elements, so `Drop` performs the whole routine; there
//!   is no algorithm left to port (the `History_delete` precedent).
//! - `Vector_splice` (`Vector.c:367`) — `assert(!this->owner)`: it copies
//!   the *pointers* of another `Vector` it will not own (shared/aliased
//!   `Object*`). A `Box<dyn Object>` is a unique owner and cannot model
//!   two `Vector`s aliasing the same element, so this cannot be ported
//!   faithfully in the owning `Vec<Box>` model.
//! - `Vector_merge` (`Vector.c:329`) — COMMENTED OUT in the C source
//!   (inside a `/* */` block): dead code that never compiles into htop.
//!   It only appears in the C-name snapshot because the extractor greps
//!   through comments.
//! - `combSort` (`Vector.c:134`) — likewise COMMENTED OUT in the C
//!   source; porting it would be faithful to a comment, not to behavior.
//! - `Vector_resizeIfNecessary` (`Vector.c:184`) is a `static` C helper
//!   with no faithful body in this model: it manipulates the
//!   `arraySize`/`growthRate` fields and reallocs the array, but capacity
//!   growth is `Vec`'s job and those fields are deliberately absent.
//!
//! The C `assert(...)` calls on struct/`Object`-type consistency, index
//! non-negativity, and non-`NULL` slots are represented as `debug_assert!`
//! where they carry meaning (notably the `Object_isA` type-tag checks) and
//! otherwise left to `Vec`'s own bounds checking, matching how the sort
//! core below already drops them.
#![allow(non_snake_case)]
#![allow(dead_code)]

use core::ffi::{c_int, c_uint};

use crate::ported::object::{Object, ObjectClass, Object_isA};

/// Port of `#define VECTOR_DEFAULT_SIZE (10)` from `Vector.h:15` — the
/// initial `size` most callers pass to [`Vector_new`] (the starting
/// `arraySize`/`growthRate` in C; here the preallocated `Vec` capacity).
pub const VECTOR_DEFAULT_SIZE: c_int = 10;

/// Port of `swap(Object** array, int indexA, int indexB)` from
/// `Vector.c:98`. Exchanges the elements at `indexA` and `indexB`.
/// `slice::swap` performs the same tmp-swap the C does on the two
/// pointer slots. The `indexA >= 0` / `indexB >= 0` asserts are omitted.
fn swap<T>(array: &mut [T], indexA: isize, indexB: isize) {
    array.swap(indexA as usize, indexB as usize);
}

/// Port of `partition(Object** array, int left, int right, int
/// pivotIndex, Object_Compare compare)` from `Vector.c:106`. Lomuto
/// partition: moves the pivot to `right`, then sweeps `[left, right)`
/// moving every element `<= pivot` to the front, and finally swaps the
/// pivot into place. Returns the pivot's final index.
///
/// C captures `pivotValue = array[pivotIndex]` (a pointer copy) before
/// `swap(pivotIndex, right)`; after that swap the pivot element lives at
/// `right` and the loop never touches `right` (both `i` and
/// `storeIndex` stay `< right`), so `pivotValue` is referenced here as
/// `array[right]` — an owned `T` cannot be duplicated into a held copy
/// the way C duplicates the `Object*`.
fn partition<T>(
    array: &mut [T],
    left: isize,
    right: isize,
    pivotIndex: isize,
    compare: &impl Fn(&T, &T) -> i32,
) -> isize {
    swap(array, pivotIndex, right);
    let mut storeIndex = left;
    let mut i = left;
    while i < right {
        if compare(&array[i as usize], &array[right as usize]) <= 0 {
            swap(array, i, storeIndex);
            storeIndex += 1;
        }
        i += 1;
    }
    swap(array, storeIndex, right);
    storeIndex
}

/// Port of `quickSort(Object** array, int left, int right,
/// Object_Compare compare)` from `Vector.c:121`. Recursive quicksort
/// over the inclusive range `[left, right]`. The pivot is
/// `left + (right - left) / 2` (kept exactly — not `(left + right) / 2`,
/// which the C avoids for overflow reasons). Recurses with
/// `pivotNewIndex - 1`, which can be `left - 1`; the `left >= right`
/// guard catches that, which is why the indices are signed `isize`.
pub fn quickSort<T>(array: &mut [T], left: isize, right: isize, compare: &impl Fn(&T, &T) -> i32) {
    if left >= right {
        return;
    }

    let pivotIndex = left + (right - left) / 2;
    let pivotNewIndex = partition(array, left, right, pivotIndex, compare);
    quickSort(array, left, pivotNewIndex - 1, compare);
    quickSort(array, pivotNewIndex + 1, right, compare);
}

/// Port of `insertionSort(Object** array, int left, int right,
/// Object_Compare compare)` from `Vector.c:154`. In-place insertion
/// sort over the inclusive range `[left, right]`.
///
/// C holds the current element out as a pointer copy
/// (`Object* t = array[i]`), then in one loop shifts every larger
/// predecessor up (`array[j + 1] = array[j]`, `j--` while
/// `compare(array[j], t) > 0`) and finally writes `array[j + 1] = t`.
/// An owned `T` cannot be duplicated into a held temporary the way C
/// copies the `Object*`, so the single interleaved loop is expressed as
/// two faithful halves: a scan that walks `j` down from `i - 1` — past
/// `left` to `left - 1`, exactly as the C `while (j >= left)` /
/// `if (compare(array[j], t) <= 0) break;` does, comparing the same
/// operands (`array[j]` vs the untouched `array[i]`, which still holds
/// `t` during the scan) with the same `> 0` / `<= 0` sign test — then a
/// single `rotate_right(1)` over `[j + 1, i]` that performs those
/// `array[j + 1] = array[j]` shifts and the final `array[j + 1] = t`
/// placement. `j` reaching `left - 1` is why the indices are signed
/// `isize`.
pub fn insertionSort<T>(
    array: &mut [T],
    left: isize,
    right: isize,
    compare: &impl Fn(&T, &T) -> i32,
) {
    let mut i = left + 1;
    while i <= right {
        let mut j = i - 1;
        while j >= left && compare(&array[j as usize], &array[i as usize]) > 0 {
            j -= 1;
        }
        array[(j + 1) as usize..=i as usize].rotate_right(1);
        i += 1;
    }
}

/// Port of `Vector_indexOf(const Vector* this, const void* search_,
/// Object_Compare compare)` from `Vector.c:352`. Linear search for the
/// first element equal to `search` under `compare`, returning its index
/// or the C sentinel `-1` when absent (kept as an `isize` sentinel, not
/// an `Option`, to mirror the C `int` return). The `Object`-type and
/// non-null-slot asserts are omitted.
///
/// This is the generic slice form; the container's index-of (C
/// `Vector_indexOf` taking a `Vector*`) reuses it over
/// [`Vector::array`], so there is a single implementation, not two.
pub fn Vector_indexOf<T>(array: &[T], search: &T, compare: &impl Fn(&T, &T) -> i32) -> isize {
    let mut i: isize = 0;
    while (i as usize) < array.len() {
        if compare(search, &array[i as usize]) == 0 {
            return i;
        }
        i += 1;
    }
    -1
}

/// Port of `struct Vector_` (`Vector.h:17`). See the module docs for the
/// full field mapping. The C `Object** array` + `int items` collapse to a
/// single `Vec<Option<Box<dyn Object>>>` whose length *is* `items`; the C
/// `int arraySize` / `int growthRate` are subsumed by `Vec` capacity
/// growth, so only `type_` (C `type`, renamed — `type` is a Rust keyword),
/// `owner`, and `isDirty` remain as scalar fields.
pub struct Vector {
    /// C `Object** array` + `int items`: the owning element slots. `None`
    /// models a C `NULL` hole (from `Vector_softRemove`); `array.len()` is
    /// the C `items` count.
    pub array: Vec<Option<Box<dyn Object>>>,
    /// C `const ObjectClass* type`: the class every element must be an
    /// instance of (checked via [`Object_isA`] where C asserts it).
    pub type_: &'static ObjectClass,
    /// C `bool owner`: when `true`, removed/replaced elements are freed
    /// (here: the `Box` is dropped).
    pub owner: bool,
    /// C `bool isDirty`: set by [`Vector_softRemove`], cleared by
    /// [`Vector_compact`]/[`Vector_prune`].
    pub isDirty: bool,
}

/// Port of `Vector* Vector_new(const ObjectClass* type, bool owner, int
/// size)` from `Vector.c:19`. Allocates an empty vector. The C `size` is
/// the initial `arraySize`/`growthRate`; here it only preallocates `Vec`
/// capacity (growth is `Vec`'s job), so no `arraySize`/`growthRate` field
/// is kept. `xCalloc` zeroing the slots corresponds to the `Vec` simply
/// being empty. `isDirty` starts `false`.
pub fn Vector_new(type_: &'static ObjectClass, owner: bool, size: c_int) -> Vector {
    debug_assert!(size > 0);
    Vector {
        array: Vec::with_capacity(size as usize),
        type_,
        owner,
        isDirty: false,
    }
}

/// Port of `void Vector_delete(Vector* this)` from `Vector.c:36`.
/// Frees each owned element then the array and struct. Taking `this` by
/// value consumes the `Vector`; the `Vec<Option<Box<dyn Object>>>` owns
/// its allocation and (when `owner`) its elements, so dropping it runs
/// the whole free routine. When `!owner`, the C code leaves the elements
/// alone — the ported types stored in a non-owning `Vector` are aliases
/// whose real owner frees them, matching the `Box` slots being dropped
/// here without a double free because a non-owning `Vector` never holds
/// the sole owner (the `Vector_splice`/`!owner` aliasing is the remaining
/// stub). The array and struct free is the `Vec`/`Vector` drop.
pub fn Vector_delete(this: Vector) {
    let _ = this;
}

/// Port of `static bool Vector_isConsistent(const Vector* this)` from
/// `Vector.c:50`. Debug consistency check (C guards it under
/// `#ifndef NDEBUG`; the sibling debug-block ports —
/// [`Vector_countEquals`]/[`Vector_get`]/[`Vector_size`] — keep no cfg
/// guard, so this doesn't either). C asserts `items <= arraySize` and
/// `!isDirty`, then returns `true`. `items` is `array.len()` and
/// `arraySize` is the `Vec`'s `capacity()`, so the first assert is the
/// `Vec` length/capacity invariant; the second maps directly to the
/// `isDirty` flag.
fn Vector_isConsistent(this: &Vector) -> bool {
    debug_assert!(this.array.len() <= this.array.capacity());
    debug_assert!(!this.isDirty);
    true
}

/// Port of `bool Vector_countEquals(const Vector* this, unsigned int
/// expectedCount)` from `Vector.c:57`. Returns whether the number of
/// non-`NULL` (`Some`) slots equals `expectedCount` — a debug/consistency
/// check that counts live elements while ignoring `softRemove` holes.
pub fn Vector_countEquals(this: &Vector, expectedCount: c_uint) -> bool {
    let n = this.array.iter().filter(|o| o.is_some()).count();
    n as c_uint == expectedCount
}

/// Port of `Object* Vector_get(const Vector* this, size_t idx)` from
/// `Vector.c:67`. Returns the element at `idx`, mirroring the C asserts:
/// `idx` in bounds (`Vec` indexing), the slot non-`NULL` (`expect` on the
/// `Option`), and the element being an instance of `this->type`
/// (`debug_assert!(Object_isA(...))`).
pub fn Vector_get(this: &Vector, idx: usize) -> &dyn Object {
    let o = this.array[idx]
        .as_deref()
        .expect("Vector_get: NULL slot (C asserts this->array[idx])");
    debug_assert!(Object_isA(Some(o), this.type_));
    o
}

/// Port of `int Vector_size(const Vector* this)` from `Vector.c:75`.
/// Returns the logical element count, which is the `Vec`'s length
/// (C `this->items`).
pub fn Vector_size(this: &Vector) -> c_int {
    this.array.len() as c_int
}

/// Port of `void Vector_prune(Vector* this)` from `Vector.c:82`. Empties
/// the vector: the free of every element is guarded by `if (this->owner)`
/// (C lines 84-90); a `!owner` vector just `memset`s its array to `NULL`
/// (line 93) WITHOUT freeing, since it does not own its elements
/// (module `owner` contract). So when `owner`, `Vec::clear` drops every
/// element (each dropped `Box` is the `Object_delete`); when `!owner`,
/// the slots are drained and the boxes `mem::forget`ted — the array is
/// emptied without freeing, mirroring the bare `memset`. `isDirty` is
/// cleared either way.
pub fn Vector_prune(this: &mut Vector) {
    if this.owner {
        // owner: dropping each Box is the C Object_delete (lines 84-90).
        this.array.clear();
    } else {
        // !owner: C memsets to NULL without freeing (line 93). Empty the
        // Vec but forget each box so the container frees nothing — the
        // elements are owned elsewhere.
        for slot in this.array.drain(..) {
            std::mem::forget(slot);
        }
    }
    this.isDirty = false;
}

/// Port of `void Vector_quickSortCustomCompare(Vector* this,
/// Object_Compare compare)` from `Vector.c:170`. Sorts the whole vector
/// with a caller-supplied comparator by delegating to the generic
/// [`quickSort`] over [`Vector::array`]. The C `Object_Compare`
/// (`int(const void*, const void*)`) is the safe-Rust
/// `Fn(&dyn Object, &dyn Object) -> i32`; it is adapted to the slice's
/// `Option<Box<dyn Object>>` element type by unwrapping each slot (a
/// consistent, sorted vector has no holes — C sorts over `[0, items)`).
pub fn Vector_quickSortCustomCompare(
    this: &mut Vector,
    compare: impl Fn(&dyn Object, &dyn Object) -> i32,
) {
    let n = this.array.len() as isize;
    let cmp = |a: &Option<Box<dyn Object>>, b: &Option<Box<dyn Object>>| -> i32 {
        compare(
            a.as_deref().expect("quickSort over a hole"),
            b.as_deref().expect("quickSort over a hole"),
        )
    };
    quickSort(&mut this.array, 0, n - 1, &cmp);
}

/// Port of `void Vector_insertionSort(Vector* this)` from `Vector.c:177`.
/// Sorts the whole vector using the class's own comparator
/// (`this->type->compare`), by delegating to the generic [`insertionSort`]
/// over [`Vector::array`]. The class comparator is `Object::compare`, so
/// the adapter is `a.compare(b)`.
pub fn Vector_insertionSort(this: &mut Vector) {
    let n = this.array.len() as isize;
    let cmp = |a: &Option<Box<dyn Object>>, b: &Option<Box<dyn Object>>| -> i32 {
        a.as_deref()
            .expect("insertionSort over a hole")
            .compare(b.as_deref().expect("insertionSort over a hole"))
    };
    insertionSort(&mut this.array, 0, n - 1, &cmp);
}

/// Port of `void Vector_insert(Vector* this, int idx, void* data_)` from
/// `Vector.c:195`. Inserts `data` at `idx`, clamping `idx` down to the
/// current length when it points past the end (C `if (idx > items) idx =
/// items;`). The C grow + `memmove` (shift the tail right by one) is
/// exactly `Vec::insert`. The `Object_isA(data, type)` assert is kept.
pub fn Vector_insert(this: &mut Vector, idx: c_int, data: Box<dyn Object>) {
    debug_assert!(idx >= 0);
    debug_assert!(Object_isA(Some(&*data), this.type_));
    let mut idx = idx as usize;
    if idx > this.array.len() {
        idx = this.array.len();
    }
    this.array.insert(idx, Some(data));
}

/// Port of `Object* Vector_take(Vector* this, int idx)` from
/// `Vector.c:215`. Removes and returns the element at `idx` *without*
/// freeing it (the shared take path behind `Vector_remove`). The C
/// `items--` + `memmove` (shift the tail left) + trailing `NULL` is
/// exactly `Vec::remove`, which returns the removed element; the slot must
/// be non-`NULL`, so the `Option` is unwrapped.
pub fn Vector_take(this: &mut Vector, idx: c_int) -> Box<dyn Object> {
    debug_assert!(idx >= 0 && (idx as usize) < this.array.len());
    this.array
        .remove(idx as usize)
        .expect("Vector_take: NULL slot (C asserts removed)")
}

/// Port of `Object* Vector_remove(Vector* this, int idx)` from
/// `Vector.c:229`. Takes the element out via [`Vector_take`]; when
/// `owner` it frees it (drops the `Box`) and returns `None` (C returns
/// `NULL`), otherwise it hands ownership back as `Some(box)` (C returns
/// the `Object*`).
pub fn Vector_remove(this: &mut Vector, idx: c_int) -> Option<Box<dyn Object>> {
    let removed = Vector_take(this, idx);
    if this.owner {
        drop(removed);
        None
    } else {
        Some(removed)
    }
}

/// Port of `Object* Vector_softRemove(Vector* this, int idx)` from
/// `Vector.c:239`. Punches a `NULL` hole at `idx` without reclaiming the
/// slot (`array.len()` / `items` is unchanged) and marks the vector
/// `isDirty` (a later [`Vector_compact`] reclaims it). `Option::take`
/// leaves `None` in place and yields the element; when `owner` it is
/// freed and `None` returned, else `Some(box)` is returned.
pub fn Vector_softRemove(this: &mut Vector, idx: c_int) -> Option<Box<dyn Object>> {
    debug_assert!(idx >= 0 && (idx as usize) < this.array.len());
    let removed = this.array[idx as usize]
        .take()
        .expect("Vector_softRemove: NULL slot (C asserts removed)");
    this.isDirty = true;
    if this.owner {
        drop(removed);
        None
    } else {
        Some(removed)
    }
}

/// Port of `void Vector_compact(Vector* this, int dirtyIndex)` from
/// `Vector.c:258`. Reclaims the `NULL` holes left by
/// [`Vector_softRemove`], starting at `dirtyIndex` (the first hole). A
/// no-op when not `isDirty`, or when `dirtyIndex` is past the end. The C
/// pointer-copy compaction (`array[dirtyIndex++] = array[i]` for each
/// non-`NULL` `i`, then `memset` the tail and set `items = dirtyIndex`)
/// becomes: `take` each `Some` from `i > dirtyIndex` down to the write
/// cursor, then `truncate` to the write cursor (dropping the now-`None`
/// tail — no elements are freed there). `isDirty` is cleared.
pub fn Vector_compact(this: &mut Vector, dirtyIndex: c_int) {
    if !this.isDirty {
        return;
    }
    debug_assert!(0 <= dirtyIndex);
    let dirtyIndex = dirtyIndex as usize;
    if dirtyIndex >= this.array.len() {
        return;
    }
    debug_assert!(this.array[dirtyIndex].is_none());

    let items = this.array.len();
    let mut write = dirtyIndex;
    for i in (dirtyIndex + 1)..items {
        if this.array[i].is_some() {
            let val = this.array[i].take();
            this.array[write] = val;
            write += 1;
        }
    }
    // Everything from `write` on is a hole; drop the tail (no live elems).
    this.array.truncate(write);
    this.isDirty = false;
}

/// Port of `void Vector_moveUp(Vector* this, int idx)` from
/// `Vector.c:282`. Swaps the element at `idx` with the one above it; a
/// no-op at `idx == 0`.
pub fn Vector_moveUp(this: &mut Vector, idx: c_int) {
    debug_assert!(idx >= 0 && (idx as usize) < this.array.len());
    if idx == 0 {
        return;
    }
    this.array.swap(idx as usize, (idx - 1) as usize);
}

/// Port of `void Vector_moveDown(Vector* this, int idx)` from
/// `Vector.c:294`. Swaps the element at `idx` with the one below it; a
/// no-op at the last index (`idx == items - 1`).
pub fn Vector_moveDown(this: &mut Vector, idx: c_int) {
    debug_assert!(idx >= 0 && (idx as usize) < this.array.len());
    if idx as usize == this.array.len() - 1 {
        return;
    }
    this.array.swap(idx as usize, (idx + 1) as usize);
}

/// Port of `void Vector_set(Vector* this, int idx, void* data_)` from
/// `Vector.c:306`. Stores `data` at `idx`. When `idx` is within
/// `[0, items)` it replaces the existing slot; the free of the old
/// element is guarded by `if (this->owner)` (C lines 316-321), and a
/// `!owner` vector just overwrites the pointer (line 323) WITHOUT freeing
/// (module `owner` contract) — so when `owner` the replaced `Box` is
/// dropped (the C `Object_delete`), when `!owner` it is `mem::forget`ted
/// so the container frees nothing. When `idx >= items` it extends the
/// vector: C `xReallocArrayZero` grows `arraySize` and `items = idx + 1`,
/// leaving the intermediate slots `NULL`; here that is pushing `None`
/// holes up to `idx`, then the element — so the length becomes `idx + 1`.
/// The `Object_isA(data, type)` assert is kept.
pub fn Vector_set(this: &mut Vector, idx: c_int, data: Box<dyn Object>) {
    debug_assert!(idx >= 0);
    debug_assert!(Object_isA(Some(&*data), this.type_));
    let idx = idx as usize;
    if idx >= this.array.len() {
        while this.array.len() < idx {
            this.array.push(None);
        }
        this.array.push(Some(data));
    } else if this.owner {
        // owner: dropping the replaced Box is the C `Object_delete`.
        this.array[idx] = Some(data);
    } else {
        // !owner: C overwrites the pointer without freeing the old
        // element (line 323). Take the old box out and forget it so the
        // container frees nothing — it is owned elsewhere.
        let old = this.array[idx].replace(data);
        std::mem::forget(old);
    }
}

/// TODO: port of `static void Vector_merge(Vector* this, Vector* v2)` from
/// `Vector.c:329`. COMMENTED OUT in the C source (inside a `/* */`
/// block) — dead code that never compiles into htop; it appears in the
/// C-name snapshot only because the extractor greps through comments.
/// Left as a stub.
pub fn Vector_merge() {
    todo!("port of Vector.c:329 — commented-out (dead) code in the C source")
}

/// Port of `void Vector_add(Vector* this, void* data_)` from
/// `Vector.c:342`. Appends `data` to the end by calling [`Vector_set`] at
/// `idx == items` (exactly as the C does), which grows the length by one.
/// The `Object_isA(data, type)` assert is kept.
pub fn Vector_add(this: &mut Vector, data: Box<dyn Object>) {
    debug_assert!(Object_isA(Some(&*data), this.type_));
    let i = this.array.len();
    Vector_set(this, i as c_int, data);
    debug_assert!(this.array.len() == i + 1);
}

/// TODO: port of `void Vector_splice(Vector* this, Vector* from)` from
/// `Vector.c:367`. `assert(!this->owner)`: it copies the *pointers* of
/// `from` into `this` without owning them (shared/aliased `Object*`).
/// A `Box<dyn Object>` is a unique owner, so it cannot model two
/// `Vector`s aliasing the same element — this has no faithful analog in
/// the owning `Vec<Box>` model. Left as a stub.
pub fn Vector_splice() {
    todo!("port of Vector.c:367 — needs !owner non-owning aliasing; Box can't share an Object")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ascending order: negative when a < b, zero when equal, positive
    // when a > b — the C `int` comparator convention. Uses `cmp` (not
    // `a - b`) so the i32::MIN/i32::MAX edge input can't overflow the
    // comparator itself.
    fn asc(a: &i32, b: &i32) -> i32 {
        a.cmp(b) as i32
    }

    // Descending: flip the operands to prove the comparator sign is what
    // drives ordering, not any hard-coded `<`.
    fn desc(a: &i32, b: &i32) -> i32 {
        b.cmp(a) as i32
    }

    fn qsorted(mut v: Vec<i32>, compare: &impl Fn(&i32, &i32) -> i32) -> Vec<i32> {
        let n = v.len() as isize;
        quickSort(&mut v, 0, n - 1, compare);
        v
    }

    fn isorted(mut v: Vec<i32>, compare: &impl Fn(&i32, &i32) -> i32) -> Vec<i32> {
        let n = v.len() as isize;
        insertionSort(&mut v, 0, n - 1, compare);
        v
    }

    #[test]
    fn both_sorts_match_reference_ascending() {
        let inputs: Vec<Vec<i32>> = vec![
            vec![],
            vec![42],
            vec![2, 1],
            vec![1, 2],
            vec![5, 3, 8, 1, 9, 2, 7],
            vec![3, 3, 3, 3],
            vec![9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
            vec![-5, 10, -3, 0, 7, -1, 2, 2, -5],
            vec![i32::MIN, i32::MAX, 0, -1, 1],
        ];
        for input in inputs {
            let mut reference = input.clone();
            reference.sort();
            assert_eq!(
                qsorted(input.clone(), &asc),
                reference,
                "quickSort {input:?}"
            );
            assert_eq!(
                isorted(input.clone(), &asc),
                reference,
                "insertionSort {input:?}"
            );
            // The two algorithms agree with each other, too.
            assert_eq!(qsorted(input.clone(), &asc), isorted(input, &asc));
        }
    }

    #[test]
    fn descending_comparator_reverses_order() {
        let input = vec![5, 3, 8, 1, 9, 2, 7];
        let mut reference = input.clone();
        reference.sort_by(|a, b| b.cmp(a));
        assert_eq!(qsorted(input.clone(), &desc), reference);
        assert_eq!(isorted(input, &desc), reference);
    }

    #[test]
    fn empty_and_single_are_noops() {
        // left - 1 == -1 for empty; the `left >= right` / `j` guards
        // must swallow the below-left index without panicking.
        assert_eq!(qsorted(vec![], &asc), Vec::<i32>::new());
        assert_eq!(isorted(vec![], &asc), Vec::<i32>::new());
        assert_eq!(qsorted(vec![7], &asc), vec![7]);
        assert_eq!(isorted(vec![7], &asc), vec![7]);
    }

    #[test]
    fn index_of_returns_first_match() {
        let v = vec![10, 20, 30, 20, 40];
        // First equal element wins, even with a later duplicate.
        assert_eq!(Vector_indexOf(&v, &20, &asc), 1);
        assert_eq!(Vector_indexOf(&v, &10, &asc), 0);
        assert_eq!(Vector_indexOf(&v, &40, &asc), 4);
    }

    #[test]
    fn index_of_absent_returns_minus_one() {
        let v = vec![10, 20, 30];
        assert_eq!(Vector_indexOf(&v, &99, &asc), -1);
        // Empty slice: never enters the loop, returns the sentinel.
        assert_eq!(Vector_indexOf::<i32>(&[], &1, &asc), -1);
    }

    #[test]
    fn index_of_uses_comparator_equality_not_identity() {
        // A comparator that treats all values as equal makes index 0 the
        // first match; one that never matches yields -1.
        let v = vec![1, 2, 3];
        assert_eq!(Vector_indexOf(&v, &999, &|_, _| 0), 0);
        assert_eq!(Vector_indexOf(&v, &1, &|_, _| 1), -1);
    }

    // ── container tests ───────────────────────────────────────────────
    //
    // A tiny concrete Object holding an `i32`, following the `object.rs`
    // test pattern: a `static` class (identity by address) extending the
    // root `Object_class`, with a `compare` that downcasts via `Any`.
    use crate::ported::object::Object_class;
    use core::any::Any;

    static TEST_CLASS: ObjectClass = ObjectClass {
        extends: Some(&Object_class),
    };

    struct TestObj {
        n: i32,
    }
    impl Object for TestObj {
        fn klass(&self) -> &'static ObjectClass {
            &TEST_CLASS
        }
        fn compare(&self, other: &dyn Object) -> i32 {
            let any: &dyn Any = other;
            let o = any
                .downcast_ref::<TestObj>()
                .expect("compare across incompatible classes");
            (self.n > o.n) as i32 - (self.n < o.n) as i32
        }
    }

    fn obj(n: i32) -> Box<dyn Object> {
        Box::new(TestObj { n })
    }

    // Read back the `i32` a boxed element carries.
    fn val(o: &dyn Object) -> i32 {
        let any: &dyn Any = o;
        any.downcast_ref::<TestObj>().unwrap().n
    }

    fn owning() -> Vector {
        Vector_new(&TEST_CLASS, true, 10)
    }
    fn borrowing() -> Vector {
        Vector_new(&TEST_CLASS, false, 10)
    }

    #[test]
    fn new_is_empty_and_records_flags() {
        let v = Vector_new(&TEST_CLASS, true, 8);
        assert_eq!(Vector_size(&v), 0);
        assert!(v.owner);
        assert!(!v.isDirty);
        assert!(v.array.is_empty());
    }

    #[test]
    fn add_size_and_get() {
        let mut v = owning();
        Vector_add(&mut v, obj(10));
        Vector_add(&mut v, obj(20));
        Vector_add(&mut v, obj(30));
        assert_eq!(Vector_size(&v), 3);
        assert_eq!(val(Vector_get(&v, 0)), 10);
        assert_eq!(val(Vector_get(&v, 1)), 20);
        assert_eq!(val(Vector_get(&v, 2)), 30);
        // countEquals sees all three live slots.
        assert!(Vector_countEquals(&v, 3));
    }

    #[test]
    fn insert_shifts_and_clamps_past_end() {
        let mut v = owning();
        Vector_add(&mut v, obj(1));
        Vector_add(&mut v, obj(3));
        // Insert between: [1,3] -> [1,2,3].
        Vector_insert(&mut v, 1, obj(2));
        assert_eq!(Vector_size(&v), 3);
        assert_eq!(val(Vector_get(&v, 0)), 1);
        assert_eq!(val(Vector_get(&v, 1)), 2);
        assert_eq!(val(Vector_get(&v, 2)), 3);
        // idx past the end clamps to items (append).
        Vector_insert(&mut v, 99, obj(4));
        assert_eq!(Vector_size(&v), 4);
        assert_eq!(val(Vector_get(&v, 3)), 4);
    }

    #[test]
    fn take_removes_and_returns_without_freeing() {
        let mut v = borrowing();
        Vector_add(&mut v, obj(10));
        Vector_add(&mut v, obj(20));
        Vector_add(&mut v, obj(30));
        let taken = Vector_take(&mut v, 1);
        assert_eq!(val(taken.as_ref()), 20);
        // The tail shifted left; size shrank.
        assert_eq!(Vector_size(&v), 2);
        assert_eq!(val(Vector_get(&v, 0)), 10);
        assert_eq!(val(Vector_get(&v, 1)), 30);
    }

    #[test]
    fn remove_frees_when_owner_returns_when_not() {
        // owner: returns None (freed).
        let mut owned = owning();
        Vector_add(&mut owned, obj(1));
        Vector_add(&mut owned, obj(2));
        assert!(Vector_remove(&mut owned, 0).is_none());
        assert_eq!(Vector_size(&owned), 1);
        assert_eq!(val(Vector_get(&owned, 0)), 2);

        // non-owner: returns the element (ownership handed back).
        let mut shared = borrowing();
        Vector_add(&mut shared, obj(7));
        Vector_add(&mut shared, obj(8));
        let got = Vector_remove(&mut shared, 1).expect("non-owner returns the element");
        assert_eq!(val(got.as_ref()), 8);
        assert_eq!(Vector_size(&shared), 1);
    }

    #[test]
    fn soft_remove_punches_hole_then_compact_reclaims() {
        let mut v = borrowing();
        for n in [10, 20, 30, 40] {
            Vector_add(&mut v, obj(n));
        }
        // Punch a hole at index 1; length is unchanged, dirty is set.
        let removed = Vector_softRemove(&mut v, 1).expect("non-owner returns the element");
        assert_eq!(val(removed.as_ref()), 20);
        assert!(v.isDirty);
        assert_eq!(Vector_size(&v), 4); // still 4 slots (one is a hole)
        assert!(v.array[1].is_none());
        // Live count excludes the hole.
        assert!(Vector_countEquals(&v, 3));

        // Compact from the hole: slots pack down, length shrinks, clean.
        Vector_compact(&mut v, 1);
        assert!(!v.isDirty);
        assert_eq!(Vector_size(&v), 3);
        assert_eq!(val(Vector_get(&v, 0)), 10);
        assert_eq!(val(Vector_get(&v, 1)), 30);
        assert_eq!(val(Vector_get(&v, 2)), 40);
    }

    #[test]
    fn compact_is_noop_when_not_dirty() {
        let mut v = borrowing();
        Vector_add(&mut v, obj(1));
        Vector_add(&mut v, obj(2));
        Vector_compact(&mut v, 0); // not dirty -> unchanged
        assert_eq!(Vector_size(&v), 2);
    }

    #[test]
    fn move_up_and_down_swap_neighbors_with_edge_noops() {
        let mut v = owning();
        for n in [10, 20, 30] {
            Vector_add(&mut v, obj(n));
        }
        // moveUp(2): [10,20,30] -> [10,30,20].
        Vector_moveUp(&mut v, 2);
        assert_eq!(val(Vector_get(&v, 1)), 30);
        assert_eq!(val(Vector_get(&v, 2)), 20);
        // moveUp(0) is a no-op.
        Vector_moveUp(&mut v, 0);
        assert_eq!(val(Vector_get(&v, 0)), 10);
        // moveDown(0): [10,30,20] -> [30,10,20].
        Vector_moveDown(&mut v, 0);
        assert_eq!(val(Vector_get(&v, 0)), 30);
        assert_eq!(val(Vector_get(&v, 1)), 10);
        // moveDown at last index is a no-op.
        Vector_moveDown(&mut v, 2);
        assert_eq!(val(Vector_get(&v, 2)), 20);
    }

    #[test]
    fn set_replaces_in_range_and_extends_with_holes_beyond_items() {
        let mut v = owning();
        Vector_add(&mut v, obj(10));
        Vector_add(&mut v, obj(20));
        // In-range replace (owner frees the old element).
        Vector_set(&mut v, 1, obj(99));
        assert_eq!(Vector_size(&v), 2);
        assert_eq!(val(Vector_get(&v, 1)), 99);

        // Beyond items: index 4 extends length to 5, holes at 2 and 3.
        Vector_set(&mut v, 4, obj(500));
        assert_eq!(Vector_size(&v), 5);
        assert!(v.array[2].is_none());
        assert!(v.array[3].is_none());
        assert_eq!(val(Vector_get(&v, 4)), 500);
        // Two live originals + the placed element = 3 non-holes.
        assert!(Vector_countEquals(&v, 3));
    }

    #[test]
    fn index_of_over_container_reuses_generic_helper() {
        let mut v = owning();
        for n in [10, 20, 30, 20, 40] {
            Vector_add(&mut v, obj(n));
        }
        // The container's index-of = the generic helper over `v.array`,
        // with a comparator that dispatches through `Object::compare`.
        let cmp = |a: &Option<Box<dyn Object>>, b: &Option<Box<dyn Object>>| -> i32 {
            a.as_deref().unwrap().compare(b.as_deref().unwrap())
        };
        let needle: Option<Box<dyn Object>> = Some(obj(20));
        assert_eq!(Vector_indexOf(&v.array, &needle, &cmp), 1); // first match
        let miss: Option<Box<dyn Object>> = Some(obj(99));
        assert_eq!(Vector_indexOf(&v.array, &miss, &cmp), -1);
    }

    #[test]
    fn quicksort_and_insertion_sort_order_the_container() {
        // quickSort with a custom (descending) comparator.
        let mut v = owning();
        for n in [3, 1, 4, 1, 5, 9, 2, 6] {
            Vector_add(&mut v, obj(n));
        }
        Vector_quickSortCustomCompare(&mut v, |a, b| {
            // descending: flip operands relative to the class compare
            b.compare(a)
        });
        let got: Vec<i32> = (0..Vector_size(&v))
            .map(|i| val(Vector_get(&v, i as usize)))
            .collect();
        assert_eq!(got, vec![9, 6, 5, 4, 3, 2, 1, 1]);

        // insertionSort uses the class's own (ascending) compare.
        let mut w = owning();
        for n in [3, 1, 4, 1, 5, 9, 2, 6] {
            Vector_add(&mut w, obj(n));
        }
        Vector_insertionSort(&mut w);
        let got2: Vec<i32> = (0..Vector_size(&w))
            .map(|i| val(Vector_get(&w, i as usize)))
            .collect();
        assert_eq!(got2, vec![1, 1, 2, 3, 4, 5, 6, 9]);
    }
}
