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
//! - [`AvailableMetersPanel_addMeter`] (`AvailableMetersPanel.c:41`) — adds a
//!   meter of a given class to a header column via the now-ported
//!   [`Header_addMeterByClass`], appends its [`Meter_toListItem`] row to the
//!   `MetersPanel`, and selects it.
//! - [`AvailableMetersPanel_addCPUMeters`] (`AvailableMetersPanel.c:103`) —
//!   adds the `"CPU average"` + per-CPU chooser rows (or a single row on a
//!   uniprocessor). Reads `type->uiName` and inlines `Settings_cpuId`
//!   (`countCPUsFromOne ? cpu + 1 : cpu`) against
//!   [`Machine::settings`]/[`Machine::existingCPUs`], all now modeled.
//!
//! # Stubbed (each blocked on specific unported substrate)
//!
//! - [`AvailableMetersPanel_delete`] (`AvailableMetersPanel.c:34`) — the C
//!   body is `free(this->meterPanels)` + `Panel_done` + `free(this)`. In
//!   Rust the owned fields are released by `Drop`, so — exactly like
//!   [`crate::ported::panel::Panel_delete`] and
//!   [`crate::ported::history::History_delete`] — there is no algorithm to
//!   port.
//! - [`AvailableMetersPanel_eventHandler`] (`AvailableMetersPanel.c:47`) —
//!   needs the `AvailableMetersPanel` struct (its `header`/`meterPanels`/
//!   `columns`/`host`/`scr` non-owning back-pointers, not defined anywhere in
//!   the crate) and the unported `Platform_meterTypes[]` table. (`HandlerResult`,
//!   `ScreenManager_resize`, `Header_calculateHeight`/`Header_updateData`/
//!   `Header_draw`, and the `addMeter` helper are now ported.)
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

use crate::ported::header::{Header, Header_addMeterByClass};
use crate::ported::listitem::ListItem_new;
use crate::ported::machine::Machine;
use crate::ported::meter::{MeterClass, Meter_toListItem};
use crate::ported::meterspanel::MetersPanel;
use crate::ported::panel::{Panel, Panel_add, Panel_setSelected, Panel_size};

/// TODO: port of `static void AvailableMetersPanel_delete(Object* object)`
/// from `AvailableMetersPanel.c:34`: `free(this->meterPanels);
/// Panel_done(&this->super); free(this);`. Blocked on missing substrate: the
/// `AvailableMetersPanel` struct (`MetersPanel** meterPanels` + the
/// `header`/`host`/`scr` back-pointer aliasing) is not modeled in this port,
/// so there is no `this` type to consume by value. Left a stub rather than
/// inventing an unused struct.
pub fn AvailableMetersPanel_delete() {
    todo!("port of AvailableMetersPanel.c:34 — AvailableMetersPanel struct is not modeled; no Rust type to consume")
}

/// Port of `static inline void AvailableMetersPanel_addMeter(Header* header,
/// MetersPanel* panel, const MeterClass* type, unsigned int param, size_t
/// column)` from `AvailableMetersPanel.c:41`.
///
/// Adds a meter of class `type` to header `column` via the now-ported
/// [`Header_addMeterByClass`], then appends its `Meter_toListItem(meter,
/// false)` row to `panel` and selects that last row. The C casts `MetersPanel*
/// panel` to `Panel*` (embedded `super`); the port reaches `panel.super_`.
/// `type` is `&'static MeterClass` because [`Header_addMeterByClass`] stores
/// the class pointer into the new `Meter` (the C `const MeterClass*` is always
/// a `&'static` `Platform_meterTypes[]` entry).
pub fn AvailableMetersPanel_addMeter(
    header: &mut Header,
    panel: &mut MetersPanel,
    type_: &'static MeterClass,
    param: u32,
    column: usize,
) {
    // C: const Meter* meter = Header_addMeterByClass(header, type, param, column);
    let meter = Header_addMeterByClass(header, type_, param, column);
    // C: Panel_add((Panel*)panel, (Object*) Meter_toListItem(meter, false));
    let item = Meter_toListItem(meter, false);
    Panel_add(&mut panel.super_, Box::new(item));
    // C: Panel_setSelected((Panel*)panel, Panel_size((Panel*)panel) - 1);
    let size = Panel_size(&panel.super_);
    Panel_setSelected(&mut panel.super_, size - 1);
}

/// TODO: port of `static HandlerResult AvailableMetersPanel_eventHandler(
/// Panel* super, int ch)` from `AvailableMetersPanel.c:47`. Blocked: needs the
/// `AvailableMetersPanel` struct, which is not defined anywhere in the crate.
/// Its fields are non-owning back-pointers with no safe-Rust model — most
/// critically `MetersPanel** meterPanels` (an array of pointers aliasing the
/// header-column panels the setup screen also owns), plus `header`/`host`/
/// `scr`. Without the struct the handler cannot take `this` to reach
/// `this->meterPanels[...]`. (`HandlerResult`, `Platform_meterTypes[]`,
/// `Header_calculateHeight`/`Header_updateData`/`Header_draw`,
/// `ScreenManager_resize`, `Panel_getSelected`, and the `addMeter` helper are
/// all now ported.)
pub fn AvailableMetersPanel_eventHandler() {
    todo!("port of AvailableMetersPanel.c:47 — needs the AvailableMetersPanel struct (MetersPanel** meterPanels + header/host/scr back-pointer aliasing)")
}

