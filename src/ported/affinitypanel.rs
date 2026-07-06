//! Port of `AffinityPanel.c` — htop's "Use CPUs:" affinity picker.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Two builds: `HAVE_LIBHWLOC` (the `hwloc` cargo feature) vs plain
//!
//! `AffinityPanel.c` is written for two configurations, split by
//! `#ifdef HAVE_LIBHWLOC`:
//!
//! - **Plain build (default, `#[cfg(not(feature = "hwloc"))]`).** A flat CPU
//!   list: each [`MaskItem`] carries an `int cpu`; the panel is a flat
//!   [`Vector`] of one item per online CPU. This is the working, byte-for-byte
//!   unchanged path — every non-hwloc item and function below is exactly what it
//!   was before the `hwloc` feature was added, and the default `cargo build`
//!   compiles only this path. The five hwloc-only functions remain the same
//!   honest `todo!()` stubs they were.
//! - **hwloc build (`--features hwloc`, `#[cfg(feature = "hwloc")]`).** A
//!   topology tree backed by `hwloc_bitmap_t` cpusets: each [`MaskItem`] carries
//!   a `cpuset`/`ownCpuset` pair instead of `cpu`, and the panel gains a
//!   `topoRoot`/`allCpuset`/`workCpuset` trio. This is a faithful analog of
//!   htop's `./configure --enable-hwloc` variant (the `unwind`/`demangle`/
//!   `capabilities` external-lib feature precedent): libhwloc does not link on
//!   macOS, so the feature is verified by primary-source reading of hwloc's
//!   `include/hwloc.h` + `include/hwloc/bitmap.h` and by `cargo check --features
//!   hwloc` type-checking the FFI without linking.
//!
//! Everything hwloc-specific is `#[cfg(feature = "hwloc")]`-gated; nothing on the
//! default path changed.
//!
//! # The five ported hwloc functions (`#[cfg(feature = "hwloc")]`)
//!
//! - [`MaskItem_newMask`] (`AffinityPanel.c:94`) — the tree-node constructor:
//!   `hwloc_bitmap_weight(cpuset) > 1 ? 1 : 0` sets `sub_tree`, `indent` is
//!   non-NULL, `ownCpuset = owner`.
//! - [`AffinityPanel_updateItem`] (`AffinityPanel.c:156`) — `value` from
//!   `hwloc_bitmap_isincluded`/`hwloc_bitmap_intersects` against `workCpuset`,
//!   then the non-owning `Panel_add` (a borrowed append, mirroring
//!   [`Panel_splice`]).
//! - [`AffinityPanel_updateTopo`] (`AffinityPanel.c:165`) — recurses the item's
//!   `children`, stopping at a collapsed (`sub_tree == 2`) node.
//! - [`AffinityPanel_addObject`] (`AffinityPanel.c:283`) — reads
//!   `hwloc_obj` fields (`type`/`os_index`/`depth`/`logical_index`/
//!   `next_sibling`/`complete_cpuset`), builds the indent guides + `TYPE #idx`
//!   label, and the collapse heuristic.
//! - [`AffinityPanel_buildTopology`] (`AffinityPanel.c:341`) — walks
//!   `obj->children[0..arity]` recursively, threading the `indent` bitmask.
//!
//! # `host->topology`
//!
//! The C `AffinityPanel_new` hwloc branch reads `host->topology` twice — to seed
//! `allCpuset` (`hwloc_topology_get_complete_cpuset`) and to build `topoRoot`
//! (`AffinityPanel_buildTopology(hwloc_get_root_obj(host->topology), …)`). The
//! ported [`Machine`] now carries the `#[cfg(feature = "hwloc")] topology` field,
//! loaded by `Machine_init` (`hwloc_topology_init` + `set_all_types_filter` +
//! `load`), so `AffinityPanel_new` seeds both from it here (`hwloc_get_root_obj`
//! is the header's `static __hwloc_inline hwloc_get_obj_by_depth(topo, 0, 0)`).
//!
//! # Shared / non-hwloc functions
//!
//! - [`MaskItem_display`] (`AffinityPanel.c:62`) — identical in both builds
//!   (reads `text`/`indent`/`value`/`sub_tree`, no cpuset), so it is shared.
//! - [`MaskItem_newSingleton`], [`AffinityPanel_new`], [`AffinityPanel_update`],
//!   [`AffinityPanel_eventHandler`], [`AffinityPanel_getAffinity`],
//!   [`MaskItem_delete`], [`AffinityPanel_delete`] each have an
//!   `#ifdef HAVE_LIBHWLOC`/`#else` split, ported as a `#[cfg]` pair.
//!   `MaskItem`'s owned `cpuset` is released by a `#[cfg(feature = "hwloc")]`
//!   [`Drop`] impl (the `History_delete`/`Panel_delete` teardown precedent), so
//!   both `MaskItem_delete` bodies stay `let _ = this`.
//!
//! The non-owning `Panel` borrows `PanelItem::Borrowed` pointers into the owning
//! `cpuids` [`Vector`] (plain build) or into the `topoRoot` tree (hwloc build);
//! [`Panel_splice`] / the borrowed `Panel_add` express that.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::affinity::{Affinity, Affinity_add, Affinity_new};
use crate::ported::crt::{
    ColorElements, ColorScheme, TreeStr, KEY_ENTER, KEY_F, KEY_MOUSE, KEY_RECLICK,
};
use crate::ported::functionbar::{FunctionBar_new, FunctionBar_setLabel};
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::{
    HandlerResult, Panel, PanelClass, Panel_done, Panel_getSelectedIndex, Panel_new, Panel_prune,
    Panel_setHeader, Panel_setSelected,
};
// Panel_splice re-adds the whole cpuids Vector at once; only the plain build's
// AffinityPanel_update calls it (the hwloc build re-adds items one at a time),
// but it stays imported in both configs so the doc links resolve — hence
// allow-unused under the feature.
#[cfg_attr(feature = "hwloc", allow(unused_imports))]
use crate::ported::panel::Panel_splice;
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_appendWide};
use crate::ported::vector::{
    Vector, Vector_add, Vector_delete, Vector_get, Vector_new, Vector_size, VECTOR_DEFAULT_SIZE,
};

// The hwloc branches also need the borrowed `Panel` item variant (for the
// non-owning `Panel_add`) and the C scalar/string FFI types. `TreeStr` and the
// panel/vector/richstring helpers are already imported above.
#[cfg(feature = "hwloc")]
use crate::ported::panel::PanelItem;
#[cfg(feature = "hwloc")]
use core::ffi::{c_char, c_int, c_uint, c_void, CStr};

// ── libhwloc FFI (`#[cfg(feature = "hwloc")]`) ────────────────────────────────
//
// Every layout and value below is transcribed from hwloc's primary-source
// headers, not guessed:
//   * `struct hwloc_obj` — `include/hwloc.h` (open-mpi/hwloc master).
//   * bitmap functions — `include/hwloc/bitmap.h`.
//   * `hwloc_topology_get_complete_cpuset` — `include/hwloc/helper.h`.
// libhwloc does not link on macOS, so this surface is type-checked, not linked
// (the `unwind`/`demangle`/`capabilities` external-lib feature precedent).

/// `typedef struct hwloc_bitmap_s * hwloc_bitmap_t;` (`bitmap.h`). Opaque.
#[cfg(feature = "hwloc")]
#[allow(non_camel_case_types)]
pub type hwloc_bitmap_t = *mut c_void;
/// `typedef const struct hwloc_bitmap_s * hwloc_const_bitmap_t;` (`bitmap.h`).
#[cfg(feature = "hwloc")]
#[allow(non_camel_case_types)]
pub type hwloc_const_bitmap_t = *const c_void;
/// `typedef hwloc_bitmap_t hwloc_cpuset_t;` (`hwloc.h:165`).
#[cfg(feature = "hwloc")]
#[allow(non_camel_case_types)]
pub type hwloc_cpuset_t = hwloc_bitmap_t;
/// `typedef hwloc_const_bitmap_t hwloc_const_cpuset_t;` (`hwloc.h:167`).
#[cfg(feature = "hwloc")]
#[allow(non_camel_case_types)]
pub type hwloc_const_cpuset_t = hwloc_const_bitmap_t;
/// `typedef struct hwloc_topology * hwloc_topology_t;` (`hwloc.h:788`). Opaque.
#[cfg(feature = "hwloc")]
#[allow(non_camel_case_types)]
pub type hwloc_topology_t = *mut c_void;
/// `typedef enum { … } hwloc_obj_type_t;` (`hwloc.h:202`). An unnamed C enum,
/// so `int`-sized.
#[cfg(feature = "hwloc")]
#[allow(non_camel_case_types)]
pub type hwloc_obj_type_t = c_int;
/// `typedef struct hwloc_obj * hwloc_obj_t;` (`hwloc.h:691`).
#[cfg(feature = "hwloc")]
#[allow(non_camel_case_types)]
pub type hwloc_obj_t = *mut hwloc_obj;

/// `HWLOC_OBJ_PU` (`hwloc.h:238`) — the 5th member of the `hwloc_obj_type_e`
/// enum (`MACHINE=0, PACKAGE=1, DIE=2, CORE=3, PU=4`), so its value is `4`.
#[cfg(feature = "hwloc")]
pub const HWLOC_OBJ_PU: hwloc_obj_type_t = 4;

