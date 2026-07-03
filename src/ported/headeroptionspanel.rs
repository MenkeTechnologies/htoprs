//! Partial port of `HeaderOptionsPanel.c` — the Setup-screen "Header Layout"
//! chooser (the panel of radio-style [`CheckItem`]
//! rows that pick how many meter columns the header shows and their widths).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
//!
//! # Data model
//!
//! htop's `struct HeaderOptionsPanel_` (`HeaderOptionsPanel.h:15`) is a
//! `Panel super` plus two non-owning back-pointers (`ScreenManager* scr`,
//! `Settings* settings`). It is modeled here as [`HeaderOptionsPanel`] with the
//! embedded panel as `super_` and the two back-pointers as raw `*mut` pointers —
//! the same `ScreensPanel`/`ColumnsPanel`/`ColorsPanel` idiom already used for
//! every other Setup sub-panel (see `colorspanel.rs` / `screentabspanel.rs`).
//! The `scr` pointer is the self-referential cycle noted in `categoriespanel.rs`
//! (the panel is added to `scr` by `ScreenManager_add(scr, super, 16)`, so
//! `scr`'s `panels` owns the very panel that points back at `scr`); a raw
//! pointer sidesteps the ownership question, exactly as the sibling panels do.
//!
//! # Ported
//!
//! - [`HeaderOptionsPanel_eventHandler`] (`HeaderOptionsPanel.c:33`) — the
//!   Enter/Space/click arm clears every [`CheckItem`]
//!   and sets the marked one, then applies the chosen layout through the
//!   `scr`/`settings` back-pointers (`Header_setLayout`, `settings->changed`/
//!   `lastUpdate`, `ScreenManager_resize` — all ported).
//!
//! # Stubbed
//!
//! - [`HeaderOptionsPanel_delete`] — C body is `Panel_done(&this->super);
//!   free(this);`, released by `Drop` in Rust (same rationale as
//!   `Panel_delete`/`Panel_done` and every other `*Panel_delete`), so there is
//!   no algorithm to port.
//! - [`HeaderOptionsPanel_new`] — blocked on the `HeaderLayout_layouts[]`
//!   description table. `_new` labels each `CheckItem` row with
//!   `HeaderLayout_layouts[i].description` (`HeaderLayout.h:40`), but only the
//!   [`HeaderLayout`] enum and
//!   `HeaderLayout_getColumns` are ported (in `settings.rs`); the
//!   `HeaderLayout_layouts` table itself — with its `name`/`description`/`widths`
//!   columns — has no ported analog (see the note at `header.rs`, which defers
//!   the same table's `widths[]`). Reproducing the row labels would mean
//!   reinventing that table rather than porting it, so the constructor is left
//!   stubbed until the table lands in a ported `HeaderLayout` module.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;

use crate::ported::crt::{KEY_ENTER, KEY_MOUSE, KEY_RECLICK};
use crate::ported::functionbar::FunctionBar_new;
use crate::ported::header::Header_setLayout;
use crate::ported::optionitem::{CheckItem, CheckItem_newByVal, CheckItem_set};
use crate::ported::panel::{
    HandlerResult, Panel, PanelClass, Panel_add, Panel_done, Panel_getSelectedIndex, Panel_new,
    Panel_setHeader,
};
use crate::ported::screenmanager::{ScreenManager, ScreenManager_resize};
use crate::ported::settings::{HeaderLayout, HeaderLayout_layouts, Settings};

/// Port of the file-scope `static const char* const HeaderOptionsFunctions[]`
/// from `HeaderOptionsPanel.c:24`. Nine blank slots followed by `"Done  "`;
/// the C trailing `NULL` sentinel is dropped (the ported `FunctionBar_new` is
/// length-bounded, not NUL-terminated).
static HeaderOptionsFunctions: [&str; 10] = [
    "      ", "      ", "      ", "      ", "      ", "      ", "      ", "      ", "      ",
    "Done  ",
];

/// Reduced model of the C `HeaderOptionsPanel` struct
/// (`HeaderOptionsPanel.h:15`): the embedded `Panel super` (as `super_`) and the
/// two non-owning back-pointers (`ScreenManager* scr`, `Settings* settings`) the
/// event handler mutates, stored as raw `*mut` pointers (the `ScreensPanel`/
/// `ColorsPanel` idiom — both the `ScreenManager` and the `Settings` are owned
/// elsewhere, and `scr` is the self-referential cycle described in the module
/// docs).
pub struct HeaderOptionsPanel {
    /// C `Panel super` — the embedded panel base.
    pub super_: Panel,
    /// C `ScreenManager* scr` — non-owning back-pointer to the screen manager
    /// whose header the handler re-lays-out (`this->scr->header`) and resizes.
    pub scr: *mut ScreenManager,
    /// C `Settings* settings` — non-owning back-pointer to the settings the
    /// handler marks `changed` / bumps `lastUpdate` on.
    pub settings: *mut Settings,
}

