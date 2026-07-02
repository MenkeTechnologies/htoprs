//! Port of `linux/IOPriorityPanel.c` — htop's "IO Priority:" picker panel.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! Both functions are ported faithfully:
//! - [`IOPriorityPanel_new`] (`IOPriorityPanel.c:22`) — builds the `Panel`
//!   of `ListItem`s ("None (based on nice)", the Realtime/Best-effort × 0..8
//!   grid, and "Idle"), pre-selecting the item that matches `currPrio`.
//! - [`IOPriorityPanel_getIOPriority`] (`IOPriorityPanel.c:56`) — reads the
//!   selected `ListItem`'s `key` (its `IOPriority`), or `IOPriority_None`
//!   when the list is empty.
//!
//! `linux/IOPriority.h` is not a Rust module; only its `IOPriority` type
//! alias is exported (from `linuxprocess.rs`). Its `#define`d class enum,
//! `IOPRIO_CLASS_SHIFT`, and the `IOPriority_tuple`/`IOPriority_None`/
//! `IOPriority_Idle` macros are C text substitutions, so — following the
//! `linuxprocess.rs` precedent — they are modeled here as module-private
//! `const`s (not free `fn`s) and the `IOPriority_tuple` macro is inlined at
//! its use site (`(klass << IOPRIO_CLASS_SHIFT) | data`).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::any::Any;

use crate::ported::functionbar::FunctionBar_newEnterEsc;
use crate::ported::linux::linuxprocess::IOPriority;
use crate::ported::listitem::{ListItem, ListItem_new};
use crate::ported::panel::{
    Panel, Panel_add, Panel_getSelected, Panel_new, Panel_setHeader, Panel_setSelected, Panel_size,
};

/// Port of the anonymous IO-priority class enum from `IOPriority.h:14`
/// (`IOPRIO_CLASS_RT` = 1, `IOPRIO_CLASS_BE` = 2). The `IOPriority_tuple`
/// macro that consumes these is inlined at each use site.
const IOPRIO_CLASS_RT: i32 = 1;
const IOPRIO_CLASS_BE: i32 = 2;

/// Port of `#define IOPRIO_CLASS_SHIFT (13)` from `IOPriority.h:23`.
const IOPRIO_CLASS_SHIFT: i32 = 13;

/// Port of `#define IOPriority_None IOPriority_tuple(IOPRIO_CLASS_NONE, 0)`
/// from `IOPriority.h:33`, which expands to `(0 << 13) | 0` == `0`.
const IOPriority_None: IOPriority = 0;

/// Port of `#define IOPriority_Idle IOPriority_tuple(IOPRIO_CLASS_IDLE, 7)`
/// from `IOPriority.h:34`, which expands to `(3 << 13) | 7`.
const IOPriority_Idle: IOPriority = (3 << IOPRIO_CLASS_SHIFT) | 7;

/// Port of `Panel* IOPriorityPanel_new(IOPriority currPrio)` from
/// `IOPriorityPanel.c:22`.
///
/// Builds a `Panel` of `ListItem`s: the "None (based on nice)" default,
/// then, for each of the Realtime and Best-effort classes, one item per
/// priority level `0..8` (labelled `"<class> <n> (High)"` for `0`,
/// `"(Low)"` for `7`, and a bare trailing space otherwise — reproducing the
/// C `"%s %d %s"` format), and finally the "Idle" item. Whenever an item's
/// `IOPriority` equals `currPrio`, it becomes the pre-selected row.
///
/// The C `Class(ListItem)`/`owner` args to `Panel_new` only type the backing
/// `Vector`; the `Vec<Box<dyn Object>>` model needs no such typing, so they
/// are dropped (per the ported `Panel_new` signature).
pub fn IOPriorityPanel_new(currPrio: IOPriority) -> Panel {
    let mut this = Panel_new(
        1,
        1,
        1,
        1,
        Some(FunctionBar_newEnterEsc("Set    ", "Cancel ")),
    );

    Panel_setHeader(&mut this, "IO Priority:");
    Panel_add(
        &mut this,
        Box::new(ListItem_new("None (based on nice)", IOPriority_None)),
    );
    if currPrio == IOPriority_None {
        Panel_setSelected(&mut this, 0);
    }

    // C: static const struct { int klass; const char* name; } classes[] = {
    //       { IOPRIO_CLASS_RT, "Realtime" }, { IOPRIO_CLASS_BE, "Best-effort" },
    //       { 0, NULL } };  — the trailing { 0, NULL } is only the loop sentinel.
    let classes: [(i32, &str); 2] = [
        (IOPRIO_CLASS_RT, "Realtime"),
        (IOPRIO_CLASS_BE, "Best-effort"),
    ];
    for (klass, class_name) in classes {
        for i in 0..8 {
            let suffix = if i == 0 {
                "(High)"
            } else if i == 7 {
                "(Low)"
            } else {
                ""
            };
            // C: xSnprintf(name, sizeof(name), "%s %d %s", name, i, suffix);
            let name = format!("{} {} {}", class_name, i, suffix);
            // C: IOPriority ioprio = IOPriority_tuple(classes[c].klass, i);
            let ioprio: IOPriority = (klass << IOPRIO_CLASS_SHIFT) | i;
            Panel_add(&mut this, Box::new(ListItem_new(&name, ioprio)));
            if currPrio == ioprio {
                let n = Panel_size(&this) - 1;
                Panel_setSelected(&mut this, n);
            }
        }
    }

    Panel_add(&mut this, Box::new(ListItem_new("Idle", IOPriority_Idle)));
    if currPrio == IOPriority_Idle {
        let n = Panel_size(&this) - 1;
        Panel_setSelected(&mut this, n);
    }

    this
}

