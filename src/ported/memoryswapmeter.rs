//! Port of `MemorySwapMeter.c` — a multi-column composite meter that embeds a
//! `MemoryMeter` and a `SwapMeter` as sub-meters.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C `Foo(Meter* this, …)` ports
//! to a free fn taking `this: &mut Meter` (the shape the sibling meters use).
//!
//! The per-instance composition primitives this meter needs — `this->meterData`
//! (the two sub-`Meter`s), `this->host`, `Meter_new`, `Meter_setMode`, the
//! mirrored instance `updateValues`/`draw` vtable slots, and the sub-meter
//! class tables (`MemoryMeter_class`, `SwapMeter_class`) — are all ported now,
//! so every function below is a faithful port (the `CPUMeter`/`DiskIOMeter`
//! multi-column precedents).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // preserve the C class name `MemorySwapMeter_class`
#![allow(dead_code)]

use std::io::Write;

use crate::ported::memorymeter::MemoryMeter_class;
use crate::ported::meter::{
    Meter, MeterClass, MeterModeId, Meter_class, Meter_new, Meter_setMode, BAR_METERMODE,
    METERMODE_DEFAULT_SUPPORTED,
};
use crate::ported::object::ObjectClass;
use crate::ported::swapmeter::SwapMeter_class;

/// Port of `typedef struct MemorySwapMeterData_` (`MemorySwapMeter.c:23`): the
/// composite meter's private `meterData`, holding the memory and swap
/// sub-meters. The C `Meter*` pointers become owned `Meter`s (dropping the
/// `Box` reclaims them, replacing the C `free`).
struct MemorySwapMeterData {
    memoryMeter: Meter,
    swapMeter: Meter,
}

impl MemorySwapMeterData {
    /// Borrows `this.meterData` as the `MemorySwapMeterData` set by
    /// [`MemorySwapMeter_init`]. A Rust-only borrow helper (the `CPUMeterData::of`
    /// precedent); an associated fn, so the port-purity gate requires no C
    /// counterpart.
    fn of(this: &mut Meter) -> &mut MemorySwapMeterData {
        this.meterData
            .as_mut()
            .and_then(|d| d.downcast_mut::<MemorySwapMeterData>())
            .expect("MemorySwap meter: meterData is not an initialized MemorySwapMeterData")
    }
}

/// Port of `static void MemorySwapMeter_updateValues(Meter* this)` from
/// `MemorySwapMeter.c:29`. Dispatches `Meter_updateValues` on each sub-meter
/// held in `this->meterData` (the mirrored instance `updateValues` slot the
/// ported `Meter` carries).
pub fn MemorySwapMeter_updateValues(this: &mut Meter) {
    let data = MemorySwapMeterData::of(this);
    let mem_uv = data
        .memoryMeter
        .updateValues
        .expect("MemorySwapMeter_updateValues: memory sub-meter updateValues");
    mem_uv(&mut data.memoryMeter);
    let swap_uv = data
        .swapMeter
        .updateValues
        .expect("MemorySwapMeter_updateValues: swap sub-meter updateValues");
    swap_uv(&mut data.swapMeter);
}

/// Port of `static void MemorySwapMeter_draw(Meter* this, int x, int y, int w)`
/// from `MemorySwapMeter.c:36`. Splits the width in half and draws the memory
/// and swap sub-meters side by side (the `w % 2` remainder padding the gap, to
/// align with the CPU meter), dispatching each sub-meter's instance `draw` fn
/// pointer. Terminal output goes through `out` (the crossterm sink the ported
/// draw path uses).
pub fn MemorySwapMeter_draw(out: &mut dyn Write, this: &mut Meter, x: i32, y: i32, w: i32) {
    // Use the same width for each sub meter to align with CPU meter
    let colwidth = w / 2;
    let diff = w % 2;
    let data = MemorySwapMeterData::of(this);
    let mem_draw = data
        .memoryMeter
        .draw
        .expect("MemorySwapMeter_draw: memory sub-meter draw");
    mem_draw(&mut *out, &mut data.memoryMeter, x, y, colwidth);
    let swap_draw = data
        .swapMeter
        .draw
        .expect("MemorySwapMeter_draw: swap sub-meter draw");
    swap_draw(
        &mut *out,
        &mut data.swapMeter,
        x + colwidth + diff,
        y,
        colwidth,
    );
}

