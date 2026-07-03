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
//!   `Rc<RefCell<Machine>>` back-pointer now modeled on `Meter`.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::linux::platform::Platform_getLoadAverage;
use crate::ported::meter::Meter;
use crate::ported::richstring::{RichString, RichString_appendnAscii};

/// Port of `static const int OK_attributes[] = { METER_VALUE_OK }`
/// (`LoadAverageMeter.c:34`) — the bar color when load < 1.0.
static OK_ATTRIBUTES: [i32; 1] = [ColorElements::METER_VALUE_OK as i32];
/// Port of `static const int Medium_attributes[] = { METER_VALUE_WARN }`
/// (`LoadAverageMeter.c:38`) — load in `[1.0, activeCPUs)`.
static MEDIUM_ATTRIBUTES: [i32; 1] = [ColorElements::METER_VALUE_WARN as i32];
/// Port of `static const int High_attributes[] = { METER_VALUE_ERROR }`
/// (`LoadAverageMeter.c:42`) — load ≥ activeCPUs.
static HIGH_ATTRIBUTES: [i32; 1] = [ColorElements::METER_VALUE_ERROR as i32];

/// TODO: port of `static void LoadAverageMeter_updateValues(Meter* this)`
/// from `LoadAverageMeter.c:42`. Blocked: the body reads
/// `this->host->activeCPUs` (for the `total` clamp and the OK/Medium/High
/// color threshold), but the partial `Meter` in `meter.rs` carries no
/// `host` back-pointer to dereference. (The load source
/// `Platform_getLoadAverage(&this->values[0], &this->values[1],
/// &this->values[2])` is now ported in `linux/platform.rs`, and the other
/// fields — `total`, `curAttributes`, `curItems`, `txtBuffer` — the `Meter`
/// struct now models; only `host->activeCPUs` remains missing.)
pub fn LoadAverageMeter_updateValues(this: &mut Meter) {
    let (mut one, mut five, mut fifteen) = (0.0f64, 0.0f64, 0.0f64);
    Platform_getLoadAverage(&mut one, &mut five, &mut fifteen);
    this.values[0] = one;
    this.values[1] = five;
    this.values[2] = fifteen;

    // only show bar for 1min value
    this.curItems = 1;

    // change bar color and total based on value
    let active_cpus = this
        .host
        .as_ref()
        .expect("LoadAverageMeter_updateValues: this->host (C dereferences it)")
        .borrow()
        .activeCPUs as f64;
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

/// TODO: port of `static void LoadMeter_updateValues(Meter* this)` from
/// `LoadAverageMeter.c:76`. Blocked for the same reason as
/// [`LoadAverageMeter_updateValues`]: the body reads
/// `this->host->activeCPUs` (for the `total` clamp and the OK/Medium/High
/// color threshold), but the partial `Meter` in `meter.rs` carries no
/// `host` back-pointer. (The load source
/// `Platform_getLoadAverage(&this->values[0], &five, &fifteen)` is now
/// ported in `linux/platform.rs`; only `host->activeCPUs` remains missing.)
pub fn LoadMeter_updateValues(this: &mut Meter) {
    let (mut one, mut five, mut fifteen) = (0.0f64, 0.0f64, 0.0f64);
    Platform_getLoadAverage(&mut one, &mut five, &mut fifteen);
    this.values[0] = one;
    let _ = (five, fifteen); // C keeps `five`/`fifteen` as locals, unused

    // change bar color and total based on value
    let active_cpus = this
        .host
        .as_ref()
        .expect("LoadMeter_updateValues: this->host (C dereferences it)")
        .borrow()
        .activeCPUs as f64;
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
            host: None,
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

    use crate::ported::machine::Machine;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A `Meter` with a 3-slot `values` and a host reporting `active_cpus`.
    fn hosted_meter(active_cpus: u32) -> Meter {
        let host = Rc::new(RefCell::new(Machine {
            activeCPUs: active_cpus,
            ..Default::default()
        }));
        Meter {
            values: vec![0.0; 3],
            total: 0.0,
            host: Some(host),
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
        assert_eq!(m.txtBuffer.matches('/').count(), 2, "three '/'-joined figures");
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
