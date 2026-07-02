//! Partial port of `ColorsPanel.c` — htop's "Colors" color-scheme picker.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Data model
//!
//! The C `ColorsPanel` (`ColorsPanel.h:14`) embeds a `Panel super` plus a
//! non-owning `Settings* settings` back-pointer. The [`ColorsPanel`] struct
//! here models both: `super_` (avoiding the Rust `super` keyword, per the
//! `MainPanel`/`ColumnsPanel`/`ScreenNamesPanel` convention) and `settings`
//! as a `*mut Settings` raw pointer — the same idiom `ScreensPanel`/
//! `ScreenNamesPanel` use for their `Settings*` back-pointers (the `Settings`
//! is owned elsewhere — `htop.c`/`ScreenManager`).
//!
//! # Ported (self-contained, no unported substrate)
//!
//! - [`ColorsPanel_new`] (`ColorsPanel.c:92`) — builds the panel: a `1×1`
//!   [`Panel`] with the `ColorsFunctions` ("Done  ") [`FunctionBar`], one
//!   [`CheckItem`] per entry of [`ColorSchemeNames`], the "Colors" header,
//!   and the initial check on the row matching the active scheme
//!   (`CRT_colorScheme`).
//!
//! # Stubbed (deliberate teardown / missing substrate)
//!
//! - [`ColorsPanel_delete`] (`ColorsPanel.c:44`) — `Panel_done` + `free`.
//!   [`ColorsPanel`] owns its fields, so `Drop` releases them; there is no
//!   algorithm to port (same precedent as every sibling `_delete`).
//! - [`ColorsPanel_eventHandler`] (`ColorsPanel.c:50`) — on Enter/click it
//!   sets `this->settings->colorScheme = mark`. The ported `Settings`
//!   (`settings.rs`) has **no `colorScheme` field** (its doc comment lists
//!   `colorScheme` among the omitted `Settings.h` fields). Without that
//!   field the write-back cannot be modeled, so the handler stays a stub
//!   naming the missing dependency (rule 4).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;
use core::sync::atomic::Ordering;

use crate::ported::crt::{CRT_colorScheme, ColorScheme};
use crate::ported::functionbar::FunctionBar_new;
// `Object` is referenced only by the `#[cfg(test)]` helpers below (via `use
// super::*`); gate the import so non-test builds don't flag it as unused.
#[cfg(test)]
use crate::ported::object::Object;
use crate::ported::optionitem::{CheckItem, CheckItem_newByVal, CheckItem_set};
use crate::ported::panel::{Panel, Panel_add, Panel_new, Panel_setHeader};
use crate::ported::settings::Settings;

/// Port of the file-scope
/// `static const char* const ColorsFunctions[]` from `ColorsPanel.c:30`.
/// Nine blank slots followed by `"Done  "`; the C trailing `NULL`
/// sentinel is dropped (the ported `FunctionBar_new` is length-bounded,
/// not NUL-terminated).
static ColorsFunctions: [&str; 10] = [
    "      ", "      ", "      ", "      ", "      ", "      ", "      ", "      ", "      ",
    "Done  ",
];

/// Port of the file-scope
/// `static const char* const ColorSchemeNames[]` from `ColorsPanel.c:32`.
/// One label per [`ColorScheme`], in the same order as that enum; the C
/// trailing `NULL` sentinel is dropped (Rust length-bounds the array).
static ColorSchemeNames: [&str; 8] = [
    "Default",
    "Monochromatic",
    "Black on White",
    "Light Terminal",
    "MC",
    "Black Night",
    "Broken Gray",
    "Nord",
];

/// Reduced model of the C `ColorsPanel` struct (`ColorsPanel.h:14`): the
/// embedded `Panel super` (as `super_`) and the non-owning `Settings*
/// settings` back-pointer (as `*mut Settings`, the `ScreensPanel` idiom —
/// the `Settings` is owned elsewhere).
pub struct ColorsPanel {
    /// C `Panel super` — the embedded panel base.
    pub super_: Panel,
    /// C `Settings* settings` — non-owning back-pointer to the settings the
    /// event handler mutates. Stored verbatim by [`ColorsPanel_new`].
    pub settings: *mut Settings,
}

/// TODO: port of `static void ColorsPanel_delete(Object* object)` from
/// `ColorsPanel.c:44`. `Panel_done(&this->super)` + `free(this)` — the
/// owned fields are released by `Drop`, so there is no algorithm to port
/// (same precedent as `Panel_delete`/`ListItem_delete`). Left as a stub.
pub fn ColorsPanel_delete() {
    todo!("port of ColorsPanel.c:44 — Drop releases the panel")
}

/// TODO: port of `static HandlerResult ColorsPanel_eventHandler(Panel*
/// super, int ch)` from `ColorsPanel.c:50`. On Enter/Space/click it clears
/// every scheme's [`CheckItem`], checks the selected one, then writes
/// `this->settings->colorScheme = mark`, bumps `changed`/`lastUpdate`, and
/// calls `CRT_setColors(mark)` + `clear()`. The ported `Settings`
/// (`settings.rs`) has **no `colorScheme` field** — it is explicitly among
/// the omitted `Settings.h` fields — so the write-back cannot be modeled.
/// Left as a stub naming the missing dependency (rule 4).
pub fn ColorsPanel_eventHandler() {
    todo!("port of ColorsPanel.c:50 — needs Settings.colorScheme field")
}

