//! Port of `Meter.c` — htop's meter layer.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port.
#![allow(non_snake_case)]

/// IEC unit prefixes. Port of `unitPrefixes` from `XUtils.h:160`
/// (`static const char unitPrefixes[] = { 'K', ... 'Q' }`).
const UNIT_PREFIXES: [char; 10] = ['K', 'M', 'G', 'T', 'P', 'E', 'Z', 'Y', 'R', 'Q'];

/// Port of `#define ONE_K 1024UL` from `Row.h:107`, as `f64` for the
/// division in [`Meter_humanUnit`].
const ONE_K: f64 = 1024.0;

/// Port of `int Meter_humanUnit(char* buffer, double value, size_t size)`
/// from `Meter.c:473`. Converts `value` in kibibytes into a human
/// readable string (e.g. `"0K"`, `"1023K"`, `"98.7M"`, `"1.23G"`).
///
/// Signature mapping: the C writes into the caller's `char* buffer`
/// bounded by `size` and returns the `xSnprintf` byte count. Rust owns
/// its allocation, so the `buffer`/`size` out-param and the `int`
/// return are dropped in favor of an owned `String` — the same mapping
/// `xutils.rs` applies to the varargs formatters.
///
/// The C `assert(value >= 0.0 || isNaN(value))` is dropped: it is a
/// debug-only precondition, not input validation, so no check is added.
pub fn Meter_humanUnit(mut value: f64) -> String {
    let mut i: usize = 0;

    while value >= ONE_K {
        if i >= UNIT_PREFIXES.len() - 1 {
            if value > 9999.0 {
                return "inf".to_string();
            }
            break;
        }

        value /= ONE_K;
        i += 1;
    }

    let mut precision = 0;

    if i > 0 {
        // Fraction digits for mebibytes and above
        precision = if value <= 99.9 {
            if value <= 9.99 {
                2
            } else {
                1
            }
        } else {
            0
        };

        // Round up if 'value' is in range (99.9, 100) or (9.99, 10)
        if precision < 2 {
            let limit = if precision == 1 { 10.0 } else { 100.0 };
            if value < limit {
                value = limit;
            }
        }
    }

    format!("{:.*}{}", precision, value, UNIT_PREFIXES[i])
}

/// Port of `typedef unsigned int MeterModeId` from `MeterMode.h:19`. The
/// mode ids are the `enum MeterModeId_` values (`MeterMode.h:11`); mode `0`
/// is reserved, so the real modes start at `1` and `LAST_METERMODE` is the
/// trailing count sentinel.
pub type MeterModeId = u32;

/// `BAR_METERMODE = 1` (`MeterMode.h:13`).
pub const BAR_METERMODE: MeterModeId = 1;
/// `TEXT_METERMODE` (`MeterMode.h:14`).
pub const TEXT_METERMODE: MeterModeId = 2;
/// `GRAPH_METERMODE` (`MeterMode.h:15`).
pub const GRAPH_METERMODE: MeterModeId = 3;
/// `LED_METERMODE` (`MeterMode.h:16`).
pub const LED_METERMODE: MeterModeId = 4;
/// `LAST_METERMODE` — trailing count sentinel (`MeterMode.h:17`).
pub const LAST_METERMODE: MeterModeId = 5;

/// A partial model of htop's `struct Meter_` (`Meter.h:111`) holding the
/// fields the pure-logic ports in this module read:
///   * `values` — the per-item value array (C `double* values`);
///   * `curItems` — the number of live entries at the front of `values`
///     (C `uint8_t curItems`);
///   * `mode` — the current draw mode (C `MeterModeId mode`), read by
///     [`Meter_nextSupportedMode`];
///   * `supportedModes` — the bitset of supported modes. In C this is a
///     `const uint32_t` on the `MeterClass` vtable, read through the
///     instance via the `Meter_supportedModes(this)` macro (`Meter.h`);
///     it is modeled here as an instance field carrying that class
///     constant, which is exactly the value the macro yields.
///
/// The remaining C fields — `super`, `draw`, `host`, `caption`, `param`,
/// `drawData`, `h`, `columnWidthCount`, `curAttributes`, `txtBuffer`,
/// `total`, `meterData` — are omitted; they are substrate (vtable
/// dispatch, terminal draw state) these ports do not touch.
pub struct Meter {
    pub values: Vec<f64>,
    pub curItems: u8,
    pub mode: MeterModeId,
    pub supportedModes: u32,
}

