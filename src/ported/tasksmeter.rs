//! Partial port of `TasksMeter.c` — htop's task-counter meter.
//!
//! Both [`TasksMeter_updateValues`] and [`TasksMeter_display`] are faithful
//! ports. `display` drives the now-ported `RichString` / `CRT_colors`
//! substrate; the two `Settings` flags it reads
//! (`hideUserlandThreads`, `hideKernelThreads`) are modelled inline on the
//! local minimal [`Settings`], the same minimal-model approach this module
//! already takes for `ProcessTable`/`Machine`/`Meter`.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake` for
//! functions and `camelCase` for struct fields), so both the
//! `non_snake_case` function-name lint and struct-field lint are allowed
//! for the whole module — matching the spec name-for-name is the point.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_appendnAscii};

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

/// Minimal model of htop's `Settings` — only the two boolean flags
/// [`TasksMeter_display`] reads (`Settings.h`, both `bool`):
/// `hideUserlandThreads` and `hideKernelThreads`. Every other `Settings`
/// field is omitted because this module never touches it.
pub struct Settings {
    pub hideUserlandThreads: bool,
    pub hideKernelThreads: bool,
}

/// Minimal model of htop's `Machine` — only `activeCPUs`
/// (`Machine.h:59`, `unsigned int`), the `processTable` pointer that
/// [`TasksMeter_updateValues`] dereferences, and the `settings`
/// back-pointer (`Machine.h`, `const Settings*`) that
/// [`TasksMeter_display`] reads. All other `Machine` fields are omitted.
pub struct Machine {
    pub activeCPUs: u32,
    pub processTable: ProcessTable,
    pub settings: Settings,
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
    this.values[2] = pt
        .totalTasks
        .wrapping_sub(pt.kernelThreads)
        .wrapping_sub(pt.userlandThreads) as f64;
    this.values[3] = u32::min(pt.runningTasks, host.activeCPUs) as f64;

    this.txtBuffer = format!(
        "{}/{}",
        u32::min(pt.runningTasks, host.activeCPUs),
        pt.totalTasks
    );
}

/// Port of `static void TasksMeter_display(const Object* cast,
/// RichString* out)` from `TasksMeter.c:41`.
///
/// Appends the coloured task breakdown: `values[2]` (non-thread tasks) as
/// `METER_VALUE`, then userland threads (` thr`) and kernel threads
/// (` kthr`) — each dimmed to `METER_SHADOW` when the corresponding
/// `Settings` hide-flag is set, else coloured `TASKS_RUNNING`/`METER_TEXT`
/// — and finally the running count (`values[3]`) as `TASKS_RUNNING`.
///
/// The C `cast` back-cast to `const Meter*` is expressed as `this: &Meter`
/// (same idiom as the other `*_display` ports). `(int)this->values[X]`
/// truncates the `double` toward zero, so `as i32` matches. `CRT_colors[X]`
/// is `ColorElements::X.packed(scheme)`; the active scheme is read once (a
/// process-global that does not change mid-call), matching C's `CRT_colors`.
pub fn TasksMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();
    let settings = &this.host.settings;

    let buffer = format!("{}", this.values[2] as i32);
    RichString_appendnAscii(
        out,
        ColorElements::METER_VALUE.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );

    RichString_appendAscii(
        out,
        if settings.hideUserlandThreads {
            ColorElements::METER_SHADOW.packed(scheme)
        } else {
            ColorElements::METER_TEXT.packed(scheme)
        },
        b", ",
    );
    let buffer = format!("{}", this.values[1] as i32);
    RichString_appendnAscii(
        out,
        if settings.hideUserlandThreads {
            ColorElements::METER_SHADOW.packed(scheme)
        } else {
            ColorElements::TASKS_RUNNING.packed(scheme)
        },
        buffer.as_bytes(),
        buffer.len(),
    );
    RichString_appendAscii(
        out,
        if settings.hideUserlandThreads {
            ColorElements::METER_SHADOW.packed(scheme)
        } else {
            ColorElements::METER_TEXT.packed(scheme)
        },
        b" thr",
    );

    RichString_appendAscii(
        out,
        if settings.hideKernelThreads {
            ColorElements::METER_SHADOW.packed(scheme)
        } else {
            ColorElements::METER_TEXT.packed(scheme)
        },
        b", ",
    );
    let buffer = format!("{}", this.values[0] as i32);
    RichString_appendnAscii(
        out,
        if settings.hideKernelThreads {
            ColorElements::METER_SHADOW.packed(scheme)
        } else {
            ColorElements::TASKS_RUNNING.packed(scheme)
        },
        buffer.as_bytes(),
        buffer.len(),
    );
    RichString_appendAscii(
        out,
        if settings.hideKernelThreads {
            ColorElements::METER_SHADOW.packed(scheme)
        } else {
            ColorElements::METER_TEXT.packed(scheme)
        },
        b" kthr",
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b"; ");
    let buffer = format!("{}", this.values[3] as i32);
    RichString_appendnAscii(
        out,
        ColorElements::TASKS_RUNNING.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );
    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" running");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(
        totalTasks: u32,
        runningTasks: u32,
        userlandThreads: u32,
        kernelThreads: u32,
        activeCPUs: u32,
    ) -> Meter {
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
                settings: Settings {
                    hideUserlandThreads: false,
                    hideKernelThreads: false,
                },
            },
        }
    }

    /// Visible characters of the valid `[0, chlen)` range of `out`.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
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

    #[test]
    fn display_full_text() {
        // values populated by updateValues: [kthr, thr, tasks, running].
        let mut m = meter(100, 3, 40, 10, 8);
        TasksMeter_updateValues(&mut m);
        // values[2]=50, values[1]=40 thr, values[0]=10 kthr, values[3]=3.
        let mut out = RichString::new();
        TasksMeter_display(&m, &mut out);
        assert_eq!(text(&out), "50, 40 thr, 10 kthr; 3 running");
    }

    #[test]
    fn display_truncates_double_toward_zero() {
        // Non-integral values are cast (int), truncating toward zero.
        let mut m = meter(0, 0, 0, 0, 0);
        m.values = [1.9, 2.9, 3.9, 4.9];
        let mut out = RichString::new();
        TasksMeter_display(&m, &mut out);
        assert_eq!(text(&out), "3, 2 thr, 1 kthr; 4 running");
    }

    #[test]
    fn display_text_unaffected_by_hide_flags() {
        // Hide flags only change colour, never the emitted characters.
        let mut m = meter(100, 3, 40, 10, 8);
        m.host.settings.hideUserlandThreads = true;
        m.host.settings.hideKernelThreads = true;
        TasksMeter_updateValues(&mut m);
        let mut out = RichString::new();
        TasksMeter_display(&m, &mut out);
        assert_eq!(text(&out), "50, 40 thr, 10 kthr; 3 running");
    }
}
