//! Partial port of `MemoryMeter.c` — htop's memory meter.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! Both of this module's functions walk the platform-specific memory-class
//! table `Platform_memoryClasses[]` / `Platform_numberOfMemoryClasses`
//! (`linux/Platform.h:50`) — the `MemoryClass` array
//! (`MemoryMeter.h:13`: `{ label, countsAsUsed, countsAsCache, color }`)
//! that names and categorizes each per-platform memory bucket. That table is
//! not ported anywhere in the crate (the `Platform` layer is stubbed —
//! `Platform_setMemoryValues` is a `todo!()` in `linux/platform.rs`), and
//! there is no `MemoryClass` type in the tree. The bodies also read
//! `this->host->settings->showCachedMemory`, but the partial `Meter` in
//! `meter.rs` models no `host` back-pointer (`host` is listed there as
//! substrate the ported renderers do not touch) and `Settings` carries no
//! `showCachedMemory` field. With neither the memory-class table nor the
//! `host`/`showCachedMemory` reachable, neither function has a faithful data
//! source to reproduce, so both remain honest `todo!()` stubs. (The pieces
//! that DO exist — `Meter_humanUnit`, `RichString_writeAscii` /
//! `RichString_appendAscii`, `CRT_colors` — are not enough on their own.)
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `static void MemoryMeter_updateValues(Meter* this)` from
/// `MemoryMeter.c:33`. Blocked: the values are filled by
/// `Platform_setMemoryValues(this)` (a `todo!()` in `linux/platform.rs`) and
/// the loop iterates `Platform_numberOfMemoryClasses` / reads
/// `Platform_memoryClasses[i].countsAsUsed` and `.countsAsCache`
/// (`linux/Platform.h:50`) — the `MemoryClass` table (`MemoryMeter.h:13`) is
/// not ported. The `used`-clearing branch further reads
/// `this->host->settings->showCachedMemory`, but the partial `Meter` in
/// `meter.rs` carries no `host` back-pointer and `Settings` has no
/// `showCachedMemory` field.
pub fn MemoryMeter_updateValues() {
    todo!("port of MemoryMeter.c:33: needs Platform_memoryClasses/Platform_numberOfMemoryClasses + Platform_setMemoryValues + Meter.host->settings->showCachedMemory")
}

/// TODO: port of `static void MemoryMeter_display(const Object* cast,
/// RichString* out)` from `MemoryMeter.c:73`. Blocked for the same reason as
/// [`MemoryMeter_updateValues`]: the per-class loop iterates
/// `Platform_numberOfMemoryClasses` and reads each
/// `Platform_memoryClasses[i]`'s `label`, `color`, and `countsAsCache`
/// (`linux/Platform.h:50`, `MemoryMeter.h:13`) — the `MemoryClass` table is
/// not ported — and it also reads `this->host->settings->showCachedMemory`,
/// which the partial `Meter` in `meter.rs` (no `host`) and `Settings` (no
/// `showCachedMemory`) do not model. (`Meter_humanUnit`,
/// `RichString_writeAscii`/`RichString_appendAscii`, and `CRT_colors` are all
/// available; the memory-class table and `host`/`showCachedMemory` are what
/// remain.)
pub fn MemoryMeter_display() {
    todo!("port of MemoryMeter.c:73: needs Platform_memoryClasses/Platform_numberOfMemoryClasses + Meter.host->settings->showCachedMemory")
}
