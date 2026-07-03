//! Port of `UptimeMeter.c` — htop's uptime meters.
//!
//! Both `updateValues` bodies open with `int totalseconds =
//! Platform_getUptime();`. The ported
//! [`Platform_getUptime`]
//! now returns an `int` (`i32`), so both functions are portable: on a
//! non-positive uptime they write `"(unknown)"` into `this->txtBuffer`
//! (modeled by [`crate::ported::meter::Meter`]); otherwise the seconds are
//! broken down into days/hours/minutes/seconds and formatted. The pure
//! `xSnprintf` formatting maps to Rust `format!` into the `String`
//! `txtBuffer`, matching the idiom used by `filedescriptormeter.rs`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // faithful C global names (UptimeMeter_class)
#![allow(dead_code)]

use crate::ported::crt::ColorElements;
// Per-OS platform selection (see htop's Platform.c linking). macOS reads uptime
// from `sysctl KERN_BOOTTIME` (darwin::platform); other targets keep the linux
// path. Without this the macOS build ran the Linux `/proc/uptime` reader, which
// fails off-Linux → "Uptime: (unknown)".
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_getUptime;
#[cfg(not(target_os = "macos"))]
use crate::ported::linux::platform::Platform_getUptime;
use crate::ported::meter::{Meter, MeterClass, Meter_class, LED_METERMODE, TEXT_METERMODE};
use crate::ported::object::ObjectClass;

/// Port of `static void UptimeMeter_updateValues(Meter* this)` from
/// `UptimeMeter.c:22`. Reads `Platform_getUptime()`; a non-positive value
/// writes `"(unknown)"`. Otherwise the total seconds are split into
/// seconds/minutes/hours/days (`% 60`, `/ 60 % 60`, `/ 3600 % 24`,
/// `/ 86400`), a `daysbuf` prefix is built (`"%d days(!), "` when `days >
/// 100`, `"%d days, "` when `days > 1`, `"1 day, "` when `days == 1`, else
/// empty), and `this->txtBuffer` is set to `"%s%02d:%02d:%02d"`.
pub fn UptimeMeter_updateValues(this: &mut Meter) {
    let totalseconds = Platform_getUptime();
    if totalseconds <= 0 {
        this.txtBuffer = "(unknown)".to_string();
        return;
    }

    let seconds = totalseconds % 60;
    let minutes = (totalseconds / 60) % 60;
    let hours = (totalseconds / 3600) % 24;
    let days = totalseconds / 86400;

    let daysbuf = if days > 100 {
        format!("{} days(!), ", days)
    } else if days > 1 {
        format!("{} days, ", days)
    } else if days == 1 {
        "1 day, ".to_string()
    } else {
        String::new()
    };
    this.txtBuffer = format!("{}{:02}:{:02}:{:02}", daysbuf, hours, minutes, seconds);
}

/// Port of `static void SecondsUptimeMeter_updateValues(Meter* this)` from
/// `UptimeMeter.c:64`. Reads `Platform_getUptime()`; a non-positive value
/// writes `"(unknown)"`, otherwise `this->txtBuffer` is set to `"%d s"`.
pub fn SecondsUptimeMeter_updateValues(this: &mut Meter) {
    let totalseconds = Platform_getUptime();
    if totalseconds <= 0 {
        this.txtBuffer = "(unknown)".to_string();
        return;
    }
    this.txtBuffer = format!("{} s", totalseconds);
}

/// Port of `static const int UptimeMeter_attributes[]` from `UptimeMeter.c`:
/// `{ UPTIME }`. Shared by both uptime classes.
static UptimeMeter_attributes: [i32; 1] = [ColorElements::UPTIME as i32];

/// Port of `const MeterClass UptimeMeter_class` from `UptimeMeter.c`. No
/// custom `display` (C class leaves it `NULL`; the meter renders its
/// `txtBuffer` via `Meter_displayBuffer`). `maxItems = 0`, TEXT/LED modes.
pub static UptimeMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: None,
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(UptimeMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: (1 << TEXT_METERMODE) | (1 << LED_METERMODE),
    total: 0.0,
    attributes: &UptimeMeter_attributes,
    name: "Uptime",
    uiName: "Uptime",
    caption: "Uptime: ",
    description: None,
    maxItems: 0,
    isMultiColumn: false,
    isPercentChart: false,
};

/// Port of `const MeterClass SecondsUptimeMeter_class` from `UptimeMeter.c`.
/// Same as [`UptimeMeter_class`] but drives [`SecondsUptimeMeter_updateValues`]
/// (raw seconds).
pub static SecondsUptimeMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: None,
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(SecondsUptimeMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: (1 << TEXT_METERMODE) | (1 << LED_METERMODE),
    total: 0.0,
    attributes: &UptimeMeter_attributes,
    name: "SecondsUptime",
    uiName: "Uptime (seconds)",
    caption: "Uptime: ",
    description: None,
    maxItems: 0,
    isMultiColumn: false,
    isPercentChart: false,
};
