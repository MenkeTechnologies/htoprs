//! Port of `Panel.c` — htop's scrollable, selectable list widget.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Data model
//!
//! htop's `Panel` (`Panel.h:64`) is an `Object` subclass wrapping a
//! heap-allocated `Vector* items`, a `RichString header`, two
//! `FunctionBar*` bars, plus scalar geometry/cursor/scroll/selection
//! state. The [`Panel`] struct here models those fields directly:
//! `items` is a `Vec<Box<dyn Object>>` (subsuming the C `Vector`), the
//! header is a real [`RichString`], and the two bars are owned
//! [`FunctionBar`] copies (C shares one pointer via `Panel_setDefaultBar`;
//! the clone reproduces the observable draw). This lets the list/scroll/
//! selection functions port faithfully against the `Vec`, instead of the
//! prior scalar-only stub.
//!
//! # What stays a stub
//!
//! - [`Panel_delete`] / [`Panel_done`] — the C bodies are `free`/
//!   `Vector_delete`/`FunctionBar_delete`/`RichString_delete`; in Rust the
//!   owned fields are released by `Drop`, so there is no algorithm to port.
//! - [`Panel_splice`] — takes a `Vector* from` (unported `Vector` type).
//! - [`Panel_selectByTyping`] — needs the `eventHandlerState` scratch
//!   buffer, `ListItem` downcasts, and the `HandlerResult` enum
//!   (`MainPanel`/`ScreensPanel` search substrate).
//! - The `PanelClass` vtable (`eventHandler`/`drawFunctionBar`/
//!   `printHeader`) is not modeled: [`Panel`] is a plain struct, not an
//!   `Object` subclass with a dispatch table. [`Panel_setSelected`] and
//!   [`Panel_draw`] therefore reproduce the *base* `Panel_class` behavior
//!   (base `eventHandler` = `Panel_selectByTyping`, which is a no-op for
//!   `EVENT_SET_SELECTED`; base `drawFunctionBar`/`printHeader` are NULL);
//!   subclass overrides (MainPanel follow-mode, printHeader) would need the
//!   vtable and are noted at each site.
//! - The `KEY_WHEELUP`/`KEY_WHEELDOWN` arms of [`Panel_onKey`] need
//!   `CRT_scrollWheelVAmount` (`CRT.c:956`), which is NOT ported in
//!   `crt.rs`; per the port rules that one path is stubbed rather than
//!   inventing the missing global.
//!
//! Drawing ([`Panel_draw`]) is a behavioral crossterm port through the
//! [`Ncurses`] emit shim: htop's `attrset`/`mvhline`/`RichString_printoffnVal`
//! against `CRT_colors`/`LINES`/`COLS` become crossterm writes resolving
//! `CRT_colors` via the ported `crt::ResolvedColor`. Its pure scroll-clamp
//! logic ("scroll follows selection") is factored into gate-skipped helper
//! methods and unit tested; the terminal side-effects are not.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::io::{self, Write};

use crate::ported::crt::{
    self, ColorElements, ColorScheme, KEY_CTRL, KEY_DOWN, KEY_END, KEY_HOME, KEY_LEFT, KEY_NPAGE,
    KEY_PPAGE, KEY_RIGHT, KEY_UP, KEY_WHEELDOWN, KEY_WHEELUP,
};
use crate::ported::functionbar::{FunctionBar, FunctionBar_draw, Ncurses};
use crate::ported::object::Object;
use crate::ported::richstring::{
    RichString, RichString_rewind, RichString_setAttr, RichString_size, RichString_sizeVal,
    RichString_writeWide,
};
use std::sync::atomic::Ordering;

// Ctrl-key codes htop matches in `Panel_onKey`. `KEY_CTRL` is a `const fn`
// in crt.rs; binding its results as `const`s makes them usable as match
// patterns (a const-fn call is not itself a pattern) without adding any
// top-level `fn`.
const CTRL_N: i32 = KEY_CTRL(b'N' as i32);
const CTRL_P: i32 = KEY_CTRL(b'P' as i32);
const CTRL_B: i32 = KEY_CTRL(b'B' as i32);
const CTRL_F: i32 = KEY_CTRL(b'F' as i32);
const CTRL_A: i32 = KEY_CTRL(b'A' as i32);
const CTRL_E: i32 = KEY_CTRL(b'E' as i32);
const CARET: i32 = b'^' as i32;
const DOLLAR: i32 = b'$' as i32;

/// Port of htop's `struct Panel_` (`Panel.h:64`). See the module docs for
/// the field mapping. `selectionColorId` is a [`ColorElements`] (C's
/// `ColorElements selectionColorId`), so [`Panel_draw`] can index the color
/// tables directly; `items` is the `Vec` analog of the C `Vector* items`.
pub struct Panel {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub cursorX: i32,
    pub cursorY: i32,
    pub items: Vec<Box<dyn Object>>,
    pub selected: i32,
    pub oldSelected: i32,
    pub prevSelected: i32,
    pub selectedLen: usize,
    pub scrollV: i32,
    pub scrollH: i32,
    pub needsRedraw: bool,
    pub cursorOn: bool,
    pub wasFocus: bool,
    pub lastMouseBarClickX: i32,
    pub currentBar: Option<FunctionBar>,
    pub defaultBar: Option<FunctionBar>,
    pub header: RichString,
    pub selectionColorId: ColorElements,
}