/// `struct hwloc_obj` (`hwloc.h:492`), transcribed field-for-field in order up
/// to and including `complete_cpuset` — the last field the port reads. `attr`
/// is `union hwloc_obj_attr_u*`, a pointer, so it is modelled opaque
/// (`*mut c_void`) and never dereferenced. Fields *after* `complete_cpuset`
/// (`nodeset`, `complete_nodeset`, `infos`, `userdata`, `gp_index`) are omitted
/// on purpose: the port reads none of them, and every access is through a
/// pointer, so the struct's total size is irrelevant — every offset up to
/// `complete_cpuset` is fixed by the preceding fields, which are all present.
#[cfg(feature = "hwloc")]
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct hwloc_obj {
    /// C `hwloc_obj_type_t type` — read via `hwloc_obj_type_string` / compared
    /// to `HWLOC_OBJ_PU`. `type` is a Rust keyword, hence `type_`.
    pub type_: hwloc_obj_type_t,
    pub subtype: *mut c_char,
    /// C `unsigned os_index`.
    pub os_index: c_uint,
    pub name: *mut c_char,
    /// C `hwloc_uint64_t total_memory`.
    pub total_memory: u64,
    /// C `union hwloc_obj_attr_u *attr` — opaque pointer, never dereferenced.
    pub attr: *mut c_void,
    /// C `int depth`.
    pub depth: c_int,
    /// C `unsigned logical_index`.
    pub logical_index: c_uint,
    pub next_cousin: *mut hwloc_obj,
    pub prev_cousin: *mut hwloc_obj,
    pub parent: *mut hwloc_obj,
    pub sibling_rank: c_uint,
    /// C `struct hwloc_obj *next_sibling`.
    pub next_sibling: *mut hwloc_obj,
    pub prev_sibling: *mut hwloc_obj,
    /// C `unsigned arity` — count of `children`.
    pub arity: c_uint,
    /// C `struct hwloc_obj **children` — `children[0 .. arity-1]`.
    pub children: *mut *mut hwloc_obj,
    pub first_child: *mut hwloc_obj,
    pub last_child: *mut hwloc_obj,
    pub symmetric_subtree: c_int,
    pub memory_arity: c_uint,
    pub memory_first_child: *mut hwloc_obj,
    pub io_arity: c_uint,
    pub io_first_child: *mut hwloc_obj,
    pub misc_arity: c_uint,
    pub misc_first_child: *mut hwloc_obj,
    pub cpuset: hwloc_cpuset_t,
    /// C `hwloc_cpuset_t complete_cpuset` — the object's complete CPU set; the
    /// last field the port reads.
    pub complete_cpuset: hwloc_cpuset_t,
}

// libhwloc functions, signatures confirmed against `bitmap.h` / `helper.h`.
// Only the functions the port actually calls are declared. Intentionally
// omitted: `hwloc_topology_get_complete_cpuset` and `hwloc_get_root_obj` — both
// are called only from `AffinityPanel_new`'s `host->topology` reads, which the
// substrate gap above elides (`hwloc_get_root_obj` is moreover `static
// __hwloc_inline` in `hwloc.h`, not an exported symbol, so it could not be an
// `extern` anyway).
#[cfg(feature = "hwloc")]
#[link(name = "hwloc")]
extern "C" {
    /// `hwloc_bitmap_t hwloc_bitmap_alloc(void);` (`bitmap.h:86`).
    fn hwloc_bitmap_alloc() -> hwloc_bitmap_t;
    /// `void hwloc_bitmap_free(hwloc_bitmap_t);` (`bitmap.h:101`).
    fn hwloc_bitmap_free(bitmap: hwloc_bitmap_t);
    /// `int hwloc_bitmap_copy(hwloc_bitmap_t dst, hwloc_const_bitmap_t src);`
    /// (`bitmap.h:110`).
    fn hwloc_bitmap_copy(dst: hwloc_bitmap_t, src: hwloc_const_bitmap_t) -> c_int;
    /// `int hwloc_bitmap_and(hwloc_bitmap_t res, hwloc_const_bitmap_t, hwloc_const_bitmap_t);`
    /// (`bitmap.h:488`).
    fn hwloc_bitmap_and(
        res: hwloc_bitmap_t,
        bitmap1: hwloc_const_bitmap_t,
        bitmap2: hwloc_const_bitmap_t,
    ) -> c_int;
    /// `int hwloc_bitmap_or(hwloc_bitmap_t res, hwloc_const_bitmap_t, hwloc_const_bitmap_t);`
    /// (`bitmap.h:482`).
    fn hwloc_bitmap_or(
        res: hwloc_bitmap_t,
        bitmap1: hwloc_const_bitmap_t,
        bitmap2: hwloc_const_bitmap_t,
    ) -> c_int;
    /// `int hwloc_bitmap_andnot(hwloc_bitmap_t res, hwloc_const_bitmap_t, hwloc_const_bitmap_t);`
    /// (`bitmap.h:494`).
    fn hwloc_bitmap_andnot(
        res: hwloc_bitmap_t,
        bitmap1: hwloc_const_bitmap_t,
        bitmap2: hwloc_const_bitmap_t,
    ) -> c_int;
    /// `int hwloc_bitmap_set(hwloc_bitmap_t bitmap, unsigned id);` (`bitmap.h:292`).
    fn hwloc_bitmap_set(bitmap: hwloc_bitmap_t, id: c_uint) -> c_int;
    /// `int hwloc_bitmap_weight(hwloc_const_bitmap_t bitmap);` (`bitmap.h:416`).
    fn hwloc_bitmap_weight(bitmap: hwloc_const_bitmap_t) -> c_int;
    /// `int hwloc_bitmap_intersects(hwloc_const_bitmap_t, hwloc_const_bitmap_t);`
    /// (`bitmap.h:519`).
    fn hwloc_bitmap_intersects(
        bitmap1: hwloc_const_bitmap_t,
        bitmap2: hwloc_const_bitmap_t,
    ) -> c_int;
    /// `int hwloc_bitmap_isincluded(hwloc_const_bitmap_t sub, hwloc_const_bitmap_t super);`
    /// (`bitmap.h:527`).
    fn hwloc_bitmap_isincluded(
        sub_bitmap: hwloc_const_bitmap_t,
        super_bitmap: hwloc_const_bitmap_t,
    ) -> c_int;
    /// `int hwloc_bitmap_first(hwloc_const_bitmap_t bitmap);` (`bitmap.h:393`).
    fn hwloc_bitmap_first(bitmap: hwloc_const_bitmap_t) -> c_int;
    /// `int hwloc_bitmap_next(hwloc_const_bitmap_t bitmap, int prev);`
    /// (`bitmap.h:401`).
    fn hwloc_bitmap_next(bitmap: hwloc_const_bitmap_t, prev: c_int) -> c_int;
    /// `const char* hwloc_obj_type_string(hwloc_obj_type_t type);` (`hwloc.h:1096`).
    fn hwloc_obj_type_string(type_: hwloc_obj_type_t) -> *const c_char;
    /// `hwloc_const_cpuset_t hwloc_topology_get_complete_cpuset(hwloc_topology_t);`
    /// (`helper.h`) — the machine's complete cpuset, owned by the topology.
    fn hwloc_topology_get_complete_cpuset(topology: hwloc_topology_t) -> hwloc_const_cpuset_t;
    /// `hwloc_obj_t hwloc_get_obj_by_depth(hwloc_topology_t, int depth, unsigned idx);`
    /// (`hwloc.h:1045`). `hwloc_get_root_obj(topo)` is the header's
    /// `static __hwloc_inline hwloc_get_obj_by_depth(topo, 0, 0)`.
    fn hwloc_get_obj_by_depth(topology: hwloc_topology_t, depth: c_int, idx: c_uint)
        -> hwloc_obj_t;
}

/// Model of the C `MaskItem` struct (`AffinityPanel.c:33`), non-hwloc
/// variant. The C type embeds an `Object super` vtable as its first field;
/// that is expressed by the `impl Object for MaskItem` below instead. `text`
/// is the row label (C `char* text`); `indent` doubles as the tree-node flag
/// (C `char* indent`, NULL when the item is a flat CPU, so `Option<String>`);
/// `value` and `sub_tree` are the C tri-states; `children` is the C
/// `Vector* children` (empty in the non-hwloc build — only the hwloc topology
/// builder populates it); `cpu` is the C `#else`-branch `int cpu`.
pub struct MaskItem {
    pub text: String,
    pub indent: Option<String>,
    pub value: i32,
    pub sub_tree: i32,
    pub children: Vec<MaskItem>,
    /// C `#else` branch: `int cpu` — the flat-CPU index (plain build only).
    #[cfg(not(feature = "hwloc"))]
    pub cpu: i32,
    /// C `#ifdef HAVE_LIBHWLOC` branch: `hwloc_bitmap_t cpuset` — the item's CPU
    /// set (a raw `hwloc_bitmap_t`, freed by [`Drop`] iff `ownCpuset`).
    #[cfg(feature = "hwloc")]
    pub cpuset: hwloc_bitmap_t,
    /// C `bool ownCpuset` — true when this item allocated `cpuset` (and so must
    /// free it); false for a borrowed topology cpuset (`obj->complete_cpuset`).
    #[cfg(feature = "hwloc")]
    pub ownCpuset: bool,
}

/// Port of the `#ifdef HAVE_LIBHWLOC` arm of `MaskItem_delete`
/// (`AffinityPanel.c:55`): `if (this->ownCpuset) hwloc_bitmap_free(this->cpuset);`.
/// Modelled as [`Drop`] (the `History`/`Panel` teardown precedent) so that
/// dropping a `MaskItem` — including recursively dropping the `children`
/// [`Vec`], which is exactly the C `Vector_delete(this->children)` recursion —
/// frees every owned cpuset. Plain build: no `Drop`, matching the C `#else`.
#[cfg(feature = "hwloc")]
impl Drop for MaskItem {
    fn drop(&mut self) {
        if self.ownCpuset {
            // SAFETY: an owned `cpuset` was produced by `hwloc_bitmap_alloc`.
            unsafe { hwloc_bitmap_free(self.cpuset) };
        }
    }
}