/// Port of `ColorsPanel* ColorsPanel_new(Settings* settings)` from
/// `ColorsPanel.c:92`.
///
/// Builds a `1×1` [`Panel`] with the `ColorsFunctions` [`FunctionBar`]
/// (`FunctionBar_new(ColorsFunctions, NULL, NULL)` → the static F-key
/// tables), stores the `settings` back-pointer, then for each entry of
/// [`ColorSchemeNames`] appends a `CheckItem_newByVal(name, false)`.
/// Finishes with `Panel_setHeader("Colors")` and checks the row whose
/// index equals the active scheme (`CRT_colorScheme`).
///
/// The C `Class(CheckItem)`/`owner` args to `Panel_init` type the
/// underlying `Vector`; the ported `Panel_new`/`Panel_init` drop them (a
/// `Vec<Box<dyn Object>>` needs no such typing), matching every sibling
/// panel port. The C `CheckItem_set((CheckItem*)Panel_get(super,
/// (int)CRT_colorScheme), true)` write is reproduced by indexing
/// `super_.items[CRT_colorScheme]` and downcasting the `&mut dyn Object`
/// to `&mut CheckItem` via the `Any` supertrait (ported `Panel_get` hands
/// back an immutable `&dyn Object`), the same mutating analog
/// `ColumnsPanel_cancelMoving` uses. `CRT_colorScheme` is always in
/// `0..LAST_COLORSCHEME` (see [`crate::ported::crt::CRT_setColors`]'s
/// clamp), so the index is a valid `CheckItem` row.
pub fn ColorsPanel_new(settings: *mut Settings) -> ColorsPanel {
    let fuBar = FunctionBar_new(Some(&ColorsFunctions[..]), None, None);
    let super_ = Panel_new(1, 1, 1, 1, Some(fuBar));

    let mut this = ColorsPanel { super_, settings };

    // C: assert(ARRAYSIZE(ColorSchemeNames) == LAST_COLORSCHEME + 1). The
    // C count includes the NULL sentinel; the ported array omits it, so
    // the faithful equality drops the `+ 1`.
    debug_assert_eq!(
        ColorSchemeNames.len(),
        ColorScheme::LAST_COLORSCHEME as usize
    );

    Panel_setHeader(&mut this.super_, "Colors");
    for name in ColorSchemeNames {
        Panel_add(&mut this.super_, Box::new(CheckItem_newByVal(name, false)));
    }

    let idx = CRT_colorScheme.load(Ordering::Relaxed);
    let any: &mut dyn Any = this.super_.items[idx].as_mut();
    if let Some(item) = any.downcast_mut::<CheckItem>() {
        CheckItem_set(item, true);
    }

    this
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::crt::CRT_setColors;
    use crate::ported::optionitem::CheckItem_get;
    use crate::ported::panel::Panel_size;
    use crate::ported::richstring::RichString_sizeVal;

    /// The `value` (checked) flag of the panel's `i`-th `CheckItem`.
    fn item_checked(cp: &ColorsPanel, i: usize) -> bool {
        let obj: &dyn Object = cp.super_.items[i].as_ref();
        let any: &dyn Any = obj;
        CheckItem_get(any.downcast_ref::<CheckItem>().unwrap())
    }

    /// The `text` label of the panel's `i`-th `CheckItem`.
    fn item_text(cp: &ColorsPanel, i: usize) -> String {
        let obj: &dyn Object = cp.super_.items[i].as_ref();
        let any: &dyn Any = obj;
        any.downcast_ref::<CheckItem>().unwrap().text.clone()
    }

    #[test]
    fn builds_one_checkitem_per_scheme_in_order() {
        let cp = ColorsPanel_new(core::ptr::null_mut());
        assert_eq!(Panel_size(&cp.super_), ColorSchemeNames.len() as i32);
        for (i, name) in ColorSchemeNames.iter().enumerate() {
            assert_eq!(item_text(&cp, i), *name);
        }
    }

    #[test]
    fn header_is_colors() {
        let cp = ColorsPanel_new(core::ptr::null_mut());
        assert_eq!(RichString_sizeVal(&cp.super_.header), "Colors".len() as i32);
    }

    #[test]
    fn function_bar_last_label_is_done() {
        let cp = ColorsPanel_new(core::ptr::null_mut());
        let bar = cp.super_.currentBar.as_ref().expect("currentBar set");
        assert_eq!(bar.functions, ColorsFunctions.to_vec());
    }

    #[test]
    fn stores_settings_backpointer() {
        let sentinel = 0xdead_beef_usize as *mut Settings;
        let cp = ColorsPanel_new(sentinel);
        assert_eq!(cp.settings, sentinel);
    }

    #[test]
    fn checks_only_the_active_scheme_row() {
        // Pin the active scheme so the checked row is deterministic. Index 2
        // == COLORSCHEME_BLACKONWHITE.
        CRT_setColors(ColorScheme::COLORSCHEME_BLACKONWHITE as i32);
        let active = CRT_colorScheme.load(Ordering::Relaxed);
        let cp = ColorsPanel_new(core::ptr::null_mut());
        for i in 0..ColorSchemeNames.len() {
            assert_eq!(item_checked(&cp, i), i == active, "row {i} check state");
        }
    }
}
