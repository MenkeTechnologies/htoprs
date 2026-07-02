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
//! - [`AvailableMetersPanel_addPlatformMeter`] (`AvailableMetersPanel.c:142`)
//!   — appends one platform-meter chooser row. Reads `type->description ?
//!   type->description : type->uiName` off a `const MeterClass*` (both fields
//!   are now modeled on [`crate::ported::meter::MeterClass`]) and pushes a
//!   [`ListItem_new`] keyed `offset << 16` via [`Panel_add`] — a self-contained
//!   slice with every dependency present.
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
//! - [`AvailableMetersPanel_new`] (`AvailableMetersPanel.c:147`) — the
//!   constructor. `FunctionBar_new`, `Panel_init`, and `Panel_setHeader` are
//!   ported, but the body's core is a loop over `Platform_meterTypes[]` (not
//!   ported) comparing each entry against `&CPUMeter_class` / `&DynamicMeter_class`
//!   (class-identity comparison with no modeled `MeterClass`), dispatching to
//!   the blocked `addDynamicMeters`/`addPlatformMeter`/`addCPUMeters` helpers.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::listitem::ListItem_new;
use crate::ported::meter::MeterClass;
use crate::ported::panel::{Panel, Panel_add};

/// TODO: port of `static void AvailableMetersPanel_delete(Object* object)`
/// from `AvailableMetersPanel.c:34`. `free(meterPanels)` + `Panel_done` +
/// `free(this)` — released by `Drop` in Rust, so there is no algorithm.
pub fn AvailableMetersPanel_delete() {
    todo!("port of AvailableMetersPanel.c:34 — Drop releases the owned fields")
}

/// TODO: port of `static inline void AvailableMetersPanel_addMeter(Header*
/// header, MetersPanel* panel, const MeterClass* type, unsigned int param,
/// size_t column)` from `AvailableMetersPanel.c:41`. Blocked: calls
/// `Header_addMeterByClass`, still a `todo!()` stub in `header.rs`
/// (`Header.c:173`), blocked on the `MeterClass` vtable + `Machine` host.
/// (`Meter_toListItem` and `Panel_setSelected`/`Panel_size` are now ported,
/// so `Header_addMeterByClass` is the sole remaining blocker.)
pub fn AvailableMetersPanel_addMeter() {
    todo!("port of AvailableMetersPanel.c:41 — needs Header_addMeterByClass (todo!() stub in header.rs)")
}

/// TODO: port of `static HandlerResult AvailableMetersPanel_eventHandler(
/// Panel* super, int ch)` from `AvailableMetersPanel.c:47`. Blocked: needs
/// the `AvailableMetersPanel` struct (its `header`/`meterPanels`/`columns`/
/// `host`/`scr` fields — the struct is not defined anywhere in the crate),
/// the unported `Platform_meterTypes[]` table, the `todo!()` stubs
/// `Header_updateData`/`Header_draw` (`header.rs`), and the blocked
/// `AvailableMetersPanel_addMeter`. (`HandlerResult`, `ScreenManager_resize`,
/// `Header_calculateHeight`, and `Panel_getSelected` are now ported.)
pub fn AvailableMetersPanel_eventHandler() {
    todo!("port of AvailableMetersPanel.c:47 — needs AvailableMetersPanel struct + Platform_meterTypes + Header_updateData/Header_draw stubs + addMeter")
}

/// TODO: port of `static void AvailableMetersPanel_addCPUMeters(Panel*
/// super, const MeterClass* type, const Machine* host)` from
/// `AvailableMetersPanel.c:103`. Blocked: the `host->existingCPUs > 1`
/// branch inlines `Settings_cpuId(host->settings, i - 1)` (`Settings.h:119`,
/// `countCPUsFromOne ? cpu + 1 : cpu`), but the `machine.rs` `Settings`
/// model that `Machine::settings` holds carries no `countCPUsFromOne` field
/// to inline against (and that field lives in an off-limits module).
/// (`MeterClass::uiName` and `Machine::existingCPUs` are now modeled.)
pub fn AvailableMetersPanel_addCPUMeters() {
    todo!("port of AvailableMetersPanel.c:103 — needs Settings_cpuId inline over machine::Settings.countCPUsFromOne (field absent)")
}

/// TODO: port of `static void AvailableMetersPanel_addDynamicMeter(
/// ht_key_t key, void* value, void* data)` from `AvailableMetersPanel.c:122`.
/// Blocked: a `Hashtable_foreach` callback reading `meter->description` /
/// `meter->caption` / `meter->name`; the `dynamicmeter.rs` `DynamicMeter`
/// model carries only `name` (the `description`/`caption` fields are
/// unmodeled), and `dynamicmeter.rs` is off-limits here. (`Hashtable_foreach`
/// is now ported.)
pub fn AvailableMetersPanel_addDynamicMeter() {
    todo!(
        "port of AvailableMetersPanel.c:122 — DynamicMeter model lacks description/caption fields"
    )
}