/// Port of `static const ObjectClass MaskItem_class` (`AffinityPanel.c:87`).
/// The C initializer sets `.display`/`.delete` but no `.extends`, so
/// `extends` is `NULL` — ported verbatim as `None`. Declared `static` so its
/// address (the type's identity, per `Object_isA`) is stable.
static MaskItem_class: ObjectClass = ObjectClass { extends: None };

impl Object for MaskItem {
    /// C `this->klass` set to `&MaskItem_class`.
    fn klass(&self) -> &'static ObjectClass {
        &MaskItem_class
    }

    /// C vtable slot `.display = MaskItem_display`.
    fn display(&self, out: &mut RichString) {
        MaskItem_display(self, out);
    }
}

/// Port of `static void MaskItem_delete(Object* cast)` from
/// `AffinityPanel.c:48`: `free(text); free(indent); Vector_delete(children);
/// if (ownCpuset) hwloc_bitmap_free(cpuset); free(this);`. Taking `this` by
/// value consumes the item; the owned `text` `String`, `indent`
/// `Option<String>`, and the `children` `Vec<MaskItem>` (whose drop recursively
/// runs each child's teardown — the C's owner-`Vector_delete` recursion) all
/// drop with the struct free. The `#ifdef HAVE_LIBHWLOC` `hwloc_bitmap_free`
/// arm is the `MaskItem` [`Drop`] impl above, which the same drop triggers, so
/// this body is `let _ = this` in both builds.
pub fn MaskItem_delete(this: MaskItem) {
    let _ = this;
}

/// Port of `static void MaskItem_display(const Object* cast, RichString* out)`
/// from `AffinityPanel.c:62`.
///
/// Appends the checkbox (`[x]` for a fully-set item, `[o]` for a partial
/// one, `[ ]` otherwise) using `CRT_colors[CHECK_BOX]`/`CHECK_MARK`, a
/// `CHECK_TEXT` space, then the item `text` in `CHECK_TEXT`. The
/// `if (this->indent)` tree-node branch draws the indent guides in
/// `PROCESS_TREE` plus the open/shut glyph via
/// [`TreeStr::TREE_STR_OPEN`]/[`TreeStr::TREE_STR_SHUT`] (`CRT_treeStr`). That
/// branch is only reachable in a `HAVE_LIBHWLOC` build (only the hwloc-only
/// [`MaskItem_newMask`] sets a non-NULL `indent`), but it is now ported
/// faithfully rather than left a stub.
pub fn MaskItem_display(this: &MaskItem, out: &mut RichString) {
    let check_box = ColorElements::CHECK_BOX.packed(ColorScheme::active());
    let check_mark = ColorElements::CHECK_MARK.packed(ColorScheme::active());
    let check_text = ColorElements::CHECK_TEXT.packed(ColorScheme::active());

    RichString_appendAscii(out, check_box, b"[");
    if this.value == 2 {
        RichString_appendAscii(out, check_mark, b"x");
    } else if this.value == 1 {
        RichString_appendAscii(out, check_mark, b"o");
    } else {
        RichString_appendAscii(out, check_mark, b" ");
    }
    RichString_appendAscii(out, check_box, b"]");
    RichString_appendAscii(out, check_text, b" ");
    if let Some(indent) = &this.indent {
        // C: RichString_appendWide(out, CRT_colors[PROCESS_TREE], this->indent);
        //    RichString_appendWide(out, CRT_colors[PROCESS_TREE],
        //       this->sub_tree == 2 ? CRT_treeStr[TREE_STR_OPEN]
        //                            : CRT_treeStr[TREE_STR_SHUT]);
        //    RichString_appendAscii(out, CRT_colors[CHECK_TEXT], " ");
        let process_tree = ColorElements::PROCESS_TREE.packed(ColorScheme::active());
        RichString_appendWide(out, process_tree, indent.as_bytes());
        let glyph = if this.sub_tree == 2 {
            TreeStr::TREE_STR_OPEN.glyph()
        } else {
            TreeStr::TREE_STR_SHUT.glyph()
        };
        RichString_appendWide(out, process_tree, glyph.as_bytes());
        RichString_appendAscii(out, check_text, b" ");
    }
    RichString_appendWide(out, check_text, this.text.as_bytes());
}

/// Plain build: `MaskItem_newMask` is entirely `#ifdef HAVE_LIBHWLOC`
/// (`AffinityPanel.c:94`), so the default build never calls it — the honest
/// stub is retained unchanged, keeping the default build byte-for-byte identical
/// and the C name present for the port gate. The real body is the
/// `#[cfg(feature = "hwloc")]` version below.
#[cfg(not(feature = "hwloc"))]
pub fn MaskItem_newMask() {
    todo!("port of AffinityPanel.c:94 — hwloc-only (no libhwloc in htoprs)")
}

/// Port of `static MaskItem* MaskItem_newMask(const char* text, const char*
/// indent, hwloc_bitmap_t cpuset, bool owner)` from `AffinityPanel.c:94`
/// (`#ifdef HAVE_LIBHWLOC`).
///
/// The tree-node constructor: `text`/`indent` are `xStrdup`'d (a non-NULL
/// `indent` marks a tree node), `value = 0`, `ownCpuset = owner`, `cpuset` is
/// stored by value, `sub_tree = hwloc_bitmap_weight(cpuset) > 1 ? 1 : 0`, and
/// `children` starts empty. Returns an owned [`MaskItem`] (the C fn
/// heap-allocates and returns a pointer).
#[cfg(feature = "hwloc")]
pub fn MaskItem_newMask(text: &str, indent: &str, cpuset: hwloc_bitmap_t, owner: bool) -> MaskItem {
    MaskItem {
        text: text.to_string(),
        indent: Some(indent.to_string()), // nonnull for tree node
        value: 0,
        // this->sub_tree = hwloc_bitmap_weight(cpuset) > 1 ? 1 : 0;
        sub_tree: if unsafe { hwloc_bitmap_weight(cpuset as hwloc_const_bitmap_t) } > 1 {
            1
        } else {
            0
        },
        children: Vec::new(),
        cpuset,
        ownCpuset: owner,
    }
}

/// Port of `static MaskItem* MaskItem_newSingleton(const char* text, int cpu,
/// bool isSet)` from `AffinityPanel.c:108`, plain (`#else`) branch.
///
/// Builds a flat-CPU item: `text` (C `xStrdup`), `indent = NULL` (not a tree
/// node), `sub_tree = 0`, an empty `children` vector, `cpu = cpu` (the
/// `#else` arm), and `value = isSet ? 2 : 0`. Returns an owned [`MaskItem`]
/// (the C fn heap-allocates and returns a pointer).
#[cfg(not(feature = "hwloc"))]
pub fn MaskItem_newSingleton(text: &str, cpu: i32, isSet: bool) -> MaskItem {
    MaskItem {
        text: text.to_string(),
        indent: None,
        value: if isSet { 2 } else { 0 },
        sub_tree: 0,
        children: Vec::new(),
        cpu,
    }
}

/// Port of `static MaskItem* MaskItem_newSingleton(const char* text, int cpu,
/// bool isSet)` from `AffinityPanel.c:108`, `#ifdef HAVE_LIBHWLOC` branch.
///
/// Same shape as the plain build, but instead of `cpu` it allocates a one-bit
/// cpuset: `ownCpuset = true`, `cpuset = hwloc_bitmap_alloc()`,
/// `hwloc_bitmap_set(cpuset, cpu)`, then `value = isSet ? 2 : 0`.
#[cfg(feature = "hwloc")]
pub fn MaskItem_newSingleton(text: &str, cpu: i32, isSet: bool) -> MaskItem {
    // this->cpuset = hwloc_bitmap_alloc(); hwloc_bitmap_set(this->cpuset, cpu);
    let cpuset = unsafe { hwloc_bitmap_alloc() };
    unsafe { hwloc_bitmap_set(cpuset, cpu as c_uint) };
    MaskItem {
        text: text.to_string(),
        indent: None, // not a tree node
        value: if isSet { 2 } else { 0 },
        sub_tree: 0,
        children: Vec::new(),
        cpuset,
        ownCpuset: true,
    }
}

/// Model of the C `AffinityPanel` struct (`AffinityPanel.c:127`), non-hwloc
/// variant. `super_` is the embedded `Panel super` (`super` is a Rust
/// keyword); `host` is the borrowed `Machine*` (raw pointer — the `Affinity`
/// `host` precedent, never dereferenced by ported code); `topoView` mirrors
/// the C flag (always `false` without hwloc); `cpuids` is the C
/// `Vector* cpuids` of flat-CPU items — an owning [`Vector`] of `MaskItem`
/// (`Box<dyn Object>`), whose element pointers the `super_` [`Panel`] borrows
/// via [`Panel_splice`] (the C's non-owning-panel / owning-`cpuids` shared
/// store); `width` is the computed panel width. The hwloc-only fields
/// (`topoRoot`, `allCpuset`, `workCpuset`) live inside the C's
/// `#ifdef HAVE_LIBHWLOC`, mirrored here behind `#[cfg(feature = "hwloc")]`.
pub struct AffinityPanel {
    pub super_: Panel,
    pub host: *mut Machine,
    pub topoView: bool,
    pub cpuids: Vector,
    pub width: u32,
    /// C `#ifdef HAVE_LIBHWLOC` field `MaskItem* topoRoot` — the root of the
    /// topology tree ([`AffinityPanel_buildTopology`] returns it). Modelled
    /// `Option<Box<MaskItem>>` (C's nullable `MaskItem*`). `None` when
    /// `host->topology` failed to load.
    #[cfg(feature = "hwloc")]
    pub topoRoot: Option<Box<MaskItem>>,
    /// C `hwloc_const_cpuset_t allCpuset` — the machine's complete cpuset (owned
    /// by the topology, so never freed). Null when `host->topology` failed to load.
    #[cfg(feature = "hwloc")]
    pub allCpuset: hwloc_const_cpuset_t,
    /// C `hwloc_bitmap_t workCpuset` — the working selection set the panel edits.
    #[cfg(feature = "hwloc")]
    pub workCpuset: hwloc_bitmap_t,
}