impl Panel {
    /// A zeroed `Panel` (all scalars 0/false, empty items, empty header,
    /// no bars). Gate-skipped associated fn — not a real C function; the C
    /// analog is `xMalloc` giving uninitialized storage that `Panel_init`
    /// then overwrites. Used by [`Panel_new`] and the tests.
    fn empty() -> Panel {
        Panel {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            cursorX: 0,
            cursorY: 0,
            items: Vec::new(),
            selected: 0,
            oldSelected: 0,
            prevSelected: -1,
            selectedLen: 0,
            scrollV: 0,
            scrollH: 0,
            needsRedraw: true,
            cursorOn: false,
            wasFocus: false,
            lastMouseBarClickX: 0,
            currentBar: None,
            defaultBar: None,
            header: RichString::new(),
            selectionColorId: ColorElements::PANEL_SELECTION_FOCUS,
        }
    }
}

/// Port of `Panel* Panel_new(int x, int y, int w, int h, const ObjectClass* type,
/// bool owner, FunctionBar* fuBar)` from `Panel.c:36`.
///
/// Allocates and initializes a panel. The C `type`/`owner` args only type
/// the underlying `Vector`; a `Vec<Box<dyn Object>>` needs no such typing,
/// so they are dropped. `Object_setClass(this, Class(Panel))` has no analog
/// (the `PanelClass` vtable is not modeled — see the module docs).
pub fn Panel_new(x: i32, y: i32, w: i32, h: i32, fuBar: Option<FunctionBar>) -> Panel {
    let mut this = Panel::empty();
    Panel_init(&mut this, x, y, w, h, fuBar);
    this
}

/// TODO: port of `void Panel_delete(Object* cast)` from `Panel.c:43`.
/// `Panel_done` + `free` — released by `Drop` in Rust, no algorithm.
pub fn Panel_delete() {
    todo!("port of Panel.c:43 — Drop releases the panel")
}

/// Port of `void Panel_init(Panel* this, int x, int y, int w, int h,
/// const ObjectClass* type, bool owner, FunctionBar* fuBar)` from
/// `Panel.c:49`. Sets every field to its initial value. `type`/`owner`
/// (Vector typing) are dropped; `items` becomes an empty `Vec`, `header` a
/// fresh `RichString` (C `RichString_beginAllocated`), and `defaultBar`/
/// `currentBar` both take `fuBar` (C shares one pointer; the clone
/// reproduces it).
pub fn Panel_init(this: &mut Panel, x: i32, y: i32, w: i32, h: i32, fuBar: Option<FunctionBar>) {
    this.x = x;
    this.y = y;
    this.w = w;
    this.h = h;
    this.cursorX = 0;
    this.cursorY = 0;
    this.items = Vec::new();
    this.scrollV = 0;
    this.scrollH = 0;
    this.selected = 0;
    this.oldSelected = 0;
    this.prevSelected = -1;
    this.selectedLen = 0;
    this.needsRedraw = true;
    this.cursorOn = false;
    this.wasFocus = false;
    this.lastMouseBarClickX = 0;
    this.header = RichString::new();
    this.defaultBar = fuBar.clone();
    this.currentBar = fuBar;
    this.selectionColorId = ColorElements::PANEL_SELECTION_FOCUS;
}

/// Port of the `Panel_setDefaultBar` macro from `Panel.h:86`:
/// `do { (this_)->currentBar = (this_)->defaultBar; } while (0)`.
///
/// C aliases the two `FunctionBar*` pointers to the one shared bar; the
/// `Vec`-model owns each bar via `Option<FunctionBar>`, so `currentBar`
/// takes a clone of `defaultBar` — reproducing the observable draw exactly
/// as [`Panel_init`] already does when it seeds both from `fuBar`.
pub fn Panel_setDefaultBar(this: &mut Panel) {
    this.currentBar = this.defaultBar.clone();
}

/// TODO: port of `void Panel_done(Panel* this)` from `Panel.c:73`.
/// Frees `eventHandlerState`/`items`/`defaultBar`/`header` — all released
/// by `Drop` in Rust, so there is no algorithm to port.
pub fn Panel_done() {
    todo!("port of Panel.c:73 — Drop releases the owned fields")
}

/// Port of `Panel.c:82`.
///
/// ```c
/// this->cursorY = this->y + this->selected - this->scrollV + 1;
/// this->cursorX = this->x + (int)this->selectedLen - this->scrollH;
/// ```
///
/// `(int)this->selectedLen` truncates the `size_t` to `int`; `as i32`
/// reproduces that, and the surrounding arithmetic stays in `i32`.
pub fn Panel_setCursorToSelection(this: &mut Panel) {
    this.cursorY = this.y + this.selected - this.scrollV + 1;
    this.cursorX = this.x + (this.selectedLen as i32) - this.scrollH;
}

/// Port of `Panel.c:87`. Stores `colorId` into `selectionColorId`.
pub fn Panel_setSelectionColor(this: &mut Panel, colorId: ColorElements) {
    this.selectionColorId = colorId;
}

/// Port of `inline void Panel_setHeader(Panel* this, const char* header)`
/// from `Panel.c:91`. Overwrites the header via `RichString_writeWide`
/// with `CRT_colors[PANEL_HEADER_FOCUS]` and marks the panel dirty.
pub fn Panel_setHeader(this: &mut Panel, header: &str) {
    let attr = ColorElements::PANEL_HEADER_FOCUS.packed(ColorScheme::active());
    RichString_writeWide(&mut this.header, attr, header.as_bytes());
    this.needsRedraw = true;
}

