//! Port of `HostnameMeter.c` — htop's hostname meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // faithful C global names (HostnameMeter_class)
#![allow(dead_code)]

use crate::ported::crt::ColorElements;
use crate::ported::linux::platform::Platform_getHostname;
use crate::ported::meter::{Meter, MeterClass, Meter_class, TEXT_METERMODE};
use crate::ported::object::ObjectClass;

/// Port of `static void HostnameMeter_updateValues(Meter* this)` from
/// `HostnameMeter.c:21`. The whole C body is
/// `Platform_getHostname(this->txtBuffer, sizeof(this->txtBuffer))`; the
/// ported [`Platform_getHostname`] returns the hostname as a `String`, which
/// is stored into the meter's `txtBuffer`.
pub fn HostnameMeter_updateValues(this: &mut Meter) {
    this.txtBuffer = Platform_getHostname();
}

/// Port of `static const int HostnameMeter_attributes[]` from
/// `HostnameMeter.c`: `{ HOSTNAME }`.
static HostnameMeter_attributes: [i32; 1] = [ColorElements::HOSTNAME as i32];

/// Port of `const MeterClass HostnameMeter_class` from `HostnameMeter.c`. No
/// custom `display` (rendered from `txtBuffer`); TEXT mode only,
/// `maxItems = 0`.
pub static HostnameMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: None,
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(HostnameMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: 1 << TEXT_METERMODE,
    total: 0.0,
    attributes: &HostnameMeter_attributes,
    name: "Hostname",
    uiName: "Hostname",
    caption: "Hostname: ",
    description: None,
    maxItems: 0,
    isMultiColumn: false,
    isPercentChart: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_values_fills_txt_buffer() {
        // gethostname yields a non-empty name in CI/dev; assert the meter's
        // txtBuffer is populated and matches the libc reading.
        let mut m = Meter { ..Meter::empty() };
        HostnameMeter_updateValues(&mut m);
        assert_eq!(m.txtBuffer, Platform_getHostname());
        assert!(!m.txtBuffer.is_empty());
    }
}
