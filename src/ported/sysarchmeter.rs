//! Port of `SysArchMeter.c` — htop's system architecture (OS release) meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // faithful C global names (SysArchMeter_class)
#![allow(dead_code)]

use crate::ported::crt::ColorElements;
// SysArchMeter.c calls `Platform_getRelease()`, resolved per-build to the
// linked platform's implementation. On macOS that is the darwin
// CoreFoundation-backed reader ("macOS <version>"); every other host uses the
// generic `uname(2)` + `/etc/os-release` reader exposed by the linux module.
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_getRelease;
#[cfg(not(target_os = "macos"))]
use crate::ported::linux::platform::Platform_getRelease;
use crate::ported::meter::{Meter, MeterClass, Meter_class, TEXT_METERMODE};
use crate::ported::object::ObjectClass;

/// Port of `static void SysArchMeter_updateValues(Meter* this)` from
/// `SysArchMeter.c:22`. The C body is
/// `String_safeStrncpy(this->txtBuffer, Platform_getRelease(), size)`; the
/// ported [`Platform_getRelease`] returns the cached `"<sysname> <release>
/// [<machine>][ @ <distro>]"` string, stored into the meter's `txtBuffer`
/// (the port's `String` is unbounded, so the fixed-buffer truncation is not
/// modeled).
pub fn SysArchMeter_updateValues(this: &mut Meter) {
    this.txtBuffer = Platform_getRelease().to_string();
}

/// Port of `static const int SysArchMeter_attributes[]` from `SysArchMeter.c`:
/// `{ HOSTNAME }` (the meter reuses the hostname color).
static SysArchMeter_attributes: [i32; 1] = [ColorElements::HOSTNAME as i32];

/// Port of `const MeterClass SysArchMeter_class` from `SysArchMeter.c`. No
/// custom `display` (rendered from `txtBuffer`); TEXT mode only,
/// `maxItems = 0`. C `name` is `"System"`.
pub static SysArchMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: None,
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(SysArchMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: 1 << TEXT_METERMODE,
    total: 0.0,
    attributes: &SysArchMeter_attributes,
    name: "System",
    uiName: "System",
    caption: "System: ",
    description: None,
    maxItems: 0,
    isMultiColumn: false,
    isPercentChart: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_values_fills_release_string() {
        let mut m = Meter { ..Meter::empty() };
        SysArchMeter_updateValues(&mut m);
        // uname always succeeds → a non-empty "<sysname> <release> [<machine>]".
        assert_eq!(m.txtBuffer, Platform_getRelease());
        assert!(!m.txtBuffer.is_empty());
        assert!(m.txtBuffer.contains('['));
    }
}
