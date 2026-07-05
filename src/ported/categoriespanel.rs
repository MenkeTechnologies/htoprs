//! Port of `CategoriesPanel.c` — the Setup screen's category list.
//!
//! `CategoriesPanel` is the left-hand list of the Setup screen ("Display
//! options", "Header layout", "Meters", "Screens", "Colors"). Selecting a row
//! tears down every panel to its right in the [`ScreenManager`] and rebuilds
//! the page for that category by calling the matching sibling-panel
//! constructor. The whole file is *glue*: it wires the `ScreenManager`, the
//! `Panel` base widget, and the per-category sub-panels together.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
//!
//! # Data model
//!
//! htop's `struct CategoriesPanel_` (`CategoriesPanel.h:16`) is a `Panel super`
//! plus three non-owning back-pointers (`ScreenManager* scr`, `Machine* host`,
//! `Header* header`). [`CategoriesPanel`] models `super_` (the `super`-keyword
//! workaround) and the three back-pointers as raw `*mut` — the same idiom the
//! ported [`crate::ported::meterspanel::MetersPanel`] /
//! [`crate::ported::headeroptionspanel::HeaderOptionsPanel`] use for their
//! `scr`/`settings` back-pointers. `scr` is the self-referential cycle: the
//! `ScreenManager`'s `panels` owns this very `CategoriesPanel` (added by
//! `ScreenManager_add(scr, super, 16)` in [`CategoriesPanel_new`]), so a raw
//! pointer is the faithful C mapping — the same raw-`scr`-deref every ported
//! event handler already uses.
//!
//! # Ported
//!
//! - [`CategoriesPanel_eventHandler`] (`CategoriesPanel.c:120`) — the key
//!   dispatch computing the [`HandlerResult`], plus the `if (result ==
//!   HANDLED)` tail that removes every panel to the right
//!   (`ScreenManager_size`/`ScreenManager_remove`) and rebuilds the selected
//!   page via `categoriesPanelPages[selected].ctor(this)`.
//! - [`CategoriesPanel_new`] (`CategoriesPanel.c:172`) — builds the list, then
//!   self-registers into `scr` and builds the first page.
//! - [`CategoriesPanel_delete`] — by-value consume (`Panel_done` + `Drop`).
//! - [`CategoriesPanel_makeDisplayOptionsPage`] /
//!   [`CategoriesPanel_makeColorsPage`] /
//!   [`CategoriesPanel_makeHeaderOptionsPage`] — build the corresponding
//!   sub-panel via the ported [`DisplayOptionsPanel_new`] /
//!   [`ColorsPanel_new`] / [`HeaderOptionsPanel_new`] and register it with
//!   `ScreenManager_add`.
//! - [`CategoriesPanel_makeScreensPage`] (`CategoriesPanel.c:87`) — builds the
//!   [`ScreensPanel_new`] editor (which itself boxes the `columns` /
//!   `availableColumns` sub-panels into `scr`) and registers it.
//!
//! # Sub-panel ownership notes
//!
//! Every page builder is ported. The two that add a sub-panel owned by another
//! panel resolve it the way htop does — the owner keeps a non-owning pointer
//! and the `ScreenManager` frees it:
//! - [`CategoriesPanel_makeMetersPage`] (`CategoriesPanel.c:43`) — moves each
//!   header column's meters into a per-column `MetersPanel` (the header column
//!   is emptied while the page is open) and restores them via the panel's
//!   `Drop` when the page closes.
//! - [`CategoriesPanel_makeScreenTabsPage`] (`CategoriesPanel.c:78`) — the
//!   `ScreenTabsPanel`'s `names` sub-panel is `Box::into_raw`'d by
//!   `ScreenTabsPanel_new`, reconstituted here, and handed to the same
//!   `ScreenManager` (which owns both, as the C's owning setup manager does).
//!   Only *registered* under `#if defined(HTOP_PCP)`, so it is never dispatched
//!   in this build, but the C function is always defined and is ported.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::ffi::c_int;

use crate::ported::availablemeterspanel::AvailableMetersPanel_new;
use crate::ported::colorspanel::ColorsPanel_new;
use crate::ported::crt::{KEY_CTRL, KEY_DOWN, KEY_END, KEY_HOME, KEY_NPAGE, KEY_PPAGE, KEY_UP};
use crate::ported::displayoptionspanel::DisplayOptionsPanel_new;
use crate::ported::functionbar::FunctionBar_new;
use crate::ported::header::Header;
use crate::ported::headeroptionspanel::HeaderOptionsPanel_new;
use crate::ported::listitem::ListItem_new;
use crate::ported::machine::Machine;
use crate::ported::meter::{Meter, Meter_class};
use crate::ported::meterspanel::{MetersPanel, MetersPanel_new};
use crate::ported::panel::{
    HandlerResult, Panel, PanelClass, Panel_add, Panel_getSelectedIndex, Panel_new, Panel_onKey,
    Panel_selectByTyping, Panel_setHeader, EVENT_SET_SELECTED,
};
use crate::ported::screenmanager::{
    ScreenManager, ScreenManager_add, ScreenManager_remove, ScreenManager_size,
};
use crate::ported::screenspanel::ScreensPanel_new;
use crate::ported::screentabspanel::ScreenTabsPanel_new;
use crate::ported::settings::{HeaderLayout_getColumns, Settings};
use crate::ported::vector::{Vector_add, Vector_new};