/// Port of `Panel.c:96`. Sets the panel's top-left corner, marks it dirty.
pub fn Panel_move(this: &mut Panel, x: i32, y: i32) {
    this.x = x;
    this.y = y;
    this.needsRedraw = true;
}

/// Port of `Panel.c:104`. Sets the panel's width/height, marks it dirty.
pub fn Panel_resize(this: &mut Panel, w: i32, h: i32) {
    this.w = w;
    this.h = h;
    this.needsRedraw = true;
}

/// Port of `Panel.c:112`. Empties the item list and resets selection/
/// scroll state (C `Vector_prune` clears the vector).
pub fn Panel_prune(this: &mut Panel) {
    this.items.clear();
    this.prevSelected = -1;
    this.scrollV = 0;
    this.selected = 0;
    this.oldSelected = 0;
    this.needsRedraw = true;
}

/// Port of `Panel.c:123`. Appends `o` to the item list.
pub fn Panel_add(this: &mut Panel, o: Box<dyn Object>) {
    this.items.push(o);
    this.prevSelected = -1;
    this.needsRedraw = true;
}

/// Port of `Panel.c:131`. Inserts `o` at index `i`.
pub fn Panel_insert(this: &mut Panel, i: i32, o: Box<dyn Object>) {
    this.items.insert(i as usize, o);
    this.prevSelected = -1;
    this.needsRedraw = true;
}

/// Port of `Panel.c:139`. Replaces the item at index `i` with `o`
/// (C `Vector_set`).
pub fn Panel_set(this: &mut Panel, i: i32, o: Box<dyn Object>) {
    this.items[i as usize] = o;
}

/// Port of `Panel.c:145`. Returns the item at index `i` (C `Vector_get`,
/// which asserts `i` in range).
pub fn Panel_get(this: &Panel, i: i32) -> &dyn Object {
    this.items[i as usize].as_ref()
}

/// Port of `Panel.c:151`. Removes and returns the item at index `i`,
/// decrementing `selected` when it fell off the (now shorter) end.
pub fn Panel_remove(this: &mut Panel, i: i32) -> Box<dyn Object> {
    this.needsRedraw = true;
    let removed = this.items.remove(i as usize);
    this.prevSelected = -1;
    if this.selected > 0 && this.selected >= this.items.len() as i32 {
        this.selected -= 1;
    }
    removed
}

/// Port of `Panel.c:164`. Returns the selected item, or `None` when the
/// list is empty.
pub fn Panel_getSelected(this: &Panel) -> Option<&dyn Object> {
    if !this.items.is_empty() {
        Some(this.items[this.selected as usize].as_ref())
    } else {
        None
    }
}

/// Port of `Panel.c:174`. Swaps the selected item with the one above it
/// (C `Vector_moveUp`), then decrements `selected`.
pub fn Panel_moveSelectedUp(this: &mut Panel) {
    let idx = this.selected;
    // Vector_moveUp: no-op at idx 0, else swap idx with idx-1.
    if idx > 0 && (idx as usize) < this.items.len() {
        this.items.swap(idx as usize, (idx - 1) as usize);
    }
    this.prevSelected = -1;
    if this.selected > 0 {
        this.selected -= 1;
    }
}

/// Port of `Panel.c:184`. Swaps the selected item with the one below it
/// (C `Vector_moveDown`), then increments `selected`.
pub fn Panel_moveSelectedDown(this: &mut Panel) {
    let idx = this.selected;
    let size = this.items.len() as i32;
    // Vector_moveDown: no-op at the last index, else swap idx with idx+1.
    if idx >= 0 && idx < size - 1 {
        this.items.swap(idx as usize, (idx + 1) as usize);
    }
    this.prevSelected = -1;
    if this.selected + 1 < size {
        this.selected += 1;
    }
}

/// Port of `Panel.c:194`. Returns the current selection index.
pub fn Panel_getSelectedIndex(this: &Panel) -> i32 {
    this.selected
}

/// Port of `Panel.c:200`. Returns the item count (C `Vector_size`).
pub fn Panel_size(this: &Panel) -> i32 {
    this.items.len() as i32
}

/// Port of `Panel.c:206`. Clamps `selected` into `[0, size)` and stores it.
///
/// The C tail `if (Panel_eventHandlerFn(this)) Panel_eventHandler(this,
/// EVENT_SET_SELECTED);` dispatches through the `PanelClass` vtable. The
/// base `Panel_class.eventHandler` is `Panel_selectByTyping`, which is a
/// no-op for `EVENT_SET_SELECTED` (`ch == ERR == -1` falls through to
/// `IGNORED`), so the base behavior is exactly this clamp. Subclass
/// overrides (e.g. MainPanel follow-mode) need the unmodeled vtable.
pub fn Panel_setSelected(this: &mut Panel, selected: i32) {
    let size = this.items.len() as i32;
    let mut selected = selected;
    if selected >= size {
        selected = size - 1;
    }
    if selected < 0 {
        selected = 0;
    }
    this.selected = selected;
}

/// TODO: port of `void Panel_splice(Panel* this, Vector* from)` from
/// `Panel.c:222`. Takes a `Vector* from` — the unported `Vector` type has
/// no analog in this `Vec`-backed model.
pub fn Panel_splice() {
    todo!("port of Panel.c:222 — needs the unported Vector type as the source")
}

