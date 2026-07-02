//! Port of `Panel.c` — only the pure field-arithmetic helpers.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! htop's `Panel` (`Panel.h:64`) is an `Object` subclass wrapping a
//! heap-allocated `Vector* items`, a `RichString header`, two
//! `FunctionBar*` bars and an `eventHandlerState` scratch buffer. Its
//! geometry/cursor/scroll/selection state is a set of plain `int`/
//! `size_t`/`bool`/`ColorElements` fields. The handful of `Panel_*`
//! functions that only read and write those scalar fields (with no
//! `Vector`, `RichString`, `FunctionBar`, ncurses, `Object` vtable, or
//! `CRT_colors` access) port faithfully; they are modelled here against
//! a plain [`Panel`] struct holding exactly the fields those functions
//! touch. Every other `Panel_*` function stays an arg-less `todo!()`
//! stub because its full behavior depends on unported substrate.
//!
//! Modelled fields (subset of `struct Panel_`): `x`, `y`, `w`, `h`,
//! `cursorX`, `cursorY`, `selected`, `scrollV`, `scrollH`,
//! `selectedLen`, `needsRedraw`, `selectionColorId`. Omitted (only used
//! by the unported fns): `super` (Object), `oldSelected`,
//! `prevSelected`, `eventHandlerState`, `cursorOn`, `wasFocus`,
//! `lastMouseBarClickX`, `items` (Vector), `currentBar`/`defaultBar`
//! (FunctionBar), `header` (RichString).
//!
//! The C `assert(this != NULL)` preconditions are dropped: `&mut Panel`
//! / `&Panel` receivers are non-null by construction, exactly as the
//! `vector.rs` port drops its struct-consistency asserts.
//!
//! Not ported (substrate-dependent), and why:
//! - `Panel_new`/`Panel_delete`/`Panel_init`/`Panel_done` — `xMalloc`/
//!   `free`, `Object_setClass` vtable, `Vector_new`/`Vector_delete`,
//!   `FunctionBar_delete`, `RichString_beginAllocated`/`RichString_delete`.
//! - `Panel_setHeader` — `RichString_writeWide` + `CRT_colors`.
//! - `Panel_prune`/`Panel_add`/`Panel_insert`/`Panel_set`/`Panel_get`/
//!   `Panel_remove`/`Panel_getSelected`/`Panel_moveSelectedUp`/
//!   `Panel_moveSelectedDown`/`Panel_size`/`Panel_splice` — all wrap the
//!   `Vector` array machinery (`Vector_add`, `Vector_remove`,
//!   `Vector_size`, `Vector_moveUp`, ...) which `vector.rs` deliberately
//!   does not port.
//! - `Panel_setSelected` — `Vector_size` + the `Panel_eventHandler`
//!   Object-vtable dispatch.
//! - `Panel_draw`/`Panel_headerHeight` — `RichString`, `Object_display`,
//!   `CRT_colors`, ncurses (`attrset`/`mvhline`), `FunctionBar_draw`.
//! - `Panel_onKey` — ncurses `KEY_*` constants, `CRT_scrollHAmount`/
//!   `CRT_scrollWheelVAmount` globals, `Vector_size`, `Panel_headerHeight`.
//! - `Panel_selectByTyping` — `Vector`/`ListItem` access, `strncasecmp`,
//!   `Panel_setSelected`, `HandlerResult`.
//! - `Panel_getCh` — ncurses `move`/`curs_set`/`getch`/`set_escdelay`.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// The scalar subset of htop's `struct Panel_` (`Panel.h:64`) that the
/// ported field-arithmetic helpers read and write. See the module doc
/// for the full field mapping and the list of omitted (substrate-only)
/// fields.
///
/// `selectedLen` mirrors the C `size_t selectedLen` (hence `usize`);
/// `selectionColorId` mirrors `ColorElements selectionColorId`, a C enum
/// backed by `int`, modelled as `i32` so the raw stored value is exactly
/// what the setter received (no `CRT_colors` lookup is involved).
pub struct Panel {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub cursorX: i32,
    pub cursorY: i32,
    pub selected: i32,
    pub scrollV: i32,
    pub scrollH: i32,
    pub selectedLen: usize,
    pub needsRedraw: bool,
    pub selectionColorId: i32,
}

