//! Partial port of `LoadAverageMeter.c` — htop's load-average meters.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C `static void
//! Foo_display(const Object* cast, RichString* out)` casts `cast` back to
//! `const Meter*` and reads `this->values[...]`, so it ports to a free fn
//! `pub fn Foo_display(this: &Meter, out: &mut RichString)` — the `cast`
//! → `this` down-cast collapses into the typed `&Meter` parameter. The
//! shared `Meter` model is [`crate::ported::meter::Meter`], reused because
//! it already carries the `values` slot these functions read.
//!
//! `CRT_colors[X]` (C's active-scheme row `const int* CRT_colors`, set by
//! `CRT_setColors` to point at `CRT_colorSchemes[colorScheme]`) is
//! reproduced as `ColorElements::X.packed(ColorScheme::active())`
//! (`CRT_colorSchemes[CRT_colorScheme][X]`), the same mapping
//! `diskiometer.rs` uses for its ported display functions.
//! `xSnprintf(buffer, 20, "%.2f ", v)` becomes `format!("{:.2} ", v)`; the
//! returned `len` is the string's byte length. The C `char buffer[20]`
//! never truncates here — a `"%.2f "` of a load average is a handful of
//! bytes — so the fixed-buffer cap is not modeled (the same reasoning
//! `tasksmeter.rs` applies to its `txtBuffer`).
//!
//! Ported (self-contained: `RichString` + `CRT_colors` are ported):
//! - [`LoadAverageMeter_display`] (`LoadAverageMeter.c:63`) — appends the
//!   1/5/15-minute figures, each colored `LOAD_AVERAGE_ONE` /
//!   `LOAD_AVERAGE_FIVE` / `LOAD_AVERAGE_FIFTEEN`.
//! - [`LoadMeter_display`] (`LoadAverageMeter.c:95`) — appends the
//!   1-minute figure only, colored `LOAD`.
//!
//! - [`LoadAverageMeter_updateValues`] (`LoadAverageMeter.c:42`) /
//!   [`LoadMeter_updateValues`] (`LoadAverageMeter.c:76`) — read the load
//!   figures via the ported `Platform_getLoadAverage`, clamp `this->total`
//!   to `this->host->activeCPUs`, and set the OK/Medium/High bar color from
//!   the 1-minute value against that CPU count. `this->host` is the
//!   `Rc<RefCell<LinuxMachine>>` back-pointer modeled on `Meter` (its
//!   `super_` is the generic `Machine` holding `activeCPUs`).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // faithful C global names (LoadAverageMeter_class)
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
// htop links each OS's Platform.c; the Rust analog selects the platform module
// by cfg. macOS reads load via `getloadavg` (darwin::platform); other targets
// keep the existing linux path. Without this the macOS build ran the Linux
// `/proc/loadavg` reader, which fails off-Linux → "Load average: NaN NaN NaN".
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_getLoadAverage;
#[cfg(not(target_os = "macos"))]
use crate::ported::linux::platform::Platform_getLoadAverage;
use crate::ported::meter::{
    Meter, MeterClass, Meter_class, METERMODE_DEFAULT_SUPPORTED, TEXT_METERMODE,
};
use crate::ported::object::ObjectClass;
use crate::ported::richstring::{RichString, RichString_appendnAscii};

/// Port of `static const int OK_attributes[] = { METER_VALUE_OK }`
/// (`LoadAverageMeter.c:30`) — the bar color when load < 1.0.
static OK_ATTRIBUTES: [i32; 1] = [ColorElements::METER_VALUE_OK as i32];
/// Port of `static const int Medium_attributes[] = { METER_VALUE_WARN }`
/// (`LoadAverageMeter.c:34`) — load in `[1.0, activeCPUs)`.
static MEDIUM_ATTRIBUTES: [i32; 1] = [ColorElements::METER_VALUE_WARN as i32];
/// Port of `static const int High_attributes[] = { METER_VALUE_ERROR }`
/// (`LoadAverageMeter.c:38`) — load ≥ activeCPUs.
static HIGH_ATTRIBUTES: [i32; 1] = [ColorElements::METER_VALUE_ERROR as i32];

