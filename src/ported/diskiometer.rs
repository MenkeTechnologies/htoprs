//! Port of `DiskIOMeter.c` — htop's disk-IO rate/time meters.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C `static void
//! Foo_display(const Object* cast, RichString* out)` ports to a free fn
//! `pub fn Foo_display(this: &Meter, out: &mut RichString)`: the `cast`
//! argument is `ATTR_UNUSED` in every display fn (the data comes from
//! file-scope statics, not the object), but the mirrored `this: &Meter` is
//! kept (unused) so the fn fits the `MeterClass.display` vtable slot —
//! matching the sibling meter convention (`loadaveragemeter.rs`).
//!
//! The file-scope static block (`DiskIOMeter.c:38`-`45`) — the
//! `MeterRateStatus status` plus the `cached_*` rate/utilisation caches —
//! is modeled as one `Mutex`-guarded [`DiskIOMeterState`]. C reads/writes
//! these as unsynchronized single-threaded file statics; the `Mutex` is
//! the safe-Rust analog for module-private mutable state (the same idiom
//! `crt.rs` uses for `CRT_degreeSign`). The cache is written by
//! [`DiskIOUpdateCache`] (driven by the ported `Platform_getDiskIO`); the
//! display readers are ported exactly, and the tests populate the cache
//! directly to exercise the `RATESTATUS_DATA` branches.
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
//! - [`DiskIOUpdateCache`] (`:47`) / [`DiskIORateMeter_updateValues`]
//!   (`:116`) / [`DiskIOTimeMeter_updateValues`] (`:163`) — the rate cache
//!   refresh (driven by the ported `Platform_getDiskIO`) and the two
//!   sub-meter value updaters.
//! - The composite [`DiskIOMeter_updateValues`] (`:237`) /
//!   [`DiskIOMeter_draw`] (`:244`) / [`DiskIOMeter_init`] (`:265`) /
//!   [`DiskIOMeter_updateMode`] (`:285`) / [`DiskIOMeter_done`] (`:296`) —
//!   now that `Meter.meterData`/`host`, `Meter_new`, `Meter_setMode`, and the
//!   mirrored instance vtable slots are ported, the composite dispatches to
//!   its [`DiskIORateMeter_class`]/[`DiskIOTimeMeter_class`] sub-meters (the
//!   `CPUMeter` multi-column precedent).
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)] // faithful C class/global names (DiskIORateMeter_class, …)
#![allow(dead_code)]

use std::io::Write;
use std::sync::Mutex;

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::meter::{
    Meter, MeterClass, MeterModeId, Meter_class, Meter_humanUnit, Meter_new, Meter_setMode,
    LED_METERMODE, METERMODE_DEFAULT_SUPPORTED, TEXT_METERMODE,
};
use crate::ported::object::ObjectClass;
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

/// Port of `typedef struct DiskIOData_` (`DiskIOMeter.h:15`) — the raw disk
/// counters read by `Platform_getDiskIO`.
#[derive(Default)]
pub struct DiskIOData {
    pub totalBytesRead: u64,
    pub totalBytesWritten: u64,
    pub totalMsTimeSpend: u64,
    pub numDisks: u64,
}

/// C `ONE_K` (`Macros.h`) — 1024, the KiB divisor the rate strings use.
const ONE_K: f64 = 1024.0;

/// The three `DiskIOUpdateCache` function-`static` running totals:
/// `(cached_last_update, cached_read_total, cached_write_total,
/// cached_msTimeSpend_total)`.
static DISK_IO_UPDATE_CACHE: Mutex<(u64, u64, u64, u64)> = Mutex::new((0, 0, 0, 0));

