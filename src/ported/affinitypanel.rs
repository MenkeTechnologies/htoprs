//! Partial port of `AffinityPanel.c` — htop's "Use CPUs:" affinity picker.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # HAVE_LIBHWLOC
//!
//! `AffinityPanel.c` is written for two builds: the `HAVE_LIBHWLOC` build
//! (a topology tree backed by `hwloc_bitmap_t` cpusets) and the plain
//! build (a flat CPU list, `int cpu` per item). htoprs links no `hwloc`,
//! so this port follows the `#else` (non-hwloc) branch throughout: the
//! [`MaskItem`] struct carries the `int cpu` field, and every function
//! whose body lives entirely inside `#ifdef HAVE_LIBHWLOC` has no
//! non-hwloc counterpart and stays an honest stub.
//!
//! # Ported (faithful, non-hwloc branch)
//!
//! - [`MaskItem_newSingleton`] (`AffinityPanel.c:108`) — the flat-CPU
//!   constructor: `text`, `indent = NULL`, `sub_tree = 0`, empty
//!   `children`, `cpu = cpu`, `value = isSet ? 2 : 0`. The `#ifdef
//!   HAVE_LIBHWLOC` arm (allocate + set a one-bit cpuset) is compiled out;
//!   the `#else` arm (`this->cpu = cpu`) is what ports. Returns an owned
//!   [`MaskItem`] (the `History_new`/`Affinity_new` idiom for a C fn that
//!   heap-allocates and returns a pointer).
//! - [`MaskItem_display`] (`AffinityPanel.c:62`) — the always-run checkbox
//!   glyph (`[x]`/`[o]`/`[ ]` in `CHECK_BOX`/`CHECK_MARK`) plus the trailing
//!   `text` (in `CHECK_TEXT`), through the real [`RichString`]/
//!   [`ColorElements`] substrate. The `if (this->indent)` tree-node branch
//!   needs `CRT_treeStr[TREE_STR_OPEN/SHUT]`, which is not ported in
//!   `crt.rs`; since only the hwloc-only [`MaskItem_newMask`] ever sets a
//!   non-NULL `indent`, that branch is unreachable in this build and stays
//!   a `todo!()` (the `ListItem_display` moving-glyph precedent).
//! - [`AffinityPanel_getAffinity`] (`AffinityPanel.c:444`) — the non-hwloc
//!   branch: `Affinity_new(host)`, then for each `cpuids` item whose
//!   `value` is set, `Affinity_add(affinity, item->cpu)`. Both
//!   `Affinity_new`/`Affinity_add` are ported. Takes `this: &AffinityPanel`
//!   (the C casts its `Panel* super` to `AffinityPanel*`).
//!
//! # Stubbed (cannot be ported faithfully yet)
//!
//! hwloc-only (no non-hwloc body exists — the whole function is inside
//! `#ifdef HAVE_LIBHWLOC`, and htoprs links no `hwloc`):
//! - [`MaskItem_newMask`] (`AffinityPanel.c:94`) — takes an
//!   `hwloc_bitmap_t cpuset` and weighs it with `hwloc_bitmap_weight`.
//! - [`AffinityPanel_updateItem`] (`AffinityPanel.c:156`) — sets `value`
//!   from `hwloc_bitmap_isincluded`/`intersects` against `workCpuset`.
//! - [`AffinityPanel_updateTopo`] (`AffinityPanel.c:165`) — recurses the
//!   topology tree built from hwloc objects.
//! - [`AffinityPanel_addObject`] (`AffinityPanel.c:283`) — reads
//!   `hwloc_obj_t` fields (`depth`, `logical_index`, `complete_cpuset`, …).
//! - [`AffinityPanel_buildTopology`] (`AffinityPanel.c:341`) — walks the
//!   `hwloc` object children recursively.
//!
//! `Drop`-teardown (a C `free`/`Vector_delete`/`Panel_done` chain with no
//! safe-Rust algorithm — owned fields are released by `Drop`, the
//! `History_delete`/`Panel_delete` precedent):
//! - [`MaskItem_delete`] (`AffinityPanel.c:48`).
//! - [`AffinityPanel_delete`] (`AffinityPanel.c:141`).
//!
//! panel/cpuids aliasing + unported substrate:
//! - [`AffinityPanel_update`] (`AffinityPanel.c:177`) — the non-hwloc arm is
//!   `Panel_splice(super, this->cpuids)`, and `Panel_splice` is itself
//!   stubbed: htop's `AffinityPanel` uses a *non-owning* `Panel` that shares
//!   the `MaskItem*` pointers held by the *owning* `cpuids` `Vector`, so a
//!   toggle applied to a spliced item is seen through `cpuids`. htoprs's
//!   `Panel` owns its items as `Vec<Box<dyn Object>>` and cannot alias
//!   `cpuids`; reproducing the shared-pointer store needs either the
//!   unported `Vector` (with its `owner` flag) or `Rc`/`RefCell` shared
//!   ownership, neither of which the substrate provides.
//! - [`AffinityPanel_eventHandler`] (`AffinityPanel.c:203`) — toggles the
//!   selected item's `value` in `super->items`, which must alias `cpuids`
//!   (see [`AffinityPanel_update`]), and calls [`AffinityPanel_update`] on a
//!   `HANDLED` result. `HandlerResult` is now modeled in `panel.rs`, so the
//!   remaining block is the `cpuids` aliasing above plus the still-stubbed
//!   [`AffinityPanel_update`] / `Panel_splice`.
//! - [`AffinityPanel_new`] (`AffinityPanel.c:379`) — builds `cpuids` while
//!   the `Panel` splices the same pointers, and its last statement calls
//!   [`AffinityPanel_update`]; blocked transitively on the same aliasing.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::affinity::{Affinity, Affinity_add, Affinity_new};
use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::{HandlerResult, Panel, PanelClass, Panel_done};
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_appendWide};

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
    pub cpu: i32,
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
/// `AffinityPanel.c:48`: `free(text); free(indent);
/// Vector_delete(children); free(this);` (the `hwloc_bitmap_free` block is
/// `#ifdef HAVE_LIBHWLOC`, not built here). Taking `this` by value consumes
/// the item; the owned `text` `String`, `indent` `Option<String>`, and the
/// `children` `Vec<MaskItem>` (whose drop recursively runs each child's
/// teardown — the C's owner-`Vector_delete` recursion) all drop with the
/// struct free.
pub fn MaskItem_delete(this: MaskItem) {
    let _ = this;
}

