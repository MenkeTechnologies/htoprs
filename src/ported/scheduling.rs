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
//! The four `sched_*`-syscall helpers below (`Scheduling_setPolicy`,
//! `Scheduling_readProcessPolicy`, `Scheduling_newPriorityPanel`, and the
//! `Scheduling_rowSetPolicy` that delegates to the first) are the Linux
//! kernel scheduler syscalls `sched_setscheduler` / `sched_getscheduler` /
//! `sched_get_priority_min` / `sched_get_priority_max`. Because
//! `SCHEDULER_SUPPORT` is itself Linux-gated in htop, their bodies are
//! `#[cfg(target_os = "linux")]` over
//! `libc::{sched_setscheduler, sched_getscheduler, sched_get_priority_min,
//! sched_get_priority_max, sched_param}`. On a non-Linux target — the
//! darwin dev host — `SCHEDULER_SUPPORT` is undefined and the C functions
//! are compiled out entirely; the `#[cfg(not(target_os = "linux"))]` arms
//! reproduce that absence as a no-op (the panel/set helpers yield "no
//! panel"/"not set", the reader leaves `scheduling_policy` untouched).
//! This is the faithful non-`SCHEDULER_SUPPORT` behavior, not a fake
//! syscall, and keeps `cargo build` clean on darwin (which lacks the
//! Linux-only `sched_setscheduler`/`sched_getscheduler` symbols).
//!
//! Ported:
//! - `Scheduling_newPolicyPanel` (`Scheduling.c:42`) — builds the
//!   "New policy" `Panel` of `ListItem`s (the reset-on-fork toggle row,
//!   then one row per named policy) on the ported `Panel` / `ListItem` /
//!   `FunctionBar` substrate. Each row is built with a `ListItem { value,
//!   key, moving: false }` struct literal (the same value `ListItem_new` +
//!   `ListItem_init` produce), matching the `panel.rs` tests.
//! - `Scheduling_togglePolicyPanelResetOnFork` (`Scheduling.c:62`) — flips
//!   the file-static `reset_on_fork` flag and rewrites the panel's row 0.
//! - `Scheduling_newPriorityPanel` (`Scheduling.c:74`) — validates the
//!   policy (platform-independent early returns), then on Linux ranges the
//!   rows over `sched_get_priority_min(policy)..=sched_get_priority_max`.
//! - `Scheduling_setPolicy` (`Scheduling.c:102`) — builds a `sched_param`
//!   and calls `sched_setscheduler` on Linux.
//! - `Scheduling_rowSetPolicy` (`Scheduling.c:124`) — the `Row*`→`Process*`
//!   cast is modeled as `Object_isA(&Process_class)` + an `Any` downcast to
//!   `&Process`, then delegates to `Scheduling_setPolicy`.
//! - `Scheduling_formatPolicy` (`Scheduling.c:130`) — the pure policy-id →
//!   short display-string map.
//! - `Scheduling_readProcessPolicy` (`Scheduling.c:162`) — sets
//!   `proc->scheduling_policy = sched_getscheduler(pid)` on Linux.
//!
//! Stubbed: none.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ported::functionbar::FunctionBar_newEnterEsc;
use crate::ported::listitem::ListItem;
use crate::ported::object::{Object, Object_isA};
use crate::ported::panel::{Panel, Panel_add, Panel_new, Panel_setHeader, Panel_setSelected};
use crate::ported::process::{Process, Process_class};

// Linux-only substrate: the `ListItem_new` constructor is used solely by
// the Linux priority-panel body, and `Process_getPid` solely by the two
// syscall bodies — importing them unconditionally would be an unused
// import on the darwin build, so they are `cfg`-gated to match.
#[cfg(target_os = "linux")]
use crate::ported::listitem::ListItem_new;
#[cfg(target_os = "linux")]
use crate::ported::process::Process_getPid;

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
    SchedulingPolicy {
        name: Some("Other"),
        id: SCHED_OTHER,
        prioritySupport: false,
    },
    // [SCHED_FIFO == 1]
    SchedulingPolicy {
        name: Some("FiFo"),
        id: SCHED_FIFO,
        prioritySupport: true,
    },
    // [SCHED_RR == 2]
    SchedulingPolicy {
        name: Some("RoundRobin"),
        id: SCHED_RR,
        prioritySupport: true,
    },
    // [SCHED_BATCH == 3]
    SchedulingPolicy {
        name: Some("Batch"),
        id: SCHED_BATCH,
        prioritySupport: false,
    },
    // [4] — the zero-initialized designated-initializer hole (C NULL name)
    SchedulingPolicy {
        name: None,
        id: 0,
        prioritySupport: false,
    },
    // [SCHED_IDLE == 5]
    SchedulingPolicy {
        name: Some("Idle"),
        id: SCHED_IDLE,
        prioritySupport: false,
    },
];

