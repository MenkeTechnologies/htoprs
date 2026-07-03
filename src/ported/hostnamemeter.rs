//! Port of `HostnameMeter.c` — htop's hostname meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::linux::platform::Platform_getHostname;
use crate::ported::meter::Meter;

/// Port of `static void HostnameMeter_updateValues(Meter* this)` from
/// `HostnameMeter.c:21`. The whole C body is
/// `Platform_getHostname(this->txtBuffer, sizeof(this->txtBuffer))`; the
/// ported [`Platform_getHostname`] returns the hostname as a `String`, which
/// is stored into the meter's `txtBuffer`.
pub fn HostnameMeter_updateValues(this: &mut Meter) {
    this.txtBuffer = Platform_getHostname();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_values_fills_txt_buffer() {
        // gethostname yields a non-empty name in CI/dev; assert the meter's
        // txtBuffer is populated and matches the libc reading.
        let mut m = Meter {
            ..Meter::empty()
        };
        HostnameMeter_updateValues(&mut m);
        assert_eq!(m.txtBuffer, Platform_getHostname());
        assert!(!m.txtBuffer.is_empty());
    }
}
