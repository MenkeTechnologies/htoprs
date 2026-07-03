//! Port of `CPUMeter.c` — the two self-contained helpers.
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
//! - [`CPUMeter_getUiName`] (`CPUMeter.c:78`) — builds the header
//!   setup-menu label. It reads only `Meter_uiName(this)` (the meter
//!   class's `uiName` vtable string, modeled as a [`Meter`] field),
//!   `this->param`, and `Settings_cpuId(settings, cpu)` — the latter a
//!   pure macro (`Settings.h:119`, `countCPUsFromOne ? cpu+1 : cpu`)
//!   inlined here over the modeled [`Settings::countCPUsFromOne`]. No
//!   curses/platform substrate is involved, so it ports faithfully.
//!
//! Not ported (and why) — every remaining function in `CPUMeter.c` needs
//! unported substrate, so each keeps its exact `todo!()` stub:
//! - `CPUMeter_init` (`:51`) — `Meter_setCaption`, and (on the
//!   multi-CPU branch) `Machine_getCPUPhysicalCoreID` /
//!   `Machine_getCPUThreadIndex`, which are platform-specific CPU-topology
//!   functions not modeled here.
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

/// Minimal stand-in for htop's `Settings` (`Settings.h`), modeling only
/// the flag [`CPUMeter_getUiName`] reads through the `Settings_cpuId`
/// macro (`Settings.h:119`). Every other `Settings` field is omitted.
pub struct Settings {
    /// `Settings.countCPUsFromOne` (`Settings.h:77`, `bool`) — when set,
    /// `Settings_cpuId` returns `cpu + 1` so CPUs are labeled 1-based.
    pub countCPUsFromOne: bool,
}

/// Minimal stand-in for htop's `Machine` (`Machine.h`), modeling only the
/// fields the ported functions read: `existingCPUs`
/// ([`AllCPUsMeter_getRange`]) and `settings` ([`CPUMeter_getUiName`],
/// through `this->host->settings`). Every other `Machine` field (process
/// table, CPU stats, active/online CPU counts, ...) is omitted.
pub struct Machine {
    /// `Machine.existingCPUs` — number of CPUs the platform reports.
    pub existingCPUs: u32,
    /// `Machine.settings` — the owning host's `Settings` (`this->host->settings`).
    pub settings: Settings,
}

/// Minimal stand-in for htop's `Meter` (`Meter.h`), modeling only what
/// the ported functions touch: the class name — `Meter_name(this)`
/// returns the `MeterClass.name` string — the class UI name —
/// `Meter_uiName(this)` returns the `MeterClass.uiName` string — the
/// `param` (processor index, `Meter.h:119`, `unsigned int`), and the host
/// machine (`this->host`). Every other `Meter` field (`values`, `mode`,
/// `h`, `w`, `meterData`, `txtBuffer`, `curItems`, ...) is omitted.
pub struct Meter {
    /// `Meter_name(this)` — the meter class's `.name` (e.g. `"AllCPUs"`,
    /// `"LeftCPUs2"`, `"RightCPUs8"`). Only the first byte is inspected.
    pub name: &'static str,
    /// `Meter_uiName(this)` — the meter class's `.uiName` display label
    /// (e.g. `"CPU"`), modeled as an instance field carrying that vtable
    /// constant, exactly the value the `Meter_uiName` macro yields.
    pub uiName: &'static str,
    /// `this->param` — the processor this meter tracks (0 == average).
    pub param: u32,
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
    todo!("port of CPUMeter.c:51: needs Meter_setCaption, Machine.activeCPUs/settings.showCPUSMTLabels, and Machine_getCPUPhysicalCoreID/Machine_getCPUThreadIndex on this module's Machine stand-in")
}

