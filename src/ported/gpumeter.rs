//! Port of `GPUMeter.c` — htop's GPU usage meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port.
//!
//! `GPUMeter_display` is now ported: its substrate (`RichString`,
//! `CRT_colors`, the `Meter` `values` field, and this module's own
//! `GPUMeter_engineData`/`totalUsage`/`totalGPUTimeDiff` file-statics) is
//! available. `GPUMeter_updateValues` stays a `todo!()` stub: it is driven
//! by `Platform_setGPUValues(this, &totalUsage, &totalGPUTimeDiff)`, whose
//! Rust port (`linux/platform.rs`) is still a no-arg stub with no output
//! params, so there is no faithful way to populate the statics.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::meter::Meter;
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendnAscii, RichString_writeAscii,
};

/// Port of `struct GPUMeterEngineData` from `GPUMeter.h:15`. Per-engine GPU
/// accounting row: `key` is the engine label (`const char*` owned by
/// `LinuxMachine` in the C, modeled as an owned `Option<String>` — `None`
/// reproduces the C `NULL` that terminates the `GPUMeter_display` loop),
/// `timeDiff` is the busy-time delta in nanoseconds (`-1ULL`/`u64::MAX`
/// meaning "unavailable"), and `percentage` is the utilisation.
pub struct GPUMeterEngineData {
    pub key: Option<String>,
    pub timeDiff: u64,
    pub percentage: f64,
}

/// Port of `struct GPUMeterEngineData GPUMeter_engineData[4]` from
/// `GPUMeter.c:20`. htop's file-scope (extern) array of per-engine rows,
/// written by the platform layer (`LinuxMachine`) and read by
/// `GPUMeter_display`. Zero-initialized like the C static storage
/// (`key = NULL`, `timeDiff = 0`, `percentage = 0.0`); held behind a
/// `Mutex` because a Rust mutable static needs interior mutability (the C
/// global is single-threaded and unlocked).
pub static GPUMeter_engineData: Mutex<[GPUMeterEngineData; 4]> = Mutex::new([
    GPUMeterEngineData {
        key: None,
        timeDiff: 0,
        percentage: 0.0,
    },
    GPUMeterEngineData {
        key: None,
        timeDiff: 0,
        percentage: 0.0,
    },
    GPUMeterEngineData {
        key: None,
        timeDiff: 0,
        percentage: 0.0,
    },
    GPUMeterEngineData {
        key: None,
        timeDiff: 0,
        percentage: 0.0,
    },
]);

/// Port of `static double totalUsage = NAN` from `GPUMeter.c:21`. The
/// aggregate GPU utilisation set by `GPUMeter_updateValues` and read by
/// `GPUMeter_display`; behind a `Mutex` for interior mutability.
static totalUsage: Mutex<f64> = Mutex::new(f64::NAN);

/// Port of `static unsigned long long int totalGPUTimeDiff = -1ULL` from
/// `GPUMeter.c:22`. The aggregate GPU busy-time delta (nanoseconds);
/// `-1ULL` is `u64::MAX`, meaning "unavailable".
static totalGPUTimeDiff: Mutex<u64> = Mutex::new(u64::MAX);

/// Port of `static size_t activeMeters` from `GPUMeter.c:32`. htop's
/// file-static counter of live GPU meters, mutated by `GPUMeter_init`
/// and `GPUMeter_done` and read by `GPUMeter_active`. Modeled as an
/// `AtomicUsize` (the C is a plain single-threaded `static size_t`;
/// `Relaxed` reproduces the exact behavior).
static ACTIVE_METERS: AtomicUsize = AtomicUsize::new(0);

/// Port of `bool GPUMeter_active(void)` from `GPUMeter.c:34`. True when
/// at least one GPU meter is live.
pub fn GPUMeter_active() -> bool {
    ACTIVE_METERS.load(Ordering::Relaxed) > 0
}