// The two Ctrl-key codes `CategoriesPanel_eventHandler` matches in its
// navigation arm (`KEY_CTRL('P')` / `KEY_CTRL('N')`). `KEY_CTRL` is a
// `const fn`; binding its results as `const`s makes them usable as `match`
// patterns without adding any top-level `fn`.
const CTRL_P: i32 = KEY_CTRL(b'P' as i32);
const CTRL_N: i32 = KEY_CTRL(b'N' as i32);

/// Port of `static const char* const CategoriesFunctions[]`
/// (`CategoriesPanel.c:35`): nine blank slots then `"Done  "`. The trailing
/// `NULL` sentinel is dropped (the ported `FunctionBar_new` is length-bounded).
static CategoriesFunctions: [&str; 10] = [
    "      ", "      ", "      ", "      ", "      ", "      ", "      ", "      ", "      ",
    "Done  ",
];

/// Reduced model of the C `CategoriesPanel` struct (`CategoriesPanel.h:16`):
/// the embedded `Panel super` (`super_`) and the three non-owning
/// `ScreenManager*`/`Machine*`/`Header*` back-pointers (raw `*mut`; `scr` is
/// the self-referential cycle described in the module docs).
pub struct CategoriesPanel {
    /// C `Panel super` — the embedded panel base.
    pub super_: Panel,
    /// C `ScreenManager* scr` — the manager that owns this panel; the
    /// event-handler tail and every `make*Page` add/remove panels through it.
    pub scr: *mut ScreenManager,
    /// C `Machine* host` — non-owning back-pointer whose `settings` each
    /// `make*Page` passes to the sub-panel constructor.
    pub host: *mut Machine,
    /// C `Header* header` — non-owning back-pointer (only
    /// [`CategoriesPanel_makeMetersPage`] reads it).
    pub header: *mut Header,
}

/// Port of `const PanelClass CategoriesPanel_class` (`CategoriesPanel.c:164`):
/// sets only `.eventHandler = CategoriesPanel_eventHandler`; `.drawFunctionBar`
/// / `.printHeader` are NULL, inheriting the `Panel` defaults.
impl PanelClass for CategoriesPanel {
    fn as_panel(&self) -> &Panel {
        &self.super_
    }
    fn as_panel_mut(&mut self) -> &mut Panel {
        &mut self.super_
    }
    fn event_handler(&mut self, ev: i32) -> HandlerResult {
        CategoriesPanel_eventHandler(self, ev)
    }
}

/// Port of the C `CategoriesPanel_makePageFunc` typedef (`CategoriesPanel.c:103`):
/// `void (*)(CategoriesPanel* ref)`.
type CategoriesPanel_makePageFunc = fn(&mut CategoriesPanel);

/// Port of the file-local `CategoriesPanelPage` struct (`CategoriesPanel.c:104`):
/// a category name plus its page-builder ctor.
///
/// `available` is an htoprs addition (no C analog): the C builds every page, but
/// a page ctor here may still be a `todo!()` stub. Navigating to such a category
/// must not panic the TUI, so an unavailable page is skipped — its right pane
/// stays empty until the ctor is ported. This flag is the guard, not a port: an
/// unported ctor keeps its `todo!()` body so coverage stays honest. Every page
/// in the current non-PCP table is ported, so all are `available = true`; the
/// flag remains as a safety net for future stubs.
struct CategoriesPanelPage {
    name: &'static str,
    ctor: CategoriesPanel_makePageFunc,
    available: bool,
}

/// Port of `static CategoriesPanelPage categoriesPanelPages[]`
/// (`CategoriesPanel.c:109`) — the name/ctor dispatch table, in the non-PCP
/// configuration (the `"Screen tabs"` entry is `#if defined(HTOP_PCP)`, which
/// this build does not define, so it is absent — matching `screenmanager.rs`'s
/// `#ifndef HAVE_GETMOUSE` feature choices).
static categoriesPanelPages: [CategoriesPanelPage; 5] = [
    CategoriesPanelPage {
        name: "Display options",
        ctor: CategoriesPanel_makeDisplayOptionsPage,
        available: true,
    },
    CategoriesPanelPage {
        name: "Header layout",
        ctor: CategoriesPanel_makeHeaderOptionsPage,
        available: true,
    },
    CategoriesPanelPage {
        name: "Meters",
        ctor: CategoriesPanel_makeMetersPage,
        available: true,
    },
    CategoriesPanelPage {
        name: "Screens",
        ctor: CategoriesPanel_makeScreensPage,
        available: true,
    },
    CategoriesPanelPage {
        name: "Colors",
        ctor: CategoriesPanel_makeColorsPage,
        available: true,
    },
];