/// Port of `static void CPUMeter_getUiName(const Meter* this,
/// char* buffer, size_t length)` from `CPUMeter.c:78`. Builds the header
/// setup-menu label: for a per-CPU meter (`param > 0`) it appends the
/// (optionally 1-based) CPU id after the class UI name; for the average
/// meter (`param == 0`) it is just the UI name.
///
/// Signature mapping: the C writes into the caller's `char* buffer`
/// bounded by `length` and returns nothing. Rust owns its allocation, so
/// the `buffer`/`length` out-params are dropped in favor of a returned
/// owned `String` — the same mapping [`crate::ported::meter::Meter_humanUnit`]
/// applies to `char*` formatters. The C `assert(length > 0)` is a
/// debug-only precondition on that dropped buffer, so it is dropped too.
///
/// `Meter_uiName(this)` (`Meter.h`) yields the class `uiName` string,
/// modeled as [`Meter::uiName`]. `Settings_cpuId(settings, cpu)`
/// (`Settings.h:119`, `countCPUsFromOne ? cpu+1 : cpu`) is inlined over
/// `cpu = this->param - 1`; `param` is `unsigned int`, and the guard
/// `param > 0` makes the `param - 1` subtraction safe.
pub fn CPUMeter_getUiName(this: &Meter) -> String {
    if this.param > 0 {
        let cpu: u32 = this.param - 1;
        let id = if this.host.settings.countCPUsFromOne {
            cpu + 1
        } else {
            cpu
        };
        format!("{} {}", this.uiName, id)
    } else {
        this.uiName.to_string()
    }
}

// CPUMeter.h `CPU_METER_*` indices / count.
const CPU_METER_NICE: usize = 0;
const CPU_METER_NORMAL: usize = 1;
const CPU_METER_KERNEL: usize = 2;
const CPU_METER_IRQ: usize = 3;
const CPU_METER_SOFTIRQ: usize = 4;
const CPU_METER_STEAL: usize = 5;
const CPU_METER_GUEST: usize = 6;
const CPU_METER_IOWAIT: usize = 7;
const CPU_METER_FREQUENCY: usize = 8;
const CPU_METER_ITEMCOUNT: usize = 10;

/// Port of `static const int CPUMeter_attributes[]` (`CPUMeter.c`) — the
/// detailed (8-class) bar palette.
static CPUMETER_ATTRIBUTES: [i32; 8] = [
    crate::ported::crt::ColorElements::CPU_NICE as i32,
    crate::ported::crt::ColorElements::CPU_NORMAL as i32,
    crate::ported::crt::ColorElements::CPU_SYSTEM as i32,
    crate::ported::crt::ColorElements::CPU_IRQ as i32,
    crate::ported::crt::ColorElements::CPU_SOFTIRQ as i32,
    crate::ported::crt::ColorElements::CPU_STEAL as i32,
    crate::ported::crt::ColorElements::CPU_GUEST as i32,
    crate::ported::crt::ColorElements::CPU_IOWAIT as i32,
];
/// Port of `static const int CPUMeter_attributes_summary[]` (`CPUMeter.c`) —
/// the 4-class summary palette.
static CPUMETER_ATTRIBUTES_SUMMARY: [i32; 4] = [
    crate::ported::crt::ColorElements::CPU_NICE as i32,
    crate::ported::crt::ColorElements::CPU_NORMAL as i32,
    crate::ported::crt::ColorElements::CPU_SYSTEM as i32,
    crate::ported::crt::ColorElements::CPU_GUEST as i32,
];

