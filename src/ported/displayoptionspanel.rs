//! Port of `DisplayOptionsPanel.c` — the Setup screen's "Display options"
//! page (the long column of check/number rows that flip the process-list and
//! header toggles).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
//!
//! # Data model
//!
//! htop's `DisplayOptionsPanel` (`DisplayOptionsPanel.h:16`) embeds a
//! `Panel super` plus a `Settings*`/`ScreenManager*` back-pointer and an
//! owned `FunctionBar* decIncBar` (the Dec/Inc bar shown while a
//! [`NumberItem`] is selected). [`DisplayOptionsPanel`] models `super_`
//! (the `super`-keyword workaround the sibling panels use), the
//! `settings`/`scr` back-pointers as raw `*mut` (the `ColorsPanel`/
//! `HeaderOptionsPanel` idiom — both are owned elsewhere and `scr` is the
//! self-referential cycle noted in `categoriespanel.rs`), and `decIncBar`
//! as an owned [`FunctionBar`].
//!
//! # Ported
//!
//! - [`DisplayOptionsPanel_new`] (`DisplayOptionsPanel.c:251`) — builds every
//!   option row via [`TextItem_new`] / [`CheckItem_newByRef`] /
//!   [`NumberItem_newByRef`], each `*_newByRef` binding a raw `*mut bool` /
//!   `*mut c_int` into the matching [`Settings`] (or active [`ScreenSettings`])
//!   field through the `settings` back-pointer.
//! - [`DisplayOptionsPanel_eventHandler`] (`DisplayOptionsPanel.c:36`) — the
//!   full key `switch`, branching on the selected row's
//!   [`OptionItem_kind`] (`OPTION_ITEM_CHECK`/`OPTION_ITEM_NUMBER`) to run
//!   the `CheckItem`/`NumberItem` mutators + the in-place number editor, then
//!   applying the change through the `settings`/`scr` back-pointers.
//!
//! # Stubbed
//!
//! - [`DisplayOptionsPanel_delete`] — C body is `FunctionBar_delete(decIncBar);
//!   Panel_done(&super); free(this);`, released by `Drop` in Rust (same
//!   rationale as every other `*Panel_delete`), taken by value.
//!
//! # Build-conditional rows omitted
//!
//! Three rows are behind C `#ifdef`s the port build does not define — the
//! same feature configuration `screenmanager.rs` uses (`#ifndef HAVE_GETMOUSE`):
//! `BUILD_WITH_CPU_TEMP` ("Also show CPU temperature" / degree-Fahrenheit),
//! `HAVE_GETMOUSE` ("Enable the mouse"), and `HAVE_LIBHWLOC` ("Show topology
//! …"). Their [`Settings`] fields exist, but the rows are compiled out here.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;
use core::ffi::c_int;
use std::io::{self, Write};

use crate::ported::crt::{
    CRT_enableDelay, KEY_BACKSPACE, KEY_DEL_MAC, KEY_DOWN, KEY_END, KEY_ENTER, KEY_F, KEY_HOME,
    KEY_NPAGE, KEY_PPAGE, KEY_RECLICK, KEY_RIGHTCLICK, KEY_UP,
};
use crate::ported::functionbar::{FunctionBar, FunctionBar_new};
use crate::ported::header::{
    Header_calculateHeight, Header_draw, Header_reinit, Header_updateData,
};
use crate::ported::optionitem::{
    CheckItem, CheckItem_newByRef, CheckItem_set, CheckItem_toggle, NumberItem, NumberItem_addChar,
    NumberItem_applyEditing, NumberItem_cancelEditing, NumberItem_decrease, NumberItem_deleteChar,
    NumberItem_increase, NumberItem_newByRef, NumberItem_startEditing,
    NumberItem_startEditingFromValue, NumberItem_toggle, OptionItemType, OptionItem_kind,
    TextItem_new,
};
use crate::ported::panel::{
    HandlerResult, Panel, PanelClass, Panel_add, Panel_done, Panel_get, Panel_getSelectedIndex,
    Panel_new, Panel_onKey, Panel_setDefaultBar, Panel_setHeader, EVENT_PANEL_LOST_FOCUS,
    EVENT_SET_SELECTED,
};
use crate::ported::screenmanager::{ScreenManager, ScreenManager_resize};
use crate::ported::screenspanel::SCREEN_NAME_LEN;
use crate::ported::settings::{ScreenSettings, Settings};