/// Port of `static void CategoriesPanel_delete(Object* object)` from
/// `CategoriesPanel.c:37`: `Panel_done(&this->super); free(this);`. Taking
/// `this` by value consumes the panel; the embedded `super_` [`Panel`] is
/// handed to [`crate::ported::panel::Panel_done`] (mirroring the C call graph),
/// and the non-owning `scr`/`host`/`header` back-pointers drop with the struct.
pub fn CategoriesPanel_delete(this: CategoriesPanel) {
    let CategoriesPanel { super_, .. } = this;
    crate::ported::panel::Panel_done(super_);
}

/// Read `this->host->settings` as a raw `*mut Settings`. Gate-skipped
/// associated fn (not a C fn) shared by the `make*Page` builders, which each
/// open with `Settings* settings = this->host->settings;`. The pointer is
/// taken through an explicit `&mut *this.host` deref so the field access does
/// not implicitly autoref a raw pointer.
impl CategoriesPanel {
    fn host_settings(&self) -> *mut Settings {
        // SAFETY: `host` is the non-owning back-pointer set at construction; its
        // `Settings` is present during Setup.
        unsafe {
            let h = &mut *self.host;
            h.settings
                .as_mut()
                .expect("CategoriesPanel: host->settings is NULL") as *mut Settings
        }
    }
}

/// Port of `static void CategoriesPanel_makeMetersPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:43`. Builds one [`MetersPanel`] per header column,
/// cross-links left/right neighbors, and appends the [`AvailableMetersPanel`].
///
/// The C shares each `this->header->columns[i]` `Vector*` between the header and
/// the panel, so edits are live. The ported [`Header`] owns its columns
/// (`Vec<Vec<Meter>>`) and [`MetersPanel`] owns its `Vector`, so the bridge is:
/// **move** the column's meters into the panel here (converting the `Vec<Meter>`
/// into a `Vector` of `Meter`-as-`Object`), and have [`MetersPanel`]'s `Drop`
/// **move them back** into `header.columns[i]` when the panel is dropped — on a
/// category switch (`CategoriesPanel_eventHandler` removes the page) or on Setup
/// close (`Action_runSetup` drops the manager before `Header_writeBackToSettings`
/// reads the columns). While the Meters page is open the header's own copy of
/// that column is therefore empty; it is restored the moment the page closes.
///
/// [`AvailableMetersPanel`]: crate::ported::availablemeterspanel::AvailableMetersPanel
pub fn CategoriesPanel_makeMetersPage(this: &mut CategoriesPanel) {
    // C: size_t columns = HeaderLayout_getColumns(this->scr->header->headerLayout);
    let header = this.header;
    let scr = this.scr;
    let host = this.host;
    let settings = this.host_settings();
    // SAFETY: `header` is the non-owning back-pointer set at construction; it
    // outlives the Setup session (Action_runSetup owns it).
    let columns = HeaderLayout_getColumns(unsafe { &*header }.headerLayout);

    // C: MetersPanel** meterPanels = xMallocArray(columns, sizeof(MetersPanel*));
    let mut meterPanels: Vec<*mut MetersPanel> = Vec::with_capacity(columns);

    for i in 0..columns {
        // C: xSnprintf(titleBuffer, sizeof(titleBuffer), "Column %zu", i + 1);
        let title = format!("Column {}", i + 1);

        // Bridge: move header.columns[i] (Vec<Meter>) into a `Vector` of Meters
        // boxed as `Object` — the store MetersPanel_new takes ownership of. The
        // header's copy is left empty until the panel's Drop restores it.
        // SAFETY: `header` valid for the session; `i < columns == columns.len()`
        // (the `Header` invariant). Deref explicitly to avoid an implicit
        // autoref on the raw pointer.
        let col_meters: Vec<Meter> = {
            let h = unsafe { &mut *header };
            core::mem::take(&mut h.columns[i])
        };
        let mut meters = Vector_new(&Meter_class.super_, true, col_meters.len().max(1) as c_int);
        for m in col_meters {
            Vector_add(&mut meters, Box::new(m));
        }

        // C: meterPanels[i] = MetersPanel_new(settings, titleBuffer, ..., this->scr);
        let mut boxed: Box<MetersPanel> = Box::new(MetersPanel_new(settings, &title, meters, scr));
        // Wire the header write-back target (see the fn/Drop docs).
        boxed.header = header;
        boxed.column = i;
        // Stable heap pointer to the panel; survives the move into `scr` below.
        let ptr: *mut MetersPanel = boxed.as_mut();

        // C: if (i != 0) { meterPanels[i]->leftNeighbor = meterPanels[i-1];
        //                  meterPanels[i-1]->rightNeighbor = meterPanels[i]; }
        if i != 0 {
            // SAFETY: both pointers reference live, `scr`-owned MetersPanels.
            unsafe {
                (*ptr).leftNeighbor = meterPanels[i - 1];
                (*meterPanels[i - 1]).rightNeighbor = ptr;
            }
        }
        meterPanels.push(ptr);

        // C: ScreenManager_add(this->scr, (Panel*) meterPanels[i], 20);
        // SAFETY: `scr` owns this panel for the Setup session.
        ScreenManager_add(unsafe { &mut *scr }, boxed, 20);
    }

    // C: Panel* availableMeters = AvailableMetersPanel_new(this->host, this->header,
    //        columns, meterPanels, this->scr);
    //    ScreenManager_add(this->scr, availableMeters, -1);
    let available = AvailableMetersPanel_new(host, header, columns, meterPanels, scr);
    ScreenManager_add(unsafe { &mut *scr }, Box::new(available), -1);
}