/// Port of `static void MaskItem_display(const Object* cast, RichString* out)`
/// from `AffinityPanel.c:62`.
///
/// Appends the checkbox (`[x]` for a fully-set item, `[o]` for a partial
/// one, `[ ]` otherwise) using `CRT_colors[CHECK_BOX]`/`CHECK_MARK`, a
/// `CHECK_TEXT` space, then the item `text` in `CHECK_TEXT`. The
/// `if (this->indent)` tree-node branch draws the indent guides and the
/// open/shut glyph via `CRT_treeStr[TREE_STR_OPEN/SHUT]`, which is not
/// ported in `crt.rs` (`crt.rs` is off-limits to this module); since only
/// the hwloc-only [`MaskItem_newMask`] sets a non-NULL `indent`, that branch
/// is unreachable in this non-hwloc build and stays a `todo!()` — the
/// `ListItem_display` moving-glyph precedent.
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
    if this.indent.is_some() {
        // C: RichString_appendWide(out, CRT_colors[PROCESS_TREE], this->indent);
        //    RichString_appendWide(out, CRT_colors[PROCESS_TREE],
        //       this->sub_tree == 2 ? CRT_treeStr[TREE_STR_OPEN]
        //                            : CRT_treeStr[TREE_STR_SHUT]);
        //    RichString_appendAscii(out, CRT_colors[CHECK_TEXT], " ");
        // The open/shut glyph needs the unported CRT_treeStr tables
        // (TREE_STR_OPEN/SHUT); crt.rs is off-limits to this module, and
        // this branch is unreachable without libhwloc (only MaskItem_newMask
        // sets a non-NULL indent).
        todo!(
            "AffinityPanel.c:77: tree-node indent needs unported CRT_treeStr (TREE_STR_OPEN/SHUT)"
        );
    }
    RichString_appendWide(out, check_text, this.text.as_bytes());
}

/// TODO: port of `static MaskItem* MaskItem_newMask(const char* text,
/// const char* indent, hwloc_bitmap_t cpuset, bool owner)` from
/// `AffinityPanel.c:94`. Entirely `#ifdef HAVE_LIBHWLOC`: it takes an
/// `hwloc_bitmap_t` and sets `sub_tree` from `hwloc_bitmap_weight(cpuset)`.
/// htoprs links no `hwloc`, so there is no body to port.
pub fn MaskItem_newMask() {
    todo!("port of AffinityPanel.c:94 — hwloc-only (no libhwloc in htoprs)")
}

/// Port of `static MaskItem* MaskItem_newSingleton(const char* text, int cpu,
/// bool isSet)` from `AffinityPanel.c:108`, non-hwloc branch.
///
/// Builds a flat-CPU item: `text` (C `xStrdup`), `indent = NULL` (not a tree
/// node), `sub_tree = 0`, an empty `children` vector, `cpu = cpu` (the
/// `#else` arm; the `#ifdef HAVE_LIBHWLOC` arm that allocates a one-bit
/// cpuset is compiled out), and `value = isSet ? 2 : 0`. Returns an owned
/// [`MaskItem`] (the C fn heap-allocates and returns a pointer).
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

