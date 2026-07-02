//! Partial port of `NetworkIOMeter.c` — htop's network-IO rate meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. The C `static void
//! NetworkIOMeter_display(ATTR_UNUSED const Object* cast, RichString* out)`
//! ports to a free fn `pub fn NetworkIOMeter_display(out: &mut RichString)`:
//! the `cast` argument is `ATTR_UNUSED` (the data comes from file-scope
//! statics, not the object), so it is dropped — the same out-param → return
//! mapping `DiskIOMeter.c` uses.
//!
//! The file-scope static block (`NetworkIOMeter.c:30`-`36`) — the
//! `MeterRateStatus status` plus the `cached_*` rate caches — is modeled as
//! one `Mutex`-guarded [`NetworkIOMeterState`]. C reads/writes these as
//! unsynchronized single-threaded file statics; the `Mutex` is the safe-Rust
//! analog for module-private mutable state (the same idiom `crt.rs` uses for
//! `CRT_degreeSign`, and `diskiometer.rs` for its cache). The cache is
//! written only by `NetworkIOMeter_updateValues` (stubbed, see below), so in
//! a running port it holds its initial `RATESTATUS_INIT` state; the display
//! reader is nonetheless ported exactly, and the tests populate the cache
//! directly to exercise the `RATESTATUS_DATA` branch.
//!
//! Ported (self-contained: `RichString` + `CRT_colors` are ported):
//! - [`NetworkIOMeter_display`] (`NetworkIOMeter.c:132`) — on a non-data
//!   status, writes a single colored word (`"no data"` / `"initializing..."`
//!   / `"stale data"`) and returns; on `RATESTATUS_DATA`, appends
//!   `rx: <r>iB/s tx: <t>iB/s (<rxpps>/<txpps> pps)`, coloring the labels
//!   `METER_TEXT` and each figure by its IO direction. `xSnprintf(buffer,
//!   ..., "%u", (unsigned int)cached_rxp_diff)` becomes `format!("{}", ...)`.
//!
//! Stubbed (blocked on unported substrate — keeps its `todo!()`):
//! - `NetworkIOMeter_updateValues` (`:38`) — the rate math itself is pure,
//!   but it is driven by `Platform_getNetworkIO(&data)` (the
//!   platform-specific network-stat reader, no platform layer ported) over a
//!   `NetworkIOData`, gated on `host->realtimeMs`, and writes
//!   `this->values[0..2]` / `this->txtBuffer`; the partial `Meter` in
//!   `meter.rs` models neither `txtBuffer` nor `host`. `Meter_humanUnit`
//!   (`meter.rs`) is ported and would supply the `cached_*_str` fields.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::sync::Mutex;

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendnAscii, RichString_writeAscii,
};

/// Port of `typedef enum { ... } MeterRateStatus` from `Meter.h:131`.
/// Same order/discriminants as the C (`RATESTATUS_DATA` == 0). Private:
/// only this file's meter uses it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum MeterRateStatus {
    RATESTATUS_DATA,
    RATESTATUS_INIT,
    RATESTATUS_NODATA,
    RATESTATUS_STALE,
}

/// Models the file-scope static block of `NetworkIOMeter.c` (`:30`-`36`):
/// the shared `status` plus the rate caches the meter reads. `cached_*_str`
/// are the C `char[6]` human-unit buffers (`"1023"`, `"98.7"`, ...);
/// `cached_*_diff` are the raw doubles copied into `this->values[]`;
/// `cached_*p_diff` are the `uint32_t` packet-per-second counters. Held
/// behind a `Mutex` because Rust module-private mutable statics need interior
/// mutability (the C statics are single-threaded and unlocked).
struct NetworkIOMeterState {
    /// `static MeterRateStatus status = RATESTATUS_INIT` (`:30`).
    status: MeterRateStatus,
    /// `static double cached_rxb_diff` (`:31`).
    cached_rxb_diff: f64,
    /// `static char cached_rxb_diff_str[6]` (`:32`).
    cached_rxb_diff_str: String,
    /// `static uint32_t cached_rxp_diff` (`:33`).
    cached_rxp_diff: u32,
    /// `static double cached_txb_diff` (`:34`).
    cached_txb_diff: f64,
    /// `static char cached_txb_diff_str[6]` (`:35`).
    cached_txb_diff_str: String,
    /// `static uint32_t cached_txp_diff` (`:36`).
    cached_txp_diff: u32,
}

/// The single instance of the file-scope static block. Zero-initialized
/// like C, except `status`, which C initializes to `RATESTATUS_INIT`.
static NETWORK_IO_METER_STATE: Mutex<NetworkIOMeterState> = Mutex::new(NetworkIOMeterState {
    status: MeterRateStatus::RATESTATUS_INIT,
    cached_rxb_diff: 0.0,
    cached_rxb_diff_str: String::new(),
    cached_rxp_diff: 0,
    cached_txb_diff: 0.0,
    cached_txb_diff_str: String::new(),
    cached_txp_diff: 0,
});

