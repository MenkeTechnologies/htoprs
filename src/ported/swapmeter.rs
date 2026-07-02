//! Partial port of `SwapMeter.c` — htop's swap-usage meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. The shared `Meter` model is
//! [`crate::ported::meter::Meter`]; the C `static void
//! SwapMeter_display(const Object* cast, RichString* out)` casts `cast` back
//! to `const Meter*` and reads `this->total` / `this->values[...]`, so it
//! ports to `pub fn SwapMeter_display(this: &Meter, out: &mut RichString)` —
//! the `cast` → `this` down-cast collapses into the typed `&Meter` parameter
//! (the same mapping `hugepagemeter.rs` / `filedescriptormeter.rs` use).
//!
//! The `SwapMeterValues` enum (`SwapMeter.h:11`) indexes `this->values[]`:
//! `SWAP_METER_USED = 0`, `SWAP_METER_CACHE = 1`, `SWAP_METER_FRONTSWAP = 2`.
//!
//! `CRT_colors[X]` (C's active-scheme row, set by `CRT_setColors`) is
//! reproduced as `ColorElements::X.packed(ColorScheme::active())`.
//! `Meter_humanUnit(buffer, v, sizeof(buffer))` becomes the owned-`String`
//! [`Meter_humanUnit`] port. `isNonnegative(x)` (`Macros.h:141`,
//! `isgreaterequal(x, 0.0)`) is inlined as `x >= 0.0` (false for `NaN`), the
//! same idiom `filedescriptormeter.rs` applies.
//!
//! Ported (self-contained: `RichString`, `CRT_colors`, and `Meter_humanUnit`
//! are ported):
//! - [`SwapMeter_display`] (`SwapMeter.c:45`).
//!
//! Stubbed (blocked on unported substrate — keeps its `todo!()`):
//! - `SwapMeter_updateValues` (`SwapMeter.c:28`) — after seeding the cache /
//!   frontswap values with `NAN`, the actual totals come from
//!   `Platform_setSwapValues(this)`, which is a `todo!()` in
//!   `linux/platform.rs` whose stub signature takes no out-param, so it cannot
//!   populate `this->total` / `this->values[SWAP_METER_USED]`. The subsequent
//!   `this->txtBuffer` formatting is pure, but there is no faithful data
//!   source to drive it (the same blocker keeps `MemoryMeter_updateValues` /
//!   `FileDescriptorMeter_updateValues` stubbed).
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::meter::{Meter, Meter_humanUnit};
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_writeAscii};

/// TODO: port of `static void SwapMeter_updateValues(Meter* this)` from
/// `SwapMeter.c:28`. Blocked: the cache / frontswap slots are seeded with
/// `NAN` here, but the real totals (`this->total`,
/// `this->values[SWAP_METER_USED]`) are filled by `Platform_setSwapValues(this)`
/// — a `todo!()` in `linux/platform.rs` whose stub signature takes no
/// out-param, so it cannot populate the meter. The trailing `Meter_humanUnit`
/// `txtBuffer` formatting is pure, but there is no faithful data source.
pub fn SwapMeter_updateValues() {
    todo!("port of SwapMeter.c:28: needs Platform_setSwapValues (todo!() in linux/platform.rs, no out-param)")
}

/// Port of `static void SwapMeter_display(const Object* cast, RichString* out)`
/// from `SwapMeter.c:45`. Writes `:<total>` (total colored `METER_VALUE`),
/// then ` used:<used>` (used colored `METER_VALUE`); if the cache value is
/// non-negative it appends ` cache:<cache>` (colored `SWAP_CACHE`), and if the
/// frontswap value is non-negative it appends ` frontswap:<frontswap>`
/// (colored `SWAP_FRONTSWAP`). `CRT_colors[X]` is
/// `ColorElements::X.packed(scheme)`; the active scheme is read once (a
/// process-global that does not change mid-call), matching the C global
/// `CRT_colors`.
pub fn SwapMeter_display(this: &Meter, out: &mut RichString) {
    const SWAP_METER_USED: usize = 0;
    const SWAP_METER_CACHE: usize = 1;
    const SWAP_METER_FRONTSWAP: usize = 2;

    let scheme = ColorScheme::active();

    RichString_writeAscii(out, ColorElements::METER_TEXT.packed(scheme), b":");
    let buffer = Meter_humanUnit(this.total);
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE.packed(scheme),
        buffer.as_bytes(),
    );
    let buffer = Meter_humanUnit(this.values[SWAP_METER_USED]);
    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" used:");
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE.packed(scheme),
        buffer.as_bytes(),
    );

    // isNonnegative(this->values[SWAP_METER_CACHE]) — false for NaN.
    if this.values[SWAP_METER_CACHE] >= 0.0 {
        let buffer = Meter_humanUnit(this.values[SWAP_METER_CACHE]);
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" cache:");
        RichString_appendAscii(
            out,
            ColorElements::SWAP_CACHE.packed(scheme),
            buffer.as_bytes(),
        );
    }

    // isNonnegative(this->values[SWAP_METER_FRONTSWAP]) — false for NaN.
    if this.values[SWAP_METER_FRONTSWAP] >= 0.0 {
        let buffer = Meter_humanUnit(this.values[SWAP_METER_FRONTSWAP]);
        RichString_appendAscii(
            out,
            ColorElements::METER_TEXT.packed(scheme),
            b" frontswap:",
        );
        RichString_appendAscii(
            out,
            ColorElements::SWAP_FRONTSWAP.packed(scheme),
            buffer.as_bytes(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Visible characters of the valid `[0, chlen)` range of `out`.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    /// Cache and frontswap both present (non-negative) → full line.
    #[test]
    fn display_with_cache_and_frontswap() {
        let m = Meter {
            total: 1024.0, // KiB → "1.00M"
            values: vec![512.0, 256.0, 128.0],
            ..Meter::empty()
        };
        let mut out = RichString::new();
        SwapMeter_display(&m, &mut out);
        assert_eq!(text(&out), ":1.00M used:512K cache:256K frontswap:128K");
    }

    /// NaN cache / frontswap (the platform default) → neither optional field.
    #[test]
    fn display_omits_nan_optionals() {
        let m = Meter {
            total: 1024.0,
            values: vec![512.0, f64::NAN, f64::NAN],
            ..Meter::empty()
        };
        let mut out = RichString::new();
        SwapMeter_display(&m, &mut out);
        assert_eq!(text(&out), ":1.00M used:512K");
    }
}
