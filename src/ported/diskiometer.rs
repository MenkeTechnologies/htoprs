//! Partial port of `DiskIOMeter.c` — htop's disk-IO rate/time meters.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C `static void
//! Foo_display(const Object* cast, RichString* out)` ports to a free fn
//! `pub fn Foo_display(out: &mut RichString)`: the `cast` argument is
//! `ATTR_UNUSED` in every display fn (the data comes from file-scope
//! statics, not the object), so it is dropped, the same way the
//! out-param → return mappings elsewhere drop unused C parameters.
//!
//! The file-scope static block (`DiskIOMeter.c:38`-`45`) — the
//! `MeterRateStatus status` plus the `cached_*` rate/utilisation caches —
//! is modeled as one `Mutex`-guarded [`DiskIOMeterState`]. C reads/writes
//! these as unsynchronized single-threaded file statics; the `Mutex` is
//! the safe-Rust analog for module-private mutable state (the same idiom
//! `crt.rs` uses for `CRT_degreeSign`). The cache is written only by
//! `DiskIOUpdateCache` (stubbed, see below), so in a running port it
//! holds its initial `RATESTATUS_INIT` state; the display readers are
//! nonetheless ported exactly, and the tests populate the cache directly
//! to exercise the `RATESTATUS_DATA` branches.
//!
//! Ported (self-contained: `RichString` + `CRT_colors` are ported):
//! - [`DiskIORateMeter_display`] (`DiskIOMeter.c:139`) — read/write byte
//!   rate line; status branches write a single colored word, the data
//!   branch appends `read: <r>iB/s write: <w>iB/s`.
//! - [`DiskIOTimeMeter_display`] (`DiskIOMeter.c:191`) — busy-percent
//!   line; the busy figure is `METER_VALUE_NOTICE` above 40%, else
//!   `METER_VALUE`, with an optional ` (<n> disks)` suffix.
//! - [`DiskIOMeter_display`] (`DiskIOMeter.c:221`) — combined display:
//!   the rate line, then (only in the data branch) `"; "` and the time
//!   line.
//!
//! Stubbed (blocked on unported substrate — each keeps its `todo!()`):
//! - `DiskIOUpdateCache` (`:47`) — the rate/utilisation math itself is
//!   pure, but it is driven by `Platform_getDiskIO(&data)` (the
//!   platform-specific disk-stat reader, no platform layer ported) over a
//!   `DiskIOData` and gated on `host->realtimeMs`; without the platform
//!   source there is nothing to feed the cache. `Meter_humanUnit`
//!   (`meter.rs`) is ported and would supply the `cached_*_str` fields.
//! - `DiskIORateMeter_updateValues` (`:116`) /
//!   `DiskIOTimeMeter_updateValues` (`:163`) — call `DiskIOUpdateCache`
//!   (stubbed) and write `this->values[...]` / `this->txtBuffer`; the
//!   partial `Meter` in `meter.rs` models neither `txtBuffer` nor `host`.
//! - `DiskIOMeter_updateValues` (`:237`) — reads `this->meterData`
//!   (`DiskIOMeterData`, two sub-`Meter` pointers) and dispatches
//!   `Meter_updateValues` through the `MeterClass` vtable, unported.
//! - `DiskIOMeter_draw` (`:244`) — dispatches `meter->draw(...)` function
//!   pointers and reads `this->mode`; no vtable / terminal draw layer.
//! - `DiskIOMeter_init` (`:265`) — `xCalloc`, `Meter_new`, `Meter_init`,
//!   `Meter_initFn`, and `Class(DiskIORateMeter)` vtable references.
//! - `DiskIOMeter_updateMode` (`:285`) — `Meter_setMode` (itself stubbed
//!   in `meter.rs`) and reads the sub-meters' `->h`.
//! - `DiskIOMeter_done` (`:296`) — `Meter_delete` + `free`; `Drop` frees
//!   owned fields, so there is no free-everything body to port.
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
/// only this file's meters use it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum MeterRateStatus {
    RATESTATUS_DATA,
    RATESTATUS_INIT,
    RATESTATUS_NODATA,
    RATESTATUS_STALE,
}