/// Port of `static void CPUMeter_updateValues(Meter* this)` from
/// `CPUMeter.c:87`. Zeroes the value slots, picks the detailed/summary bar
/// palette, and — for a present, online CPU — fills the per-class figures
/// via the ported `Platform_setCPUValues`, then formats `txtBuffer` as
/// `<usage%> <freqMHz>` (each optional per `showCPUUsage`/`showCPUFrequency`).
/// Absent CPUs render `"absent"`, offline ones `"offline"`. Temperature is
/// omitted (no `BUILD_WITH_CPU_TEMP` in this build).
pub fn CPUMeter_updateValues(this: &mut crate::ported::meter::Meter) {
    for i in 0..CPU_METER_ITEMCOUNT {
        this.values[i] = 0.0;
    }

    let (show_cpu_usage, show_cpu_frequency, detailed, existing_cpus) = {
        let host = this
            .host
            .as_ref()
            .expect("CPUMeter_updateValues: this->host (C dereferences it)")
            .borrow();
        let s = host
            .super_
            .settings
            .as_ref()
            .expect("CPUMeter_updateValues: host->settings");
        (
            s.showCPUUsage,
            s.showCPUFrequency,
            s.detailedCPUTime,
            host.super_.existingCPUs,
        )
    };

    this.curAttributes = Some(if detailed {
        &CPUMETER_ATTRIBUTES[..]
    } else {
        &CPUMETER_ATTRIBUTES_SUMMARY[..]
    });

    let cpu = this.param;
    if cpu > existing_cpus {
        this.txtBuffer = "absent".to_string();
        return;
    }

    let percent = crate::ported::linux::platform::Platform_setCPUValues(this, cpu);
    // isNonnegative(percent) — false for NaN.
    if !(percent >= 0.0) {
        this.txtBuffer = "offline".to_string();
        return;
    }

    let mut cpu_usage = String::new();
    let mut cpu_frequency = String::new();
    if show_cpu_usage {
        cpu_usage = format!("{percent:.1}%");
    }
    if show_cpu_frequency {
        let f = this.values[CPU_METER_FREQUENCY];
        cpu_frequency = if f >= 0.0 {
            format!("{:>4}MHz", f as u32)
        } else {
            "N/A".to_string()
        };
    }

    let sep = if !cpu_usage.is_empty() && !cpu_frequency.is_empty() {
        " "
    } else {
        ""
    };
    this.txtBuffer = format!("{cpu_usage}{sep}{cpu_frequency}");
}

