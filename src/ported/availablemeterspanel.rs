//! Port of `AvailableMetersPanel.c` — htop's "Available meters" chooser (the
//! rightmost column of the Meters setup screen: pick a meter class and add it
//! to a header column with F5/F6/Enter).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Data model
//!
//! htop's `AvailableMetersPanel` (`AvailableMetersPanel.h:19`) embeds a
//! `Panel super` plus the `ScreenManager*`/`Machine*`/`Header*` back-pointers,
//! the `size_t columns` count, and `MetersPanel** meterPanels` — a *non-owning*
//! array of pointers to the header-column [`MetersPanel`]s the
//! [`ScreenManager`] also owns. [`AvailableMetersPanel`] models `super_` (the
//! `super`-keyword workaround), the three back-pointers as raw `*mut` (the
//! `MetersPanel`/`HeaderOptionsPanel` idiom — all owned elsewhere, and `scr`
//! is the self-referential cycle noted in `categoriespanel.rs`), `columns`,
//! and `meterPanels` as a `Vec<*mut MetersPanel>` (the C `MetersPanel**`
//! non-owning array).
//!
//! # Ported
//!
//! - [`AvailableMetersPanel_addPlatformMeter`] (`AvailableMetersPanel.c:142`)
//!   — appends one platform-meter chooser row (`type->description ?:
//!   type->uiName`, keyed `offset << 16`).
//! - [`AvailableMetersPanel_addMeter`] (`AvailableMetersPanel.c:41`) — adds a
//!   meter of a given class to a header column via [`Header_addMeterByClass`],
//!   appends its [`Meter_toListItem`] row to the `MetersPanel`, and selects it.
//! - [`AvailableMetersPanel_addCPUMeters`] (`AvailableMetersPanel.c:103`) —
//!   the `"CPU average"` + per-CPU chooser rows (or a single row on a
//!   uniprocessor).
//! - [`AvailableMetersPanel_new`] (`AvailableMetersPanel.c:147`) — the
//!   constructor: builds the panel, then walks [`Platform_meterTypes`] adding
//!   each platform meter and the CPU meters. **The `type == &DynamicMeter_class`
//!   discrimination is elided**: `DynamicMeter_class` (a `MeterClass` static)
//!   is not ported *and* does not appear in the ported `Platform_meterTypes`,
//!   so every entry takes the `addPlatformMeter` path (the branch is dead on
//!   the ported platform table). When `DynamicMeter_class` +
//!   [`AvailableMetersPanel_addDynamicMeters`] land, the comparison should be
//!   restored.
//! - [`AvailableMetersPanel_eventHandler`] (`AvailableMetersPanel.c:47`) —
//!   F5/`l`/`L` add the selected meter to column 0; Enter/F6/`r`/`R`/reclick
//!   add it to the last column (returning the `KEY_LEFT` synth-key). Reaches
//!   the header/columns/host/scr through the raw back-pointers (the same idiom
//!   the ported [`crate::ported::meterspanel::MetersPanel_eventHandler`] uses).
//!
//! # Stubbed (blocked on specific unported substrate)
//!
//! - [`AvailableMetersPanel_delete`] (`AvailableMetersPanel.c:34`) — by-value
//!   consume; the owned `super_` is handed to `Panel_done` and the non-owning
//!   `meterPanels` array (`free(this->meterPanels)`) drops, so there is no
//!   algorithm to port.
//! - [`AvailableMetersPanel_addDynamicMeter`] (`AvailableMetersPanel.c:122`)
//!   — a `Hashtable_foreach` callback reading `meter->description` /
//!   `meter->caption` / `meter->name`; the `dynamicmeter.rs` `DynamicMeter`
//!   model carries only `name` (the `description`/`caption` fields are
//!   unmodeled and `dynamicmeter.rs` is off-limits to this module).
//! - [`AvailableMetersPanel_addDynamicMeters`] (`AvailableMetersPanel.c:134`)
//!   — drives `Hashtable_foreach` over `settings->dynamicMeters` (now a
//!   modeled field), but transitively needs the blocked
//!   [`AvailableMetersPanel_addDynamicMeter`] callback
//!   (`DynamicMeter.description`/`.caption`).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;
use std::io::{self, Write};