/// TODO: port of `static void NetworkIOMeter_updateValues(Meter* this)` from
/// `NetworkIOMeter.c:38`. Blocked on `Platform_getNetworkIO(&data)` (the
/// platform-specific network-stat reader; no platform layer ported), the
/// `NetworkIOData` struct it fills, and `host->realtimeMs`; it also writes
/// `this->values[0..2]` / `this->txtBuffer`, and the partial `Meter` in
/// `meter.rs` models neither `txtBuffer` nor `host`. The rate arithmetic and
/// `Meter_humanUnit` (`meter.rs`) are available, but without the platform
/// source there is nothing to compute over.
pub fn NetworkIOMeter_updateValues() {
    todo!("port of NetworkIOMeter.c:38: needs Platform_getNetworkIO/NetworkIOData, Meter.host/realtimeMs, Meter.values/txtBuffer")
}

/// Port of `static void NetworkIOMeter_display(ATTR_UNUSED const Object*
/// cast, RichString* out)` from `NetworkIOMeter.c:132`. On a non-data
/// status, writes a single colored word (`"no data"` / `"initializing..."`
/// / `"stale data"`) and returns; on `RATESTATUS_DATA`, appends
/// `rx: <r>iB/s tx: <t>iB/s (<rxpps>/<txpps> pps)`, coloring the labels
/// `METER_TEXT` and each rate/count by its IO direction. `CRT_colors[X]` is
/// `ColorElements::X.packed(ColorScheme::active())`. The C `xSnprintf(buffer,
/// ..., "%u", (unsigned int)cached_rxp_diff)` + `RichString_appendnAscii(...,
/// buffer, len)` becomes `format!("{}", ...)` fed to `RichString_appendnAscii`.
pub fn NetworkIOMeter_display(out: &mut RichString) {
    let scheme = ColorScheme::active();
    let state = NETWORK_IO_METER_STATE.lock().unwrap();

    match state.status {
        MeterRateStatus::RATESTATUS_NODATA => {
            RichString_writeAscii(
                out,
                ColorElements::METER_VALUE_ERROR.packed(scheme),
                b"no data",
            );
            return;
        }
        MeterRateStatus::RATESTATUS_INIT => {
            RichString_writeAscii(
                out,
                ColorElements::METER_VALUE.packed(scheme),
                b"initializing...",
            );
            return;
        }
        MeterRateStatus::RATESTATUS_STALE => {
            RichString_writeAscii(
                out,
                ColorElements::METER_VALUE_WARN.packed(scheme),
                b"stale data",
            );
            return;
        }
        MeterRateStatus::RATESTATUS_DATA => {}
    }

    RichString_writeAscii(out, ColorElements::METER_TEXT.packed(scheme), b"rx: ");
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE_IOREAD.packed(scheme),
        state.cached_rxb_diff_str.as_bytes(),
    );
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE_IOREAD.packed(scheme),
        b"iB/s",
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" tx: ");
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE_IOWRITE.packed(scheme),
        state.cached_txb_diff_str.as_bytes(),
    );
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE_IOWRITE.packed(scheme),
        b"iB/s",
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" (");
    let buffer = format!("{}", state.cached_rxp_diff);
    RichString_appendnAscii(
        out,
        ColorElements::METER_VALUE_IOREAD.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );
    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b"/");
    let buffer = format!("{}", state.cached_txp_diff);
    RichString_appendnAscii(
        out,
        ColorElements::METER_VALUE_IOWRITE.packed(scheme),
        buffer.as_bytes(),
        buffer.len(),
    );
    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" pps)");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the display tests: they share the single file-scope
    /// [`NETWORK_IO_METER_STATE`], so a test sets it, then runs a display —
    /// two steps that must not interleave with another test.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Visible characters of the valid `[0, chlen)` range of `out`.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    /// Overwrite the whole file-scope cache for a test.
    fn set_state(status: MeterRateStatus, rxb: &str, txb: &str, rxp: u32, txp: u32) {
        let mut s = NETWORK_IO_METER_STATE.lock().unwrap();
        s.status = status;
        s.cached_rxb_diff_str = rxb.to_string();
        s.cached_txb_diff_str = txb.to_string();
        s.cached_rxp_diff = rxp;
        s.cached_txp_diff = txp;
    }

    #[test]
    fn display_status_words() {
        let _g = TEST_LOCK.lock().unwrap();

        set_state(MeterRateStatus::RATESTATUS_NODATA, "", "", 0, 0);
        let mut out = RichString::new();
        NetworkIOMeter_display(&mut out);
        assert_eq!(text(&out), "no data");

        set_state(MeterRateStatus::RATESTATUS_INIT, "", "", 0, 0);
        let mut out = RichString::new();
        NetworkIOMeter_display(&mut out);
        assert_eq!(text(&out), "initializing...");

        set_state(MeterRateStatus::RATESTATUS_STALE, "", "", 0, 0);
        let mut out = RichString::new();
        NetworkIOMeter_display(&mut out);
        assert_eq!(text(&out), "stale data");
    }

    #[test]
    fn display_data_line() {
        let _g = TEST_LOCK.lock().unwrap();

        set_state(MeterRateStatus::RATESTATUS_DATA, "1.23G", "45.6M", 120, 34);
        let mut out = RichString::new();
        NetworkIOMeter_display(&mut out);
        assert_eq!(text(&out), "rx: 1.23GiB/s tx: 45.6MiB/s (120/34 pps)");
    }
}