/// Port of `static void CategoriesPanel_makeDisplayOptionsPage(CategoriesPanel*
/// this)` from `CategoriesPanel.c:65`.
///
/// ```c
/// Settings* settings = this->host->settings;
/// Panel* displayOptions = (Panel*) DisplayOptionsPanel_new(settings, this->scr);
/// ScreenManager_add(this->scr, displayOptions, -1);
/// ```
pub fn CategoriesPanel_makeDisplayOptionsPage(this: &mut CategoriesPanel) {
    let settings = this.host_settings();
    let scr = this.scr;
    let displayOptions = DisplayOptionsPanel_new(settings, scr);
    // SAFETY: `scr` is the self-referential back-pointer (owns this panel); the
    // same raw-`scr` add every sibling `make*Page` performs.
    ScreenManager_add(unsafe { &mut *scr }, Box::new(displayOptions), -1);
}

/// Port of `static void CategoriesPanel_makeColorsPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:71`.
///
/// ```c
/// Settings* settings = this->host->settings;
/// Panel* colors = (Panel*) ColorsPanel_new(settings);
/// ScreenManager_add(this->scr, colors, -1);
/// ```
pub fn CategoriesPanel_makeColorsPage(this: &mut CategoriesPanel) {
    let settings = this.host_settings();
    let scr = this.scr;
    let colors = ColorsPanel_new(settings);
    // SAFETY: see makeDisplayOptionsPage.
    ScreenManager_add(unsafe { &mut *scr }, Box::new(colors), -1);
}

/// Port of `static void CategoriesPanel_makeScreenTabsPage(CategoriesPanel*
/// this)` from `CategoriesPanel.c:78`.
///
/// Adds the `ScreenTabsPanel` and its `names` sub-panel to the setup
/// `ScreenManager`. Only *registered* under `#if defined(HTOP_PCP)` (absent
/// from the non-PCP `categoriesPanelPages`), so it is never dispatched in this
/// build — but the C function is always defined, so it is ported.
///
/// The C hands the `ScreenTabsPanel`'s `names` sub-panel to a *second*
/// `ScreenManager_add`: in htop `names` is created by `ScreenTabsPanel_new` but
/// freed by the owning `ScreenManager` (`ScreenTabsPanel_delete` leaves it), so
/// it is a non-owning pointer from the panel's side. The port mirrors that —
/// `ScreenTabsPanel_new` stores `names` via `Box::into_raw`; here that box is
/// reconstituted and given to the same manager, which then owns both panels.
pub fn CategoriesPanel_makeScreenTabsPage(this: &mut CategoriesPanel) {
    // Settings* settings = this->host->settings;
    let settings = this.host_settings();
    let scr = this.scr;

    // Panel* screenTabs = (Panel*) ScreenTabsPanel_new(settings);
    // SAFETY: `settings` is the live host settings; `scr` is the
    // self-referential back-pointer every sibling make*Page adds through.
    let screenTabs = unsafe { ScreenTabsPanel_new(settings) };

    // Panel* screenNames = (Panel*) ((ScreenTabsPanel*)screenTabs)->names;
    // Capture the heap `names` pointer before `screenTabs` moves into the manager.
    let names_ptr = screenTabs.names;

    // ScreenManager_add(this->scr, screenTabs, 20);
    ScreenManager_add(unsafe { &mut *scr }, Box::new(screenTabs), 20);

    // ScreenManager_add(this->scr, screenNames, -1);
    // SAFETY: `names_ptr` was `Box::into_raw`'d in `ScreenTabsPanel_new` and is
    // not reclaimed elsewhere (`ScreenTabsPanel_delete` leaves it) — this is its
    // sole owning reclaim. The now-boxed `screenTabs` keeps a non-owning pointer
    // to the same `ScreenNamesPanel`; boxing does not relocate the heap object,
    // so that pointer stays valid for the panel's interactions.
    let screenNames = unsafe { Box::from_raw(names_ptr) };
    ScreenManager_add(unsafe { &mut *scr }, screenNames, -1);
}

