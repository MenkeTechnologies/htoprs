//! Port of `CPUMeter.c` — only the one pure-arithmetic helper.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! Ported:
//! - [`AllCPUsMeter_getRange`] (`CPUMeter.c:236`) — pure integer
//!   arithmetic over `this->host->existingCPUs` and the first character
//!   of the meter's class name (`Meter_name(this)[0]`), writing the
//!   `start`/`count` out-params. The two inputs it reads are modeled as
//!   small plain structs ([`Meter`] / [`Machine`], below).
//!
//! Not ported (and why) — every other function in `CPUMeter.c` needs
//! unported substrate, so each keeps its exact `todo!()` stub:
//! - `CPUMeter_init` (`:51`) — `Meter_setCaption`, `Machine_getCPU*`,
//!   `Settings_cpuId`, `xSnprintf` into a fixed caption buffer.
//! - `CPUMeter_getUiName` (`:78`) — `Meter_uiName`, `Settings_cpuId`.
//! - `CPUMeter_updateValues` (`:87`) — `Platform_setCPUValues`, the
//!   `Settings` flags, `CRT_degreeSign`, and writes to the `Meter`'s
//!   `values`/`curAttributes`/`txtBuffer` fields.
//! - `CPUMeter_display` (`:147`) — `RichString` and `CRT_colors[]`.
//! - `AllCPUsMeter_updateValues` (`:255`) — `Meter_updateValues` on the
//!   sub-meter array.
//! - `CPUMeterCommonInit` (`:264`) — `xCalloc`, `Meter_new`, `Meter_init`.
//! - `CPUMeterCommonUpdateMode` (`:285`) — `Meter_setMode` and reads
//!   `meters[0]->h`; the ceiling-division height is inseparable from the
//!   substrate calls around it.
//! - `AllCPUsMeter_done` (`:303`) — `Meter_delete` and `free`.
//! - `SingleColCPUsMeter_updateMode` / `DualColCPUsMeter_updateMode` /
//!   `QuadColCPUsMeter_updateMode` / `OctoColCPUsMeter_updateMode`
//!   (`:314`/`:318`/`:322`/`:326`) — thin wrappers delegating to the
//!   substrate-dependent `CPUMeterCommonUpdateMode`.
//! - `CPUMeterCommonDraw` (`:330`) — dispatches `meters[i]->draw(...)`.
//! - `DualColCPUsMeter_draw` / `QuadColCPUsMeter_draw` /
//!   `OctoColCPUsMeter_draw` / `SingleColCPUsMeter_draw`
//!   (`:346`/`:350`/`:354`/`:359`) — draw via the `Meter` vtable.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// Minimal stand-in for htop's `Machine` (`Machine.h`), modeling only the
/// field [`AllCPUsMeter_getRange`] reads: `existingCPUs` (an
/// `unsigned int`). Every other `Machine` field (process table, CPU
/// stats, `settings`, active/online CPU counts, ...) is omitted.
pub struct Machine {
    /// `Machine.existingCPUs` — number of CPUs the platform reports.
    pub existingCPUs: u32,
}

/// Minimal stand-in for htop's `Meter` (`Meter.h`), modeling only what
/// [`AllCPUsMeter_getRange`] touches: the class name — `Meter_name(this)`
/// returns the `MeterClass.name` string — and the host machine
/// (`this->host`). Every other `Meter` field (`values`, `mode`, `h`,
/// `w`, `meterData`, `txtBuffer`, `curItems`, `param`, ...) is omitted.
pub struct Meter {
    /// `Meter_name(this)` — the meter class's `.name` (e.g. `"AllCPUs"`,
    /// `"LeftCPUs2"`, `"RightCPUs8"`). Only the first byte is inspected.
    pub name: &'static str,
    /// `this->host` — the owning `Machine`.
    pub host: Machine,
}

