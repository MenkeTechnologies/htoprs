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
//! Stubbed (blocked on unported substrate — each keeps its `todo!()`):
//! - `LoadAverageMeter_updateValues` (`LoadAverageMeter.c:42`) /
//!   `LoadMeter_updateValues` (`LoadAverageMeter.c:76`) — the values are
//!   sourced by `Platform_getLoadAverage(&this->values[0], ...)`, the
//!   platform-specific load reader in `Platform.c` (no platform layer
//!   ported); there is no data source to reproduce faithfully. The bodies
//!   also touch `this->total`, `this->host->activeCPUs`,
//!   `this->curAttributes` (assigned the file-scope `OK_/Medium_/High_`
//!   attribute arrays), and `this->txtBuffer` — none of which the partial
//!   `Meter` in `meter.rs` models. Faking the load source as a struct read
//!   would be an adhoc reimplementation, not a faithful port.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::meter::Meter;
use crate::ported::richstring::{RichString, RichString_appendnAscii};

/// TODO: port of `static void LoadAverageMeter_updateValues(Meter* this)`
/// from `LoadAverageMeter.c:42`. Blocked: the 1/5/15-minute values come
/// from `Platform_getLoadAverage(&this->values[0], &this->values[1],
/// &this->values[2])`, the platform-specific load reader in `Platform.c`,
/// which is not ported — there is no faithful data source to feed. The
/// body further reads/writes `this->total` and `this->host->activeCPUs`,
/// assigns `this->curAttributes` from the file-scope `OK_/Medium_/High_`
/// attribute arrays, and formats `this->txtBuffer`, none of which the
/// partial `Meter` in `meter.rs` models.
pub fn LoadAverageMeter_updateValues() {
    todo!("port of LoadAverageMeter.c:42")
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
/// [`LoadAverageMeter_updateValues`]: the 1-minute value comes from
/// `Platform_getLoadAverage(&this->values[0], &five, &fifteen)` (unported
/// `Platform.c` reader), and the body also touches `this->total`,
/// `this->host->activeCPUs`, `this->curAttributes`, and `this->txtBuffer`,
/// none of which the partial `Meter` in `meter.rs` models.
pub fn LoadMeter_updateValues() {
    todo!("port of LoadAverageMeter.c:76")
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
    /// display functions read); the remaining fields are inert.
    fn meter(values: Vec<f64>) -> Meter {
        Meter {
            values,
            curItems: 0,
            mode: BAR_METERMODE,
            supportedModes: 0,
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
}
