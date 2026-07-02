//! Partial port of `TasksMeter.c` — htop's task-counter meter.
//!
//! [`TasksMeter_updateValues`] is a faithful port. [`TasksMeter_display`]
//! remains a `todo!()` stub: it drives `RichString`, `CRT_colors`, and
//! `Settings` (curses/color substrate not yet ported), so it cannot be
//! reproduced faithfully without inventing that substrate.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake` for
//! functions and `camelCase` for struct fields), so both the
//! `non_snake_case` function-name lint and struct-field lint are allowed
//! for the whole module — matching the spec name-for-name is the point.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// Minimal model of htop's `ProcessTable` — only the four counters
/// [`TasksMeter_updateValues`] reads (`ProcessTable.h:25-28`, all
/// `unsigned int`). Every other `ProcessTable` field is omitted because
/// the ported function never touches it.
pub struct ProcessTable {
    pub totalTasks: u32,
    pub runningTasks: u32,
    pub userlandThreads: u32,
    pub kernelThreads: u32,
}

/// Minimal model of htop's `Machine` — only `activeCPUs`
/// (`Machine.h:59`, `unsigned int`) and the `processTable` pointer that
/// the ported function dereferences. All other `Machine` fields are
/// omitted.
pub struct Machine {
    pub activeCPUs: u32,
    pub processTable: ProcessTable,
}

/// Minimal model of htop's `Meter` — the `values` output slots
/// (`Meter.h:126`, `double*`; `TasksMeter_class.maxItems == 4`), the
/// `txtBuffer` text field (`Meter.h:125`, `char[256]`), and the `host`
/// back-pointer (`Meter.h:115`, `const Machine*`). The C `txtBuffer` is
/// a fixed 256-byte buffer; the `"%u/%u"` of two `u32` is at most 21
/// bytes so it never truncates, hence an owned `String` (the same
/// mapping `meter.rs`/`xutils.rs` apply to `char*` formatters).
pub struct Meter {
    pub values: [f64; 4],
    pub txtBuffer: String,
    pub host: Machine,
}

/// Port of `TasksMeter.c:29`
/// (`static void TasksMeter_updateValues(Meter* this)`).
///
/// Fills the four meter values and the text buffer. The C reads
/// `pt = (const ProcessTable*) host->processTable`; here `processTable`
/// is modelled inline on [`Machine`]. `MINIMUM(a, b)` (`Macros.h:17`)
/// maps to `u32::min`. The `values[2]` subtraction is `unsigned int`
/// arithmetic in C, which wraps modulo 2^32, so `wrapping_sub` preserves
/// the exact result before the widening to `double`.
pub fn TasksMeter_updateValues(this: &mut Meter) {
    let host = &this.host;
    let pt = &host.processTable;

    this.values[0] = pt.kernelThreads as f64;
    this.values[1] = pt.userlandThreads as f64;
    this.values[2] =
        pt.totalTasks.wrapping_sub(pt.kernelThreads).wrapping_sub(pt.userlandThreads) as f64;
    this.values[3] = u32::min(pt.runningTasks, host.activeCPUs) as f64;

    this.txtBuffer = format!(
        "{}/{}",
        u32::min(pt.runningTasks, host.activeCPUs),
        pt.totalTasks
    );
}

/// TODO: port of `static void TasksMeter_display(const Object* cast, RichString* out` from `TasksMeter.c:41`.
pub fn TasksMeter_display() {
    todo!("port of TasksMeter.c:41")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(totalTasks: u32, runningTasks: u32, userlandThreads: u32, kernelThreads: u32, activeCPUs: u32) -> Meter {
        Meter {
            values: [0.0; 4],
            txtBuffer: String::new(),
            host: Machine {
                activeCPUs,
                processTable: ProcessTable {
                    totalTasks,
                    runningTasks,
                    userlandThreads,
                    kernelThreads,
                },
            },
        }
    }

    #[test]
    fn typical_counts_and_text() {
        // total=100, running=3, userland=40, kernel=10, activeCPUs=8.
        let mut m = meter(100, 3, 40, 10, 8);
        TasksMeter_updateValues(&mut m);
        assert_eq!(m.values[0], 10.0); // kernelThreads
        assert_eq!(m.values[1], 40.0); // userlandThreads
        assert_eq!(m.values[2], 50.0); // 100 - 10 - 40
        assert_eq!(m.values[3], 3.0); // MINIMUM(3, 8)
        assert_eq!(m.txtBuffer, "3/100"); // "%u/%u" MINIMUM(3,8)/100
    }

    #[test]
    fn minimum_clamps_running_to_active_cpus() {
        // runningTasks(12) > activeCPUs(4): MINIMUM picks activeCPUs.
        let mut m = meter(200, 12, 0, 0, 4);
        TasksMeter_updateValues(&mut m);
        assert_eq!(m.values[3], 4.0);
        assert_eq!(m.txtBuffer, "4/200");
    }

    #[test]
    fn minimum_keeps_running_when_below_active_cpus() {
        // runningTasks(2) < activeCPUs(16): MINIMUM picks runningTasks.
        let mut m = meter(50, 2, 0, 0, 16);
        TasksMeter_updateValues(&mut m);
        assert_eq!(m.values[3], 2.0);
        assert_eq!(m.txtBuffer, "2/50");
    }

    #[test]
    fn unsigned_subtraction_wraps_like_c() {
        // total(0) - kernel(1) - userland(0): C `unsigned int` wraps to
        // 2^32-1 before conversion to double. wrapping_sub preserves it.
        let mut m = meter(0, 0, 0, 1, 1);
        TasksMeter_updateValues(&mut m);
        assert_eq!(m.values[2], u32::MAX as f64); // 4294967295.0
    }

    #[test]
    fn all_threads_no_userland_tasks() {
        // total == kernel + userland: values[2] == 0.
        let mut m = meter(64, 0, 40, 24, 8);
        TasksMeter_updateValues(&mut m);
        assert_eq!(m.values[2], 0.0);
        assert_eq!(m.txtBuffer, "0/64"); // MINIMUM(0,8)=0
    }
}