/// Port of `static int humanTimeUnit(char* buffer, size_t size,
/// unsigned long long int value)` from `GPUMeter.c:38`. Formats a
/// nanosecond `value` into a fixed 6-column-or-less human time string
/// (`ns`/`us`/`ms`/`s`/`m`/`h`/`d`), cascading through 1000/60/60/24
/// dividers exactly as the C does.
///
/// Signature mapping: the C writes into the caller's `char* buffer`
/// bounded by `size` and returns the `xSnprintf` byte count. Rust owns
/// its allocation, so the `buffer`/`size` out-param and the `int`
/// return are dropped in favor of an owned `String` — the same mapping
/// `meter.rs`/`xutils.rs` apply to the formatters. The `char buffer[50]`
/// is never overrun by any input, so no truncation logic is needed.
/// C's `%3llu` becomes `{:3}` (space-padded min-width 3) and `%1llu`
/// becomes `{:1}`.
pub fn humanTimeUnit(mut value: u64) -> String {
    if value < 1000 {
        return format!("{:3}ns", value);
    }

    if value < 10000 {
        return format!("{:1}.{:1}us", value / 1000, (value % 1000) / 100);
    }

    value /= 1000;

    if value < 1000 {
        return format!("{:3}us", value);
    }

    if value < 10000 {
        return format!("{:1}.{:1}ms", value / 1000, (value % 1000) / 100);
    }

    value /= 1000;

    if value < 1000 {
        return format!("{:3}ms", value);
    }

    if value < 10000 {
        return format!("{:1}.{:1}s", value / 1000, (value % 1000) / 100);
    }

    value /= 1000;

    if value < 600 {
        return format!("{:3}s", value);
    }

    value /= 60;

    if value < 600 {
        return format!("{:3}m", value);
    }

    value /= 60;

    if value < 96 {
        return format!("{:3}h", value);
    }

    value /= 24;

    format!("{:3}d", value)
}

/// Port of `static void GPUMeter_updateValues(Meter* this)` from
/// `GPUMeter.c:82`. Drives the ported
/// [`Platform_setGPUValues`](crate::ported::linux::platform::Platform_setGPUValues)
/// against the file-static `totalUsage`/`totalGPUTimeDiff` (which retain
/// their prior values across unchanged samples), then writes `txtBuffer` as
/// the aggregate usage `%.1f%%`, or `N/A` when usage is negative/NaN.
pub fn GPUMeter_updateValues(this: &mut Meter) {
    let mut tu = totalUsage.lock().unwrap();
    let mut td = totalGPUTimeDiff.lock().unwrap();
    crate::ported::linux::platform::Platform_setGPUValues(this, &mut tu, &mut td);

    // isNonnegative(totalUsage) — false for NaN.
    if !(*tu >= 0.0) {
        this.txtBuffer = "N/A".to_string();
        return;
    }
    this.txtBuffer = format!("{:.1}%", *tu);
}

