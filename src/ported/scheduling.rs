//! Stub scaffold for `Scheduling.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `Scheduling.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

// Linux `<sched.h>` scheduling-policy constants, used verbatim by
// `Scheduling_formatPolicy` (the only member reproducible without
// syscalls/UI substrate). Values match `libc` 0.2.186
// (`unix/linux_like`): SCHED_OTHER=0, SCHED_FIFO=1, SCHED_RR=2,
// SCHED_BATCH=3, SCHED_IDLE=5, SCHED_DEADLINE=6,
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

/// TODO: port of `Panel* Scheduling_newPolicyPanel(int preSelectedPolicy` from `Scheduling.c:42`.
pub fn Scheduling_newPolicyPanel() {
    todo!("port of Scheduling.c:42")
}

/// TODO: port of `void Scheduling_togglePolicyPanelResetOnFork(Panel* schedPanel` from `Scheduling.c:62`.
pub fn Scheduling_togglePolicyPanelResetOnFork() {
    todo!("port of Scheduling.c:62")
}

/// TODO: port of `Panel* Scheduling_newPriorityPanel(int policy, int preSelectedPriority` from `Scheduling.c:74`.
pub fn Scheduling_newPriorityPanel() {
    todo!("port of Scheduling.c:74")
}

/// TODO: port of `static bool Scheduling_setPolicy(Process* p, Arg arg` from `Scheduling.c:102`.
pub fn Scheduling_setPolicy() {
    todo!("port of Scheduling.c:102")
}

/// TODO: port of `bool Scheduling_rowSetPolicy(Row* row, Arg arg` from `Scheduling.c:124`.
pub fn Scheduling_rowSetPolicy() {
    todo!("port of Scheduling.c:124")
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

/// TODO: port of `void Scheduling_readProcessPolicy(Process* proc` from `Scheduling.c:162`.
pub fn Scheduling_readProcessPolicy() {
    todo!("port of Scheduling.c:162")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
