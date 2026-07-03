//! Partial port of `linux/PressureStallMeter.c` — htop's Linux PSI
//! (Pressure Stall Information) meters.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. The C `static void
//! PressureStallMeter_display(const Object* cast, RichString* out)` casts
//! `cast` back to `const Meter*` and reads `this->values[...]`, so it ports
//! to a free fn `pub fn PressureStallMeter_display(this: &Meter, out: &mut
//! RichString)` — the `cast` → `this` down-cast collapses into the typed
//! `&Meter` parameter. The shared `Meter` model is
//! [`crate::ported::meter::Meter`], reused because it already carries the
//! `values` slot the display function reads.
//!
//! `CRT_colors[X]` (C's active-scheme row `const int* CRT_colors`, set by
//! `CRT_setColors` to point at `CRT_colorSchemes[colorScheme]`) is
//! reproduced as `ColorElements::X.packed(ColorScheme::active())`
//! (`CRT_colorSchemes[CRT_colorScheme][X]`), the same mapping
//! `loadaveragemeter.rs` uses for its ported display functions.
//! `xSnprintf(buffer, 20, "%5.2lf%% ", v)` becomes `format!("{:5.2}% ", v)`
//! — width 5, precision 2, a literal `%` (C's `%%`), then a trailing space;
//! the returned `len` is the string's byte length. The C `char buffer[20]`
//! does not truncate a PSI percentage (a handful of bytes), so the
//! fixed-buffer cap is not modeled (the reasoning `loadaveragemeter.rs`
//! applies to its buffer).
//!
//! Ported:
//! - [`PressureStallMeter_updateValues`] (`PressureStallMeter.c:30`) —
//!   dispatches on [`Meter::name`] (the mirrored class name) to select the
//!   `/proc/pressure` file and some/full flavor, calls
//!   [`Platform_getPressureStall`] to fill `this->values[0..=2]`, and formats
//!   `this->txtBuffer`.
//! - [`PressureStallMeter_display`] (`PressureStallMeter.c:57`) — appends
//!   the 10s/60s/300s figures, each colored `PRESSURE_STALL_TEN` /
//!   `PRESSURE_STALL_SIXTY` / `PRESSURE_STALL_THREEHUNDRED`.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::linux::platform::Platform_getPressureStall;
use crate::ported::meter::Meter;
use crate::ported::richstring::{RichString, RichString_appendnAscii};

/// Port of `static void PressureStallMeter_updateValues(Meter* this)` from
/// `PressureStallMeter.c:30`. Dispatches on `Meter_name(this)` (the meter
/// class's internal name, mirrored onto the instance as [`Meter::name`]) to
/// pick the `/proc/pressure` file (`cpu`/`io`/`irq`/`memory`) and the
/// some/full flavor, calls [`Platform_getPressureStall`] to fill
/// `this->values[0..=2]`, marks `curItems = 1` (only the 10s figure is a
/// bar — the sum is meaningless), and formats `this->txtBuffer`.
///
/// The C `strstr(Meter_name(this), "CPU")` substring tests become
/// [`str::contains`]. The three `&this->values[i]` out-params are threaded
/// through locals (the ported [`Platform_getPressureStall`] takes `&mut f64`
/// out-params, matching the C `double*`) and written back. The
/// `xSnprintf("%s %s %5.2lf%% %5.2lf%% %5.2lf%%", …)` becomes a `format!` with
/// the same width-5/precision-2 fields and literal `%`.
pub fn PressureStallMeter_updateValues(this: &mut Meter) {
    // const char* file; based on Meter_name(this).
    let file = if this.name.contains("CPU") {
        "cpu"
    } else if this.name.contains("IO") {
        "io"
    } else if this.name.contains("IRQ") {
        "irq"
    } else {
        "memory"
    };

    // bool some = strstr(Meter_name(this), "Some") != NULL;
    let some = this.name.contains("Some");

    // Platform_getPressureStall(file, some, &this->values[0..2]).
    let mut v0 = this.values[0];
    let mut v1 = this.values[1];
    let mut v2 = this.values[2];
    Platform_getPressureStall(file, some, &mut v0, &mut v1, &mut v2);
    this.values[0] = v0;
    this.values[1] = v1;
    this.values[2] = v2;

    // Only print bar for ten (not sixty and three hundred).
    this.curItems = 1;

    this.txtBuffer = format!(
        "{} {} {:5.2}% {:5.2}% {:5.2}%",
        if some { "some" } else { "full" },
        file,
        v0,
        v1,
        v2
    );
}

/// Port of `static void PressureStallMeter_display(const Object* cast,
/// RichString* out)` from `PressureStallMeter.c:57`. Appends the three PSI
/// figures — the 10-, 60-, and 300-second averages — each as `"%5.2lf%% "`,
/// colored `PRESSURE_STALL_TEN`, `PRESSURE_STALL_SIXTY`, and
/// `PRESSURE_STALL_THREEHUNDRED` respectively. `CRT_colors[X]` is
/// `ColorElements::X.packed(scheme)`; the active scheme is read once (a
/// process-global that does not change mid-call), matching the C global
/// `CRT_colors`.
pub fn PressureStallMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();

    let buffer = format!("{:5.2}% ", this.values[0]);
    RichString_appendnAscii(
        out,
        ColorElements::PRESSURE_STALL_TEN.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );
    let buffer = format!("{:5.2}% ", this.values[1]);
    RichString_appendnAscii(
        out,
        ColorElements::PRESSURE_STALL_SIXTY.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );
    let buffer = format!("{:5.2}% ", this.values[2]);
    RichString_appendnAscii(
        out,
        ColorElements::PRESSURE_STALL_THREEHUNDRED.packed(scheme),
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
    /// display function reads); the remaining fields are inert
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
    fn pressure_stall_display_three_figures() {
        // Three values, each "%5.2lf%% ": width 5, 2 decimals, a literal '%'
        // and a trailing space.
        let m = meter(vec![12.34, 5.60, 0.00]);
        let mut out = RichString::new();
        PressureStallMeter_display(&m, &mut out);
        assert_eq!(text(&out), "12.34%  5.60%  0.00% ");
    }

    #[test]
    fn pressure_stall_display_pads_to_width_five() {
        // "%5.2lf" right-aligns in a field of 5, so "1.20" -> " 1.20".
        let m = meter(vec![1.2, 100.0, 0.05]);
        let mut out = RichString::new();
        PressureStallMeter_display(&m, &mut out);
        assert_eq!(text(&out), " 1.20% 100.00%  0.05% ");
    }
}