use crate::ported::cpumeter::CPUMeter_class;
use crate::ported::crt::{KEY_ENTER, KEY_F, KEY_LEFT, KEY_RECLICK};
use crate::ported::dynamicmeter::DynamicMeter;
use crate::ported::functionbar::FunctionBar_new;
use crate::ported::hashtable::Hashtable_foreach;
use crate::ported::header::{
    Header, Header_addMeterByClass, Header_calculateHeight, Header_draw, Header_updateData,
};
use crate::ported::listitem::{ListItem, ListItem_new};
use crate::ported::machine::Machine;
use crate::ported::meter::{MeterClass, Meter_toListItem};
use crate::ported::meterspanel::MetersPanel;
use crate::ported::panel::{
    HandlerResult, Panel, PanelClass, Panel_add, Panel_done, Panel_getSelected, Panel_new,
    Panel_setHeader, Panel_setSelected, Panel_size,
};
use crate::ported::screenmanager::{ScreenManager, ScreenManager_resize};
use crate::ported::settings::Settings;
// Platform dispatch (darwin-first): the available-meter registry comes from
// this build's platform, mirroring htop linking one platform's `Platform.c`
// (the same cfg split `header.rs` uses).
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_meterTypes;
#[cfg(not(target_os = "macos"))]
use crate::ported::linux::platform::Platform_meterTypes;

/// Port of `static const char* const AvailableMetersFunctions[]`
/// (`AvailableMetersPanel.c:32`): four blank slots, `Add Lt`/`Add Rt`, three
/// blanks, `Done  `. The trailing `NULL` sentinel is dropped (the ported
/// `FunctionBar_new` is length-bounded).
static AvailableMetersFunctions: [&str; 10] = [
    "      ", "      ", "      ", "      ", "Add Lt", "Add Rt", "      ", "      ", "      ",
    "Done  ",
];

/// Reduced model of the C `AvailableMetersPanel` struct
/// (`AvailableMetersPanel.h:19`): the embedded `Panel super` (`super_`), the
/// `ScreenManager*`/`Machine*`/`Header*` back-pointers (raw `*mut`), the
/// `columns` count, and the non-owning `MetersPanel** meterPanels` array
/// (`Vec<*mut MetersPanel>` — the panels are owned by the `ScreenManager`).
pub struct AvailableMetersPanel {
    /// C `Panel super` — the embedded panel base.
    pub super_: Panel,
    /// C `ScreenManager* scr` — non-owning back-pointer resized after a change.
    pub scr: *mut ScreenManager,
    /// C `Machine* host` — non-owning back-pointer whose `settings` the handler
    /// marks `changed`.
    pub host: *mut Machine,
    /// C `Header* header` — non-owning back-pointer the handler adds meters to.
    pub header: *mut Header,
    /// C `size_t columns` — the number of header columns.
    pub columns: usize,
    /// C `MetersPanel** meterPanels` — non-owning pointers to the column
    /// [`MetersPanel`]s (owned by the `ScreenManager`).
    pub meterPanels: Vec<*mut MetersPanel>,
}

/// Port of `const PanelClass AvailableMetersPanel_class`
/// (`AvailableMetersPanel.c:94`): sets only `.eventHandler =
/// AvailableMetersPanel_eventHandler`; `.drawFunctionBar` / `.printHeader`
/// are NULL, so those slots inherit the `Panel` defaults.
impl PanelClass for AvailableMetersPanel {
    fn as_panel(&self) -> &Panel {
        &self.super_
    }
    fn as_panel_mut(&mut self) -> &mut Panel {
        &mut self.super_
    }
    fn event_handler(&mut self, ev: i32) -> HandlerResult {
        AvailableMetersPanel_eventHandler(self, ev)
    }
}

