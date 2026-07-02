//! Port of `BatteryMeter.c`.
//!
//! The module's own `ACPresence` type (from `BatteryMeter.h`) and
//! `BatteryMeter_updateValues` are ported below. The latter is driven by the
//! ported [`Platform_getBattery`](crate::ported::linux::platform::Platform_getBattery),
//! which fills the `percent`/AC-presence out-params.
#![allow(non_snake_case)]
#![allow(dead_code)]
// ACPresence variants keep their exact C names (AC_ABSENT/AC_PRESENT/AC_ERROR).
#![allow(non_camel_case_types)]

use crate::ported::linux::platform::Platform_getBattery;
use crate::ported::meter::{Meter, TEXT_METERMODE};

/// Port of `typedef enum ACPresence_ { ... } ACPresence` from
/// `BatteryMeter.h:15`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ACPresence {
    AC_ABSENT,
    AC_PRESENT,
    AC_ERROR,
}

/// Port of `static void BatteryMeter_updateValues(Meter* this)` from
/// `BatteryMeter.c:27`. Reads the battery percentage and AC presence via
/// [`Platform_getBattery`]. When the percent is negative or `NaN`
/// (`!isNonnegative(percent)`, `Macros.h:141` = `!isgreaterequal(x, 0.0)`)
/// the value is set to `NAN` and the text buffer to `"N/A"`. Otherwise the
/// value is stored and `txtBuffer` is formatted as `"%.1f%%%s"` with an AC
/// suffix that varies by `this->mode` (verbose in `TEXT_METERMODE`, terse
/// otherwise).
pub fn BatteryMeter_updateValues(this: &mut Meter) {
    let mut isOnAC = ACPresence::AC_ABSENT;
    let mut percent: f64 = 0.0;

    Platform_getBattery(&mut percent, &mut isOnAC);

    // !isNonnegative(percent) — negative or NaN.
    if !(percent >= 0.0) {
        this.values[0] = f64::NAN;
        this.txtBuffer = "N/A".to_string();
        return;
    }

    this.values[0] = percent;

    let text: &str = match isOnAC {
        ACPresence::AC_PRESENT => {
            if this.mode == TEXT_METERMODE {
                " (Running on A/C)"
            } else {
                "(A/C)"
            }
        }
        ACPresence::AC_ABSENT => {
            if this.mode == TEXT_METERMODE {
                " (Running on battery)"
            } else {
                "(bat)"
            }
        }
        ACPresence::AC_ERROR => "",
    };

    this.txtBuffer = format!("{:.1}%{}", percent, text);
}