/// Port of `const PanelClass HeaderOptionsPanel_class`
/// (`HeaderOptionsPanel.c:66`): sets only `.eventHandler =
/// HeaderOptionsPanel_eventHandler`; `.drawFunctionBar` / `.printHeader` are
/// NULL, so those slots inherit the `Panel` defaults.
impl PanelClass for HeaderOptionsPanel {
    fn as_panel(&self) -> &Panel {
        &self.super_
    }
    fn as_panel_mut(&mut self) -> &mut Panel {
        &mut self.super_
    }
    fn event_handler(&mut self, ev: i32) -> HandlerResult {
        HeaderOptionsPanel_eventHandler(self, ev)
    }
}

/// Port of `static void HeaderOptionsPanel_delete(Object* object)` from
/// `HeaderOptionsPanel.c:27`: `Panel_done(&this->super); free(this);`.
/// Taking `this` by value consumes the panel; the embedded `super_`
/// [`Panel`] is handed to [`Panel_done`] (mirroring the C call graph), and
/// the non-owning `scr`/`settings` back-pointers drop with the struct free.
pub fn HeaderOptionsPanel_delete(this: HeaderOptionsPanel) {
    let HeaderOptionsPanel {
        super_,
        scr,
        settings,
    } = this;
    Panel_done(super_);
    let _ = scr;
    let _ = settings;
}

/// Port of `static HandlerResult HeaderOptionsPanel_eventHandler(Panel* super,
/// int ch)` from `HeaderOptionsPanel.c:33`.
///
/// On the Enter/newline/carriage-return/mouse/reclick/space keys the handler
/// treats the panel as a radio group: it clears every one of the
/// `LAST_HEADER_LAYOUT` [`CheckItem`] rows
/// (via [`CheckItem_set`]`(.., false)`) and sets the selected one, then applies
/// the picked layout — `Header_setLayout(this->scr->header, mark)` (the C
/// implicit `int`→`HeaderLayout` cast is spelled out as the [`HeaderLayout`]
/// match below, valid under the two asserts), `this->settings->changed = true`
/// / `this->settings->lastUpdate++`, and finally `ScreenManager_resize(this->scr)`
/// — all reached through the `scr`/`settings` back-pointers. Every other key is
/// `IGNORED`.
///
/// Following the `ScreensPanel`/`ScreenTabsPanel`/`ColumnsPanel` port
/// convention, the C `Panel* super` (upcast to `HeaderOptionsPanel*`) becomes
/// the reduced-struct receiver `this: &mut HeaderOptionsPanel`; `this.super_` is
/// the embedded panel. The mutable `(CheckItem*)Panel_get(super, i)` writes are
/// reproduced by indexing `this.super_.items[i]` and downcasting the
/// `&mut dyn Object` to `&mut CheckItem` via the `Any` supertrait (ported
/// `Panel_get` hands back an immutable `&dyn Object`), the same mutating analog
/// `ColorsPanel_new` uses.
pub fn HeaderOptionsPanel_eventHandler(this: &mut HeaderOptionsPanel, ch: i32) -> HandlerResult {
    let mut result = HandlerResult::IGNORED;

    match ch {
        // 0x0a (LF), 0x0d (CR), KEY_ENTER, KEY_MOUSE, KEY_RECLICK, ' ' (0x20).
        0x0a | 0x0d | KEY_ENTER | KEY_MOUSE | KEY_RECLICK | 0x20 => {
            let mark = Panel_getSelectedIndex(&this.super_);
            debug_assert!(mark >= 0);
            debug_assert!((mark as usize) < HeaderLayout::LAST_HEADER_LAYOUT as usize);

            for i in 0..(HeaderLayout::LAST_HEADER_LAYOUT as usize) {
                let any: &mut dyn Any = this.super_.items[i].object_mut();
                if let Some(item) = any.downcast_mut::<CheckItem>() {
                    CheckItem_set(item, false);
                }
            }
            let any: &mut dyn Any = this.super_.items[mark as usize].object_mut();
            if let Some(item) = any.downcast_mut::<CheckItem>() {
                CheckItem_set(item, true);
            }

            // C: Header_setLayout(this->scr->header, mark) — the implicit
            // (HeaderLayout)mark cast, valid under the asserts above.
            let layout = match mark {
                0 => HeaderLayout::HF_ONE_100,
                1 => HeaderLayout::HF_TWO_50_50,
                2 => HeaderLayout::HF_TWO_33_67,
                3 => HeaderLayout::HF_TWO_67_33,
                4 => HeaderLayout::HF_THREE_33_34_33,
                5 => HeaderLayout::HF_THREE_25_25_50,
                6 => HeaderLayout::HF_THREE_25_50_25,
                7 => HeaderLayout::HF_THREE_50_25_25,
                8 => HeaderLayout::HF_THREE_40_30_30,
                9 => HeaderLayout::HF_THREE_30_40_30,
                10 => HeaderLayout::HF_THREE_30_30_40,
                11 => HeaderLayout::HF_THREE_40_20_40,
                12 => HeaderLayout::HF_FOUR_25_25_25_25,
                _ => unreachable!("mark out of [0, LAST_HEADER_LAYOUT) range"),
            };

            // SAFETY: `scr`/`settings` are the non-owning back-pointers stored at
            // construction (`HeaderOptionsPanel_new`); the `ScreenManager` and
            // `Settings` they alias outlive this panel (which the `ScreenManager`
            // itself owns). They alias distinct objects, so the two `&mut`s below
            // do not overlap.
            let scr = unsafe { &mut *this.scr };
            // SAFETY: scr->header outlives this panel; NULL only before wiring.
            let header = unsafe { scr.header.as_mut() }
                .expect("HeaderOptionsPanel_eventHandler: scr->header is NULL");
            Header_setLayout(header, layout);

            let settings = unsafe { &mut *this.settings };
            settings.changed = true;
            settings.lastUpdate += 1;

            ScreenManager_resize(scr);

            result = HandlerResult::HANDLED;
        }
        _ => {}
    }

    result
}

