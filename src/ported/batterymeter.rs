//! Partial port of `BatteryMeter.c`.
//!
//! `BatteryMeter_updateValues` remains a documented `todo!()`: a faithful
//! port requires `Platform_getBattery(&mut percent, &mut isOnAC)`, which in
//! Rust currently exists only as a no-arg placeholder
//! (`crate::ported::linux::platform::Platform_getBattery`) that returns
//! nothing, so it cannot supply the percent/AC-presence values. The
//! module's own `ACPresence` type (from `BatteryMeter.h`) is ported below.
#![allow(non_snake_case)]
#![allow(dead_code)]
// ACPresence variants keep their exact C names (AC_ABSENT/AC_PRESENT/AC_ERROR).
#![allow(non_camel_case_types)]

/// Port of `typedef enum ACPresence_ { ... } ACPresence` from
/// `BatteryMeter.h:15`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ACPresence {
    AC_ABSENT,
    AC_PRESENT,
    AC_ERROR,
}

/// TODO: port of `static void BatteryMeter_updateValues(Meter* this)` from
/// `BatteryMeter.c:27`. Blocked: needs a usable
/// `Platform_getBattery(&mut percent, &mut isOnAC)` (only a no-arg stub
/// exists in `linux::platform`) to fill `percent`/`isOnAC`.
pub fn BatteryMeter_updateValues() {
    todo!("port of BatteryMeter.c:27: needs Platform_getBattery(percent, ACPresence)")
}