/// Port of `void Panel_draw(Panel* this, bool force_redraw, bool focus,
/// bool highlightSelected, bool hideFunctionBar)` from `Panel.c:231`.
///
/// Behavioral crossterm port. Reproduces htop's header line, the scroll
/// clamp ("scroll follows selection", factored into
/// [`Panel::ensure_scroll`]), the per-row `Object_display` → `RichString`
/// → blit with selection highlight, the trailing blank fill, and the
/// focused function-bar draw — emitting through the [`Ncurses`] shim.
/// The base-class vtable branches are taken for `printHeader`/
/// `drawFunctionBar` (both NULL on `Panel_class`); subclass overrides need
/// the unmodeled vtable.
pub fn Panel_draw(
    this: &mut Panel,
    force_redraw: bool,
    focus: bool,
    highlightSelected: bool,
    hideFunctionBar: bool,
) {
    let size = this.items.len() as i32;
    let scrollH = this.scrollH;
    let mut y = this.y;
    let x = this.x;
    let w = this.w;
    let mut h = this.h;

    if hideFunctionBar {
        h += 1;
    }

    let header_attr = if focus {
        ColorElements::PANEL_HEADER_FOCUS.packed(ColorScheme::active())
    } else {
        ColorElements::PANEL_HEADER_UNFOCUS.packed(ColorScheme::active())
    };

    let mut out = io::stdout().lock();

    if force_redraw {
        // Base Panel_class has no printHeader vtable slot -> the else branch.
        RichString_setAttr(&mut this.header, header_attr);
    }
    let header_len = RichString_sizeVal(&this.header);
    if header_len > 0 {
        Ncurses::attrset(&mut out, header_attr);
        Ncurses::mvhline(&mut out, y, x, ' ', w);
        if scrollH < header_len {
            Panel::print_offset(
                &mut out,
                y,
                x,
                &this.header,
                scrollH,
                (header_len - scrollH).min(w),
            );
        }
        Ncurses::attrset(&mut out, ColorElements::RESET_COLOR.packed(ColorScheme::active()));
        y += 1;
        h -= 1;
    }

    this.ensure_scroll(size, h);

    let first = this.scrollV;
    let up_to = (first + h).min(size);

    let selection_color = if focus {
        this.selectionColorId.packed(ColorScheme::active())
    } else {
        ColorElements::PANEL_SELECTION_UNFOCUS.packed(ColorScheme::active())
    };

    let mut item = RichString::new();
    if this.needsRedraw || force_redraw {
        let mut line = 0i32;
        let mut i = first;
        while line < h && i < up_to {
            let mut highlight_attr = 0i32;
            {
                let item_obj: &dyn Object = this.items[i as usize].as_ref();
                let sz = RichString_size(&item);
                RichString_rewind(&mut item, sz);
                item.highlightAttr = 0;
                item_obj.display(&mut item);
            }
            let item_len = RichString_sizeVal(&item);
            let amt = (item_len - scrollH).min(w);
            if highlightSelected && i == this.selected {
                item.highlightAttr = selection_color;
                highlight_attr = selection_color;
            }
            if item.highlightAttr != 0 {
                Ncurses::attrset(&mut out, item.highlightAttr);
                let ha = item.highlightAttr;
                RichString_setAttr(&mut item, ha);
                this.selectedLen = item_len as usize;
                highlight_attr = item.highlightAttr;
            }
            Ncurses::mvhline(&mut out, y + line, x, ' ', w);
            if amt > 0 {
                Panel::print_offset(&mut out, y + line, x, &item, scrollH, amt);
            }
            if highlight_attr != 0 {
                Ncurses::attrset(&mut out, ColorElements::RESET_COLOR.packed(ColorScheme::active()));
            }
            line += 1;
            i += 1;
        }
        while line < h {
            Ncurses::mvhline(&mut out, y + line, x, ' ', w);
            line += 1;
        }
    } else {
        let old_selected = this.oldSelected;
        {
            let old_obj: &dyn Object = this.items[old_selected as usize].as_ref();
            let sz = RichString_size(&item);
                RichString_rewind(&mut item, sz);
            old_obj.display(&mut item);
        }
        let old_len = RichString_sizeVal(&item);
        Ncurses::mvhline(&mut out, y + old_selected - first, x, ' ', w);
        if scrollH < old_len {
            Panel::print_offset(
                &mut out,
                y + old_selected - first,
                x,
                &item,
                scrollH,
                (old_len - scrollH).min(w),
            );
        }

        let selected = this.selected;
        {
            let new_obj: &dyn Object = this.items[selected as usize].as_ref();
            let sz = RichString_size(&item);
                RichString_rewind(&mut item, sz);
            item.highlightAttr = 0;
            new_obj.display(&mut item);
        }
        let new_len = RichString_sizeVal(&item);
        this.selectedLen = new_len as usize;
        Ncurses::attrset(&mut out, selection_color);
        Ncurses::mvhline(&mut out, y + selected - first, x, ' ', w);
        RichString_setAttr(&mut item, selection_color);
        if scrollH < new_len {
            Panel::print_offset(
                &mut out,
                y + selected - first,
                x,
                &item,
                scrollH,
                (new_len - scrollH).min(w),
            );
        }
        Ncurses::attrset(&mut out, ColorElements::RESET_COLOR.packed(ColorScheme::active()));
    }

    if focus && (this.needsRedraw || force_redraw || !this.wasFocus) {
        // Base Panel_class has no drawFunctionBar vtable slot -> the else branch.
        if !hideFunctionBar {
            if let Some(bar) = &this.currentBar {
                FunctionBar_draw(bar);
            }
        }
    }

    let _ = out.flush();

    this.oldSelected = this.selected;
    this.wasFocus = focus;
    this.needsRedraw = false;
}

