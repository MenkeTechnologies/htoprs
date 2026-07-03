//! Port of `DateTimeMeter.c` — htop's clock/date/date-time meters.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::meter::Meter;

/// Port of `static void DateTimeMeter_updateValues(Meter* this)` from
/// `DateTimeMeter.c:32`. Formats the host's current sample time with
/// `localtime_r` + `strftime`, choosing the format by concrete meter class:
/// `ClockMeter` → `"%H:%M:%S"`, `DateMeter` → `"%F"`, else
/// (`DateTimeMeter`) → `"%F %H:%M:%S"`.
///
/// Two faithful adaptations: (1) the C reads `host->realtime.tv_sec` (a
/// `struct timeval`); the ported `Machine` models the same sample time as
/// `realtimeMs`, so `tv_sec = realtimeMs / 1000`. (2) The C `As_Meter(this)
/// == &ClockMeter_class` class dispatch is reproduced via the per-instance
/// `uiName` (`"Clock"` / `"Date"` / `"Date and Time"`), which is 1:1 with the
/// concrete `MeterClass`, since the ported `Meter` carries no concrete class
/// pointer.
pub fn DateTimeMeter_updateValues(this: &mut Meter) {
    let secs = {
        let host = this
            .host
            .as_ref()
            .expect("DateTimeMeter_updateValues: this->host (C reads host->realtime)")
            .borrow();
        (host.super_.realtimeMs / 1000) as libc::time_t
    };

    // localtime_r(&host->realtime.tv_sec, &result)
    let mut result: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&secs, &mut result);
    }

    let fmt: &str = match this.uiName {
        "Clock" => "%H:%M:%S",
        "Date" => "%F",
        _ => "%F %H:%M:%S", // DateTime (uiName "Date and Time")
    };

    let cfmt = std::ffi::CString::new(fmt).expect("static strftime format has no NUL");
    let mut buf = [0u8; 64]; // C txtBuffer is 64+ bytes; these formats fit
    let n = unsafe {
        libc::strftime(
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            cfmt.as_ptr(),
            &result,
        )
    };
    this.txtBuffer = String::from_utf8_lossy(&buf[..n]).into_owned();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::linux::linuxmachine::LinuxMachine;
    use crate::ported::machine::Machine;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A meter whose host reports a fixed sample time (ms) and whose class is
    /// selected by `uiName`.
    fn meter(ui_name: &'static str, realtime_ms: u64) -> Meter {
        let host = Rc::new(RefCell::new(LinuxMachine {
            super_: Machine {
                realtimeMs: realtime_ms,
                ..Default::default()
            },
            ..Default::default()
        }));
        Meter {
            uiName: ui_name,
            host: Some(host),
            ..Meter::empty()
        }
    }

    /// The three classes select different strftime formats; assert the shape
    /// (length/separators) which is locale/timezone-independent.
    #[test]
    fn clock_is_hh_mm_ss() {
        let mut m = meter("Clock", 1_000_000_000_000); // arbitrary fixed ms
        DateTimeMeter_updateValues(&mut m);
        // "%H:%M:%S" → 8 chars, two colons.
        assert_eq!(m.txtBuffer.len(), 8);
        assert_eq!(m.txtBuffer.matches(':').count(), 2);
    }

    #[test]
    fn date_is_iso_ymd() {
        let mut m = meter("Date", 1_000_000_000_000);
        DateTimeMeter_updateValues(&mut m);
        // "%F" → YYYY-MM-DD, 10 chars, two dashes.
        assert_eq!(m.txtBuffer.len(), 10);
        assert_eq!(m.txtBuffer.matches('-').count(), 2);
    }

    #[test]
    fn datetime_is_date_space_time() {
        let mut m = meter("Date and Time", 1_000_000_000_000);
        DateTimeMeter_updateValues(&mut m);
        // "%F %H:%M:%S" → 19 chars with one space between date and time.
        assert_eq!(m.txtBuffer.len(), 19);
        assert_eq!(m.txtBuffer.matches(':').count(), 2);
        assert_eq!(m.txtBuffer.matches('-').count(), 2);
    }
}
