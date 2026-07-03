//! Partial port of `linux/ZramMeter.c` — htop's Linux zram (compressed swap)
//! usage meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module. The C `static void
//! ZramMeter_display(const Object* cast, RichString* out)` casts `cast` back to
//! `const Meter*` and reads `this->total` / `this->values[...]`, so it ports to
//! a free fn `pub fn ZramMeter_display(this: &Meter, out: &mut RichString)` —
//! the `cast` → `this` down-cast collapses into the typed `&Meter` parameter
//! (the same mapping `hugepagemeter.rs` / `filedescriptormeter.rs` use). The
//! shared `Meter` model is [`crate::ported::meter::Meter`].
//!
//! `CRT_colors[X]` is reproduced as `ColorElements::X.packed(ColorScheme::active())`
//! and `Meter_humanUnit(buffer, v, sizeof(buffer))` becomes the owned-`String`
//! [`Meter_humanUnit`] port.
//!
//! Ported (self-contained: `RichString`, `CRT_colors`, and `Meter_humanUnit`
//! are ported):
//! - [`ZramMeter_display`] (`ZramMeter.c:51`).
//!
//! Stubbed (blocked on unported substrate — keeps its `todo!()`):
//! - `ZramMeter_updateValues` (`ZramMeter.c:28`) — its first statement is
//!   `Platform_setZramValues(this)`, which populates `this->values[]` /
//!   `this->total` from the platform. The Linux `Platform_setZramValues`
//!   ([`crate::ported::linux::platform::Platform_setZramValues`]) is itself an
//!   unported `todo!()` stub with no `Meter` parameter, so there is no faithful
//!   data source to drive the `txtBuffer` formatting.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::meter::{Meter, Meter_humanUnit};
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_writeAscii};

/// Port of `ZRAM_METER_COMPRESSED` from `ZramMeter.h:14` — index into
/// `Meter::values` for the compressed (used) zram size.
const ZRAM_METER_COMPRESSED: usize = 0;
/// Port of `ZRAM_METER_UNCOMPRESSED` from `ZramMeter.h:15` — index into
/// `Meter::values` for the extra uncompressed size.
const ZRAM_METER_UNCOMPRESSED: usize = 1;

/// TODO: port of `static void ZramMeter_updateValues(Meter* this)` from
/// `ZramMeter.c:28`. Blocked: the body opens with `Platform_setZramValues(this)`,
/// which fills `this->values[]` / `this->total` from the platform before the
/// `txtBuffer` formatting runs. The Linux
/// [`crate::ported::linux::platform::Platform_setZramValues`] is still an
/// unported `todo!()` (no `Meter` parameter), so there is no faithful source to
/// populate the values here.
pub fn ZramMeter_updateValues() {
    todo!("port of ZramMeter.c:28: needs Platform_setZramValues(Meter*)")
}

/// Port of `static void ZramMeter_display(const Object* cast, RichString* out)`
/// from `ZramMeter.c:51`. Writes `:<total>`, then ` used:<compressed>`, then
/// ` uncompressed:<compressed + uncompressed>`, coloring the labels `METER_TEXT`
/// and each value `METER_VALUE`. `CRT_colors[X]` is
/// `ColorElements::X.packed(scheme)`; the active scheme is read once (a
/// process-global that does not change mid-call), matching the C global
/// `CRT_colors`. The human-readable values come from [`Meter_humanUnit`].
pub fn ZramMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();

    RichString_writeAscii(out, ColorElements::METER_TEXT.packed(scheme), b":");

    let buffer = Meter_humanUnit(this.total);
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE.packed(scheme),
        buffer.as_bytes(),
    );

    let buffer = Meter_humanUnit(this.values[ZRAM_METER_COMPRESSED]);
    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" used:");
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE.packed(scheme),
        buffer.as_bytes(),
    );

    let uncompressed = this.values[ZRAM_METER_COMPRESSED] + this.values[ZRAM_METER_UNCOMPRESSED];
    let buffer = Meter_humanUnit(uncompressed);
    RichString_appendAscii(
        out,
        ColorElements::METER_TEXT.packed(scheme),
        b" uncompressed:",
    );
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE.packed(scheme),
        buffer.as_bytes(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Visible characters of the valid `[0, chlen)` range of `out`.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    /// Exercises the total prefix, the ` used:` compressed value, and the
    /// ` uncompressed:` sum (compressed + uncompressed).
    #[test]
    fn display_writes_total_used_and_uncompressed() {
        let m = Meter {
            host: None,
            total: 1024.0,                      // KiB → "1.00M"
            values: vec![1024.0 * 2.0, 1024.0], // compressed "2.00M", +1M uncompressed
            ..Meter::empty()
        };
        let mut out = RichString::new();
        ZramMeter_display(&m, &mut out);
        // ":" + total + " used:" + compressed + " uncompressed:" + (comp+uncomp)
        assert_eq!(text(&out), ":1.00M used:2.00M uncompressed:3.00M");
    }
}