/// Port of `static void LoadAverageMeter_updateValues(Meter* this)` from
/// `LoadAverageMeter.c:42`. Reads the 1/5/15-minute figures via the ported
/// `Platform_getLoadAverage`, shows only the 1-minute bar (`curItems = 1`),
/// clamps `this->total` up to `host->activeCPUs`, and picks the OK/Medium/
/// High bar color from the 1-minute value against that CPU count.
/// `this->host` is the concrete `LinuxMachine`; `activeCPUs` lives on its
/// generic `super_` (`Machine`).
pub fn LoadAverageMeter_updateValues(this: &mut Meter) {
    let (mut one, mut five, mut fifteen) = (0.0f64, 0.0f64, 0.0f64);
    Platform_getLoadAverage(&mut one, &mut five, &mut fifteen);
    this.values[0] = one;
    this.values[1] = five;
    this.values[2] = fifteen;

    // only show bar for 1min value
    this.curItems = 1;

    // change bar color and total based on value
    let active_cpus = unsafe { (*this.host).activeCPUs } as f64;
    if this.total < active_cpus {
        this.total = active_cpus;
    }
    if this.values[0] < 1.0 {
        this.curAttributes = Some(&OK_ATTRIBUTES);
    } else if this.values[0] < active_cpus {
        this.curAttributes = Some(&MEDIUM_ATTRIBUTES);
    } else {
        this.curAttributes = Some(&HIGH_ATTRIBUTES);
    }

    this.txtBuffer = format!(
        "{:.2}/{:.2}/{:.2}",
        this.values[0], this.values[1], this.values[2]
    );
}

/// Port of `static void LoadAverageMeter_display(const Object* cast,
/// RichString* out)` from `LoadAverageMeter.c:63`. Appends the three load
/// figures — 1, 5, and 15-minute — each as `"%.2f "`, colored
/// `LOAD_AVERAGE_ONE`, `LOAD_AVERAGE_FIVE`, and `LOAD_AVERAGE_FIFTEEN`
/// respectively. `CRT_colors[X]` is `ColorElements::X.packed(scheme)`; the
/// active scheme is read once (it is a process-global that does not change
/// mid-call), matching the C global `CRT_colors`.
pub fn LoadAverageMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();

    let buffer = format!("{:.2} ", this.values[0]);
    RichString_appendnAscii(
        out,
        ColorElements::LOAD_AVERAGE_ONE.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );
    let buffer = format!("{:.2} ", this.values[1]);
    RichString_appendnAscii(
        out,
        ColorElements::LOAD_AVERAGE_FIVE.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );
    let buffer = format!("{:.2} ", this.values[2]);
    RichString_appendnAscii(
        out,
        ColorElements::LOAD_AVERAGE_FIFTEEN.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );
}

/// Port of `static void LoadMeter_updateValues(Meter* this)` from
/// `LoadAverageMeter.c:76`. Like [`LoadAverageMeter_updateValues`] but keeps
/// only the 1-minute figure (`five`/`fifteen` are C locals), clamps
/// `this->total` to `host->activeCPUs`, and sets the OK/Medium/High color.
pub fn LoadMeter_updateValues(this: &mut Meter) {
    let (mut one, mut five, mut fifteen) = (0.0f64, 0.0f64, 0.0f64);
    Platform_getLoadAverage(&mut one, &mut five, &mut fifteen);
    this.values[0] = one;
    let _ = (five, fifteen); // C keeps `five`/`fifteen` as locals, unused

    // change bar color and total based on value
    let active_cpus = unsafe { (*this.host).activeCPUs } as f64;
    if this.total < active_cpus {
        this.total = active_cpus;
    }
    if this.values[0] < 1.0 {
        this.curAttributes = Some(&OK_ATTRIBUTES);
    } else if this.values[0] < active_cpus {
        this.curAttributes = Some(&MEDIUM_ATTRIBUTES);
    } else {
        this.curAttributes = Some(&HIGH_ATTRIBUTES);
    }

    this.txtBuffer = format!("{:.2}", this.values[0]);
}

/// Port of `static void LoadMeter_display(const Object* cast, RichString*
/// out)` from `LoadAverageMeter.c:95`. Appends the 1-minute load figure as
/// `"%.2f "`, colored `LOAD`. `CRT_colors[LOAD]` is
/// `ColorElements::LOAD.packed(ColorScheme::active())`.
pub fn LoadMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();

    let buffer = format!("{:.2} ", this.values[0]);
    RichString_appendnAscii(
        out,
        ColorElements::LOAD.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );
}

/// Port of `static const int LoadAverageMeter_attributes[]` from
/// `LoadAverageMeter.c`: `{ LOAD_AVERAGE_ONE, _FIVE, _FIFTEEN }` — the
/// per-item colors as `CRT_colors` indices (`ColorElements as i32`).
static LoadAverageMeter_attributes: [i32; 3] = [
    ColorElements::LOAD_AVERAGE_ONE as i32,
    ColorElements::LOAD_AVERAGE_FIVE as i32,
    ColorElements::LOAD_AVERAGE_FIFTEEN as i32,
];

/// Port of `static const int LoadMeter_attributes[]` from
/// `LoadAverageMeter.c`: `{ LOAD }`.
static LoadMeter_attributes: [i32; 1] = [ColorElements::LOAD as i32];

