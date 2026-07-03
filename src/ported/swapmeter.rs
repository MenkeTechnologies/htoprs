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
//! - [`SwapMeter_updateValues`] (`SwapMeter.c:28`) — seeds the cache /
//!   frontswap slots with `NAN`, then drives the ported
//!   [`Platform_setSwapValues`](crate::ported::linux::platform::Platform_setSwapValues),
//!   which reads the host swap counters (and the zswap adjustment) from the
//!   [`LinuxMachine`](crate::ported::linux::linuxmachine::LinuxMachine) via
//!   the `Meter::host` back-pointer.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // faithful C global names (SwapMeter_class)
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
// Platform dispatch (darwin-first): each build calls its own platform's swap
// value setter, mirroring htop linking one platform's `Platform.c`. The tests
// are `#[cfg]`-split to match (live-invariant on macOS, mocked host on linux).
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_setSwapValues;
#[cfg(not(target_os = "macos"))]
use crate::ported::linux::platform::Platform_setSwapValues;
use crate::ported::meter::{
    Meter, MeterClass, Meter_class, Meter_humanUnit, BAR_METERMODE, METERMODE_DEFAULT_SUPPORTED,
};
use crate::ported::object::ObjectClass;
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_writeAscii};

/// TODO: port of `static void SwapMeter_updateValues(Meter* this)` from
/// Port of `static void SwapMeter_updateValues(Meter* this)` from
/// `SwapMeter.c:28`. Seeds the cache/frontswap slots with `NAN` (not present
/// on all platforms), fills the real figures via the ported
/// [`Platform_setSwapValues`], then formats `txtBuffer` as `used/total`
/// through [`Meter_humanUnit`]. `SwapMeter.h` indices: `USED=0`, `CACHE=1`,
/// `FRONTSWAP=2`.
pub fn SwapMeter_updateValues(this: &mut Meter) {
    const SWAP_METER_USED: usize = 0;
    const SWAP_METER_CACHE: usize = 1;
    const SWAP_METER_FRONTSWAP: usize = 2;

    this.values[SWAP_METER_CACHE] = f64::NAN; // 'cached' not present on all platforms
    this.values[SWAP_METER_FRONTSWAP] = f64::NAN; // 'frontswap' likewise
    Platform_setSwapValues(this);

    this.txtBuffer = format!(
        "{}/{}",
        Meter_humanUnit(this.values[SWAP_METER_USED]),
        Meter_humanUnit(this.total)
    );
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

/// Port of `static const int SwapMeter_attributes[]` from `SwapMeter.c`:
/// `{ SWAP, SWAP_CACHE, SWAP_FRONTSWAP }` — the per-item bar colors as
/// `CRT_colors` indices (`ColorElements as i32`), in `SwapMeter.h` index
/// order (`USED=0`, `CACHE=1`, `FRONTSWAP=2`).
static SwapMeter_attributes: [i32; 3] = [
    ColorElements::SWAP as i32,
    ColorElements::SWAP_CACHE as i32,
    ColorElements::SWAP_FRONTSWAP as i32,
];

/// Port of `const MeterClass SwapMeter_class` from `SwapMeter.c`. Wires the
/// ported [`SwapMeter_updateValues`]/[`SwapMeter_display`] slots onto the
/// vtable. A percent chart (`total = 100.0`), default `BAR_METERMODE`,
/// `maxItems = SWAP_METER_ITEMCOUNT` (3). `super.delete` is dropped (Rust
/// `Drop`); `super.extends` becomes the `Meter_class` base link.
pub static SwapMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: Some(SwapMeter_display),
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(SwapMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: BAR_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 100.0,
    attributes: &SwapMeter_attributes,
    name: "Swap",
    uiName: "Swap",
    caption: "Swp",
    description: None,
    maxItems: 3, // SWAP_METER_ITEMCOUNT (SwapMeter.h:16)
    isMultiColumn: false,
    isPercentChart: true,
};

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
            host: core::ptr::null(),
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
            host: core::ptr::null(),
            total: 1024.0,
            values: vec![512.0, f64::NAN, f64::NAN],
            ..Meter::empty()
        };
        let mut out = RichString::new();
        SwapMeter_display(&m, &mut out);
        assert_eq!(text(&out), ":1.00M used:512K");
    }

    #[cfg(not(target_os = "macos"))]
    use crate::ported::linux::linuxmachine::{LinuxMachine, ZswapStats};
    #[cfg(not(target_os = "macos"))]
    use crate::ported::machine::Machine;

    /// On macOS the swap setter reads real system swap via `sysctl` (no host
    /// mock), so assert live invariants instead of fixed values: used never
    /// exceeds total, both non-negative, and the platform leaves the
    /// cache/frontswap slots at `NAN`.
    #[cfg(target_os = "macos")]
    #[test]
    fn update_values_reads_live_system_swap() {
        let mut m = Meter {
            values: vec![0.0; 3],
            host: core::ptr::null(),
            ..Meter::empty()
        };
        SwapMeter_updateValues(&mut m);
        assert!(m.total >= 0.0);
        assert!(m.values[0] >= 0.0 && m.values[0] <= m.total); // USED
        assert!(m.values[1].is_nan()); // CACHE not set on darwin
        assert!(m.values[2].is_nan()); // FRONTSWAP not set on darwin
    }

    /// update_values pulls the host swap totals; with no zswap the used/cache
    /// slots are the raw counters and txtBuffer is `used/total`.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn update_values_no_zswap() {
        let host = Box::leak(Box::new(LinuxMachine {
            super_: Machine {
                totalSwap: 2048,
                usedSwap: 512,
                cachedSwap: 256,
                ..Default::default()
            },
            ..Default::default()
        }));
        let mut m = Meter {
            values: vec![0.0; 3],
            host: &host.super_ as *const crate::ported::machine::Machine,
            ..Meter::empty()
        };
        SwapMeter_updateValues(&mut m);
        assert_eq!(m.total, 2048.0);
        assert_eq!(m.values[0], 512.0); // USED
        assert_eq!(m.values[1], 256.0); // CACHE
        assert_eq!(m.values[2], 0.0); // FRONTSWAP (no zswap)
        assert_eq!(m.txtBuffer, "512K/2.00M");
    }

    /// zswap subtracts from USED and adds to FRONTSWAP (C: Platform.c:475).
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn update_values_zswap_moves_used_to_frontswap() {
        let host = Box::leak(Box::new(LinuxMachine {
            super_: Machine {
                totalSwap: 2048,
                usedSwap: 512,
                cachedSwap: 256,
                ..Default::default()
            },
            zswap: ZswapStats {
                usedZswapOrig: 100,
                usedZswapComp: 40,
            },
            ..Default::default()
        }));
        let mut m = Meter {
            values: vec![0.0; 3],
            host: &host.super_ as *const crate::ported::machine::Machine,
            ..Meter::empty()
        };
        SwapMeter_updateValues(&mut m);
        assert_eq!(m.values[0], 412.0); // 512 - 100 orig
        assert_eq!(m.values[2], 100.0); // frontswap += orig
    }
}