/// Port of `static int Panel_headerHeight(const Panel* this)` from
/// `Panel.c:357`: `1` when the header is non-empty, else `0`.
pub fn Panel_headerHeight(this: &Panel) -> i32 {
    if RichString_sizeVal(&this.header) > 0 {
        1
    } else {
        0
    }
}

/// Port of `bool Panel_onKey(Panel* this, int key)` from `Panel.c:363`.
///
/// Navigation/scroll key handling. Ported faithfully against the item
/// count and `CRT_scrollHAmount` (`crt.rs`). The `KEY_WHEELUP`/
/// `KEY_WHEELDOWN` arms need `CRT_scrollWheelVAmount` (`CRT.c:956`), which
/// is not ported in `crt.rs`, so those two arms are stubbed per the port
/// rules. Returns `true` when `key` was handled, `false` for the default
/// (unhandled) case.
pub fn Panel_onKey(this: &mut Panel, key: i32) -> bool {
    let size = this.items.len() as i32;
    let scroll_h_amount = crt::CRT_scrollHAmount.load(Ordering::Relaxed);

    match key {
        KEY_DOWN | CTRL_N => {
            this.selected += 1;
        }
        KEY_UP | CTRL_P => {
            this.selected -= 1;
        }
        KEY_LEFT | CTRL_B => {
            if this.scrollH > 0 {
                this.scrollH -= scroll_h_amount.max(0);
                this.needsRedraw = true;
            }
        }
        KEY_RIGHT | CTRL_F => {
            this.scrollH += scroll_h_amount;
            this.needsRedraw = true;
        }
        KEY_PPAGE => {
            let amt = this.h - Panel_headerHeight(this);
            this.panel_scroll(-amt, size);
        }
        KEY_NPAGE => {
            let amt = this.h - Panel_headerHeight(this);
            this.panel_scroll(amt, size);
        }
        KEY_WHEELUP => {
            todo!("Panel.c:415 — KEY_WHEELUP needs CRT_scrollWheelVAmount (CRT.c:956), not ported in crt.rs");
        }
        KEY_WHEELDOWN => {
            todo!("Panel.c:419 — KEY_WHEELDOWN needs CRT_scrollWheelVAmount (CRT.c:956), not ported in crt.rs");
        }
        KEY_HOME => {
            this.selected = 0;
        }
        KEY_END => {
            this.selected = size - 1;
        }
        CTRL_A | CARET => {
            this.scrollH = 0;
            this.needsRedraw = true;
        }
        CTRL_E | DOLLAR => {
            debug_assert!(this.w > 0);
            if this.selectedLen < this.w as usize {
                this.scrollH = 0;
            } else if this.selectedLen - (this.w as usize) > i32::MAX as usize {
                this.scrollH = i32::MAX;
            } else {
                this.scrollH = (this.selectedLen - (this.w as usize)) as i32;
            }
            this.needsRedraw = true;
        }
        _ => return false,
    }

    // ensure selection within bounds
    if this.selected < 0 || size == 0 {
        this.selected = 0;
        this.needsRedraw = true;
    } else if this.selected >= size {
        this.selected = size - 1;
        this.needsRedraw = true;
    }

    true
}

/// TODO: port of `HandlerResult Panel_selectByTyping(Panel* this, int ch)`
/// from `Panel.c:468`. Needs the `eventHandlerState` scratch buffer,
/// `ListItem` value downcasts, `Panel_setSelected`, and the
/// `HandlerResult` enum — search substrate not ported here.
pub fn Panel_selectByTyping() {
    todo!("port of Panel.c:468 — needs eventHandlerState + HandlerResult substrate")
}

/// Port of `int Panel_getCh(Panel* this)` from `Panel.c:526`.
///
/// Behavioral crossterm port. Positions/shows the cursor when `cursorOn`
/// (C `move`+`curs_set(1)`), else hides it, then reads a key via the
/// ported `crt::CRT_readKey` (C `getch`). The `set_escdelay(25)` tuning
/// has no crossterm analog and is dropped.
pub fn Panel_getCh(this: &Panel) -> i32 {
    let mut out = io::stdout().lock();
    if this.cursorOn {
        Ncurses::move_to(&mut out, this.cursorY, this.cursorX);
        Ncurses::curs_set(&mut out, true);
    } else {
        Ncurses::curs_set(&mut out, false);
    }
    let _ = out.flush();
    crt::CRT_readKey()
}

impl Panel {
    /// Pure scroll clamp from `Panel_draw` (`Panel.c:265-280`): keeps the
    /// scroll area and the selection on screen, mutating `scrollV`/
    /// `needsRedraw`. Factored out so the "scroll follows selection"
    /// behavior is unit-testable without a TTY. `h` is the drawable row
    /// count after the header adjustment.
    fn ensure_scroll(&mut self, size: i32, h: i32) {
        if self.scrollV < 0 {
            self.scrollV = 0;
            self.needsRedraw = true;
        } else if self.scrollV > size - h {
            self.scrollV = (size - h).max(0);
            self.needsRedraw = true;
        }
        if self.selected < self.scrollV {
            self.scrollV = self.selected;
            self.needsRedraw = true;
        } else if self.selected >= self.scrollV + h {
            self.scrollV = self.selected - h + 1;
            self.needsRedraw = true;
        }
    }