/// Port of `static void CategoriesPanel_makeScreensPage(CategoriesPanel* this)`
/// from `CategoriesPanel.c:87`.
///
/// ```c
/// Settings* settings = this->host->settings;
/// Panel* screens = (Panel*) ScreensPanel_new(settings);
/// Panel* columns = (Panel*) ((ScreensPanel*)screens)->columns;
/// Panel* availableColumns = (Panel*) ((ScreensPanel*)screens)->availableColumns;
/// ScreenManager_add(this->scr, screens, 20);
/// ScreenManager_add(this->scr, columns, 20);
/// ScreenManager_add(this->scr, availableColumns, -1);
/// ```
///
/// [`ScreensPanel_new`] already boxes the `columns` / `availableColumns`
/// sub-panels and moves them into `scr` (the Rust `ScreenManager.panels` is
/// the single owner — see its docs), so the two extra `ScreenManager_add`s
/// the C performs here are done inside the constructor. This function only
/// adds the `ScreensPanel` itself, matching the sibling `make*Page` ports.
pub fn CategoriesPanel_makeScreensPage(this: &mut CategoriesPanel) {
    let settings = this.host_settings();
    let scr = this.scr;
    // SAFETY: `scr` is the self-referential back-pointer (owns this panel and
    // the sub-panels the constructor adds); the same raw-`scr` add every
    // sibling `make*Page` performs.
    let screens = ScreensPanel_new(settings, scr);
    ScreenManager_add(unsafe { &mut *scr }, Box::new(screens), 20);
}

/// Port of `static void CategoriesPanel_makeHeaderOptionsPage(CategoriesPanel*
/// this)` from `CategoriesPanel.c:97`.
///
/// ```c
/// Settings* settings = this->host->settings;
/// Panel* colors = (Panel*) HeaderOptionsPanel_new(settings, this->scr);
/// ScreenManager_add(this->scr, colors, -1);
/// ```
pub fn CategoriesPanel_makeHeaderOptionsPage(this: &mut CategoriesPanel) {
    let settings = this.host_settings();
    let scr = this.scr;
    let headerOptions = HeaderOptionsPanel_new(settings, scr);
    // SAFETY: see makeDisplayOptionsPage.
    ScreenManager_add(unsafe { &mut *scr }, Box::new(headerOptions), -1);
}

/// Port of `static HandlerResult CategoriesPanel_eventHandler(Panel* super,
/// int ch)` from `CategoriesPanel.c:120`.
///
/// The C `Panel* super` (upcast to `CategoriesPanel*`) becomes the
/// reduced-struct receiver `this: &mut CategoriesPanel`; `this.super_` is the
/// embedded panel. The key dispatch:
/// - `EVENT_SET_SELECTED` → `HANDLED`.
/// - the navigation keys call [`Panel_onKey`] and report `HANDLED` only when
///   the selection index moved.
/// - any other graphic char falls through to [`Panel_selectByTyping`]; a
///   `BREAK_LOOP` result is downgraded to `IGNORED`.
///
/// The `if (result == HANDLED)` tail removes every panel to the right of the
/// category list (`ScreenManager_size` / `ScreenManager_remove`, both ported)
/// and rebuilds the selected page via `categoriesPanelPages[selected].ctor(this)`
/// — reaching the manager through `this->scr` (the self-referential
/// back-pointer, dereferenced under `unsafe` as every ported handler does).
pub fn CategoriesPanel_eventHandler(this: &mut CategoriesPanel, ch: i32) -> HandlerResult {
    let mut result = HandlerResult::IGNORED;

    let mut selected = Panel_getSelectedIndex(&this.super_);
    match ch {
        EVENT_SET_SELECTED => {
            result = HandlerResult::HANDLED;
        }
        KEY_UP | CTRL_P | KEY_DOWN | CTRL_N | KEY_NPAGE | KEY_PPAGE | KEY_HOME | KEY_END => {
            let previous = selected;
            Panel_onKey(&mut this.super_, ch);
            selected = Panel_getSelectedIndex(&this.super_);
            if previous != selected {
                result = HandlerResult::HANDLED;
            }
        }
        _ => {
            if 0 < ch && ch < 255 && (ch as u8).is_ascii_graphic() {
                result = Panel_selectByTyping(&mut this.super_, ch);
            }
            if result == HandlerResult::BREAK_LOOP {
                result = HandlerResult::IGNORED;
            }
        }
    }

    if result == HandlerResult::HANDLED {
        // C: int size = ScreenManager_size(this->scr);
        //    for (int i = 1; i < size; i++) ScreenManager_remove(this->scr, 1);
        // SAFETY: `scr` is the self-referential back-pointer (owns this panel);
        // the same raw-`scr` deref every ported handler uses.
        {
            let scr = unsafe { &mut *this.scr };
            let size = ScreenManager_size(scr);
            for _ in 1..size {
                // Returned Box<dyn PanelClass> is dropped (the C caller discards it).
                let _ = ScreenManager_remove(scr, 1);
            }
        }
        // C: if (selected >= 0 && selected < ARRAYSIZE(categoriesPanelPages))
        //       categoriesPanelPages[selected].ctor(this);
        // htoprs: skip pages whose ctor is not yet ported (`available == false`)
        // so navigating to them leaves an empty pane instead of panicking.
        if selected >= 0 && (selected as usize) < categoriesPanelPages.len() {
            let page = &categoriesPanelPages[selected as usize];
            if page.available {
                (page.ctor)(this);
            }
        }
    }

    result
}

