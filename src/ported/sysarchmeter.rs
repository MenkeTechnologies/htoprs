//! Port of `SysArchMeter.c` — htop's system architecture (OS release) meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::linux::platform::Platform_getRelease;
use crate::ported::meter::Meter;

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
