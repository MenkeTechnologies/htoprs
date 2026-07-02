//! Partial port of `FileDescriptorMeter.c` — htop's allocated/available
//! file-descriptor meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. The C `static void
//! FileDescriptorMeter_display(const Object* cast, RichString* out)` casts
//! `cast` back to `const Meter*` and reads `this->values[...]`, so it ports
//! to a free fn `pub fn FileDescriptorMeter_display(this: &Meter, out: &mut
//! RichString)` — the `cast` → `this` down-cast collapses into the typed
//! `&Meter` parameter. The shared `Meter` model is
//! [`crate::ported::meter::Meter`], reused because it already carries the
//! `values` slot this function reads.
//!
//! `CRT_colors[X]` (C's active-scheme row `const int* CRT_colors`, set by
//! `CRT_setColors` to point at `CRT_colorSchemes[colorScheme]`) is
//! reproduced as `ColorElements::X.packed(ColorScheme::active())`, the same
//! mapping `diskiometer.rs` / `loadaveragemeter.rs` use. `xSnprintf(buffer,
//! sizeof(buffer), "%.0lf", v)` becomes `format!("{:.0}", v)`; the returned
//! `len` is the string's byte length. The `char buffer[50]` never truncates
//! here — a `"%.0lf"` of a file-descriptor count is a handful of bytes — so
//! the fixed-buffer cap is not modeled.
//!
//! The two file-scope helpers are inlined at their call sites, matching how
//! `row.rs` inlines the same `Macros.h` predicates:
//! - `isNonnegative(x)` (`Macros.h:141`, `isgreaterequal(x, 0.0)`) is
//!   `x >= 0.0` (false for `NaN`).
//! - `FD_EFFECTIVE_UNLIMITED(x)` (`FileDescriptorMeter.c:23`,
//!   `!isgreaterequal((double)(1<<30), (x))`) is `!((1u32 << 30) as f64 >=
//!   x)` — `true` for `NaN` (the `>=` is `false`, then negated), matching
//!   the C `isgreaterequal` NaN semantics.
//!
//! Ported (self-contained: `RichString` + `CRT_colors` are ported):
//! - [`FileDescriptorMeter_display`] (`FileDescriptorMeter.c:80`) — writes
//!   `used: <n> max: <n|unlimited>`, coloring `used:`/`max:` `METER_TEXT`,
//!   the used count `FILE_DESCRIPTOR_USED`, and the max count (or the word
//!   `unlimited`) `FILE_DESCRIPTOR_MAX`; a negative/NaN used count instead
//!   writes a single `METER_TEXT` `unknown` and returns.
//!
//! Stubbed (blocked on unported substrate — keeps its `todo!()`):
//! - `FileDescriptorMeter_updateValues` (`FileDescriptorMeter.c:30`) — the
//!   used/max figures are sourced by `Platform_getFileDescriptors(&this->
//!   values[0], &this->values[1])`, the platform-specific reader in
//!   `Platform.c` (`linux/platform.rs` has it as a `todo!()` stub whose
//!   signature takes no out-params, so there is nothing to feed
//!   `this->values[]`). The bar-scaling arithmetic over `this->total` and
//!   the `this->txtBuffer` formatting are pure, but without the platform
//!   data source there is nothing to compute over — faking the source would
//!   be an adhoc reimplementation, not a faithful port.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::meter::Meter;
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_appendnAscii};

/// TODO: port of `static void FileDescriptorMeter_updateValues(Meter* this)`
/// from `FileDescriptorMeter.c:30`. Blocked: the used/max values come from
/// `Platform_getFileDescriptors(&this->values[0], &this->values[1])`, the
/// platform-specific reader in `Platform.c`, which is only a `todo!()` stub
/// in `linux/platform.rs` (and its stub signature takes no out-params, so it
/// cannot populate `this->values[]`). The subsequent bar-scaling over
/// `this->total` and the `this->txtBuffer` formatting are pure, but there is
/// no faithful data source to drive them.
pub fn FileDescriptorMeter_updateValues() {
    todo!("port of FileDescriptorMeter.c:30: needs Platform_getFileDescriptors")
}

/// Port of `static void FileDescriptorMeter_display(const Object* cast,
/// RichString* out)` from `FileDescriptorMeter.c:80`. If the used count is
/// negative or `NaN` (`!isNonnegative`), writes a single `METER_TEXT`
/// `unknown` and returns. Otherwise appends `used: <n>` (count colored
/// `FILE_DESCRIPTOR_USED`) then ` max: ` and either the word `unlimited`
/// (when `FD_EFFECTIVE_UNLIMITED`) or the max `<n>`, colored
/// `FILE_DESCRIPTOR_MAX`. `CRT_colors[X]` is `ColorElements::X.packed(scheme)`;
/// the active scheme is read once (a process-global that does not change
/// mid-call), matching the C global `CRT_colors`.
pub fn FileDescriptorMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();

    // !isNonnegative(this->values[0]) — negative or NaN.
    if !(this.values[0] >= 0.0) {
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b"unknown");
        return;
    }

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b"used: ");
    let buffer = format!("{:.0}", this.values[0]);
    RichString_appendnAscii(
        out,
        ColorElements::FILE_DESCRIPTOR_USED.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" max: ");
    // FD_EFFECTIVE_UNLIMITED(this->values[1]) — !((1<<30) >= x), true for NaN.
    if !((1u32 << 30) as f64 >= this.values[1]) {
        RichString_appendAscii(
            out,
            ColorElements::FILE_DESCRIPTOR_MAX.packed(scheme),
            b"unlimited",
        );
    } else {
        let buffer = format!("{:.0}", this.values[1]);
        RichString_appendnAscii(
            out,
            ColorElements::FILE_DESCRIPTOR_MAX.packed(scheme),
            buffer.as_bytes(),
            buffer.len(),
        );
    }
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
            values,
            mode: BAR_METERMODE,
            ..Meter::empty()
        }
    }

    #[test]
    fn display_unknown_on_negative_used() {
        let m = meter(vec![-1.0, 1024.0]);
        let mut out = RichString::new();
        FileDescriptorMeter_display(&m, &mut out);
        assert_eq!(text(&out), "unknown");
    }

    #[test]
    fn display_unknown_on_nan_used() {
        let m = meter(vec![f64::NAN, 1024.0]);
        let mut out = RichString::new();
        FileDescriptorMeter_display(&m, &mut out);
        assert_eq!(text(&out), "unknown");
    }

    #[test]
    fn display_used_and_max() {
        let m = meter(vec![512.0, 1024.0]);
        let mut out = RichString::new();
        FileDescriptorMeter_display(&m, &mut out);
        assert_eq!(text(&out), "used: 512 max: 1024");
    }

    #[test]
    fn display_unlimited_max() {
        // max > 1<<30 is effectively unlimited.
        let m = meter(vec![512.0, 2.0 * (1u32 << 30) as f64]);
        let mut out = RichString::new();
        FileDescriptorMeter_display(&m, &mut out);
        assert_eq!(text(&out), "used: 512 max: unlimited");
    }

    #[test]
    fn display_max_exactly_1_30_is_not_unlimited() {
        // FD_EFFECTIVE_UNLIMITED is !(1<<30 >= x); at x == 1<<30 the >= holds
        // so it is NOT unlimited — the boundary is printed as a number.
        let m = meter(vec![0.0, (1u32 << 30) as f64]);
        let mut out = RichString::new();
        FileDescriptorMeter_display(&m, &mut out);
        assert_eq!(text(&out), "used: 0 max: 1073741824");
    }
}