/// Port of `static void DiskIOUpdateCache(const Machine* host)` from
/// `DiskIOMeter.c:47`. Throttled to once per >500ms; reads
/// [`Platform_getDiskIO`](crate::ported::linux::platform::Platform_getDiskIO),
/// sets the rate `status`, and (past the first sample) computes read/write
/// B/s and disk utilisation into the shared state. `host` is the concrete
/// [`LinuxMachine`]; `realtimeMs` lives on its `super_`.
pub fn DiskIOUpdateCache(host: &crate::ported::linux::linuxmachine::LinuxMachine) {
    let realtime_ms = host.super_.realtimeMs;
    let mut c = DISK_IO_UPDATE_CACHE.lock().unwrap();
    let passed_time_ms = realtime_ms.wrapping_sub(c.0);

    // update only every 500ms to have a sane span for rate calculation
    if passed_time_ms <= 500 {
        return;
    }

    let mut data = DiskIOData::default();
    let has_new_data = crate::ported::linux::platform::Platform_getDiskIO(&mut data);

    let mut st = DISK_IO_METER_STATE.lock().unwrap();
    st.status = if !has_new_data {
        MeterRateStatus::RATESTATUS_NODATA
    } else if c.0 == 0 {
        MeterRateStatus::RATESTATUS_INIT
    } else if passed_time_ms > 30000 {
        MeterRateStatus::RATESTATUS_STALE
    } else {
        MeterRateStatus::RATESTATUS_DATA
    };

    c.0 = realtime_ms;

    if !has_new_data {
        return;
    }

    if st.status != MeterRateStatus::RATESTATUS_INIT {
        let read_diff = if data.totalBytesRead > c.1 {
            (1000 * (data.totalBytesRead - c.1)) / passed_time_ms
        } else {
            0
        };
        st.cached_read_diff = read_diff as f64;
        st.cached_read_diff_str = Meter_humanUnit(st.cached_read_diff / ONE_K);

        let write_diff = if data.totalBytesWritten > c.2 {
            (1000 * (data.totalBytesWritten - c.2)) / passed_time_ms
        } else {
            0
        };
        st.cached_write_diff = write_diff as f64;
        st.cached_write_diff_str = Meter_humanUnit(st.cached_write_diff / ONE_K);

        st.cached_num_disks = data.numDisks;
        st.cached_utilisation_diff = 0.0;
        st.cached_utilisation_norm = 0.0;
        if data.totalMsTimeSpend > c.3 {
            let diff = data.totalMsTimeSpend - c.3;
            st.cached_utilisation_diff = 100.0 * diff as f64 / passed_time_ms as f64;
            if data.numDisks > 0 {
                st.cached_utilisation_norm =
                    (diff as f64 / (passed_time_ms as f64 * data.numDisks as f64)).min(1.0);
            }
        }
    }

    c.1 = data.totalBytesRead;
    c.2 = data.totalBytesWritten;
    c.3 = data.totalMsTimeSpend;
}

/// Port of `static void DiskIORateMeter_updateValues(Meter* this)` from
/// `DiskIOMeter.c:116`. Refreshes the cache, writes read/write B/s into
/// `values[0..2]`, and formats `txtBuffer` — a status word
/// (`no data`/`init`/`stale`) or `r:<x>iB/s w:<y>iB/s`.
pub fn DiskIORateMeter_updateValues(this: &mut crate::ported::meter::Meter) {
    {
        // Linux-path meter: on Linux the host is a LinuxMachine (this is
        // dead on darwin, where DiskIO is unported). Downcast the base host.
        let h = unsafe { &*(this.host as *const crate::ported::linux::linuxmachine::LinuxMachine) };
        DiskIOUpdateCache(h);
    }

    let st = DISK_IO_METER_STATE.lock().unwrap();
    this.values[0] = st.cached_read_diff;
    this.values[1] = st.cached_write_diff;

    match st.status {
        MeterRateStatus::RATESTATUS_NODATA => {
            this.txtBuffer = "no data".to_string();
            return;
        }
        MeterRateStatus::RATESTATUS_INIT => {
            this.txtBuffer = "init".to_string();
            return;
        }
        MeterRateStatus::RATESTATUS_STALE => {
            this.txtBuffer = "stale".to_string();
            return;
        }
        MeterRateStatus::RATESTATUS_DATA => {}
    }

    this.txtBuffer = format!(
        "r:{}iB/s w:{}iB/s",
        st.cached_read_diff_str, st.cached_write_diff_str
    );
}

