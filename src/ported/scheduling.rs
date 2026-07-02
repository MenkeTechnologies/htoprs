//! Partial port of `Scheduling.c` — htop's CPU-scheduling policy/priority
//! selection UI plus the per-process scheduling-policy read/set helpers.
//!
//! The whole file sits under the C `#ifdef SCHEDULER_SUPPORT`
//! (`HAVE_SCHED_SETSCHEDULER && HAVE_SCHED_GETSCHEDULER`, i.e. the Linux
//! build); this module ports that variant. C names are preserved verbatim
//! (htop uses `CamelCase_snake`), so `non_snake_case` is allowed for the
//! whole module — matching the spec name-for-name is the point of the
//! port. C `Foo(Panel* this, …)` maps to a free fn `Foo(this: &mut Panel,
//! …)` (the same shape the `Panel.c` / `History.c` ports use). The C
//! `Panel*` return becomes an owned `Panel` (as `Panel_new` itself
//! returns), and a NULL return becomes `Option<Panel>::None`.
//!
//! Ported (no unported substrate):
//! - `Scheduling_newPolicyPanel` (`Scheduling.c:42`) — builds the
//!   "New policy" `Panel` of `ListItem`s (the reset-on-fork toggle row,
//!   then one row per named policy) on the ported `Panel` / `ListItem` /
//!   `FunctionBar` substrate. `ListItem_new(value, key)` is stubbed in
//!   `listitem.rs` (heap `AllocThis` with no free-fn analog — the modeled
//!   construction is a `ListItem { value, key, moving: false }` struct
//!   literal, exactly what `ListItem_new` + `ListItem_init` produce), so
//!   each row is built that way here, matching the `panel.rs` tests.
//! - `Scheduling_togglePolicyPanelResetOnFork` (`Scheduling.c:62`) — flips
//!   the file-static `reset_on_fork` flag and rewrites the panel's row 0.
//! - `Scheduling_formatPolicy` (`Scheduling.c:130`) — the pure policy-id →
//!   short display-string map.
//!
//! Stubbed (blocked on the libc/nix FFI gap `affinity.rs:26` documents:
//! the crate depends on neither, and these are Linux-only syscalls that do
//! not exist on the darwin dev target — porting real FFI would break
//! `cargo build`):
//! - `Scheduling_newPriorityPanel` (`Scheduling.c:74`) — the loop bounds
//!   come from `sched_get_priority_min` / `sched_get_priority_max`, and
//!   every early return keys off their results.
//! - `Scheduling_setPolicy` (`Scheduling.c:102`) — calls
//!   `sched_setscheduler`.
//! - `Scheduling_rowSetPolicy` (`Scheduling.c:124`) — its whole body
//!   delegates to the syscall-blocked `Scheduling_setPolicy`; blocked
//!   transitively (the `Affinity_rowSet` precedent).
//! - `Scheduling_readProcessPolicy` (`Scheduling.c:162`) — calls
//!   `sched_getscheduler`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ported::functionbar::FunctionBar_newEnterEsc;
use crate::ported::listitem::ListItem;
use crate::ported::panel::{Panel, Panel_add, Panel_new, Panel_setHeader, Panel_setSelected};

// Linux `<sched.h>` scheduling-policy constants, used by
// `Scheduling_formatPolicy` and by the `policies` table below. Values
// match `libc` 0.2.186 (`unix/linux_like`): SCHED_OTHER=0, SCHED_FIFO=1,
// SCHED_RR=2, SCHED_BATCH=3, SCHED_IDLE=5, SCHED_DEADLINE=6,
// SCHED_RESET_ON_FORK=0x40000000. On the platforms where htop defines
// `SCHEDULER_SUPPORT` these are all available, so every `#ifdef` arm of
// the C switch is included.
const SCHED_OTHER: i32 = 0;
const SCHED_FIFO: i32 = 1;
const SCHED_RR: i32 = 2;
const SCHED_BATCH: i32 = 3;
const SCHED_IDLE: i32 = 5;
const SCHED_DEADLINE: i32 = 6;
const SCHED_RESET_ON_FORK: i32 = 0x4000_0000;

/// Port of `typedef struct { const char* name; int id; bool
/// prioritySupport; } SchedulingPolicy` from `Scheduling.h:22`. `name` is
/// `Option<&'static str>` so the designated-initializer hole in the
/// `policies` table (see below) is a faithful `None` (C `NULL`).
struct SchedulingPolicy {
    name: Option<&'static str>,
    id: i32,
    prioritySupport: bool,
}