/// Port of `static void CPUMeter_display(const Object* cast, RichString*
/// out)` from `CPUMeter.c:147`. Appends the labeled per-class percentages —
/// the 8-class detailed line (`:`/`sy:`/`ni:`/`hi:`/`si:`/`st:`/`gu:`/`wa:`)
/// or the 4-class summary line (`:`/`sys:`/`low:`/`vir:`), each colored by
/// its `CRT_colors` class entry — plus the optional `freq:` field. `absent`/
/// `offline` short-circuit as in [`CPUMeter_updateValues`]. Temperature is
/// omitted (no `BUILD_WITH_CPU_TEMP`). `isNonnegative(x)` is `x >= 0.0`.
pub fn CPUMeter_display(
    this: &crate::ported::meter::Meter,
    out: &mut crate::ported::richstring::RichString,
) {
    use crate::ported::crt::{ColorElements as CE, ColorScheme};
    use crate::ported::richstring::{
        RichString_appendAscii, RichString_appendnAscii, RichString_appendnWide,
    };
    let scheme = ColorScheme::active();

    let (detailed, show_frequency, existing_cpus) = {
        let host = this
            .host
            .as_ref()
            .expect("CPUMeter_display: this->host (C dereferences it)")
            .borrow();
        let s = host
            .super_
            .settings
            .as_ref()
            .expect("CPUMeter_display: host->settings");
        (s.detailedCPUTime, s.showCPUFrequency, host.super_.existingCPUs)
    };

    // "%5.1f%% " — width-5 float, "%", trailing space.
    let pct = |v: f64| -> String { format!("{v:5.1}% ") };

    if this.param > existing_cpus {
        RichString_appendAscii(out, CE::METER_SHADOW.packed(scheme), b" absent");
        return;
    }
    if this.curItems == 0 {
        RichString_appendAscii(out, CE::METER_SHADOW.packed(scheme), b" offline");
        return;
    }

    let v = &this.values;
    let text = CE::METER_TEXT.packed(scheme);
    let buffer = pct(v[CPU_METER_NORMAL]);
    RichString_appendAscii(out, text, b":");
    RichString_appendnAscii(out, CE::CPU_NORMAL.packed(scheme), buffer.as_bytes(), buffer.len());

    if detailed {
        for (label, idx, color, gated) in [
            ("sy:", CPU_METER_KERNEL, CE::CPU_SYSTEM, false),
            ("ni:", CPU_METER_NICE, CE::CPU_NICE_TEXT, false),
            ("hi:", CPU_METER_IRQ, CE::CPU_IRQ, false),
            ("si:", CPU_METER_SOFTIRQ, CE::CPU_SOFTIRQ, false),
            ("st:", CPU_METER_STEAL, CE::CPU_STEAL, true),
            ("gu:", CPU_METER_GUEST, CE::CPU_GUEST, true),
            ("wa:", CPU_METER_IOWAIT, CE::CPU_IOWAIT, false),
        ] {
            if gated && !(v[idx] >= 0.0) {
                continue; // isNonnegative gate for steal/guest
            }
            let buffer = pct(v[idx]);
            RichString_appendAscii(out, text, label.as_bytes());
            RichString_appendnAscii(out, color.packed(scheme), buffer.as_bytes(), buffer.len());
        }
    } else {
        let buffer = pct(v[CPU_METER_KERNEL]);
        RichString_appendAscii(out, text, b"sys:");
        RichString_appendnAscii(out, CE::CPU_SYSTEM.packed(scheme), buffer.as_bytes(), buffer.len());
        let buffer = pct(v[CPU_METER_NICE]);
        RichString_appendAscii(out, text, b"low:");
        RichString_appendnAscii(out, CE::CPU_NICE_TEXT.packed(scheme), buffer.as_bytes(), buffer.len());
        if v[CPU_METER_IRQ] >= 0.0 {
            let buffer = pct(v[CPU_METER_IRQ]);
            RichString_appendAscii(out, text, b"vir:");
            RichString_appendnAscii(out, CE::CPU_GUEST.packed(scheme), buffer.as_bytes(), buffer.len());
        }
    }

    if show_frequency {
        let f = v[CPU_METER_FREQUENCY];
        let buffer = if f >= 0.0 {
            format!("{:>4}MHz ", f as u32)
        } else {
            "N/A     ".to_string()
        };
        RichString_appendAscii(out, text, b"freq: ");
        RichString_appendnWide(out, CE::METER_VALUE.packed(scheme), buffer.as_bytes(), buffer.len());
    }
}

/// TODO: port of `static void AllCPUsMeter_updateValues(Meter* this` from `CPUMeter.c:255`.
pub fn AllCPUsMeter_updateValues() {
    todo!("port of CPUMeter.c:255: needs CPUMeterData/Meter.meterData and Meter_updateValues")
}

/// TODO: port of `static void CPUMeterCommonInit(Meter* this` from `CPUMeter.c:264`.
pub fn CPUMeterCommonInit() {
    todo!("port of CPUMeter.c:264: needs CPUMeterData/Meter.meterData, Meter_new, and Meter_init")
}

/// TODO: port of `static void CPUMeterCommonUpdateMode(Meter* this, MeterModeId mode, int ncol` from `CPUMeter.c:285`.
pub fn CPUMeterCommonUpdateMode() {
    todo!("port of CPUMeter.c:285: needs CPUMeterData/Meter.meterData and Meter_setMode on the sub-meter array")
}

/// TODO: port of `static void AllCPUsMeter_done(Meter* this` from `CPUMeter.c:303`.
pub fn AllCPUsMeter_done() {
    todo!("port of CPUMeter.c:303")
}

/// TODO: port of `static void SingleColCPUsMeter_updateMode(Meter* this, MeterModeId mode` from `CPUMeter.c:314`.
pub fn SingleColCPUsMeter_updateMode() {
    todo!("port of CPUMeter.c:314: delegates to CPUMeterCommonUpdateMode, which needs CPUMeterData/Meter.meterData and Meter_setMode")
}

/// TODO: port of `static void DualColCPUsMeter_updateMode(Meter* this, MeterModeId mode` from `CPUMeter.c:318`.
pub fn DualColCPUsMeter_updateMode() {
    todo!("port of CPUMeter.c:318: delegates to CPUMeterCommonUpdateMode, which needs CPUMeterData/Meter.meterData and Meter_setMode")
}

