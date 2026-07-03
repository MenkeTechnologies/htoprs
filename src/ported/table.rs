//! Port of `Table.c` — htop's base container for the row-table half of
//! the screen: the known-row set, the id→row lookup, the flattened
//! display list, and the tree-building / sort / expand-collapse /
//! prepare-cleanup lifecycle.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Data model (how htop's `Table` maps to Rust)
//!
//! htop's `Table` (`Table.h:26`) is:
//!
//! ```c
//! Vector* rows;         // all known rows; owns them; sort order varies
//! Vector* displayList;  // tree flattened in display order (BORROWED)
//! Hashtable* table;     // id -> row, fast lookup
//! struct Machine_* host;
//! const char* incFilter;
//! bool needsSort;
//! int following;
//! struct Panel_* panel;
//! ```
//!
//! | C field           | Rust field                    | Notes |
//! |-------------------|-------------------------------|-------|
//! | `Vector* rows`    | [`Table::rows`] `Vec<Option<Box<dyn Object>>>` | owns the rows polymorphically — htop's `Vector` of upcast `Row*` (really `Process*`/`DarwinProcess*`) becomes boxed `dyn Object`, recovered as `&Row` via [`Object::as_row`] (the private `Table::row`/`Table::row_mut` helpers). `Option` models the `NULL` a slot holds after `Vector_softRemove`, reclaimed by [`Table_compact`]. `Vector_size` == `rows.len()` (holes included, exactly like C `items`). |
//! | `Vector* displayList` | [`Table::displayList`] `Vec<usize>` | the C list *borrows* `Row*`; here it borrows by index into `rows` (valid because tree building never reorders `rows` after the sort). |
//! | `Hashtable* table`| [`Table::table`] `HashMap<i32, usize>` | id → index in `rows`. Rebuilt after any reordering (`Table::rebuild_index`) — the Rust-model equivalent of C's pointer stability across a `Vector` sort. |
//! | `Machine* host`   | [`Table::host`] `*const Machine` | a real back-pointer, exactly like C. The ported fns that dereference it (`Table_add`, `Table_updateDisplayList`, `Table_cleanupRow`) read `host->monotonicMs` / `host->settings` through it; a non-null `host` is their precondition, as in C. |
//! | `bool needsSort` / `int following` / `Panel* panel` / `incFilter` | modeled directly; `panel` is an opaque `usize` handle (the ncurses `Panel` is not dereferenced by any ported fn). |
//!
//! `rows_isDirty` mirrors the `isDirty` flag of the C `rows` Vector
//! (set by soft-remove, cleared by compaction); it is a plain field, not
//! a function.
//!
//! # Ported
//!
//! `Table_init`, `Table_done`, `Table_setPanel`, `Table_add`,
//! `Table_removeIndex`, `Table_buildTreeBranch`,
//! `compareRowByKnownParentThenNatural`, `Table_buildTree`,
//! `Table_updateDisplayList`, `Table_expandTree`,
//! `Table_collapseAllBranches`, `Table_prepareEntries`,
//! `Table_cleanupRow`, `Table_cleanupEntries`, `Table_compact`,
//! `Table_findRow`.
//!
//! `Table_delete`, `Table_rebuildPanel`, and `Table_printHeader` are also
//! ported now that their substrate (`panel.rs`, `richstring.rs`, `crt.rs`,
//! `settings.rs`, `RowField_alignedTitle`) has landed. This module has no
//! remaining `todo!()` stubs.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::collections::HashMap;

use crate::ported::crt::{ColorElements, ColorScheme, TreeStr};
use crate::ported::machine::Machine;
use crate::ported::object::Object;
use crate::ported::panel::{
    Panel, PanelItem, Panel_getSelectedIndex, Panel_prune, Panel_setSelected,
    Panel_setSelectionColor, Panel_size,
};
use crate::ported::process::ProcessField;
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendWide, RichString_getCharVal,
    RichString_rewind, RichString_size,
};
use crate::ported::row::{
    Row, RowField_alignedTitle, Row_compare, Row_compareByParent_Base, Row_getGroupOrParent,
    Row_isChildOf,
};
use crate::ported::settings::{
    RowField, ScreenSettings_getActiveDirection, ScreenSettings_getActiveSortKey, Settings,
};
use crate::ported::vector::{insertionSort, quickSort};

/// Port of htop's `struct Table_` (`Table.h:26`). See the module docs
/// for the field-by-field mapping to C.
pub struct Table {
    /// C `Vector* rows` — all known rows, owned. A `None` slot models a
    /// `NULL` left by `Vector_softRemove`, reclaimed by [`Table_compact`].
    pub rows: Vec<Option<Box<dyn Object>>>,
    /// C `Vector* displayList` — the row tree flattened in display order,
    /// borrowed by index into [`rows`](Table::rows).
    pub displayList: Vec<usize>,
    /// C `Hashtable* table` — fast id → row lookup, as id → index.
    pub table: HashMap<i32, usize>,
    /// C `struct Machine_* host` — back-pointer to the owning machine.
    pub host: *const Machine,
    /// C `const char* incFilter` — incremental filter string (only read
    /// by the stubbed `Table_rebuildPanel`).
    pub incFilter: Option<String>,
    /// C `bool needsSort`.
    pub needsSort: bool,
    /// C `int following` — `-1` or the id of the row being tracked.
    pub following: i32,
    /// C `int stableId` (`Table.h:37`) — stable tree view: row id to keep
    /// at a fixed screen position (`-1` = inactive).
    pub stableId: i32,
    /// C `int stableLastIdx` (`Table.h:38`) — panel index where the
    /// `stableId` row was placed in the last rebuild.
    pub stableLastIdx: i32,
    /// C `struct Panel_* panel` — the `Panel` this table renders into, set
    /// by [`Table_setPanel`] and read by `Table_rebuildPanel`. A raw
    /// `*mut Panel` mirroring htop's pointer graph; null until wired.
    pub panel: *mut Panel,
    /// The `isDirty` flag of the C `rows` Vector: set by soft-remove in
    /// `Table_removeIndex`, cleared by [`Table_compact`].
    pub rows_isDirty: bool,
    /// The scan vtable this table dispatches through. C stores the class in
    /// the `Object` base (`this->super.klass`) and the `As_Table(t)` macro
    /// (`Table.h:52`) casts it to `const TableClass*`; the ported `Table`
    /// models no `Object` base, so the [`TableClass`] pointer is held
    /// directly here. Null models a table whose class was never set; the
    /// scan dispatchers fall back to the base defaults (prepare/cleanup) and
    /// treat a missing `iterate` as the C's undefined behavior (panic). Set
    /// by the concrete `*_new` (e.g. the Darwin `ProcessTable_new`), mirroring
    /// C's `Object_setClass`.
    pub klass: *const TableClass,
}

