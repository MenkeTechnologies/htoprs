//! Port scaffold for `AvailableMetersPanel.c` — htop's "Available meters"
//! chooser (the left column of the Meters setup screen: pick a meter class
//! and add it to a header column with F5/F6/Enter).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Ported
//!
//! None. This panel is a leaf of the meter/header UI stack, and every one
//! of its functions bottoms out in substrate that is not yet ported. There
//! is no pure-logic slice (as there was in e.g. `Panel.c`'s scroll math or
//! `ListItem.c`'s compare) that can be faithfully translated in isolation
//! without inventing the missing types — so, per the port rules, all eight
//! functions stay honest `todo!()` stubs rather than faked bodies.
//!
//! # Stubbed (each blocked on specific unported substrate)
//!
//! - [`AvailableMetersPanel_delete`] (`AvailableMetersPanel.c:34`) — the C
//!   body is `free(this->meterPanels)` + `Panel_done` + `free(this)`. In
//!   Rust the owned fields are released by `Drop`, so — exactly like
//!   [`crate::ported::panel::Panel_delete`] and
//!   [`crate::ported::history::History_delete`] — there is no algorithm to
//!   port.
//! - [`AvailableMetersPanel_addMeter`] (`AvailableMetersPanel.c:41`) —
//!   calls `Header_addMeterByClass` (a `todo!()` stub in `header.rs`, blocked
//!   on the `MeterClass` vtable + `Machine` host) and `Meter_toListItem`,
//!   which is not ported anywhere in the crate.
//! - [`AvailableMetersPanel_eventHandler`] (`AvailableMetersPanel.c:47`) —
//!   needs the `HandlerResult` enum (`IGNORED`/`HANDLED`/`SYNTH_KEY`), which
//!   is not modeled anywhere; `Platform_meterTypes[]` (the platform meter
//!   class table), not ported; `ScreenManager_resize` and `Header_updateData`/
//!   `Header_draw`, all `todo!()` stubs; plus the blocked `addMeter` above.
//! - [`AvailableMetersPanel_addCPUMeters`] (`AvailableMetersPanel.c:103`) —
//!   takes a `const MeterClass* type` and reads `type->uiName`; the
//!   `MeterClass` vtable type is not modeled (`meter.rs` models only the
//!   `Meter` instance). Also uses `Settings_cpuId`, which is not ported as a
//!   free fn, and the `machine.rs` `Settings` model carries no
//!   `countCPUsFromOne` field to inline it against.
//! - [`AvailableMetersPanel_addDynamicMeter`] (`AvailableMetersPanel.c:122`)
//!   — a `Hashtable_foreach` callback reading `meter->description` /
//!   `meter->caption` / `meter->name`; the `dynamicmeter.rs` `DynamicMeter`
//!   model carries only `name` (the other fields are unmodeled and
//!   `dynamicmeter.rs` is off-limits to this module).
//! - [`AvailableMetersPanel_addDynamicMeters`] (`AvailableMetersPanel.c:134`)
//!   — drives `Hashtable_foreach` over `settings->dynamicMeters`;
//!   `Hashtable_foreach` is not ported (`hashtable.rs` ports only the prime
//!   math) and the `Settings` model has no `dynamicMeters` field.
//! - [`AvailableMetersPanel_addPlatformMeter`] (`AvailableMetersPanel.c:142`)
//!   — reads `type->description` / `type->uiName` off a `const MeterClass*`;
//!   the `MeterClass` vtable type is not modeled (same blocker as
//!   `addCPUMeters`).
//! - [`AvailableMetersPanel_new`] (`AvailableMetersPanel.c:147`) — the
//!   constructor. `FunctionBar_new`, `Panel_init`, and `Panel_setHeader` are
//!   ported, but the body's core is a loop over `Platform_meterTypes[]` (not
//!   ported) comparing each entry against `&CPUMeter_class` / `&DynamicMeter_class`
//!   (class-identity comparison with no modeled `MeterClass`), dispatching to
//!   the blocked `addDynamicMeters`/`addPlatformMeter`/`addCPUMeters` helpers.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `static void AvailableMetersPanel_delete(Object* object)`
/// from `AvailableMetersPanel.c:34`. `free(meterPanels)` + `Panel_done` +
/// `free(this)` — released by `Drop` in Rust, so there is no algorithm.
pub fn AvailableMetersPanel_delete() {
    todo!("port of AvailableMetersPanel.c:34 — Drop releases the owned fields")
}