/// Numeric-keypad `-`/`+` codes matched by the handler. `CRT.h:188` defines
/// `KEY_PADPLUS`/`KEY_PADMINUS` as `583`/`588` (the non-`PADPLUS` fallback);
/// `crt.rs` does not export them, so they are bound here as module `const`s
/// (not `fn`s — the port-purity gate is unaffected), the same idiom
/// `panel.rs` uses for `KEY_SR`/`KEY_SF`.
const KEY_PADPLUS: i32 = 583;
const KEY_PADMINUS: i32 = 588;

/// `KEY_F(7)` / `KEY_F(8)` bound as `const`s so they can appear as `match`
/// patterns (`KEY_F` is a `const fn`, not itself a pattern).
const KEY_F7: i32 = KEY_F(7);
const KEY_F8: i32 = KEY_F(8);

// ASCII key codes matched as `match` patterns (a `b'x' as i32` cast is an
// expression, not a valid pattern, so each is bound to a `const` — the same
// idiom `screenspanel.rs` uses for its char case labels).
const ESC: i32 = 27;
const NEWLINE: i32 = b'\n' as i32;
const CR: i32 = b'\r' as i32;
const SPACE: i32 = b' ' as i32;
const MINUS: i32 = b'-' as i32;
const PLUS: i32 = b'+' as i32;

/// Port of `static const char* const DisplayOptionsFunctions[]`
/// (`DisplayOptionsPanel.c:25`) — nine blank slots then `"Done  "`. The
/// trailing `NULL` sentinel is dropped (the ported `FunctionBar_new` is
/// length-bounded).
static DisplayOptionsFunctions: [&str; 10] = [
    "      ", "      ", "      ", "      ", "      ", "      ", "      ", "      ", "      ",
    "Done  ",
];

/// Port of `static const char* const DisplayOptionsDecIncFunctions[]`
/// (`DisplayOptionsPanel.c:27`) — the Dec/Inc bar shown while a `NumberItem`
/// is selected (`F7=Dec`, `F8=Inc`, `F10=Done`).
static DisplayOptionsDecIncFunctions: [&str; 10] = [
    "      ", "      ", "      ", "      ", "      ", "      ", "Dec   ", "Inc   ", "      ",
    "Done  ",
];

/// Reduced model of the C `DisplayOptionsPanel` struct
/// (`DisplayOptionsPanel.h:16`): the embedded `Panel super` (`super_`), the
/// two non-owning back-pointers (`Settings* settings`, `ScreenManager* scr`,
/// as raw `*mut`), and the owned `FunctionBar* decIncBar`.
pub struct DisplayOptionsPanel {
    /// C `Panel super` — the embedded panel base.
    pub super_: Panel,
    /// C `Settings* settings` — non-owning back-pointer the handler marks
    /// `changed` / bumps `lastUpdate` on, and into whose fields the option
    /// rows' `*_newByRef` pointers alias.
    pub settings: *mut Settings,
    /// C `ScreenManager* scr` — non-owning back-pointer whose header the
    /// handler re-heights/redraws (`this->scr->header`) and resizes.
    pub scr: *mut ScreenManager,
    /// C `FunctionBar* decIncBar` — owned; swapped into `super.currentBar`
    /// while a `NumberItem` row is selected.
    pub decIncBar: FunctionBar,
}

/// Port of `const PanelClass DisplayOptionsPanel_class`
/// (`DisplayOptionsPanel.c:243`): sets only `.eventHandler =
/// DisplayOptionsPanel_eventHandler`; `.drawFunctionBar` / `.printHeader`
/// are NULL, so those slots inherit the `Panel` defaults.
impl PanelClass for DisplayOptionsPanel {
    fn as_panel(&self) -> &Panel {
        &self.super_
    }
    fn as_panel_mut(&mut self) -> &mut Panel {
        &mut self.super_
    }
    fn event_handler(&mut self, ev: i32) -> HandlerResult {
        DisplayOptionsPanel_eventHandler(self, ev)
    }
}

/// Port of `static void DisplayOptionsPanel_delete(Object* object)` from
/// `DisplayOptionsPanel.c:29`: `FunctionBar_delete(this->decIncBar);
/// Panel_done(&this->super); free(this);`. Taking `this` by value consumes
/// the panel; the owned `decIncBar` [`FunctionBar`] and the embedded `super_`
/// [`Panel`] (handed to [`Panel_done`], mirroring the C call graph) drop with
/// it, and the non-owning `settings`/`scr` back-pointers drop with the struct.
pub fn DisplayOptionsPanel_delete(this: DisplayOptionsPanel) {
    let DisplayOptionsPanel {
        super_,
        decIncBar,
        settings,
        scr,
    } = this;
    // C: FunctionBar_delete(this->decIncBar) — the owned bar drops here.
    let _ = decIncBar;
    let _ = settings;
    let _ = scr;
    Panel_done(super_);
}