/// TODO: port of `static void AvailableMetersPanel_addDynamicMeters(Panel*
/// super, const Settings* settings, unsigned int offset)` from
/// `AvailableMetersPanel.c:134`. Blocked: drives `Hashtable_foreach` over
/// `settings->dynamicMeters`, but the `Settings` model carries no
/// `dynamicMeters` field. Also blocked transitively on the callback
/// [`AvailableMetersPanel_addDynamicMeter`] (missing `DynamicMeter`
/// description/caption fields). (`Hashtable_foreach` is now ported.)
pub fn AvailableMetersPanel_addDynamicMeters() {
    todo!("port of AvailableMetersPanel.c:134 — needs Settings.dynamicMeters field + the blocked addDynamicMeter callback")
}

/// Port of `static void AvailableMetersPanel_addPlatformMeter(Panel* super,
/// const MeterClass* type, unsigned int offset)` from
/// `AvailableMetersPanel.c:142`.
///
/// ```c
/// const char* label = type->description ? type->description : type->uiName;
/// Panel_add(super, (Object*) ListItem_new(label, offset << 16));
/// ```
///
/// `type->description` is `NULL` for most meter classes and falls back to
/// `type->uiName`; the ported [`MeterClass`] models `description` as an
/// `Option<&'static str>`, so the C ternary is `unwrap_or(type_.uiName)`. The
/// `offset << 16` key packs the platform-meter table index into the high half
/// of the `ListItem` key (the low half holds the meter `param`, always 0 for
/// a platform meter). The heap `ListItem*` becomes an owned `ListItem` boxed
/// as `Object` for [`Panel_add`].
pub fn AvailableMetersPanel_addPlatformMeter(super_: &mut Panel, type_: &MeterClass, offset: u32) {
    let label = type_.description.unwrap_or(type_.uiName);
    Panel_add(super_, Box::new(ListItem_new(label, (offset << 16) as i32)));
}

/// TODO: port of `AvailableMetersPanel* AvailableMetersPanel_new(Machine*
/// host, Header* header, size_t columns, MetersPanel** meterPanels,
/// ScreenManager* scr)` from `AvailableMetersPanel.c:147`. Blocked: needs the
/// `AvailableMetersPanel` struct (not defined anywhere in the crate) to store
/// `host`/`header`/`columns`/`meterPanels`/`scr`, and its core loops over the
/// unported `Platform_meterTypes[]` table, comparing each entry against
/// `&CPUMeter_class` / `&DynamicMeter_class` (class-identity with no modeled
/// `CPUMeter_class`/`DynamicMeter_class`) and dispatching to the blocked
/// `addDynamicMeters`/`addCPUMeters` helpers. (`FunctionBar_new`, `Panel_init`,
/// `Panel_setHeader`, and `addPlatformMeter` are ported, but the surrounding
/// struct, driving loop, and class-identity tests are not.)
pub fn AvailableMetersPanel_new() {
    todo!("port of AvailableMetersPanel.c:147 — needs AvailableMetersPanel struct + Platform_meterTypes + MeterClass identity + blocked add* helpers")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::listitem::ListItem;
    use crate::ported::object::{ObjectClass, Object_class};
    use crate::ported::panel::{Panel_get, Panel_new, Panel_size};

    // A minimal `MeterClass` with a chosen `uiName`/`description`. Every other
    // slot is the empty/None/default the C `.super = { .extends = Class(Meter) }`
    // initializers leave zero — none is read by `addPlatformMeter`.
    fn mk_class(uiName: &'static str, description: Option<&'static str>) -> MeterClass {
        MeterClass {
            super_: ObjectClass {
                extends: Some(&Object_class),
            },
            display: None,
            init: None,
            done: None,
            updateMode: None,
            updateValues: None,
            draw: None,
            getCaption: None,
            getUiName: None,
            defaultMode: 0,
            supportedModes: 0,
            total: 0.0,
            attributes: &[],
            name: "",
            uiName,
            caption: "",
            description,
            maxItems: 0,
            isMultiColumn: false,
            isPercentChart: false,
        }
    }

    fn row(panel: &Panel, i: i32) -> &ListItem {
        let any: &dyn core::any::Any = Panel_get(panel, i);
        any.downcast_ref::<ListItem>()
            .expect("addPlatformMeter row is not a ListItem")
    }

    #[test]
    fn add_platform_meter_uses_description_when_present() {
        let mut panel = Panel_new(1, 1, 1, 1, None);
        let class = mk_class("Uptime", Some("System uptime"));

        AvailableMetersPanel_addPlatformMeter(&mut panel, &class, 3);

        assert_eq!(Panel_size(&panel), 1);
        let item = row(&panel, 0);
        // C: label = type->description ? type->description : type->uiName;
        assert_eq!(item.value, "System uptime");
        // C: key = offset << 16;
        assert_eq!(item.key, 3 << 16);
    }

    #[test]
    fn add_platform_meter_falls_back_to_ui_name() {
        let mut panel = Panel_new(1, 1, 1, 1, None);
        let class = mk_class("Hostname", None);

        AvailableMetersPanel_addPlatformMeter(&mut panel, &class, 7);

        assert_eq!(Panel_size(&panel), 1);
        let item = row(&panel, 0);
        // description == None (C NULL) => uiName.
        assert_eq!(item.value, "Hostname");
        assert_eq!(item.key, 7 << 16);
    }
}