/// Port of `HeaderOptionsPanel* HeaderOptionsPanel_new(Settings* settings,
/// ScreenManager* scr)` from `HeaderOptionsPanel.c:74`.
///
/// Builds a `1×1` [`Panel`] with the `HeaderOptionsFunctions` `FunctionBar`,
/// stores the `scr`/`settings` back-pointers, sets the "Header Layout" header,
/// then appends one `CheckItem_newByVal(HeaderLayout_layouts[i].description,
/// false)` for each of the `LAST_HEADER_LAYOUT` layouts and checks the row
/// matching the active `scr->header->headerLayout`.
///
/// The C `Class(CheckItem)`/`owner` args to `Panel_init` type the underlying
/// `Vector`; the ported `Panel_new` drops them (a `Vec<Box<dyn Object>>` needs
/// no such typing), matching every sibling panel port. The final
/// `CheckItem_set((CheckItem*)Panel_get(super, headerLayout), true)` write is
/// reproduced by indexing `super_.items[headerLayout]` and downcasting via the
/// `Any` supertrait, the same mutating analog `ColorsPanel_new` uses.
pub fn HeaderOptionsPanel_new(
    settings: *mut Settings,
    scr: *mut ScreenManager,
) -> HeaderOptionsPanel {
    let fuBar = FunctionBar_new(Some(&HeaderOptionsFunctions[..]), None, None);
    let super_ = Panel_new(1, 1, 1, 1, Some(fuBar));

    let mut this = HeaderOptionsPanel {
        super_,
        scr,
        settings,
    };

    Panel_setHeader(&mut this.super_, "Header Layout");
    for i in 0..(HeaderLayout::LAST_HEADER_LAYOUT as usize) {
        Panel_add(
            &mut this.super_,
            Box::new(CheckItem_newByVal(
                HeaderLayout_layouts[i].description,
                false,
            )),
        );
    }

    // C: CheckItem_set((CheckItem*)Panel_get(super, scr->header->headerLayout), true);
    // SAFETY: `scr` is the non-owning back-pointer just stored; the
    // `ScreenManager` it aliases outlives this panel, and its `header` is
    // always present at construction.
    // SAFETY: scr and scr->header are caller-owned and outlive this panel.
    let headerLayout = unsafe { (*scr).header.as_ref() }
        .expect("HeaderOptionsPanel_new: scr->header is NULL")
        .headerLayout as usize;
    let any: &mut dyn Any = this.super_.items[headerLayout].object_mut();
    if let Some(item) = any.downcast_mut::<CheckItem>() {
        CheckItem_set(item, true);
    }

    this
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::action::State;
    use crate::ported::machine::Machine;
    use crate::ported::optionitem::{CheckItem_get, CheckItem_newByVal};
    use crate::ported::panel::{Panel_add, Panel_new, Panel_setSelected};
    use crate::ported::screenmanager::ScreenManager_new;

    /// A `State` with the display toggles off (only `hideMeters` is read by the
    /// layout ops reached through `ScreenManager_resize`).
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

    /// A `Settings` carrying only the fields the handler touches.
    fn settings() -> Settings {
        Settings {
            hLayout: HeaderLayout::HF_ONE_100,
            hColumns: Vec::new(),
            screens: Vec::new(),
            ssIndex: 0,
            changed: false,
            lastUpdate: 0,
            ..Default::default()
        }
    }

    /// A `Header` starting at the single-column layout (`columns.len()` must
    /// equal `HeaderLayout_getColumns(headerLayout)`, so one empty column).
    fn header() -> crate::ported::header::Header {
        crate::ported::header::Header {
            host: core::ptr::null(),
            columns: vec![Vec::new()],
            headerLayout: HeaderLayout::HF_ONE_100,
            pad: 0,
            height: 0,
            headerMargin: false,
            screenTabs: false,
        }
    }

    /// A `HeaderOptionsPanel`-flavored `Panel`: one `CheckItem` per header
    /// layout (`LAST_HEADER_LAYOUT` rows), all unchecked.
    fn options_panel() -> Panel {
        let mut p = Panel_new(1, 1, 20, 10, None);
        for _ in 0..(HeaderLayout::LAST_HEADER_LAYOUT as usize) {
            Panel_add(&mut p, Box::new(CheckItem_newByVal("row", false)));
        }
        p
    }

    fn item_checked(p: &Panel, i: usize) -> bool {
        let any: &dyn Any = p.items[i].object();
        CheckItem_get(any.downcast_ref::<CheckItem>().unwrap())
    }

    #[test]
    fn enter_applies_selected_layout_and_marks_only_that_row() {
        let mut scr = ScreenManager_new(Some(header()), Machine::default(), state());
        // ScreenManager_resize reads panels[panelCount - 1]; give it one panel.
        scr.panelCount = 1;
        scr.panels.push(Box::new(Panel_new(0, 0, 10, 5, None)));
        let mut set = settings();

        let mut this = HeaderOptionsPanel {
            super_: options_panel(),
            scr: &mut scr as *mut ScreenManager,
            settings: &mut set as *mut Settings,
        };
        // Select index 2 == HF_TWO_33_67.
        Panel_setSelected(&mut this.super_, 2);

        let r = HeaderOptionsPanel_eventHandler(&mut this, ' ' as i32);
        assert_eq!(r, HandlerResult::HANDLED);

        // Only row 2 is checked.
        for i in 0..(HeaderLayout::LAST_HEADER_LAYOUT as usize) {
            assert_eq!(item_checked(&this.super_, i), i == 2, "row {i} check state");
        }
        // The chosen layout is applied to the header via the scr back-pointer.
        assert_eq!(
            scr.header.as_ref().unwrap().headerLayout,
            HeaderLayout::HF_TWO_33_67
        );
        // Settings marked changed and lastUpdate bumped via the settings pointer.
        assert!(set.changed);
        assert_eq!(set.lastUpdate, 1);
    }

    #[test]
    fn non_activation_key_is_ignored() {
        let mut scr = ScreenManager_new(Some(header()), Machine::default(), state());
        scr.panelCount = 1;
        scr.panels.push(Box::new(Panel_new(0, 0, 10, 5, None)));
        let mut set = settings();

        let mut this = HeaderOptionsPanel {
            super_: options_panel(),
            scr: &mut scr as *mut ScreenManager,
            settings: &mut set as *mut Settings,
        };
        Panel_setSelected(&mut this.super_, 2);

        // 'x' is not an activation key: nothing is checked, nothing mutated.
        let r = HeaderOptionsPanel_eventHandler(&mut this, 'x' as i32);
        assert_eq!(r, HandlerResult::IGNORED);
        for i in 0..(HeaderLayout::LAST_HEADER_LAYOUT as usize) {
            assert!(!item_checked(&this.super_, i));
        }
        assert!(!set.changed);
        assert_eq!(set.lastUpdate, 0);
        assert_eq!(
            scr.header.as_ref().unwrap().headerLayout,
            HeaderLayout::HF_ONE_100
        );
    }
}