/// Models the file-scope static block of `DiskIOMeter.c` (`:38`-`45`):
/// the shared `status` plus the rate/utilisation caches the meters read.
/// `cached_*_str` are the C `char[6]` human-unit buffers (`"1023"`,
/// `"98.7"`, ...); `cached_*_diff` / `cached_*_norm` are the raw doubles
/// `*_updateValues` copies into `this->values[]`. Held behind a `Mutex`
/// because Rust module-private mutable statics need interior mutability
/// (the C statics are single-threaded and unlocked).
struct DiskIOMeterState {
    /// `static MeterRateStatus status = RATESTATUS_INIT` (`:38`).
    status: MeterRateStatus,
    /// `static double cached_read_diff` (`:39`).
    cached_read_diff: f64,
    /// `static char cached_read_diff_str[6]` (`:40`).
    cached_read_diff_str: String,
    /// `static double cached_write_diff` (`:41`).
    cached_write_diff: f64,
    /// `static char cached_write_diff_str[6]` (`:42`).
    cached_write_diff_str: String,
    /// `static uint64_t cached_num_disks` (`:43`).
    cached_num_disks: u64,
    /// `static double cached_utilisation_diff` (`:44`).
    cached_utilisation_diff: f64,
    /// `static double cached_utilisation_norm` (`:45`).
    cached_utilisation_norm: f64,
}

/// The single instance of the file-scope static block. Zero-initialized
/// like C, except `status`, which C initializes to `RATESTATUS_INIT`.
static DISK_IO_METER_STATE: Mutex<DiskIOMeterState> = Mutex::new(DiskIOMeterState {
    status: MeterRateStatus::RATESTATUS_INIT,
    cached_read_diff: 0.0,
    cached_read_diff_str: String::new(),
    cached_write_diff: 0.0,
    cached_write_diff_str: String::new(),
    cached_num_disks: 0,
    cached_utilisation_diff: 0.0,
    cached_utilisation_norm: 0.0,
});

/// TODO: port of `static void DiskIOUpdateCache(const Machine* host)` from
/// `DiskIOMeter.c:47`. Blocked on `Platform_getDiskIO(&data)` (the
/// platform-specific disk-stat reader; no platform layer ported) and
/// `host->realtimeMs`. The rate/utilisation arithmetic and
/// `Meter_humanUnit` (`meter.rs`) are available, but without the platform
/// source there is nothing to compute over.
pub fn DiskIOUpdateCache() {
    todo!("port of DiskIOMeter.c:47")
}

/// TODO: port of `static void DiskIORateMeter_updateValues(Meter* this)`
/// from `DiskIOMeter.c:116`. Blocked: calls `DiskIOUpdateCache` (stubbed)
/// and writes `this->values[0..2]` / `this->txtBuffer`; the partial
/// `Meter` in `meter.rs` models neither `txtBuffer` nor `host`.
pub fn DiskIORateMeter_updateValues() {
    todo!("port of DiskIOMeter.c:116")
}

/// Port of `static void DiskIORateMeter_display(ATTR_UNUSED const Object*
/// cast, RichString* out)` from `DiskIOMeter.c:139`. On a non-data
/// status, writes a single colored word (`"no data"` / `"initializing..."`
/// / `"stale data"`) and returns; on `RATESTATUS_DATA`, appends
/// `read: <r>iB/s write: <w>iB/s`, coloring the labels `METER_TEXT` and
/// each rate by its IO direction. `CRT_colors[X]` is
/// `ColorElements::X.packed(ColorScheme::active())`.
pub fn DiskIORateMeter_display(out: &mut RichString) {
    let scheme = ColorScheme::active();
    let state = DISK_IO_METER_STATE.lock().unwrap();

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

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b"read: ");
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE_IOREAD.packed(scheme),
        state.cached_read_diff_str.as_bytes(),
    );
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE_IOREAD.packed(scheme),
        b"iB/s",
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" write: ");
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE_IOWRITE.packed(scheme),
        state.cached_write_diff_str.as_bytes(),
    );
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE_IOWRITE.packed(scheme),
        b"iB/s",
    );
}

/// TODO: port of `static void DiskIOTimeMeter_updateValues(Meter* this)`
/// from `DiskIOMeter.c:163`. Blocked: calls `DiskIOUpdateCache` (stubbed)
/// and writes `this->values[0]` / `this->txtBuffer`; the partial `Meter`
/// in `meter.rs` models neither field.
pub fn DiskIOTimeMeter_updateValues() {
    todo!("port of DiskIOMeter.c:163")
}