/// Port of `const PanelClass AffinityPanel_class` (`AffinityPanel.c:358`): sets
/// only `.eventHandler = AffinityPanel_eventHandler`; `.drawFunctionBar` /
/// `.printHeader` are NULL and inherit the `Panel` defaults.
impl PanelClass for AffinityPanel {
    fn as_panel(&self) -> &Panel {
        &self.super_
    }
    fn as_panel_mut(&mut self) -> &mut Panel {
        &mut self.super_
    }
    fn event_handler(&mut self, ev: i32) -> HandlerResult {
        AffinityPanel_eventHandler(self, ev)
    }
}

/// Port of `static void AffinityPanel_delete(Object* cast)` from
/// `AffinityPanel.c:141`, plain (`#else`) branch: `Vector_delete(this->cpuids);
/// Panel_done(&this->super); free(this);`. Taking `this` by value consumes the
/// panel; the owning `cpuids` [`Vector`] is handed to [`Vector_delete`] (which
/// drops each `MaskItem` box) and the embedded `super_` [`Panel`] to
/// [`Panel_done`] — the `super_` panel only *borrows* the `cpuids` items, so
/// dropping it first (its `Borrowed` pointers) then the owner is a safe order.
#[cfg(not(feature = "hwloc"))]
pub fn AffinityPanel_delete(this: AffinityPanel) {
    let AffinityPanel { super_, cpuids, .. } = this;
    // C: Panel_done(&this->super) then implicitly the cpuids Vector_delete;
    // the panel holds only Borrowed pointers into cpuids, so releasing it
    // before the owner cannot dangle.
    Panel_done(super_);
    Vector_delete(cpuids);
}

/// Port of `static void AffinityPanel_delete(Object* cast)` from
/// `AffinityPanel.c:141`, `#ifdef HAVE_LIBHWLOC` branch: additionally
/// `hwloc_bitmap_free(this->workCpuset)` and `MaskItem_delete(this->topoRoot)`
/// (which recursively frees the topology tree's owned cpusets via [`Drop`]).
/// `allCpuset` is *not* freed — it is owned by the topology. Same
/// panel-before-owners ordering as the plain build (the panel `Borrowed`s into
/// both `cpuids` and the `topoRoot` tree).
#[cfg(feature = "hwloc")]
pub fn AffinityPanel_delete(this: AffinityPanel) {
    let AffinityPanel {
        super_,
        cpuids,
        workCpuset,
        topoRoot,
        ..
    } = this;
    Panel_done(super_);
    Vector_delete(cpuids);
    // SAFETY: workCpuset came from hwloc_bitmap_alloc in AffinityPanel_new.
    unsafe { hwloc_bitmap_free(workCpuset) };
    if let Some(root) = topoRoot {
        MaskItem_delete(*root);
    }
}

/// Plain build: `AffinityPanel_updateItem` is entirely `#ifdef HAVE_LIBHWLOC`
/// (`AffinityPanel.c:156`); the stub is retained unchanged. Real body below.
#[cfg(not(feature = "hwloc"))]
pub fn AffinityPanel_updateItem() {
    todo!("port of AffinityPanel.c:156 — hwloc-only (no libhwloc in htoprs)")
}

/// Port of `static void AffinityPanel_updateItem(AffinityPanel* this, MaskItem*
/// item)` from `AffinityPanel.c:156` (`#ifdef HAVE_LIBHWLOC`).
///
/// Sets `item->value` to 2 if `item->cpuset` is fully included in `workCpuset`,
/// 1 if it merely intersects it, else 0; then `Panel_add(super, item)`. The
/// panel is non-owning, so the C `Panel_add` here is a *borrowed* append (the
/// item is owned by the `cpuids` [`Vector`] or the `topoRoot` tree), expressed
/// as a [`PanelItem::Borrowed`] push — the same model as [`Panel_splice`]. Takes
/// `item` as a raw `*mut MaskItem` (the C `MaskItem*`), because it aliases an
/// element `this` also owns.
///
/// # Safety
/// `item` must point at a live `MaskItem` owned by `this` (its `cpuids` or
/// `topoRoot`) for the duration of the call.
#[cfg(feature = "hwloc")]
pub fn AffinityPanel_updateItem(this: &mut AffinityPanel, item: *mut MaskItem) {
    // item->value = isincluded ? 2 : intersects ? 1 : 0;
    // SAFETY: item is a live MaskItem owned by `this` (see fn contract).
    unsafe {
        let cpuset = (*item).cpuset as hwloc_const_bitmap_t;
        let work = this.workCpuset as hwloc_const_bitmap_t;
        (*item).value = if hwloc_bitmap_isincluded(cpuset, work) != 0 {
            2
        } else if hwloc_bitmap_intersects(cpuset, work) != 0 {
            1
        } else {
            0
        };
    }

    // Panel_add(super, (Object*) item) on a non-owning panel == borrowed append.
    let obj: *mut dyn Object = item;
    this.super_.items.push(PanelItem::Borrowed(obj));
    this.super_.prevSelected = -1;
    this.super_.needsRedraw = true;
}

/// Plain build: `AffinityPanel_updateTopo` is entirely `#ifdef HAVE_LIBHWLOC`
/// (`AffinityPanel.c:165`); the stub is retained unchanged. Real body below.
#[cfg(not(feature = "hwloc"))]
pub fn AffinityPanel_updateTopo() {
    todo!("port of AffinityPanel.c:165 — hwloc-only (no libhwloc in htoprs)")
}

/// Port of `static void AffinityPanel_updateTopo(AffinityPanel* this, MaskItem*
/// item)` from `AffinityPanel.c:165` (`#ifdef HAVE_LIBHWLOC`).
///
/// `AffinityPanel_updateItem(this, item)`; if the item is collapsed
/// (`sub_tree == 2`), stop; otherwise recurse into every child. Raw `*mut
/// MaskItem` (the C `MaskItem*`), matching the tree pointers `this` owns.
///
/// # Safety
/// `item` must point at a live `MaskItem` (and its subtree) owned by `this`.
#[cfg(feature = "hwloc")]
pub fn AffinityPanel_updateTopo(this: &mut AffinityPanel, item: *mut MaskItem) {
    AffinityPanel_updateItem(this, item);

    // SAFETY: item is a live MaskItem owned by `this` (see fn contract).
    unsafe {
        if (*item).sub_tree == 2 {
            return;
        }
        // for (i = 0; i < Vector_size(item->children); i++)
        //     AffinityPanel_updateTopo(this, Vector_get(item->children, i));
        let n = (*item).children.len();
        for i in 0..n {
            // Element pointer without an intermediate reference (autoref of a
            // raw-pointer deref is denied); the Vec is not resized here.
            let child = (*item).children.as_mut_ptr().add(i);
            AffinityPanel_updateTopo(this, child);
        }
    }
}

/// Port of `static void AffinityPanel_update(AffinityPanel* this, bool
/// keepSelected)` from `AffinityPanel.c:177`, plain (`#else`) branch.
///
/// Re-syncs the panel display with `cpuids`: sets the `F3` label (empty
/// without `topoView`), prunes the panel's borrowed item list, then
/// [`Panel_splice`]s the `cpuids` element pointers back in (the panel borrows,
/// `cpuids` owns), optionally restoring the prior selection. `Panel_setSelected`
/// clamps out-of-range indices, matching the C.
#[cfg(not(feature = "hwloc"))]
pub fn AffinityPanel_update(this: &mut AffinityPanel, keepSelected: bool) {
    // FunctionBar_setLabel(super->currentBar, KEY_F(3),
    //     this->topoView ? "Collapse/Expand" : "");
    if let Some(bar) = &mut this.super_.currentBar {
        FunctionBar_setLabel(
            bar,
            KEY_F(3),
            if this.topoView { "Collapse/Expand" } else { "" },
        );
    }

    let old_selected = Panel_getSelectedIndex(&this.super_);
    Panel_prune(&mut this.super_);

    // #else: Panel_splice(super, this->cpuids);
    Panel_splice(&mut this.super_, &this.cpuids);

    if keepSelected {
        Panel_setSelected(&mut this.super_, old_selected);
    }

    this.super_.needsRedraw = true;
}