/// Model of the C `AffinityPanel` struct (`AffinityPanel.c:127`), non-hwloc
/// variant. `super_` is the embedded `Panel super` (`super` is a Rust
/// keyword); `host` is the borrowed `Machine*` (raw pointer — the `Affinity`
/// `host` precedent, never dereferenced by ported code); `topoView` mirrors
/// the C flag (always `false` without hwloc); `cpuids` is the C
/// `Vector* cpuids` of flat-CPU items; `width` is the computed panel width.
/// The hwloc-only fields (`topoRoot`, `allCpuset`, `workCpuset`) live inside
/// `#ifdef HAVE_LIBHWLOC` and are omitted.
pub struct AffinityPanel {
    pub super_: Panel,
    pub host: *mut Machine,
    pub topoView: bool,
    pub cpuids: Vec<MaskItem>,
    pub width: u32,
}

/// Port of `const PanelClass AffinityPanel_class` (`AffinityPanel.c:358`): sets
/// only `.eventHandler = AffinityPanel_eventHandler`; `.drawFunctionBar` /
/// `.printHeader` are NULL and inherit the `Panel` defaults. The ported
/// [`AffinityPanel_eventHandler`] is a `todo!()` stub whose signature is `()`
/// (not `(&mut AffinityPanel, i32) -> HandlerResult` — it awaits the
/// panel/cpuids `Panel_splice` aliasing), so the `event_handler` slot cannot be
/// wired without a signature mismatch and inherits the default here.
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
/// `AffinityPanel.c:141`: `Vector_delete(this->cpuids); Panel_done(&this->super);
/// free(this);` (the `hwloc_bitmap_free`/`MaskItem_delete(topoRoot)` block is
/// `#ifdef HAVE_LIBHWLOC`, not built here). Taking `this` by value consumes
/// the panel; the owned `cpuids` `Vec<MaskItem>` (C's `Vector_delete`, its
/// drop recursively running each `MaskItem` teardown) and the non-owning
/// `host`/`topoView`/`width` fields drop, and the embedded `super_` [`Panel`]
/// is handed to [`Panel_done`] (mirroring the C call graph).
pub fn AffinityPanel_delete(this: AffinityPanel) {
    let AffinityPanel { super_, cpuids, .. } = this;
    // C: Vector_delete(this->cpuids) — the owned Vec drop recurses per item.
    let _ = cpuids;
    Panel_done(super_);
}

/// TODO: port of `static void AffinityPanel_updateItem(AffinityPanel* this,
/// MaskItem* item)` from `AffinityPanel.c:156`. Entirely `#ifdef
/// HAVE_LIBHWLOC`: it computes `item->value` from
/// `hwloc_bitmap_isincluded`/`hwloc_bitmap_intersects` against `workCpuset`.
/// htoprs links no `hwloc`, so there is no body to port.
pub fn AffinityPanel_updateItem() {
    todo!("port of AffinityPanel.c:156 — hwloc-only (no libhwloc in htoprs)")
}

/// TODO: port of `static void AffinityPanel_updateTopo(AffinityPanel* this,
/// MaskItem* item)` from `AffinityPanel.c:165`. Entirely `#ifdef
/// HAVE_LIBHWLOC`: it recurses the hwloc topology tree. htoprs links no
/// `hwloc`, so there is no body to port.
pub fn AffinityPanel_updateTopo() {
    todo!("port of AffinityPanel.c:165 — hwloc-only (no libhwloc in htoprs)")
}

/// TODO: port of `static void AffinityPanel_update(AffinityPanel* this,
/// bool keepSelected)` from `AffinityPanel.c:177`. The non-hwloc arm is
/// `Panel_splice(super, this->cpuids)`, and `Panel_splice` (`Panel.c:222`)
/// is itself stubbed: htop's `AffinityPanel` uses a non-owning `Panel`
/// that shares the `MaskItem*` pointers held by the owning `cpuids`
/// `Vector`, so a toggle on a spliced item is seen through `cpuids`.
/// htoprs's `Panel` owns its items as `Vec<Box<dyn Object>>` and cannot
/// alias `cpuids`; reproducing the shared store needs the unported `Vector`
/// (with its `owner` flag) or `Rc`/`RefCell`, neither in the substrate.
pub fn AffinityPanel_update() {
    todo!("port of AffinityPanel.c:177 — needs Panel_splice + panel/cpuids shared-pointer aliasing")
}