/// Port of `Panel.c:82`.
///
/// ```c
/// this->cursorY = this->y + this->selected - this->scrollV + 1;
/// this->cursorX = this->x + (int)this->selectedLen - this->scrollH;
/// ```
///
/// `(int)this->selectedLen` truncates the `size_t` to `int`; `as i32`
/// reproduces that truncation, and the surrounding arithmetic stays in
/// `i32` exactly as the C computes it in `int`.
pub fn Panel_setCursorToSelection(this: &mut Panel) {
    this.cursorY = this.y + this.selected - this.scrollV + 1;
    this.cursorX = this.x + (this.selectedLen as i32) - this.scrollH;
}

/// Port of `Panel.c:87`. Stores `colorId` into `selectionColorId`; the
/// C `ColorElements` enum value is carried verbatim as an `i32`.
pub fn Panel_setSelectionColor(this: &mut Panel, colorId: i32) {
    this.selectionColorId = colorId;
}

/// Port of `Panel.c:96`. Sets the panel's top-left corner and marks it
/// dirty. The `assert(this != NULL)` precondition is dropped.
pub fn Panel_move(this: &mut Panel, x: i32, y: i32) {
    this.x = x;
    this.y = y;
    this.needsRedraw = true;
}

/// Port of `Panel.c:104`. Sets the panel's width/height and marks it
/// dirty. The `assert(this != NULL)` precondition is dropped.
pub fn Panel_resize(this: &mut Panel, w: i32, h: i32) {
    this.w = w;
    this.h = h;
    this.needsRedraw = true;
}

/// Port of `Panel.c:194`. Returns the current selection index. The
/// `assert(this != NULL)` precondition is dropped; the C receiver is
/// `const Panel*`, mirrored by `&Panel`.
pub fn Panel_getSelectedIndex(this: &Panel) -> i32 {
    this.selected
}

/// TODO: port of `Panel* Panel_new(int x, int y, int w, int h, const ObjectClass* type, bool owner, FunctionBar* fuBar` from `Panel.c:36`.
pub fn Panel_new() {
    todo!("port of Panel.c:36")
}

/// TODO: port of `void Panel_delete(Object* cast` from `Panel.c:43`.
pub fn Panel_delete() {
    todo!("port of Panel.c:43")
}

/// TODO: port of `void Panel_init(Panel* this, int x, int y, int w, int h, const ObjectClass* type, bool owner, FunctionBar* fuBar` from `Panel.c:49`.
pub fn Panel_init() {
    todo!("port of Panel.c:49")
}

/// TODO: port of `void Panel_done(Panel* this` from `Panel.c:73`.
pub fn Panel_done() {
    todo!("port of Panel.c:73")
}

/// TODO: port of `inline void Panel_setHeader(Panel* this, const char* header` from `Panel.c:91`.
pub fn Panel_setHeader() {
    todo!("port of Panel.c:91")
}

/// TODO: port of `void Panel_prune(Panel* this` from `Panel.c:112`.
pub fn Panel_prune() {
    todo!("port of Panel.c:112")
}

/// TODO: port of `void Panel_add(Panel* this, Object* o` from `Panel.c:123`.
pub fn Panel_add() {
    todo!("port of Panel.c:123")
}

/// TODO: port of `void Panel_insert(Panel* this, int i, Object* o` from `Panel.c:131`.
pub fn Panel_insert() {
    todo!("port of Panel.c:131")
}

/// TODO: port of `void Panel_set(Panel* this, int i, Object* o` from `Panel.c:139`.
pub fn Panel_set() {
    todo!("port of Panel.c:139")
}

/// TODO: port of `Object* Panel_get(Panel* this, int i` from `Panel.c:145`.
pub fn Panel_get() {
    todo!("port of Panel.c:145")
}

/// TODO: port of `Object* Panel_remove(Panel* this, int i` from `Panel.c:151`.
pub fn Panel_remove() {
    todo!("port of Panel.c:151")
}

/// TODO: port of `Object* Panel_getSelected(Panel* this` from `Panel.c:164`.
pub fn Panel_getSelected() {
    todo!("port of Panel.c:164")
}

/// TODO: port of `void Panel_moveSelectedUp(Panel* this` from `Panel.c:174`.
pub fn Panel_moveSelectedUp() {
    todo!("port of Panel.c:174")
}

/// TODO: port of `void Panel_moveSelectedDown(Panel* this` from `Panel.c:184`.
pub fn Panel_moveSelectedDown() {
    todo!("port of Panel.c:184")
}

/// TODO: port of `int Panel_size(const Panel* this` from `Panel.c:200`.
pub fn Panel_size() {
    todo!("port of Panel.c:200")
}

/// TODO: port of `void Panel_setSelected(Panel* this, int selected` from `Panel.c:206`.
pub fn Panel_setSelected() {
    todo!("port of Panel.c:206")
}