/// Port of `typedef void (*Table_ScanPrepare)(Table* this)` (`Table.h:38`).
pub type Table_ScanPrepare = fn(*mut Table);
/// Port of `typedef void (*Table_ScanIterate)(Table* this)` (`Table.h:39`).
pub type Table_ScanIterate = fn(*mut Table);
/// Port of `typedef void (*Table_ScanCleanup)(Table* this)` (`Table.h:40`).
pub type Table_ScanCleanup = fn(*mut Table);

/// Port of `typedef struct TableClass_` (`Table.h:46`). The scan-vtable half
/// of htop's `TableClass` (the `ObjectClass super` half — `extends`/`delete`
/// — is class identity in Rust, so it is not modeled). Each slot takes a
/// `*mut Table` exactly like the C function pointers; a concrete override
/// downcasts it to its subclass (`(ProcessTable*)super`). `prepare`/`cleanup`
/// are `Option` because the C macros substitute [`Table_prepareEntries`] /
/// [`Table_cleanupEntries`] when the slot is NULL; `iterate` is `Option` for
/// the base class (which sets none) but is mandatory at dispatch time.
pub struct TableClass {
    /// C `const Table_ScanPrepare prepare` (`Table.h:48`).
    pub prepare: Option<Table_ScanPrepare>,
    /// C `const Table_ScanIterate iterate` (`Table.h:49`) — mandatory.
    pub iterate: Option<Table_ScanIterate>,
    /// C `const Table_ScanCleanup cleanup` (`Table.h:50`).
    pub cleanup: Option<Table_ScanCleanup>,
}

/// Port of the `Table_scanPrepare(t_)` macro (`Table.h:55`):
/// `As_Table(t_)->prepare ? As_Table(t_)->prepare(t_) : Table_prepareEntries(t_)`.
/// Dispatches to the class `prepare` override, or the base
/// [`Table_prepareEntries`] when the class sets none (the C macro's `:` arm —
/// inlined here, which is also the fallback for a class-less table).
///
/// # Safety precondition
/// `this` must be a valid non-null `*mut Table` with `klass` either null or a
/// valid `*const TableClass` (as C dereferences `this->super.klass`).
pub fn Table_scanPrepare(this: *mut Table) {
    // SAFETY: `this` is a valid `*mut Table`; `klass.as_ref()` yields `None`
    // for a null class (falling back to the base default).
    let klass = unsafe { (*this).klass.as_ref() };
    match klass.and_then(|k| k.prepare) {
        Some(f) => f(this),
        // C `: Table_prepareEntries(t_)`.
        None => Table_prepareEntries(unsafe { &mut *this }),
    }
}

/// Port of the `Table_scanIterate(t_)` macro (`Table.h:56`):
/// `As_Table(t_)->iterate(t_)` — mandatory, no default. Dispatches to the
/// class `iterate` override.
///
/// # Safety precondition
/// `this` must be a valid non-null `*mut Table` whose `klass` is non-null and
/// defines an `iterate` slot (C requires "a custom iterate method").
pub fn Table_scanIterate(this: *mut Table) {
    // SAFETY: `this` is a valid `*mut Table` (dispatch precondition).
    let klass = unsafe { (*this).klass.as_ref() }
        .expect("Table_scanIterate: table has no class (null klass)");
    let iterate = klass
        .iterate
        .expect("Table_scanIterate: iterate is mandatory but unset");
    iterate(this);
}

/// Port of the `Table_scanCleanup(t_)` macro (`Table.h:57`):
/// `As_Table(t_)->cleanup ? As_Table(t_)->cleanup(t_) : Table_cleanupEntries(t_)`.
/// Dispatches to the class `cleanup` override, or the base
/// [`Table_cleanupEntries`] when the class sets none (or has no class).
///
/// # Safety precondition
/// `this` must be a valid non-null `*mut Table` with `klass` either null or a
/// valid `*const TableClass`.
pub fn Table_scanCleanup(this: *mut Table) {
    // SAFETY: `this` is a valid `*mut Table`; `klass.as_ref()` yields `None`
    // for a null class (falling back to the base default).
    let klass = unsafe { (*this).klass.as_ref() };
    match klass.and_then(|k| k.cleanup) {
        Some(f) => f(this),
        // C `: Table_cleanupEntries(t_)`.
        None => Table_cleanupEntries(unsafe { &mut *this }),
    }
}

impl Table {
    /// A zeroed `Table` (null host, empty containers). Gate-skipped
    /// associated fn — not a real C function; the C analog is `xMalloc`
    /// returning uninitialized storage that `Table_init` overwrites.
    pub fn empty() -> Table {
        Table {
            rows: Vec::new(),
            displayList: Vec::new(),
            table: HashMap::new(),
            host: core::ptr::null(),
            incFilter: None,
            needsSort: true,
            following: -1,
            stableId: -1,
            stableLastIdx: 0,
            panel: core::ptr::null_mut(),
            rows_isDirty: false,
            klass: core::ptr::null(),
        }
    }

    /// Rebuild the id → index map from the current `rows` order. The C
    /// `Hashtable` needs no rebuild because it stores `Row*` pointers
    /// that survive a `Vector` sort; this index-based model must refresh
    /// the map after any reordering (the sort in [`Table_buildTree`] /
    /// [`Table_updateDisplayList`], and the compaction in
    /// [`Table_compact`]). Gate-skipped method (no C analog — it is the
    /// bookkeeping cost of representing pointers as indices).
    fn rebuild_index(&mut self) {
        self.table.clear();
        for (i, slot) in self.rows.iter().enumerate() {
            if let Some(obj) = slot {
                self.table.insert(obj.as_row().unwrap().id, i);
            }
        }
    }

    /// The embedded [`Row`] base of the object at slot `i`. The rows are
    /// owned polymorphically (`Box<dyn Object>`, really `Process`es for a
    /// `ProcessTable`); this recovers the shared `Row` view the tree/display
    /// algorithms read/write. Panics on a `NULL` slot or a non-`Row` object
    /// — both are Table invariants (holes are only transient, and a `Table`
    /// only ever holds `Row`s), mirroring the C `(Row*)Vector_get(...)`.
    #[inline]
    fn row(&self, i: usize) -> &Row {
        self.rows[i]
            .as_ref()
            .expect("Table::row: NULL slot")
            .as_row()
            .expect("Table::row: non-Row object")
    }