/// TODO: port of `static HandlerResult AffinityPanel_eventHandler(Panel* super,
/// int ch)` from `AffinityPanel.c:203`. `HandlerResult`
/// (`IGNORED`/`HANDLED`/`BREAK_LOOP`) is now modeled in `panel.rs`, but the
/// body toggles the selected item's `value` in `super->items` — which must
/// alias `cpuids` (see [`AffinityPanel_update`]) — and calls
/// `AffinityPanel_update` on a `HANDLED` result. Both still depend on the
/// panel/cpuids shared-pointer aliasing, i.e. the stubbed `Panel_splice`.
pub fn AffinityPanel_eventHandler(this: &mut AffinityPanel, ch: i32) -> HandlerResult {
    let _ = (this, ch);
    todo!("port of AffinityPanel.c:203 — needs panel/cpuids aliasing (Panel_splice) + AffinityPanel_update")
}

/// TODO: port of `static MaskItem* AffinityPanel_addObject(AffinityPanel* this,
/// hwloc_obj_t obj, unsigned indent, MaskItem* parent)` from
/// `AffinityPanel.c:283`. Entirely `#ifdef HAVE_LIBHWLOC`: it reads
/// `hwloc_obj_t` fields (`depth`, `logical_index`, `complete_cpuset`, …) and
/// builds an indent string. htoprs links no `hwloc`, so there is no body to
/// port.
pub fn AffinityPanel_addObject() {
    todo!("port of AffinityPanel.c:283 — hwloc-only (no libhwloc in htoprs)")
}

/// TODO: port of `static MaskItem* AffinityPanel_buildTopology(AffinityPanel* this,
/// hwloc_obj_t obj, unsigned indent, MaskItem* parent)` from
/// `AffinityPanel.c:341`. Entirely `#ifdef HAVE_LIBHWLOC`: it walks the
/// `hwloc` object children recursively. htoprs links no `hwloc`, so there is
/// no body to port.
pub fn AffinityPanel_buildTopology() {
    todo!("port of AffinityPanel.c:341 — hwloc-only (no libhwloc in htoprs)")
}

/// TODO: port of `Panel* AffinityPanel_new(Machine* host, const Affinity* affinity,
/// int* width)` from `AffinityPanel.c:379`. Builds `cpuids` (one
/// [`MaskItem_newSingleton`] per online CPU) while the `Panel` is meant to
/// splice the same `MaskItem*` pointers, and its final statement calls
/// [`AffinityPanel_update`]. Blocked on the same panel/cpuids shared-pointer
/// aliasing as [`AffinityPanel_update`] (htoprs's `Panel` owns its items and
/// cannot alias `cpuids`).
pub fn AffinityPanel_new() {
    todo!("port of AffinityPanel.c:379 — needs panel/cpuids shared-pointer aliasing + AffinityPanel_update")
}

/// Port of `Affinity* AffinityPanel_getAffinity(Panel* super, Machine* host)`
/// from `AffinityPanel.c:444`, non-hwloc branch.
///
/// Allocates a fresh [`Affinity`] for `host`, then for every `cpuids` item
/// whose `value` is set (non-zero), adds that item's `cpu` to the affinity
/// set. The C casts `Panel* super` to `AffinityPanel*`; the faithful analog
/// takes `this: &AffinityPanel` directly. `item->cpu` is a non-negative CPU
/// index; the C passes it to `Affinity_add(unsigned int)`, so it is widened
/// to `u32`. Returns an owned [`Affinity`] (the C fn returns a pointer).
pub fn AffinityPanel_getAffinity(this: &AffinityPanel, host: *mut Machine) -> Affinity {
    let mut affinity = Affinity_new(host);
    for item in &this.cpuids {
        if item.value != 0 {
            Affinity_add(&mut affinity, item.cpu as u32);
        }
    }
    affinity
}

#[cfg(test)]
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
    #[should_panic(expected = "CRT_treeStr")]
    fn display_tree_node_branch_is_blocked_on_crt_treestr() {
        // A non-NULL indent marks a tree node; that branch needs the
        // unported CRT_treeStr tables and is unreachable without libhwloc.
        let it = MaskItem {
            text: "Package".to_string(),
            indent: Some("|- ".to_string()),
            value: 2,
            sub_tree: 1,
            children: Vec::new(),
            cpu: 0,
        };
        let mut rs = RichString::new();
        MaskItem_display(&it, &mut rs);
    }

    // ── AffinityPanel_getAffinity ─────────────────────────────────────

    fn panel_with_cpuids(cpuids: Vec<MaskItem>) -> AffinityPanel {
        AffinityPanel {
            super_: Panel_new(1, 1, 1, 1, None),
            host: core::ptr::null_mut(),
            topoView: false,
            cpuids,
            width: 14,
        }
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