/// Port of `static void AffinityPanel_update(AffinityPanel* this, bool
/// keepSelected)` from `AffinityPanel.c:177`, `#ifdef HAVE_LIBHWLOC` branch.
///
/// Sets the `F3` label, prunes, then either walks the topology tree
/// ([`AffinityPanel_updateTopo`] on `topoRoot`) when `topoView` is set, or
/// re-adds each `cpuids` item via [`AffinityPanel_updateItem`]. When
/// `host->topology` failed to load `topoRoot` is `None`, so the topology arm
/// falls through to the `cpuids` loop when it is absent — the one place this
/// port adds a guard the C (which always has a `topoRoot`) does not.
#[cfg(feature = "hwloc")]
pub fn AffinityPanel_update(this: &mut AffinityPanel, keepSelected: bool) {
    if let Some(bar) = &mut this.super_.currentBar {
        FunctionBar_setLabel(
            bar,
            KEY_F(3),
            if this.topoView { "Collapse/Expand" } else { "" },
        );
    }

    let old_selected = Panel_getSelectedIndex(&this.super_);
    Panel_prune(&mut this.super_);

    // C: if (topoView) updateTopo(this, topoRoot); else for cpuids: updateItem.
    let root = this.topoRoot.as_deref_mut().map(|r| r as *mut MaskItem);
    if this.topoView && root.is_some() {
        AffinityPanel_updateTopo(this, root.unwrap());
    } else {
        // for (i = 0; i < Vector_size(this->cpuids); i++)
        //     AffinityPanel_updateItem(this, Vector_get(this->cpuids, i));
        let n = Vector_size(&this.cpuids);
        for i in 0..n {
            let item = (Vector_get(&this.cpuids, i as usize) as &dyn core::any::Any)
                .downcast_ref::<MaskItem>()
                .expect("cpuids holds MaskItem") as *const MaskItem
                as *mut MaskItem;
            AffinityPanel_updateItem(this, item);
        }
    }

    if keepSelected {
        Panel_setSelected(&mut this.super_, old_selected);
    }

    this.super_.needsRedraw = true;
}

/// Port of `static HandlerResult AffinityPanel_eventHandler(Panel* super,
/// int ch)` from `AffinityPanel.c:203`, plain (`#else`) branch.
///
/// On mouse click / re-click / space, toggles the selected item's `value`
/// between 0 and 2 (`selected->value ? 0 : 2`); Enter breaks the picker loop.
/// The toggle mutates the item through the panel's `Borrowed` pointer, which
/// aliases the `cpuids`-owned `MaskItem` (so [`AffinityPanel_getAffinity`] sees
/// it). A `HANDLED` result re-runs [`AffinityPanel_update`] (keeping the
/// selection). The hwloc-only `F1`/`F2`/`F3`/`+`/`-` cases are compiled out.
#[cfg(not(feature = "hwloc"))]
pub fn AffinityPanel_eventHandler(this: &mut AffinityPanel, ch: i32) -> HandlerResult {
    let mut result = HandlerResult::IGNORED;
    let keep_selected = true;

    // MaskItem* selected = (MaskItem*) Panel_getSelected(super);
    let sel = Panel_getSelectedIndex(&this.super_);

    // ' ' is 0x20; KEY_MOUSE / KEY_RECLICK share this arm.
    if ch == KEY_MOUSE || ch == KEY_RECLICK || ch == b' ' as i32 {
        // if (!selected) return result; (IGNORED)
        if sel < 0 || sel as usize >= this.super_.items.len() {
            return result;
        }
        // #else: selected->value = selected->value ? 0 : 2;
        let obj = this.super_.items[sel as usize].object_mut();
        let item = (obj as &mut dyn core::any::Any)
            .downcast_mut::<MaskItem>()
            .expect("AffinityPanel item is a MaskItem");
        item.value = if item.value != 0 { 0 } else { 2 };
        result = HandlerResult::HANDLED;
    } else if ch == 0x0a || ch == 0x0d || ch == KEY_ENTER {
        result = HandlerResult::BREAK_LOOP;
    }

    // if (HANDLED == result) AffinityPanel_update(this, keepSelected);
    if result == HandlerResult::HANDLED {
        AffinityPanel_update(this, keep_selected);
    }

    result
}

/// Port of `static HandlerResult AffinityPanel_eventHandler(Panel* super,
/// int ch)` from `AffinityPanel.c:203`, `#ifdef HAVE_LIBHWLOC` branch.
///
/// Space/click toggles the selected item's whole cpuset in/out of `workCpuset`
/// (`hwloc_bitmap_andnot`/`hwloc_bitmap_or`); `F1` copies `allCpuset` into
/// `workCpuset` ("All"); `F2` flips `topoView` (not keeping the selection);
/// `F3`/`-`/`+` toggle the selected node's `sub_tree` collapse state. Enter
/// breaks the picker loop. A `HANDLED` result re-runs [`AffinityPanel_update`].
#[cfg(feature = "hwloc")]
pub fn AffinityPanel_eventHandler(this: &mut AffinityPanel, ch: i32) -> HandlerResult {
    let mut result = HandlerResult::IGNORED;
    let mut keep_selected = true;

    // MaskItem* selected = (MaskItem*) Panel_getSelected(super);
    let sel = Panel_getSelectedIndex(&this.super_);
    let selected: *mut MaskItem = if sel >= 0 && (sel as usize) < this.super_.items.len() {
        (this.super_.items[sel as usize].object_mut() as &mut dyn core::any::Any)
            .downcast_mut::<MaskItem>()
            .expect("AffinityPanel item is a MaskItem") as *mut MaskItem
    } else {
        core::ptr::null_mut()
    };

    if ch == KEY_MOUSE || ch == KEY_RECLICK || ch == b' ' as i32 {
        // if (!selected) return result;
        if selected.is_null() {
            return result;
        }
        // SAFETY: selected points at a live panel MaskItem (bounds-checked above).
        unsafe {
            if (*selected).value == 2 {
                // remove this mask from the top cpuset
                hwloc_bitmap_andnot(
                    this.workCpuset,
                    this.workCpuset as hwloc_const_bitmap_t,
                    (*selected).cpuset as hwloc_const_bitmap_t,
                );
                (*selected).value = 0;
            } else {
                // set all bits from this object in the top cpuset
                hwloc_bitmap_or(
                    this.workCpuset,
                    this.workCpuset as hwloc_const_bitmap_t,
                    (*selected).cpuset as hwloc_const_bitmap_t,
                );
                (*selected).value = 2;
            }
        }
        result = HandlerResult::HANDLED;
    } else if ch == KEY_F(1) {
        // hwloc_bitmap_copy(this->workCpuset, this->allCpuset);
        unsafe { hwloc_bitmap_copy(this.workCpuset, this.allCpuset) };
        result = HandlerResult::HANDLED;
    } else if ch == KEY_F(2) {
        this.topoView = !this.topoView;
        keep_selected = false;
        result = HandlerResult::HANDLED;
    } else if ch == KEY_F(3) || ch == b'-' as i32 || ch == b'+' as i32 {
        // if (!selected) break; (leaves result IGNORED — no update)
        if !selected.is_null() {
            // SAFETY: selected points at a live panel MaskItem.
            unsafe {
                if (*selected).sub_tree != 0 {
                    // selected->sub_tree = 1 + !(selected->sub_tree - 1);
                    // toggles between 1 and 2.
                    (*selected).sub_tree = 1 + if (*selected).sub_tree - 1 != 0 { 0 } else { 1 };
                }
            }
            result = HandlerResult::HANDLED;
        }
    } else if ch == 0x0a || ch == 0x0d || ch == KEY_ENTER {
        result = HandlerResult::BREAK_LOOP;
    }

    // if (HANDLED == result) AffinityPanel_update(this, keepSelected);
    if result == HandlerResult::HANDLED {
        AffinityPanel_update(this, keep_selected);
    }

    result
}

/// Plain build: `AffinityPanel_addObject` is entirely `#ifdef HAVE_LIBHWLOC`
/// (`AffinityPanel.c:283`); the stub is retained unchanged. Real body below.
#[cfg(not(feature = "hwloc"))]
pub fn AffinityPanel_addObject() {
    todo!("port of AffinityPanel.c:283 — hwloc-only (no libhwloc in htoprs)")
}