    /// Mutable counterpart of [`row`](Table::row).
    #[inline]
    fn row_mut(&mut self, i: usize) -> &mut Row {
        self.rows[i]
            .as_mut()
            .expect("Table::row_mut: NULL slot")
            .as_row_mut()
            .expect("Table::row_mut: non-Row object")
    }

    /// The embedded [`Row`] of slot `i`, or `None` for a `NULL` (soft-removed)
    /// slot — for the scan paths that tolerate holes.
    #[inline]
    fn row_opt(&self, i: usize) -> Option<&Row> {
        self.rows[i].as_ref().and_then(|o| o.as_row())
    }
}

/// Port of `Table* Table_init(Table* this, const ObjectClass* klass,
/// Machine* host)` from `Table.c:27`. Initializes the row/display
/// vectors and the lookup table, sets `needsSort = true`,
/// `following = -1`, `stableId = -1`, `stableLastIdx = 0`, and stores the
/// `host` back-pointer.
///
/// Signature mapping: the C `klass` argument selects the `Object` type
/// tag for the two `Vector_new` calls — class identity in Rust is the
/// concrete type, not a runtime tag, so the parameter is dropped. The C
/// returns `this`; the in-place mutation returns nothing.
pub fn Table_init(this: &mut Table, host: *const Machine) {
    this.rows = Vec::new();
    this.displayList = Vec::new();
    this.table = HashMap::new();
    this.needsSort = true;
    this.following = -1;
    this.stableId = -1;
    this.stableLastIdx = 0;
    this.host = host;
    this.rows_isDirty = false;
}

/// Port of `void Table_done(Table* this)` from `Table.c:39`. The C body
/// is `Hashtable_delete` + `Vector_delete` × 2; Rust `Drop` releases the
/// owned fields, so clearing them reproduces the observable teardown.
pub fn Table_done(this: &mut Table) {
    this.table.clear();
    this.displayList.clear();
    this.rows.clear();
}

/// Port of `static void Table_delete(Object* cast)` from `Table.c:42`:
/// `Table_done(this); free(this);`. Taking `this` by value reproduces
/// `free(this)`; [`Table_done`] runs the C teardown (mirroring the call
/// graph), then the consumed struct drops with its remaining owned fields.
pub fn Table_delete(mut this: Table) {
    Table_done(&mut this);
}

/// Port of `void Table_setPanel(Table* this, Panel* panel)` from
/// `Table.c:51`. C: `this->panel = panel;` — stores the `Panel*` verbatim.
pub fn Table_setPanel(this: &mut Table, panel: *mut Panel) {
    this.panel = panel;
}

/// Port of `void Table_add(Table* this, Row* row)` from `Table.c:55`.
/// Stamps the row's `seenStampMs` from `host->monotonicMs`, appends it to
/// `rows`, and registers it in the lookup table.
///
/// Signature mapping: the C `Row*` is owned by the `rows` Vector; here
/// the `Row` is moved in by value. The two pre-add `assert`s
/// (`Vector_indexOf(...) == -1`, `Hashtable_get(...) == NULL`) both
/// assert the id is not already present — modeled as the hashtable
/// membership `debug_assert!`. The `Vector_countEquals` post-assert is
/// implied by inserting exactly one row and one map entry.
///
/// # Safety precondition
/// `this.host` must be a valid non-null `*const Machine` (as in C, where
/// `this->host->monotonicMs` is dereferenced unconditionally).
pub fn Table_add(this: &mut Table, mut row: Box<dyn Object>) {
    let id = row.as_row().expect("Table_add: non-Row object").id;
    debug_assert!(
        !this.table.contains_key(&id),
        "Table_add: id already present"
    );

    // highlighting row found in first scan by first scan marked "far in the past"
    row.as_row_mut().unwrap().seenStampMs = unsafe { (*this.host).monotonicMs };

    let idx = this.rows.len();
    this.rows.push(Some(row));
    this.table.insert(id, idx);

    debug_assert!(this.table.contains_key(&id));
}

/// Port of `static void Table_removeIndex(Table* this, const Row* row,
/// int idx)` from `Table.c:75`. Removes the row's id from the lookup
/// table and soft-removes it from `rows` (leaving a `None` hole marked
/// dirty for [`Table_compact`]). If the removed row was being followed,
/// clears `following`. If the removed row is the stable-tree-view anchor,
/// walks the anchor up to its parent (or clears it).
///
/// Signature mapping: the C `const Row* row` is redundant with `idx`
/// (`row == Vector_get(rows, idx)`), so only `idx` is taken; the id and
/// the pre-removal `Row_getGroupOrParent(row)` (`rowparent`) are read back
/// from the slot before it is cleared. The C `Panel_setSelectionColor(
/// panel, PANEL_SELECTION_FOCUS)` on the follow-reset path is a
/// side-effect on the unported ncurses `Panel` and is omitted (the pure
/// state change, `following = -1`, is applied).
fn Table_removeIndex(this: &mut Table, idx: usize) {
    let row = this.row(idx);
    let rowid = row.id;
    // save before row is freed
    let rowparent = Row_getGroupOrParent(row);

    debug_assert!(this.table.contains_key(&rowid));

    this.table.remove(&rowid);

    // Vector_softRemove: NULL the slot, mark the vector dirty.
    this.rows[idx] = None;
    this.rows_isDirty = true;

    if this.following != -1 && this.following == rowid {
        this.following = -1;
        // C: Panel_setSelectionColor(this->panel, PANEL_SELECTION_FOCUS)
        // — ncurses Panel side-effect, applied by the UI layer.
    }

    // When the stable-tree-view anchor exits, walk up to its parent.
    if this.stableId != -1 && this.stableId == rowid {
        if rowparent != 0 && rowparent != rowid && this.table.contains_key(&rowparent) {
            this.stableId = rowparent;
        } else {
            this.stableId = -1;
        }
    }

    debug_assert!(!this.table.contains_key(&rowid));
}

