//! Port of `GPUMeter.c` — htop's GPU usage meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port.
//!
//! `GPUMeter_updateValues` and `GPUMeter_display` depend on unported
//! substrate (`Platform_setGPUValues`, the `Meter` `txtBuffer`/`values`
//! fields, `RichString`, and `CRT_colors`), so they are left as their
//! exact `todo!()` stubs.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

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

/// TODO: port of `static void GPUMeter_updateValues(Meter* this` from `GPUMeter.c:82`.
pub fn GPUMeter_updateValues() {
    todo!("port of GPUMeter.c:82")
}

/// TODO: port of `static void GPUMeter_display(const Object* cast, RichString* out` from `GPUMeter.c:94`.
pub fn GPUMeter_display() {
    todo!("port of GPUMeter.c:94")
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
}
