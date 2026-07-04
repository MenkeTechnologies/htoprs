//! Port of `zfs/ZfsArcMeter.c` — htop's ZFS ARC meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! [`ZfsArcMeter_readStats`] copies a [`ZfsArcStats`] snapshot into the
//! generic `Meter` value slots; the platform setter
//! [`Platform_setZfsArcValues`] bridges the concrete host machine's `zfs`
//! field to it (darwin-first dispatch, mirroring `MemoryMeter`).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // faithful C global names (ZfsArcMeter_class)
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::linux::linuxmachine::ZfsArcStats;
// Platform dispatch (darwin-first): the ZFS value setter comes from this
// build's platform.
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_setZfsArcValues;
#[cfg(not(target_os = "macos"))]
use crate::ported::linux::platform::Platform_setZfsArcValues;
use crate::ported::meter::{
    Meter, MeterClass, Meter_class, Meter_humanUnit, METERMODE_DEFAULT_SUPPORTED, TEXT_METERMODE,
};
use crate::ported::object::ObjectClass;
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_writeAscii};

/// Port of `static const int ZfsArcMeter_attributes[]` from `ZfsArcMeter.c:22`:
/// `{ ZFS_MFU, ZFS_MRU, ZFS_ANON, ZFS_HEADER, ZFS_OTHER }`.
static ZfsArcMeter_attributes: [i32; 5] = [
    ColorElements::ZFS_MFU as i32,
    ColorElements::ZFS_MRU as i32,
    ColorElements::ZFS_ANON as i32,
    ColorElements::ZFS_HEADER as i32,
    ColorElements::ZFS_OTHER as i32,
];

/// Port of `void ZfsArcMeter_readStats(Meter* this, const ZfsArcStats* stats)`
/// from `ZfsArcMeter.c:26`. Copies the ARC breakdown into the meter's value
/// slots; `values[5]` (`size`) is "hidden" past `curItems = 5` so it is only
/// reachable by index — the Bar/Graph styles never draw it.
pub fn ZfsArcMeter_readStats(this: &mut Meter, stats: &ZfsArcStats) {
    this.total = stats.max as f64;
    this.values[0] = stats.MFU as f64;
    this.values[1] = stats.MRU as f64;
    this.values[2] = stats.anon as f64;
    this.values[3] = stats.header as f64;
    this.values[4] = stats.other as f64;

    // "Hide" the last value so it can
    // only be accessed by index and is not
    // displayed by the Bar or Graph style
    this.curItems = 5;
    this.values[5] = stats.size as f64;
}

/// Port of `static void ZfsArcMeter_updateValues(Meter* this)` from
/// `ZfsArcMeter.c:41`. Fills the slots via [`Platform_setZfsArcValues`] then
/// formats `txtBuffer` as `used/total` (`values[5]` = ARC size over max).
pub fn ZfsArcMeter_updateValues(this: &mut Meter) {
    Platform_setZfsArcValues(this);

    this.txtBuffer = format!(
        "{}/{}",
        Meter_humanUnit(this.values[5]),
        Meter_humanUnit(this.total)
    );
}

/// Port of `static void ZfsArcMeter_display(const Object* cast, RichString*
/// out)` from `ZfsArcMeter.c:55`. When the ARC is populated (`values[5] > 0`)
/// writes `<total> Used:<size> MFU:.. MRU:.. Anon:.. Hdr:.. Oth:..`, coloring
/// each figure with its `CRT_colors` entry; otherwise " Unavailable".
pub fn ZfsArcMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();

    if this.values[5] > 0.0 {
        let buffer = Meter_humanUnit(this.total);
        RichString_appendAscii(
            out,
            ColorElements::METER_VALUE.packed(scheme),
            buffer.as_bytes(),
        );
        let buffer = Meter_humanUnit(this.values[5]);
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" Used:");
        RichString_appendAscii(
            out,
            ColorElements::METER_VALUE.packed(scheme),
            buffer.as_bytes(),
        );
        let buffer = Meter_humanUnit(this.values[0]);
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" MFU:");
        RichString_appendAscii(out, ColorElements::ZFS_MFU.packed(scheme), buffer.as_bytes());
        let buffer = Meter_humanUnit(this.values[1]);
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" MRU:");
        RichString_appendAscii(out, ColorElements::ZFS_MRU.packed(scheme), buffer.as_bytes());
        let buffer = Meter_humanUnit(this.values[2]);
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" Anon:");
        RichString_appendAscii(out, ColorElements::ZFS_ANON.packed(scheme), buffer.as_bytes());
        let buffer = Meter_humanUnit(this.values[3]);
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" Hdr:");
        RichString_appendAscii(
            out,
            ColorElements::ZFS_HEADER.packed(scheme),
            buffer.as_bytes(),
        );
        let buffer = Meter_humanUnit(this.values[4]);
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" Oth:");
        RichString_appendAscii(
            out,
            ColorElements::ZFS_OTHER.packed(scheme),
            buffer.as_bytes(),
        );
    } else {
        RichString_writeAscii(out, ColorElements::METER_TEXT.packed(scheme), b" ");
        RichString_appendAscii(
            out,
            ColorElements::FAILED_READ.packed(scheme),
            b"Unavailable",
        );
    }
}

/// Port of `const MeterClass ZfsArcMeter_class` from `ZfsArcMeter.c:86`. A
/// percent chart (`total = 100.0`), default `TEXT_METERMODE`, `maxItems = 6`
/// (five drawn classes plus the hidden `size` slot). `super.delete` is dropped
/// (Rust `Drop`); `super.extends` becomes the `Meter_class` base link.
pub static ZfsArcMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: Some(ZfsArcMeter_display),
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(ZfsArcMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 100.0,
    attributes: &ZfsArcMeter_attributes,
    name: "ZFSARC",
    uiName: "ZFS ARC",
    caption: "ARC: ",
    description: None,
    maxItems: 6,
    isMultiColumn: false,
    isPercentChart: true,
};