/// Port of `static void DiskIORateMeter_display(ATTR_UNUSED const Object*
/// cast, RichString* out)` from `DiskIOMeter.c:139`. On a non-data
/// status, writes a single colored word (`"no data"` / `"initializing..."`
/// / `"stale data"`) and returns; on `RATESTATUS_DATA`, appends
/// `read: <r>iB/s write: <w>iB/s`, coloring the labels `METER_TEXT` and
/// each rate by its IO direction. `CRT_colors[X]` is
/// `ColorElements::X.packed(ColorScheme::active())`.
pub fn DiskIORateMeter_display(this: &Meter, out: &mut RichString) {
    let _ = this;
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

/// Port of `static void DiskIOTimeMeter_updateValues(Meter* this)` from
/// `DiskIOMeter.c:163`. Refreshes the cache, writes the normalized
/// utilisation into `values[0]`, and formats `txtBuffer` — a status word or
/// `<util>%[ (<n>disks)]`.
pub fn DiskIOTimeMeter_updateValues(this: &mut crate::ported::meter::Meter) {
    {
        // Linux-path meter (dead on darwin); downcast the base host.
        let h = unsafe { &*(this.host as *const crate::ported::linux::linuxmachine::LinuxMachine) };
        DiskIOUpdateCache(h);
    }

    let st = DISK_IO_METER_STATE.lock().unwrap();
    this.values[0] = st.cached_utilisation_norm;

    match st.status {
        MeterRateStatus::RATESTATUS_NODATA => {
            this.txtBuffer = "no data".to_string();
            return;
        }
        MeterRateStatus::RATESTATUS_INIT => {
            this.txtBuffer = "init".to_string();
            return;
        }
        MeterRateStatus::RATESTATUS_STALE => {
            this.txtBuffer = "stale".to_string();
            return;
        }
        MeterRateStatus::RATESTATUS_DATA => {}
    }

    let num_disks_str = if st.cached_num_disks > 1 && st.cached_num_disks < 1000 {
        format!(" ({}disks)", st.cached_num_disks)
    } else {
        String::new()
    };
    this.txtBuffer = format!("{:.1}%{}", st.cached_utilisation_diff, num_disks_str);
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
pub fn DiskIOTimeMeter_display(this: &Meter, out: &mut RichString) {
    let _ = this;
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
pub fn DiskIOMeter_display(this: &Meter, out: &mut RichString) {
    DiskIORateMeter_display(this, out);

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
    DiskIOTimeMeter_display(this, out);
}

/// Port of `typedef struct DiskIOMeterData_` (`DiskIOMeter.c:34`): the
/// composite meter's private `meterData`, holding the rate and time sub-meters.
/// The C `Meter*` pointers become owned `Meter`s (dropping the `Box` reclaims
/// them, replacing the C `free`).
struct DiskIOMeterData {
    diskIORateMeter: Meter,
    diskIOTimeMeter: Meter,
}

impl DiskIOMeterData {
    /// Borrows `this.meterData` as the `DiskIOMeterData` set by
    /// [`DiskIOMeter_init`]. A Rust-only borrow helper (the `CPUMeterData::of`
    /// precedent); an associated fn, so the port-purity gate requires no C
    /// counterpart.
    fn of(this: &mut Meter) -> &mut DiskIOMeterData {
        this.meterData
            .as_mut()
            .and_then(|d| d.downcast_mut::<DiskIOMeterData>())
            .expect("DiskIO meter: meterData is not an initialized DiskIOMeterData")
    }
}

/// Port of `static const int DiskIORateMeter_attributes[]` (`DiskIOMeter.c:29`).
static DiskIORateMeter_attributes: [i32; 2] = [
    ColorElements::METER_VALUE_IOREAD as i32,
    ColorElements::METER_VALUE_IOWRITE as i32,
];

/// Port of `static const int DiskIOTimeMeter_attributes[]` (`DiskIOMeter.c:34`).
static DiskIOTimeMeter_attributes: [i32; 1] = [ColorElements::METER_VALUE_NOTICE as i32];

/// Port of `const MeterClass DiskIORateMeter_class` (`DiskIOMeter.c:306`): the
/// read/write byte-rate sub-meter. Wires [`DiskIORateMeter_updateValues`] and
/// [`DiskIORateMeter_display`]. `super.delete` → `Drop`; `super.extends` → the
/// `Meter_class` base link. Default `TEXT_METERMODE`, `total = 1.0`.
pub static DiskIORateMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: Some(DiskIORateMeter_display),
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(DiskIORateMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 1.0,
    attributes: &DiskIORateMeter_attributes,
    name: "DiskIORate",
    uiName: "Disk IO Rate",
    caption: "Dsk: ",
    description: Some("Disk IO read & write bytes per second"),
    maxItems: 2,
    isMultiColumn: false,
    isPercentChart: false,
};

/// Port of `const MeterClass DiskIOTimeMeter_class` (`DiskIOMeter.c:323`): the
/// percent-busy sub-meter. Wires [`DiskIOTimeMeter_updateValues`] and
/// [`DiskIOTimeMeter_display`]. A percent chart, default `TEXT_METERMODE`.
pub static DiskIOTimeMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: Some(DiskIOTimeMeter_display),
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(DiskIOTimeMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 1.0,
    attributes: &DiskIOTimeMeter_attributes,
    name: "DiskIOTime",
    uiName: "Disk IO Time",
    caption: "Dsk: ",
    description: Some("Disk percent time busy"),
    maxItems: 1,
    isMultiColumn: false,
    isPercentChart: true,
};

/// Port of `const MeterClass DiskIOMeter_class` (`DiskIOMeter.c:342`): the
/// combined multi-column meter compositing the rate and time sub-meters. Wires
/// the ported [`DiskIOMeter_display`]/[`DiskIOMeter_updateValues`]/
/// [`DiskIOMeter_draw`]/[`DiskIOMeter_init`]/[`DiskIOMeter_updateMode`]/
/// [`DiskIOMeter_done`] slots. The C sets no `.attributes`/`.total`/`.maxItems`
/// (`NULL`/`0`), so those default (empty slice / `0.0` / `0`); the empty
/// attribute slice is never indexed because the class provides a `display`
/// slot and its `draw` dispatches to the sub-meters.
pub static DiskIOMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: Some(DiskIOMeter_display),
    init: Some(DiskIOMeter_init),
    done: Some(DiskIOMeter_done),
    updateMode: Some(DiskIOMeter_updateMode),
    updateValues: Some(DiskIOMeter_updateValues),
    draw: Some(DiskIOMeter_draw),
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 0.0,
    attributes: &[],
    name: "DiskIO",
    uiName: "Disk IO",
    caption: "Dsk: ",
    description: Some("Disk IO rate & time combined display"),
    maxItems: 0,
    isMultiColumn: true,
    isPercentChart: false,
};

/// Port of `static void DiskIOMeter_updateValues(Meter* this)` from
/// `DiskIOMeter.c:237`. Dispatches `Meter_updateValues` on each sub-meter held
/// in `this->meterData` (`As_Meter(m)->updateValues(m)` — the mirrored instance
/// `updateValues` slot the ported `Meter` carries).
pub fn DiskIOMeter_updateValues(this: &mut Meter) {
    let data = DiskIOMeterData::of(this);
    let rate_uv = data
        .diskIORateMeter
        .updateValues
        .expect("DiskIOMeter_updateValues: rate sub-meter updateValues");
    rate_uv(&mut data.diskIORateMeter);
    let time_uv = data
        .diskIOTimeMeter
        .updateValues
        .expect("DiskIOMeter_updateValues: time sub-meter updateValues");
    time_uv(&mut data.diskIOTimeMeter);
}

/// Port of `static void DiskIOMeter_draw(Meter* this, int x, int y, int w)`
/// from `DiskIOMeter.c:244`. In `TEXT`/`LED` mode it renders the composite's
/// combined display by calling the rate sub-meter's `draw` fn pointer with
/// `this` (the composite) as the meter — exactly as C does — so
/// `Meter_displayBuffer` dispatches `this->display` (`DiskIOMeter_display`).
/// Otherwise it splits the width in half and draws the rate and time sub-meters
/// side by side (the `w % 2` remainder padding the gap, aligning with the CPU
/// meter). Terminal output goes through `out` (the crossterm sink the ported
/// draw path uses).
pub fn DiskIOMeter_draw(out: &mut dyn Write, this: &mut Meter, x: i32, y: i32, w: i32) {
    if this.mode == TEXT_METERMODE || this.mode == LED_METERMODE {
        let draw = DiskIOMeterData::of(this)
            .diskIORateMeter
            .draw
            .expect("DiskIOMeter_draw: rate sub-meter draw");
        draw(&mut *out, this, x, y, w);
        return;
    }

    // Use the same width for each sub meter to align with CPU meter
    let colwidth = w / 2;
    let diff = w % 2;
    let data = DiskIOMeterData::of(this);
    let rate_draw = data
        .diskIORateMeter
        .draw
        .expect("DiskIOMeter_draw: rate sub-meter draw");
    rate_draw(&mut *out, &mut data.diskIORateMeter, x, y, colwidth);
    let time_draw = data
        .diskIOTimeMeter
        .draw
        .expect("DiskIOMeter_draw: time sub-meter draw");
    time_draw(&mut *out, &mut data.diskIOTimeMeter, x + colwidth + diff, y, colwidth);
}

/// Port of `static void DiskIOMeter_init(Meter* this)` from `DiskIOMeter.c:265`.
/// Allocates the [`DiskIOMeterData`] on first use, constructing the rate and
/// time sub-meters via `Meter_new(this->host, 0, Class(...))`. `Meter_new`
/// already runs each class `init` slot + the default `Meter_setMode`, and
/// neither [`DiskIORateMeter_class`] nor [`DiskIOTimeMeter_class`] defines an
/// `init` slot, so the C `if (Meter_initFn(sub)) Meter_init(sub)` re-init calls
/// are no-ops here.
pub fn DiskIOMeter_init(this: &mut Meter) {
    if this.meterData.is_none() {
        let host = this.host;
        this.meterData = Some(Box::new(DiskIOMeterData {
            diskIORateMeter: Meter_new(host, 0, &DiskIORateMeter_class),
            diskIOTimeMeter: Meter_new(host, 0, &DiskIOTimeMeter_class),
        }));
    }
}

/// Port of `static void DiskIOMeter_updateMode(Meter* this, MeterModeId mode)`
/// from `DiskIOMeter.c:285`. Sets the meter mode, propagates it to both
/// sub-meters via `Meter_setMode`, and takes the container height as the
/// taller of the two (`MAXIMUM`).
pub fn DiskIOMeter_updateMode(this: &mut Meter, mode: MeterModeId) {
    this.mode = mode;
    let data = DiskIOMeterData::of(this);
    Meter_setMode(&mut data.diskIORateMeter, mode);
    Meter_setMode(&mut data.diskIOTimeMeter, mode);
    let h = data.diskIORateMeter.h.max(data.diskIOTimeMeter.h);
    this.h = h;
}

/// Port of `static void DiskIOMeter_done(Meter* this)` from `DiskIOMeter.c:296`.
/// The C deletes both sub-meters and frees the `DiskIOMeterData`; clearing the
/// owned `meterData` slot drops the sub-meters and reclaims all of it.
pub fn DiskIOMeter_done(this: &mut Meter) {
    this.meterData = None;
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

    /// The display fns take an `ATTR_UNUSED` `&Meter` (data comes from the
    /// file-scope statics), so any empty meter suffices as the ignored arg.
    fn dummy() -> Meter {
        Meter::empty()
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
        DiskIORateMeter_display(&dummy(), &mut out);
        assert_eq!(text(&out), "no data");

        set_state(MeterRateStatus::RATESTATUS_INIT, "", "", 0.0, 0);
        let mut out = RichString::new();
        DiskIORateMeter_display(&dummy(), &mut out);
        assert_eq!(text(&out), "initializing...");

        set_state(MeterRateStatus::RATESTATUS_STALE, "", "", 0.0, 0);
        let mut out = RichString::new();
        DiskIORateMeter_display(&dummy(), &mut out);
        assert_eq!(text(&out), "stale data");
    }

    #[test]
    fn rate_display_data_line() {
        let _g = TEST_LOCK.lock().unwrap();

        set_state(MeterRateStatus::RATESTATUS_DATA, "1.23G", "45.6M", 0.0, 0);
        let mut out = RichString::new();
        DiskIORateMeter_display(&dummy(), &mut out);
        assert_eq!(text(&out), "read: 1.23GiB/s write: 45.6MiB/s");
    }

    #[test]
    fn time_display_status_words() {
        let _g = TEST_LOCK.lock().unwrap();

        set_state(MeterRateStatus::RATESTATUS_NODATA, "", "", 0.0, 0);
        let mut out = RichString::new();
        DiskIOTimeMeter_display(&dummy(), &mut out);
        assert_eq!(text(&out), "no data");
    }

    #[test]
    fn time_display_busy_no_disk_suffix() {
        let _g = TEST_LOCK.lock().unwrap();

        // num_disks <= 1 => no suffix.
        set_state(MeterRateStatus::RATESTATUS_DATA, "", "", 12.34, 1);
        let mut out = RichString::new();
        DiskIOTimeMeter_display(&dummy(), &mut out);
        assert_eq!(text(&out), "12.3% busy");
    }

    #[test]
    fn time_display_busy_with_disk_suffix() {
        let _g = TEST_LOCK.lock().unwrap();

        // 1 < num_disks < 1000 => suffix; util > 40 exercises the NOTICE
        // color branch (color isn't asserted here, only the text).
        set_state(MeterRateStatus::RATESTATUS_DATA, "", "", 87.65, 4);
        let mut out = RichString::new();
        DiskIOTimeMeter_display(&dummy(), &mut out);
        assert_eq!(text(&out), "87.7% busy (4 disks)");
    }

    #[test]
    fn time_display_suffix_suppressed_at_1000_disks() {
        let _g = TEST_LOCK.lock().unwrap();

        // num_disks >= 1000 => suffix suppressed (the C `< 1000` guard).
        set_state(MeterRateStatus::RATESTATUS_DATA, "", "", 5.0, 1000);
        let mut out = RichString::new();
        DiskIOTimeMeter_display(&dummy(), &mut out);
        assert_eq!(text(&out), "5.0% busy");
    }

    #[test]
    fn combined_display_data_joins_with_semicolon() {
        let _g = TEST_LOCK.lock().unwrap();

        set_state(MeterRateStatus::RATESTATUS_DATA, "10K", "20K", 33.3, 2);
        let mut out = RichString::new();
        DiskIOMeter_display(&dummy(), &mut out);
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
        DiskIOMeter_display(&dummy(), &mut out);
        assert_eq!(text(&out), "stale data");
    }

    #[test]
    fn rate_meter_update_first_sample_status() {
        use crate::ported::linux::linuxmachine::LinuxMachine;
        use crate::ported::machine::Machine;
        use crate::ported::meter::Meter;
        // First sample (cached_last_update == 0) with a >500ms span forces the
        // update path. On Linux CI Platform_getDiskIO succeeds → "init"; on a
        // host without /proc/diskstats it fails → "no data". Assert the meter
        // produces one of those and populates its value slots without panic.
        let host = Box::leak(Box::new(LinuxMachine {
            super_: Machine {
                realtimeMs: 1000,
                ..Default::default()
            },
            ..Default::default()
        }));
        let mut m = Meter {
            values: vec![0.0; 2],
            host: &host.super_ as *const crate::ported::machine::Machine,
            ..Meter::empty()
        };
        super::DiskIORateMeter_updateValues(&mut m);
        assert!(m.txtBuffer == "init" || m.txtBuffer == "no data" || m.txtBuffer.starts_with("r:"));
        assert!(m.values[0] >= 0.0 && m.values[1] >= 0.0);
    }
}