/// Port of `static void Table_buildTreeBranch(Table* this, int rowid,
/// unsigned int level, int32_t indent, bool show)` from `Table.c:104`.
/// Appends the children of `rowid` (and, recursively, their subtrees) to
/// `displayList` in tree order, setting each row's `indent`, `show`, and
/// `tree_depth`.
///
/// The children of `rowid` form a contiguous run in the parent-sorted
/// `rows`: the run start is found by bisection, the run end by scanning
/// while `Row_isChildOf`. The indent bitmask is built exactly as C —
/// `indent | (1 << MINIMUM(level, sizeof(int32_t)*8 - 2))` (== 30) — and
/// the last shown sibling gets a negated indent (the "last child" tree
/// marker).
///
/// Borrow mapping: C holds a `Row*` across the recursive call; Rust
/// cannot alias `this.rows[i]` while recursing on `this`, so the needed
/// scalars are copied out and each field write indexes `this.rows[i]`
/// afresh, preserving the C read/write order (children pushed and `show`
/// cleared before recursion; `indent`/`tree_depth` written after).
fn Table_buildTreeBranch(this: &mut Table, rowid: i32, level: u32, indent: i32, show: bool) {
    // Do not treat zero as root of any tree.
    if rowid == 0 {
        return;
    }

    // The vector is sorted by parent, find the start of the range by bisection
    let vsize = this.rows.len() as isize;
    let mut l: isize = 0;
    let mut r: isize = vsize;
    while l < r {
        let c = l + (r - l) / 2;
        let row = this.row(c as usize);
        let parent = if row.isRoot {
            0
        } else {
            Row_getGroupOrParent(row)
        };
        if parent < rowid {
            l = c + 1;
        } else {
            r = c;
        }
    }
    // Find the end to know the last line for indent handling purposes
    let mut last_shown = r;
    while r < vsize {
        let row = this.row(r as usize);
        if !Row_isChildOf(row, rowid) {
            break;
        }
        if row.show {
            last_shown = r;
        }
        r += 1;
    }

    let mut i = l;
    while i < r {
        if !show {
            this.row_mut(i as usize).show = false;
        }

        let row_id = this.row(i as usize).id;
        this.displayList.push(i as usize);

        // MINIMUM(level, sizeof(row->indent) * 8 - 2); int32_t => 30.
        let shift = core::cmp::min(level, (core::mem::size_of::<i32>() as u32) * 8 - 2);
        let next_indent = indent | (1i32 << shift);

        let child_show = {
            let row = this.row(i as usize);
            row.show && row.showChildren
        };
        let branch_indent = if i < last_shown { next_indent } else { indent };
        Table_buildTreeBranch(this, row_id, level + 1, branch_indent, child_show);

        if i == last_shown {
            this.row_mut(i as usize).indent = -next_indent;
        } else {
            this.row_mut(i as usize).indent = next_indent;
        }

        this.row_mut(i as usize).tree_depth = level + 1;

        i += 1;
    }
}

/// Port of `static int compareRowByKnownParentThenNatural(const void*
/// v1, const void* v2)` from `Table.c:154`. The C dispatches through the
/// `Row_compareByParent` macro (`As_Row(r1)->compareByParent ? ... :
/// Row_compareByParent_Base(r1, r2)`); the base `Table` rows have no
/// override, so this calls [`Row_compareByParent_Base`] directly. (A
/// `ProcessTable` whose rows are `Process`es would dispatch to
/// `Process_compareByParent`; that vtable specialization is not modeled
/// by the base `Table` port.)
fn compareRowByKnownParentThenNatural(v1: &Row, v2: &Row) -> i32 {
    Row_compareByParent_Base(v1, v2)
}

/// Port of `static void Table_buildTree(Table* this)` from `Table.c:159`.
/// Builds a sorted tree from scratch: marks root rows (self-parented,
/// parentless, or parent-unknown), sorts `rows` by known parent then id,
/// then walks each root emitting its subtree into `displayList` via
/// `Table_buildTreeBranch`. Clears `needsSort`.
pub fn Table_buildTree(this: &mut Table) {
    // Vector_prune(displayList)
    this.displayList.clear();

    // Mark root processes
    let vsize = this.rows.len();
    for i in 0..vsize {
        let (id, parent) = {
            let row = this.row(i);
            (row.id, Row_getGroupOrParent(row))
        };

        // Faithful mirror of the C's two separate `continue` branches
        // (Table.c:169-181); keep them distinct rather than merging.
        #[allow(clippy::if_same_then_else)]
        let is_root = if id == parent {
            true
        } else if parent == 0 {
            true
        } else {
            // We don't know about its parent for whatever reason
            Table_findRow(this, parent).is_none()
        };
        this.row_mut(i).isRoot = is_root;
    }

    // Sort by known parent (roots first), then row ID
    let n = this.rows.len() as isize;
    quickSort(&mut this.rows, 0, n - 1, &|a, b| {
        compareRowByKnownParentThenNatural(
            a.as_ref().unwrap().as_row().unwrap(),
            b.as_ref().unwrap().as_row().unwrap(),
        )
    });
    // Pointers survive a C Vector sort; the index map must be refreshed.
    this.rebuild_index();

    // Find all processes whose parent is not visible
    for i in 0..vsize {
        // If parent not found, then construct the tree with this node as root
        if this.row(i).isRoot {
            {
                let row = this.row_mut(i);
                row.indent = 0;
                row.tree_depth = 0;
            }
            let id = this.row(i).id;
            let show_children = this.row(i).showChildren;
            this.displayList.push(i);
            Table_buildTreeBranch(this, id, 0, 0, show_children);
        }
    }

    this.needsSort = false;

    // Check consistency of the built structures
    debug_assert_eq!(this.displayList.len(), vsize);
}

/// Port of `void Table_updateDisplayList(Table* this)` from
/// `Table.c:208`. In tree view, rebuilds the tree when `needsSort`;
/// otherwise insertion-sorts `rows` (when `needsSort`) and copies them
/// straight into `displayList`. Clears `needsSort`.
///
/// Reads `host->settings->ss->treeView`; a valid non-null `host` with a
/// populated `settings` is the precondition (as in C). The canonical
/// `Settings` models the active screen `ss` as `screens[ssIndex]` (C's `ss`
/// is a pointer into `screens`), so the read goes through that index.
pub fn Table_updateDisplayList(this: &mut Table) {
    let tree_view = unsafe {
        let settings = (*this.host)
            .settings
            .as_ref()
            .expect("Table_updateDisplayList: host->settings is NULL");
        settings.screens[settings.ssIndex as usize].treeView
    };

    if tree_view {
        if this.needsSort {
            Table_buildTree(this);
        }
    } else {
        if this.needsSort {
            let n = this.rows.len() as isize;
            insertionSort(&mut this.rows, 0, n - 1, &|a, b| {
                Row_compare(
                    a.as_ref().unwrap().as_row().unwrap(),
                    b.as_ref().unwrap().as_row().unwrap(),
                )
            });
            this.rebuild_index();
        }
        this.displayList.clear();
        for i in 0..this.rows.len() {
            this.displayList.push(i);
        }
    }
    this.needsSort = false;
}