/// Port of `static const SchedulingPolicy policies[]` from
/// `Scheduling.c:25`. The C array uses C99 designated initializers keyed
/// by the `SCHED_*` id (`[SCHED_OTHER] = …`), so the entries land at
/// their id's index and the largest id present (`SCHED_IDLE == 5`) fixes
/// `ARRAYSIZE(policies) == 6`. Index `4` is never assigned, so it is the
/// zero-initialized hole (`{ NULL, 0, false }`) the `!policies[i].name`
/// guard in `Scheduling_newPolicyPanel` skips. Reproduced index-for-index
/// (note `SCHED_DEADLINE` is deliberately absent — it appears only in
/// `Scheduling_formatPolicy`).
static policies: [SchedulingPolicy; 6] = [
    // [SCHED_OTHER == 0]
    SchedulingPolicy { name: Some("Other"), id: SCHED_OTHER, prioritySupport: false },
    // [SCHED_FIFO == 1]
    SchedulingPolicy { name: Some("FiFo"), id: SCHED_FIFO, prioritySupport: true },
    // [SCHED_RR == 2]
    SchedulingPolicy { name: Some("RoundRobin"), id: SCHED_RR, prioritySupport: true },
    // [SCHED_BATCH == 3]
    SchedulingPolicy { name: Some("Batch"), id: SCHED_BATCH, prioritySupport: false },
    // [4] — the zero-initialized designated-initializer hole (C NULL name)
    SchedulingPolicy { name: None, id: 0, prioritySupport: false },
    // [SCHED_IDLE == 5]
    SchedulingPolicy { name: Some("Idle"), id: SCHED_IDLE, prioritySupport: false },
];

/// Port of `static bool reset_on_fork` from `Scheduling.c:38` (guarded by
/// `#ifdef SCHED_RESET_ON_FORK`). The file-static toggle shared between
/// `Scheduling_newPolicyPanel` (which seeds row 0's label from it) and
/// `Scheduling_togglePolicyPanelResetOnFork` (which flips it), modeled as
/// an atomic file-static exactly like `FunctionBar.c`'s `currentLen`.
static reset_on_fork: AtomicBool = AtomicBool::new(false);

/// Port of `typedef struct { int policy; int priority; } SchedulingArg`
/// from `Scheduling.h:37`. The callback payload `Scheduling_setPolicy`
/// reads out of `Arg.v`; kept here to model the C data type this file
/// owns. Only the (syscall-blocked) `Scheduling_setPolicy` consumes it.
struct SchedulingArg {
    policy: i32,
    priority: i32,
}

/// Port of `Panel* Scheduling_newPolicyPanel(int preSelectedPolicy)` from
/// `Scheduling.c:42`. Builds a `ListItem` panel with an Enter/Esc
/// function bar and the header "New policy:": row 0 is the reset-on-fork
/// toggle (label chosen from the `reset_on_fork` static), then one row per
/// `policies` entry with a non-NULL name (the hole at index 4 is skipped).
///
/// Faithful quirk: the C `Panel_setSelected(this, (int) i)` selects by the
/// `policies` *array* index `i`, not by the panel row position — with the
/// reset-on-fork row prepended these differ by one, so a preselected
/// policy highlights the row above its own. Ported verbatim (no fix).
pub fn Scheduling_newPolicyPanel(preSelectedPolicy: i32) -> Panel {
    let mut this = Panel_new(0, 0, 0, 0, Some(FunctionBar_newEnterEsc("Select ", "Cancel ")));
    Panel_setHeader(&mut this, "New policy:");

    // #ifdef SCHED_RESET_ON_FORK
    let rof = reset_on_fork.load(Ordering::Relaxed);
    Panel_add(
        &mut this,
        Box::new(ListItem {
            value: if rof {
                "Reset on fork: on".to_string()
            } else {
                "Reset on fork: off".to_string()
            },
            key: -1,
            moving: false,
        }),
    );

    for i in 0..policies.len() {
        let name = match policies[i].name {
            Some(n) => n,
            None => continue,
        };

        Panel_add(
            &mut this,
            Box::new(ListItem {
                value: name.to_string(),
                key: policies[i].id,
                moving: false,
            }),
        );
        if policies[i].id == preSelectedPolicy {
            Panel_setSelected(&mut this, i as i32);
        }
    }

    this
}