/// Port of `IOPriority IOPriorityPanel_getIOPriority(Panel* this)` from
/// `IOPriorityPanel.c:56`.
///
/// Returns the selected `ListItem`'s `key` (its `IOPriority`), or
/// `IOPriority_None` when nothing is selected (empty list). The C hard-casts
/// the selected `Object*` to `ListItem*`; the safe-Rust analog downcasts the
/// trait object via `Any` (as the sibling `*Panel` ports do).
pub fn IOPriorityPanel_getIOPriority(this: &Panel) -> IOPriority {
    match Panel_getSelected(this) {
        Some(selected) => {
            let any: &dyn Any = selected;
            let li = any
                .downcast_ref::<ListItem>()
                .expect("IOPriorityPanel_getIOPriority: selected item is not a ListItem");
            li.key
        }
        None => IOPriority_None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 ("None") + 2 classes × 8 levels + 1 ("Idle") = 18 items.
    #[test]
    fn new_builds_all_items() {
        let p = IOPriorityPanel_new(IOPriority_None);
        assert_eq!(Panel_size(&p), 18);
    }

    #[test]
    fn new_selects_none_by_default() {
        let p = IOPriorityPanel_new(IOPriority_None);
        assert_eq!(p.selected, 0);
        assert_eq!(IOPriorityPanel_getIOPriority(&p), IOPriority_None);
    }

    #[test]
    fn new_preselects_matching_class_level() {
        // IOPriority_tuple(IOPRIO_CLASS_RT, 3): item index 1 + 3 = 4.
        let rt3: IOPriority = (IOPRIO_CLASS_RT << IOPRIO_CLASS_SHIFT) | 3;
        let p = IOPriorityPanel_new(rt3);
        assert_eq!(p.selected, 4);
        assert_eq!(IOPriorityPanel_getIOPriority(&p), rt3);

        // Best-effort 0 (High): index 1 + 8 = 9.
        let be0: IOPriority = IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT;
        let q = IOPriorityPanel_new(be0);
        assert_eq!(q.selected, 9);
        assert_eq!(IOPriorityPanel_getIOPriority(&q), be0);
    }

    #[test]
    fn new_preselects_idle_last() {
        let p = IOPriorityPanel_new(IOPriority_Idle);
        assert_eq!(p.selected, 17); // last item
        assert_eq!(IOPriorityPanel_getIOPriority(&p), IOPriority_Idle);
    }

    #[test]
    fn item_labels_follow_c_format() {
        let p = IOPriorityPanel_new(IOPriority_None);
        // index 1 == Realtime 0 (High)
        let a: &dyn Any = crate::ported::panel::Panel_get(&p, 1);
        assert_eq!(
            a.downcast_ref::<ListItem>().unwrap().value,
            "Realtime 0 (High)"
        );
        // index 2 == Realtime 1 with a bare trailing space
        let b: &dyn Any = crate::ported::panel::Panel_get(&p, 2);
        assert_eq!(b.downcast_ref::<ListItem>().unwrap().value, "Realtime 1 ");
        // index 8 == Realtime 7 (Low)
        let c: &dyn Any = crate::ported::panel::Panel_get(&p, 8);
        assert_eq!(
            c.downcast_ref::<ListItem>().unwrap().value,
            "Realtime 7 (Low)"
        );
    }
}
