//! Port of `zfs/ZfsCompressedArcMeter.c` — htop's ZFS compressed-ARC meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! [`ZfsCompressedArcMeter_readStats`] copies a [`ZfsArcStats`] snapshot into
//! the generic `Meter` value slots; the platform setter
//! [`Platform_setZfsCompressedArcValues`] bridges the concrete host machine's
//! `zfs` field to it (darwin-first dispatch, mirroring `MemoryMeter`).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // faithful C global names (ZfsCompressedArcMeter_class)
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::linux::linuxmachine::ZfsArcStats;
// Platform dispatch (darwin-first): the ZFS value setter comes from this
// build's platform.
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_setZfsCompressedArcValues;
#[cfg(not(target_os = "macos"))]
use crate::ported::linux::platform::Platform_setZfsCompressedArcValues;
use crate::ported::meter::{
    Meter, MeterClass, Meter_class, Meter_humanUnit, METERMODE_DEFAULT_SUPPORTED, TEXT_METERMODE,
};
use crate::ported::object::ObjectClass;
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendnAscii, RichString_writeAscii,
};

/// Port of `static const int ZfsCompressedArcMeter_attributes[]` from
/// `ZfsCompressedArcMeter.c:23`: `{ ZFS_COMPRESSED }`.
static ZfsCompressedArcMeter_attributes: [i32; 1] = [ColorElements::ZFS_COMPRESSED as i32];

/// Port of `void ZfsCompressedArcMeter_readStats(Meter* this, const
/// ZfsArcStats* stats)` from `ZfsCompressedArcMeter.c:27`. When the ARC is
/// compressed, `total` is the uncompressed size and `values[0]` the compressed
/// size; otherwise a 1:1 ratio is reported (both set to `size`).
pub fn ZfsCompressedArcMeter_readStats(this: &mut Meter, stats: &ZfsArcStats) {
    if stats.isCompressed != 0 {
        this.total = stats.uncompressed as f64;
        this.values[0] = stats.compressed as f64;
    } else {
        // For uncompressed ARC, report 1:1 ratio
        this.total = stats.size as f64;
        this.values[0] = stats.size as f64;
    }
}

/// Port of `static int ZfsCompressedArcMeter_printRatioString(const Meter*
/// this, char* buffer, size_t size)` from `ZfsCompressedArcMeter.c:38`. Formats
/// `<ratio>:1` (`total / values[0]`) or `N/A` when there is no compressed data.
/// The C returns the written length; the Rust analog returns the formatted
/// `String` (its byte length is the equivalent count).
pub fn ZfsCompressedArcMeter_printRatioString(this: &Meter) -> String {
    if this.values[0] > 0.0 {
        return format!("{:.2}:1", this.total / this.values[0]);
    }

    "N/A".to_string()
}

/// Port of `static void ZfsCompressedArcMeter_updateValues(Meter* this)` from
/// `ZfsCompressedArcMeter.c:46`. Fills the slots via
/// [`Platform_setZfsCompressedArcValues`] then writes the ratio string to
/// `txtBuffer`.
pub fn ZfsCompressedArcMeter_updateValues(this: &mut Meter) {
    Platform_setZfsCompressedArcValues(this);

    this.txtBuffer = ZfsCompressedArcMeter_printRatioString(this);
}

/// Port of `static void ZfsCompressedArcMeter_display(const Object* cast,
/// RichString* out)` from `ZfsCompressedArcMeter.c:52`. When populated
/// (`values[0] > 0`) writes `<uncompressed> Uncompressed, <compressed>
/// Compressed, <ratio>:1 Ratio`; otherwise " Compression Unavailable".
pub fn ZfsCompressedArcMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();

    if this.values[0] > 0.0 {
        let buffer = Meter_humanUnit(this.total);
        RichString_appendAscii(
            out,
            ColorElements::METER_VALUE.packed(scheme),
            buffer.as_bytes(),
        );
        RichString_appendAscii(
            out,
            ColorElements::METER_TEXT.packed(scheme),
            b" Uncompressed, ",
        );
        let buffer = Meter_humanUnit(this.values[0]);
        RichString_appendAscii(
            out,
            ColorElements::METER_VALUE.packed(scheme),
            buffer.as_bytes(),
        );
        RichString_appendAscii(
            out,
            ColorElements::METER_TEXT.packed(scheme),
            b" Compressed, ",
        );
        let buffer = ZfsCompressedArcMeter_printRatioString(this);
        let len = buffer.len();
        RichString_appendnAscii(
            out,
            ColorElements::ZFS_RATIO.packed(scheme),
            buffer.as_bytes(),
            len,
        );
        RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" Ratio");
    } else {
        RichString_writeAscii(out, ColorElements::METER_TEXT.packed(scheme), b" ");
        RichString_appendAscii(
            out,
            ColorElements::FAILED_READ.packed(scheme),
            b"Compression Unavailable",
        );
    }
}

/// Port of `const MeterClass ZfsCompressedArcMeter_class` from
/// `ZfsCompressedArcMeter.c:74`. A percent chart (`total = 100.0`), default
/// `TEXT_METERMODE`, `maxItems = 1`. `super.delete` is dropped (Rust `Drop`);
/// `super.extends` becomes the `Meter_class` base link.
pub static ZfsCompressedArcMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: Some(ZfsCompressedArcMeter_display),
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(ZfsCompressedArcMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 100.0,
    attributes: &ZfsCompressedArcMeter_attributes,
    name: "ZFSCARC",
    uiName: "ZFS CARC",
    caption: "ARC: ",
    description: Some("ZFS CARC: Compressed ARC statistics"),
    maxItems: 1,
    isMultiColumn: false,
    isPercentChart: true,
};