/// Port of `static void DiskIOTimeMeter_display(ATTR_UNUSED const Object*
/// cast, RichString* out)` from `DiskIOMeter.c:191`. On a non-data
/// status, writes a single colored word and returns; on `RATESTATUS_DATA`,
/// appends `<pct>% busy`, coloring the percentage `METER_VALUE_NOTICE`
/// when busy > 40% else `METER_VALUE`, then — when `1 < num_disks < 1000`
/// — a ` (<n> disks)` suffix. `xSnprintf(buffer, ..., "%.1f%%", ...)`
/// (`%%` is a literal `%`) becomes `format!("{:.1}%", ...)`; the `%u`
/// count is written from `cached_num_disks as u32`, matching the C
/// `(unsigned int)` cast.
pub fn DiskIOTimeMeter_display(out: &mut RichString) {
    let scheme = ColorScheme::active();
    let state = DISK_IO_METER_STATE.lock().unwrap();

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

    let color = if state.cached_utilisation_diff > 40.0 {
        ColorElements::METER_VALUE_NOTICE
    } else {
        ColorElements::METER_VALUE
    };
    let buffer = format!("{:.1}%", state.cached_utilisation_diff);
    RichString_appendnAscii(out, color.packed(scheme), buffer.as_bytes(), buffer.len());
    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" busy");

    if state.cached_num_disks > 1 && state.cached_num_disks < 1000 {
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" (");
        let buffer = format!("{}", state.cached_num_disks as u32);
        RichString_appendnAscii(
            out,
            ColorElements::METER_VALUE.packed(scheme),
            buffer.as_bytes(),
            buffer.len(),
        );
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" disks)");
    }
}

/// Port of `static void DiskIOMeter_display(const Object* cast,
/// RichString* out)` from `DiskIOMeter.c:221`. Draws the rate line first;
/// then, only on `RATESTATUS_DATA`, appends `"; "` and the time line. The
/// `status` read sits between the two sub-display calls, so the shared
/// `Mutex` is locked just long enough to copy the (`Copy`) status out —
/// the sub-displays lock it themselves, and `std::sync::Mutex` is not
/// reentrant.
pub fn DiskIOMeter_display(out: &mut RichString) {
    DiskIORateMeter_display(out);

    let status = DISK_IO_METER_STATE.lock().unwrap().status;
    match status {
        MeterRateStatus::RATESTATUS_NODATA
        | MeterRateStatus::RATESTATUS_INIT
        | MeterRateStatus::RATESTATUS_STALE => return,
        MeterRateStatus::RATESTATUS_DATA => {}
    }

    RichString_appendAscii(
        out,
        ColorElements::METER_TEXT.packed(ColorScheme::active()),
        b"; ",
    );
    DiskIOTimeMeter_display(out);
}

/// TODO: port of `static void DiskIOMeter_updateValues(Meter* this)` from
/// `DiskIOMeter.c:237`. Blocked: reads `this->meterData` (`DiskIOMeterData`,
/// the two sub-`Meter` pointers) and dispatches `Meter_updateValues`
/// through the `MeterClass` vtable, neither of which is ported.
pub fn DiskIOMeter_updateValues() {
    todo!("port of DiskIOMeter.c:237")
}

/// TODO: port of `static void DiskIOMeter_draw(Meter* this, int x, int y,
/// int w)` from `DiskIOMeter.c:244`. Blocked: dispatches the sub-meters'
/// `->draw(...)` function pointers and reads `this->mode`; there is no
/// vtable / terminal draw layer ported.
pub fn DiskIOMeter_draw() {
    todo!("port of DiskIOMeter.c:244")
}

/// TODO: port of `static void DiskIOMeter_init(Meter* this)` from
/// `DiskIOMeter.c:265`. Blocked: `xCalloc`, `Meter_new`, `Meter_init`,
/// `Meter_initFn`, and the `Class(DiskIORateMeter)` / `Class(DiskIOTimeMeter)`
/// vtable references, none ported.
pub fn DiskIOMeter_init() {
    todo!("port of DiskIOMeter.c:265")
}

/// TODO: port of `static void DiskIOMeter_updateMode(Meter* this,
/// MeterModeId mode)` from `DiskIOMeter.c:285`. Blocked: `Meter_setMode`
/// (itself stubbed in `meter.rs`) and reads the sub-meters' `->h` to size
/// `this->h`.
pub fn DiskIOMeter_updateMode() {
    todo!("port of DiskIOMeter.c:285")
}