/// Port of `static void GPUMeter_display(const Object* cast, RichString*
/// out)` from `GPUMeter.c:94`. The C casts `cast` back to `const Meter*`
/// and reads `this->values[i]`, so the down-cast collapses into the typed
/// `&Meter` parameter (the `loadaveragemeter.rs` precedent). `CRT_colors[X]`
/// is `ColorElements::X.packed(ColorScheme::active())`; the active scheme is
/// read once (a process-global that does not change mid-call). Each
/// `xSnprintf(buffer, ..., "%5.1f%%", v)` becomes `format!("{:5.1}%", v)`
/// and the `written` byte count becomes `buffer.len()`; `humanTimeUnit`
/// returns its owned `String` directly. `isNonnegative(x)` (`Macros.h:141`,
/// `isgreaterequal(x, 0.0)`, false for NaN) is inlined as `x >= 0.0`.
pub fn GPUMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();
    let meter_text = ColorElements::METER_TEXT.packed(scheme);
    let meter_value = ColorElements::METER_VALUE.packed(scheme);

    RichString_writeAscii(out, meter_text, b":");
    let total_usage = *totalUsage.lock().unwrap();
    if !(total_usage >= 0.0) {
        RichString_appendAscii(out, meter_value, b" N/A");
        return;
    }

    let buffer = format!("{:5.1}%", total_usage);
    RichString_appendnAscii(out, meter_value, buffer.as_bytes(), buffer.len());
    let total_time_diff = *totalGPUTimeDiff.lock().unwrap();
    if total_time_diff != u64::MAX {
        RichString_appendAscii(out, meter_text, b"(");
        let buffer = humanTimeUnit(total_time_diff);
        RichString_appendnAscii(out, meter_value, buffer.as_bytes(), buffer.len());
        RichString_appendAscii(out, meter_text, b")");
    }

    let engine_data = GPUMeter_engineData.lock().unwrap();
    for i in 0..engine_data.len() {
        let key = match &engine_data[i].key {
            Some(k) => k,
            None => break,
        };

        RichString_appendAscii(out, meter_text, b" ");
        RichString_appendAscii(out, meter_text, key.as_bytes());
        RichString_appendAscii(out, meter_text, b":");
        if this.values[i] >= 0.0 {
            let buffer = format!("{:5.1}%", this.values[i]);
            RichString_appendnAscii(out, meter_value, buffer.as_bytes(), buffer.len());
        } else {
            RichString_appendAscii(out, meter_value, b" N/A");
        }
        if engine_data[i].timeDiff != u64::MAX {
            RichString_appendAscii(out, meter_text, b"(");
            let buffer = humanTimeUnit(engine_data[i].timeDiff);
            RichString_appendnAscii(out, meter_value, buffer.as_bytes(), buffer.len());
            RichString_appendAscii(out, meter_text, b")");
        }
    }
}

/// Port of `static void GPUMeter_init(Meter* this ATTR_UNUSED)` from
/// `GPUMeter.c:137`. Increments the live-meter counter. The unused
/// `Meter*` param is dropped.
pub fn GPUMeter_init() {
    ACTIVE_METERS.fetch_add(1, Ordering::Relaxed);
}