/// Port of `static double Meter_computeSum(const Meter* this)` from
/// `Meter.c:51`. Sums the strictly-positive live values
/// (`sumPositiveValues(this->values, this->curItems)`) and clamps the
/// result to `DBL_MAX` so IEEE-754 rounding cannot yield infinity.
///
/// The C `assert(this->curItems > 0)` and `assert(this->values)` are
/// debug-only preconditions (not input validation), so they are dropped —
/// the same treatment [`Meter_humanUnit`] gives its `assert`.
pub fn Meter_computeSum(this: &Meter) -> f64 {
    let sum = crate::ported::xutils::sumPositiveValues(&this.values[..this.curItems as usize]);
    // Prevent rounding to infinity in IEEE 754. `MINIMUM(DBL_MAX, sum)`
    // expands to `((DBL_MAX) < (sum) ? (DBL_MAX) : (sum))` (`Macros.h:17`).
    if f64::MAX < sum {
        f64::MAX
    } else {
        sum
    }
}

/// Port of `MeterModeId Meter_nextSupportedMode(const Meter* this)` from
/// `Meter.c:556`. Given the current `mode`, returns the next supported
/// mode id, cycling back to the lowest supported mode once the highest is
/// passed. The selection is a pure bit operation over the
/// `supportedModes` bitset: mask off every mode id `<= this->mode`
/// (`((uint32_t)-1 << 1) << this->mode`), and if nothing remains fall back
/// to the full set, then take the lowest set bit
/// ([`countTrailingZeros`](crate::ported::xutils::countTrailingZeros)).
///
/// The C `assert(supportedModes)` and `assert(this->mode < UINT32_WIDTH)`
/// are debug-only preconditions, kept as `debug_assert!`. As in C, the
/// shift by `this->mode` is only well-defined for `mode < 32`.
pub fn Meter_nextSupportedMode(this: &Meter) -> MeterModeId {
    let supportedModes = this.supportedModes;
    debug_assert!(supportedModes != 0);
    debug_assert!(this.mode < 32);

    let mode_mask = (u32::MAX << 1) << this.mode;
    let mut next_modes = supportedModes & mode_mask;
    if next_modes == 0 {
        next_modes = supportedModes;
    }

    crate::ported::xutils::countTrailingZeros(next_modes) as MeterModeId
}

/// TODO: port of `static inline void Meter_displayBuffer(const Meter*
/// this, RichString* out)` from `Meter.c:43`. Stubbed: the `Object_display`
/// branch needs the `Object` vtable dispatch (`Object_displayFn` /
/// `Object_display`) and the else branch needs `Meter_attributes(this)[0]`
/// (the `MeterClass` vtable), `this->txtBuffer`, and `CRT_colors` — none of
/// that substrate is ported yet.
pub fn Meter_displayBuffer() {
    todo!("port of Meter.c:43")
}

/// TODO: port of `static void TextMeterMode_draw(Meter* this, int x, int y,
/// int w)` from `Meter.c:61`. Stubbed: the body is terminal cursor drawing
/// (`attrset`, `mvaddnstr`) plus `Meter_displayBuffer` and
/// `RichString_printoffnVal`, none of which are ported (no terminal layer,
/// and `RichString_printoffnVal` is absent from `richstring.rs`).
pub fn TextMeterMode_draw() {
    todo!("port of Meter.c:61")
}