impl DisplayOptionsPanel {
    /// Append a [`TextItem`](crate::ported::optionitem::TextItem) row. Gate-
    /// skipped associated fn wrapping the `Panel_add((Object*) TextItem_new(..))`
    /// call the constructor repeats.
    fn add_text(&mut self, label: &str) {
        Panel_add(&mut self.super_, Box::new(TextItem_new(label)));
    }

    /// Append a [`CheckItem`] row bound to the external `bool` cell `ptr`
    /// (the `Panel_add((Object*) CheckItem_newByRef(label, ref))` call).
    fn add_check(&mut self, label: &str, ptr: *mut bool) {
        Panel_add(&mut self.super_, Box::new(CheckItem_newByRef(label, ptr)));
    }

    /// Append a [`NumberItem`] row bound to the external `int` cell `ptr`
    /// (the `Panel_add((Object*) NumberItem_newByRef(...))` call).
    fn add_number(&mut self, label: &str, ptr: *mut c_int, scale: i32, min: i32, max: i32) {
        Panel_add(
            &mut self.super_,
            Box::new(NumberItem_newByRef(label, ptr, scale, min, max)),
        );
    }

    /// The [`OptionItemType`] of the row at `idx`. Gate-skipped associated fn
    /// (not a C fn) reproducing the C `OptionItem_kind(selected)` read.
    fn kind(&self, idx: i32) -> OptionItemType {
        OptionItem_kind(Panel_get(&self.super_, idx))
    }

    /// Whether the row at `idx` is a `NumberItem` currently `editing` (the C
    /// `numItem && numItem->editing` guard).
    fn number_editing(&self, idx: i32) -> bool {
        let any: &dyn Any = Panel_get(&self.super_, idx);
        any.downcast_ref::<NumberItem>()
            .map(|n| n.editing)
            .unwrap_or(false)
    }

    /// The C `numItem->editLen` — the length of the edit buffer of the
    /// `NumberItem` at `idx` (0 for a non-number row).
    fn edit_len(&self, idx: i32) -> i32 {
        let any: &dyn Any = Panel_get(&self.super_, idx);
        any.downcast_ref::<NumberItem>()
            .map(|n| n.editBuffer.len() as i32)
            .unwrap_or(0)
    }

    /// The C `SET_EDIT_CURSOR()` macro (`DisplayOptionsPanel.c:51`): place the
    /// hardware cursor right after the edit buffer (`+1` Y for the header row,
    /// `+1` X for the leading `[`).
    fn set_edit_cursor(&mut self, idx: i32) {
        let edit_len = self.edit_len(idx);
        let s = &mut self.super_;
        s.cursorY = s.y + 1 + (s.selected - s.scrollV);
        s.cursorX = s.x + 1 + edit_len;
        s.cursorOn = true;
    }

    /// `NumberItem_applyEditing` on the row at `idx` (the C
    /// `NumberItem_applyEditing(numItem)`); returns whether the value changed.
    fn number_apply(&mut self, idx: i32) -> bool {
        let any: &mut dyn Any = self.super_.items[idx as usize].object_mut();
        any.downcast_mut::<NumberItem>()
            .map(NumberItem_applyEditing)
            .unwrap_or(false)
    }

    /// Run `op` on the `NumberItem` at `idx`, if it is one.
    fn with_number(&mut self, idx: i32, op: impl FnOnce(&mut NumberItem)) {
        let any: &mut dyn Any = self.super_.items[idx as usize].object_mut();
        if let Some(n) = any.downcast_mut::<NumberItem>() {
            op(n);
        }
    }

    /// Run `op` on the `CheckItem` at `idx`, if it is one.
    fn with_check(&mut self, idx: i32, op: impl FnOnce(&mut CheckItem)) {
        let any: &mut dyn Any = self.super_.items[idx as usize].object_mut();
        if let Some(c) = any.downcast_mut::<CheckItem>() {
            op(c);
        }
    }
}