/// Port of `static MaskItem* AffinityPanel_addObject(AffinityPanel* this,
/// hwloc_obj_t obj, unsigned indent, MaskItem* parent)` from
/// `AffinityPanel.c:283` (`#ifdef HAVE_LIBHWLOC`).
///
/// Builds one [`MaskItem`] for `obj`: the label is `"<type> #<index>"` (for a
/// `HWLOC_OBJ_PU` it is `"CPU <cpuId>"` with no `#`), the indent guides are
/// drawn from the `indent` bitmask + `obj->next_sibling`, and the collapse
/// heuristic sets `sub_tree = 2` when the object is fully in or fully out of
/// `workCpuset`. Returns a raw `*mut MaskItem` into wherever the item is stored:
/// pushed into `parent->children` when `parent` is non-null, else boxed as the
/// standalone root (ownership handed to the caller via [`Box::into_raw`]). The C
/// `Vector_add(parent->children, item)` maps to `Vec::push` because the ported
/// `children` is a `Vec<MaskItem>`.
///
/// # Safety
/// `obj` must be a live `hwloc_obj`; `parent`, if non-null, a live `MaskItem`
/// whose `children` outlive the returned pointer.
#[cfg(feature = "hwloc")]
#[allow(clippy::manual_c_str_literals)]
pub fn AffinityPanel_addObject(
    this: &mut AffinityPanel,
    obj: hwloc_obj_t,
    indent: c_uint,
    parent: *mut MaskItem,
) -> *mut MaskItem {
    // SAFETY: obj is a live hwloc_obj (see fn contract).
    let obj_ref: &hwloc_obj = unsafe { &*obj };

    // const char* type_name = hwloc_obj_type_string(obj->type);
    let type_name_c = unsafe { hwloc_obj_type_string(obj_ref.type_) };
    // SAFETY: hwloc_obj_type_string returns a static NUL-terminated string.
    let mut type_name: String = unsafe { CStr::from_ptr(type_name_c) }
        .to_string_lossy()
        .into_owned();
    let mut index_prefix = "#";
    let depth: c_uint = obj_ref.depth as c_uint; // C: unsigned depth = obj->depth;
    let mut index: c_uint = obj_ref.logical_index;

    if obj_ref.type_ == HWLOC_OBJ_PU {
        // index = Settings_cpuId(this->host->settings, obj->os_index);
        //   == countCPUsFromOne ? os_index + 1 : os_index
        let count_from_one = unsafe { &*this.host }
            .settings
            .as_ref()
            .is_some_and(|s| s.countCPUsFromOne);
        index = if count_from_one {
            obj_ref.os_index + 1
        } else {
            obj_ref.os_index
        };
        type_name = "CPU".to_string();
        index_prefix = "";
    }

    // Build indent_buf: for depth>0, one "<guide>  " per level 1..depth, then
    // the RTEE/BEND joint. CRT_treeStr[TREE_STR_VERT/RTEE/BEND] == glyphs.
    let mut indent_buf = String::new();
    if depth > 0 {
        for i in 1..depth {
            let guide = if indent & (1u32 << i) != 0 {
                TreeStr::TREE_STR_VERT.glyph()
            } else {
                " "
            };
            indent_buf.push_str(guide);
            indent_buf.push_str("  ");
        }
        let joint = if !obj_ref.next_sibling.is_null() {
            TreeStr::TREE_STR_RTEE.glyph()
        } else {
            TreeStr::TREE_STR_BEND.glyph()
        };
        indent_buf.push_str(joint);
    }

    // xSnprintf(buf, sizeof(buf), "%s %s%u", type_name, index_prefix, index);
    let buf = format!("{type_name} {index_prefix}{index}");

    let mut item = MaskItem_newMask(&buf, &indent_buf, obj_ref.complete_cpuset, false);

    // if (item->sub_tree && parent && parent->sub_tree == 1) { collapse test }
    // SAFETY: parent, if non-null, is a live MaskItem (see fn contract).
    let parent_sub_tree_1 = !parent.is_null() && unsafe { (*parent).sub_tree } == 1;
    if item.sub_tree != 0 && parent_sub_tree_1 {
        // SAFETY: bitmap handles are live for the duration of the call.
        unsafe {
            let result = hwloc_bitmap_alloc();
            hwloc_bitmap_and(
                result,
                obj_ref.complete_cpuset as hwloc_const_bitmap_t,
                this.workCpuset as hwloc_const_bitmap_t,
            );
            let weight = hwloc_bitmap_weight(result as hwloc_const_bitmap_t);
            hwloc_bitmap_free(result);
            if weight == 0
                || weight
                    == hwloc_bitmap_weight(this.workCpuset as hwloc_const_bitmap_t)
                        + hwloc_bitmap_weight(obj_ref.complete_cpuset as hwloc_const_bitmap_t)
            {
                item.sub_tree = 2;
            }
        }
    }

    // "[x] " + "|- " * depth + ("- ")?(root) + name
    // unsigned int indent_width = 4 + 3 * depth + (2 * !depth);
    let indent_width = 4 + 3 * depth + (2 * u32::from(depth == 0));
    let width = indent_width + buf.len() as u32;
    if width > this.width {
        this.width = width;
    }

    // if (parent) Vector_add(parent->children, item);  — else the standalone
    // root, boxed and handed to the caller (buildTopology stores it in topoRoot).
    if !parent.is_null() {
        // SAFETY: parent is a live MaskItem whose children outlive the return.
        unsafe {
            (*parent).children.push(item);
            (*parent).children.last_mut().unwrap() as *mut MaskItem
        }
    } else {
        Box::into_raw(Box::new(item))
    }
}

/// Plain build: `AffinityPanel_buildTopology` is entirely `#ifdef HAVE_LIBHWLOC`
/// (`AffinityPanel.c:341`); the stub is retained unchanged. Real body below.
#[cfg(not(feature = "hwloc"))]
pub fn AffinityPanel_buildTopology() {
    todo!("port of AffinityPanel.c:341 — hwloc-only (no libhwloc in htoprs)")
}

/// Port of `static MaskItem* AffinityPanel_buildTopology(AffinityPanel* this,
/// hwloc_obj_t obj, unsigned indent, MaskItem* parent)` from
/// `AffinityPanel.c:341` (`#ifdef HAVE_LIBHWLOC`).
///
/// Adds `obj` via [`AffinityPanel_addObject`], threads the `indent` bitmask for
/// this depth (set the bit if `obj` has a next sibling, else clear it), and
/// recurses into `obj->children[0..arity]`. Returns the root item pointer when
/// `parent` is null (the top-level call), else null — matching the C
/// `parent == NULL ? item : NULL`.
///
/// # Safety
/// `obj` must be a live `hwloc_obj` tree; `parent`, if non-null, a live
/// `MaskItem`.
#[cfg(feature = "hwloc")]
pub fn AffinityPanel_buildTopology(
    this: &mut AffinityPanel,
    obj: hwloc_obj_t,
    indent: c_uint,
    parent: *mut MaskItem,
) -> *mut MaskItem {
    let item = AffinityPanel_addObject(this, obj, indent, parent);

    // SAFETY: obj is a live hwloc_obj (see fn contract).
    let obj_ref: &hwloc_obj = unsafe { &*obj };
    // if (obj->next_sibling) indent |= (1U << obj->depth); else indent &= ~(1U << obj->depth);
    let mut indent = indent;
    let bit = 1u32 << (obj_ref.depth as c_uint);
    if !obj_ref.next_sibling.is_null() {
        indent |= bit;
    } else {
        indent &= !bit;
    }

    // for (i = 0; i < obj->arity; i++)
    //     AffinityPanel_buildTopology(this, obj->children[i], indent, item);
    let arity = obj_ref.arity;
    for i in 0..arity {
        // SAFETY: children[0..arity] are valid hwloc_obj pointers.
        let child = unsafe { *obj_ref.children.add(i as usize) };
        AffinityPanel_buildTopology(this, child, indent, item);
    }

    if parent.is_null() {
        item
    } else {
        core::ptr::null_mut()
    }
}

/// Port of `static const char* const AffinityPanelFunctions[]`
/// (`AffinityPanel.c:366`), plain variant. The `"All"`/`"Topology"`/blank
/// entries are `#ifdef HAVE_LIBHWLOC`; only `"Set"`/`"Cancel"` remain (the C
/// `NULL` terminator is the slice length here).
#[cfg(not(feature = "hwloc"))]
static AffinityPanelFunctions: [&str; 2] = ["Set    ", "Cancel "];

/// Port of `static const char* const AffinityPanelFunctions[]`
/// (`AffinityPanel.c:366`), `#ifdef HAVE_LIBHWLOC` variant: the `"All"`
/// (`F1`), `"Topology"` (`F2`), and blank (`F3`) labels are added.
#[cfg(feature = "hwloc")]
static AffinityPanelFunctions: [&str; 5] =
    ["Set    ", "Cancel ", "All", "Topology", "               "];

/// Port of `static const char* const AffinityPanelKeys[]` (`AffinityPanel.c:376`).
static AffinityPanelKeys: [&str; 5] = ["Enter", "Esc", "F1", "F2", "F3"];

/// Port of `static const int AffinityPanelEvents[]` (`AffinityPanel.c:377`).
/// `13`/`27` are Enter/Esc; the `F1`/`F2`/`F3` events pair with the hwloc-only
/// labels, so in the non-hwloc build `FunctionBar_new` binds only the first two
/// (it stops at the 2-entry `AffinityPanelFunctions`).
static AffinityPanelEvents: [i32; 5] = [13, 27, KEY_F(1), KEY_F(2), KEY_F(3)];

/// Port of `Panel* AffinityPanel_new(Machine* host, const Affinity* affinity,
/// int* width)` from `AffinityPanel.c:379`, plain (`#else`) branch.
///
/// Builds one [`MaskItem_newSingleton`] per online CPU into the owning `cpuids`
/// [`Vector`], marking each set iff it appears in `affinity` (whose entries are
/// sorted ascending, walked by `curCpu`), tracks the widest label into `width`,
/// then [`AffinityPanel_update`]s to splice the items into the panel. Returned
/// by value (the C `AllocThis` heap panel; the `ColumnsPanel_new` precedent) —
/// the panel's `Borrowed` item pointers target the `cpuids` boxes' heap
/// allocations, which survive the move. The `Class(MaskItem)`/`owner` args to
/// the C `Panel_init` only typed its `Vector` and are dropped, as in every
/// sibling `_new`.
#[cfg(not(feature = "hwloc"))]
pub fn AffinityPanel_new(
    host: *mut Machine,
    affinity: &Affinity,
    width: Option<&mut i32>,
) -> AffinityPanel {
    // Panel_init(super, 1,1,1,1, Class(MaskItem), false,
    //     FunctionBar_new(AffinityPanelFunctions, AffinityPanelKeys, AffinityPanelEvents));
    let fu_bar = FunctionBar_new(
        Some(&AffinityPanelFunctions[..]),
        Some(&AffinityPanelKeys[..]),
        Some(&AffinityPanelEvents[..]),
    );
    let super_ = Panel_new(1, 1, 1, 1, Some(fu_bar));

    let mut this = AffinityPanel {
        super_,
        host,            // this->host = host;
        topoView: false, // #else: this->topoView = false;
        cpuids: Vector_new(&MaskItem_class, true, VECTOR_DEFAULT_SIZE),
        width: 14, // this->width = 14;
    };

    Panel_setHeader(&mut this.super_, "Use CPUs:");

    // Settings_cpuId(settings, cpu) == countCPUsFromOne ? cpu+1 : cpu (macro).
    // SAFETY: host is a live Machine* (C precondition); settings is set.
    let count_from_one = unsafe { &*host }
        .settings
        .as_ref()
        .is_some_and(|s| s.countCPUsFromOne);
    let existing = unsafe { &*host }.existingCPUs;

    let mut cur_cpu: u32 = 0;
    for i in 0..existing {
        // if (!Machine_isCPUonline(host, i)) continue; — the reader is
        // per-platform (Linux takes a LinuxMachine), so select it by cfg.
        #[cfg(target_os = "macos")]
        let online =
            crate::ported::darwin::darwinmachine::Machine_isCPUonline(unsafe { &*host }, i);
        #[cfg(not(target_os = "macos"))]
        let online = crate::ported::linux::linuxmachine::Machine_isCPUonline(
            unsafe { &*(host as *const crate::ported::linux::linuxmachine::LinuxMachine) },
            i,
        );
        if !online {
            continue;
        }

        // xSnprintf(number, 9, "CPU %d", Settings_cpuId(host->settings, i));
        let cpu_id = if count_from_one { i + 1 } else { i };
        let number = format!("CPU {cpu_id}");
        // cpu_width = 4 + strlen(number);
        let cpu_width = 4 + number.len() as u32;
        if cpu_width > this.width {
            this.width = cpu_width;
        }

        // isSet = curCpu < affinity->used && affinity->cpus[curCpu] == i;
        let is_set = cur_cpu < affinity.used && affinity.cpus[cur_cpu as usize] == i;
        if is_set {
            cur_cpu += 1;
        }

        // MaskItem* cpuItem = MaskItem_newSingleton(number, i, isSet);
        // Vector_add(this->cpuids, (Object*) cpuItem);
        Vector_add(
            &mut this.cpuids,
            Box::new(MaskItem_newSingleton(&number, i as i32, is_set)),
        );
    }

    // if (width) *width = this->width;
    if let Some(w) = width {
        *w = this.width as i32;
    }

    // AffinityPanel_update(this, false);
    AffinityPanel_update(&mut this, false);

    this
}