/// Port of `static void GPUMeter_done(Meter* this ATTR_UNUSED)` from
/// `GPUMeter.c:141`. Decrements the live-meter counter, preserving the
/// C `assert(activeMeters > 0)` precondition. The unused `Meter*` param
/// is dropped.
pub fn GPUMeter_done() {
    assert!(ACTIVE_METERS.load(Ordering::Relaxed) > 0);
    ACTIVE_METERS.fetch_sub(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_time_unit_nanoseconds_width3() {
        // value < 1000: "%3lluns", right-padded to width 3.
        assert_eq!(humanTimeUnit(0), "  0ns");
        assert_eq!(humanTimeUnit(7), "  7ns");
        assert_eq!(humanTimeUnit(999), "999ns");
    }

    #[test]
    fn human_time_unit_sub_microsecond_fraction() {
        // 1000..10000: "%1llu.%1lluus" with value/1000 and (value%1000)/100.
        assert_eq!(humanTimeUnit(1000), "1.0us");
        assert_eq!(humanTimeUnit(1500), "1.5us");
        assert_eq!(humanTimeUnit(9999), "9.9us");
    }

    #[test]
    fn human_time_unit_microseconds_width3() {
        // 10000 -> /1000 = 10, < 1000: "%3lluus".
        assert_eq!(humanTimeUnit(10_000), " 10us");
        // 999_000 -> /1000 = 999, < 1000: "999us".
        assert_eq!(humanTimeUnit(999_000), "999us");
    }

    #[test]
    fn human_time_unit_millisecond_fraction_and_width() {
        // 1_000_000 -> /1000 = 1000 -> "%1llu.%1llums": 1000/1000=1, 0.
        assert_eq!(humanTimeUnit(1_000_000), "1.0ms");
        // 10_000_000 -> /1000=10000 -> /1000=10 (<1000): "%3llums".
        assert_eq!(humanTimeUnit(10_000_000), " 10ms");
    }

    #[test]
    fn human_time_unit_second_fraction() {
        // Two /1000 divides land 1000..10000 at the "%1llu.%1llus" branch.
        // 1e9 -> /1000=1e6 -> /1000=1000 (<10000): 1000/1000=1, 0 -> "1.0s".
        assert_eq!(humanTimeUnit(1_000_000_000), "1.0s");
        // 5e9 -> /1000=5e6 -> /1000=5000 (<10000): 5000/1000=5, 0 -> "5.0s".
        assert_eq!(humanTimeUnit(5_000_000_000), "5.0s");
    }

    #[test]
    fn human_time_unit_seconds_minutes_hours_days() {
        // 60 s = 6e10 ns -> after 3 /1000 = 60000 (>=10000) -> /1000 = 60 (<600): "%3llus".
        assert_eq!(humanTimeUnit(60_000_000_000), " 60s");
        // 10 min = 6e11 ns -> 600000 -> /1000 = 600 (>=600) -> /60 = 10 (<600): "%3llum".
        assert_eq!(humanTimeUnit(600_000_000_000), " 10m");
        // 10 h = 3.6e13 ns -> 36000000 -> /1000 = 36000 -> /60 = 600 (>=600)
        //   -> /60 = 10 (<96): "%3lluh".
        assert_eq!(humanTimeUnit(36_000_000_000_000), " 10h");
        // 4 d = 100 h path: 3.6e14 ns -> 360000000 -> /1000 = 360000 -> /60 = 6000
        //   -> /60 = 100 (>=96) -> /24 = 4: "%3llud".
        assert_eq!(humanTimeUnit(360_000_000_000_000), "  4d");
    }

    /// Visible characters of the valid `[0, chlen)` range of `out`.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    #[test]
    fn display_na_when_total_usage_unavailable() {
        // With the file-statics at their C defaults (totalUsage = NAN,
        // engineData all-NULL), GPUMeter_display takes the `!isNonnegative`
        // branch: ":" then " N/A", and never touches the engine loop. This
        // test only reads the statics (GPUMeter_updateValues, the only
        // writer, is stubbed and never runs), so it is order-independent.
        let m = Meter {
            host: core::ptr::null(),
            values: vec![0.0; 5],
            ..Meter::empty()
        };
        let mut out = RichString::new();
        GPUMeter_display(&m, &mut out);
        assert_eq!(text(&out), ": N/A");
    }

    #[test]
    fn active_meter_counter_lifecycle() {
        // Only test touching ACTIVE_METERS, so it starts and ends at 0.
        assert!(!GPUMeter_active());
        GPUMeter_init();
        assert!(GPUMeter_active());
        GPUMeter_init();
        assert!(GPUMeter_active());
        GPUMeter_done();
        assert!(GPUMeter_active());
        GPUMeter_done();
        assert!(!GPUMeter_active());
    }

    #[test]
    fn update_values_computes_usage_percentage() {
        use crate::ported::linux::linuxmachine::LinuxMachine;
        use crate::ported::machine::Machine;
        // total_gpu_time_diff = curGpuTime - prevGpuTime = 1e9; monotonic
        // delta = 2000ms → usage = 100 * 1e9 / 1e6 / 2000 = 50.0%.
        let host = Box::leak(Box::new(LinuxMachine {
            super_: Machine {
                monotonicMs: 2000,
                ..Default::default()
            },
            curGpuTime: 1_000_000_000,
            prevGpuTime: 0,
            gpuEngineData: None,
            ..Default::default()
        }));
        let mut m = Meter {
            values: vec![0.0; 5],
            host: &host.super_ as *const crate::ported::machine::Machine,
            ..Meter::empty()
        };
        GPUMeter_updateValues(&mut m);
        assert_eq!(m.txtBuffer, "50.0%");
        assert_eq!(m.curItems, 5);
    }
}
