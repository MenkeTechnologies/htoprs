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
//! Ported (self-contained: `RichString` + `CRT_colors` are ported):
//! - [`PressureStallMeter_display`] (`PressureStallMeter.c:57`) — appends
//!   the 10s/60s/300s figures, each colored `PRESSURE_STALL_TEN` /
//!   `PRESSURE_STALL_SIXTY` / `PRESSURE_STALL_THREEHUNDRED`.
//!
//! Stubbed (blocked on unported substrate — keeps its `todo!()`):
//! - `PressureStallMeter_updateValues` (`PressureStallMeter.c:30`) — the
//!   C body selects the `/proc/pressure` file and the some/full flavor via
//!   `Meter_name(this)` (`As_Meter(this)->name`, the concrete meter class's
//!   internal name), then fills `this->values[0..2]` and formats
//!   `this->txtBuffer`. The ported [`Meter`] carries no `name` field and no
//!   per-instance concrete `MeterClass` (no concrete PSI meter type is
//!   migrated), so `Meter_name(this)` has no faithful source — there is no
//!   way to know which of the six `PressureStall*Meter_class`es a given
//!   `Meter` instance is. The value reader `Platform_getPressureStall` is
//!   ported and would feed the values, but the `file`/`some` selection it
//!   needs cannot be reproduced without the class name.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::meter::Meter;
use crate::ported::richstring::{RichString, RichString_appendnAscii};

/// TODO: port of `static void PressureStallMeter_updateValues(Meter* this)`
/// from `PressureStallMeter.c:30`. Blocked: the C body dispatches on
/// `Meter_name(this)` (`As_Meter(this)->name`, the concrete meter class's
/// internal name — "PressureStallCPUSome", "PressureStallIOFull", …) to pick
/// the `/proc/pressure` file (`cpu`/`io`/`irq`/`memory`) and the some/full
/// flavor, then calls `Platform_getPressureStall(file, some, &this->values[0],
/// …)` and formats `this->txtBuffer`. The ported [`Meter`] has no `name` field
/// and holds no per-instance concrete `MeterClass` (no concrete PSI meter type
/// is migrated), so `Meter_name(this)` has no faithful data source; the
/// `file`/`some` selection cannot be reproduced. (`Platform_getPressureStall`
/// itself is ported.)
pub fn PressureStallMeter_updateValues() {
    todo!("port of PressureStallMeter.c:30: needs Meter_name (no `name`/concrete MeterClass on ported Meter)")
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
            host: None,
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