/// TODO: port of `static void DiskIOMeter_done(Meter* this)` from
/// `DiskIOMeter.c:296`. Blocked: `Meter_delete` on each sub-meter and
/// `free(data)`; `Drop` frees owned fields, so there is no
/// free-everything body to port faithfully.
pub fn DiskIOMeter_done() {
    todo!("port of DiskIOMeter.c:296")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the display tests: they share the single file-scope
    /// [`DISK_IO_METER_STATE`], so a test sets it, then runs a display —
    /// two steps that must not interleave with another test.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Visible characters of the valid `[0, chlen)` range of `out`.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    /// Overwrite the whole file-scope cache for a test.
    fn set_state(status: MeterRateStatus, read: &str, write: &str, util: f64, disks: u64) {
        let mut s = DISK_IO_METER_STATE.lock().unwrap();
        s.status = status;
        s.cached_read_diff_str = read.to_string();
        s.cached_write_diff_str = write.to_string();
        s.cached_utilisation_diff = util;
        s.cached_num_disks = disks;
    }

    #[test]
    fn rate_display_status_words() {
        let _g = TEST_LOCK.lock().unwrap();

        set_state(MeterRateStatus::RATESTATUS_NODATA, "", "", 0.0, 0);
        let mut out = RichString::new();
        DiskIORateMeter_display(&mut out);
        assert_eq!(text(&out), "no data");

        set_state(MeterRateStatus::RATESTATUS_INIT, "", "", 0.0, 0);
        let mut out = RichString::new();
        DiskIORateMeter_display(&mut out);
        assert_eq!(text(&out), "initializing...");

        set_state(MeterRateStatus::RATESTATUS_STALE, "", "", 0.0, 0);
        let mut out = RichString::new();
        DiskIORateMeter_display(&mut out);
        assert_eq!(text(&out), "stale data");
    }

    #[test]
    fn rate_display_data_line() {
        let _g = TEST_LOCK.lock().unwrap();

        set_state(MeterRateStatus::RATESTATUS_DATA, "1.23G", "45.6M", 0.0, 0);
        let mut out = RichString::new();
        DiskIORateMeter_display(&mut out);
        assert_eq!(text(&out), "read: 1.23GiB/s write: 45.6MiB/s");
    }

    #[test]
    fn time_display_status_words() {
        let _g = TEST_LOCK.lock().unwrap();

        set_state(MeterRateStatus::RATESTATUS_NODATA, "", "", 0.0, 0);
        let mut out = RichString::new();
        DiskIOTimeMeter_display(&mut out);
        assert_eq!(text(&out), "no data");
    }

    #[test]
    fn time_display_busy_no_disk_suffix() {
        let _g = TEST_LOCK.lock().unwrap();

        // num_disks <= 1 => no suffix.
        set_state(MeterRateStatus::RATESTATUS_DATA, "", "", 12.34, 1);
        let mut out = RichString::new();
        DiskIOTimeMeter_display(&mut out);
        assert_eq!(text(&out), "12.3% busy");
    }

    #[test]
    fn time_display_busy_with_disk_suffix() {
        let _g = TEST_LOCK.lock().unwrap();

        // 1 < num_disks < 1000 => suffix; util > 40 exercises the NOTICE
        // color branch (color isn't asserted here, only the text).
        set_state(MeterRateStatus::RATESTATUS_DATA, "", "", 87.65, 4);
        let mut out = RichString::new();
        DiskIOTimeMeter_display(&mut out);
        assert_eq!(text(&out), "87.7% busy (4 disks)");
    }

    #[test]
    fn time_display_suffix_suppressed_at_1000_disks() {
        let _g = TEST_LOCK.lock().unwrap();

        // num_disks >= 1000 => suffix suppressed (the C `< 1000` guard).
        set_state(MeterRateStatus::RATESTATUS_DATA, "", "", 5.0, 1000);
        let mut out = RichString::new();
        DiskIOTimeMeter_display(&mut out);
        assert_eq!(text(&out), "5.0% busy");
    }

    #[test]
    fn combined_display_data_joins_with_semicolon() {
        let _g = TEST_LOCK.lock().unwrap();

        set_state(MeterRateStatus::RATESTATUS_DATA, "10K", "20K", 33.3, 2);
        let mut out = RichString::new();
        DiskIOMeter_display(&mut out);
        assert_eq!(
            text(&out),
            "read: 10KiB/s write: 20KiB/s; 33.3% busy (2 disks)"
        );
    }

    #[test]
    fn combined_display_nondata_is_rate_word_only() {
        let _g = TEST_LOCK.lock().unwrap();

        // Non-data status: only the rate line's word, no "; " and no time
        // line (the C returns before appending the separator).
        set_state(MeterRateStatus::RATESTATUS_STALE, "", "", 0.0, 0);
        let mut out = RichString::new();
        DiskIOMeter_display(&mut out);
        assert_eq!(text(&out), "stale data");
    }
}