/// Port of `static void AvailableMetersPanel_delete(Object* object)` from
/// `AvailableMetersPanel.c:34`: `free(this->meterPanels);
/// Panel_done(&this->super); free(this);`. Taking `this` by value consumes
/// the panel; the non-owning `meterPanels` `Vec` of raw pointers drops here
/// (the C `free(this->meterPanels)` — the `MetersPanel`s themselves are owned
/// by the `ScreenManager`), the embedded `super_` [`Panel`] is handed to
/// [`Panel_done`] (mirroring the C call graph), and the `scr`/`host`/`header`
/// back-pointers drop with the struct free.
pub fn AvailableMetersPanel_delete(this: AvailableMetersPanel) {
    let AvailableMetersPanel {
        super_,
        meterPanels,
        ..
    } = this;
    // C: free(this->meterPanels) — the non-owning MetersPanel* array.
    let _ = meterPanels;
    Panel_done(super_);
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

/// Port of `static HandlerResult AvailableMetersPanel_eventHandler(Panel*
/// super, int ch)` from `AvailableMetersPanel.c:47`.
///
/// Reads the selected [`ListItem`]'s key (`param = key & 0xffff`,
/// `type = key >> 16`); F5/`l`/`L` add the meter of class
/// `Platform_meterTypes[type]` to column 0, and Enter/CR/F6/`r`/`R`/reclick
/// add it to the last column (returning `(KEY_LEFT << 16) | SYNTH_KEY` so the
/// manager focuses left). On a change it marks `host->settings` dirty,
/// recomputes/redraws the header, and resizes the manager.
///
/// The C `Panel* super` (upcast to `AvailableMetersPanel*`) becomes the
/// reduced-struct receiver `this: &mut AvailableMetersPanel`; `this.header` /
/// `this.meterPanels[i]` / `this.host` / `this.scr` are the raw back-pointers,
/// dereferenced under `unsafe` (the same idiom the ported
/// [`crate::ported::meterspanel::MetersPanel_eventHandler`] uses). `header` and
/// the target `MetersPanel` are distinct objects, so the two `&mut`s that
/// [`AvailableMetersPanel_addMeter`] takes do not overlap.
pub fn AvailableMetersPanel_eventHandler(
    this: &mut AvailableMetersPanel,
    ch: i32,
) -> HandlerResult {
    // `KEY_F(n)` is a `const fn`; bind the two matched codes as `const`s.
    const KEY_F5: i32 = KEY_F(5);
    const KEY_F6: i32 = KEY_F(6);
    const L_LOWER: i32 = b'l' as i32;
    const L_UPPER: i32 = b'L' as i32;
    const R_LOWER: i32 = b'r' as i32;
    const R_UPPER: i32 = b'R' as i32;

    // C: const ListItem* selected = (ListItem*) Panel_getSelected(super);
    //    if (!selected) return IGNORED;
    let key = match Panel_getSelected(&this.super_) {
        None => return HandlerResult::IGNORED,
        Some(obj) => {
            let any: &dyn Any = obj;
            any.downcast_ref::<ListItem>()
                .expect("AvailableMetersPanel_eventHandler: selected row is not a ListItem")
                .key
        }
    };
    let param = (key & 0xffff) as u32;
    let type_idx = (key >> 16) as usize;

    let mut result = HandlerResult::IGNORED;
    let mut update = false;

    match ch {
        KEY_F5 | L_LOWER | L_UPPER => {
            // AvailableMetersPanel_addMeter(header, this->meterPanels[0], Platform_meterTypes[type], param, 0);
            let type_ = Platform_meterTypes[type_idx];
            // SAFETY: `header` and `meterPanels[0]` are the raw back-pointers set
            // at construction; both outlive this panel (owned by the
            // ScreenManager) and are distinct objects.
            let header = unsafe { &mut *this.header };
            let panel = unsafe { &mut *this.meterPanels[0] };
            AvailableMetersPanel_addMeter(header, panel, type_, param, 0);
            result = HandlerResult::HANDLED;
            update = true;
        }
        0x0a | 0x0d | KEY_ENTER | KEY_F6 | R_LOWER | R_UPPER | KEY_RECLICK => {
            let type_ = Platform_meterTypes[type_idx];
            let column = this.columns - 1;
            // SAFETY: see above; `meterPanels[columns - 1]` is the last column.
            let header = unsafe { &mut *this.header };
            let panel = unsafe { &mut *this.meterPanels[column] };
            AvailableMetersPanel_addMeter(header, panel, type_, param, column);
            // C: result = (KEY_LEFT << 16) | SYNTH_KEY;
            result = HandlerResult(((KEY_LEFT as u32) << 16) | HandlerResult::SYNTH_KEY.0);
            update = true;
        }
        _ => {}
    }

    if update {
        // C: Settings* settings = this->host->settings;
        //    settings->changed = true; settings->lastUpdate++;
        // SAFETY: `host` is the raw back-pointer set at construction; its
        // `Settings` is present during Setup.
        {
            let settings = unsafe { (*this.host).settings.as_mut() }
                .expect("AvailableMetersPanel_eventHandler: host->settings is NULL");
            settings.changed = true;
            settings.lastUpdate += 1;
        }
        // Header_calculateHeight(header); Header_updateData(header); Header_draw(header);
        // SAFETY: `header` is the raw back-pointer; no other live borrow of it.
        {
            let header = unsafe { &mut *this.header };
            Header_calculateHeight(header);
            Header_updateData(header);
            let mut out = io::stdout().lock();
            Header_draw(header, &mut out);
            let _ = out.flush();
        }
        // ScreenManager_resize(this->scr);
        // SAFETY: `scr` is the self-referential back-pointer (owns this panel).
        let scr = unsafe { &mut *this.scr };
        ScreenManager_resize(scr);
    }

    result
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

/// Port of `static void AvailableMetersPanel_addDynamicMeter(ht_key_t key,
/// void* value, void* data)` from `AvailableMetersPanel.c:122`. The C
/// `Hashtable_foreach` callback: pick the display label
/// (`description ?: caption ?: name`) and append a `ListItem` carrying the
/// packed `(offset << 16) | id` identifier. The `void* value` / `void* data`
/// pointers are de-void'd to the concrete `&DynamicMeter` and the pieces the C
/// `DynamicIterator` carried (`identifier` + the target `super` panel), which
/// [`AvailableMetersPanel_addDynamicMeters`]' closure supplies.
pub fn AvailableMetersPanel_addDynamicMeter(
    meter: &DynamicMeter,
    identifier: u32,
    super_: &mut Panel,
) {
    // const char* label = meter->description ? meter->description : meter->caption;
    // if (!label) label = meter->name; /* last fallback, guaranteed set */
    let label = meter
        .description
        .as_deref()
        .or(meter.caption.as_deref())
        .unwrap_or(&meter.name);
    // Panel_add(iter->super, (Object*) ListItem_new(label, identifier));
    Panel_add(super_, Box::new(ListItem_new(label, identifier as i32)));
}

/// Port of `static void AvailableMetersPanel_addDynamicMeters(Panel* super,
/// const Settings* settings, unsigned int offset)` from
/// `AvailableMetersPanel.c:134`. Drives `Hashtable_foreach` over
/// `settings->dynamicMeters`, invoking [`AvailableMetersPanel_addDynamicMeter`]
/// for each entry with a running `id` (from 1) packed with `offset`. The C
/// `DynamicIterator { .super, .id, .offset }` state is carried by the closure;
/// the borrowed `dynamicMeters` `*mut Hashtable` (owned by the Machine) is
/// dereferenced for the walk.
pub fn AvailableMetersPanel_addDynamicMeters(super_: &mut Panel, settings: &Settings, offset: u32) {
    // Hashtable* dynamicMeters = settings->dynamicMeters; assert(dynamicMeters != NULL);
    let dynamic_meters = settings
        .dynamicMeters
        .expect("AvailableMetersPanel_addDynamicMeters: dynamicMeters is NULL");
    // DynamicIterator iter = { .super = super, .id = 1, .offset = offset };
    let mut id: u32 = 1;
    // Hashtable_foreach(dynamicMeters, AvailableMetersPanel_addDynamicMeter, &iter);
    // SAFETY: `dynamicMeters` is the Settings-borrowed Hashtable (a `*mut`
    // aliasing the Machine-owned table for the run).
    Hashtable_foreach(unsafe { &*dynamic_meters }, &mut |_key, value| {
        let meter = value
            .as_dynamic_meter()
            .expect("AvailableMetersPanel_addDynamicMeters: hashtable value is not a DynamicMeter");
        // unsigned int identifier = (iter->offset << 16) | iter->id;
        let identifier = (offset << 16) | id;
        AvailableMetersPanel_addDynamicMeter(meter, identifier, super_);
        id += 1;
    });
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

/// Port of `AvailableMetersPanel* AvailableMetersPanel_new(Machine* host,
/// Header* header, size_t columns, MetersPanel** meterPanels,
/// ScreenManager* scr)` from `AvailableMetersPanel.c:147`.
///
/// Builds a `1×1` [`Panel`] with the `AvailableMetersFunctions` `FunctionBar`,
/// stores the `host`/`header`/`columns`/`meterPanels`/`scr` back-pointers, sets
/// the "Available meters" header, then walks [`Platform_meterTypes`] from index
/// 1 (index 0 is `&CPUMeter_class`, handled separately) adding each entry via
/// [`AvailableMetersPanel_addPlatformMeter`], and finally the CPU chooser rows
/// via [`AvailableMetersPanel_addCPUMeters`].
///
/// The C `if (type == &DynamicMeter_class) addDynamicMeters(...) else
/// addPlatformMeter(...)` discrimination is elided: `DynamicMeter_class` (the
/// `MeterClass` static) is not ported and does not appear in the ported
/// `Platform_meterTypes`, so every entry takes the `addPlatformMeter` path
/// (the branch is dead on this platform table; see the module docs and the
/// [`AvailableMetersPanel_addDynamicMeters`] stub).
///
/// `Panel_init(super, 1, 1, 1, 1, Class(ListItem), true, fuBar)` is
/// [`Panel_new`] at those coords (dropping the `Vector`-typing args).
///
/// # Safety
///
/// `host`/`header`/`scr` must point at live objects that outlive the panel
/// (as in C, where the setup screen owns them), and every pointer in
/// `meterPanels` must reference a live [`MetersPanel`] (the header columns).
pub fn AvailableMetersPanel_new(
    host: *mut Machine,
    header: *mut Header,
    columns: usize,
    meterPanels: Vec<*mut MetersPanel>,
    scr: *mut ScreenManager,
) -> AvailableMetersPanel {
    let fu_bar = FunctionBar_new(Some(&AvailableMetersFunctions[..]), None, None);
    let super_ = Panel_new(1, 1, 1, 1, Some(fu_bar));

    let mut this = AvailableMetersPanel {
        super_,
        scr,
        host,
        header,
        columns,
        meterPanels,
    };

    Panel_setHeader(&mut this.super_, "Available meters");

    // C: Platform_meterTypes[0] is always &CPUMeter_class, handled below.
    // for (unsigned int i = 1; Platform_meterTypes[i]; i++) { ... }
    for i in 1..Platform_meterTypes.len() {
        let type_ = Platform_meterTypes[i];
        // C: assert(type != &CPUMeter_class);
        // C: if (type == &DynamicMeter_class) addDynamicMeters(...) else
        //    addPlatformMeter(super, type, i);  — see fn/module docs for the
        //    elided DynamicMeter branch.
        AvailableMetersPanel_addPlatformMeter(&mut this.super_, type_, i as u32);
    }

    // C: AvailableMetersPanel_addCPUMeters(super, &CPUMeter_class, host);
    // SAFETY: `host` is the raw back-pointer just stored; it outlives the panel.
    let host_ref = unsafe { &*host };
    AvailableMetersPanel_addCPUMeters(&mut this.super_, &CPUMeter_class, host_ref);

    this
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

    // ── AvailableMetersPanel_new ──────────────────────────────────────

    #[test]
    fn new_builds_platform_rows_then_cpu_rows() {
        // A uniprocessor host: one CPU chooser row, plus one row per
        // Platform_meterTypes entry after index 0 (CPUMeter).
        let mut host = Machine {
            existingCPUs: 1,
            settings: Some(Settings::default()),
            ..Default::default()
        };
        let hptr: *mut Machine = &mut host;
        let panel = AvailableMetersPanel_new(
            hptr,
            core::ptr::null_mut(),
            1,
            Vec::new(),
            core::ptr::null_mut(),
        );

        let platform_rows = Platform_meterTypes.len() - 1; // skip index 0 (CPU)
                                                           // uniprocessor => addCPUMeters adds exactly one row.
        assert_eq!(Panel_size(&panel.super_), platform_rows as i32 + 1);

        // Row 0 is the first non-CPU platform meter (Platform_meterTypes[1]),
        // labeled by description ?: uiName, keyed (offset=1) << 16.
        let first = Platform_meterTypes[1];
        let expected = first.description.unwrap_or(first.uiName);
        assert_eq!(row(&panel.super_, 0).value, expected);
        assert_eq!(row(&panel.super_, 0).key, 1 << 16);

        // Stored back-pointers/fields.
        assert_eq!(panel.columns, 1);
        assert!(panel.meterPanels.is_empty());
        assert_eq!(panel.host, hptr);
    }
}