/// Port of `CategoriesPanel* CategoriesPanel_new(ScreenManager* scr,
/// Header* header, Machine* host)` from `CategoriesPanel.c:172`.
///
/// Builds a `1×1` [`Panel`] with the `CategoriesFunctions` `FunctionBar`,
/// stores the `scr`/`host`/`header` back-pointers, sets the "Categories"
/// header, appends one [`ListItem_new`] row per `categoriesPanelPages` entry,
/// then self-registers into `scr` (`ScreenManager_add(scr, super, 16)`) and
/// builds the first page (`categoriesPanelPages[0].ctor(this)`).
///
/// The C returns `CategoriesPanel*`, but its only caller (`Action.c:104`,
/// `Action_runSetup`) discards the return — the `ScreenManager` owns the
/// panel. The port therefore boxes the panel, moves it into `scr` via
/// [`ScreenManager_add`] (a `Box<dyn PanelClass>`), and returns `()`. A raw
/// pointer captured before the move (the `Box` heap allocation is stable)
/// reaches the now-`scr`-owned panel for the first-page dispatch — the exact
/// C aliasing where `this` and `scr->panels[last]` are the same object (the
/// raw-pointer back-pointer convention every ported handler already relies on).
///
/// # Safety
///
/// `scr`/`header`/`host` must point at live objects that outlive the setup
/// session (as in C, where `Action_runSetup` owns them for the
/// `ScreenManager_run` duration).
pub fn CategoriesPanel_new(scr: *mut ScreenManager, header: *mut Header, host: *mut Machine) {
    let fu_bar = FunctionBar_new(Some(&CategoriesFunctions[..]), None, None);
    let super_ = Panel_new(1, 1, 1, 1, Some(fu_bar));

    let mut this = Box::new(CategoriesPanel {
        super_,
        scr,
        host,
        header,
    });

    Panel_setHeader(&mut this.super_, "Categories");
    for page in categoriesPanelPages.iter() {
        Panel_add(&mut this.super_, Box::new(ListItem_new(page.name, 0)));
    }

    // C: ScreenManager_add(scr, super, 16); categoriesPanelPages[0].ctor(this);
    // Capture a raw pointer to the panel before moving it into `scr`; the Box
    // heap allocation is stable across the move, so the pointer stays valid.
    let self_ptr: *mut CategoriesPanel = this.as_mut();
    // SAFETY: `scr` is the self-referential back-pointer (it will own `this`).
    ScreenManager_add(unsafe { &mut *scr }, this, 16);
    // `this` now lives in `scr->panels`; dispatch the first page through the
    // still-valid raw pointer (the C `categoriesPanelPages[0].ctor(this)`).
    // SAFETY: `self_ptr` points at the just-added, `scr`-owned panel.
    // htoprs: guard on `available` for the same reason as the eventHandler — a
    // not-yet-ported first page must not panic on open (page[0] is Display
    // options today, so this is defensive).
    if categoriesPanelPages[0].available {
        (categoriesPanelPages[0].ctor)(unsafe { &mut *self_ptr });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::action::State;
    use crate::ported::listitem::ListItem;
    use crate::ported::object::Object;
    use crate::ported::panel::{Panel_add, Panel_new};
    use crate::ported::screenmanager::ScreenManager_new;
    use crate::ported::settings::{HeaderLayout, ScreenSettings};

    /// A `Panel` populated with the five (non-PCP) category rows (matching
    /// [`categoriesPanelPages`]), so the graphic-typing fall-through has real
    /// `ListItem` values to search.
    fn categories_panel() -> Panel {
        let mut p = Panel_new(1, 1, 20, 10, None);
        for page in categoriesPanelPages.iter() {
            let li: Box<dyn Object> = Box::new(ListItem::new_row(page.name));
            Panel_add(&mut p, li);
        }
        p
    }

    // Local test helper: build a ListItem via its public fields.
    impl ListItem {
        fn new_row(value: &str) -> ListItem {
            ListItem {
                value: value.to_string(),
                key: 0,
                moving: false,
            }
        }
    }

    fn state() -> State {
        State {
            pauseUpdate: false,
            hideSelection: false,
            hideMeters: false,
            host: core::ptr::null_mut(),
            mainPanel: core::ptr::null_mut(),
            header: core::ptr::null_mut(),
            failedUpdate: None,
        }
    }

    /// A `ScreenManager` wired with a header, a state, and one placeholder
    /// panel — enough for the `make*Page` builders reached by the HANDLED tail
    /// (`ScreenManager_add`/`_resize` read `state`/`header`/`panels[0]`).
    fn scr_wired() -> ScreenManager {
        let header = crate::ported::header::Header {
            host: core::ptr::null(),
            columns: vec![Vec::new()],
            headerLayout: HeaderLayout::HF_ONE_100,
            pad: 0,
            height: 0,
            headerMargin: false,
            screenTabs: false,
        };
        // `header`/`state` must outlive the returned `scr`, which only aliases
        // them by raw pointer; leak them for the test process's lifetime.
        let header_raw = Box::into_raw(Box::new(header));
        let state_raw = Box::into_raw(Box::new(state()));
        let host_raw = Box::into_raw(Box::new(Machine::default()));
        let mut scr = ScreenManager_new(header_raw, host_raw, state_raw);
        scr.panelCount = 1;
        scr.panels.push(Box::new(Panel_new(0, 0, 10, 5, None)));
        scr
    }

    /// A `Machine` whose `settings` carry one active screen — the config the
    /// `make*Page` sub-panel constructors read.
    fn host_wired() -> Machine {
        Machine {
            existingCPUs: 1,
            settings: Some(Settings {
                hLayout: HeaderLayout::HF_ONE_100,
                screens: vec![ScreenSettings {
                    heading: Some("Main".to_string()),
                    ..Default::default()
                }],
                ssIndex: 0,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn cat_with(super_: Panel, scr: *mut ScreenManager, host: *mut Machine) -> CategoriesPanel {
        CategoriesPanel {
            super_,
            scr,
            host,
            header: core::ptr::null_mut(),
        }
    }

    // ── result-only paths (IGNORED: the HANDLED tail never runs, null scr) ──

    #[test]
    fn navigation_that_does_not_move_selection_is_ignored() {
        let mut c = cat_with(
            categories_panel(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        // Already at the top row: KEY_UP clamps to 0, unchanged -> IGNORED.
        let r = CategoriesPanel_eventHandler(&mut c, KEY_UP);
        assert_eq!(r, HandlerResult::IGNORED);
        assert_eq!(Panel_getSelectedIndex(&c.super_), 0);
    }

    #[test]
    fn q_on_empty_buffer_break_loop_is_downgraded_to_ignored() {
        let mut c = cat_with(
            categories_panel(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        // 'q' on the empty type-to-search buffer -> BREAK_LOOP, downgraded to
        // IGNORED (CategoriesPanel.c:148-149).
        let r = CategoriesPanel_eventHandler(&mut c, 'q' as i32);
        assert_eq!(r, HandlerResult::IGNORED);
    }

    #[test]
    fn nongraphic_non_navigation_char_is_ignored() {
        let mut c = cat_with(
            categories_panel(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        // Ctrl-B is not a CategoriesPanel navigation key and is not graphic.
        let r = CategoriesPanel_eventHandler(&mut c, KEY_CTRL(b'B' as i32));
        assert_eq!(r, HandlerResult::IGNORED);
        assert_eq!(Panel_getSelectedIndex(&c.super_), 0);
    }

    // ── HANDLED paths (the tail rebuilds a page through the wired scr/host) ──

    #[test]
    fn event_set_selected_is_handled_and_builds_display_page() {
        let mut scr = scr_wired();
        let mut host = host_wired();
        let mut c = cat_with(categories_panel(), &mut scr, &mut host);
        // Selection stays 0 (Display options) -> Display page built.
        let r = CategoriesPanel_eventHandler(&mut c, EVENT_SET_SELECTED);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(scr.panelCount, 2, "the Display page was added");
    }

    #[test]
    fn key_down_builds_header_page() {
        let mut scr = scr_wired();
        let mut host = host_wired();
        let mut c = cat_with(categories_panel(), &mut scr, &mut host);
        let r = CategoriesPanel_eventHandler(&mut c, KEY_DOWN);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&c.super_), 1); // Header layout
        assert_eq!(scr.panelCount, 2, "the Header page was added");
    }

    #[test]
    fn navigating_to_meters_builds_the_page_without_panicking() {
        // Regression: the Meters page ctor used to be a `todo!()` and arrowing
        // to it panicked the TUI. It is now ported (header column moved into a
        // MetersPanel, restored on Drop). Navigating to it (index 2) builds one
        // MetersPanel per header column plus the AvailableMetersPanel — HANDLED,
        // no panic.
        let mut scr = scr_wired();
        let mut host = host_wired();
        let mut c = cat_with(categories_panel(), &mut scr, &mut host);
        // The real CategoriesPanel_new wires `header`; cat_with leaves it null,
        // and the Meters page needs it. `scr.header` is the wired test header.
        c.header = scr.header;
        CategoriesPanel_eventHandler(&mut c, KEY_DOWN); // 0 -> 1 Header (built)
        let r = CategoriesPanel_eventHandler(&mut c, KEY_DOWN); // 1 -> 2 Meters (built)
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&c.super_), 2);
        // 1 placeholder + 1 column MetersPanel + 1 AvailableMetersPanel.
        assert_eq!(scr.panelCount, 3, "the Meters page must build its panels");
    }

    #[test]
    fn make_screen_tabs_page_adds_tabs_and_names_panels() {
        // "Screen tabs" is #if HTOP_PCP in the page registry, so it is never
        // reached via the eventHandler dispatch; call the always-defined ctor
        // directly. It adds the ScreenTabsPanel (size 20) and its `names`
        // sub-panel (size -1) to the manager — two panels.
        use crate::ported::hashtable::Hashtable_new;
        let mut scr = scr_wired();
        let before = scr.panelCount;
        // ScreenTabsPanel_new requires settings->dynamicScreens != NULL.
        let ds = Box::into_raw(Box::new(Hashtable_new(10, true)));
        let mut host = Machine {
            existingCPUs: 1,
            settings: Some(Settings {
                dynamicScreens: Some(ds),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut c = cat_with(categories_panel(), &mut scr, &mut host);

        CategoriesPanel_makeScreenTabsPage(&mut c);

        assert_eq!(scr.panelCount, before + 2, "tabs + names panels were added");
        assert_eq!(scr.panels.len(), (before + 2) as usize);
    }

    #[test]
    fn meters_page_moves_column_out_and_drop_restores_it() {
        // The header→panel→header meter round-trip: a header column with one
        // meter is moved into the MetersPanel (header column emptied while the
        // page is open), then restored by the panel's Drop when the page closes.
        use crate::ported::meter::Meter;
        let mut scr = scr_wired();
        let header = scr.header;
        // Seed the wired header's single column with one meter.
        unsafe {
            let h = &mut *header;
            h.columns = vec![vec![Meter {
                uiName: "CPU",
                ..Meter::empty()
            }]];
        }
        let mut host = host_wired();
        let mut c = cat_with(categories_panel(), &mut scr, &mut host);
        c.header = header;

        // Walk to Meters and back off it, exercising build + Drop-restore.
        CategoriesPanel_eventHandler(&mut c, KEY_DOWN); // Header (index 1)
        CategoriesPanel_eventHandler(&mut c, KEY_DOWN); // Meters (index 2) — column moved out
        assert!(
            unsafe { &*header }.columns[0].is_empty(),
            "the column is moved into the MetersPanel while the page is open"
        );
        CategoriesPanel_eventHandler(&mut c, KEY_UP); // back to Header — Meters page dropped
        assert_eq!(
            unsafe { &*header }.columns[0].len(),
            1,
            "MetersPanel Drop must restore the column into the header"
        );
    }

    #[test]
    fn ctrl_n_moves_selection_like_key_down() {
        let mut scr = scr_wired();
        let mut host = host_wired();
        let mut c = cat_with(categories_panel(), &mut scr, &mut host);
        let r = CategoriesPanel_eventHandler(&mut c, CTRL_N);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&c.super_), 1);
    }

    #[test]
    fn key_end_then_home_rebuilds_and_removes_prior_page() {
        let mut scr = scr_wired();
        let mut host = host_wired();
        let mut c = cat_with(categories_panel(), &mut scr, &mut host);
        // KEY_END -> last row (Colors, index 4) -> Colors page built.
        let r = CategoriesPanel_eventHandler(&mut c, KEY_END);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&c.super_), 4);
        assert_eq!(scr.panelCount, 2);
        // KEY_HOME -> row 0 (Display). The tail removes the Colors page first,
        // then rebuilds, so the count stays at 2 (list + one page).
        let r = CategoriesPanel_eventHandler(&mut c, KEY_HOME);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&c.super_), 0);
        assert_eq!(scr.panelCount, 2);
    }

    #[test]
    fn graphic_char_type_selects_header_and_builds_it() {
        let mut scr = scr_wired();
        let mut host = host_wired();
        let mut c = cat_with(categories_panel(), &mut scr, &mut host);
        // 'H' matches "Header layout" (index 1) via Panel_selectByTyping.
        let r = CategoriesPanel_eventHandler(&mut c, 'H' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&c.super_), 1);
        assert_eq!(scr.panelCount, 2);
    }

    #[test]
    fn graphic_char_with_no_match_is_still_handled_and_rebuilds() {
        let mut scr = scr_wired();
        let mut host = host_wired();
        let mut c = cat_with(categories_panel(), &mut scr, &mut host);
        // 'z' matches no row; Panel_selectByTyping still returns HANDLED and
        // leaves the selection at 0 (Display) -> Display page built.
        let r = CategoriesPanel_eventHandler(&mut c, 'z' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(Panel_getSelectedIndex(&c.super_), 0);
        assert_eq!(scr.panelCount, 2);
    }
}