/// Port of `void Scheduling_togglePolicyPanelResetOnFork(Panel* schedPanel)`
/// from `Scheduling.c:62` (`#ifdef SCHED_RESET_ON_FORK`). Flips the
/// `reset_on_fork` static, then rewrites row 0's label to match.
///
/// The C reads the row via `Panel_get(schedPanel, 0)` and mutates
/// `item->value` through the returned pointer. The ported `Panel_get`
/// returns a shared `&dyn Object`, so the mutation instead goes through
/// the panel's `items` vector with a mutable `Any` downcast to `ListItem`
/// (the safe-Rust analog of the C `(ListItem*) Panel_get(...)` cast).
/// `free_and_xStrdup(&item->value, s)` (free old, dup new) is a plain
/// string assignment.
pub fn Scheduling_togglePolicyPanelResetOnFork(schedPanel: &mut Panel) {
    let rof = !reset_on_fork.load(Ordering::Relaxed);
    reset_on_fork.store(rof, Ordering::Relaxed);

    let item = (schedPanel.items[0].as_mut() as &mut dyn Any)
        .downcast_mut::<ListItem>()
        .expect("Scheduling_togglePolicyPanelResetOnFork: panel row 0 is not a ListItem");
    item.value = if rof {
        "Reset on fork: on".to_string()
    } else {
        "Reset on fork: off".to_string()
    };
}

/// TODO: port of `Panel* Scheduling_newPriorityPanel(int policy, int
/// preSelectedPriority)` from `Scheduling.c:74`. Blocked: the row range is
/// `sched_get_priority_min(policy)..=sched_get_priority_max(policy)` and
/// every early `return NULL` keys off those two syscalls' results. Both
/// are Linux-only libc calls; the crate depends on neither `libc` nor
/// `nix` (see `affinity.rs:26`), and they do not exist on the darwin dev
/// target, so a faithful port is not reachable yet. Left as a stub.
pub fn Scheduling_newPriorityPanel() {
    todo!("port of Scheduling.c:74 — needs sched_get_priority_min/max (libc/nix FFI)")
}

/// TODO: port of `static bool Scheduling_setPolicy(Process* p, Arg arg)`
/// from `Scheduling.c:102`. Blocked on `sched_setscheduler` (with a
/// `struct sched_param`), a Linux-only libc call the crate cannot reach
/// (no `libc`/`nix` dependency; absent on darwin). Left as a stub.
pub fn Scheduling_setPolicy() {
    todo!("port of Scheduling.c:102 — needs sched_setscheduler (libc/nix FFI)")
}

/// TODO: port of `bool Scheduling_rowSetPolicy(Row* row, Arg arg)` from
/// `Scheduling.c:124`. The body casts `Row*`→`Process*`, asserts
/// `Object_isA(&Process_class)`, and delegates to `Scheduling_setPolicy`
/// — which is syscall-blocked, so this is blocked transitively (the
/// `Affinity_rowSet` precedent). Left as a stub.
pub fn Scheduling_rowSetPolicy() {
    todo!("port of Scheduling.c:124 — delegates to syscall-blocked Scheduling_setPolicy")
}

/// Port of `Scheduling.c:130`.
///
/// `const char* Scheduling_formatPolicy(int policy)`. Strips the
/// `SCHED_RESET_ON_FORK` bit, then maps the base policy id to its short
/// display string, returning `"???"` for anything unrecognized. The C
/// returns a `const char*` into static storage; Rust returns the
/// equivalent `&'static str`.
pub fn Scheduling_formatPolicy(policy: i32) -> &'static str {
    let policy = policy & !SCHED_RESET_ON_FORK;

    match policy {
        SCHED_OTHER => "OTHER",
        SCHED_FIFO => "FIFO",
        SCHED_RR => "RR",
        SCHED_BATCH => "BATCH",
        SCHED_IDLE => "IDLE",
        SCHED_DEADLINE => "EDF",
        _ => "???",
    }
}