/// TODO: port of `void Panel_splice(Panel* this, Vector* from` from `Panel.c:222`.
pub fn Panel_splice() {
    todo!("port of Panel.c:222")
}

/// TODO: port of `void Panel_draw(Panel* this, bool force_redraw, bool focus, bool highlightSelected, bool hideFunctionBar` from `Panel.c:231`.
pub fn Panel_draw() {
    todo!("port of Panel.c:231")
}

/// TODO: port of `static int Panel_headerHeight(const Panel* this` from `Panel.c:357`.
pub fn Panel_headerHeight() {
    todo!("port of Panel.c:357")
}

/// TODO: port of `bool Panel_onKey(Panel* this, int key` from `Panel.c:363`.
pub fn Panel_onKey() {
    todo!("port of Panel.c:363")
}

/// TODO: port of `HandlerResult Panel_selectByTyping(Panel* this, int ch` from `Panel.c:468`.
pub fn Panel_selectByTyping() {
    todo!("port of Panel.c:468")
}

/// TODO: port of `int Panel_getCh(Panel* this` from `Panel.c:526`.
pub fn Panel_getCh() {
    todo!("port of Panel.c:526")
}

#[cfg(test)]
mod tests {
    use super::*;

    // A Panel with all modelled scalars zeroed, so each test can set
    // only the fields it exercises.
    fn blank() -> Panel {
        Panel {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            cursorX: 0,
            cursorY: 0,
            selected: 0,
            scrollV: 0,
            scrollH: 0,
            selectedLen: 0,
            needsRedraw: false,
            selectionColorId: 0,
        }
    }

    #[test]
    fn set_cursor_to_selection_matches_c_arithmetic() {
        let mut p = blank();
        p.x = 3;
        p.y = 5;
        p.selected = 7;
        p.scrollV = 2;
        p.scrollH = 4;
        p.selectedLen = 20;
        Panel_setCursorToSelection(&mut p);
        // cursorY = y + selected - scrollV + 1 = 5 + 7 - 2 + 1
        assert_eq!(p.cursorY, 11);
        // cursorX = x + (int)selectedLen - scrollH = 3 + 20 - 4
        assert_eq!(p.cursorX, 19);
    }

    #[test]
    fn set_cursor_to_selection_can_go_negative() {
        // Scrolled past the field: the C computes signed ints, so the
        // cursor coords are allowed to be negative — no clamping.
        let mut p = blank();
        p.x = 0;
        p.y = 0;
        p.selected = 0;
        p.scrollV = 10;
        p.scrollH = 30;
        p.selectedLen = 5;
        Panel_setCursorToSelection(&mut p);
        assert_eq!(p.cursorY, 0 + 0 - 10 + 1); // -9
        assert_eq!(p.cursorX, 0 + 5 - 30); // -25
    }

    #[test]
    fn set_cursor_to_selection_truncates_selectedlen_like_int_cast() {
        // (int)this->selectedLen truncates a size_t to 32-bit int.
        // 0x1_0000_0007 as i32 == 7, so only the low 32 bits contribute.
        let mut p = blank();
        p.selectedLen = 0x1_0000_0007;
        Panel_setCursorToSelection(&mut p);
        assert_eq!(p.cursorX, 7);
    }

    #[test]
    fn set_selection_color_stores_value_verbatim() {
        let mut p = blank();
        Panel_setSelectionColor(&mut p, 42);
        assert_eq!(p.selectionColorId, 42);
        // Negative enum values are carried through unchanged.
        Panel_setSelectionColor(&mut p, -1);
        assert_eq!(p.selectionColorId, -1);
    }

    #[test]
    fn move_sets_position_and_dirties() {
        let mut p = blank();
        p.needsRedraw = false;
        Panel_move(&mut p, 12, 34);
        assert_eq!(p.x, 12);
        assert_eq!(p.y, 34);
        assert!(p.needsRedraw);
    }

    #[test]
    fn resize_sets_dimensions_and_dirties() {
        let mut p = blank();
        p.needsRedraw = false;
        Panel_resize(&mut p, 80, 24);
        assert_eq!(p.w, 80);
        assert_eq!(p.h, 24);
        assert!(p.needsRedraw);
    }

    #[test]
    fn get_selected_index_returns_field() {
        let mut p = blank();
        p.selected = 9;
        assert_eq!(Panel_getSelectedIndex(&p), 9);
        p.selected = 0;
        assert_eq!(Panel_getSelectedIndex(&p), 0);
    }
}