/// Port of `Panel* AffinityPanel_new(Machine* host, const Affinity* affinity,
/// int* width)` from `AffinityPanel.c:379`, `#ifdef HAVE_LIBHWLOC` branch.
///
/// Same flat-CPU build as the plain branch, but `topoView` comes from
/// `host->settings->topologyAffinity`, `workCpuset` is allocated, and each set
/// CPU is also marked in `workCpuset`. The C additionally seeds
/// `allCpuset = hwloc_topology_get_complete_cpuset(host->topology)` and
/// `topoRoot = AffinityPanel_buildTopology(hwloc_get_root_obj(host->topology),
/// …)`. Both read the ported `Machine`'s `#[cfg(feature = "hwloc")] topology`
/// field (loaded by `Machine_init`); when it failed to load `allCpuset` stays
/// null and `topoRoot` `None`.
#[cfg(feature = "hwloc")]
pub fn AffinityPanel_new(
    host: *mut Machine,
    affinity: &Affinity,
    width: Option<&mut i32>,
) -> AffinityPanel {
    let fu_bar = FunctionBar_new(
        Some(&AffinityPanelFunctions[..]),
        Some(&AffinityPanelKeys[..]),
        Some(&AffinityPanelEvents[..]),
    );
    let super_ = Panel_new(1, 1, 1, 1, Some(fu_bar));

    // this->topoView = host->settings->topologyAffinity;
    // SAFETY: host is a live Machine* (C precondition); settings is set.
    let topo_view = unsafe { &*host }
        .settings
        .as_ref()
        .is_some_and(|s| s.topologyAffinity);
    // this->allCpuset = hwloc_topology_get_complete_cpuset(host->topology);
    // (`host->topology` is the handle `Machine_init` loaded; `None` only if
    // `hwloc_topology_init` itself failed, where the C would read a garbage
    // handle — the port yields a null cpuset instead.)
    let all_cpuset = match unsafe { (*host).topology } {
        Some(topo) => unsafe { hwloc_topology_get_complete_cpuset(topo) },
        None => core::ptr::null(),
    };
    // this->workCpuset = hwloc_bitmap_alloc();
    let work_cpuset = unsafe { hwloc_bitmap_alloc() };

    let mut this = AffinityPanel {
        super_,
        host,
        topoView: topo_view,
        cpuids: Vector_new(&MaskItem_class, true, VECTOR_DEFAULT_SIZE),
        width: 14,
        allCpuset: all_cpuset,
        workCpuset: work_cpuset,
        // this->topoRoot set after the cpuids loop (needs `this`), matching the C.
        topoRoot: None,
    };

    Panel_setHeader(&mut this.super_, "Use CPUs:");

    let count_from_one = unsafe { &*host }
        .settings
        .as_ref()
        .is_some_and(|s| s.countCPUsFromOne);
    let existing = unsafe { &*host }.existingCPUs;

    let mut cur_cpu: u32 = 0;
    for i in 0..existing {
        #[cfg(target_os = "macos")]
        let online =
            crate::ported::darwin::darwinmachine::Machine_isCPUonline(unsafe { &*host }, i);
        #[cfg(not(target_os = "macos"))]
        let online = crate::ported::linux::linuxmachine::Machine_isCPUonline(
            unsafe { &*(host as *const crate::ported::linux::linuxmachine::LinuxMachine) },
            i,
        );
        if !online {
            continue;
        }

        let cpu_id = if count_from_one { i + 1 } else { i };
        let number = format!("CPU {cpu_id}");
        let cpu_width = 4 + number.len() as u32;
        if cpu_width > this.width {
            this.width = cpu_width;
        }

        let is_set = cur_cpu < affinity.used && affinity.cpus[cur_cpu as usize] == i;
        if is_set {
            // #ifdef HAVE_LIBHWLOC: hwloc_bitmap_set(this->workCpuset, i);
            unsafe { hwloc_bitmap_set(this.workCpuset, i as c_uint) };
            cur_cpu += 1;
        }

        Vector_add(
            &mut this.cpuids,
            Box::new(MaskItem_newSingleton(&number, i as i32, is_set)),
        );
    }

    // this->topoRoot = AffinityPanel_buildTopology(this, hwloc_get_root_obj(host->topology), 0, NULL);
    // `buildTopology` `Box::into_raw`s the root (parent == NULL branch of
    // `addObject`), so the returned `*mut MaskItem` is reclaimed here into the
    // owning `topoRoot` box (its `children` tree drops recursively with it).
    if let Some(topo) = unsafe { (*host).topology } {
        let root = AffinityPanel_buildTopology(
            &mut this,
            unsafe { hwloc_get_obj_by_depth(topo, 0, 0) },
            0,
            core::ptr::null_mut(),
        );
        if !root.is_null() {
            this.topoRoot = Some(unsafe { Box::from_raw(root) });
        }
    }

    if let Some(w) = width {
        *w = this.width as i32;
    }

    AffinityPanel_update(&mut this, false);

    this
}

/// Port of `Affinity* AffinityPanel_getAffinity(Panel* super, Machine* host)`
/// from `AffinityPanel.c:444`, plain (`#else`) branch.
///
/// Allocates a fresh [`Affinity`] for `host`, then for every `cpuids` item
/// whose `value` is set (non-zero), adds that item's `cpu` to the affinity
/// set. The C casts `Panel* super` to `AffinityPanel*`; the faithful analog
/// takes `this: &AffinityPanel` directly. `item->cpu` is a non-negative CPU
/// index; the C passes it to `Affinity_add(unsigned int)`, so it is widened
/// to `u32`. Returns an owned [`Affinity`] (the C fn returns a pointer).
#[cfg(not(feature = "hwloc"))]
pub fn AffinityPanel_getAffinity(this: &AffinityPanel, host: *mut Machine) -> Affinity {
    let mut affinity = Affinity_new(host);
    // for (i = 0; i < Vector_size(this->cpuids); i++) { item = Vector_get(...) }
    for i in 0..Vector_size(&this.cpuids) {
        let item = (Vector_get(&this.cpuids, i as usize) as &dyn core::any::Any)
            .downcast_ref::<MaskItem>()
            .expect("cpuids holds MaskItem");
        if item.value != 0 {
            Affinity_add(&mut affinity, item.cpu as u32);
        }
    }
    affinity
}

/// Port of `Affinity* AffinityPanel_getAffinity(Panel* super, Machine* host)`
/// from `AffinityPanel.c:444`, `#ifdef HAVE_LIBHWLOC` branch.
///
/// Iterates every bit set in `workCpuset` and `Affinity_add`s it. The C uses
/// the `hwloc_bitmap_foreach_begin/end` *macro* pair (not functions); it is
/// reimplemented here with `hwloc_bitmap_first`/`hwloc_bitmap_next`, matching
/// the macro's `for (id = first; id != -1; id = next(bm, id))` semantics.
#[cfg(feature = "hwloc")]
pub fn AffinityPanel_getAffinity(this: &AffinityPanel, host: *mut Machine) -> Affinity {
    let mut affinity = Affinity_new(host);
    // hwloc_bitmap_foreach_begin(i, this->workCpuset) { Affinity_add(affinity, i); }
    let set = this.workCpuset as hwloc_const_bitmap_t;
    // SAFETY: workCpuset is a live bitmap allocated in AffinityPanel_new.
    unsafe {
        let mut i = hwloc_bitmap_first(set);
        while i != -1 {
            Affinity_add(&mut affinity, i as u32);
            i = hwloc_bitmap_next(set, i);
        }
    }
    affinity
}

#[cfg(all(test, not(feature = "hwloc")))]
mod tests {
    use super::*;
    use crate::ported::panel::Panel_new;

    /// Visible characters of the valid `[0, chlen)` range.
    fn rendered(rs: &RichString) -> String {
        rs.chptr
            .iter()
            .take(rs.chlen as usize)
            .map(|c| c.chars)
            .collect()
    }

    // ── MaskItem_newSingleton ─────────────────────────────────────────