    /// The `PANEL_SCROLL(amount)` macro body (`Panel.c:368`): shift the
    /// selection and clamp `scrollV` into `[0, MAX(0, size - h - headerHeight)]`.
    fn panel_scroll(&mut self, amount: i32, size: i32) {
        self.selected += amount;
        let hi = (size - self.h - Panel_headerHeight(self)).max(0);
        self.scrollV = (self.scrollV + amount).clamp(0, hi);
        self.needsRedraw = true;
    }

    /// Reproduces the `RichString_printoffnVal(item, y, x, off, n)` blit
    /// (`RichString.h:28` = `mvadd_wchnstr(y, x, chptr + off, n)`): print
    /// `n` cells starting at cell `off`, each carrying its own attribute.
    /// A gate-skipped associated fn using the `RichString`'s public cell
    /// data (not re-implementing a `RichString` string op); the missing
    /// `RichString_printoffnVal` C macro is an ncurses blit with no ported
    /// analog, so the blit lives with the draw code that needs it.
    fn print_offset<W: Write>(
        out: &mut W,
        y: i32,
        x: i32,
        item: &RichString,
        off: i32,
        n: i32,
    ) {
        for k in 0..n {
            let idx = (off + k) as usize;
            if idx >= item.chptr.len() {
                break;
            }
            let cell = item.chptr[idx];
            Ncurses::attrset(out, cell.attr);
            Ncurses::mvaddch(out, y, x + k, cell.chars);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::listitem::ListItem;

    fn blank() -> Panel {
        Panel::empty()
    }

    fn li(value: &str) -> Box<dyn Object> {
        Box::new(ListItem {
            value: value.to_string(),
            key: 0,
            moving: false,
        })
    }

    fn fill(p: &mut Panel, n: usize) {
        for i in 0..n {
            p.items.push(li(&format!("item{i}")));
        }
    }

    // ── field arithmetic ──────────────────────────────────────────────

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
        assert_eq!(p.cursorY, 11); // 5 + 7 - 2 + 1
        assert_eq!(p.cursorX, 19); // 3 + 20 - 4
    }

    #[test]
    fn set_cursor_to_selection_truncates_selectedlen_like_int_cast() {
        let mut p = blank();
        p.selectedLen = 0x1_0000_0007;
        Panel_setCursorToSelection(&mut p);
        assert_eq!(p.cursorX, 7);
    }

    #[test]
    fn set_selection_color_stores_value() {
        let mut p = blank();
        Panel_setSelectionColor(&mut p, ColorElements::PANEL_SELECTION_FOLLOW);
        assert_eq!(p.selectionColorId, ColorElements::PANEL_SELECTION_FOLLOW);
        Panel_setSelectionColor(&mut p, ColorElements::PANEL_SELECTION_UNFOCUS);
        assert_eq!(p.selectionColorId, ColorElements::PANEL_SELECTION_UNFOCUS);
    }

    #[test]
    fn move_sets_position_and_dirties() {
        let mut p = blank();
        p.needsRedraw = false;
        Panel_move(&mut p, 12, 34);
        assert_eq!((p.x, p.y), (12, 34));
        assert!(p.needsRedraw);
    }

    #[test]
    fn resize_sets_dimensions_and_dirties() {
        let mut p = blank();
        p.needsRedraw = false;
        Panel_resize(&mut p, 80, 24);
        assert_eq!((p.w, p.h), (80, 24));
        assert!(p.needsRedraw);
    }

    #[test]
    fn get_selected_index_returns_field() {
        let mut p = blank();
        p.selected = 9;
        assert_eq!(Panel_getSelectedIndex(&p), 9);
    }

    // ── init / new ────────────────────────────────────────────────────

    #[test]
    fn init_sets_all_fields() {
        let mut p = blank();
        p.selected = 5;
        p.scrollV = 3;
        let bar = FunctionBar {
            functions: vec!["x".into()],
            keys: vec!["F1".into()],
            events: vec![1],
            staticData: false,
        };
        Panel_init(&mut p, 1, 2, 3, 4, Some(bar));
        assert_eq!((p.x, p.y, p.w, p.h), (1, 2, 3, 4));
        assert_eq!(p.selected, 0);
        assert_eq!(p.scrollV, 0);
        assert_eq!(p.prevSelected, -1);
        assert!(p.needsRedraw);
        assert!(p.items.is_empty());
        assert_eq!(p.selectionColorId, ColorElements::PANEL_SELECTION_FOCUS);
        assert!(p.currentBar.is_some());
        assert!(p.defaultBar.is_some());
    }

    #[test]
    fn new_builds_initialized_panel() {
        let p = Panel_new(0, 0, 10, 5, None);
        assert_eq!((p.w, p.h), (10, 5));
        assert!(p.items.is_empty());
        assert!(p.needsRedraw);
    }

    #[test]
    fn set_default_bar_restores_current_from_default() {
        let default_bar = FunctionBar {
            functions: vec!["DEFAULT".into()],
            keys: vec!["F1".into()],
            events: vec![1],
            staticData: false,
        };
        let mut p = Panel_new(0, 0, 10, 5, Some(default_bar));
        // Swap currentBar out to a different bar (as an IncSet would).
        p.currentBar = Some(FunctionBar {
            functions: vec!["SEARCH".into()],
            keys: vec!["Esc".into()],
            events: vec![2],
            staticData: false,
        });
        Panel_setDefaultBar(&mut p);
        // currentBar is now the defaultBar's content again.
        assert_eq!(
            p.currentBar.as_ref().unwrap().functions,
            vec!["DEFAULT".to_string()]
        );
        assert!(p.defaultBar.is_some());
    }

    // ── list ops ──────────────────────────────────────────────────────

    #[test]
    fn add_and_size_and_get() {
        let mut p = blank();
        Panel_add(&mut p, li("a"));
        Panel_add(&mut p, li("b"));
        assert_eq!(Panel_size(&p), 2);
        assert_eq!(p.prevSelected, -1);
        // Panel_get returns the object; verify via its ListItem value.
        let any: &dyn std::any::Any = Panel_get(&p, 1);
        assert_eq!(any.downcast_ref::<ListItem>().unwrap().value, "b");
    }

    #[test]
    fn insert_and_set() {
        let mut p = blank();
        Panel_add(&mut p, li("a"));
        Panel_add(&mut p, li("c"));
        Panel_insert(&mut p, 1, li("b"));
        assert_eq!(Panel_size(&p), 3);
        let any: &dyn std::any::Any = Panel_get(&p, 1);
        assert_eq!(any.downcast_ref::<ListItem>().unwrap().value, "b");
        Panel_set(&mut p, 0, li("Z"));
        let any0: &dyn std::any::Any = Panel_get(&p, 0);
        assert_eq!(any0.downcast_ref::<ListItem>().unwrap().value, "Z");
    }

    #[test]
    fn remove_decrements_selected_when_at_end() {
        let mut p = blank();
        fill(&mut p, 3); // item0,item1,item2
        p.selected = 2;
        let removed = Panel_remove(&mut p, 2);
        let any: &dyn std::any::Any = removed.as_ref();
        assert_eq!(any.downcast_ref::<ListItem>().unwrap().value, "item2");
        assert_eq!(Panel_size(&p), 2);
        // selected was == size, decremented to 1
        assert_eq!(p.selected, 1);
    }

    #[test]
    fn remove_keeps_selected_when_in_range() {
        let mut p = blank();
        fill(&mut p, 3);
        p.selected = 0;
        Panel_remove(&mut p, 2);
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn get_selected_empty_is_none() {
        let p = blank();
        assert!(Panel_getSelected(&p).is_none());
    }

    #[test]
    fn get_selected_returns_current() {
        let mut p = blank();
        fill(&mut p, 3);
        p.selected = 1;
        let sel: &dyn std::any::Any = Panel_getSelected(&p).unwrap();
        assert_eq!(sel.downcast_ref::<ListItem>().unwrap().value, "item1");
    }

    #[test]
    fn move_selected_up_swaps_and_decrements() {
        let mut p = blank();
        fill(&mut p, 3); // item0,item1,item2
        p.selected = 2;
        Panel_moveSelectedUp(&mut p);
        // item2 swapped with item1; selection follows up to 1
        let a1: &dyn std::any::Any = Panel_get(&p, 1);
        let a2: &dyn std::any::Any = Panel_get(&p, 2);
        assert_eq!(a1.downcast_ref::<ListItem>().unwrap().value, "item2");
        assert_eq!(a2.downcast_ref::<ListItem>().unwrap().value, "item1");
        assert_eq!(p.selected, 1);
    }

    #[test]
    fn move_selected_up_at_top_is_noop() {
        let mut p = blank();
        fill(&mut p, 3);
        p.selected = 0;
        Panel_moveSelectedUp(&mut p);
        let a0: &dyn std::any::Any = Panel_get(&p, 0);
        assert_eq!(a0.downcast_ref::<ListItem>().unwrap().value, "item0");
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn move_selected_down_swaps_and_increments() {
        let mut p = blank();
        fill(&mut p, 3);
        p.selected = 0;
        Panel_moveSelectedDown(&mut p);
        let a0: &dyn std::any::Any = Panel_get(&p, 0);
        let a1: &dyn std::any::Any = Panel_get(&p, 1);
        assert_eq!(a0.downcast_ref::<ListItem>().unwrap().value, "item1");
        assert_eq!(a1.downcast_ref::<ListItem>().unwrap().value, "item0");
        assert_eq!(p.selected, 1);
    }

    #[test]
    fn move_selected_down_at_bottom_is_noop() {
        let mut p = blank();
        fill(&mut p, 3);
        p.selected = 2;
        Panel_moveSelectedDown(&mut p);
        let a2: &dyn std::any::Any = Panel_get(&p, 2);
        assert_eq!(a2.downcast_ref::<ListItem>().unwrap().value, "item2");
        assert_eq!(p.selected, 2);
    }

    #[test]
    fn prune_clears_and_resets() {
        let mut p = blank();
        fill(&mut p, 4);
        p.selected = 3;
        p.scrollV = 2;
        Panel_prune(&mut p);
        assert_eq!(Panel_size(&p), 0);
        assert_eq!(p.selected, 0);
        assert_eq!(p.scrollV, 0);
        assert_eq!(p.oldSelected, 0);
        assert_eq!(p.prevSelected, -1);
        assert!(p.needsRedraw);
    }

    // ── setSelected clamp ─────────────────────────────────────────────

    #[test]
    fn set_selected_clamps_high_low_and_empty() {
        let mut p = blank();
        fill(&mut p, 5); // valid 0..4
        Panel_setSelected(&mut p, 10);
        assert_eq!(p.selected, 4); // clamped to size-1
        Panel_setSelected(&mut p, -3);
        assert_eq!(p.selected, 0); // clamped to 0
        Panel_setSelected(&mut p, 2);
        assert_eq!(p.selected, 2);

        let mut empty = blank();
        // size 0: selected>=size -> size-1 == -1, then <0 -> 0
        Panel_setSelected(&mut empty, 4);
        assert_eq!(empty.selected, 0);
    }

    // ── headerHeight / setHeader ──────────────────────────────────────

    #[test]
    fn header_height_reflects_header_content() {
        let mut p = blank();
        assert_eq!(Panel_headerHeight(&p), 0);
        Panel_setHeader(&mut p, "PID USER");
        assert_eq!(Panel_headerHeight(&p), 1);
        assert!(p.needsRedraw);
        assert_eq!(RichString_sizeVal(&p.header), "PID USER".len() as i32);
    }

    // ── onKey navigation ──────────────────────────────────────────────

    #[test]
    fn onkey_down_up_move_selection_within_bounds() {
        let mut p = blank();
        fill(&mut p, 3);
        p.h = 3;
        assert!(Panel_onKey(&mut p, KEY_DOWN));
        assert_eq!(p.selected, 1);
        assert!(Panel_onKey(&mut p, KEY_DOWN));
        assert_eq!(p.selected, 2);
        // at last item, DOWN clamps to size-1
        assert!(Panel_onKey(&mut p, KEY_DOWN));
        assert_eq!(p.selected, 2);
        assert!(Panel_onKey(&mut p, KEY_UP));
        assert_eq!(p.selected, 1);
    }

    #[test]
    fn onkey_up_at_top_clamps_to_zero() {
        let mut p = blank();
        fill(&mut p, 3);
        p.selected = 0;
        assert!(Panel_onKey(&mut p, KEY_UP)); // -> -1 -> clamp 0
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn onkey_home_end() {
        let mut p = blank();
        fill(&mut p, 5);
        p.selected = 2;
        assert!(Panel_onKey(&mut p, KEY_END));
        assert_eq!(p.selected, 4);
        assert!(Panel_onKey(&mut p, KEY_HOME));
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn onkey_left_right_scroll_horizontal() {
        let mut p = blank();
        fill(&mut p, 3);
        let step = crt::CRT_scrollHAmount.load(Ordering::Relaxed);
        assert!(Panel_onKey(&mut p, KEY_RIGHT));
        assert_eq!(p.scrollH, step);
        assert!(Panel_onKey(&mut p, KEY_LEFT));
        assert_eq!(p.scrollH, 0);
        // LEFT at scrollH 0 is a no-op
        assert!(Panel_onKey(&mut p, KEY_LEFT));
        assert_eq!(p.scrollH, 0);
    }

    #[test]
    fn onkey_caret_resets_scrollh_and_dollar_scrolls_to_end() {
        let mut p = blank();
        fill(&mut p, 3);
        p.w = 10;
        p.scrollH = 7;
        assert!(Panel_onKey(&mut p, CARET));
        assert_eq!(p.scrollH, 0);
        // '$': selectedLen (25) - w (10) = 15
        p.selectedLen = 25;
        assert!(Panel_onKey(&mut p, DOLLAR));
        assert_eq!(p.scrollH, 15);
        // selectedLen < w -> scrollH 0
        p.selectedLen = 4;
        assert!(Panel_onKey(&mut p, DOLLAR));
        assert_eq!(p.scrollH, 0);
    }

    #[test]
    fn onkey_page_down_up_scrolls_by_page() {
        let mut p = blank();
        fill(&mut p, 20);
        p.h = 5; // no header -> headerHeight 0
        assert!(Panel_onKey(&mut p, KEY_NPAGE));
        // selected += 5; scrollV clamped to [0, 20-5-0]=15 -> 5
        assert_eq!(p.selected, 5);
        assert_eq!(p.scrollV, 5);
        assert!(Panel_onKey(&mut p, KEY_PPAGE));
        assert_eq!(p.selected, 0);
        assert_eq!(p.scrollV, 0);
    }

    #[test]
    fn onkey_unhandled_returns_false() {
        let mut p = blank();
        fill(&mut p, 3);
        assert!(!Panel_onKey(&mut p, b'z' as i32));
    }

    // ── ensure_scroll (scroll-follows-selection) ──────────────────────

    #[test]
    fn ensure_scroll_pulls_selection_into_view_downward() {
        let mut p = blank();
        fill(&mut p, 20);
        p.h = 5;
        p.selected = 10;
        p.scrollV = 0;
        p.ensure_scroll(20, 5);
        // selected >= scrollV + h  ->  scrollV = selected - h + 1 = 6
        assert_eq!(p.scrollV, 6);
        assert!(p.needsRedraw);
    }

    #[test]
    fn ensure_scroll_pulls_selection_into_view_upward() {
        let mut p = blank();
        fill(&mut p, 20);
        p.h = 5;
        p.selected = 2;
        p.scrollV = 8;
        p.ensure_scroll(20, 5);
        // selected < scrollV -> scrollV = selected = 2
        assert_eq!(p.scrollV, 2);
    }

    #[test]
    fn ensure_scroll_clamps_negative_and_overshoot() {
        let mut p = blank();
        fill(&mut p, 4);
        p.h = 10;
        p.selected = 0;
        p.scrollV = -3;
        p.ensure_scroll(4, 10);
        assert_eq!(p.scrollV, 0);

        // scrollV beyond size-h (which is negative) -> MAX(size-h,0) = 0
        p.scrollV = 5;
        p.ensure_scroll(4, 10);
        assert_eq!(p.scrollV, 0);
    }
}