/// Port of `static void AllCPUsMeter_getRange(const Meter* this,
/// int* start, int* count)` from `CPUMeter.c:236`. Computes the
/// `[start, start + count)` CPU index range a multi-column CPU meter
/// covers, dispatching on the first character of the meter's class name:
/// `'A'` (All) → the whole range, `'L'` (Left / first half) → the lower
/// `(cpus + 1) / 2`, `'R'` (Right / second half) → the remainder. Any
/// other first character falls through to the `'A'` behavior, exactly as
/// the C `switch`'s `default:` fallthrough into `case 'A':`.
///
/// Signature mapping: the C `int* start` / `int* count` out-params become
/// a returned `(start, count)` tuple (the same out-param → return mapping
/// `meter.rs` uses). `cpus` is `unsigned int` in C: the `(cpus + 1) / 2`
/// halving and the `cpus - *start` remainder are computed in `u32` so the
/// unsigned arithmetic is preserved, then cast to `i32` to match the C
/// `int` out-params (`*count = cpus` is likewise an unsigned→int store).
pub fn AllCPUsMeter_getRange(this: &Meter) -> (i32, i32) {
    let cpus: u32 = this.host.existingCPUs;
    let start: i32;
    let count: i32;
    match this.name.as_bytes().first().copied() {
        // 'L' — First Half
        Some(b'L') => {
            start = 0;
            count = ((cpus + 1) / 2) as i32;
        }
        // 'R' — Second Half
        Some(b'R') => {
            start = ((cpus + 1) / 2) as i32;
            count = (cpus - start as u32) as i32;
        }
        // default and 'A' — All
        _ => {
            start = 0;
            count = cpus as i32;
        }
    }
    (start, count)
}

/// TODO: port of `static void CPUMeter_init(Meter* this` from `CPUMeter.c:51`.
pub fn CPUMeter_init() {
    todo!("port of CPUMeter.c:51")
}

/// TODO: port of `static void CPUMeter_getUiName(const Meter* this, char* buffer, size_t length` from `CPUMeter.c:78`.
pub fn CPUMeter_getUiName() {
    todo!("port of CPUMeter.c:78")
}

/// TODO: port of `static void CPUMeter_updateValues(Meter* this` from `CPUMeter.c:87`.
pub fn CPUMeter_updateValues() {
    todo!("port of CPUMeter.c:87")
}

/// TODO: port of `static void CPUMeter_display(const Object* cast, RichString* out` from `CPUMeter.c:147`.
pub fn CPUMeter_display() {
    todo!("port of CPUMeter.c:147")
}

/// TODO: port of `static void AllCPUsMeter_updateValues(Meter* this` from `CPUMeter.c:255`.
pub fn AllCPUsMeter_updateValues() {
    todo!("port of CPUMeter.c:255")
}

/// TODO: port of `static void CPUMeterCommonInit(Meter* this` from `CPUMeter.c:264`.
pub fn CPUMeterCommonInit() {
    todo!("port of CPUMeter.c:264")
}

/// TODO: port of `static void CPUMeterCommonUpdateMode(Meter* this, MeterModeId mode, int ncol` from `CPUMeter.c:285`.
pub fn CPUMeterCommonUpdateMode() {
    todo!("port of CPUMeter.c:285")
}

/// TODO: port of `static void AllCPUsMeter_done(Meter* this` from `CPUMeter.c:303`.
pub fn AllCPUsMeter_done() {
    todo!("port of CPUMeter.c:303")
}

/// TODO: port of `static void SingleColCPUsMeter_updateMode(Meter* this, MeterModeId mode` from `CPUMeter.c:314`.
pub fn SingleColCPUsMeter_updateMode() {
    todo!("port of CPUMeter.c:314")
}

/// TODO: port of `static void DualColCPUsMeter_updateMode(Meter* this, MeterModeId mode` from `CPUMeter.c:318`.
pub fn DualColCPUsMeter_updateMode() {
    todo!("port of CPUMeter.c:318")
}

/// TODO: port of `static void QuadColCPUsMeter_updateMode(Meter* this, MeterModeId mode` from `CPUMeter.c:322`.
pub fn QuadColCPUsMeter_updateMode() {
    todo!("port of CPUMeter.c:322")
}

/// TODO: port of `static void OctoColCPUsMeter_updateMode(Meter* this, MeterModeId mode` from `CPUMeter.c:326`.
pub fn OctoColCPUsMeter_updateMode() {
    todo!("port of CPUMeter.c:326")
}

/// TODO: port of `static void CPUMeterCommonDraw(Meter* this, int x, int y, int w, int ncol` from `CPUMeter.c:330`.
pub fn CPUMeterCommonDraw() {
    todo!("port of CPUMeter.c:330")
}

/// TODO: port of `static void DualColCPUsMeter_draw(Meter* this, int x, int y, int w` from `CPUMeter.c:346`.
pub fn DualColCPUsMeter_draw() {
    todo!("port of CPUMeter.c:346")
}

/// TODO: port of `static void QuadColCPUsMeter_draw(Meter* this, int x, int y, int w` from `CPUMeter.c:350`.
pub fn QuadColCPUsMeter_draw() {
    todo!("port of CPUMeter.c:350")
}