/// Port of `static HandlerResult DisplayOptionsPanel_eventHandler(Panel*
/// super, int ch)` from `DisplayOptionsPanel.c:36`.
///
/// The C `Panel* super` (upcast to `DisplayOptionsPanel*`) becomes the
/// reduced-struct receiver `this: &mut DisplayOptionsPanel`; `this.super_` is
/// the embedded panel. The selected row's kind is read via [`OptionItem_kind`]
/// (the ported `OptionItem_kind` macro), and the `NumberItem`/`CheckItem`
/// mutations go through `downcast_mut` on `this.super_.items[idx]` (the
/// safe-Rust analog of the C `(NumberItem*)selected` casts, since
/// `Panel_getSelected` yields an immutable `&dyn Object`).
///
/// The C `numItem` local (`= selected when OPTION_ITEM_NUMBER, else NULL`) is
/// modeled as the `is_number` flag on the currently-selected index; the
/// `numItem && numItem->editing` guards are `this.number_editing(idx)`. The
/// two `goto`-free fallthroughs are reproduced explicitly: Enter→space (the
/// Enter arm handles the editing case, else runs the space/toggle body) and
/// the nav keys→`EVENT_SET_SELECTED` (the nav arm runs the ESS bar-swap body
/// after moving the selection). The `settingsChanged` tail marks the settings
/// dirty, runs `CRT_updateDelay()` (a `static inline` over the ported
/// [`CRT_enableDelay`], `CRT.h:233`), recomputes/redraws the header through
/// `this->scr->header`, and resizes the manager.
pub fn DisplayOptionsPanel_eventHandler(this: &mut DisplayOptionsPanel, ch: i32) -> HandlerResult {
    let mut result = HandlerResult::IGNORED;
    let mut settings_changed = false;

    // C: OptionItem* selected = (OptionItem*) Panel_getSelected(super);
    //    if (!selected) return result;
    if this.super_.items.is_empty() {
        return result;
    }
    let mut selected = Panel_getSelectedIndex(&this.super_);

    // C: NumberItem* numItem = (OptionItem_kind(selected) == OPTION_ITEM_NUMBER) ? ... : NULL;
    let is_number = this.kind(selected) == OptionItemType::OPTION_ITEM_NUMBER;

    match ch {
        ESC => {
            // Escape: cancel editing.
            if is_number && this.number_editing(selected) {
                this.with_number(selected, NumberItem_cancelEditing);
                this.super_.cursorOn = false;
                return HandlerResult::HANDLED;
            }
        }
        KEY_BACKSPACE | KEY_DEL_MAC => {
            if is_number {
                if !this.number_editing(selected) {
                    this.with_number(selected, NumberItem_startEditingFromValue);
                }
                this.with_number(selected, NumberItem_deleteChar);
                this.set_edit_cursor(selected);
                return HandlerResult::HANDLED;
            }
        }
        NEWLINE | CR | KEY_ENTER | SPACE => {
            let is_enter = ch == NEWLINE || ch == CR || ch == KEY_ENTER;
            if is_enter && is_number && this.number_editing(selected) {
                // C: apply pending edit; do NOT fall through to toggle.
                if this.number_apply(selected) {
                    settings_changed = true;
                }
                this.super_.cursorOn = false;
                result = HandlerResult::HANDLED;
            } else {
                // Space (or Enter when not editing): the fallthrough body.
                if is_number && this.number_editing(selected) {
                    if this.number_apply(selected) {
                        settings_changed = true;
                    }
                    this.super_.cursorOn = false;
                }
                match this.kind(selected) {
                    OptionItemType::OPTION_ITEM_NUMBER => {
                        this.with_number(selected, NumberItem_toggle);
                        result = HandlerResult::HANDLED;
                        settings_changed = true;
                    }
                    OptionItemType::OPTION_ITEM_CHECK => {
                        this.with_check(selected, CheckItem_toggle);
                        result = HandlerResult::HANDLED;
                        settings_changed = true;
                    }
                    OptionItemType::OPTION_ITEM_TEXT => {}
                }
            }
        }
        MINUS | KEY_PADMINUS | KEY_F7 | KEY_RIGHTCLICK => {
            if is_number && this.number_editing(selected) {
                if this.number_apply(selected) {
                    settings_changed = true;
                }
                this.super_.cursorOn = false;
            }
            match this.kind(selected) {
                OptionItemType::OPTION_ITEM_NUMBER => {
                    this.with_number(selected, NumberItem_decrease);
                    result = HandlerResult::HANDLED;
                    settings_changed = true;
                }
                OptionItemType::OPTION_ITEM_CHECK => {
                    this.with_check(selected, |c| CheckItem_set(c, false));
                    result = HandlerResult::HANDLED;
                    settings_changed = true;
                }
                OptionItemType::OPTION_ITEM_TEXT => {}
            }
        }
        PLUS | KEY_PADPLUS | KEY_F8 => {
            if is_number && this.number_editing(selected) {
                if this.number_apply(selected) {
                    settings_changed = true;
                }
                this.super_.cursorOn = false;
            }
            match this.kind(selected) {
                OptionItemType::OPTION_ITEM_NUMBER => {
                    this.with_number(selected, NumberItem_increase);
                    result = HandlerResult::HANDLED;
                    settings_changed = true;
                }
                OptionItemType::OPTION_ITEM_CHECK => {
                    this.with_check(selected, |c| CheckItem_set(c, true));
                    result = HandlerResult::HANDLED;
                    settings_changed = true;
                }
                OptionItemType::OPTION_ITEM_TEXT => {}
            }
        }
        KEY_RECLICK => {
            if is_number && this.number_editing(selected) {
                if this.number_apply(selected) {
                    settings_changed = true;
                }
                this.super_.cursorOn = false;
            }
            match this.kind(selected) {
                OptionItemType::OPTION_ITEM_NUMBER => {
                    this.with_number(selected, NumberItem_increase);
                    result = HandlerResult::HANDLED;
                    settings_changed = true;
                }
                OptionItemType::OPTION_ITEM_CHECK => {
                    this.with_check(selected, CheckItem_toggle);
                    result = HandlerResult::HANDLED;
                    settings_changed = true;
                }
                OptionItemType::OPTION_ITEM_TEXT => {}
            }
        }
        KEY_UP | KEY_DOWN | KEY_NPAGE | KEY_PPAGE | KEY_HOME | KEY_END | EVENT_SET_SELECTED => {
            // The nav keys apply a pending edit, move the selection, then fall
            // through to EVENT_SET_SELECTED; EVENT_SET_SELECTED enters here
            // directly (no nav).
            if ch != EVENT_SET_SELECTED {
                if is_number && this.number_editing(selected) {
                    if this.number_apply(selected) {
                        settings_changed = true;
                    }
                    this.super_.cursorOn = false;
                }
                let previous = selected;
                Panel_onKey(&mut this.super_, ch);
                selected = Panel_getSelectedIndex(&this.super_);
                if previous != selected {
                    result = HandlerResult::HANDLED;
                    settings_changed = true;
                }
            }
            // EVENT_SET_SELECTED body (shared / fallthrough target):
            this.super_.cursorOn = false;
            if !this.super_.items.is_empty()
                && this.kind(selected) == OptionItemType::OPTION_ITEM_NUMBER
            {
                this.super_.currentBar = Some(this.decIncBar.clone());
            } else {
                Panel_setDefaultBar(&mut this.super_);
            }
        }
        EVENT_PANEL_LOST_FOCUS => {
            if is_number && this.number_editing(selected) && this.number_apply(selected) {
                settings_changed = true;
            }
            this.super_.cursorOn = false;
            Panel_setDefaultBar(&mut this.super_);
        }
        _ => {
            let is_edit_char =
                (b'0' as i32..=b'9' as i32).contains(&ch) || ch == b'.' as i32 || ch == b',' as i32;
            if is_number && this.number_editing(selected) {
                if is_edit_char {
                    let c = ch as u8 as char;
                    this.with_number(selected, |n| {
                        NumberItem_addChar(n, c);
                    });
                    this.set_edit_cursor(selected);
                    return HandlerResult::HANDLED;
                }
                // Non-edit key while editing: apply the pending edit first.
                if this.number_apply(selected) {
                    settings_changed = true;
                }
                this.super_.cursorOn = false;
            } else if is_number {
                // Start editing when a digit or decimal separator is typed.
                if is_edit_char {
                    let c = ch as u8 as char;
                    this.with_number(selected, |n| {
                        NumberItem_startEditing(n);
                        NumberItem_addChar(n, c);
                    });
                    this.set_edit_cursor(selected);
                    return HandlerResult::HANDLED;
                }
            }
        }
    }

    if settings_changed {
        // SAFETY: `settings`/`scr` are the non-owning back-pointers stored at
        // construction (`DisplayOptionsPanel_new`); both outlive this panel
        // (the ScreenManager owns it). They alias distinct objects.
        {
            let settings = unsafe { &mut *this.settings };
            settings.changed = true;
            settings.lastUpdate += 1;
        }
        // C: CRT_updateDelay() — the static inline over CRT_enableDelay (CRT.h:233).
        CRT_enableDelay();
        // C: Header* header = this->scr->header; Header_calculateHeight(header);
        //    Header_reinit(header); Header_updateData(header); Header_draw(header);
        let scr = unsafe { &mut *this.scr };
        {
            // SAFETY: scr->header points to the caller-owned Header that
            // outlives this panel; NULL only before wiring.
            let header = unsafe { scr.header.as_mut() }
                .expect("DisplayOptionsPanel_eventHandler: scr->header is NULL");
            Header_calculateHeight(header);
            Header_reinit(header);
            Header_updateData(header);
            let mut out = io::stdout().lock();
            Header_draw(header, &mut out);
            let _ = out.flush();
        }
        ScreenManager_resize(scr);
    }

    result
}

