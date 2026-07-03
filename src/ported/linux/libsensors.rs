//! Port of `linux/LibSensors.c`.
//!
//! The C file lives under `#ifdef HAVE_SENSORS_SENSORS_H` and talks to the
//! external `libsensors` C library (`dlopen`ed at runtime, or linked with
//! `BUILD_STATIC`). This port keeps the *behavior* — enumerate the CPU-temp
//! hwmon chips (`coretemp`/`via_cputemp`/`k10temp`/`zenpower`/…), read each
//! `tempN_input`, and map the values onto per-thread `CPUData::temperature`
//! with the package/average in slot 0 — but sources the data from the
//! **pure-Rust `libmedium` crate** (which reads `/sys/class/hwmon` directly,
//! no FFI, no `dlopen`, no C library) instead of `sensors_*`.
//!
//! Behavioral substitutions vs `LibSensors.c`:
//! - `sensors_get_detected_chips` loop → `libmedium::parse_hwmons()` +
//!   `Hwmons::iter()`.
//! - `chip->prefix` → `Hwmon::name()`.
//! - `sensors_get_features` (temp features) → `Hwmon::temps()` (a
//!   `BTreeMap<u16, impl TempSensor>`; the key is the `N` of `tempN`).
//! - `sensors_get_subfeature(TEMP_INPUT)` + `sensors_get_value` →
//!   `TempSensor::read_input()` → `Temperature::as_degrees_celsius()`.
//! - `sensors_get_label` → `SyncSensor::name()` (returns the `tempN_label`
//!   contents, or a `"tempN"` descriptor when unlabelled — same fallback the C
//!   library performs).
//! - The C `dlopenHandle != NULL` guard (used by `countCCDs`/
//!   `getCPUTemperatures` to short-circuit when the library never opened) is
//!   modelled by the `SENSORS_INITIALIZED` flag set in `LibSensors_init`.
//!
//! `libmedium` is a **Linux-only** dependency (`cfg(target_os = "linux")` in
//! `Cargo.toml`), so every entry point splits into a `#[cfg(target_os =
//! "linux")]` arm (the real port) and a `#[cfg(not(target_os = "linux"))]`
//! arm (htop's no-sensors build variant: init/cleanup/reload do nothing,
//! `getCPUTemperatures` leaves every `temperature` as NaN "no reading"). This
//! keeps the un-`cfg`-gated linux module compiling on darwin, where
//! `libmedium` is absent.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::linux::linuxmachine::CPUData;

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "linux")]
use libmedium::{
    parse_hwmons,
    sensors::sync_sensors::{temp::TempSensor, SyncSensor},
};

