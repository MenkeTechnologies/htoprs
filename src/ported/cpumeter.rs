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
#![allow(non_upper_case_globals)] // faithful C global names (CPUMeter_class)
#![allow(dead_code)]

// Unified to the canonical `Meter` (`meter.rs`) — the earlier per-file
// `Meter`/`Machine`/`Settings` stubs are gone; `getRange`/`getUiName` now read
// the same `Meter` (with its `name`/`uiName`/`param` fields and `*const Machine`
// host) that `updateValues`/`display` use.
use std::io::Write;

use crate::ported::meter::{
    Meter, MeterClass, MeterModeId, Meter_class, Meter_new, Meter_setCaption, Meter_setMode,
    BAR_METERMODE, METERMODE_DEFAULT_SUPPORTED,
};
use crate::ported::object::ObjectClass;

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
    let cpus: u32 = unsafe { (*this.host).existingCPUs };
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

/// Port of `static void CPUMeter_init(Meter* this)` from `CPUMeter.c:51`.
///
/// Sets the meter caption from its `param`: the average meter (`param == 0`)
/// is `"Avg"`; a per-CPU meter on a multi-CPU host (`activeCPUs > 1`) is the
/// CPU id, either `"%2d%c"` (physical-core id + a thread letter) when
/// `showCPUSMTLabels` is set, or `"%3u"` otherwise. On a single-CPU host a
/// per-CPU meter keeps its default caption (the C `else if` has no `else`).
///
/// `Settings_cpuId(settings, id)` (`Settings.h:119`,
/// `countCPUsFromOne ? id + 1 : id`) is inlined, as in [`CPUMeter_getUiName`].
/// The SMT helpers `Machine_getCPUPhysicalCoreID`/`Machine_getCPUThreadIndex`
/// are the platform machine's (darwin takes the base `Machine`, linux the
/// concrete `LinuxMachine` reached by the `super_`-at-offset-0 downcast).
pub fn CPUMeter_init(this: &mut Meter) {
    let cpu = this.param;
    if cpu == 0 {
        Meter_setCaption(this, "Avg");
        return;
    }

    let (active_cpus, show_smt, count_from_one) = unsafe {
        let host = &*this.host;
        let s = host
            .settings
            .as_ref()
            .expect("CPUMeter_init: host->settings");
        (host.activeCPUs, s.showCPUSMTLabels, s.countCPUsFromOne)
    };

    if active_cpus <= 1 {
        return;
    }

    let caption = if show_smt {
        // Machine_getCPUPhysicalCoreID / Machine_getCPUThreadIndex — the
        // platform machine's SMT topology helpers.
        #[cfg(target_os = "macos")]
        let (core_id, thread_index) = unsafe {
            use crate::ported::darwin::darwinmachine::{
                Machine_getCPUPhysicalCoreID, Machine_getCPUThreadIndex,
            };
            let host = &*this.host;
            (
                Machine_getCPUPhysicalCoreID(host, cpu - 1),
                Machine_getCPUThreadIndex(host, cpu - 1),
            )
        };
        #[cfg(not(target_os = "macos"))]
        let (core_id, thread_index) = unsafe {
            use crate::ported::linux::linuxmachine::{
                LinuxMachine, Machine_getCPUPhysicalCoreID, Machine_getCPUThreadIndex,
            };
            // Machine* → LinuxMachine* (super_ at offset 0, #[repr(C)]).
            let lm = &*(this.host as *const LinuxMachine);
            (
                Machine_getCPUPhysicalCoreID(lm, cpu - 1),
                Machine_getCPUThreadIndex(lm, cpu - 1),
            )
        };

        let mut thread_letter = b'a' + (thread_index % 26) as u8;
        // > 26 threads/core → capitals; > 52 → repeats, but far apart.
        if (thread_index % 52) > 26 {
            thread_letter -= b'a' - b'A';
        }
        let core_disp = if count_from_one { core_id + 1 } else { core_id };
        format!("{:2}{}", core_disp, thread_letter as char)
    } else {
        let id = cpu - 1;
        let disp = if count_from_one { id + 1 } else { id };
        format!("{:3}", disp)
    };
    Meter_setCaption(this, &caption);
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
        let id = if unsafe {
            (*this.host)
                .settings
                .as_ref()
                .expect("CPUMeter_getUiName: host->settings")
                .countCPUsFromOne
        } {
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

/// Port of `const MeterClass CPUMeter_class` from `CPUMeter.c:371` — the
/// single per-CPU / average meter. All four vtable slots it uses are ported:
/// [`CPUMeter_updateValues`], [`CPUMeter_display`], [`CPUMeter_getUiName`],
/// and [`CPUMeter_init`]. A percent chart (`total = 100.0`), default
/// `BAR_METERMODE`, `maxItems = CPU_METER_ITEMCOUNT` (10). `super.delete`
/// dropped (Rust `Drop`); `super.extends` → the `Meter_class` base link.
///
/// (The multi-column `AllCPUs*`/`Left*`/`Right*` classes are not yet
/// registered — their draw/updateMode/init/done/updateValues slots remain
/// `todo!()` stubs.)
pub static CPUMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: Some(CPUMeter_display),
    init: Some(CPUMeter_init),
    done: None,
    updateMode: None,
    updateValues: Some(CPUMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: Some(CPUMeter_getUiName),
    defaultMode: BAR_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 100.0,
    attributes: &CPUMETER_ATTRIBUTES,
    name: "CPU",
    uiName: "CPU",
    caption: "CPU",
    description: None,
    maxItems: CPU_METER_ITEMCOUNT as u8,
    isMultiColumn: false,
    isPercentChart: true,
};

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

    let (show_cpu_usage, show_cpu_frequency, detailed, existing_cpus) = unsafe {
        let host = &*this.host;
        let s = host
            .settings
            .as_ref()
            .expect("CPUMeter_updateValues: host->settings");
        (
            s.showCPUUsage,
            s.showCPUFrequency,
            s.detailedCPUTime,
            host.existingCPUs,
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

    // Platform dispatch (darwin-first): this build's CPU value setter.
    #[cfg(target_os = "macos")]
    let percent = crate::ported::darwin::platform::Platform_setCPUValues(this, cpu);
    #[cfg(not(target_os = "macos"))]
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

    let (detailed, show_frequency, existing_cpus) = unsafe {
        let host = &*this.host;
        let s = host
            .settings
            .as_ref()
            .expect("CPUMeter_display: host->settings");
        (s.detailedCPUTime, s.showCPUFrequency, host.existingCPUs)
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
    RichString_appendnAscii(
        out,
        CE::CPU_NORMAL.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );

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
        RichString_appendnAscii(
            out,
            CE::CPU_SYSTEM.packed(scheme),
            buffer.as_bytes(),
            buffer.len(),
        );
        let buffer = pct(v[CPU_METER_NICE]);
        RichString_appendAscii(out, text, b"low:");
        RichString_appendnAscii(
            out,
            CE::CPU_NICE_TEXT.packed(scheme),
            buffer.as_bytes(),
            buffer.len(),
        );
        if v[CPU_METER_IRQ] >= 0.0 {
            let buffer = pct(v[CPU_METER_IRQ]);
            RichString_appendAscii(out, text, b"vir:");
            RichString_appendnAscii(
                out,
                CE::CPU_GUEST.packed(scheme),
                buffer.as_bytes(),
                buffer.len(),
            );
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
        RichString_appendnWide(
            out,
            CE::METER_VALUE.packed(scheme),
            buffer.as_bytes(),
            buffer.len(),
        );
    }
}

/// Port of `typedef struct CPUMeterData_` (`CPUMeter.c:46`): the multi-column
/// CPU meter's private `meterData`. `meters` holds one sub-`Meter` per CPU in
/// the meter's range — the C `Meter**` array of heap `Meter*` becomes an owned
/// `Vec<Meter>` (dropping it reclaims the sub-meters, replacing the C `free`).
struct CPUMeterData {
    cpus: u32,
    meters: Vec<Meter>,
}

impl CPUMeterData {
    /// Borrows `this.meterData` as the `CPUMeterData` a multi-column CPU meter
    /// stores there (set by [`CPUMeterCommonInit`]). An associated fn (not a
    /// free `fn`) — it is a Rust-only borrow helper with no C counterpart, so
    /// it lives in an `impl` where the port-purity gate does not require a
    /// `/// Port of` origin (the [`Meter::empty`] precedent).
    fn of(this: &mut Meter) -> &mut CPUMeterData {
        this.meterData
            .as_mut()
            .and_then(|d| d.downcast_mut::<CPUMeterData>())
            .expect("CPU meter: meterData is not an initialized CPUMeterData")
    }
}

/// Port of `static void AllCPUsMeter_updateValues(Meter* this)` from
/// `CPUMeter.c:255`. Dispatches `Meter_updateValues` on each sub-meter in the
/// meter's CPU range.
pub fn AllCPUsMeter_updateValues(this: &mut Meter) {
    let (_start, count) = AllCPUsMeter_getRange(this);
    let data = CPUMeterData::of(this);
    for i in 0..count as usize {
        let m = &mut data.meters[i];
        let uv = m
            .updateValues
            .expect("AllCPUsMeter_updateValues: sub-meter updateValues");
        uv(m);
    }
}

/// Port of `static void CPUMeterCommonInit(Meter* this)` from `CPUMeter.c:264`.
/// Allocates the [`CPUMeterData`] on first use (recording `existingCPUs`),
/// then ensures one sub-`Meter` of class [`CPUMeter_class`] exists per CPU in
/// the range (`param = start + i + 1`) and runs its `init`. `Meter_new`
/// already runs the class `init` + default `Meter_setMode`, so freshly-created
/// sub-meters are ready; pre-existing ones are re-`init`ed (C `Meter_init`),
/// which for `CPUMeter` is [`CPUMeter_init`].
pub fn CPUMeterCommonInit(this: &mut Meter) {
    let (start, count) = AllCPUsMeter_getRange(this);
    let host = this.host;
    if this.meterData.is_none() {
        let cpus = unsafe { (*host).existingCPUs };
        this.meterData = Some(Box::new(CPUMeterData {
            cpus,
            meters: Vec::new(),
        }));
    }
    let data = CPUMeterData::of(this);
    for i in 0..count as usize {
        if i < data.meters.len() {
            CPUMeter_init(&mut data.meters[i]);
        } else {
            // Meter_new runs CPUMeter_init + Meter_setMode(default) internally.
            let param = (start + i as i32 + 1) as u32;
            data.meters.push(Meter_new(host, param, &CPUMeter_class));
        }
    }
}

/// Port of `static void CPUMeterCommonUpdateMode(Meter* this, MeterModeId
/// mode, int ncol)` from `CPUMeter.c:285`. Sets the meter mode, applies it to
/// every sub-meter, and computes the container height as `subMeterHeight *
/// ceil(count / ncol)`. An empty range collapses to `h = 1`.
pub fn CPUMeterCommonUpdateMode(this: &mut Meter, mode: MeterModeId, ncol: i32) {
    this.mode = mode;
    let (_start, count) = AllCPUsMeter_getRange(this);
    if count == 0 {
        this.h = 1;
        return;
    }
    let data = CPUMeterData::of(this);
    for i in 0..count as usize {
        Meter_setMode(&mut data.meters[i], mode);
    }
    let h = data.meters[0].h;
    debug_assert!(h > 0);
    this.h = h * ((count + ncol - 1) / ncol);
}

/// Port of `static void AllCPUsMeter_done(Meter* this)` from `CPUMeter.c:303`.
/// The C deletes each sub-meter and frees the `CPUMeterData`; clearing the
/// owned `meterData` slot drops the `Vec<Meter>` and reclaims all of it.
pub fn AllCPUsMeter_done(this: &mut Meter) {
    this.meterData = None;
}

/// Port of `static void SingleColCPUsMeter_updateMode` (`CPUMeter.c:314`) —
/// `CPUMeterCommonUpdateMode(this, mode, 1)`.
pub fn SingleColCPUsMeter_updateMode(this: &mut Meter, mode: MeterModeId) {
    CPUMeterCommonUpdateMode(this, mode, 1);
}

/// Port of `static void DualColCPUsMeter_updateMode` (`CPUMeter.c:318`) —
/// `CPUMeterCommonUpdateMode(this, mode, 2)`.
pub fn DualColCPUsMeter_updateMode(this: &mut Meter, mode: MeterModeId) {
    CPUMeterCommonUpdateMode(this, mode, 2);
}

/// Port of `static void QuadColCPUsMeter_updateMode` (`CPUMeter.c:322`) —
/// `CPUMeterCommonUpdateMode(this, mode, 4)`.
pub fn QuadColCPUsMeter_updateMode(this: &mut Meter, mode: MeterModeId) {
    CPUMeterCommonUpdateMode(this, mode, 4);
}

/// Port of `static void OctoColCPUsMeter_updateMode` (`CPUMeter.c:326`) —
/// `CPUMeterCommonUpdateMode(this, mode, 8)`.
pub fn OctoColCPUsMeter_updateMode(this: &mut Meter, mode: MeterModeId) {
    CPUMeterCommonUpdateMode(this, mode, 8);
}

/// Port of `static void CPUMeterCommonDraw(Meter* this, int x, int y, int w,
/// int ncol)` from `CPUMeter.c:330`. Tiles the sub-meters into `ncol` columns
/// of `ceil(count / ncol)` rows, dispatching each sub-meter's `draw` slot at
/// its computed cell. The C `d` term distributes the `w % ncol` remainder as a
/// one-column spacer across the first `diff` columns. Terminal output goes
/// through `out` (the crossterm sink the ported `Meter` draw path uses).
pub fn CPUMeterCommonDraw(
    out: &mut dyn Write,
    this: &mut Meter,
    x: i32,
    y: i32,
    w: i32,
    ncol: i32,
) {
    let (_start, count) = AllCPUsMeter_getRange(this);
    let colwidth = w / ncol;
    let diff = w % ncol;
    let nrows = (count + ncol - 1) / ncol;
    let data = CPUMeterData::of(this);
    let h0 = if count > 0 { data.meters[0].h } else { 0 };
    for i in 0..count {
        let col = i / nrows;
        let d = if col > diff { diff } else { col }; // dynamic spacer
        let xpos = x + col * colwidth + d;
        let ypos = y + (i % nrows) * h0;
        let m = &mut data.meters[i as usize];
        let draw = m.draw.expect("CPUMeterCommonDraw: sub-meter draw");
        draw(&mut *out, m, xpos, ypos, colwidth);
    }
}

/// Port of `static void DualColCPUsMeter_draw` (`CPUMeter.c:346`) —
/// `CPUMeterCommonDraw(this, x, y, w, 2)`.
pub fn DualColCPUsMeter_draw(out: &mut dyn Write, this: &mut Meter, x: i32, y: i32, w: i32) {
    CPUMeterCommonDraw(out, this, x, y, w, 2);
}

/// Port of `static void QuadColCPUsMeter_draw` (`CPUMeter.c:350`) —
/// `CPUMeterCommonDraw(this, x, y, w, 4)`.
pub fn QuadColCPUsMeter_draw(out: &mut dyn Write, this: &mut Meter, x: i32, y: i32, w: i32) {
    CPUMeterCommonDraw(out, this, x, y, w, 4);
}

/// Port of `static void OctoColCPUsMeter_draw` (`CPUMeter.c:354`) —
/// `CPUMeterCommonDraw(this, x, y, w, 8)`.
pub fn OctoColCPUsMeter_draw(out: &mut dyn Write, this: &mut Meter, x: i32, y: i32, w: i32) {
    CPUMeterCommonDraw(out, this, x, y, w, 8);
}

/// Port of `static void SingleColCPUsMeter_draw(Meter* this, int x, int y,
/// int w)` from `CPUMeter.c:359`. Stacks the sub-meters vertically in one
/// column, advancing `y` by each sub-meter's height.
pub fn SingleColCPUsMeter_draw(out: &mut dyn Write, this: &mut Meter, x: i32, mut y: i32, w: i32) {
    let (_start, count) = AllCPUsMeter_getRange(this);
    let data = CPUMeterData::of(this);
    for i in 0..count as usize {
        let m = &mut data.meters[i];
        let draw = m.draw.expect("SingleColCPUsMeter_draw: sub-meter draw");
        draw(&mut *out, m, x, y, w);
        y += m.h;
    }
}

/// Emits one `AllCPUs*`/`Left*`/`Right*` `MeterClass` static from `CPUMeter.c`.
/// The 12 multi-column CPU classes share every slot except the column-layout
/// `draw`/`updateMode` pair, `isMultiColumn`, and the `name`/`uiName`/
/// `description` strings — so they are a data table, one macro row per C
/// `const MeterClass` initializer (the anti-duplication rule's "shared
/// constructor before N instances"). All share `CPUMeter_display`,
/// `AllCPUsMeter_updateValues`, `CPUMeterCommonInit`, `AllCPUsMeter_done`,
/// `BAR_METERMODE`, the `CPUMeter_attributes` palette, and `caption = "CPU"`.
macro_rules! all_cpus_meter_class {
    ($id:ident, $draw:path, $mode:path, $multicol:expr, $name:literal, $ui:literal, $desc:literal) => {
        /// Port of the correspondingly-named `const MeterClass` from
        /// `CPUMeter.c` (an `AllCPUsMeter`-family multi-column CPU meter).
        pub static $id: MeterClass = MeterClass {
            super_: ObjectClass {
                extends: Some(&Meter_class.super_),
            },
            display: Some(CPUMeter_display),
            init: Some(CPUMeterCommonInit),
            done: Some(AllCPUsMeter_done),
            updateMode: Some($mode),
            updateValues: Some(AllCPUsMeter_updateValues),
            draw: Some($draw),
            getCaption: None,
            getUiName: None,
            defaultMode: BAR_METERMODE,
            supportedModes: METERMODE_DEFAULT_SUPPORTED,
            total: 100.0,
            attributes: &CPUMETER_ATTRIBUTES,
            name: $name,
            uiName: $ui,
            caption: "CPU",
            description: Some($desc),
            maxItems: 0,
            isMultiColumn: $multicol,
            isPercentChart: false,
        };
    };
}

// The 12 multi-column CPU classes, in `CPUMeter.c` declaration order. Only
// `AllCPUs` (all CPUs in one column) is not `isMultiColumn`.
all_cpus_meter_class!(
    AllCPUsMeter_class,
    SingleColCPUsMeter_draw,
    SingleColCPUsMeter_updateMode,
    false,
    "AllCPUs",
    "CPUs (1/1)",
    "CPUs (1/1): all CPUs"
);
all_cpus_meter_class!(
    AllCPUs2Meter_class,
    DualColCPUsMeter_draw,
    DualColCPUsMeter_updateMode,
    true,
    "AllCPUs2",
    "CPUs (1&2/2)",
    "CPUs (1&2/2): all CPUs in 2 shorter columns"
);
all_cpus_meter_class!(
    LeftCPUsMeter_class,
    SingleColCPUsMeter_draw,
    SingleColCPUsMeter_updateMode,
    true,
    "LeftCPUs",
    "CPUs (1/2)",
    "CPUs (1/2): first half of list"
);
all_cpus_meter_class!(
    RightCPUsMeter_class,
    SingleColCPUsMeter_draw,
    SingleColCPUsMeter_updateMode,
    true,
    "RightCPUs",
    "CPUs (2/2)",
    "CPUs (2/2): second half of list"
);
all_cpus_meter_class!(
    LeftCPUs2Meter_class,
    DualColCPUsMeter_draw,
    DualColCPUsMeter_updateMode,
    true,
    "LeftCPUs2",
    "CPUs (1&2/4)",
    "CPUs (1&2/4): first half in 2 shorter columns"
);
all_cpus_meter_class!(
    RightCPUs2Meter_class,
    DualColCPUsMeter_draw,
    DualColCPUsMeter_updateMode,
    true,
    "RightCPUs2",
    "CPUs (3&4/4)",
    "CPUs (3&4/4): second half in 2 shorter columns"
);
all_cpus_meter_class!(
    AllCPUs4Meter_class,
    QuadColCPUsMeter_draw,
    QuadColCPUsMeter_updateMode,
    true,
    "AllCPUs4",
    "CPUs (1&2&3&4/4)",
    "CPUs (1&2&3&4/4): all CPUs in 4 shorter columns"
);
all_cpus_meter_class!(
    LeftCPUs4Meter_class,
    QuadColCPUsMeter_draw,
    QuadColCPUsMeter_updateMode,
    true,
    "LeftCPUs4",
    "CPUs (1-4/8)",
    "CPUs (1-4/8): first half in 4 shorter columns"
);
all_cpus_meter_class!(
    RightCPUs4Meter_class,
    QuadColCPUsMeter_draw,
    QuadColCPUsMeter_updateMode,
    true,
    "RightCPUs4",
    "CPUs (5-8/8)",
    "CPUs (5-8/8): second half in 4 shorter columns"
);
all_cpus_meter_class!(
    AllCPUs8Meter_class,
    OctoColCPUsMeter_draw,
    OctoColCPUsMeter_updateMode,
    true,
    "AllCPUs8",
    "CPUs (1-8/8)",
    "CPUs (1-8/8): all CPUs in 8 shorter columns"
);
all_cpus_meter_class!(
    LeftCPUs8Meter_class,
    OctoColCPUsMeter_draw,
    OctoColCPUsMeter_updateMode,
    true,
    "LeftCPUs8",
    "CPUs (1-8/16)",
    "CPUs (1-8/16): first half in 8 shorter columns"
);
all_cpus_meter_class!(
    RightCPUs8Meter_class,
    OctoColCPUsMeter_draw,
    OctoColCPUsMeter_updateMode,
    true,
    "RightCPUs8",
    "CPUs (9-16/16)",
    "CPUs (9-16/16): second half in 8 shorter columns"
);

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ported::machine::Machine as CanonMachine;
    use crate::ported::settings::Settings as CanonSettings;

    /// A leaked canonical `Machine` (`'static`, address-stable for the
    /// `Meter`'s `*const Machine` host).
    fn host(existingCPUs: u32, countCPUsFromOne: bool) -> *const CanonMachine {
        let m: &'static CanonMachine = Box::leak(Box::new(CanonMachine {
            existingCPUs,
            settings: Some(CanonSettings {
                countCPUsFromOne,
                ..Default::default()
            }),
            ..Default::default()
        }));
        m as *const CanonMachine
    }

    fn meter(name: &'static str, existingCPUs: u32) -> Meter {
        Meter {
            name,
            uiName: "CPU",
            param: 0,
            host: host(existingCPUs, false),
            ..Meter::empty()
        }
    }

    /// Builds a `Meter` exercising [`CPUMeter_getUiName`]: sets the
    /// `uiName`, the tracked `param`, and the `countCPUsFromOne` flag.
    fn ui_meter(uiName: &'static str, param: u32, countCPUsFromOne: bool) -> Meter {
        Meter {
            name: "CPU",
            uiName,
            param,
            host: host(0, countCPUsFromOne),
            ..Meter::empty()
        }
    }

    /// Host for [`CPUMeter_init`]: also carries `activeCPUs` and the
    /// `showCPUSMTLabels` flag that the caption logic reads.
    fn init_host(activeCPUs: u32, countCPUsFromOne: bool, smt: bool) -> *const CanonMachine {
        let m: &'static CanonMachine = Box::leak(Box::new(CanonMachine {
            activeCPUs,
            existingCPUs: activeCPUs,
            settings: Some(CanonSettings {
                countCPUsFromOne,
                showCPUSMTLabels: smt,
                ..Default::default()
            }),
            ..Default::default()
        }));
        m as *const CanonMachine
    }

    fn init_meter(param: u32, host: *const CanonMachine) -> Meter {
        Meter {
            name: "CPU",
            uiName: "CPU",
            param,
            host,
            ..Meter::empty()
        }
    }

    #[test]
    fn init_average_meter_is_avg() {
        let mut m = init_meter(0, init_host(4, false, false));
        CPUMeter_init(&mut m);
        assert_eq!(m.caption, "Avg");
    }

    #[test]
    fn init_per_cpu_caption_is_zero_based_id() {
        // param=1 -> id = param-1 = 0, countCPUsFromOne=false -> "%3u" of 0.
        let mut m = init_meter(1, init_host(4, false, false));
        CPUMeter_init(&mut m);
        assert_eq!(m.caption, "  0");
    }

    #[test]
    fn init_per_cpu_caption_counts_from_one() {
        // param=2 -> id=1, countCPUsFromOne=true -> Settings_cpuId = id+1 = 2.
        let mut m = init_meter(2, init_host(4, true, false));
        CPUMeter_init(&mut m);
        assert_eq!(m.caption, "  2");
    }

    #[test]
    fn init_single_cpu_host_leaves_caption_default() {
        // activeCPUs <= 1 and param != 0: the C `else if` has no else, so the
        // caption is untouched (empty from Meter::empty()).
        let mut m = init_meter(1, init_host(1, false, false));
        CPUMeter_init(&mut m);
        assert_eq!(m.caption, "");
    }

    // ---- multi-column CPU meters (CPUMeterData substrate) ------------

    fn cpu_meter_data(m: &Meter) -> &CPUMeterData {
        m.meterData
            .as_ref()
            .unwrap()
            .downcast_ref::<CPUMeterData>()
            .unwrap()
    }

    #[test]
    fn allcpus_new_builds_one_submeter_per_cpu() {
        // Meter_new runs CPUMeterCommonInit (init slot): AllCPUs covers the
        // whole range [0, existingCPUs), so 4 sub-meters with params 1..=4
        // (start + i + 1).
        let m = Meter_new(init_host(4, false, false), 0, &AllCPUsMeter_class);
        let data = cpu_meter_data(&m);
        assert_eq!(data.cpus, 4);
        assert_eq!(data.meters.len(), 4);
        assert_eq!(
            data.meters.iter().map(|s| s.param).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        // Meter_new then applied the default mode via the updateMode slot.
        assert_eq!(m.mode, BAR_METERMODE);
    }

    #[test]
    fn left_right_split_range_across_submeters() {
        // 4 CPUs: LeftCPUs -> [0,2), RightCPUs -> [2,4). Params are
        // start + i + 1.
        let left = Meter_new(init_host(4, false, false), 0, &LeftCPUsMeter_class);
        let right = Meter_new(init_host(4, false, false), 0, &RightCPUsMeter_class);
        assert_eq!(
            cpu_meter_data(&left)
                .meters
                .iter()
                .map(|s| s.param)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            cpu_meter_data(&right)
                .meters
                .iter()
                .map(|s| s.param)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
    }

    #[test]
    fn dualcol_updatemode_stacks_height_over_two_columns() {
        // 4 CPUs in 2 columns -> ceil(4/2) = 2 rows -> h = subH * 2.
        let mut m = Meter_new(init_host(4, false, false), 0, &AllCPUs2Meter_class);
        let sub_h = cpu_meter_data(&m).meters[0].h;
        DualColCPUsMeter_updateMode(&mut m, BAR_METERMODE);
        assert!(sub_h > 0);
        assert_eq!(m.h, sub_h * 2);
    }

    #[test]
    fn done_releases_submeters() {
        let mut m = Meter_new(init_host(4, false, false), 0, &AllCPUsMeter_class);
        assert!(m.meterData.is_some());
        AllCPUsMeter_done(&mut m);
        assert!(m.meterData.is_none());
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

// Linux-flavored updateValues/display tests (LinuxMachine host + per-CPU
// `cpuData` for the linux setter). On macOS the darwin setter reads live mach
// CPU-load deltas instead, covered by `cpu_data_darwin_tests` below.
#[cfg(test)]
#[cfg(not(target_os = "macos"))]
mod cpu_data_tests {
    use crate::ported::linux::linuxmachine::{CPUData, LinuxMachine};
    use crate::ported::machine::{Machine, Settings};
    use crate::ported::meter::Meter;

    fn hosted(cpu: CPUData, settings: Settings, existing: u32) -> Meter {
        let host = Box::leak(Box::new(LinuxMachine {
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
            host: &host.super_ as *const crate::ported::machine::Machine,
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
        let cpu = CPUData {
            online: false,
            ..Default::default()
        };
        let mut m = hosted(
            cpu,
            Settings {
                showCPUUsage: true,
                ..Default::default()
            },
            8,
        );
        super::CPUMeter_updateValues(&mut m);
        assert_eq!(m.txtBuffer, "offline");
        assert_eq!(m.curItems, 0);
    }

    #[test]
    fn absent_cpu_renders_absent() {
        // param (0) > existingCPUs would need existing < 0; instead set the
        // meter param above existing via a fresh meter.
        let cpu = CPUData {
            online: true,
            totalPeriod: 100,
            ..Default::default()
        };
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
            userPeriod: 50,      // normal 50.0
            nicePeriod: 10,      // nice 10.0
            systemAllPeriod: 20, // kernel 20.0 (summary)
            ..Default::default()
        };
        let settings = Settings {
            detailedCPUTime: false,
            ..Default::default()
        };
        let mut m = hosted(cpu, settings, 8);
        super::CPUMeter_updateValues(&mut m);
        let mut out = crate::ported::richstring::RichString::new();
        super::CPUMeter_display(&m, &mut out);
        // IRQ = steal+guest = 0 (nonnegative) → "vir:" 0.0 shown.
        assert_eq!(text(&out), ": 50.0% sys: 20.0% low: 10.0% vir:  0.0% ");
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod cpu_data_darwin_tests {
    use crate::ported::darwin::darwinmachine::{DarwinMachine_freeCPULoadInfo, Machine_new};
    use crate::ported::machine::{ScreenSettings, Settings};
    use crate::ported::meter::Meter;

    /// macOS: `CPUMeter_updateValues` drives the darwin setter (mach CPU-load
    /// deltas from a real `DarwinMachine`) and forms a live usage percentage.
    #[test]
    fn update_values_reads_live_cpu_load() {
        let mut dm = Machine_new(None, 0);
        dm.super_.settings = Some(Settings {
            showCPUUsage: true,
            detailedCPUTime: false,
            screens: vec![ScreenSettings::default()],
            ..Default::default()
        });

        let mut m = Meter {
            values: vec![0.0; 10],
            param: 1, // a single physical CPU (#1)
            host: &dm.super_ as *const crate::ported::machine::Machine,
            ..Meter::empty()
        };
        super::CPUMeter_updateValues(&mut m);

        // Not offline/absent — a valid "N.N%" usage string, 3 darwin classes.
        assert!(m.txtBuffer.ends_with('%'), "got {:?}", m.txtBuffer);
        assert_eq!(m.curItems, 3);

        DarwinMachine_freeCPULoadInfo(&mut dm.prev_load);
        DarwinMachine_freeCPULoadInfo(&mut dm.curr_load);
    }
}