/// Port of `static void AvailableMetersPanel_addCPUMeters(Panel* super, const
/// MeterClass* type, const Machine* host)` from `AvailableMetersPanel.c:103`.
///
/// With more than one CPU, adds a `"CPU average"` row (key 0) followed by one
/// `"<uiName> <cpuId>"` row per CPU (key `i`, `1..=existingCPUs`); with a
/// single CPU, adds one `type->uiName` row (key 1). `Settings_cpuId(settings,
/// cpu)` — `settings->countCPUsFromOne ? cpu + 1 : cpu` (`Settings.h`) — is
/// inlined against `host.settings.countCPUsFromOne` (`settings::Settings`,
/// which `Machine::settings` holds). The C stack `char buffer[50]` is a
/// truncating `xSnprintf`; a `"<uiName> <int>"` CPU label never reaches that
/// bound, so `format!` is the faithful content translation.
pub fn AvailableMetersPanel_addCPUMeters(super_: &mut Panel, type_: &MeterClass, host: &Machine) {
    if host.existingCPUs > 1 {
        // C: Panel_add(super, (Object*) ListItem_new("CPU average", 0));
        Panel_add(super_, Box::new(ListItem_new("CPU average", 0)));
        // C: Settings_cpuId(host->settings, cpu) == countCPUsFromOne ? cpu+1 : cpu
        let count_from_one = host
            .settings
            .as_ref()
            .expect("AvailableMetersPanel_addCPUMeters: Machine.settings is set")
            .countCPUsFromOne;
        for i in 1..=host.existingCPUs {
            let cpu = i - 1;
            let cpu_id = if count_from_one { cpu + 1 } else { cpu };
            // C: xSnprintf(buffer, sizeof(buffer), "%s %d", type->uiName, cpuId);
            let buffer = format!("{} {}", type_.uiName, cpu_id);
            Panel_add(super_, Box::new(ListItem_new(&buffer, i as i32)));
        }
    } else {
        // C: Panel_add(super, (Object*) ListItem_new(type->uiName, 1));
        Panel_add(super_, Box::new(ListItem_new(type_.uiName, 1)));
    }
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
/// the `host`/`header`/`columns`/`meterPanels`/`scr` back-pointers (the same
/// `MetersPanel**` aliasing as the event handler), and its loop dispatches to
/// the still-blocked [`AvailableMetersPanel_addDynamicMeters`] (missing
/// `Settings.dynamicMeters` + `DynamicMeter` description/caption). (`Platform_meterTypes[]`,
/// the ported [`AvailableMetersPanel_addCPUMeters`]/[`AvailableMetersPanel_addPlatformMeter`]
/// helpers, `FunctionBar_new`, `Panel_init`, and `Panel_setHeader` are now
/// available.)
pub fn AvailableMetersPanel_new() {
    todo!("port of AvailableMetersPanel.c:147 — needs AvailableMetersPanel struct (meterPanels/scr aliasing) + the blocked addDynamicMeters helper")
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

    // ── AvailableMetersPanel_addCPUMeters ─────────────────────────────

    use crate::ported::settings::Settings;

    #[test]
    fn add_cpu_meters_single_cpu_adds_one_row() {
        // C: else branch — one `type->uiName` row keyed 1.
        let mut panel = Panel_new(1, 1, 1, 1, None);
        let class = mk_class("CPU", None);
        let host = Machine {
            existingCPUs: 1,
            ..Default::default()
        };
        AvailableMetersPanel_addCPUMeters(&mut panel, &class, &host);
        assert_eq!(Panel_size(&panel), 1);
        assert_eq!(row(&panel, 0).value, "CPU");
        assert_eq!(row(&panel, 0).key, 1);
    }

    #[test]
    fn add_cpu_meters_multi_cpu_average_plus_per_cpu() {
        // C: existingCPUs > 1 — "CPU average" (key 0) + one row per CPU.
        let mut panel = Panel_new(1, 1, 1, 1, None);
        let class = mk_class("CPU", None);
        let host = Machine {
            existingCPUs: 3,
            settings: Some(Settings::default()), // countCPUsFromOne == false
            ..Default::default()
        };
        AvailableMetersPanel_addCPUMeters(&mut panel, &class, &host);
        assert_eq!(Panel_size(&panel), 4);
        assert_eq!(row(&panel, 0).value, "CPU average");
        assert_eq!(row(&panel, 0).key, 0);
        // Settings_cpuId with countCPUsFromOne false => cpu index i-1.
        assert_eq!(row(&panel, 1).value, "CPU 0");
        assert_eq!(row(&panel, 1).key, 1);
        assert_eq!(row(&panel, 3).value, "CPU 2");
        assert_eq!(row(&panel, 3).key, 3);
    }

    #[test]
    fn add_cpu_meters_count_from_one_shifts_labels() {
        // countCPUsFromOne => Settings_cpuId returns cpu + 1.
        let mut panel = Panel_new(1, 1, 1, 1, None);
        let class = mk_class("CPU", None);
        let settings = Settings {
            countCPUsFromOne: true,
            ..Default::default()
        };
        let host = Machine {
            existingCPUs: 2,
            settings: Some(settings),
            ..Default::default()
        };
        AvailableMetersPanel_addCPUMeters(&mut panel, &class, &host);
        assert_eq!(row(&panel, 1).value, "CPU 1"); // cpu 0 -> id 1
        assert_eq!(row(&panel, 2).value, "CPU 2"); // cpu 1 -> id 2
    }
}
