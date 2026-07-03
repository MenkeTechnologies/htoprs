//! Port of `BatteryMeter.c`.
//!
//! The module's own `ACPresence` type (from `BatteryMeter.h`) and
//! `BatteryMeter_updateValues` are ported below. The latter is driven by the
//! ported [`Platform_getBattery`],
//! which fills the `percent`/AC-presence out-params.
#![allow(non_snake_case)]
#![allow(dead_code)]
// ACPresence variants keep their exact C names (AC_ABSENT/AC_PRESENT/AC_ERROR).
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)] // faithful C global names (BatteryMeter_class)

use crate::ported::crt::ColorElements;
use crate::ported::linux::platform::Platform_getBattery;
use crate::ported::meter::{
    Meter, MeterClass, Meter_class, METERMODE_DEFAULT_SUPPORTED, TEXT_METERMODE,
};
use crate::ported::object::ObjectClass;

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

/// Port of `static const int BatteryMeter_attributes[]` from `BatteryMeter.c`:
/// `{ BATTERY }`.
static BatteryMeter_attributes: [i32; 1] = [ColorElements::BATTERY as i32];

/// Port of `const MeterClass BatteryMeter_class` from `BatteryMeter.c`. No
/// custom `display` (rendered from `txtBuffer` / the default bar). A percent
/// chart (`total = 100.0`), `maxItems = 1`.
pub static BatteryMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: None,
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(BatteryMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 100.0,
    attributes: &BatteryMeter_attributes,
    name: "Battery",
    uiName: "Battery",
    caption: "Battery: ",
    description: None,
    maxItems: 1,
    isMultiColumn: false,
    isPercentChart: true,
};