/// Port of `void Table_expandTree(Table* this)` from `Table.c:225`. Sets
/// `showChildren = true` on every row (expand-all).
pub fn Table_expandTree(this: &mut Table) {
    for obj in this.rows.iter_mut().flatten() {
        obj.as_row_mut().unwrap().showChildren = true;
    }
}

/// Port of `void Table_collapseAllBranches(Table* this)` from
/// `Table.c:234`. Rebuilds the tree to refresh `tree_depth`, forces a
/// re-sort, then collapses every non-root row (`tree_depth > 0 && id >
/// 1`, so PID 0/1 stay expanded on platforms where init has depth 1).
pub fn Table_collapseAllBranches(this: &mut Table) {
    Table_buildTree(this); // Update `tree_depth` fields of the rows
    this.needsSort = true; // Table is sorted by parent now, force new sort
    for obj in this.rows.iter_mut().flatten() {
        let row = obj.as_row_mut().unwrap();
        // FreeBSD has pid 0 = kernel and pid 1 = init, so init has tree_depth = 1
        if row.tree_depth > 0 && row.id > 1 {
            row.showChildren = false;
        }
    }
}

/// Port of `void Table_rebuildPanel(Table* this)` from `Table.c:246`.
/// Rebuilds the tree/display list, then re-populates the (borrowing) live
/// `Panel` with the visible rows: it prunes the panel, walks the display
/// list, and sets each shown row as a [`PanelItem::Borrowed`] pointer into
/// `this.rows` (the panel displays rows the `Table` owns — the Rust analog
/// of htop's non-owning `Panel_set(panel, i, (Object*)row)`). Selection and
/// scroll are preserved across the rebuild, following the `following` anchor
/// when set.
///
/// The `Table::panel` (`*mut Panel`) is dereferenced here — its non-null
/// validity is the precondition (set by `Machine_setTablesPanel`), exactly
/// as C dereferences `this->panel`.
///
/// Deviation: htop's `Row_isVisible` / `Row_matchesFilter` dispatch through
/// the `RowClass` vtable; the base slots are `true` / `false`, and the
/// `Process` overrides (the incremental filter) are not yet ported. So
/// `matchesFilter` is treated as `false` (no rows filtered out) and
/// `isVisible` as `true` here — all `row.show` rows are displayed. Wiring
/// the `Process` filter override will refine this.
pub fn Table_rebuildPanel(this: &mut Table) {
    Table_updateDisplayList(this);

    let panel = this.panel;
    debug_assert!(!panel.is_null());

    // Snapshot selection/scroll/size before pruning.
    let (curr_pos, curr_scroll_v, curr_size) = unsafe {
        (
            Panel_getSelectedIndex(&*panel),
            (*panel).scrollV,
            Panel_size(&*panel),
        )
    };

    unsafe {
        Panel_prune(&mut *panel);
    }

    // Follow the main group row if the followed row is occluded. With the
    // base `Row_isVisible == true`, the `== false` guard is never satisfied,
    // so this reduces to a no-op until the `Process` override lands; kept for
    // fidelity to the C control flow.
    if this.following != -1 {
        if let Some(&fidx) = this.table.get(&this.following) {
            let group = this.row(fidx).group;
            let followed_is_visible = true; // Row_isVisible base default
            if this.table.contains_key(&group) && !followed_is_visible {
                this.following = group;
            }
        }
    }

    let row_count = this.displayList.len();
    let mut found_followed = false;
    let mut idx: i32 = 0;

    for i in 0..row_count {
        let row_index = this.displayList[i];
        let (show, id) = {
            let row = this.row(row_index);
            (row.show, row.id)
        };

        // C: if (!row->show || Row_matchesFilter(row, this)) continue;
        // Row_matchesFilter base default is false.
        if !show {
            continue;
        }

        // Panel_set(panel, idx, (Object*)row): a borrowed pointer to the
        // table-owned row. Vector_set past the end grows the panel.
        let row_ptr: *mut dyn Object = this.rows[row_index].as_mut().unwrap().as_mut();
        unsafe {
            let p = &mut *panel;
            if (idx as usize) < p.items.len() {
                p.items[idx as usize] = PanelItem::Borrowed(row_ptr);
            } else {
                p.items.push(PanelItem::Borrowed(row_ptr));
            }
        }

        if this.following != -1 && id == this.following {
            found_followed = true;
            unsafe {
                Panel_setSelected(&mut *panel, idx);
                // Keep scroll position relative to the followed row.
                (*panel).scrollV = idx - (curr_pos - curr_scroll_v);
            }
        }
        idx += 1;
    }

    if this.following != -1 && !found_followed {
        // Current followed row not found — reset.
        this.following = -1;
        unsafe {
            Panel_setSelectionColor(&mut *panel, ColorElements::PANEL_SELECTION_FOCUS);
        }
    }

    if this.following == -1 {
        unsafe {
            // If the last item was selected, keep the new last item selected.
            if curr_pos > 0 && curr_pos == curr_size - 1 {
                Panel_setSelected(&mut *panel, Panel_size(&*panel) - 1);
            } else {
                Panel_setSelected(&mut *panel, curr_pos);
            }
            (*panel).scrollV = curr_scroll_v;
        }
    }
}

