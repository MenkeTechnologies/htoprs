//! Port of `Vector.c` — only the pure array algorithms (the sort and
//! search core).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake` and
//! `camelCase` locals), so `non_snake_case` is allowed for the whole
//! module — matching the spec name-for-name is the point of the port.
//!
//! htop's `Vector` is a heap-allocated `Object**` with an owner flag, a
//! type tag, and a growth policy. The C sort/search helpers operate on
//! that pointer array through a comparator
//! `Object_Compare = int (*)(const void*, const void*)`. They port
//! faithfully to generics over a slice `&mut [T]` / `&[T]` with a
//! comparator `compare: &impl Fn(&T, &T) -> i32`: the pointer array
//! becomes a slice of elements/handles, and swapping two elements
//! mirrors swapping two pointers. The comparator returns C `int`, so
//! the `<= 0` / `== 0` / `> 0` sign tests are preserved verbatim.
//!
//! All C index arithmetic uses signed `int` (`left`, `right`,
//! `pivotIndex`, `storeIndex`, `i`, `j`): `quickSort` recurses with
//! `pivotNewIndex - 1` (which can be `left - 1`, i.e. one below `left`)
//! and `insertionSort` scans `j` down past `left` to `left - 1`. To
//! preserve that exactly, every index is `isize`, converted to `usize`
//! only at the point of slice indexing.
//!
//! The C `assert(...)` calls (index non-negativity, `Vector` struct /
//! `Object` type-tag consistency, non-null slots) are omitted: they
//! reference the `Vector`/`Object` machinery not ported here, and
//! Rust's slice indexing already bounds-checks.
//!
//! Not ported (and why):
//! - `combSort` (`Vector.c:134`) — it is COMMENTED OUT in the C source
//!   (inside a `/* */` block), i.e. dead code that never compiles into
//!   htop. It only shows up in the C-name snapshot because the extractor
//!   greps through comments. Porting it would be faithful to a comment,
//!   not to htop's behavior.
//! - The dynamic-array memory machinery: `Vector_new`, `Vector_delete`,
//!   `Vector_insert`, `Vector_add`, `Vector_resizeIfNecessary`,
//!   `Vector_prune`, `Vector_compact`, `Vector_moveUp`,
//!   `Vector_moveDown`, `Vector_set`, `Vector_merge`, `Vector_splice`,
//!   `Vector_size`, `Vector_isConsistent`, `Vector_countEquals`,
//!   `Vector_quickSortCustomCompare`, `Vector_insertionSort`. These
//!   manage a heap-allocated `Object**` with an owner/type/growth
//!   policy — like the XUtils allocation wrappers they have no faithful
//!   safe-Rust analog, because Rust's `Vec` owns its allocation,
//!   bounds, and element lifetimes rather than hand-rolling them over a
//!   raw pointer array.
#![allow(non_snake_case)]

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
pub fn quickSort<T>(
    array: &mut [T],
    left: isize,
    right: isize,
    compare: &impl Fn(&T, &T) -> i32,
) {
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
pub fn Vector_indexOf<T>(
    array: &[T],
    search: &T,
    compare: &impl Fn(&T, &T) -> i32,
) -> isize {
    let mut i: isize = 0;
    while (i as usize) < array.len() {
        if compare(search, &array[i as usize]) == 0 {
            return i;
        }
        i += 1;
    }
    -1
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
            assert_eq!(qsorted(input.clone(), &asc), reference, "quickSort {input:?}");
            assert_eq!(isorted(input.clone(), &asc), reference, "insertionSort {input:?}");
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
}