    #[test]
    fn new_singleton_set_produces_value_two() {
        let it = MaskItem_newSingleton("CPU 3", 3, true);
        assert_eq!(it.text, "CPU 3");
        assert_eq!(it.cpu, 3);
        assert_eq!(it.value, 2); // isSet -> 2
        assert_eq!(it.sub_tree, 0);
        assert!(it.indent.is_none()); // flat CPU, not a tree node
        assert!(it.children.is_empty());
    }

    #[test]
    fn new_singleton_unset_produces_value_zero() {
        let it = MaskItem_newSingleton("CPU 0", 0, false);
        assert_eq!(it.value, 0); // !isSet -> 0
        assert_eq!(it.cpu, 0);
    }

    // ── MaskItem_display ──────────────────────────────────────────────

    #[test]
    fn display_full_set_draws_x_checkbox() {
        let it = MaskItem_newSingleton("CPU 1", 1, true); // value 2
        let mut rs = RichString::new();
        MaskItem_display(&it, &mut rs);
        // "[" + "x" + "]" + " " + "CPU 1"
        assert_eq!(rendered(&rs), "[x] CPU 1");
    }

    #[test]
    fn display_unset_draws_blank_checkbox() {
        let it = MaskItem_newSingleton("CPU 2", 2, false); // value 0
        let mut rs = RichString::new();
        MaskItem_display(&it, &mut rs);
        assert_eq!(rendered(&rs), "[ ] CPU 2");
    }

    #[test]
    fn display_partial_draws_o_checkbox() {
        // value == 1 only arises from the hwloc updateItem path; construct
        // it directly to exercise the middle branch of MaskItem_display.
        let it = MaskItem {
            text: "Core".to_string(),
            indent: None,
            value: 1,
            sub_tree: 0,
            children: Vec::new(),
            cpu: 0,
        };
        let mut rs = RichString::new();
        MaskItem_display(&it, &mut rs);
        assert_eq!(rendered(&rs), "[o] Core");
    }

    #[test]
    fn display_tree_node_draws_indent_and_openshut_glyph() {
        // A non-NULL indent marks a tree node (only set in a HAVE_LIBHWLOC
        // build). The branch draws "[<box>] <indent><glyph> <text>", where the
        // glyph is CRT_treeStr[TREE_STR_OPEN] when sub_tree == 2 else
        // TREE_STR_SHUT. Compare against the same table so the assertion holds
        // in both the ASCII and UTF-8 glyph modes.
        let shut = MaskItem {
            text: "Package".to_string(),
            indent: Some("|- ".to_string()),
            value: 2,    // [x]
            sub_tree: 1, // != 2 -> SHUT
            children: Vec::new(),
            cpu: 0,
        };
        let mut rs = RichString::new();
        MaskItem_display(&shut, &mut rs);
        assert_eq!(
            rendered(&rs),
            format!("[x] |- {} Package", TreeStr::TREE_STR_SHUT.glyph())
        );

        let open = MaskItem {
            sub_tree: 2, // -> OPEN
            ..shut
        };
        let mut rs = RichString::new();
        MaskItem_display(&open, &mut rs);
        assert_eq!(
            rendered(&rs),
            format!("[x] |- {} Package", TreeStr::TREE_STR_OPEN.glyph())
        );
    }

    // ── AffinityPanel_getAffinity ─────────────────────────────────────

    fn panel_with_cpuids(cpuids: Vec<MaskItem>) -> AffinityPanel {
        // cpuids is the owning Vector (as AffinityPanel_new builds it).
        let mut v = Vector_new(&MaskItem_class, true, cpuids.len() as core::ffi::c_int);
        for it in cpuids {
            Vector_add(&mut v, Box::new(it));
        }
        AffinityPanel {
            super_: Panel_new(1, 1, 1, 1, None),
            host: core::ptr::null_mut(),
            topoView: false,
            cpuids: v,
            width: 14,
        }
    }

    // ── AffinityPanel_update / _eventHandler ──────────────────────────

    #[test]
    fn update_splices_cpuids_into_the_panel() {
        let mut ap = panel_with_cpuids(vec![
            MaskItem_newSingleton("CPU 0", 0, true),
            MaskItem_newSingleton("CPU 1", 1, false),
        ]);
        // The panel starts empty; update splices the cpuids' items in (as
        // borrowed pointers) and they render their current state.
        AffinityPanel_update(&mut ap, false);
        assert_eq!(ap.super_.items.len(), 2);
        let mut rs = RichString::new();
        ap.super_.items[0].object().display(&mut rs);
        assert_eq!(rendered(&rs), "[x] CPU 0");
    }

    #[test]
    fn eventhandler_space_toggles_selected_visible_through_cpuids() {
        let mut ap = panel_with_cpuids(vec![
            MaskItem_newSingleton("CPU 0", 0, false), // value 0
            MaskItem_newSingleton("CPU 1", 1, false),
        ]);
        AffinityPanel_update(&mut ap, false);
        Panel_setSelected(&mut ap.super_, 0);

        // Space toggles item 0's value 0 -> 2; HANDLED re-runs update. Because
        // the panel item aliases the cpuids-owned MaskItem, getAffinity (which
        // reads cpuids) sees the change — the whole point of the shared store.
        let r = AffinityPanel_eventHandler(&mut ap, b' ' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        let aff = AffinityPanel_getAffinity(&ap, core::ptr::null_mut());
        assert_eq!(aff.used, 1);
        assert_eq!(aff.cpus[0], 0);

        // Toggling the same item again clears it (2 -> 0).
        Panel_setSelected(&mut ap.super_, 0);
        assert_eq!(
            AffinityPanel_eventHandler(&mut ap, b' ' as i32),
            HandlerResult::HANDLED
        );
        assert_eq!(
            AffinityPanel_getAffinity(&ap, core::ptr::null_mut()).used,
            0
        );
    }

    #[test]
    fn eventhandler_enter_breaks_loop_without_toggling() {
        let mut ap = panel_with_cpuids(vec![MaskItem_newSingleton("CPU 0", 0, false)]);
        AffinityPanel_update(&mut ap, false);
        Panel_setSelected(&mut ap.super_, 0);
        assert_eq!(
            AffinityPanel_eventHandler(&mut ap, KEY_ENTER),
            HandlerResult::BREAK_LOOP
        );
        assert_eq!(
            AffinityPanel_getAffinity(&ap, core::ptr::null_mut()).used,
            0
        );
    }

    #[test]
    fn eventhandler_unhandled_key_is_ignored() {
        let mut ap = panel_with_cpuids(vec![MaskItem_newSingleton("CPU 0", 0, false)]);
        AffinityPanel_update(&mut ap, false);
        assert_eq!(
            AffinityPanel_eventHandler(&mut ap, b'z' as i32),
            HandlerResult::IGNORED
        );
    }

    // ── AffinityPanel_new ─────────────────────────────────────────────

    #[cfg(target_os = "macos")]
    #[test]
    fn new_builds_cpuids_and_splices_marking_affinity() {
        use crate::ported::darwin::darwinmachine::{DarwinMachine_freeCPULoadInfo, Machine_new};

        // A real host: existingCPUs from host_processor_info; darwin reports
        // every CPU online, so one MaskItem is built per CPU.
        let mut dm = Machine_new(None, 0);
        let host = &mut dm.super_ as *mut Machine;
        let existing = dm.super_.existingCPUs;
        assert!(existing >= 1);

        // Affinity marks only CPU 0 (entries are ascending, walked by curCpu).
        let aff = Affinity {
            host,
            size: existing,
            used: 1,
            cpus: vec![0],
        };

        let mut width = 0;
        let ap = AffinityPanel_new(host, &aff, Some(&mut width));

        // One item per online CPU, and the panel was spliced to match.
        assert_eq!(Vector_size(&ap.cpuids) as u32, existing);
        assert_eq!(ap.super_.items.len() as u32, existing);
        // width starts at 14 and grows to fit the widest "CPU N" label.
        assert!(width >= 14);
        // Only CPU 0 is marked set → getAffinity returns exactly {0}.
        let out = AffinityPanel_getAffinity(&ap, host);
        assert_eq!(out.used, 1);
        assert_eq!(out.cpus[0], 0);

        DarwinMachine_freeCPULoadInfo(&mut dm.prev_load);
        DarwinMachine_freeCPULoadInfo(&mut dm.curr_load);
    }

    #[test]
    fn get_affinity_collects_only_set_items() {
        let ap = panel_with_cpuids(vec![
            MaskItem_newSingleton("CPU 0", 0, true),
            MaskItem_newSingleton("CPU 1", 1, false),
            MaskItem_newSingleton("CPU 2", 2, true),
            MaskItem_newSingleton("CPU 3", 3, false),
        ]);
        let aff = AffinityPanel_getAffinity(&ap, core::ptr::null_mut());
        assert_eq!(aff.used, 2);
        assert_eq!(&aff.cpus[..2], &[0, 2]);
    }

    #[test]
    fn get_affinity_partial_value_counts_as_set() {
        // value == 1 (partial) is non-zero, so `if (item->value)` is true.
        let ap = panel_with_cpuids(vec![MaskItem {
            text: "CPU 5".to_string(),
            indent: None,
            value: 1,
            sub_tree: 0,
            children: Vec::new(),
            cpu: 5,
        }]);
        let aff = AffinityPanel_getAffinity(&ap, core::ptr::null_mut());
        assert_eq!(aff.used, 1);
        assert_eq!(aff.cpus[0], 5);
    }

    #[test]
    fn get_affinity_empty_when_nothing_set() {
        let ap = panel_with_cpuids(vec![
            MaskItem_newSingleton("CPU 0", 0, false),
            MaskItem_newSingleton("CPU 1", 1, false),
        ]);
        let aff = AffinityPanel_getAffinity(&ap, core::ptr::null_mut());
        assert_eq!(aff.used, 0);
    }
}
