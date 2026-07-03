//! Port of `MemoryMeter.c` — htop's memory meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! Both functions walk the platform memory-class table
//! [`Platform_memoryClasses`](crate::ported::linux::platform::Platform_memoryClasses)
//! / [`Platform_numberOfMemoryClasses`](crate::ported::linux::platform::Platform_numberOfMemoryClasses)
//! and read `this->host->settings->showCachedMemory`. `this->host` is the
//! concrete [`LinuxMachine`](crate::ported::linux::linuxmachine::LinuxMachine);
//! its `super_` is the generic `Machine` carrying `settings`. The per-class
//! figures are filled by the ported
//! [`Platform_setMemoryValues`](crate::ported::linux::platform::Platform_setMemoryValues).
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::linux::platform::{
    Platform_memoryClasses, Platform_numberOfMemoryClasses, Platform_setMemoryValues,
};
use crate::ported::meter::{Meter, Meter_humanUnit, BAR_METERMODE, GRAPH_METERMODE};
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_writeAscii};

/// Port of `static void MemoryMeter_updateValues(Meter* this)` from
/// `MemoryMeter.c:33`. Seeds every class with `NAN` (not all classes exist
/// on every platform), fills the real figures via [`Platform_setMemoryValues`],
/// sums the `countsAsUsed` classes into `used`, masks (to `NAN`) the cache
/// classes when `showCachedMemory` is off and any class that is neither
/// used nor cache — but only in graph/bar mode — and formats `txtBuffer` as
/// `used/total`.
pub fn MemoryMeter_updateValues(this: &mut Meter) {
    // C: `Settings *settings = this->host->settings;` (dereferenced
    // unconditionally). `showCachedMemory` is a `Copy` bool, so the borrow
    // is released before `Platform_setMemoryValues` re-borrows the host.
    let show_cached_memory = {
        let host = this
            .host
            .as_ref()
            .expect("MemoryMeter_updateValues: this->host (C dereferences it)")
            .clone();
        let sc = host
            .borrow()
            .super_
            .settings
            .as_ref()
            .expect("MemoryMeter_updateValues: host->settings")
            .showCachedMemory;
        sc
    };

    // not all memory classes are supported on all platforms
    for i in 0..Platform_numberOfMemoryClasses {
        this.values[i] = f64::NAN;
    }

    Platform_setMemoryValues(this);
    this.curItems = Platform_numberOfMemoryClasses as u8;

    // compute the used memory
    let mut used = 0.0;
    for i in 0..Platform_numberOfMemoryClasses {
        if Platform_memoryClasses[i].countsAsUsed {
            used += this.values[i];
        }
    }

    // clear the values we don't want to see
    if this.mode == GRAPH_METERMODE || this.mode == BAR_METERMODE {
        for i in 0..Platform_numberOfMemoryClasses {
            let mc = &Platform_memoryClasses[i];
            if (mc.countsAsCache && !show_cached_memory)
                || !(mc.countsAsCache || mc.countsAsUsed)
            {
                this.values[i] = f64::NAN;
            }
        }
    }

    this.txtBuffer = format!("{}/{}", Meter_humanUnit(used), Meter_humanUnit(this.total));
}

/// Port of `static void MemoryMeter_display(const Object* cast, RichString*
/// out)` from `MemoryMeter.c:73`. Writes `:<total>` then, for each memory
/// class in platform order, ` <label>:<value>` — coloring the label
/// `METER_TEXT` and the value with the class's own `CRT_colors` entry, or
/// both `METER_SHADOW` when the class is a cache class and `showCachedMemory`
/// is off.
pub fn MemoryMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();
    let show_cached_memory = this
        .host
        .as_ref()
        .expect("MemoryMeter_display: this->host (C dereferences it)")
        .borrow()
        .super_
        .settings
        .as_ref()
        .expect("MemoryMeter_display: host->settings")
        .showCachedMemory;

    RichString_writeAscii(out, ColorElements::METER_TEXT.packed(scheme), b":");
    let buffer = Meter_humanUnit(this.total);
    RichString_appendAscii(out, ColorElements::METER_VALUE.packed(scheme), buffer.as_bytes());

    // print the memory classes in the order supplied (specific to each platform)
    for i in 0..Platform_numberOfMemoryClasses {
        let mc = &Platform_memoryClasses[i];
        let (label_color, value_color) = if !show_cached_memory && mc.countsAsCache {
            let shadow = ColorElements::METER_SHADOW.packed(scheme);
            (shadow, shadow)
        } else {
            (ColorElements::METER_TEXT.packed(scheme), mc.color.packed(scheme))
        };

        let buffer = Meter_humanUnit(this.values[i]);
        RichString_appendAscii(out, label_color, b" ");
        RichString_appendAscii(out, label_color, mc.label.as_bytes());
        RichString_appendAscii(out, label_color, b":");
        RichString_appendAscii(out, value_color, buffer.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::linux::linuxmachine::LinuxMachine;
    use crate::ported::machine::{Machine, Settings};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn hosted(show_cached: bool, mode: crate::ported::meter::MeterModeId) -> Meter {
        let host = Rc::new(RefCell::new(LinuxMachine {
            super_: Machine {
                totalMem: 8192,
                settings: Some(Settings {
                    showCachedMemory: show_cached,
                    ..Default::default()
                }),
                ..Default::default()
            },
            usedMem: 2048,
            sharedMem: 128,
            buffersMem: 256,
            cachedMem: 1024,
            availableMem: 4096,
            ..Default::default()
        }));
        Meter {
            values: vec![0.0; Platform_numberOfMemoryClasses],
            mode,
            host: Some(host),
            ..Meter::empty()
        }
    }

    #[test]
    fn update_values_sums_used_and_formats() {
        use crate::ported::meter::TEXT_METERMODE;
        // TEXT mode (not graph/bar) → no masking. used = used+shared+compressed.
        let mut m = hosted(true, TEXT_METERMODE);
        MemoryMeter_updateValues(&mut m);
        assert_eq!(m.curItems as usize, Platform_numberOfMemoryClasses);
        // used(2048) + shared(128) + compressed(0) = 2176 KiB → 2.125 MiB,
        // "{:.2}" round-half-to-even → 2.12M; total 8192 KiB → 8.00M.
        assert_eq!(m.txtBuffer, "2.12M/8.00M");
        // no masking in text mode: cache slot keeps its value.
        assert_eq!(m.values[4], 1024.0);
    }

    #[test]
    fn bar_mode_masks_cache_when_hidden() {
        // BAR mode + showCachedMemory=false → buffers/cache masked to NaN.
        let mut m = hosted(false, BAR_METERMODE);
        MemoryMeter_updateValues(&mut m);
        assert!(m.values[3].is_nan(), "buffers masked");
        assert!(m.values[4].is_nan(), "cache masked");
        assert_eq!(m.values[0], 2048.0, "used kept");
    }
}