/// TODO: port of `static void QuadColCPUsMeter_updateMode(Meter* this, MeterModeId mode` from `CPUMeter.c:322`.
pub fn QuadColCPUsMeter_updateMode() {
    todo!("port of CPUMeter.c:322: delegates to CPUMeterCommonUpdateMode, which needs CPUMeterData/Meter.meterData and Meter_setMode")
}

/// TODO: port of `static void OctoColCPUsMeter_updateMode(Meter* this, MeterModeId mode` from `CPUMeter.c:326`.
pub fn OctoColCPUsMeter_updateMode() {
    todo!("port of CPUMeter.c:326: delegates to CPUMeterCommonUpdateMode, which needs CPUMeterData/Meter.meterData and Meter_setMode")
}

/// TODO: port of `static void CPUMeterCommonDraw(Meter* this, int x, int y, int w, int ncol` from `CPUMeter.c:330`.
pub fn CPUMeterCommonDraw() {
    todo!("port of CPUMeter.c:330: needs CPUMeterData/Meter.meterData and the Meter draw vtable slot (meters[i]->draw)")
}

/// TODO: port of `static void DualColCPUsMeter_draw(Meter* this, int x, int y, int w` from `CPUMeter.c:346`.
pub fn DualColCPUsMeter_draw() {
    todo!("port of CPUMeter.c:346: delegates to CPUMeterCommonDraw, which needs CPUMeterData/Meter.meterData and the Meter draw vtable slot")
}

/// TODO: port of `static void QuadColCPUsMeter_draw(Meter* this, int x, int y, int w` from `CPUMeter.c:350`.
pub fn QuadColCPUsMeter_draw() {
    todo!("port of CPUMeter.c:350: delegates to CPUMeterCommonDraw, which needs CPUMeterData/Meter.meterData and the Meter draw vtable slot")
}

/// TODO: port of `static void OctoColCPUsMeter_draw(Meter* this, int x, int y, int w` from `CPUMeter.c:354`.
pub fn OctoColCPUsMeter_draw() {
    todo!("port of CPUMeter.c:354: delegates to CPUMeterCommonDraw, which needs CPUMeterData/Meter.meterData and the Meter draw vtable slot")
}