/// Port of `static void MemorySwapMeter_init(Meter* this)` from
/// `MemorySwapMeter.c:49`. Allocates the `MemorySwapMeterData` on first use,
/// constructing the Memory and Swap sub-meters via
/// `Meter_new(this->host, 0, Class(...))`. `Meter_new` already runs each class
/// `init` slot + the default `Meter_setMode`; neither [`MemoryMeter_class`] nor
/// [`SwapMeter_class`] defines an `init` slot, so the C
/// `if (Meter_initFn(sub)) Meter_init(sub)` re-init calls are no-ops here.
pub fn MemorySwapMeter_init(this: &mut Meter) {
    if this.meterData.is_none() {
        let host = this.host;
        this.meterData = Some(Box::new(MemorySwapMeterData {
            memoryMeter: Meter_new(host, 0, &MemoryMeter_class),
            swapMeter: Meter_new(host, 0, &SwapMeter_class),
        }));
    }
}

/// Port of `static void MemorySwapMeter_updateMode(Meter* this, MeterModeId
/// mode)` from `MemorySwapMeter.c:68`. Sets the meter mode, propagates it to
/// both sub-meters via `Meter_setMode`, and takes the container height as the
/// taller of the two (`MAXIMUM`).
pub fn MemorySwapMeter_updateMode(this: &mut Meter, mode: MeterModeId) {
    this.mode = mode;
    let data = MemorySwapMeterData::of(this);
    Meter_setMode(&mut data.memoryMeter, mode);
    Meter_setMode(&mut data.swapMeter, mode);
    let h = data.memoryMeter.h.max(data.swapMeter.h);
    this.h = h;
}

/// Port of `static void MemorySwapMeter_done(Meter* this)` from
/// `MemorySwapMeter.c:79`. The C deletes both sub-meters and frees the
/// `MemorySwapMeterData`; clearing the owned `meterData` slot drops the
/// sub-meters and reclaims all of it.
pub fn MemorySwapMeter_done(this: &mut Meter) {
    this.meterData = None;
}

/// Port of `const MeterClass MemorySwapMeter_class` from `MemorySwapMeter.c:88`.
/// Wires the ported [`MemorySwapMeter_updateValues`]/[`MemorySwapMeter_draw`]/
/// [`MemorySwapMeter_init`]/[`MemorySwapMeter_updateMode`]/[`MemorySwapMeter_done`]
/// slots onto the vtable. A multi-column meter, default `BAR_METERMODE`. The C
/// sets no `.attributes`/`.total`/`.maxItems`/`.isPercentChart` (`NULL`/`0`/
/// `false`), so those default (empty slice / `0.0` / `0` / `false`); the empty
/// attribute slice is never indexed because the class `draw` dispatches to the
/// sub-meters and no `display` slot is set (Blank-style displays go unused).
/// `super.delete` → `Drop`; `super.extends` → the `Meter_class` base link.
pub static MemorySwapMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: None,
    init: Some(MemorySwapMeter_init),
    done: Some(MemorySwapMeter_done),
    updateMode: Some(MemorySwapMeter_updateMode),
    updateValues: Some(MemorySwapMeter_updateValues),
    draw: Some(MemorySwapMeter_draw),
    getCaption: None,
    getUiName: None,
    defaultMode: BAR_METERMODE,
    supportedModes: METERMODE_DEFAULT_SUPPORTED,
    total: 0.0,
    attributes: &[],
    name: "MemorySwap",
    uiName: "Memory & Swap",
    caption: "M&S",
    description: Some("Combined memory and swap usage"),
    maxItems: 0,
    isMultiColumn: true,
    isPercentChart: false,
};