/// Substitute for the C `static void* dlopenHandle` (`LibSensors.c:51`).
/// `libmedium` has no shared-library handle to track; this flag records
/// whether `LibSensors_init` succeeded so `countCCDs`/`getCPUTemperatures` can
/// reproduce the C `if (!dlopenHandle) …` early-out.
#[cfg(target_os = "linux")]
static SENSORS_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Port of `int LibSensors_init(void)` from `LibSensors.c:55`.
///
/// The C original `dlopen`s `libsensors.so[.5|.4]` then calls
/// `sensors_init(NULL)`, returning `0` on success and `-1` on failure. The
/// pure-Rust `libmedium` substitute has nothing to `dlopen` — it reads
/// `/sys/class/hwmon` directly — so `parse_hwmons()` stands in for
/// `sensors_init`. Success is recorded in `SENSORS_INITIALIZED` (the
/// `dlopenHandle` substitute).
pub fn LibSensors_init() -> i32 {
    #[cfg(target_os = "linux")]
    {
        match parse_hwmons() {
            Ok(_) => {
                SENSORS_INITIALIZED.store(true, Ordering::Relaxed);
                0
            }
            Err(_) => -1,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        // No-sensors build variant: the hwmon backend is unavailable.
        -1
    }
}

/// Port of `void LibSensors_cleanup(void)` from `LibSensors.c:106`.
///
/// The C original calls `sensors_cleanup()` and `dlclose`s the handle.
/// `libmedium` holds no global state; clearing `SENSORS_INITIALIZED` mirrors
/// dropping the handle.
pub fn LibSensors_cleanup() {
    #[cfg(target_os = "linux")]
    {
        SENSORS_INITIALIZED.store(false, Ordering::Relaxed);
    }
}

/// Port of `int LibSensors_reload(void)` from `LibSensors.c:123`.
///
/// C: `if (!dlopenHandle) { errno = ENOTSUP; return -1; }` then
/// `sensors_cleanup(); return sensors_init(NULL);`. `libmedium` caches nothing
/// between calls, so a re-`parse_hwmons()` reproduces the reload/reprobe.
pub fn LibSensors_reload() -> i32 {
    #[cfg(target_os = "linux")]
    {
        if !SENSORS_INITIALIZED.load(Ordering::Relaxed) {
            return -1;
        }
        match parse_hwmons() {
            Ok(_) => 0,
            Err(_) => -1,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        -1
    }
}

/// Port of `static int tempDriverPriority(const sensors_chip_name* chip)` from
/// `LibSensors.c:135`. `chip->prefix` maps to `libmedium`'s `Hwmon::name()`.
/// Returns the driver priority (`0` = preferred CPU temp source, `1` = low
/// priority) or `-1` for a chip that is not a known CPU temperature source.
fn tempDriverPriority(prefix: &str) -> i32 {
    // Port of the C `static const struct TempDriverDefs tempDrivers[]` table.
    const TEMP_DRIVERS: &[(&str, i32)] = &[
        ("coretemp", 0),
        ("via_cputemp", 0),
        ("cpu_thermal", 0),
        ("k10temp", 0),
        ("zenpower", 0),
        // Rockchip RK3588
        ("littlecore_thermal", 0),
        ("bigcore0_thermal", 0),
        ("bigcore1_thermal", 0),
        ("bigcore2_thermal", 0),
        // Rockchip RK3566
        ("soc_thermal", 0),
        // Snapdragon 8cx
        ("cpu0_thermal", 0),
        ("cpu1_thermal", 0),
        ("cpu2_thermal", 0),
        ("cpu3_thermal", 0),
        ("cpu4_thermal", 0),
        ("cpu5_thermal", 0),
        ("cpu6_thermal", 0),
        ("cpu7_thermal", 0),
        // Amlogic S905W
        ("scpi_sensors", 0),
        // Snapdragon 410
        ("cpu0_1_thermal", 0),
        ("cpu2_3_thermal", 0),
        // Low priority drivers
        ("acpitz", 1),
    ];

    for &(p, priority) in TEMP_DRIVERS {
        if prefix == p {
            return priority;
        }
    }

    -1
}

/// Port of `int LibSensors_countCCDs(void)` from `LibSensors.c:177`.
///
/// Counts hwmon temperature sensors whose label starts with `"Tccd"` (AMD
/// per-CCD sensors). `sensors_get_features` (temp-only, name starts with
/// `"temp"`) → `Hwmon::temps()`; `sensors_get_label` → `SyncSensor::name()`.
pub fn LibSensors_countCCDs() -> i32 {
    #[cfg(target_os = "linux")]
    {
        // C: if (!dlopenHandle) return 0;
        if !SENSORS_INITIALIZED.load(Ordering::Relaxed) {
            return 0;
        }

        let hwmons = match parse_hwmons() {
            Ok(h) => h,
            Err(_) => return 0,
        };

        let mut ccds = 0;
        for hwmon in hwmons.iter() {
            // temps() only yields temp sensors, so the C guards
            // `feature->type != SENSORS_FEATURE_TEMP` and
            // `!String_startsWith(feature->name, "temp")` are implicit.
            for sensor in hwmon.temps().values() {
                if sensor.name().starts_with("Tccd") {
                    ccds += 1;
                }
            }
        }

        ccds
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Port of `static int LibSensors_stringToID(const char* str)` from
/// `LibSensors.c:209`.
///
/// Mirrors `strtoul(str, &endptr, 10)` followed by the C guard
/// `if (parsedID >= INT_MAX || *endptr != '\0') return -1;`. `strtoul` skips
/// leading whitespace and an optional sign, consumes the decimal digit run,
/// and points `endptr` at the first unconsumed byte (or at `str` itself when
/// no digits were converted); the guard therefore rejects any input that is
/// not a pure decimal number filling the whole string, or that reaches
/// `INT_MAX`. Overflow saturates to `u64::MAX` (matching `strtoul`'s
/// `ULONG_MAX`), so it is likewise rejected by the `>= INT_MAX` test.
fn LibSensors_stringToID(str: &str) -> i32 {
    let bytes = str.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // strtoul() skips leading whitespace.
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    // strtoul() accepts an optional leading sign.
    let mut negate = false;
    if i < len && (bytes[i] == b'+' || bytes[i] == b'-') {
        negate = bytes[i] == b'-';
        i += 1;
    }

    let digitStart = i;
    let mut parsedID: u64 = 0;
    while i < len && bytes[i].is_ascii_digit() {
        parsedID = parsedID
            .saturating_mul(10)
            .saturating_add((bytes[i] - b'0') as u64);
        i += 1;
    }

    // POSIX: when no digits are converted, strtoul returns 0 and stores the
    // original `nptr` in `endptr`; otherwise `endptr` is the first unconsumed
    // byte. `*endptr != '\0'` holds iff endptr is not at the terminating NUL.
    let endptr = if i == digitStart { 0 } else { i };
    let value = if negate {
        0u64.wrapping_sub(parsedID)
    } else {
        parsedID
    };

    if value >= i32::MAX as u64 || endptr < len {
        return -1;
    }
    value as i32
}

/// Port of `void LibSensors_getCPUTemperatures(CPUData* cpus, unsigned int
/// existingCPUs, unsigned int activeCPUs)` from `LibSensors.c:217`.
///
/// `cpus[0]` is the aggregate/package slot; `cpus[1..=existingCPUs]` are the
/// physical threads. Reads each detected CPU-temp chip's `tempN_input` via
/// `libmedium` and applies the same core-mapping/adjustment logic as the C
/// original (Intel `Core N`/`Package id N`, AMD `Tccd`/`Tctl`, and the
/// SoC-specific Snapdragon/Rockchip/Amlogic prefix maps), writing the result
/// into each `CPUData::temperature` (NaN = "no reading").
pub fn LibSensors_getCPUTemperatures(cpus: &mut [CPUData], existingCPUs: u32, activeCPUs: u32) {
    // C: assert(existingCPUs > 0 && existingCPUs < 16384);
    debug_assert!(existingCPUs > 0 && existingCPUs < 16384);

    let existing = existingCPUs as usize;

    // C: double* data = xMallocArray(existingCPUs + 1, ...); init to NAN.
    // `mut` is used only by the linux probing arm; unused on other targets.
    #[allow(unused_mut)]
    let mut data = vec![f64::NAN; existing + 1];

    #[cfg(target_os = "linux")]
    {
        let active = activeCPUs as usize;

        // C: if (!dlopenHandle) goto out;
        if SENSORS_INITIALIZED.load(Ordering::Relaxed) {
            if let Ok(hwmons) = parse_hwmons() {
                let mut coreTempCount: usize = 0;
                let mut topPriority: i32 = 99;
                let mut ccdID: i32 = 0;

                for hwmon in hwmons.iter() {
                    let prefix = hwmon.name();

                    let priority = tempDriverPriority(prefix);
                    if priority < 0 {
                        continue;
                    }
                    if priority > topPriority {
                        continue;
                    }
                    if priority < topPriority {
                        // Clear data from lower priority sensor.
                        for d in data.iter_mut() {
                            *d = f64::NAN;
                        }
                    }
                    topPriority = priority;

                    let mut physicalID: i32 = 0;

                    for (&tempIDraw, sensor) in hwmon.temps() {
                        // C: tempID = strtoul(feature->name + strlen("temp"));
                        //    if (tempID == 0 || tempID == ULONG_MAX) continue;
                        //    tempID--;  (feature IDs are 1-based; data is 0-based)
                        if tempIDraw == 0 {
                            continue;
                        }
                        let tempID = (tempIDraw - 1) as usize;

                        // C: subFeature TEMP_INPUT + sensors_get_value.
                        let temp = match sensor.read_input() {
                            Ok(t) => t.as_degrees_celsius(),
                            Err(_) => continue,
                        };

                        if existing == 8 {
                            let pb = prefix.as_bytes();
                            // Snapdragon 8cx cores: "cpuN_thermal", N in 0..=7.
                            if prefix.starts_with("cpu")
                                && pb.len() > 3
                                && pb[3] >= b'0'
                                && pb[3] <= b'7'
                                && &prefix[4..] == "_thermal"
                            {
                                data[1 + (pb[3] - b'0') as usize] = temp;
                                coreTempCount += 1;
                                continue;
                            }

                            // Rockchip cores: littlecore -> 1..4, bigcore0 ->
                            // 5,6, bigcore1/2 -> 7,8.
                            if prefix == "littlecore_thermal" {
                                data[1] = temp;
                                data[2] = temp;
                                data[3] = temp;
                                data[4] = temp;
                                coreTempCount += 4;
                                continue;
                            }
                            if prefix == "bigcore0_thermal" {
                                data[5] = temp;
                                data[6] = temp;
                                coreTempCount += 2;
                                continue;
                            }
                            if prefix == "bigcore1_thermal" || prefix == "bigcore2_thermal" {
                                data[7] = temp;
                                data[8] = temp;
                                coreTempCount += 2;
                                continue;
                            }
                        }

                        // Rockchip RK3566
                        if existing == 4 && prefix == "soc_thermal" {
                            data[1] = temp;
                            data[2] = temp;
                            data[3] = temp;
                            data[4] = temp;
                            coreTempCount += 4;
                            continue;
                        }

                        // Snapdragon 410
                        if existing == 4 {
                            if prefix == "cpu0_1_thermal" {
                                data[1] = temp;
                                data[2] = temp;
                                coreTempCount += 2;
                                continue;
                            }
                            if prefix == "cpu2_3_thermal" {
                                data[3] = temp;
                                data[4] = temp;
                                coreTempCount += 2;
                                continue;
                            }
                        }

                        // Amlogic S905W — package temperature for all cores.
                        if prefix == "scpi_sensors" {
                            for d in data.iter_mut() {
                                *d = temp;
                            }
                            coreTempCount = existing;
                            continue;
                        }

                        // C: char* label = sensors_get_label(chip, feature);
                        let label = sensor.name();
                        {
                            let mut skip = true;
                            // Intel coretemp labels mention package/physical id.
                            if let Some(rest) = label.strip_prefix("Package id ") {
                                let id = LibSensors_stringToID(rest);
                                if id != -1 {
                                    physicalID = id;
                                }
                            } else if let Some(rest) = label.strip_prefix("Physical id ") {
                                let id = LibSensors_stringToID(rest);
                                if id != -1 {
                                    physicalID = id;
                                }
                            } else if let Some(rest) = label.strip_prefix("Core ") {
                                let id = LibSensors_stringToID(rest);
                                if id != -1 {
                                    for i in 1..=existing {
                                        if cpus[i].physicalID == physicalID && cpus[i].coreID == id
                                        {
                                            data[i] = temp;
                                            coreTempCount += 1;
                                        }
                                    }
                                }
                            }
                            // AMD k10temp/zenpower: only CCD is known.
                            else if label.starts_with("Tccd") {
                                for i in 1..=existing {
                                    if cpus[i].ccdID == ccdID {
                                        data[i] = temp;
                                        coreTempCount += 1;
                                    }
                                }
                                ccdID += 1;
                            }
                            // AMD k10temp with only one general Tctl.
                            else if label == "Tctl" {
                                for i in 0..=existing {
                                    if data[i].is_nan() {
                                        data[i] = temp;
                                        if i > 0 {
                                            coreTempCount += 1;
                                        }
                                    }
                                }
                            } else {
                                skip = false;
                            }

                            if skip {
                                continue;
                            }
                        }

                        // C: if (tempID > existingCPUs) continue;
                        if tempID > existing {
                            continue;
                        }

                        // If already set (e.g. Ryzen platform temp per die),
                        // keep the larger value.
                        if data[tempID].is_nan() {
                            data[tempID] = temp;
                            if tempID > 0 {
                                coreTempCount += 1;
                            }
                        } else {
                            data[tempID] = data[tempID].max(temp);
                        }
                    }
                }

                // Adjustments (C: LibSensors.c:418..465). `goto out` short-
                // circuits to the final copy — modelled with `break 'adjust`.
                'adjust: {
                    // Adjust data for chips not providing a platform temp.
                    if coreTempCount + 1 == active || coreTempCount + 1 == active / 2 {
                        // C: memmove(&data[1], &data[0], existingCPUs * ...);
                        data.copy_within(0..existing, 1);
                        data[0] = f64::NAN;
                        coreTempCount += 1;
                    }

                    // Only package temperature — copy to all cores.
                    if coreTempCount == 0 && !data[0].is_nan() {
                        for i in 1..=existing {
                            data[i] = data[0];
                        }
                        break 'adjust;
                    }

                    // No package temperature — set to max core temperature.
                    if coreTempCount > 0 && data[0].is_nan() {
                        let mut maxTemp = f64::NEG_INFINITY;
                        for i in 1..=existing {
                            // C: isgreater(data[i], maxTemp) — false for NaN.
                            if data[i] > maxTemp {
                                maxTemp = data[i];
                                data[0] = data[i];
                            }
                        }
                    }

                    // Only temperature for core 0 (maybe Ryzen) — copy to rest.
                    if coreTempCount == 1 && !data[1].is_nan() {
                        for i in 2..=existing {
                            data[i] = data[1];
                        }
                        break 'adjust;
                    }

                    // Half the temperatures (probably HT/SMT) — copy to 2nd half.
                    let delta = active / 2;
                    if coreTempCount == delta && delta > 0 {
                        // C: memcpy(&data[delta + 1], &data[1], delta * ...);
                        data.copy_within(1..1 + delta, delta + 1);
                        break 'adjust;
                    }
                }
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        // No-sensors build variant: nothing probes; `data` stays all-NaN.
        let _ = activeCPUs;
    }

    // C out: for (i = 0; i <= existingCPUs; i++) cpus[i].temperature = data[i];
    for i in 0..=existing {
        cpus[i].temperature = data[i];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn LibSensors_stringToID_parses_plain_decimal() {
        assert_eq!(LibSensors_stringToID("0"), 0);
        assert_eq!(LibSensors_stringToID("7"), 7);
        assert_eq!(LibSensors_stringToID("12"), 12);
    }

    #[test]
    fn LibSensors_stringToID_rejects_trailing_garbage() {
        assert_eq!(LibSensors_stringToID("12abc"), -1);
        assert_eq!(LibSensors_stringToID("1 "), -1);
        assert_eq!(LibSensors_stringToID("abc"), -1);
    }

    #[test]
    fn LibSensors_stringToID_rejects_at_or_above_int_max() {
        // INT_MAX itself is rejected (>=).
        assert_eq!(LibSensors_stringToID(&i32::MAX.to_string()), -1);
        assert_eq!(
            LibSensors_stringToID(&(i32::MAX as i64 - 1).to_string()),
            i32::MAX - 1
        );
        // Overflow saturates to u64::MAX and is rejected.
        assert_eq!(LibSensors_stringToID("999999999999999999999999"), -1);
    }

    #[test]
    fn LibSensors_stringToID_empty_string_is_zero() {
        // strtoul("") -> 0 with endptr at the NUL, so the guard passes.
        assert_eq!(LibSensors_stringToID(""), 0);
    }

    #[test]
    fn LibSensors_stringToID_negative_is_rejected() {
        // strtoul negates modulo 2^64 -> huge value -> rejected by >= INT_MAX.
        assert_eq!(LibSensors_stringToID("-5"), -1);
    }
}
