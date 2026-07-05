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
//! - [`MaskItem_display`] (`AffinityPanel.c:62`) — the checkbox glyph
//!   (`[x]`/`[o]`/`[ ]` in `CHECK_BOX`/`CHECK_MARK`), the `if (this->indent)`
//!   tree-node branch (indent guides + `CRT_treeStr[TREE_STR_OPEN/SHUT]` in
//!   `PROCESS_TREE`), and the trailing `text` (in `CHECK_TEXT`), through the
//!   real [`RichString`]/[`ColorElements`]/[`TreeStr`] substrate. The tree
//!   branch is only reached in a `HAVE_LIBHWLOC` build (only
//!   [`MaskItem_newMask`] sets a non-NULL `indent`) but is ported in full.
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
//! Ported via the shared `cpuids` store — the `super_` [`Panel`] holds
//! `PanelItem::Borrowed` pointers into the owning `cpuids` [`Vector`] (the C's
//! non-owning-panel / owning-`cpuids` model), spliced by the now-ported
//! [`Panel_splice`]:
//! - [`AffinityPanel_new`] (`AffinityPanel.c:379`) — builds one `MaskItem` per
//!   online CPU, marks the `affinity` set, then splices via `update`.
//! - [`AffinityPanel_update`] (`AffinityPanel.c:177`) — prune + re-splice.
//! - [`AffinityPanel_eventHandler`] (`AffinityPanel.c:203`) — toggles the
//!   selected item's `value` through its borrowed pointer (visible via
//!   `cpuids`), then re-runs `update` on `HANDLED`.
//!
//! The whole non-hwloc `AffinityPanel` is now ported; only the five hwloc-only
//! functions above remain stubbed (they need `libhwloc`, which htoprs omits).
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
    Panel_setHeader, Panel_setSelected, Panel_splice,
};
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_appendWide};
use crate::ported::vector::{
    Vector, Vector_add, Vector_delete, Vector_get, Vector_new, Vector_size, VECTOR_DEFAULT_SIZE,
};

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
/// `Vector* cpuids` of flat-CPU items — an owning [`Vector`] of `MaskItem`
/// (`Box<dyn Object>`), whose element pointers the `super_` [`Panel`] borrows
/// via [`Panel_splice`] (the C's non-owning-panel / owning-`cpuids` shared
/// store); `width` is the computed panel width. The hwloc-only fields
/// (`topoRoot`, `allCpuset`, `workCpuset`) live inside `#ifdef HAVE_LIBHWLOC`
/// and are omitted.
pub struct AffinityPanel {
    pub super_: Panel,
    pub host: *mut Machine,
    pub topoView: bool,
    pub cpuids: Vector,
    pub width: u32,
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
/// `AffinityPanel.c:141`: `Vector_delete(this->cpuids); Panel_done(&this->super);
/// free(this);` (the `hwloc_bitmap_free`/`MaskItem_delete(topoRoot)` block is
/// `#ifdef HAVE_LIBHWLOC`, not built here). Taking `this` by value consumes the
/// panel; the owning `cpuids` [`Vector`] is handed to [`Vector_delete`] (which
/// drops each `MaskItem` box) and the embedded `super_` [`Panel`] to
/// [`Panel_done`] — the `super_` panel only *borrows* the `cpuids` items, so
/// dropping it first (its `Borrowed` pointers) then the owner is a safe order.
pub fn AffinityPanel_delete(this: AffinityPanel) {
    let AffinityPanel { super_, cpuids, .. } = this;
    // C: Panel_done(&this->super) then implicitly the cpuids Vector_delete;
    // the panel holds only Borrowed pointers into cpuids, so releasing it
    // before the owner cannot dangle.
    Panel_done(super_);
    Vector_delete(cpuids);
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

/// Port of `static void AffinityPanel_update(AffinityPanel* this, bool
/// keepSelected)` from `AffinityPanel.c:177`, non-hwloc branch.
///
/// Re-syncs the panel display with `cpuids`: sets the `F3` label (empty
/// without `topoView`), prunes the panel's borrowed item list, then
/// [`Panel_splice`]s the `cpuids` element pointers back in (the panel borrows,
/// `cpuids` owns), optionally restoring the prior selection. `Panel_setSelected`
/// clamps out-of-range indices, matching the C.
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

/// Port of `static HandlerResult AffinityPanel_eventHandler(Panel* super,
/// int ch)` from `AffinityPanel.c:203`, non-hwloc branch.
///
/// On mouse click / re-click / space, toggles the selected item's `value`
/// between 0 and 2 (`selected->value ? 0 : 2`); Enter breaks the picker loop.
/// The toggle mutates the item through the panel's `Borrowed` pointer, which
/// aliases the `cpuids`-owned `MaskItem` (so [`AffinityPanel_getAffinity`] sees
/// it). A `HANDLED` result re-runs [`AffinityPanel_update`] (keeping the
/// selection). The hwloc-only `F1`/`F2`/`F3`/`+`/`-` cases are compiled out.
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

/// Port of `static const char* const AffinityPanelFunctions[]`
/// (`AffinityPanel.c:366`), non-hwloc variant. The `"All"`/`"Topology"`/blank
/// entries are `#ifdef HAVE_LIBHWLOC`; only `"Set"`/`"Cancel"` remain (the C
/// `NULL` terminator is the slice length here).
static AffinityPanelFunctions: [&str; 2] = ["Set    ", "Cancel "];

/// Port of `static const char* const AffinityPanelKeys[]` (`AffinityPanel.c:376`).
static AffinityPanelKeys: [&str; 5] = ["Enter", "Esc", "F1", "F2", "F3"];

/// Port of `static const int AffinityPanelEvents[]` (`AffinityPanel.c:377`).
/// `13`/`27` are Enter/Esc; the `F1`/`F2`/`F3` events pair with the hwloc-only
/// labels, so in the non-hwloc build `FunctionBar_new` binds only the first two
/// (it stops at the 2-entry `AffinityPanelFunctions`).
static AffinityPanelEvents: [i32; 5] = [13, 27, KEY_F(1), KEY_F(2), KEY_F(3)];

/// Port of `Panel* AffinityPanel_new(Machine* host, const Affinity* affinity,
/// int* width)` from `AffinityPanel.c:379`, non-hwloc branch.
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