/// Port of `DisplayOptionsPanel* DisplayOptionsPanel_new(Settings* settings,
/// ScreenManager* scr)` from `DisplayOptionsPanel.c:251`.
///
/// Builds a `1×1` [`Panel`] with the `DisplayOptionsFunctions` [`FunctionBar`],
/// the owned `decIncBar` (`DisplayOptionsDecIncFunctions`), stores the
/// `settings`/`scr` back-pointers, sets the "Display options" header, then
/// appends every option row. Each `CheckItem_newByRef`/`NumberItem_newByRef`
/// binds a raw `*mut bool`/`*mut c_int` into the matching [`Settings`] field
/// (or, for the first three tree rows, the active [`ScreenSettings`]
/// `settings->ss` == `settings.screens[ssIndex]`) reached through the
/// `settings` back-pointer.
///
/// # Safety
///
/// `settings` must point at a live [`Settings`] that outlives the returned
/// panel (as in C, where the setup screen borrows the process `Settings`);
/// the `*_newByRef` rows store raw pointers into its fields (and into the
/// active-screen `ScreenSettings`), which must not be reallocated while the
/// panel lives.
pub fn DisplayOptionsPanel_new(
    settings: *mut Settings,
    scr: *mut ScreenManager,
) -> DisplayOptionsPanel {
    let fu_bar = FunctionBar_new(Some(&DisplayOptionsFunctions[..]), None, None);
    let super_ = Panel_new(1, 1, 1, 1, Some(fu_bar));

    let dec_inc = FunctionBar_new(Some(&DisplayOptionsDecIncFunctions[..]), None, None);

    let mut this = DisplayOptionsPanel {
        super_,
        settings,
        scr,
        decIncBar: dec_inc,
    };

    Panel_setHeader(&mut this.super_, "Display options");

    // C: char tabheader[...] = "For current screen tab: ";
    //    strncat(tabheader, settings->ss->heading, SCREEN_NAME_LEN);
    // settings->ss aliases settings->screens[ssIndex] (the active screen).
    let ss_index = unsafe { (*settings).ssIndex as usize };
    let tabheader = {
        // SAFETY: `settings` is live for the panel's lifetime (see fn docs).
        let s = unsafe { &*settings };
        let heading = s.screens[ss_index].heading.as_deref().unwrap_or("");
        let n = heading.len().min(SCREEN_NAME_LEN);
        format!("For current screen tab: {}", &heading[..n])
    };
    this.add_text(&tabheader);

    // The three tree-view rows bind into the active screen's ScreenSettings.
    // SAFETY (this and every unsafe block below): raw pointers into the live
    // `settings` (and its `screens[ss_index]`), which outlive the panel (see fn
    // docs); each is a distinct field, so no two aliasing pointers overlap. The
    // ScreenSettings element pointer is taken through an explicit `&mut *settings`
    // deref so the `screens[ss_index]` index does not implicitly autoref a raw
    // pointer.
    let ss_ptr: *mut ScreenSettings = unsafe {
        let s = &mut *settings;
        &mut s.screens[ss_index] as *mut ScreenSettings
    };
    this.add_check("Tree view", unsafe { &mut (*ss_ptr).treeView as *mut bool });
    this.add_check(
        "- Tree view is always sorted by PID (htop 2 behavior)",
        unsafe { &mut (*ss_ptr).treeViewAlwaysByPID as *mut bool },
    );
    this.add_check("- Tree view is collapsed by default", unsafe {
        &mut (*ss_ptr).allBranchesCollapsed as *mut bool
    });

    this.add_text("Global options:");
    this.add_check("Show tabs for screens", unsafe {
        &mut (*settings).screenTabs as *mut bool
    });
    this.add_check("Shadow other users' processes", unsafe {
        &mut (*settings).shadowOtherUsers as *mut bool
    });
    this.add_check("Hide kernel threads", unsafe {
        &mut (*settings).hideKernelThreads as *mut bool
    });
    this.add_check("Hide userland process threads", unsafe {
        &mut (*settings).hideUserlandThreads as *mut bool
    });
    this.add_check("Hide processes running in containers", unsafe {
        &mut (*settings).hideRunningInContainer as *mut bool
    });
    this.add_check("Display threads in a different color", unsafe {
        &mut (*settings).highlightThreads as *mut bool
    });
    this.add_check("Show custom thread names", unsafe {
        &mut (*settings).showThreadNames as *mut bool
    });
    this.add_check("Show program path", unsafe {
        &mut (*settings).showProgramPath as *mut bool
    });
    this.add_check("Highlight program \"basename\"", unsafe {
        &mut (*settings).highlightBaseName as *mut bool
    });
    this.add_check(
        "Highlight out-dated/removed programs (red) / libraries (yellow)",
        unsafe { &mut (*settings).highlightDeletedExe as *mut bool },
    );
    this.add_check("Shadow distribution path prefixes", unsafe {
        &mut (*settings).shadowDistPathPrefix as *mut bool
    });
    this.add_check("Merge exe, comm and cmdline in Command", unsafe {
        &mut (*settings).showMergedCommand as *mut bool
    });
    this.add_check(
        "- Try to find comm in cmdline (when Command is merged)",
        unsafe { &mut (*settings).findCommInCmdline as *mut bool },
    );
    this.add_check(
        "- Try to strip exe from cmdline (when Command is merged)",
        unsafe { &mut (*settings).stripExeFromCmdline as *mut bool },
    );
    this.add_check("Highlight large numbers in memory counters", unsafe {
        &mut (*settings).highlightMegabytes as *mut bool
    });
    this.add_check("Leave a margin around header", unsafe {
        &mut (*settings).headerMargin as *mut bool
    });
    this.add_check(
        "Detailed CPU time (System/IO-Wait/Hard-IRQ/Soft-IRQ/Steal/Guest)",
        unsafe { &mut (*settings).detailedCPUTime as *mut bool },
    );
    this.add_check("Count CPUs from 1 instead of 0", unsafe {
        &mut (*settings).countCPUsFromOne as *mut bool
    });
    this.add_check(
        "Label CPUs based on SMT topology (e.g. 0a, 0b) instead of CPU index",
        unsafe { &mut (*settings).showCPUSMTLabels as *mut bool },
    );
    this.add_check("Update process names on every refresh", unsafe {
        &mut (*settings).updateProcessNames as *mut bool
    });
    this.add_check("Add guest time in CPU meter percentage", unsafe {
        &mut (*settings).accountGuestInCPUMeter as *mut bool
    });
    this.add_check("Also show CPU percentage numerically", unsafe {
        &mut (*settings).showCPUUsage as *mut bool
    });
    this.add_check("Also show CPU frequency", unsafe {
        &mut (*settings).showCPUFrequency as *mut bool
    });
    // #ifdef BUILD_WITH_CPU_TEMP: "Also show CPU temperature ..." +
    // "- Show temperature in degree Fahrenheit ..." — omitted (feature not
    // defined in this build; `showCPUTemperature`/`degreeFahrenheit` fields exist).
    this.add_check("Show cached memory in graph and bar modes", unsafe {
        &mut (*settings).showCachedMemory as *mut bool
    });
    // #ifdef HAVE_GETMOUSE: "Enable the mouse" — omitted (this is the
    // `#ifndef HAVE_GETMOUSE` build; `enableMouse` field exists).
    this.add_number(
        "Update interval (in seconds)",
        unsafe { &mut (*settings).delay as *mut c_int },
        -1,
        1,
        255,
    );
    this.add_check("Highlight new and old processes", unsafe {
        &mut (*settings).highlightChanges as *mut bool
    });
    this.add_number(
        "- Highlight time (in seconds)",
        unsafe { &mut (*settings).highlightDelaySecs as *mut c_int },
        0,
        1,
        24 * 60 * 60,
    );
    this.add_number(
        "Hide main function bar (0 - off, 1 - on ESC until next input, 2 - permanently)",
        unsafe { &mut (*settings).hideFunctionBar as *mut c_int },
        0,
        0,
        2,
    );
    // #ifdef HAVE_LIBHWLOC: "Show topology when selecting affinity ..." —
    // omitted (feature not defined in this build; `topologyAffinity` field exists).

    this
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::action::State;
    use crate::ported::machine::Machine;
    use crate::ported::optionitem::{CheckItem_get, NumberItem_get};
    use crate::ported::panel::Panel_setSelected;
    use crate::ported::screenmanager::ScreenManager_new;
    use crate::ported::settings::{HeaderLayout, ScreenSettings};

    fn settings() -> Box<Settings> {
        Box::new(Settings {
            hLayout: HeaderLayout::HF_ONE_100,
            screens: vec![ScreenSettings {
                heading: Some("Main".to_string()),
                ..Default::default()
            }],
            ssIndex: 0,
            ..Default::default()
        })
    }

    #[test]
    fn new_builds_rows_and_binds_refs_into_settings() {
        let mut s = settings();
        let ptr: *mut Settings = s.as_mut();
        let panel = DisplayOptionsPanel_new(ptr, core::ptr::null_mut());

        // First row is the "For current screen tab: Main" TextItem.
        assert!(panel.super_.items.len() > 4);

        // Find the "Tree view" CheckItem (row 1) and flip it through the panel;
        // the change must land in the active screen's ScreenSettings.
        let any: &dyn Any = panel.super_.items[1].object();
        let tree = any
            .downcast_ref::<CheckItem>()
            .expect("row 1 is a CheckItem");
        assert_eq!(tree.text, "Tree view");
        // The ref binds into settings.screens[0].treeView (false initially).
        assert!(!CheckItem_get(tree));
        s.screens[0].treeView = true;
        let any2: &dyn Any = panel.super_.items[1].object();
        let tree2 = any2.downcast_ref::<CheckItem>().unwrap();
        assert!(
            CheckItem_get(tree2),
            "the CheckItem reads the external cell"
        );
    }

    #[test]
    fn new_number_rows_bind_into_settings() {
        let mut s = settings();
        s.delay = 15;
        let ptr: *mut Settings = s.as_mut();
        let panel = DisplayOptionsPanel_new(ptr, core::ptr::null_mut());

        // The "Update interval" NumberItem reads settings->delay via its ref.
        let num = panel
            .super_
            .items
            .iter()
            .filter_map(|it| {
                let any: &dyn Any = it.object();
                any.downcast_ref::<NumberItem>()
            })
            .find(|n| n.text == "Update interval (in seconds)")
            .expect("Update interval NumberItem present");
        assert_eq!(NumberItem_get(num), 15);
    }

    // A ScreenManager wired with a header + state, enough for the
    // settingsChanged tail (Header_* + ScreenManager_resize).
    fn scr_with_header() -> ScreenManager {
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
        let state = State {
            pauseUpdate: false,
            hideSelection: false,
            hideMeters: false,
            host: core::ptr::null_mut(),
            mainPanel: core::ptr::null_mut(),
            header: core::ptr::null_mut(),
            failedUpdate: None,
        };
        let mut scr = ScreenManager_new(Some(header()), Machine::default(), state);
        scr.panelCount = 1;
        scr.panels.push(Box::new(Panel_new(0, 0, 10, 5, None)));
        scr
    }

    #[test]
    fn space_toggles_selected_checkitem_and_marks_changed() {
        let mut s = settings();
        let sptr: *mut Settings = s.as_mut();
        let mut scr = scr_with_header();
        let scrptr: *mut ScreenManager = &mut scr;

        let mut panel = DisplayOptionsPanel_new(sptr, scrptr);
        // Select the "Tree view" CheckItem (row 1).
        Panel_setSelected(&mut panel.super_, 1);
        let before = s.screens[0].treeView;

        let r = DisplayOptionsPanel_eventHandler(&mut panel, b' ' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        // Toggled the external cell.
        assert_ne!(s.screens[0].treeView, before);
        assert!(s.changed);
        assert_eq!(s.lastUpdate, 1);
    }

    #[test]
    fn number_row_enters_editing_and_applies_digit() {
        let mut s = settings();
        s.delay = 15;
        let sptr: *mut Settings = s.as_mut();
        let mut scr = scr_with_header();
        let scrptr: *mut ScreenManager = &mut scr;

        let mut panel = DisplayOptionsPanel_new(sptr, scrptr);
        // Locate the "Update interval" NumberItem row index.
        let idx = panel
            .super_
            .items
            .iter()
            .position(|it| {
                let any: &dyn Any = it.object();
                any.downcast_ref::<NumberItem>()
                    .map(|n| n.text == "Update interval (in seconds)")
                    .unwrap_or(false)
            })
            .unwrap() as i32;
        Panel_setSelected(&mut panel.super_, idx);

        // Type '9': starts editing (does not yet commit to settings).
        let r = DisplayOptionsPanel_eventHandler(&mut panel, b'9' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(s.delay, 15, "editing does not commit until Enter");

        // Enter: applies the edit. The delay row has scale -1 (display is in
        // seconds with one decimal, internal value in tenths), so typing "9"
        // == 9.0s == internal 90, then clamped to [1,255].
        let r = DisplayOptionsPanel_eventHandler(&mut panel, KEY_ENTER);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(s.delay, 90);
        assert!(s.changed);
    }

    #[test]
    fn non_edit_key_on_text_row_is_ignored() {
        let mut s = settings();
        let sptr: *mut Settings = s.as_mut();
        let mut panel = DisplayOptionsPanel_new(sptr, core::ptr::null_mut());
        // Row 0 is the TextItem header row.
        Panel_setSelected(&mut panel.super_, 0);
        let r = DisplayOptionsPanel_eventHandler(&mut panel, b'x' as i32);
        assert_eq!(r, HandlerResult::IGNORED);
    }
}
