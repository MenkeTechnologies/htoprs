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
//! Ported:
//! - [`ZramMeter_display`] (`ZramMeter.c:51`).
//! - [`ZramMeter_updateValues`] (`ZramMeter.c:28`) — drives the ported
//!   [`Platform_setZramValues`], which reads the zram counters from the host
//!   [`LinuxMachine`](crate::ported::linux::linuxmachine::LinuxMachine) via
//!   the `Meter::host` back-pointer.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::linux::platform::Platform_setZramValues;
use crate::ported::meter::{Meter, Meter_humanUnit};
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_writeAscii};

/// Port of `ZRAM_METER_COMPRESSED` from `ZramMeter.h:14` — index into
/// `Meter::values` for the compressed (used) zram size.
const ZRAM_METER_COMPRESSED: usize = 0;
/// Port of `ZRAM_METER_UNCOMPRESSED` from `ZramMeter.h:15` — index into
/// `Meter::values` for the extra uncompressed size.
const ZRAM_METER_UNCOMPRESSED: usize = 1;

/// Port of `static void ZramMeter_updateValues(Meter* this)` from
/// `ZramMeter.c:28`. Fills `this->values[]`/`this->total` via the ported
/// [`Platform_setZramValues`], then formats `txtBuffer` as
/// `<comp>(<comp+uncomp>)/<total>` through [`Meter_humanUnit`].
pub fn ZramMeter_updateValues(this: &mut Meter) {
    Platform_setZramValues(this);

    let uncompressed =
        this.values[ZRAM_METER_COMPRESSED] + this.values[ZRAM_METER_UNCOMPRESSED];
    // C: "<comp>(<comp+uncomp>)/<total>", each figure via Meter_humanUnit.
    this.txtBuffer = format!(
        "{}({})/{}",
        Meter_humanUnit(this.values[ZRAM_METER_COMPRESSED]),
        Meter_humanUnit(uncompressed),
        Meter_humanUnit(this.total)
    );
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

    /// update_values pulls zram counters from the host: values[0]=comp,
    /// values[1]=orig-comp, total=device size; txtBuffer is
    /// `<comp>(<comp+uncomp>)/<total>`.
    #[test]
    fn update_values_from_host_zram() {
        use crate::ported::linux::linuxmachine::{LinuxMachine, ZramStats};
        use std::cell::RefCell;
        use std::rc::Rc;
        let host = Rc::new(RefCell::new(LinuxMachine {
            zram: ZramStats {
                totalZram: 1024,       // "1.00M"
                usedZramComp: 1024 * 2, // "2.00M"
                usedZramOrig: 1024 * 3, // orig-comp = 1M uncompressed
            },
            ..Default::default()
        }));
        let mut m = Meter {
            values: vec![0.0; 2],
            host: Some(host),
            ..Meter::empty()
        };
        ZramMeter_updateValues(&mut m);
        assert_eq!(m.total, 1024.0);
        assert_eq!(m.values[0], 2048.0);
        assert_eq!(m.values[1], 1024.0);
        assert_eq!(m.txtBuffer, "2.00M(3.00M)/1.00M");
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