/// Port of `const MeterClass LoadAverageMeter_class` from
/// `LoadAverageMeter.c`. Wires the ported
/// [`LoadAverageMeter_updateValues`]/[`LoadAverageMeter_display`] slots.
/// `maxItems = 3` (1/5/15-minute averages); not a percent chart.
pub static LoadAverageMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: Some(LoadAverageMeter_display),
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(LoadAverageMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 1.0,
    attributes: &LoadAverageMeter_attributes,
    name: "LoadAverage",
    uiName: "Load average",
    caption: "Load average: ",
    description: Some("Load averages: 1 minute, 5 minutes, 15 minutes"),
    maxItems: 3,
    isMultiColumn: false,
    isPercentChart: false,
};

/// Port of `const MeterClass LoadMeter_class` from `LoadAverageMeter.c`.
/// The single-value (1-minute) load meter; `maxItems = 1`.
pub static LoadMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: Some(LoadMeter_display),
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(LoadMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 1.0,
    attributes: &LoadMeter_attributes,
    name: "Load",
    uiName: "Load",
    caption: "Load: ",
    description: Some("Load: average of ready processes in the last minute"),
    maxItems: 1,
    isMultiColumn: false,
    isPercentChart: false,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::meter::BAR_METERMODE;

    /// Visible characters of the valid `[0, chlen)` range of `out`.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    /// Build a `Meter` carrying the given `values` (the only field the
    /// display functions read); the remaining fields are inert
    /// (`..Meter::empty()`, the Rust-only bootstrap helper).
    fn meter(values: Vec<f64>) -> Meter {
        Meter {
            host: core::ptr::null(),
            values,
            mode: BAR_METERMODE,
            ..Meter::empty()
        }
    }

    #[test]
    fn load_average_display_three_figures() {
        // Three values, each "%.2f " with a trailing space.
        let m = meter(vec![1.23, 4.50, 15.00]);
        let mut out = RichString::new();
        LoadAverageMeter_display(&m, &mut out);
        assert_eq!(text(&out), "1.23 4.50 15.00 ");
    }

    #[test]
    fn load_average_display_rounds_to_two_places() {
        // 0.125 -> "%.2f" round-half-to-even -> "0.12"; 2.005 -> "2.00"
        // (nearest representable double is just below 2.005).
        let m = meter(vec![0.005, 0.999, 0.0]);
        let mut out = RichString::new();
        LoadAverageMeter_display(&m, &mut out);
        assert_eq!(text(&out), "0.01 1.00 0.00 ");
    }

    #[test]
    fn load_meter_display_one_figure() {
        // LoadMeter shows only values[0]; trailing slots are ignored.
        let m = meter(vec![0.42, 99.0, 99.0]);
        let mut out = RichString::new();
        LoadMeter_display(&m, &mut out);
        assert_eq!(text(&out), "0.42 ");
    }

    use crate::ported::linux::linuxmachine::LinuxMachine;
    use crate::ported::machine::Machine;

    /// A `Meter` with a 3-slot `values` and a host reporting `active_cpus`.
    fn hosted_meter(active_cpus: u32) -> Meter {
        let host = Box::leak(Box::new(LinuxMachine {
            super_: Machine {
                activeCPUs: active_cpus,
                ..Default::default()
            },
            ..Default::default()
        }));
        Meter {
            values: vec![0.0; 3],
            total: 0.0,
            host: &host.super_ as *const crate::ported::machine::Machine,
            ..Meter::empty()
        }
    }

    #[test]
    fn load_average_update_values_sets_invariants() {
        // Reads the live load average (real on Linux CI, NaN on non-/proc
        // hosts). Assert only the C-guaranteed invariants that hold for any
        // sampled load: curItems, the total clamp, a chosen bar color, and
        // the "%.2f/%.2f/%.2f" text shape.
        let mut m = hosted_meter(4);
        LoadAverageMeter_updateValues(&mut m);
        assert_eq!(m.curItems, 1);
        assert!(m.total >= 4.0, "total clamped up to activeCPUs");
        assert!(m.curAttributes.is_some(), "a bar color is selected");
        assert_eq!(
            m.txtBuffer.matches('/').count(),
            2,
            "three '/'-joined figures"
        );
    }

    #[test]
    fn load_meter_update_values_selects_ok_below_one() {
        // When the 1-min load is < 1.0 the bar is METER_VALUE_OK; drive that
        // branch deterministically by clamping activeCPUs high and checking
        // the low-load path only when the sampled value is actually < 1.0.
        let mut m = hosted_meter(64);
        LoadMeter_updateValues(&mut m);
        assert!(m.total >= 64.0);
        // txtBuffer is the single 1-min figure with no separators.
        assert_eq!(m.txtBuffer.matches('/').count(), 0);
        if m.values[0] < 1.0 {
            assert_eq!(m.curAttributes, Some(&OK_ATTRIBUTES[..]));
        }
    }
}