/// Port of `void Table_printHeader(const Settings* settings, RichString*
/// header)` from `Table.c:368`. Rebuilds the column-header `RichString`: for
/// each active-screen field, appends its aligned title in the header or
/// selection color, overlays the ascending/descending tree glyph on the
/// active sort column, and appends `"(merged)"` after `COMM` when
/// `showMergedCommand` is set. Pure (never touches a `Table`).
pub fn Table_printHeader(settings: &Settings, header: &mut RichString) {
    // C `RichString_rewind(header, RichString_size(header))` — clear it.
    RichString_rewind(header, RichString_size(header));

    let ss = &settings.screens[settings.ssIndex as usize];
    let key = ScreenSettings_getActiveSortKey(ss);
    let scheme = ColorScheme::active();

    for &field in &ss.fields {
        if field == 0 {
            break; // NULL_FIELD terminator
        }

        let color = if ss.treeView && ss.treeViewAlwaysByPID {
            ColorElements::PANEL_HEADER_FOCUS.packed(scheme)
        } else if key == field {
            ColorElements::PANEL_SELECTION_FOCUS.packed(scheme)
        } else {
            ColorElements::PANEL_HEADER_FOCUS.packed(scheme)
        };

        RichString_appendWide(
            header,
            color,
            RowField_alignedTitle(settings, field).as_bytes(),
        );

        // On the active sort column, override a trailing space with the
        // ascending/descending tree glyph.
        if key == field
            && RichString_getCharVal(header, (RichString_size(header) - 1) as usize) == ' '
        {
            let ascending = ScreenSettings_getActiveDirection(ss) == 1;
            RichString_rewind(header, 1);
            let glyph = if ascending {
                TreeStr::TREE_STR_ASC
            } else {
                TreeStr::TREE_STR_DESC
            };
            RichString_appendWide(
                header,
                ColorElements::PANEL_SELECTION_FOCUS.packed(scheme),
                glyph.glyph().as_bytes(),
            );
        }

        if field == ProcessField::COMM as RowField && settings.showMergedCommand {
            RichString_appendAscii(header, color, b"(merged)");
        }
    }
}

/// Port of `void Table_prepareEntries(Table* this)` from `Table.c:401`.
/// Resets per-scan row flags before a refresh: `updated = false`,
/// `wasShown = show`, `show = true`.
pub fn Table_prepareEntries(this: &mut Table) {
    for obj in this.rows.iter_mut().flatten() {
        let row = obj.as_row_mut().unwrap();
        row.updated = false;
        row.wasShown = row.show;
        row.show = true;
    }
}

/// Port of `Row* Table_cleanupRow(Table* table, Row* row, int idx)` from
/// `Table.c:411`. Decides a row's fate after a refresh: a tombed row is
/// removed once its `tombStampMs` elapses; a not-updated row is either
/// tombed (when `highlightChanges` and it was shown) or removed
/// immediately; otherwise it is kept.
///
/// Signature mapping: the C `Row* row` is redundant with `idx`
/// (`row == Vector_get(rows, idx)`), so only `idx` is taken. The C
/// return (`row` kept / `NULL` removed) becomes a `bool` (`true` = kept).
/// Reads `host->monotonicMs`, `settings->highlightChanges`, and
/// `settings->highlightDelaySecs`; a valid non-null `host`/`settings` is
/// the precondition (as in C).
pub fn Table_cleanupRow(this: &mut Table, idx: usize) -> bool {
    let (mono, highlight_changes, highlight_delay) = unsafe {
        let host = &*this.host;
        let settings = host
            .settings
            .as_ref()
            .expect("Table_cleanupRow: host->settings is NULL");
        (
            host.monotonicMs,
            settings.highlightChanges,
            settings.highlightDelaySecs,
        )
    };

    let (tomb, updated, was_shown) = {
        let row = this.row(idx);
        (row.tombStampMs, row.updated, row.wasShown)
    };

    let should_remove;
    if tomb > 0 {
        // remove tombed process once its stamp has elapsed
        should_remove = mono >= tomb;
    } else if !updated {
        // process no longer exists
        if highlight_changes && was_shown {
            // mark tombed
            this.row_mut(idx).tombStampMs = mono + (1000i64 * highlight_delay as i64) as u64;
            should_remove = false;
        } else {
            // immediately remove
            should_remove = true;
        }
    } else {
        should_remove = false;
    }

    if should_remove {
        Table_removeIndex(this, idx);
        return false;
    }
    true
}

/// Port of `void Table_cleanupEntries(Table* this)` from `Table.c:437`.
/// Walks `rows` back-to-front applying [`Table_cleanupRow`], tracking the
/// lowest removed index, then compacts from there.
pub fn Table_cleanupEntries(this: &mut Table) {
    // Lowest index of the row that is soft-removed. Used to speed up compaction.
    let mut dirty_index = this.rows.len();

    // Finish process table update, culling any removed rows
    for i in (0..this.rows.len()).rev() {
        if !Table_cleanupRow(this, i) {
            dirty_index = i;
        }
    }

    // compact the table in case of any earlier row removals
    Table_compact(this, dirty_index);
}

/// Port of `static inline void Table_compact(Table* this, int
/// dirtyIndex)` from `Table.h:91`: `Vector_compact(this->rows,
/// dirtyIndex)` then `this->needsSort = true`.
///
/// `Vector_compact` (`Vector.c:258`) is inlined here because `vector.rs`
/// does not port the dynamic-array machinery: if the vector is dirty and
/// `dirtyIndex` is within bounds, every non-`None` slot after
/// `dirtyIndex` is shifted down over the holes and the tail is
/// truncated. The index map is then rebuilt (C's `Row*` pointers survive
/// compaction; the index model must refresh). `needsSort` is set
/// unconditionally, matching the C inline.
pub fn Table_compact(this: &mut Table, dirtyIndex: usize) {
    // Vector_compact: no-op when not dirty or dirtyIndex past the end.
    if this.rows_isDirty && dirtyIndex < this.rows.len() {
        debug_assert!(this.rows[dirtyIndex].is_none());

        let items = this.rows.len();
        let mut di = dirtyIndex;
        for i in (dirtyIndex + 1)..items {
            if this.rows[i].is_some() {
                this.rows.swap(di, i);
                di += 1;
            }
        }
        this.rows.truncate(di);
        this.rows_isDirty = false;
        this.rebuild_index();
    }

    this.needsSort = true;
}