/// Port of `static bool reset_on_fork` from `Scheduling.c:38` (guarded by
/// `#ifdef SCHED_RESET_ON_FORK`). The file-static toggle shared between
/// `Scheduling_newPolicyPanel` (which seeds row 0's label from it) and
/// `Scheduling_togglePolicyPanelResetOnFork` (which flips it), modeled as
/// an atomic file-static exactly like `FunctionBar.c`'s `currentLen`.
static reset_on_fork: AtomicBool = AtomicBool::new(false);

/// Port of `typedef struct { int policy; int priority; } SchedulingArg`
/// from `Scheduling.h:37`. The callback payload `Scheduling_setPolicy`
/// reads out of the `Arg` union's `v` pointer; the ported
/// `Scheduling_setPolicy` / `Scheduling_rowSetPolicy` take a
/// `&SchedulingArg` in place of that `void*` (the faithful typed borrow).
/// `pub` because the C type lives in the public `Scheduling.h` header (and
/// so appears in the `pub fn` `Scheduling_setPolicy`/`_rowSetPolicy`
/// signatures).
pub struct SchedulingArg {
    pub policy: i32,
    pub priority: i32,
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
    let mut this = Panel_new(
        0,
        0,
        0,
        0,
        Some(FunctionBar_newEnterEsc("Select ", "Cancel ")),
    );
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

/// Port of `Panel* Scheduling_newPriorityPanel(int policy, int
/// preSelectedPriority)` from `Scheduling.c:74`. Returns `None` (C `NULL`)
/// when `policy` is out of range, is the unnamed hole, or does not support
/// priorities — these guards are platform-independent so they run before
/// the `cfg` split. On Linux the row range is
/// `sched_get_priority_min(policy)..=sched_get_priority_max(policy)`
/// (each negative result short-circuits to `None`, as the C `return NULL`
/// does), with one `ListItem` per priority and the `preSelectedPriority`
/// row highlighted. Off Linux the syscall bounds do not exist
/// (`SCHEDULER_SUPPORT` undefined), so — after the shared guards — the
/// function yields `None`, matching the compiled-out C.
pub fn Scheduling_newPriorityPanel(policy: i32, preSelectedPriority: i32) -> Option<Panel> {
    if policy < 0 || policy as usize >= policies.len() || policies[policy as usize].name.is_none() {
        return None;
    }

    if !policies[policy as usize].prioritySupport {
        return None;
    }

    #[cfg(target_os = "linux")]
    {
        let min = unsafe { libc::sched_get_priority_min(policy) };
        if min < 0 {
            return None;
        }
        let max = unsafe { libc::sched_get_priority_max(policy) };
        if max < 0 {
            return None;
        }

        let mut this = Panel_new(
            0,
            0,
            0,
            0,
            Some(FunctionBar_newEnterEsc("Select ", "Cancel ")),
        );
        Panel_setHeader(&mut this, "Priority:");

        for i in min..=max {
            // xSnprintf(buf, sizeof(buf), "%d", i)
            let buf = format!("{}", i);
            Panel_add(&mut this, Box::new(ListItem_new(&buf, i)));
            if i == preSelectedPriority {
                Panel_setSelected(&mut this, i);
            }
        }

        Some(this)
    }

    #[cfg(not(target_os = "linux"))]
    {
        // SCHEDULER_SUPPORT is undefined off Linux: no priority-min/max
        // syscalls exist, so there is no panel to build.
        let _ = preSelectedPriority;
        None
    }
}

/// Port of `static bool Scheduling_setPolicy(Process* p, Arg arg)` from
/// `Scheduling.c:102`. The C reads its payload out of the `Arg` union
/// (`const SchedulingArg* sarg = arg.v`); the faithful safe-Rust analog
/// takes the typed payload `&SchedulingArg` directly (the pointer is only
/// ever dereferenced, so — unlike the `Affinity` `Arg` precedent that
/// keeps a never-dereferenced raw pointer — passing the concrete borrow is
/// the honest model). The three `assert()`s become `debug_assert!` (the C
/// `assert()` is `NDEBUG`-gated), matching the `vector.rs` precedent.
///
/// On Linux the priority comes from `sarg->priority` only when the policy
/// supports it (else `0`), the `reset_on_fork` static folds into `policy`
/// (the C `policy &= SCHED_RESET_ON_FORK` is ported verbatim, `&=` and
/// all), and `sched_setscheduler` is invoked; `r != -1` is the result
/// (POSIX returns the previous policy, Linux always `0`). Off Linux the
/// syscall does not exist (`SCHEDULER_SUPPORT` undefined), so the no-op
/// arm reports "not set" (`false`).
pub fn Scheduling_setPolicy(p: &Process, arg: &SchedulingArg) -> bool {
    let sarg = arg;
    let policy = sarg.policy;

    debug_assert!(policy >= 0);
    debug_assert!((policy as usize) < policies.len());
    debug_assert!(policies[policy as usize].name.is_some());

    #[cfg(target_os = "linux")]
    {
        let param = libc::sched_param {
            sched_priority: if policies[policy as usize].prioritySupport {
                sarg.priority
            } else {
                0
            },
        };

        // #ifdef SCHED_RESET_ON_FORK
        let mut policy = policy;
        if reset_on_fork.load(Ordering::Relaxed) {
            policy &= SCHED_RESET_ON_FORK;
        }

        let r = unsafe { libc::sched_setscheduler(Process_getPid(p), policy, &param) };

        // POSIX says on success the previous scheduling policy should be
        // returned, but Linux always returns 0.
        r != -1
    }

    #[cfg(not(target_os = "linux"))]
    {
        // SCHEDULER_SUPPORT is undefined off Linux: sched_setscheduler
        // does not exist, so no policy can be set.
        let _ = p;
        false
    }
}

/// Port of `bool Scheduling_rowSetPolicy(Row* row, Arg arg)` from
/// `Scheduling.c:124`. The C `Process* p = (Process*) row` cast (safe
/// because `Row` is the first member of `Process`) is modeled as an
/// `Object_isA(&Process_class)` guard followed by an `Any` downcast to
/// `&Process` — the same `(&dyn Object as &dyn Any).downcast_ref` idiom
/// `Process_compare` uses for its `const void*` cast. The C `assert(...)`
/// becomes `debug_assert!` (matching `vector.rs`). Delegates to
/// [`Scheduling_setPolicy`].
pub fn Scheduling_rowSetPolicy(row: &dyn Object, arg: &SchedulingArg) -> bool {
    // Process* p = (Process*) row;
    // assert(Object_isA((const Object*) p, (const ObjectClass*) &Process_class));
    debug_assert!(Object_isA(Some(row), &Process_class));
    let any: &dyn Any = row;
    let p = any
        .downcast_ref::<Process>()
        .expect("Scheduling_rowSetPolicy: row is not a Process");
    Scheduling_setPolicy(p, arg)
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

/// Port of `void Scheduling_readProcessPolicy(Process* proc)` from
/// `Scheduling.c:162`: `proc->scheduling_policy =
/// sched_getscheduler(Process_getPid(proc))`. On Linux the syscall fills
/// the field; off Linux (`SCHEDULER_SUPPORT` undefined) the C function is
/// compiled out, so the no-op arm leaves `scheduling_policy` untouched.
/// (`proc` is a Rust-reserved word, so the parameter is spelled `proc_`.)
pub fn Scheduling_readProcessPolicy(proc_: &mut Process) {
    #[cfg(target_os = "linux")]
    {
        proc_.scheduling_policy = unsafe { libc::sched_getscheduler(Process_getPid(proc_)) };
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = proc_;
    }
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

    // --- Scheduling_newPriorityPanel: platform-independent guards -----------
    // The `policy` validity / prioritySupport early returns (Scheduling.c:75,
    // 78) run before the `#[cfg(target_os = "linux")]` split, so they yield
    // `None` on every target and are testable on the darwin dev host.

    #[test]
    fn priority_panel_rejects_out_of_range_policy() {
        // policy < 0  and  policy >= ARRAYSIZE(policies) (== 6).
        assert!(Scheduling_newPriorityPanel(-1, 50).is_none());
        assert!(Scheduling_newPriorityPanel(6, 50).is_none());
        assert!(Scheduling_newPriorityPanel(100, 50).is_none());
    }

    #[test]
    fn priority_panel_rejects_unnamed_hole() {
        // Index 4 is the designated-initializer hole (name == None): the
        // `policies[policy].name == NULL` guard rejects it.
        assert!(policies[4].name.is_none());
        assert!(Scheduling_newPriorityPanel(4, 50).is_none());
    }

    #[test]
    fn priority_panel_rejects_policies_without_priority_support() {
        // OTHER (0), BATCH (3), IDLE (5) have prioritySupport == false.
        assert!(Scheduling_newPriorityPanel(SCHED_OTHER, 50).is_none());
        assert!(Scheduling_newPriorityPanel(SCHED_BATCH, 50).is_none());
        assert!(Scheduling_newPriorityPanel(SCHED_IDLE, 50).is_none());
    }

    // --- Scheduling_rowSetPolicy: the Row*->Process* cast ------------------
    // The `Object_isA` guard + `Any` downcast run before the syscall `cfg`
    // split, so the type-check behavior is testable on any target.

    #[test]
    fn row_set_policy_accepts_a_process() {
        // A Process IS-A Row/Process; the debug_assert + downcast succeed
        // and control reaches the (cfg'd) syscall body. On non-Linux the
        // no-op body returns false; on Linux setting the current process to
        // its existing policy is exercised by the linux-only test below.
        let p = Process::default();
        let arg = SchedulingArg {
            policy: SCHED_OTHER,
            priority: 0,
        };
        let obj: &dyn Object = &p;
        let r = Scheduling_rowSetPolicy(obj, &arg);
        #[cfg(not(target_os = "linux"))]
        assert!(!r);
        // On Linux the result depends on privileges; just require it ran.
        #[cfg(target_os = "linux")]
        let _ = r;
    }

    #[test]
    #[should_panic]
    fn row_set_policy_rejects_non_process() {
        // A ListItem is not a Process; Object_isA(&Process_class) is false,
        // so the debug_assert fires (the C assert()).
        let item = ListItem {
            value: String::new(),
            key: 0,
            moving: false,
        };
        let arg = SchedulingArg {
            policy: SCHED_OTHER,
            priority: 0,
        };
        let obj: &dyn Object = &item;
        let _ = Scheduling_rowSetPolicy(obj, &arg);
    }

    // --- Linux-only: the real syscalls ------------------------------------

    #[cfg(target_os = "linux")]
    #[test]
    fn priority_panel_builds_for_fifo_on_linux() {
        // SCHED_FIFO supports priorities; the panel spans
        // sched_get_priority_min..=max, one row each, header "Priority:".
        let min = unsafe { libc::sched_get_priority_min(SCHED_FIFO) };
        let max = unsafe { libc::sched_get_priority_max(SCHED_FIFO) };
        assert!(min >= 0 && max >= min);

        let pre = min; // preselect the first priority
        let panel = Scheduling_newPriorityPanel(SCHED_FIFO, pre)
            .expect("FIFO priority panel must be Some on Linux");

        let expected_rows = (max - min + 1) as usize;
        assert_eq!(panel.items.len(), expected_rows);
        // First row's value is the numeric min; selection landed on it.
        assert_eq!(row_value(&panel, 0), format!("{}", min));
        assert_eq!(panel.selected, min);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_process_policy_reads_own_scheduler_on_linux() {
        // sched_getscheduler(getpid()) on an ordinary process is a valid,
        // non-negative policy (SCHED_OTHER == 0 for a normal process).
        let pid = unsafe { libc::getpid() };
        let mut proc_ = Process::default();
        proc_.super_.id = pid;

        Scheduling_readProcessPolicy(&mut proc_);

        let direct = unsafe { libc::sched_getscheduler(pid) };
        assert!(direct >= 0);
        assert_eq!(proc_.scheduling_policy, direct);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn set_policy_to_other_succeeds_on_linux() {
        // Setting the current process to SCHED_OTHER with priority 0 is
        // permitted without privileges (it is a no-op if already OTHER).
        let pid = unsafe { libc::getpid() };
        let mut proc_ = Process::default();
        proc_.super_.id = pid;

        let arg = SchedulingArg {
            policy: SCHED_OTHER,
            priority: 0,
        };
        assert!(Scheduling_setPolicy(&proc_, &arg));
    }
}