/// TODO: port of `static inline void AvailableMetersPanel_addMeter(Header*
/// header, MetersPanel* panel, const MeterClass* type, unsigned int param,
/// size_t column)` from `AvailableMetersPanel.c:41`. Blocked: calls
/// `Header_addMeterByClass` (a `todo!()` stub in `header.rs`) and
/// `Meter_toListItem`, which is not ported anywhere in the crate.
pub fn AvailableMetersPanel_addMeter() {
    todo!("port of AvailableMetersPanel.c:41 — needs Header_addMeterByClass (stub) + unported Meter_toListItem")
}

/// TODO: port of `static HandlerResult AvailableMetersPanel_eventHandler(
/// Panel* super, int ch)` from `AvailableMetersPanel.c:47`. Blocked: needs
/// the unmodeled `HandlerResult` enum, the unported `Platform_meterTypes[]`
/// table, the `todo!()` stubs `ScreenManager_resize`/`Header_updateData`/
/// `Header_draw`, and the blocked `AvailableMetersPanel_addMeter`.
pub fn AvailableMetersPanel_eventHandler() {
    todo!("port of AvailableMetersPanel.c:47 — needs HandlerResult + Platform_meterTypes + ScreenManager_resize/Header_* + addMeter")
}

/// TODO: port of `static void AvailableMetersPanel_addCPUMeters(Panel*
/// super, const MeterClass* type, const Machine* host)` from
/// `AvailableMetersPanel.c:103`. Blocked: reads `type->uiName` off a
/// `const MeterClass*` (the `MeterClass` vtable is not modeled; `meter.rs`
/// models only the `Meter` instance) and uses `Settings_cpuId`, unported as
/// a free fn (the `machine.rs` `Settings` model has no `countCPUsFromOne`).
pub fn AvailableMetersPanel_addCPUMeters() {
    todo!("port of AvailableMetersPanel.c:103 — needs unmodeled MeterClass::uiName + unported Settings_cpuId")
}

/// TODO: port of `static void AvailableMetersPanel_addDynamicMeter(
/// ht_key_t key, void* value, void* data)` from `AvailableMetersPanel.c:122`.
/// Blocked: a `Hashtable_foreach` callback reading `meter->description` /
/// `meter->caption` / `meter->name`; the `dynamicmeter.rs` `DynamicMeter`
/// model carries only `name`, and `dynamicmeter.rs` is off-limits here.
pub fn AvailableMetersPanel_addDynamicMeter() {
    todo!("port of AvailableMetersPanel.c:122 — DynamicMeter model lacks description/caption; driven by unported Hashtable_foreach")
}

/// TODO: port of `static void AvailableMetersPanel_addDynamicMeters(Panel*
/// super, const Settings* settings, unsigned int offset)` from
/// `AvailableMetersPanel.c:134`. Blocked: drives `Hashtable_foreach` over
/// `settings->dynamicMeters`; `Hashtable_foreach` is not ported (`hashtable.rs`
/// ports only the prime math) and the `Settings` model has no `dynamicMeters`.
pub fn AvailableMetersPanel_addDynamicMeters() {
    todo!("port of AvailableMetersPanel.c:134 — needs unported Hashtable_foreach + Settings.dynamicMeters")
}

/// TODO: port of `static void AvailableMetersPanel_addPlatformMeter(Panel*
/// super, const MeterClass* type, unsigned int offset)` from
/// `AvailableMetersPanel.c:142`. Blocked: reads `type->description` /
/// `type->uiName` off a `const MeterClass*`; the `MeterClass` vtable type is
/// not modeled anywhere (same blocker as `addCPUMeters`).
pub fn AvailableMetersPanel_addPlatformMeter() {
    todo!("port of AvailableMetersPanel.c:142 — needs unmodeled MeterClass::description/uiName")
}

/// TODO: port of `AvailableMetersPanel* AvailableMetersPanel_new(Machine*
/// host, Header* header, size_t columns, MetersPanel** meterPanels,
/// ScreenManager* scr)` from `AvailableMetersPanel.c:147`. Blocked: the
/// constructor's core loops over the unported `Platform_meterTypes[]` table,
/// comparing each entry against `&CPUMeter_class` / `&DynamicMeter_class`
/// (class-identity with no modeled `MeterClass`) and dispatching to the
/// blocked `addDynamicMeters`/`addPlatformMeter`/`addCPUMeters`.
pub fn AvailableMetersPanel_new() {
    todo!("port of AvailableMetersPanel.c:147 — needs Platform_meterTypes + MeterClass identity + blocked add* helpers")
}