/// Port of `static inline Row* Table_findRow(Table* this, int id)` from
/// `Table.h:81`: `Hashtable_get(this->table, id)`. Returns the row with
/// the given id, or `None` when absent.
pub fn Table_findRow(this: &Table, id: i32) -> Option<&Row> {
    this.table.get(&id).and_then(|&i| this.row_opt(i))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::machine::{Machine, ScreenSettings, Settings};

    /// Build a `Machine` back-reference carrying only the fields the
    /// ported `Table` functions dereference.
    fn host(mono: u64, tree_view: bool, highlight_changes: bool, highlight_delay: i32) -> Machine {
        let mut m = Machine::default();
        m.monotonicMs = mono;
        m.settings = Some(Settings {
            highlightChanges: highlight_changes,
            highlightDelaySecs: highlight_delay,
            screens: vec![ScreenSettings {
                treeView: tree_view,
                ..Default::default()
            }],
            ..Default::default()
        });
        m
    }

    /// A row with the id/group/parent that drive tree building. `show`
    /// and `showChildren` default true (as `Row_init` sets them).
    fn row(id: i32, group: i32, parent: i32) -> Row {
        let mut r = Row::default();
        r.id = id;
        r.group = group;
        r.parent = parent;
        r.show = true;
        r.showChildren = true;
        r
    }

    /// A Table wired to `h` with the given rows added via `Table_add`.
    fn table_with(h: &Machine, rows: Vec<Row>) -> Table {
        let mut t = Table::empty();
        Table_init(&mut t, h as *const Machine);
        for r in rows {
            Table_add(&mut t, Box::new(r));
        }
        t
    }

    /// The ids in display order.
    fn display_ids(t: &Table) -> Vec<i32> {
        t.displayList.iter().map(|&i| t.row(i).id).collect()
    }

    #[test]
    fn init_sets_defaults() {
        let h = host(0, false, false, 0);
        let mut t = Table::empty();
        Table_init(&mut t, &h as *const Machine);
        assert!(t.needsSort);
        assert_eq!(t.following, -1);
        assert!(t.rows.is_empty());
        assert!(t.displayList.is_empty());
    }

    #[test]
    fn rebuild_panel_borrows_only_shown_rows() {
        use crate::ported::panel::{Panel_get, Panel_new, Panel_size};

        let h = host(1000, false, false, 0);
        let mut t = table_with(&h, vec![row(1, 0, 0), row(2, 0, 0), row(3, 0, 0)]);
        // Hide id 2 (added at index 1) before the rebuild.
        t.row_mut(1).show = false;

        // A live panel the table renders into (borrowing its rows).
        let mut panel = Panel_new(0, 0, 80, 24, None);
        t.panel = &mut panel as *mut Panel;

        Table_rebuildPanel(&mut t);

        // Only the two shown rows were borrowed into the panel.
        assert_eq!(Panel_size(&panel), 2);
        let shown: Vec<i32> = (0..Panel_size(&panel))
            .map(|i| Panel_get(&panel, i).as_row().unwrap().id)
            .collect();
        assert!(shown.contains(&1) && shown.contains(&3));
        assert!(!shown.contains(&2));
    }

    #[test]
    fn add_registers_row_and_stamps_seen() {
        let h = host(4242, false, false, 0);
        let t = table_with(&h, vec![row(10, 10, 0), row(20, 20, 0)]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(Table_findRow(&t, 10).unwrap().id, 10);
        assert_eq!(Table_findRow(&t, 20).unwrap().id, 20);
        assert!(Table_findRow(&t, 99).is_none());
        // seenStampMs stamped from host->monotonicMs
        assert_eq!(t.row(0).seenStampMs, 4242);
    }

    #[test]
    fn non_tree_display_list_is_insertion_sorted_by_id() {
        let h = host(1, false, false, 0);
        // Added out of id order; non-tree view sorts by Row_compare (id).
        let mut t = table_with(&h, vec![row(30, 30, 0), row(10, 10, 0), row(20, 20, 0)]);
        Table_updateDisplayList(&mut t);
        assert_eq!(display_ids(&t), vec![10, 20, 30]);
        assert!(!t.needsSort);
    }

    #[test]
    fn build_tree_orders_children_after_parent_with_depth_and_indent() {
        // 1 is root; 2 and 3 are children of 1; 4 is child of 2.
        //   1
        //   ├ 2
        //   │ └ 4
        //   └ 3
        let h = host(1, true, false, 0);
        let mut t = table_with(
            &h,
            vec![row(1, 1, 0), row(2, 2, 1), row(3, 3, 1), row(4, 4, 2)],
        );
        Table_updateDisplayList(&mut t);

        assert_eq!(display_ids(&t), vec![1, 2, 4, 3]);

        // depth: root 0, its children 1, grandchild 2
        let depth = |id: i32| Table_findRow(&t, id).unwrap().tree_depth;
        assert_eq!(depth(1), 0);
        assert_eq!(depth(2), 1);
        assert_eq!(depth(4), 2);
        assert_eq!(depth(3), 1);

        // root indent is 0
        assert_eq!(Table_findRow(&t, 1).unwrap().indent, 0);
        // 3 is the last shown child of 1 => negated indent (last-child marker)
        assert!(Table_findRow(&t, 3).unwrap().indent < 0);
        // 2 is a non-last child of 1 => positive indent
        assert!(Table_findRow(&t, 2).unwrap().indent > 0);
        // 4 is the only (last) child of 2 => negated indent
        assert!(Table_findRow(&t, 4).unwrap().indent < 0);

        assert_eq!(t.displayList.len(), t.rows.len());
    }

    #[test]
    fn build_tree_multiple_roots_sorted_by_id() {
        // Two independent roots (100, 50); each with one child.
        let h = host(1, true, false, 0);
        let mut t = table_with(
            &h,
            vec![
                row(100, 100, 0),
                row(101, 101, 100),
                row(50, 50, 0),
                row(51, 51, 50),
            ],
        );
        Table_updateDisplayList(&mut t);
        // roots sorted by id: 50 before 100, each followed by its child.
        assert_eq!(display_ids(&t), vec![50, 51, 100, 101]);
    }

    #[test]
    fn unknown_parent_becomes_root() {
        // 5's parent 999 is unknown => 5 is treated as a root.
        let h = host(1, true, false, 0);
        let mut t = table_with(&h, vec![row(5, 5, 999), row(6, 6, 5)]);
        Table_updateDisplayList(&mut t);
        assert_eq!(display_ids(&t), vec![5, 6]);
        assert!(Table_findRow(&t, 5).unwrap().isRoot);
    }

    #[test]
    fn collapsed_children_are_hidden_from_show_but_present_in_list() {
        // Parent 1 with showChildren=false hides 2 and 4 (show=false),
        // but they still appear in displayList (Table_rebuildPanel is what
        // filters on `show`, not buildTree).
        let h = host(1, true, false, 0);
        let mut t = table_with(&h, vec![row(1, 1, 0), row(2, 2, 1), row(4, 4, 2)]);
        t.row_mut(0).showChildren = false;
        Table_updateDisplayList(&mut t);
        assert_eq!(display_ids(&t), vec![1, 2, 4]);
        assert!(Table_findRow(&t, 1).unwrap().show);
        assert!(!Table_findRow(&t, 2).unwrap().show);
        assert!(!Table_findRow(&t, 4).unwrap().show);
    }

    #[test]
    fn expand_tree_sets_show_children_everywhere() {
        let h = host(1, true, false, 0);
        let mut t = table_with(&h, vec![row(1, 1, 0), row(2, 2, 1)]);
        t.row_mut(0).showChildren = false;
        t.row_mut(1).showChildren = false;
        Table_expandTree(&mut t);
        assert!(t.row(0).showChildren);
        assert!(t.row(1).showChildren);
    }

    #[test]
    fn collapse_all_branches_collapses_non_root_rows() {
        // 1 root, 2 child, 3 grandchild. After collapse-all, non-root
        // rows (depth>0, id>1) have showChildren=false; root keeps it.
        let h = host(1, true, false, 0);
        let mut t = table_with(&h, vec![row(1, 1, 0), row(2, 2, 1), row(3, 3, 2)]);
        Table_collapseAllBranches(&mut t);
        assert!(t.needsSort);
        assert!(Table_findRow(&t, 1).unwrap().showChildren); // root stays
        assert!(!Table_findRow(&t, 2).unwrap().showChildren);
        assert!(!Table_findRow(&t, 3).unwrap().showChildren);
    }

    #[test]
    fn prepare_entries_resets_scan_flags() {
        let h = host(1, false, false, 0);
        let mut t = table_with(&h, vec![row(1, 1, 0)]);
        {
            let r = t.row_mut(0);
            r.updated = true;
            r.show = false;
        }
        Table_prepareEntries(&mut t);
        let r = t.row(0);
        assert!(!r.updated);
        assert!(r.show);
        assert!(!r.wasShown); // wasShown = old show (false)
    }

    #[test]
    fn cleanup_removes_not_updated_row_when_not_highlighting() {
        // No highlightChanges: a not-updated row is removed immediately,
        // then compacted out of `rows` and the lookup table.
        let h = host(1000, false, false, 0);
        let mut t = table_with(&h, vec![row(1, 1, 0), row(2, 2, 0), row(3, 3, 0)]);
        // Mark all updated except id 2.
        t.row_mut(0).updated = true;
        t.row_mut(1).updated = false;
        t.row_mut(2).updated = true;

        Table_cleanupEntries(&mut t);

        // id 2 removed; 1 and 3 remain, holes compacted away.
        assert_eq!(t.rows.len(), 2);
        assert!(t.rows.iter().all(|s| s.is_some()));
        assert!(Table_findRow(&t, 1).is_some());
        assert!(Table_findRow(&t, 2).is_none());
        assert!(Table_findRow(&t, 3).is_some());
        assert!(t.needsSort);
    }

    #[test]
    fn cleanup_tombs_not_updated_row_when_highlighting() {
        // highlightChanges + wasShown => row is tombed (kept) with a
        // future tombStampMs = monotonicMs + 1000*highlightDelaySecs.
        let h = host(1000, false, true, 5);
        let mut t = table_with(&h, vec![row(7, 7, 0)]);
        {
            let r = t.row_mut(0);
            r.updated = false;
            r.wasShown = true;
        }
        Table_cleanupEntries(&mut t);
        // Still present, now tombed.
        assert_eq!(t.rows.len(), 1);
        assert_eq!(t.row(0).tombStampMs, 1000 + 1000 * 5);
    }

    #[test]
    fn cleanup_removes_tombed_row_after_stamp_elapses() {
        // A row already tombed at 500ms is removed once monotonicMs >= 500.
        let h = host(1000, false, true, 5);
        let mut t = table_with(&h, vec![row(8, 8, 0)]);
        {
            let r = t.row_mut(0);
            r.updated = false;
            r.tombStampMs = 500;
        }
        Table_cleanupEntries(&mut t);
        assert!(t.rows.is_empty());
        assert!(Table_findRow(&t, 8).is_none());
    }

    #[test]
    fn remove_index_via_cleanup_clears_following() {
        // Following a row that then gets removed resets following to -1.
        let h = host(1000, false, false, 0);
        let mut t = table_with(&h, vec![row(1, 1, 0), row(2, 2, 0)]);
        t.following = 2;
        t.row_mut(0).updated = true;
        t.row_mut(1).updated = false; // id 2 removed
        Table_cleanupEntries(&mut t);
        assert_eq!(t.following, -1);
    }

    #[test]
    fn compact_preserves_order_of_survivors() {
        // Remove the middle of five; survivors keep their relative order
        // and the index map stays consistent.
        let h = host(1000, false, false, 0);
        let mut t = table_with(
            &h,
            vec![
                row(1, 1, 0),
                row(2, 2, 0),
                row(3, 3, 0),
                row(4, 4, 0),
                row(5, 5, 0),
            ],
        );
        for i in 0..5 {
            t.row_mut(i).updated = true;
        }
        t.row_mut(2).updated = false; // remove id 3
        Table_cleanupEntries(&mut t);
        let ids: Vec<i32> = t
            .rows
            .iter()
            .map(|s| s.as_ref().unwrap().as_row().unwrap().id)
            .collect();
        assert_eq!(ids, vec![1, 2, 4, 5]);
        // index map points at the right slots after compaction
        assert_eq!(t.row(*t.table.get(&4).unwrap()).id, 4);
    }

    /// [`Table_printHeader`] builds a non-empty header from the active
    /// screen's columns and is idempotent — the leading `RichString_rewind`
    /// clears the prior contents, so a second call yields the same size.
    #[test]
    fn print_header_renders_columns_and_is_idempotent() {
        use crate::ported::settings::ScreenSettings;

        let mut settings = Settings::default();
        settings.screens = vec![ScreenSettings {
            fields: vec![
                ProcessField::PID as RowField,
                ProcessField::NICE as RowField,
            ],
            sortKey: ProcessField::PID as RowField,
            direction: 1,
            ..Default::default()
        }];

        let mut header = RichString::default();
        Table_printHeader(&settings, &mut header);
        let n = RichString_size(&header);
        assert!(n > 0);

        // Rewind-and-rebuild → identical size (not doubled).
        Table_printHeader(&settings, &mut header);
        assert_eq!(RichString_size(&header), n);
    }
}