/// TODO: port of `static void OctoColCPUsMeter_draw(Meter* this, int x, int y, int w` from `CPUMeter.c:354`.
pub fn OctoColCPUsMeter_draw() {
    todo!("port of CPUMeter.c:354")
}

/// TODO: port of `static void SingleColCPUsMeter_draw(Meter* this, int x, int y, int w` from `CPUMeter.c:359`.
pub fn SingleColCPUsMeter_draw() {
    todo!("port of CPUMeter.c:359")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(name: &'static str, existingCPUs: u32) -> Meter {
        Meter {
            name,
            host: Machine { existingCPUs },
        }
    }

    #[test]
    fn all_covers_whole_range() {
        // 'A' — start 0, count == cpus.
        assert_eq!(AllCPUsMeter_getRange(&meter("AllCPUs", 8)), (0, 8));
        assert_eq!(AllCPUsMeter_getRange(&meter("AllCPUs4", 12)), (0, 12));
    }

    #[test]
    fn default_first_char_falls_through_to_all() {
        // The C `switch` has `default:` fall into `case 'A':`, so any
        // first char that isn't 'L'/'R' yields the All range. "CPU"
        // starts with 'C'; 'X'/'z' likewise.
        assert_eq!(AllCPUsMeter_getRange(&meter("CPU", 6)), (0, 6));
        assert_eq!(AllCPUsMeter_getRange(&meter("Xyz", 6)), (0, 6));
        assert_eq!(AllCPUsMeter_getRange(&meter("z", 6)), (0, 6));
    }

    #[test]
    fn left_is_ceiling_half() {
        // 'L' — first half, rounded UP: (cpus + 1) / 2.
        assert_eq!(AllCPUsMeter_getRange(&meter("LeftCPUs", 8)), (0, 4));
        // Odd count: the extra CPU goes to the first (left) half.
        assert_eq!(AllCPUsMeter_getRange(&meter("LeftCPUs2", 5)), (0, 3));
        assert_eq!(AllCPUsMeter_getRange(&meter("LeftCPUs4", 7)), (0, 4));
    }

    #[test]
    fn right_is_remainder_after_left() {
        // 'R' — start at the left half's end, count is what's left.
        assert_eq!(AllCPUsMeter_getRange(&meter("RightCPUs", 8)), (4, 4));
        // Odd: left got the extra, so right is the smaller half.
        assert_eq!(AllCPUsMeter_getRange(&meter("RightCPUs2", 5)), (3, 2));
        assert_eq!(AllCPUsMeter_getRange(&meter("RightCPUs8", 7)), (4, 3));
    }

    #[test]
    fn left_and_right_partition_all_cpus() {
        // For every count, Left+Right must tile [0, cpus) with no gap or
        // overlap: right.start == left.count, and the counts sum to cpus.
        for cpus in 0u32..=64 {
            let (l_start, l_count) = AllCPUsMeter_getRange(&meter("LeftCPUs", cpus));
            let (r_start, r_count) = AllCPUsMeter_getRange(&meter("RightCPUs", cpus));
            assert_eq!(l_start, 0, "left always starts at 0 (cpus={cpus})");
            assert_eq!(
                r_start, l_count,
                "right starts where left ends (cpus={cpus})"
            );
            assert_eq!(
                l_count + r_count,
                cpus as i32,
                "halves sum to cpus (cpus={cpus})"
            );
            // Left never smaller than right (ceiling half on the left).
            assert!(l_count >= r_count, "left >= right (cpus={cpus})");
        }
    }

    #[test]
    fn zero_and_one_cpu_edges() {
        // cpus == 0: every range is empty.
        assert_eq!(AllCPUsMeter_getRange(&meter("AllCPUs", 0)), (0, 0));
        assert_eq!(AllCPUsMeter_getRange(&meter("LeftCPUs", 0)), (0, 0));
        assert_eq!(AllCPUsMeter_getRange(&meter("RightCPUs", 0)), (0, 0));
        // cpus == 1: All/Left take the single CPU, Right is empty.
        assert_eq!(AllCPUsMeter_getRange(&meter("AllCPUs", 1)), (0, 1));
        assert_eq!(AllCPUsMeter_getRange(&meter("LeftCPUs", 1)), (0, 1));
        assert_eq!(AllCPUsMeter_getRange(&meter("RightCPUs", 1)), (1, 0));
    }
}
