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
//! # Stubbed (blocked on specific unported substrate)
//!
//! - [`CategoriesPanel_makeMetersPage`] (`CategoriesPanel.c:43`) — builds one
//!   `MetersPanel_new(settings, title, this->header->columns[i], scr)` per
//!   header column, but the C `MetersPanel*` **shares** the header's
//!   `Vector*` meter store; the ported [`crate::ported::header::Header`]
//!   models `columns` as an owned `Vec<Vec<Meter>>` and
//!   [`crate::ported::meterspanel::MetersPanel`] *owns* its `Vector` meters —
//!   there is no shared-ownership bridge, so the panel cannot be built without
//!   moving the header's meters out (breaking the header).
//! - [`CategoriesPanel_makeScreenTabsPage`] (`CategoriesPanel.c:78`) — PCP-only
//!   in C (`#if defined(HTOP_PCP)`), and it hands the `ScreenTabsPanel`'s owned
//!   `names` sub-panel to a separate `ScreenManager_add`, which the
//!   owned-sub-panel model cannot split off. Not in the non-PCP page table.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::ported::colorspanel::ColorsPanel_new;
use crate::ported::crt::{KEY_CTRL, KEY_DOWN, KEY_END, KEY_HOME, KEY_NPAGE, KEY_PPAGE, KEY_UP};
use crate::ported::displayoptionspanel::DisplayOptionsPanel_new;
use crate::ported::functionbar::FunctionBar_new;
use crate::ported::header::Header;
use crate::ported::headeroptionspanel::HeaderOptionsPanel_new;
use crate::ported::listitem::ListItem_new;
use crate::ported::machine::Machine;
use crate::ported::panel::{
    HandlerResult, Panel, PanelClass, Panel_add, Panel_getSelectedIndex, Panel_new, Panel_onKey,
    Panel_selectByTyping, Panel_setHeader, EVENT_SET_SELECTED,
};
use crate::ported::screenmanager::{
    ScreenManager, ScreenManager_add, ScreenManager_remove, ScreenManager_size,
};
use crate::ported::screenspanel::ScreensPanel_new;
use crate::ported::settings::Settings;

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
struct CategoriesPanelPage {
    name: &'static str,
    ctor: CategoriesPanel_makePageFunc,
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
    },
    CategoriesPanelPage {
        name: "Header layout",
        ctor: CategoriesPanel_makeHeaderOptionsPage,
    },
    CategoriesPanelPage {
        name: "Meters",
        ctor: CategoriesPanel_makeMetersPage,
    },
    CategoriesPanelPage {
        name: "Screens",
        ctor: CategoriesPanel_makeScreensPage,
    },
    CategoriesPanelPage {
        name: "Colors",
        ctor: CategoriesPanel_makeColorsPage,
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

/// TODO: port of `static void CategoriesPanel_makeMetersPage(CategoriesPanel*
/// this)` from `CategoriesPanel.c:43`. Blocked: the C builds one
/// `MetersPanel_new(settings, title, this->header->columns[i], this->scr)` per
/// header column, sharing the header's `Vector*` meter store with the panel.
/// The ported [`Header`] models `columns` as an owned `Vec<Vec<Meter>>` and
/// [`crate::ported::meterspanel::MetersPanel`] *owns* its `Vector` meters, so
/// the panel can't alias the header's column — building it would move the
/// header's meters out (breaking the header). Missing substrate: a
/// shared-ownership bridge between `Header.columns[i]` and `MetersPanel.meters`.
pub fn CategoriesPanel_makeMetersPage(this: &mut CategoriesPanel) {
    let _ = this;
    todo!("port of CategoriesPanel.c:43 — Header.columns is Vec<Vec<Meter>> (owned) but MetersPanel_new needs the shared Vector* meter store the header and panel co-own; no shared-ownership bridge")
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

/// TODO: port of `static void CategoriesPanel_makeScreenTabsPage(CategoriesPanel*
/// this)` from `CategoriesPanel.c:78`. PCP-only in C (`#if defined(HTOP_PCP)`)
/// and absent from the non-PCP `categoriesPanelPages`, so it is never
/// dispatched in this build. Blocked anyway: the C hands the
/// `ScreenTabsPanel`'s owned `names` sub-panel (`((ScreenTabsPanel*)screenTabs)
/// ->names`) to a separate `ScreenManager_add`, which the owned-sub-panel model
/// (the `ScreenTabsPanel` owns `names`) cannot split into two independently
/// added `Box<dyn PanelClass>`s.
pub fn CategoriesPanel_makeScreenTabsPage(this: &mut CategoriesPanel) {
    let _ = this;
    todo!("port of CategoriesPanel.c:78 — PCP-only; also needs to split the ScreenTabsPanel's owned `names` sub-panel into a separately ScreenManager_add-able panel (owned-sub-panel model can't)")
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
        if selected >= 0 && (selected as usize) < categoriesPanelPages.len() {
            (categoriesPanelPages[selected as usize].ctor)(this);
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
    (categoriesPanelPages[0].ctor)(unsafe { &mut *self_ptr });
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
        let mut scr = ScreenManager_new(Some(header), Machine::default(), state());
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
