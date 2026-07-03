//! Partial port of `linux/HugePageMeter.c` — htop's Linux huge-page usage
//! meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. The C `static void
//! HugePageMeter_display(const Object* cast, RichString* out)` casts `cast`
//! back to `const Meter*` and reads `this->total` / `this->values[...]`, so
//! it ports to a free fn `pub fn HugePageMeter_display(this: &Meter, out:
//! &mut RichString)` — the `cast` → `this` down-cast collapses into the typed
//! `&Meter` parameter (the same mapping `filedescriptormeter.rs` uses). The
//! shared `Meter` model is [`crate::ported::meter::Meter`].
//!
//! `CRT_colors[X]` (C's active-scheme row `const int* CRT_colors`, set by
//! `CRT_setColors` to point at `CRT_colorSchemes[colorScheme]`) is reproduced
//! as `ColorElements::X.packed(ColorScheme::active())`; the per-item color
//! `CRT_colors[HUGEPAGE_1 + i]` is the raw scheme-row index
//! `CRT_colorSchemes[scheme][HUGEPAGE_1 as usize + i]` (the enum is
//! `#[repr(usize)]` with the C discriminants, so `HUGEPAGE_1 + i` addresses
//! `HUGEPAGE_1..=HUGEPAGE_4`, matching the class `HugePageMeter_attributes`).
//! `Meter_humanUnit(buffer, v, sizeof(buffer))` becomes the owned-`String`
//! [`Meter_humanUnit`] port.
//!
//! The file-scope `HugePageMeter_active_labels[4]` is a mutable C static
//! written by `HugePageMeter_updateValues` and read by
//! `HugePageMeter_display`, so it ports to a `Mutex<[Option<&'static str>;
//! 4]>` (the idiom for a global mutable C static that is neither a plain
//! counter nor a flag). `HugePageMeter_labels` is the immutable label table
//! `updateValues` selects from (retained for the eventual `updateValues`
//! port).
//!
//! Ported:
//! - [`HugePageMeter_updateValues`] (`HugePageMeter.c:39`) — downcasts
//!   `this->host` to [`LinuxMachine`] and drives `this->total` /
//!   `this->values[]` / the active-labels table from `totalHugePageMem` and
//!   `usedHugePageMem[]`.
//! - [`HugePageMeter_display`] (`HugePageMeter.c:76`) — writes `:<total>`
//!   then, for each active label, `<label><value>`, coloring the total
//!   `METER_VALUE`, each label `METER_TEXT`, and each value `HUGEPAGE_1 + i`.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::sync::Mutex;

use crate::ported::crt::{CRT_colorSchemes, ColorElements, ColorScheme};
use crate::ported::linux::linuxmachine::{memory_t, LinuxMachine, HTOP_HUGEPAGE_COUNT, MEMORY_MAX};
use crate::ported::meter::{Meter, Meter_humanUnit};
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_writeAscii};