/// TODO: port of `static void SingleColCPUsMeter_draw(Meter* this, int x, int y, int w` from `CPUMeter.c:359`.
pub fn SingleColCPUsMeter_draw() {
    todo!("port of CPUMeter.c:359: needs CPUMeterData/Meter.meterData and the Meter draw vtable slot (meters[i]->draw)")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(name: &'static str, existingCPUs: u32) -> Meter {
        Meter {
            name,
            uiName: "CPU",
            param: 0,
            host: Machine {
                existingCPUs,
                settings: Settings {
                    countCPUsFromOne: false,
                },
            },
        }
    }

    /// Builds a `Meter` exercising [`CPUMeter_getUiName`]: sets the
    /// `uiName`, the tracked `param`, and the `countCPUsFromOne` flag.
    fn ui_meter(uiName: &'static str, param: u32, countCPUsFromOne: bool) -> Meter {
        Meter {
            name: "CPU",
            uiName,
            param,
            host: Machine {
                existingCPUs: 0,
                settings: Settings { countCPUsFromOne },
            },
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
    fn ui_name_average_meter_is_bare_ui_name() {
        // param == 0 (the "Avg" meter): buffer is just Meter_uiName.
        assert_eq!(CPUMeter_getUiName(&ui_meter("CPU", 0, false)), "CPU");
        assert_eq!(CPUMeter_getUiName(&ui_meter("CPU", 0, true)), "CPU");
    }

    #[test]
    fn ui_name_per_cpu_zero_based() {
        // param > 0, countCPUsFromOne off: id == param - 1.
        assert_eq!(CPUMeter_getUiName(&ui_meter("CPU", 1, false)), "CPU 0");
        assert_eq!(CPUMeter_getUiName(&ui_meter("CPU", 8, false)), "CPU 7");
    }

    #[test]
    fn ui_name_per_cpu_one_based() {
        // param > 0, countCPUsFromOne on: Settings_cpuId adds 1 back, so
        // id == (param - 1) + 1 == param.
        assert_eq!(CPUMeter_getUiName(&ui_meter("CPU", 1, true)), "CPU 1");
        assert_eq!(CPUMeter_getUiName(&ui_meter("CPU", 8, true)), "CPU 8");
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

#[cfg(test)]
mod cpu_data_tests {
    use crate::ported::linux::linuxmachine::{CPUData, LinuxMachine};
    use crate::ported::machine::{Machine, Settings};
    use crate::ported::meter::Meter;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn hosted(cpu: CPUData, settings: Settings, existing: u32) -> Meter {
        let host = Rc::new(RefCell::new(LinuxMachine {
            super_: Machine {
                existingCPUs: existing,
                settings: Some(settings),
                ..Default::default()
            },
            cpuData: vec![cpu],
            ..Default::default()
        }));
        Meter {
            values: vec![0.0; 10],
            param: 0,
            host: Some(host),
            ..Meter::empty()
        }
    }

    #[test]
    fn summary_mode_usage_percent() {
        // total=100: nice 10, user 50, systemAll 20 → summary curItems=4,
        // percent = 10+50+20+0 = 80 → "80.0%".
        let cpu = CPUData {
            online: true,
            totalPeriod: 100,
            userPeriod: 50,
            nicePeriod: 10,
            systemAllPeriod: 20,
            ..Default::default()
        };
        let settings = Settings {
            showCPUUsage: true,
            detailedCPUTime: false,
            ..Default::default()
        };
        let mut m = hosted(cpu, settings, 8);
        super::CPUMeter_updateValues(&mut m);
        assert_eq!(m.txtBuffer, "80.0%");
        assert_eq!(m.curItems, 4);
        assert_eq!(m.values[0], 10.0); // nice
        assert_eq!(m.values[1], 50.0); // normal(user)
    }

    #[test]
    fn offline_cpu_renders_offline() {
        let cpu = CPUData { online: false, ..Default::default() };
        let mut m = hosted(cpu, Settings { showCPUUsage: true, ..Default::default() }, 8);
        super::CPUMeter_updateValues(&mut m);
        assert_eq!(m.txtBuffer, "offline");
        assert_eq!(m.curItems, 0);
    }

    #[test]
    fn absent_cpu_renders_absent() {
        // param (0) > existingCPUs would need existing < 0; instead set the
        // meter param above existing via a fresh meter.
        let cpu = CPUData { online: true, totalPeriod: 100, ..Default::default() };
        let mut m = hosted(cpu, Settings::default(), 0);
        m.param = 5; // 5 > existingCPUs(0)
        super::CPUMeter_updateValues(&mut m);
        assert_eq!(m.txtBuffer, "absent");
    }

    fn text(r: &crate::ported::richstring::RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    #[test]
    fn display_summary_line() {
        // Summary line: ":" + normal + "sys:" + kernel + "low:" + nice.
        let cpu = CPUData {
            online: true,
            totalPeriod: 100,
            userPeriod: 50,   // normal 50.0
            nicePeriod: 10,   // nice 10.0
            systemAllPeriod: 20, // kernel 20.0 (summary)
            ..Default::default()
        };
        let settings = Settings { detailedCPUTime: false, ..Default::default() };
        let mut m = hosted(cpu, settings, 8);
        super::CPUMeter_updateValues(&mut m);
        let mut out = crate::ported::richstring::RichString::new();
        super::CPUMeter_display(&m, &mut out);
        // IRQ = steal+guest = 0 (nonnegative) → "vir:" 0.0 shown.
        assert_eq!(text(&out), ": 50.0% sys: 20.0% low: 10.0% vir:  0.0% ");
    }
}
