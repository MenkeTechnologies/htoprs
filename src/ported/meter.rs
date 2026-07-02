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
}