/// Port of `static const char* HugePageMeter_active_labels[4]` from
/// `HugePageMeter.c:24`. A mutable file-scope C static (populated by
/// `HugePageMeter_updateValues`, read by [`HugePageMeter_display`]); modeled
/// as a `Mutex` guarding the four-slot table of `Option<&'static str>` labels
/// (`NULL` ⇒ `None`).
#[allow(non_upper_case_globals)] // keep the exact C static name (port convention)
static HugePageMeter_active_labels: Mutex<[Option<&'static str>; 4]> =
    Mutex::new([None, None, None, None]);

/// Port of `static const char* const HugePageMeter_labels[]` from
/// `HugePageMeter.c:33` — the `HTOP_HUGEPAGE_COUNT` label strings
/// `HugePageMeter_updateValues` selects the active ones from.
#[allow(non_upper_case_globals)] // keep the exact C static name (port convention)
static HugePageMeter_labels: [&str; 24] = [
    " 64K:", " 128K:", " 256K:", " 512K:", " 1M:", " 2M:", " 4M:", " 8M:", " 16M:", " 32M:",
    " 64M:", " 128M:", " 256M:", " 512M:", " 1G:", " 2G:", " 4G:", " 8G:", " 16G:", " 32G:",
    " 64G:", " 128G:", " 256G:", " 512G:",
];

/// Port of `static void HugePageMeter_updateValues(Meter* this)` from
/// `HugePageMeter.c:39`. Downcasts `this->host` to the [`LinuxMachine`]
/// (the C `(const LinuxMachine*) this->host`, the same downcast
/// `Platform_setZramValues` uses), sets `this->total` from
/// `host->totalHugePageMem`, resets the four value slots (index 0 to `0`, the
/// rest to `NAN`) and the four active labels (` used:` then `NULL`), then
/// walks all [`HTOP_HUGEPAGE_COUNT`] page sizes: every set entry
/// (`!= MEMORY_MAX`) fills the next value slot, accumulates `usedTotal`, and
/// records its label, stopping once all four slots are used.
///
/// The C's `Meter_humanUnit(buffer, usedTotal, size)` +
/// `METER_BUFFER_APPEND_CHR('/')` + `Meter_humanUnit(buffer, total, size)`
/// sequence writes `"<usedTotal>/<total>"` into `this->txtBuffer`; the
/// truncation guards (`METER_BUFFER_CHECK`) only cap an over-long write that
/// two human-unit strings never produce, so the net result is the `format!`
/// concatenation. The mutable file-scope `HugePageMeter_active_labels` static
/// is the [`Mutex`]-guarded table.
pub fn HugePageMeter_updateValues(this: &mut Meter) {
    let host = unsafe { &*(this.host as *const LinuxMachine) };

    let mut usedTotal: memory_t = 0;
    let mut nextUsed: usize = 0;

    this.total = host.totalHugePageMem as f64;
    this.values[0] = 0.0;

    let mut labels = HugePageMeter_active_labels.lock().unwrap();
    labels[0] = Some(" used:");
    for i in 1..labels.len() {
        this.values[i] = f64::NAN;
        labels[i] = None;
    }

    for i in 0..HTOP_HUGEPAGE_COUNT {
        let value = host.usedHugePageMem[i];
        if value != MEMORY_MAX {
            this.values[nextUsed] = value as f64;
            usedTotal += value;
            labels[nextUsed] = Some(HugePageMeter_labels[i]);
            nextUsed += 1;
            if nextUsed == labels.len() {
                break;
            }
        }
    }
    drop(labels);

    this.txtBuffer = format!(
        "{}/{}",
        Meter_humanUnit(usedTotal as f64),
        Meter_humanUnit(this.total)
    );
}

/// Port of `static void HugePageMeter_display(const Object* cast, RichString*
/// out)` from `HugePageMeter.c:76`. Writes `:` then the human-readable
/// `this->total` (colored `METER_VALUE`), then walks the active-labels table:
/// for each non-`NULL` label it appends the label (`METER_TEXT`) followed by
/// the human-readable `this->values[i]` colored `CRT_colors[HUGEPAGE_1 + i]`,
/// stopping at the first empty slot. `CRT_colors[X]` is
/// `ColorElements::X.packed(scheme)`; the active scheme is read once (a
/// process-global that does not change mid-call), matching the C global
/// `CRT_colors`.
pub fn HugePageMeter_display(this: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();

    RichString_writeAscii(out, ColorElements::METER_TEXT.packed(scheme), b":");
    let buffer = Meter_humanUnit(this.total);
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE.packed(scheme),
        buffer.as_bytes(),
    );

    let labels = HugePageMeter_active_labels.lock().unwrap();
    for i in 0..labels.len() {
        let label = match labels[i] {
            Some(label) => label,
            None => break,
        };
        RichString_appendAscii(
            out,
            ColorElements::METER_TEXT.packed(scheme),
            label.as_bytes(),
        );
        let buffer = Meter_humanUnit(this.values[i]);
        RichString_appendAscii(
            out,
            CRT_colorSchemes[scheme as usize][ColorElements::HUGEPAGE_1 as usize + i],
            buffer.as_bytes(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Visible characters of the valid `[0, chlen)` range of `out`.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    /// Exercises both the total prefix and the active-label loop (including
    /// the `NULL`-slot break). The active-labels table is a shared file-scope
    /// static, so this is the only test that touches it (kept single to avoid
    /// cross-test races) and it resets the table afterwards.
    #[test]
    fn display_writes_total_then_active_labels() {
        {
            let mut labels = HugePageMeter_active_labels.lock().unwrap();
            *labels = [Some(" used:"), Some(" 2M:"), None, None];
        }

        let m = Meter {
            host: core::ptr::null(),
            total: 2.0 * 1024.0 * 1024.0, // KiB → "2.00G"
            values: vec![1024.0, 512.0, 0.0, 0.0],
            ..Meter::empty()
        };
        let mut out = RichString::new();
        HugePageMeter_display(&m, &mut out);
        // ":" + total + " used:" + values[0] + " 2M:" + values[1]; the third
        // (NULL) label breaks the loop.
        assert_eq!(text(&out), ":2.00G used:1.00M 2M:512K");

        // Reset the shared static for isolation.
        *HugePageMeter_active_labels.lock().unwrap() = [None, None, None, None];
    }
}