/// TODO: port of `static void BarMeterMode_draw(Meter* this, int x, int y,
/// int w)` from `Meter.c:89`. Stubbed: the bar-fill string assembly needs
/// RichString primitives absent from `richstring.rs`
/// (`RichString_setChar` as a public fn, `RichString_getCharVal`,
/// `RichString_sizeVal`), and the surrounding body is terminal cursor
/// drawing (`attrset`, `mvaddch`, `move`, `RichString_printoffnVal`). The
/// bar glyphs are `BarMeterMode_characters = "|#*@$%&."` (`Meter.c:87`),
/// with `'|'` used outside `COLORSCHEME_MONOCHROME`.
pub fn BarMeterMode_draw() {
    todo!("port of Meter.c:89")
}

/// TODO: port of `void Meter_setMode(Meter* this, MeterModeId modeIndex)`
/// from `Meter.c:525`. Stubbed: the body assigns C draw function pointers
/// (`this->draw = mode->draw`), reads the `MeterClass` vtable
/// (`Meter_updateModeFn` / `Meter_drawFn` / `Meter_supportedModes`), calls
/// `Meter_updateMode`, resets the `GraphData` `drawData` buffer, and
/// indexes the `Meter_modes[]` draw-fn/height table (`Meter.c:419`) — none
/// of that substrate (vtable dispatch, function pointers, `GraphData`, the
/// `Meter_modes` table) is ported yet.
pub fn Meter_setMode() {
    todo!("port of Meter.c:525")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_stays_kibibytes_no_fraction() {
        // 0 < ONE_K: loop never runs, i=0 => precision 0, prefix 'K'.
        assert_eq!(Meter_humanUnit(0.0), "0K");
    }

    #[test]
    fn below_one_k_stays_kibibytes() {
        // 999 < 1024: no division, i=0, precision 0.
        assert_eq!(Meter_humanUnit(999.0), "999K");
    }

    #[test]
    fn one_k_promotes_to_mebi_two_fraction_digits() {
        // 1024 -> one division -> i=1, value=1.0; 1.0 <= 9.99 => prec 2.
        assert_eq!(Meter_humanUnit(1024.0), "1.00M");
    }

    #[test]
    fn precision_one_in_range() {
        // 1024*50 -> value=50.0, i=1; 50 <= 99.9 but > 9.99 => prec 1;
        // limit 10.0, 50 not < 10 => "50.0M".
        assert_eq!(Meter_humanUnit(1024.0 * 50.0), "50.0M");
    }

    #[test]
    fn precision_zero_above_ninety_nine_nine() {
        // 1024*500 -> value=500.0, i=1; 500 > 99.9 => prec 0;
        // limit 100.0, 500 not < 100 => "500M".
        assert_eq!(Meter_humanUnit(1024.0 * 500.0), "500M");
    }

    #[test]
    fn round_up_boundary_forces_limit() {
        // 1024*9.995 -> value~9.995, i=1; 9.995 > 9.99 => prec 1;
        // limit 10.0, 9.995 < 10 => value forced to 10.0 => "10.0M".
        assert_eq!(Meter_humanUnit(1024.0 * 9.995), "10.0M");
    }

    #[test]
    fn inf_when_still_huge_at_last_prefix() {
        // After 9 divisions i reaches len-1=9 with value=19998 > 9999
        // => early "inf" return.
        let v = 9999.0 * f64::powi(1024.0, 9) * 2.0;
        assert_eq!(Meter_humanUnit(v), "inf");
    }

    #[test]
    fn caps_at_last_prefix_without_inf() {
        // After 9 divisions i=9, value=5000 <= 9999 => break, format
        // with prefix 'Q'; 5000 > 99.9 => prec 0 => "5000Q".
        let v = 5000.0 * f64::powi(1024.0, 9);
        assert_eq!(Meter_humanUnit(v), "5000Q");
    }

    #[test]
    fn compute_sum_ignores_negatives_and_nan() {
        // sumPositiveValues skips values <= 0 and NaN: 5 + 2 = 7.
        let m = Meter {
            values: vec![5.0, -3.0, f64::NAN, 2.0],
            curItems: 4,
            mode: BAR_METERMODE,
            supportedModes: 0,
        };
        assert_eq!(Meter_computeSum(&m), 7.0);
    }

    #[test]
    fn compute_sum_honors_cur_items() {
        // Only the first curItems entries are summed; trailing 100.0 unused.
        let m = Meter {
            values: vec![1.0, 2.0, 100.0],
            curItems: 2,
            mode: BAR_METERMODE,
            supportedModes: 0,
        };
        assert_eq!(Meter_computeSum(&m), 3.0);
    }

    #[test]
    fn compute_sum_clamps_to_dbl_max() {
        // Two DBL_MAX positives overflow to +inf; MINIMUM(DBL_MAX, inf)
        // picks DBL_MAX since DBL_MAX < inf.
        let m = Meter {
            values: vec![f64::MAX, f64::MAX],
            curItems: 2,
            mode: BAR_METERMODE,
            supportedModes: 0,
        };
        assert_eq!(Meter_computeSum(&m), f64::MAX);
    }

    // ── Meter_nextSupportedMode ───────────────────────────────────────

    /// `METERMODE_DEFAULT_SUPPORTED` (`MeterMode.h:21`): all four real
    /// modes supported = bits 1..4 set.
    const ALL_MODES: u32 =
        (1 << BAR_METERMODE) | (1 << TEXT_METERMODE) | (1 << GRAPH_METERMODE) | (1 << LED_METERMODE);

    fn mode_meter(mode: MeterModeId, supportedModes: u32) -> Meter {
        Meter { values: vec![], curItems: 0, mode, supportedModes }
    }

    #[test]
    fn next_supported_mode_cycles_through_all_modes() {
        // With every mode supported, cycling advances 1->2->3->4 and wraps
        // 4->1 (LED back to BAR).
        assert_eq!(Meter_nextSupportedMode(&mode_meter(BAR_METERMODE, ALL_MODES)), TEXT_METERMODE);
        assert_eq!(Meter_nextSupportedMode(&mode_meter(TEXT_METERMODE, ALL_MODES)), GRAPH_METERMODE);
        assert_eq!(Meter_nextSupportedMode(&mode_meter(GRAPH_METERMODE, ALL_MODES)), LED_METERMODE);
        // highest mode wraps to the lowest supported mode
        assert_eq!(Meter_nextSupportedMode(&mode_meter(LED_METERMODE, ALL_MODES)), BAR_METERMODE);
    }

    #[test]
    fn next_supported_mode_skips_unsupported_modes() {
        // Only BAR and LED supported: BAR -> LED (skips TEXT/GRAPH),
        // LED wraps back to BAR.
        let supported = (1 << BAR_METERMODE) | (1 << LED_METERMODE);
        assert_eq!(Meter_nextSupportedMode(&mode_meter(BAR_METERMODE, supported)), LED_METERMODE);
        assert_eq!(Meter_nextSupportedMode(&mode_meter(LED_METERMODE, supported)), BAR_METERMODE);
    }

    #[test]
    fn next_supported_mode_single_mode_stays_put() {
        // Only TEXT supported: the mask above TEXT is empty, so it falls
        // back to the full set and returns TEXT again.
        let supported = 1 << TEXT_METERMODE;
        assert_eq!(Meter_nextSupportedMode(&mode_meter(TEXT_METERMODE, supported)), TEXT_METERMODE);
    }

    #[test]
    fn next_supported_mode_from_lower_than_all_supported() {
        // mode below the lowest supported bit: BAR (1) current, but only
        // GRAPH and LED supported -> next is GRAPH.
        let supported = (1 << GRAPH_METERMODE) | (1 << LED_METERMODE);
        assert_eq!(Meter_nextSupportedMode(&mode_meter(BAR_METERMODE, supported)), GRAPH_METERMODE);
    }
}