/// TODO: port of `void Scheduling_readProcessPolicy(Process* proc)` from
/// `Scheduling.c:162`. The body is
/// `proc->scheduling_policy = sched_getscheduler(Process_getPid(proc))`;
/// `sched_getscheduler` is a Linux-only libc call the crate cannot reach
/// (no `libc`/`nix` dependency; absent on darwin). Left as a stub.
pub fn Scheduling_readProcessPolicy() {
    todo!("port of Scheduling.c:162 — needs sched_getscheduler (libc/nix FFI)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // `reset_on_fork` is a process-wide file-static; the two tests that
    // read/flip it must not run concurrently. Serialize them and reset the
    // flag to a known state at the start of each.
    static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

    /// Downcast a panel row to its `ListItem` value for assertions.
    fn row_value(p: &Panel, i: usize) -> &str {
        let any: &dyn Any = p.items[i].as_ref();
        &any.downcast_ref::<ListItem>().unwrap().value
    }

    #[test]
    fn maps_each_known_policy() {
        assert_eq!(Scheduling_formatPolicy(SCHED_OTHER), "OTHER");
        assert_eq!(Scheduling_formatPolicy(SCHED_FIFO), "FIFO");
        assert_eq!(Scheduling_formatPolicy(SCHED_RR), "RR");
        assert_eq!(Scheduling_formatPolicy(SCHED_BATCH), "BATCH");
        assert_eq!(Scheduling_formatPolicy(SCHED_IDLE), "IDLE");
        // SCHED_DEADLINE renders as "EDF", not "DEADLINE".
        assert_eq!(Scheduling_formatPolicy(SCHED_DEADLINE), "EDF");
    }

    #[test]
    fn unknown_policy_is_question_marks() {
        assert_eq!(Scheduling_formatPolicy(4), "???");
        assert_eq!(Scheduling_formatPolicy(7), "???");
        assert_eq!(Scheduling_formatPolicy(-1), "???");
    }

    #[test]
    fn reset_on_fork_bit_is_stripped_before_lookup() {
        // The RESET_ON_FORK flag must not change the rendered policy name.
        assert_eq!(
            Scheduling_formatPolicy(SCHED_FIFO | SCHED_RESET_ON_FORK),
            "FIFO"
        );
        assert_eq!(
            Scheduling_formatPolicy(SCHED_OTHER | SCHED_RESET_ON_FORK),
            "OTHER"
        );
        // Bare flag with no base policy masks down to SCHED_OTHER (0).
        assert_eq!(Scheduling_formatPolicy(SCHED_RESET_ON_FORK), "OTHER");
    }

    #[test]
    fn new_policy_panel_builds_reset_row_then_named_policies() {
        let _guard = GLOBAL_LOCK.lock().unwrap();
        reset_on_fork.store(false, Ordering::Relaxed);

        let p = Scheduling_newPolicyPanel(SCHED_RR);

        // Row 0 is the reset-on-fork toggle (off by default), then the five
        // named policies in `policies` array order (the index-4 hole is
        // skipped): Other, FiFo, RoundRobin, Batch, Idle.
        assert_eq!(p.items.len(), 6);
        assert_eq!(row_value(&p, 0), "Reset on fork: off");
        assert_eq!(row_value(&p, 1), "Other");
        assert_eq!(row_value(&p, 2), "FiFo");
        assert_eq!(row_value(&p, 3), "RoundRobin");
        assert_eq!(row_value(&p, 4), "Batch");
        assert_eq!(row_value(&p, 5), "Idle");

        // preSelectedPolicy == SCHED_RR (id 2) selects by the policies array
        // index i == 2 (the verbatim C quirk — that row is "FiFo", not
        // "RoundRobin", because row 0 is the reset toggle).
        assert_eq!(p.selected, 2);
    }

    #[test]
    fn new_policy_panel_reflects_reset_on_fork_on() {
        let _guard = GLOBAL_LOCK.lock().unwrap();
        reset_on_fork.store(true, Ordering::Relaxed);

        let p = Scheduling_newPolicyPanel(SCHED_OTHER);
        assert_eq!(row_value(&p, 0), "Reset on fork: on");
        // preSelectedPolicy SCHED_OTHER (id 0) -> array index 0.
        assert_eq!(p.selected, 0);

        reset_on_fork.store(false, Ordering::Relaxed);
    }

    #[test]
    fn toggle_flips_global_and_row_zero() {
        let _guard = GLOBAL_LOCK.lock().unwrap();
        reset_on_fork.store(false, Ordering::Relaxed);

        let mut p = Scheduling_newPolicyPanel(SCHED_OTHER);
        assert_eq!(row_value(&p, 0), "Reset on fork: off");

        Scheduling_togglePolicyPanelResetOnFork(&mut p);
        assert!(reset_on_fork.load(Ordering::Relaxed));
        assert_eq!(row_value(&p, 0), "Reset on fork: on");

        Scheduling_togglePolicyPanelResetOnFork(&mut p);
        assert!(!reset_on_fork.load(Ordering::Relaxed));
        assert_eq!(row_value(&p, 0), "Reset on fork: off");
    }
}
