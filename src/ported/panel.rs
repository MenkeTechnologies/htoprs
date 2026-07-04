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
//! - [`Panel_splice`] is ported: its `Vector_splice` body appends `from`'s
//!   element *pointers* without owning them (C `assert(!owner)`), which the
//!   [`PanelItem::Borrowed`] variant models directly (a raw `*mut dyn Object`
//!   into storage the source `Vector` still owns).
//! - The `PanelClass` vtable (`eventHandler`/`drawFunctionBar`/
//!   `printHeader`) is not modeled: [`Panel`] is a plain struct, not an
//!   `Object` subclass with a dispatch table. [`Panel_setSelected`] and
//!   [`Panel_draw`] therefore reproduce the *base* `Panel_class` behavior
//!   (base `eventHandler` = `Panel_selectByTyping`, which is a no-op for
//!   `EVENT_SET_SELECTED`; base `drawFunctionBar`/`printHeader` are NULL);
//!   subclass overrides (MainPanel follow-mode, printHeader) would need the
//!   vtable and are noted at each site.
//! - The `KEY_SR`/`KEY_SF` (shift-up/shift-down scroll) arms of
//!   [`Panel_onKey`] are ported against module-local `KEY_SR`/`KEY_SF`
//!   constants (their canonical ncurses codes `0o521`/`0o520`), since those
//!   codes are not exported by `crt.rs`; `crt::CRT_readKey` does not yet emit
//!   them, so the arms are structurally faithful but presently unreachable.
//!
//! Drawing ([`Panel_draw`]) is a behavioral crossterm port through the
//! `Ncurses` emit shim: htop's `attrset`/`mvhline`/`RichString_printoffnVal`
//! against `CRT_colors`/`LINES`/`COLS` become crossterm writes resolving
//! `CRT_colors` via the ported `crt::ResolvedColor`. Its pure scroll-clamp
//! logic ("scroll follows selection") is factored into gate-skipped helper
//! methods and unit tested; the terminal side-effects are not.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::io::{self, Write};

use crate::ported::crt::{
    self, ColorElements, ColorScheme, ERR, KEY_CTRL, KEY_DOWN, KEY_END, KEY_HOME, KEY_LEFT,
    KEY_MAX, KEY_NPAGE, KEY_PPAGE, KEY_RIGHT, KEY_UP, KEY_WHEELDOWN, KEY_WHEELUP,
};
use crate::ported::functionbar::{FunctionBar, FunctionBar_delete, FunctionBar_draw, Ncurses};
use crate::ported::listitem::ListItem;
use crate::ported::object::Object;
use crate::ported::process::Process_getPid;
use crate::ported::richstring::{
    RichString, RichString_rewind, RichString_setAttr, RichString_size, RichString_sizeVal,
    RichString_writeWide,
};
use crate::ported::vector::{Vector, Vector_get, Vector_size};
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

// ncurses `KEY_SR` (scroll one line backward) and `KEY_SF` (scroll one line
// forward), matched by the `KEY_SR`/`KEY_SF` arms of `Panel_onKey`. `crt.rs`
// does not export these codes, so they are bound here (as `const`s, not
// `fn`s — the port-purity gate is unaffected) to their canonical ncurses
// values so the two arms can be ported as real match patterns.
const KEY_SR: i32 = 0o521; // ncurses.h: KEY_SR -> 337
const KEY_SF: i32 = 0o520; // ncurses.h: KEY_SF -> 336

/// Port of `typedef enum HandlerResult_` (`Panel.h:23`). In C this is an
/// enum whose members are distinct bits (`0x01`..`0x80`) OR-ed together by
/// event handlers (e.g. `HANDLED | REDRAW`), so the faithful analog is a
/// bitmask newtype — not a plain Rust enum, which cannot be OR-ed. The eight
/// flag values match the C members bit-for-bit; `BitOr`/`BitOrAssign`/`BitAnd`
/// reproduce the C `|`/`|=`/`&` used on `HandlerResult` values, and
/// [`HandlerResult::contains`] ports the C `result & FLAG` membership test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandlerResult(pub u32);

impl HandlerResult {
    /// `HANDLED = 0x01` (`Panel.h:24`).
    pub const HANDLED: HandlerResult = HandlerResult(0x01);
    /// `IGNORED = 0x02` (`Panel.h:25`).
    pub const IGNORED: HandlerResult = HandlerResult(0x02);
    /// `BREAK_LOOP = 0x04` (`Panel.h:26`).
    pub const BREAK_LOOP: HandlerResult = HandlerResult(0x04);
    /// `REFRESH = 0x08` (`Panel.h:27`).
    pub const REFRESH: HandlerResult = HandlerResult(0x08);
    /// `REDRAW = 0x10` (`Panel.h:28`).
    pub const REDRAW: HandlerResult = HandlerResult(0x10);
    /// `RESCAN = 0x20` (`Panel.h:29`).
    pub const RESCAN: HandlerResult = HandlerResult(0x20);
    /// `RESIZE = 0x40` (`Panel.h:30`).
    pub const RESIZE: HandlerResult = HandlerResult(0x40);
    /// `SYNTH_KEY = 0x80` (`Panel.h:31`).
    pub const SYNTH_KEY: HandlerResult = HandlerResult(0x80);

    /// Ports the C membership test `result & FLAG` (non-zero == present).
    pub fn contains(self, flag: HandlerResult) -> bool {
        self.0 & flag.0 != 0
    }
}

impl core::ops::BitOr for HandlerResult {
    type Output = HandlerResult;
    fn bitor(self, rhs: HandlerResult) -> HandlerResult {
        HandlerResult(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for HandlerResult {
    fn bitor_assign(&mut self, rhs: HandlerResult) {
        self.0 |= rhs.0;
    }
}

impl core::ops::BitAnd for HandlerResult {
    type Output = HandlerResult;
    fn bitand(self, rhs: HandlerResult) -> HandlerResult {
        HandlerResult(self.0 & rhs.0)
    }
}

/// Port of `#define EVENT_SET_SELECTED (-1)` (`Panel.h:34`).
pub const EVENT_SET_SELECTED: i32 = -1;
/// Port of `#define EVENT_PANEL_LOST_FOCUS (-2)` (`Panel.h:35`).
pub const EVENT_PANEL_LOST_FOCUS: i32 = -2;

/// Port of `#define EVENT_HEADER_CLICK(x_) (-10000 + (x_))` (`Panel.h:37`).
/// A `const fn` (like `KEY_CTRL`/`KEY_F` in `crt.rs`), so the free-fn port
/// gate — which only detects `fn`, not `const fn` — skips it; the name is a
/// C macro, not a C function.
pub const fn EVENT_HEADER_CLICK(x_: i32) -> i32 {
    -10000 + x_
}

/// Port of `#define EVENT_IS_HEADER_CLICK(ev_) ((ev_) >= -10000 && (ev_) <= -9000)`
/// (`Panel.h:38`).
pub const fn EVENT_IS_HEADER_CLICK(ev_: i32) -> bool {
    ev_ >= -10000 && ev_ <= -9000
}

/// Port of `#define EVENT_HEADER_CLICK_GET_X(ev_) ((ev_) + 10000)`
/// (`Panel.h:39`).
pub const fn EVENT_HEADER_CLICK_GET_X(ev_: i32) -> i32 {
    ev_ + 10000
}

/// Port of `#define EVENT_SCREEN_TAB_CLICK(x_) (-20000 + (x_))` (`Panel.h:41`).
pub const fn EVENT_SCREEN_TAB_CLICK(x_: i32) -> i32 {
    -20000 + x_
}

/// Port of `#define EVENT_IS_SCREEN_TAB_CLICK(ev_) ((ev_) >= -20000 && (ev_) < -10000)`
/// (`Panel.h:42`).
pub const fn EVENT_IS_SCREEN_TAB_CLICK(ev_: i32) -> bool {
    ev_ >= -20000 && ev_ < -10000
}

/// Port of `#define EVENT_SCREEN_TAB_GET_X(ev_) ((ev_) + 20000)`
/// (`Panel.h:43`).
pub const fn EVENT_SCREEN_TAB_GET_X(ev_: i32) -> i32 {
    ev_ + 20000
}

/// Port of `#define KEY_MOUSE_BAR_CLICK (KEY_MAX + 50)` (`Panel.h:93`) — the
/// synthetic sentinel key returned by `IncSet_synthesizeEvent` for a mouse
/// click in the function-bar input field (with the click X stashed in
/// [`Panel::lastMouseBarClickX`]). Owned by the Panel module; consumed by
/// `IncSet` (`incset.rs`).
pub const KEY_MOUSE_BAR_CLICK: i32 = KEY_MAX + 50;

/// An item in a [`Panel`]'s list. htop's `Vector* items` carries a whole-list
/// `owner` flag deciding whether it frees its elements; here that choice is
/// per item. An owning panel stores [`Owned`](PanelItem::Owned) boxes
/// (dropped with the panel); a panel that only *displays* rows another
/// structure owns — e.g. the process [`Table`](crate::ported::table::Table) —
/// stores [`Borrowed`](PanelItem::Borrowed) raw pointers into that owner
/// (never dropped here). The faithful analog of a non-owning C `Panel`.
pub enum PanelItem {
    /// The panel owns this object and drops it.
    Owned(Box<dyn Object>),
    /// The panel only references an object owned elsewhere (a `Table` row);
    /// `*mut` because htop mutates rows through the panel (expand/collapse).
    Borrowed(*mut dyn Object),
}

impl PanelItem {
    /// The item as a shared `&dyn Object`, regardless of ownership.
    #[inline]
    pub fn object(&self) -> &dyn Object {
        match self {
            PanelItem::Owned(b) => b.as_ref(),
            // SAFETY: a Borrowed item points at an object owned elsewhere
            // (a Table row) that outlives its display in the panel.
            PanelItem::Borrowed(p) => unsafe { &**p },
        }
    }

    /// The item as a unique `&mut dyn Object`, regardless of ownership.
    #[inline]
    pub fn object_mut(&mut self) -> &mut dyn Object {
        match self {
            PanelItem::Owned(b) => b.as_mut(),
            // SAFETY: as `object`; the row is not aliased mutably elsewhere
            // while the panel edits it (htop's single-threaded row graph).
            PanelItem::Borrowed(p) => unsafe { &mut **p },
        }
    }
}

/// Port of htop's `struct Panel_` (`Panel.h:64`). See the module docs for
/// the field mapping. `selectionColorId` is a [`ColorElements`] (C's
/// `ColorElements selectionColorId`), so [`Panel_draw`] can index the color
/// tables directly; `items` is the `Vec` analog of the C `Vector* items`,
/// each a [`PanelItem`] (owned or borrowed) — the C `owner` flag made per-item.
pub struct Panel {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub cursorX: i32,
    pub cursorY: i32,
    pub items: Vec<PanelItem>,
    pub selected: i32,
    pub oldSelected: i32,
    pub prevSelected: i32,
    pub selectedLen: usize,
    /// Port of `void* eventHandlerState` (`Panel.h:73`). C uses it as a
    /// lazily `xCalloc(100, 1)`'d NUL-terminated scratch buffer for the
    /// incremental type-to-search in [`Panel_selectByTyping`]. Modeled as an
    /// `Option<Vec<u8>>`: `None` == the C `NULL` (not yet allocated), `Some`
    /// == the 100-byte zeroed buffer. Held as bytes (not `String`) because
    /// the C code indexes it, NUL-terminates it, and `strncasecmp`s against
    /// it byte-for-byte.
    pub eventHandlerState: Option<Vec<u8>>,
    pub scrollV: i32,
    pub scrollH: i32,
    pub needsRedraw: bool,
    pub cursorOn: bool,
    pub wasFocus: bool,
    /// Port of `bool allowExcessScrollV` (`Panel.h:79`). When true, `scrollV`
    /// outside `[0, size-h]` is permitted so [`Panel_draw`] can render blank
    /// lines above/below the list (used by stable tree-view hard mode, a
    /// subclass). `false` for a base panel, so its scroll is always clamped.
    pub allowExcessScrollV: bool,
    pub lastMouseBarClickX: i32,
    pub currentBar: Option<FunctionBar>,
    pub defaultBar: Option<FunctionBar>,
    pub header: RichString,
    pub selectionColorId: ColorElements,
    /// htoprs extension (no C analog): terminal lines consumed per list item.
    /// `1` (the default) is the faithful htop 1-item-1-line model; the process
    /// panel sets `2` for the inline double-height sparkline mode, where line 1
    /// is the normal row and line 2 is a full-width per-PID CPU sparkline. All
    /// screen-Y projection, page-step, and visible-capacity math in
    /// [`Panel_draw`]/[`Panel_onKey`] scales by this; at `1` every formula is
    /// byte-identical to the port so non-process panels are unaffected.
    pub rowHeight: i32,
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
            eventHandlerState: None,
            scrollV: 0,
            scrollH: 0,
            needsRedraw: true,
            cursorOn: false,
            wasFocus: false,
            allowExcessScrollV: false,
            lastMouseBarClickX: 0,
            currentBar: None,
            defaultBar: None,
            header: RichString::new(),
            selectionColorId: ColorElements::PANEL_SELECTION_FOCUS,
            rowHeight: 1,
        }
    }
}

/// Port of the `PanelClass` vtable (`Panel.h:36`). C dispatches
/// `Panel_eventHandler`/`Panel_drawFunctionBar`/`Panel_printHeader` through
/// `As_Panel(this)->klass`; the faithful Rust analog is a trait every panel
/// subclass implements, so a `Box<dyn PanelClass>` (the `ScreenManager`'s
/// `Panel*` element) dispatches to the concrete subclass. Each subclass
/// embeds `super_: Panel` and returns it from [`PanelClass::as_panel`] /
/// [`PanelClass::as_panel_mut`] (the C `(Panel*)this` upcast).
///
/// The three behavioural slots default to the C base-class behaviour: the
/// base `Panel_class` sets `eventHandler`/`drawFunctionBar`/`printHeader` to
/// `NULL`, and C guards every call with `if (Panel_..Fn(this))`, so a NULL
/// slot is a no-op. The default `PanelClass::eventHandler` returns
/// [`HandlerResult::IGNORED`] (the base panel handles no keys); the default
/// `draw_function_bar`/`print_header` do nothing (NULL slot skipped).
pub trait PanelClass {
    /// The C `(Panel*)this` upcast — the embedded base panel.
    fn as_panel(&self) -> &Panel;
    /// Mutable form of [`PanelClass::as_panel`].
    fn as_panel_mut(&mut self) -> &mut Panel;
    /// C `PanelClass.eventHandler` slot (`Panel_EventHandler`). Default =
    /// base `Panel_class` NULL slot → [`HandlerResult::IGNORED`].
    fn event_handler(&mut self, _ev: i32) -> HandlerResult {
        HandlerResult::IGNORED
    }
    /// htoprs extension hook: whether this panel is currently capturing raw
    /// text input (e.g. the `MainPanel`'s incremental search/filter is active).
    /// While true, `ScreenManager_run` must route keys straight to
    /// [`PanelClass::event_handler`] and NOT let the monitoring/theme overlays
    /// consume their hotkeys — otherwise typing `h`, `f`, etc. into the search
    /// box would fire global actions instead of entering the query. Default
    /// `false` (no text field); `MainPanel` overrides it.
    fn is_text_input_active(&self) -> bool {
        false
    }
    /// C `PanelClass.drawFunctionBar` slot. Default = NULL slot (no-op); the
    /// caller draws `panel->currentBar` itself.
    fn draw_function_bar(&mut self, _hide_function_bar: bool) {}
    /// C `PanelClass.printHeader` slot. Default = NULL slot (no-op).
    fn print_header(&mut self) {}
}

/// The base `Panel_class` (`Panel.c` `const PanelClass Panel_class`): a plain
/// panel with no behavioural overrides — every slot NULL, so the trait
/// defaults apply. Lets a bare [`Panel`] be stored as a `Box<dyn PanelClass>`.
impl PanelClass for Panel {
    fn as_panel(&self) -> &Panel {
        self
    }
    fn as_panel_mut(&mut self) -> &mut Panel {
        self
    }
    /// C `Panel_class.eventHandler = Panel_selectByTyping` (`Panel.c:33`): the
    /// base panel's event handler is incremental type-to-select. It returns
    /// `BREAK_LOOP` on Enter, which is what lets a modal picker built on a
    /// plain `Panel` (the sort-by-column / signal / affinity lists that
    /// [`crate::ported::action::Action_pickFromVector`] runs) exit and hand
    /// back the selected row. Overriding it here — rather than inheriting the
    /// trait's `IGNORED` NULL-slot default — mirrors the C class table, where
    /// the base `Panel_class` sets this slot explicitly.
    fn event_handler(&mut self, ev: i32) -> HandlerResult {
        Panel_selectByTyping(self, ev)
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

/// Port of `void Panel_delete(Object* cast)` from `Panel.c:43`:
/// `Panel_done(this); free(this);`. Taking `this` by value reproduces
/// `free(this)`; [`Panel_done`] releases the owned fields (mirroring the C
/// call graph). No `PanelClass` vtable dispatch is modeled.
pub fn Panel_delete(this: Panel) {
    Panel_done(this);
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
    this.eventHandlerState = None; // C: this->eventHandlerState = NULL (Panel.c:56)
    this.needsRedraw = true;
    this.cursorOn = false;
    this.wasFocus = false;
    this.allowExcessScrollV = false; // C: Panel.c:67
    this.lastMouseBarClickX = 0;
    this.header = RichString::new();
    this.defaultBar = fuBar.clone();
    this.currentBar = fuBar;
    this.selectionColorId = ColorElements::PANEL_SELECTION_FOCUS;
}

/// Port of the `Panel_setDefaultBar` macro from `Panel.h:87`:
/// `do { (this_)->currentBar = (this_)->defaultBar; } while (0)`.
///
/// C aliases the two `FunctionBar*` pointers to the one shared bar; the
/// `Vec`-model owns each bar via `Option<FunctionBar>`, so `currentBar`
/// takes a clone of `defaultBar` — reproducing the observable draw exactly
/// as [`Panel_init`] already does when it seeds both from `fuBar`.
pub fn Panel_setDefaultBar(this: &mut Panel) {
    this.currentBar = this.defaultBar.clone();
}

/// Port of `void Panel_done(Panel* this)` from `Panel.c:74`:
/// `free(eventHandlerState); Vector_delete(items);
/// FunctionBar_delete(defaultBar); RichString_delete(&header);`.
///
/// Taking `this` by value consumes the panel. The owned `defaultBar` is
/// handed to [`FunctionBar_delete`] (mirroring the C call graph); the
/// `eventHandlerState` scratch buffer, the `items` list (each `Owned`
/// [`PanelItem`] — the C `Vector_delete`, since a non-owning panel keeps
/// `Borrowed` raw pointers it must not free), the `header` `RichString`,
/// and the cloned `currentBar` alias all drop with the remaining fields.
pub fn Panel_done(this: Panel) {
    let Panel {
        defaultBar,
        items,
        eventHandlerState,
        header,
        ..
    } = this;
    if let Some(bar) = defaultBar {
        FunctionBar_delete(bar);
    }
    let _ = items;
    let _ = eventHandlerState;
    let _ = header;
}

/// Port of `Panel.c:83`.
///
/// ```c
/// this->cursorY = this->y + this->selected - this->scrollV + 1;
/// this->cursorX = this->x + (int)this->selectedLen - this->scrollH;
/// ```
///
/// `(int)this->selectedLen` truncates the `size_t` to `int`; `as i32`
/// reproduces that, and the surrounding arithmetic stays in `i32`.
pub fn Panel_setCursorToSelection(this: &mut Panel) {
    // htoprs: sum the physical heights of the items above the selection to find
    // its top line. At the default `rowHeight == 1` this reduces to the C form
    // `selected - scrollV`; in graph mode rows are variable-height, so the span
    // is accumulated per item.
    let offset = if this.rowHeight.max(1) <= 1 {
        this.selected - this.scrollV
    } else {
        this.line_span(this.scrollV, this.selected)
    };
    this.cursorY = this.y + offset + 1;
    this.cursorX = this.x + (this.selectedLen as i32) - this.scrollH;
}

/// Port of `Panel.c:88`. Stores `colorId` into `selectionColorId`.
pub fn Panel_setSelectionColor(this: &mut Panel, colorId: ColorElements) {
    this.selectionColorId = colorId;
}

/// Port of `inline void Panel_setHeader(Panel* this, const char* header)`
/// from `Panel.c:92`. Overwrites the header via `RichString_writeWide`
/// with `CRT_colors[PANEL_HEADER_FOCUS]` and marks the panel dirty.
pub fn Panel_setHeader(this: &mut Panel, header: &str) {
    let attr = ColorElements::PANEL_HEADER_FOCUS.packed(ColorScheme::active());
    RichString_writeWide(&mut this.header, attr, header.as_bytes());
    this.needsRedraw = true;
}

/// Port of `Panel.c:97`. Sets the panel's top-left corner, marks it dirty.
pub fn Panel_move(this: &mut Panel, x: i32, y: i32) {
    this.x = x;
    this.y = y;
    this.needsRedraw = true;
}

/// Port of `Panel.c:105`. Sets the panel's width/height, marks it dirty.
pub fn Panel_resize(this: &mut Panel, w: i32, h: i32) {
    this.w = w;
    this.h = h;
    this.needsRedraw = true;
}

/// Port of `Panel.c:113`. Empties the item list and resets selection/
/// scroll state (C `Vector_prune` clears the vector).
pub fn Panel_prune(this: &mut Panel) {
    this.items.clear();
    this.prevSelected = -1;
    this.scrollV = 0;
    this.selected = 0;
    this.oldSelected = 0;
    this.needsRedraw = true;
    this.allowExcessScrollV = false; // C: Panel.c:122
}

/// Port of `Panel.c:125`. Appends `o` to the item list (owned).
pub fn Panel_add(this: &mut Panel, o: Box<dyn Object>) {
    this.items.push(PanelItem::Owned(o));
    this.prevSelected = -1;
    this.needsRedraw = true;
}

/// Port of `Panel.c:133`. Inserts `o` at index `i` (owned).
pub fn Panel_insert(this: &mut Panel, i: i32, o: Box<dyn Object>) {
    this.items.insert(i as usize, PanelItem::Owned(o));
    this.prevSelected = -1;
    this.needsRedraw = true;
}

/// Port of `Panel.c:141`. Replaces the item at index `i` with `o`
/// (C `Vector_set`), taking ownership.
pub fn Panel_set(this: &mut Panel, i: i32, o: Box<dyn Object>) {
    this.items[i as usize] = PanelItem::Owned(o);
}

/// Port of `Panel.c:147`. Returns the item at index `i` (C `Vector_get`,
/// which asserts `i` in range).
pub fn Panel_get(this: &Panel, i: i32) -> &dyn Object {
    this.items[i as usize].object()
}

/// Port of `Panel.c:153`. Removes and returns the (owned) item at index
/// `i`, decrementing `selected` when it fell off the (now shorter) end.
/// Only valid on an owning panel — a borrowing panel prunes and re-refs
/// rather than removing individual items.
pub fn Panel_remove(this: &mut Panel, i: i32) -> Box<dyn Object> {
    this.needsRedraw = true;
    let removed = match this.items.remove(i as usize) {
        PanelItem::Owned(b) => b,
        PanelItem::Borrowed(_) => panic!("Panel_remove on a borrowed item"),
    };
    this.prevSelected = -1;
    if this.selected > 0 && this.selected >= this.items.len() as i32 {
        this.selected -= 1;
    }
    removed
}

/// Port of `Panel.c:166`. Returns the selected item, or `None` when the
/// list is empty.
pub fn Panel_getSelected(this: &Panel) -> Option<&dyn Object> {
    if !this.items.is_empty() {
        Some(this.items[this.selected as usize].object())
    } else {
        None
    }
}

/// Port of `Panel.c:176`. Swaps the selected item with the one above it
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

/// Port of `Panel.c:186`. Swaps the selected item with the one below it
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

/// Port of `Panel.c:196`. Returns the current selection index.
pub fn Panel_getSelectedIndex(this: &Panel) -> i32 {
    this.selected
}

/// Port of `Panel.c:202`. Returns the item count (C `Vector_size`).
pub fn Panel_size(this: &Panel) -> i32 {
    this.items.len() as i32
}

/// Port of `Panel.c:208`. Clamps `selected` into `[0, size)` and stores it.
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

/// Port of `void Panel_splice(Panel* this, Vector* from)` from `Panel.c:224`:
/// `Vector_splice(this->items, from); this->prevSelected = -1;
/// this->needsRedraw = true;`.
///
/// `Vector_splice` (`Vector.c:367`) asserts `!this->owner` and appends the
/// *pointers* of `from`'s elements into `this->items` without owning them —
/// both `Vector`s then alias the same `Object*`. The [`PanelItem`] model
/// expresses exactly that non-owning alias with [`PanelItem::Borrowed`]: each
/// element of `from` (owned by whoever owns the `Vector`, e.g. `AffinityPanel`'s
/// `cpuids`) is referenced by a raw `*mut dyn Object` the panel never frees.
/// So the faithful port pushes a `Borrowed` pointer to every element of `from`,
/// reproducing the appended aliasing while keeping single ownership with the
/// source. The `assert(this != NULL)`/`assert(from != NULL)` preconditions are
/// carried by the `&mut Panel`/`&Vector` reference types.
///
/// # Safety
///
/// The elements of `from` must outlive their display in this panel (the same
/// contract every [`PanelItem::Borrowed`] carries — the real owner frees them).
pub fn Panel_splice(this: &mut Panel, from: &Vector) {
    // C: Vector_splice(this->items, from) — append `from`'s element pointers.
    let n = Vector_size(from);
    for i in 0..n {
        // The element is owned by `from`; the panel only references it.
        let obj = Vector_get(from, i as usize) as *const dyn Object as *mut dyn Object;
        this.items.push(PanelItem::Borrowed(obj));
    }
    this.prevSelected = -1;
    this.needsRedraw = true;
}

/// Port of `void Panel_draw(Panel* this, bool force_redraw, bool focus,
/// bool highlightSelected, bool hideFunctionBar)` from `Panel.c:233`.
///
/// Behavioral crossterm port. Reproduces htop's header line, the scroll
/// clamp ("scroll follows selection", factored into
/// `Panel::ensure_scroll`), the per-row `Object_display` → `RichString`
/// → blit with selection highlight, the trailing blank fill, and the
/// focused function-bar draw — emitting through the `Ncurses` shim.
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

    // htoprs extension: draw into the frame buffer (see `extensions::frame`) so
    // the whole repaint is presented atomically; falls back to stdout when no
    // frame is open (e.g. a direct draw outside the run loop).
    let mut out = crate::extensions::frame::frame_out();

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
        Ncurses::attrset(
            &mut out,
            ColorElements::RESET_COLOR.packed(ColorScheme::active()),
        );
        y += 1;
        h -= 1;
    }

    // htoprs: `rh` is the per-item line-height ceiling (1 = the faithful htop
    // model; > 1 = graph mode, where each process is `1 + graph_lines(cpu)`
    // tall). `ensure_scroll_var` keeps the selection visible under either model.
    let rh = this.rowHeight.max(1);

    this.ensure_scroll_var(size, h);

    // topPad: empty screen lines above the first row (non-zero only when
    // allowExcessScrollV left scrollV negative). C: Panel.c:293-296.
    let top_pad = if this.scrollV < 0 { -this.scrollV } else { 0 };
    let first = this.scrollV + top_pad;
    // In graph mode the item count that fits is variable, so draw until the
    // lines run out (`line < h` below); uniform mode keeps the ported bound.
    let up_to = if rh <= 1 {
        (first + h - top_pad).min(size)
    } else {
        size
    };

    let selection_color = if focus {
        this.selectionColorId.packed(ColorScheme::active())
    } else {
        ColorElements::PANEL_SELECTION_UNFOCUS.packed(ColorScheme::active())
    };

    let mut item = RichString::new();
    // htoprs: graph mode (`rh > 1`) uses variable per-row heights, so the
    // two-row incremental cursor-move path can't project rows by a fixed
    // stride — force the full redraw there.
    if this.needsRedraw || force_redraw || rh > 1 {
        let mut line = 0i32;
        // Blank pad lines above the first row (C: Panel.c:305-308). `top_pad`
        // counts items, so `* rh` gives the screen lines to blank.
        while line < top_pad * rh {
            Ncurses::mvhline(&mut out, y + line, x, ' ', w);
            line += 1;
        }
        let mut i = first;
        while line < h && i < up_to {
            // htoprs: physical lines this item occupies (1 in single-height
            // mode; CPU-scaled `1 + graph_lines` for a process in graph mode).
            let ih = this.item_height(i as usize);
            let mut highlight_attr = 0i32;
            // htoprs extension: the pid of a process row, for the alert recolor
            // and sparkline hooks below. `None` for non-process rows (setup
            // screens, meter lists) — those get no decoration.
            let row_pid: Option<u32>;
            {
                let item_obj: &dyn Object = this.items[i as usize].object();
                row_pid = item_obj
                    .as_process()
                    .map(|p| Process_getPid(p).max(0) as u32);
                let sz = RichString_size(&item);
                RichString_rewind(&mut item, sz);
                item.highlightAttr = 0;
                item_obj.display(&mut item);
            }
            let item_len = RichString_sizeVal(&item);
            let amt = (item_len - scrollH).min(w);
            // htoprs extension: a firing threshold-alert row gets a distinct hot
            // color (bold white-on-red). No-op otherwise. Applied BEFORE the
            // selection so the cursor highlight below wins on the selected row —
            // the cursor must stay visible even over a hot row.
            if let Some(pid) = row_pid {
                if let Some(attr) = crate::extensions::panels::alert_attr(pid) {
                    item.highlightAttr = attr;
                    highlight_attr = attr;
                }
            }
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
            // htoprs extension: overdraw the per-PID CPU sparkline column at the
            // row's right edge when the `v` toggle is on the side-column state
            // (no-op in Off / double-height states).
            if let Some(pid) = row_pid {
                crate::extensions::panels::draw_spark_col(&mut out, y + line, x, w, pid);
            }
            // htoprs extension: in graph mode a process row owns `ih - 1` lines
            // below it carrying a CPU braille graph whose height scales with the
            // process's CPU. Fill each line with the row's highlight background
            // first (so a selected/hot row's graph matches), then paint glyphs.
            let graph_h = (ih - 1).min(h - line - 1).max(0);
            if graph_h > 0 {
                if highlight_attr != 0 {
                    Ncurses::attrset(&mut out, highlight_attr);
                }
                for g in 1..=graph_h {
                    Ncurses::mvhline(&mut out, y + line + g, x, ' ', w);
                }
                if let Some(pid) = row_pid {
                    crate::extensions::panels::draw_spark_row(
                        &mut out,
                        y + line + 1,
                        x,
                        w,
                        graph_h,
                        pid,
                    );
                }
            }
            if highlight_attr != 0 {
                Ncurses::attrset(
                    &mut out,
                    ColorElements::RESET_COLOR.packed(ColorScheme::active()),
                );
            }
            line += ih;
            i += 1;
        }
        while line < h {
            Ncurses::mvhline(&mut out, y + line, x, ' ', w);
            line += 1;
        }
    } else {
        // C positions the two touched rows against scrollV directly
        // (Panel.c:341/343/353/356), not the topPad-adjusted `first`.
        let scroll_v = this.scrollV;
        let old_selected = this.oldSelected;
        let old_pid: Option<u32>;
        {
            let old_obj: &dyn Object = this.items[old_selected as usize].object();
            old_pid = old_obj
                .as_process()
                .map(|p| Process_getPid(p).max(0) as u32);
            let sz = RichString_size(&item);
            RichString_rewind(&mut item, sz);
            item.highlightAttr = 0;
            old_obj.display(&mut item);
        }
        let old_len = RichString_sizeVal(&item);
        // htoprs extension: if the row the cursor is leaving is still a firing
        // (hot) row, repaint it in its hot color rather than clearing it to
        // normal — otherwise moving off a hot row would drop its highlight until
        // the next full redraw.
        let old_hot = old_pid.and_then(crate::extensions::panels::alert_attr);
        if let Some(attr) = old_hot {
            Ncurses::attrset(&mut out, attr);
            RichString_setAttr(&mut item, attr);
        }
        // htoprs: `* rh` projects the item index to its top physical line.
        let old_y = y + (old_selected - scroll_v) * rh;
        Ncurses::mvhline(&mut out, old_y, x, ' ', w);
        if scrollH < old_len {
            Panel::print_offset(
                &mut out,
                old_y,
                x,
                &item,
                scrollH,
                (old_len - scrollH).min(w),
            );
        }
        // NB: this incremental cursor-move path runs only in single-height
        // mode (`rh == 1`); graph mode forces the full redraw above, so no
        // second graph line needs repainting here.
        if old_hot.is_some() {
            Ncurses::attrset(
                &mut out,
                ColorElements::RESET_COLOR.packed(ColorScheme::active()),
            );
        }

        let selected = this.selected;
        {
            let new_obj: &dyn Object = this.items[selected as usize].object();
            let sz = RichString_size(&item);
            RichString_rewind(&mut item, sz);
            item.highlightAttr = 0;
            new_obj.display(&mut item);
        }
        let new_len = RichString_sizeVal(&item);
        this.selectedLen = new_len as usize;
        Ncurses::attrset(&mut out, selection_color);
        // htoprs: `* rh` projects the item index to its top physical line.
        let new_y = y + (selected - scroll_v) * rh;
        Ncurses::mvhline(&mut out, new_y, x, ' ', w);
        RichString_setAttr(&mut item, selection_color);
        if scrollH < new_len {
            Panel::print_offset(
                &mut out,
                new_y,
                x,
                &item,
                scrollH,
                (new_len - scrollH).min(w),
            );
        }
        Ncurses::attrset(
            &mut out,
            ColorElements::RESET_COLOR.packed(ColorScheme::active()),
        );
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
/// `Panel.c:374`: `1` when the header is non-empty, else `0`.
pub fn Panel_headerHeight(this: &Panel) -> i32 {
    if RichString_sizeVal(&this.header) > 0 {
        1
    } else {
        0
    }
}

/// Port of `bool Panel_onKey(Panel* this, int key)` from `Panel.c:380`.
///
/// Navigation/scroll key handling. Ported faithfully against the item
/// count, `CRT_scrollHAmount`, and `CRT_scrollWheelVAmount` (`crt.rs`).
/// The `KEY_SR`/`KEY_SF` (single-line shift scroll) arms are ported against
/// the module-local `KEY_SR`/`KEY_SF` constants (`crt.rs` does not export
/// those ncurses codes); `availableHeight`/`maxScroll` (`Panel.c:384-385`)
/// serve those two arms. Returns `true` when `key` was handled, `false` for
/// the default (unhandled) case.
pub fn Panel_onKey(this: &mut Panel, key: i32) -> bool {
    let size = this.items.len() as i32;
    // htoprs: `available_height` is a count of *visible items*, so in
    // double-height mode (`rowHeight > 1`) the visible line span is divided by
    // the per-item height. Identity at the default `rowHeight == 1`.
    let rh = this.rowHeight.max(1);
    let available_height = ((this.h - Panel_headerHeight(this)) / rh).max(1); // C: Panel.c:384
    let max_scroll = (size - available_height).max(0); // C: Panel.c:385
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
            // htoprs: page by however many items fill one screen of lines,
            // walking up from the selection — variable-height-aware.
            let lines = this.h - Panel_headerHeight(this);
            let step = this.items_per_page(this.selected, lines, -1);
            this.panel_scroll(-step, size);
        }
        KEY_NPAGE => {
            let lines = this.h - Panel_headerHeight(this);
            let step = this.items_per_page(this.selected, lines, 1);
            this.panel_scroll(step, size);
        }
        KEY_WHEELUP => {
            let amt = crt::CRT_scrollWheelVAmount.load(Ordering::Relaxed);
            this.panel_scroll(-amt, size);
        }
        KEY_WHEELDOWN => {
            let amt = crt::CRT_scrollWheelVAmount.load(Ordering::Relaxed);
            this.panel_scroll(amt, size);
        }
        KEY_SR => {
            if this.scrollV > 0 {
                // keep selection within the now-visible area
                if this.selected < this.scrollV + available_height {
                    this.scrollV -= 1;
                    this.needsRedraw = true;
                }
            }
        }
        KEY_SF => {
            if this.scrollV < max_scroll {
                // keep selection within the now-visible area
                if this.selected >= this.scrollV {
                    this.scrollV += 1;
                    this.needsRedraw = true;
                }
            }
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

/// Port of `HandlerResult Panel_selectByTyping(Panel* this, int ch)` from
/// `Panel.c:507` — the base `Panel_class.eventHandler`, an incremental
/// type-to-search over the list's `ListItem` values.
///
/// Faithful port of the C control flow:
/// - `'#'` is ignored outright.
/// - The `eventHandlerState` scratch buffer is lazily allocated as 100
///   zeroed bytes (`xCalloc(100, sizeof(char))`) and kept NUL-terminated.
/// - A graphic char (`0 < ch < 255 && isgraph`, here `is_ascii_graphic`,
///   which matches `isgraph` in the C locale: `0x21..=0x7e`) is appended to
///   the buffer; on an empty buffer `'/'` becomes the `\001` search marker
///   and `'q'` breaks the loop; a lone `\001` marker is dropped when the
///   next char arrives.
/// - It then scans items for the first whose value (after skipping leading
///   spaces) case-insensitively starts with the buffer, via the same
///   semantics as the C `strncasecmp(cur, buffer, len) == 0` (ASCII
///   case-fold, per the C locale). No match ⇒ retry once treating the last
///   char as the start of a new word.
/// - A non-graphic, non-`ERR` char clears the buffer; `13` (Enter) breaks
///   the loop; everything else is ignored.
///
/// `Panel_get(this, i)` yields a `&dyn Object`; the C cast
/// `((ListItem*)…)->value` is reproduced by the `&dyn Any` downcast idiom
/// used across the crate (the hard C cast panics here on a wrong class,
/// where C would invoke UB). `eventHandlerState` is `take`n into a local so
/// `this` stays free for `Panel_get`/`Panel_setSelected`, then restored on
/// every exit path.
pub fn Panel_selectByTyping(this: &mut Panel, ch: i32) -> HandlerResult {
    let size = Panel_size(this);

    if ch == '#' as i32 {
        return HandlerResult::IGNORED;
    }

    if this.eventHandlerState.is_none() {
        this.eventHandlerState = Some(vec![0u8; 100]); // xCalloc(100, sizeof(char))
    }
    // Take the buffer out so `this` can be borrowed for Panel_get /
    // Panel_setSelected; it is restored before returning.
    let mut buffer = this.eventHandlerState.take().unwrap();

    let mut ch = ch;
    // strlen(buffer): index of the first NUL (the C string length).
    let strlen = |b: &[u8]| b.iter().position(|&c| c == 0).unwrap_or(b.len());

    let result = 'done: {
        if 0 < ch && ch < 255 && (ch as u8).is_ascii_graphic() {
            let mut len = strlen(&buffer);
            if len == 0 {
                if ch == '/' as i32 {
                    ch = 0x01; // '\001'
                } else if ch == 'q' as i32 {
                    break 'done HandlerResult::BREAK_LOOP;
                }
            } else if len == 1 && buffer[0] == 0x01 {
                len -= 1;
            }

            if len < 99 {
                buffer[len] = ch as u8;
                buffer[len + 1] = b'\0';
            }

            for _try in 0..2 {
                len = strlen(&buffer);
                for i in 0..size {
                    // C: const char* cur = ((ListItem*) Panel_get(this, i))->value;
                    //    while (*cur == ' ') cur++;
                    //    strncasecmp(cur, buffer, len) == 0
                    let matched = {
                        let obj = Panel_get(this, i);
                        let any: &dyn core::any::Any = obj;
                        let li = any
                            .downcast_ref::<ListItem>()
                            .expect("Panel_selectByTyping: panel item is not a ListItem");
                        let val = li.value.as_bytes();
                        let start = val.iter().position(|&c| c != b' ').unwrap_or(val.len());
                        let cur = &val[start..];
                        // strncasecmp over `len` bytes: buffer[..len] has no
                        // interior NUL, so equality needs `cur` to be at least
                        // `len` bytes and case-fold-equal on that prefix.
                        cur.len() >= len && cur[..len].eq_ignore_ascii_case(&buffer[..len])
                    };
                    if matched {
                        Panel_setSelected(this, i);
                        break 'done HandlerResult::HANDLED;
                    }
                }

                // if current word did not match, retry considering the
                // character the start of a new word.
                buffer[0] = ch as u8;
                buffer[1] = b'\0';
            }

            break 'done HandlerResult::HANDLED;
        } else if ch != ERR {
            buffer[0] = b'\0';
        }

        if ch == 13 {
            break 'done HandlerResult::BREAK_LOOP;
        }

        HandlerResult::IGNORED
    };

    this.eventHandlerState = Some(buffer);
    result
}

/// Port of `int Panel_getCh(Panel* this)` from `Panel.c:565`.
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
    /// Pure scroll clamp from `Panel_draw` (`Panel.c:272-291`): keeps the
    /// scroll area and the selection on screen, mutating `scrollV`/
    /// `needsRedraw`. Factored out so the "scroll follows selection"
    /// behavior is unit-testable without a TTY. `h` is the drawable row
    /// count after the header adjustment. The `scrollV` clamp is skipped
    /// when `allowExcessScrollV` is set (C guards it with
    /// `if (!this->allowExcessScrollV)`); the selection-on-screen check is
    /// always applied, matching the C control flow.
    fn ensure_scroll(&mut self, size: i32, h: i32) {
        if !self.allowExcessScrollV {
            if self.scrollV < 0 {
                self.scrollV = 0;
                self.needsRedraw = true;
            } else if self.scrollV > size - h {
                self.scrollV = (size - h).max(0);
                self.needsRedraw = true;
            }
        }
        if self.selected < self.scrollV {
            self.scrollV = self.selected;
            self.needsRedraw = true;
        } else if self.selected >= self.scrollV + h {
            self.scrollV = self.selected - h + 1;
            self.needsRedraw = true;
        }
    }

    /// htoprs: physical terminal lines item `i` occupies. `1` in the default
    /// single-height model. When the panel is in graph mode (`rowHeight > 1`) a
    /// process row is `1 + graph_lines(pid)` tall — CPU-scaled, so busier
    /// processes are taller — while any non-process item stays uniform at
    /// `rowHeight`. This is the single source every screen-Y projection uses.
    fn item_height(&self, i: usize) -> i32 {
        let rh = self.rowHeight.max(1);
        if rh <= 1 || i >= self.items.len() {
            return rh.max(1);
        }
        let obj = self.items[i].object();
        match obj.as_process() {
            Some(p) => 1 + crate::extensions::panels::graph_lines(Process_getPid(p).max(0) as u32),
            None => rh,
        }
    }

    /// Cumulative physical lines from item `lo` up to (excluding) `hi`, summing
    /// per-item heights. `lo > hi` yields 0.
    fn line_span(&self, lo: i32, hi: i32) -> i32 {
        (lo.max(0)..hi.max(0))
            .map(|k| self.item_height(k as usize))
            .sum()
    }

    /// Items that fit in `lines` starting at `from` and walking `dir` (+1 down,
    /// -1 up), by accumulating [`item_height`]. At least 1. Used for page steps
    /// when rows are variable-height.
    ///
    /// [`item_height`]: Panel::item_height
    fn items_per_page(&self, from: i32, lines: i32, dir: i32) -> i32 {
        let size = self.items.len() as i32;
        let mut used = 0;
        let mut cnt = 0;
        let mut k = from;
        while k >= 0 && k < size {
            used += self.item_height(k as usize);
            if used > lines {
                break;
            }
            cnt += 1;
            k += dir;
        }
        cnt.max(1)
    }

    /// Variable-height-aware scroll clamp. For the default single-height model
    /// (`rowHeight <= 1`) this defers to [`ensure_scroll`] for byte-identical
    /// ported behavior. In graph mode it keeps the selected row fully visible by
    /// summing per-item heights instead of assuming a fixed stride. `h` is the
    /// drawable line count after the header adjustment.
    ///
    /// [`ensure_scroll`]: Panel::ensure_scroll
    fn ensure_scroll_var(&mut self, size: i32, h: i32) {
        if self.rowHeight.max(1) <= 1 {
            self.ensure_scroll(size, h);
            return;
        }
        if size == 0 {
            self.scrollV = 0;
            return;
        }
        self.selected = self.selected.clamp(0, size - 1);
        self.scrollV = self.scrollV.clamp(0, size - 1);
        // Selected above the viewport top → scroll up to it.
        if self.selected < self.scrollV {
            self.scrollV = self.selected;
            self.needsRedraw = true;
        }
        // Selected below the viewport bottom → scroll down one item at a time
        // until the run [scrollV..=selected] fits in `h` lines.
        while self.scrollV < self.selected && self.line_span(self.scrollV, self.selected + 1) > h {
            self.scrollV += 1;
            self.needsRedraw = true;
        }
    }

    /// The `PANEL_SCROLL(amount)` macro body (`Panel.c:387`): shift the
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
    fn print_offset<W: Write>(out: &mut W, y: i32, x: i32, item: &RichString, off: i32, n: i32) {
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
            p.items.push(PanelItem::Owned(li(&format!("item{i}"))));
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
    fn set_cursor_to_selection_scales_by_row_height() {
        // htoprs double-height mode: the item index projects to its top physical
        // line via `(selected - scrollV) * rowHeight`, so row 7 with 2 lines each
        // and scrollV=2 lands twice as far down as the single-height case.
        let mut p = blank();
        fill(&mut p, 10); // non-process (ListItem) rows are uniform at rowHeight
        p.x = 3;
        p.y = 5;
        p.selected = 7;
        p.scrollV = 2;
        p.scrollH = 4;
        p.selectedLen = 20;
        p.rowHeight = 2;
        Panel_setCursorToSelection(&mut p);
        assert_eq!(p.cursorY, 16); // 5 + sum(heights[2..7]=5*2) + 1
        assert_eq!(p.cursorX, 19); // 3 + 20 - 4 (horizontal is unaffected)
    }

    #[test]
    fn on_key_page_step_halves_at_double_height() {
        // A page (NPAGE) advances the selection by the number of *visible items*,
        // which halves when each item is two lines tall.
        let mut single = blank();
        single.h = 20; // no header -> headerHeight 0
        fill(&mut single, 100);
        assert!(Panel_onKey(&mut single, KEY_NPAGE));
        assert_eq!(single.selected, 20); // full 20-line page at rowHeight 1

        let mut dbl = blank();
        dbl.h = 20;
        dbl.rowHeight = 2;
        fill(&mut dbl, 100);
        assert!(Panel_onKey(&mut dbl, KEY_NPAGE));
        assert_eq!(dbl.selected, 10); // 20 lines / 2 = 10 visible items
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
    fn onkey_wheel_down_up_scrolls_by_wheel_amount() {
        let mut p = blank();
        fill(&mut p, 40);
        p.h = 5; // no header -> headerHeight 0
        let amt = crt::CRT_scrollWheelVAmount.load(Ordering::Relaxed);
        assert!(Panel_onKey(&mut p, KEY_WHEELDOWN));
        // selected += amt; scrollV clamped to [0, 40-5-0]=35 -> amt
        assert_eq!(p.selected, amt);
        assert_eq!(p.scrollV, amt);
        assert!(Panel_onKey(&mut p, KEY_WHEELUP));
        assert_eq!(p.selected, 0);
        assert_eq!(p.scrollV, 0);
    }

    #[test]
    fn onkey_shift_up_scrolls_one_line_without_moving_selection() {
        let mut p = blank();
        fill(&mut p, 20);
        p.h = 5; // no header -> availableHeight 5
        p.scrollV = 5;
        p.selected = 5;
        assert!(Panel_onKey(&mut p, KEY_SR));
        // scrollV>0 and selected < scrollV+availableHeight -> scrollV--
        assert_eq!(p.scrollV, 4);
        assert_eq!(p.selected, 5);
        // at scrollV 0, KEY_SR is a no-op
        p.scrollV = 0;
        assert!(Panel_onKey(&mut p, KEY_SR));
        assert_eq!(p.scrollV, 0);
    }

    #[test]
    fn onkey_shift_down_scrolls_one_line_without_moving_selection() {
        let mut p = blank();
        fill(&mut p, 20);
        p.h = 5; // maxScroll = 20 - 5 = 15
        p.scrollV = 0;
        p.selected = 0;
        assert!(Panel_onKey(&mut p, KEY_SF));
        // scrollV<maxScroll and selected>=scrollV -> scrollV++
        assert_eq!(p.scrollV, 1);
        assert_eq!(p.selected, 0);
        // at scrollV == maxScroll, KEY_SF is a no-op
        p.scrollV = 15;
        assert!(Panel_onKey(&mut p, KEY_SF));
        assert_eq!(p.scrollV, 15);
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

    // ── selectByTyping (incremental type-to-search) ───────────────────

    fn with_items(values: &[&str]) -> Panel {
        let mut p = blank();
        for v in values {
            p.items.push(PanelItem::Owned(li(v)));
        }
        p
    }

    /// The NUL-terminated contents of the `eventHandlerState` scratch buffer.
    fn search_buf(p: &Panel) -> String {
        let b = p.eventHandlerState.as_ref().expect("buffer not allocated");
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        String::from_utf8(b[..end].to_vec()).unwrap()
    }

    #[test]
    fn typing_selects_first_matching_prefix() {
        let mut p = with_items(&["apple", "banana", "cherry"]);
        let r = Panel_selectByTyping(&mut p, 'b' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(p.selected, 1); // "banana"
        assert_eq!(search_buf(&p), "b");
    }

    #[test]
    fn typing_narrows_selection_as_chars_accumulate() {
        let mut p = with_items(&["bee", "banana", "bat"]);
        // 'b' -> first "b*" is "bee" (index 0)
        assert_eq!(
            Panel_selectByTyping(&mut p, 'b' as i32),
            HandlerResult::HANDLED
        );
        assert_eq!(p.selected, 0);
        // "ba" -> "banana" (index 1)
        assert_eq!(
            Panel_selectByTyping(&mut p, 'a' as i32),
            HandlerResult::HANDLED
        );
        assert_eq!(p.selected, 1);
        // "bat" -> "bat" (index 2)
        assert_eq!(
            Panel_selectByTyping(&mut p, 't' as i32),
            HandlerResult::HANDLED
        );
        assert_eq!(p.selected, 2);
        assert_eq!(search_buf(&p), "bat");
    }

    #[test]
    fn typing_is_case_insensitive() {
        let mut p = with_items(&["apple", "Banana", "cherry"]);
        // uppercase 'B' matches "Banana"; lowercase would too
        assert_eq!(
            Panel_selectByTyping(&mut p, 'B' as i32),
            HandlerResult::HANDLED
        );
        assert_eq!(p.selected, 1);

        let mut q = with_items(&["apple", "Banana", "cherry"]);
        assert_eq!(
            Panel_selectByTyping(&mut q, 'b' as i32),
            HandlerResult::HANDLED
        );
        assert_eq!(q.selected, 1);
    }

    #[test]
    fn no_match_keeps_selection_and_returns_handled() {
        let mut p = with_items(&["apple", "banana"]);
        p.selected = 1;
        let r = Panel_selectByTyping(&mut p, 'z' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(p.selected, 1); // unchanged, no item starts with 'z'
        assert_eq!(search_buf(&p), "z");
    }

    #[test]
    fn leading_spaces_are_skipped_before_matching() {
        let mut p = with_items(&["   indented", "other"]);
        let r = Panel_selectByTyping(&mut p, 'i' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(p.selected, 0); // matches "indented" after skipping spaces
    }

    #[test]
    fn retry_treats_last_char_as_start_of_new_word() {
        let mut p = with_items(&["apple", "xray"]);
        // 'z' -> no match; buffer "z"
        assert_eq!(
            Panel_selectByTyping(&mut p, 'z' as i32),
            HandlerResult::HANDLED
        );
        assert_eq!(p.selected, 0);
        // 'x' -> buffer "zx" (no match on try 0), retry with just "x" -> "xray"
        assert_eq!(
            Panel_selectByTyping(&mut p, 'x' as i32),
            HandlerResult::HANDLED
        );
        assert_eq!(p.selected, 1);
        // buffer was rewritten to the single retry char "x"
        assert_eq!(search_buf(&p), "x");
    }

    #[test]
    fn hash_is_ignored_and_leaves_buffer_unallocated() {
        let mut p = with_items(&["apple"]);
        let r = Panel_selectByTyping(&mut p, '#' as i32);
        assert_eq!(r, HandlerResult::IGNORED);
        assert!(p.eventHandlerState.is_none());
    }

    #[test]
    fn q_on_empty_buffer_breaks_loop() {
        let mut p = with_items(&["apple", "banana"]);
        let r = Panel_selectByTyping(&mut p, 'q' as i32);
        assert_eq!(r, HandlerResult::BREAK_LOOP);
        // buffer stays empty (the 'q' was consumed as the quit key)
        assert_eq!(search_buf(&p), "");
    }

    #[test]
    fn q_after_text_is_a_normal_search_char() {
        let mut p = with_items(&["apple", "aqua"]);
        assert_eq!(
            Panel_selectByTyping(&mut p, 'a' as i32),
            HandlerResult::HANDLED
        );
        // buffer now "a"; 'q' extends it to "aq" -> matches "aqua"
        let r = Panel_selectByTyping(&mut p, 'q' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(p.selected, 1);
        assert_eq!(search_buf(&p), "aq");
    }

    #[test]
    fn slash_marker_is_dropped_when_next_char_arrives() {
        let mut p = with_items(&["apple"]);
        // '/' on an empty buffer becomes the \001 search marker
        let r = Panel_selectByTyping(&mut p, '/' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(p.eventHandlerState.as_ref().unwrap()[0], 0x01);
        // next char drops the marker and searches for just that char
        let r = Panel_selectByTyping(&mut p, 'a' as i32);
        assert_eq!(r, HandlerResult::HANDLED);
        assert_eq!(p.selected, 0); // "apple"
        assert_eq!(search_buf(&p), "a");
    }

    #[test]
    fn nongraphic_char_clears_buffer_and_returns_ignored() {
        let mut p = with_items(&["apple", "banana"]);
        assert_eq!(
            Panel_selectByTyping(&mut p, 'b' as i32),
            HandlerResult::HANDLED
        );
        assert_eq!(search_buf(&p), "b");
        // ASCII backspace (0x08) is non-graphic, != ERR -> clears the buffer
        let r = Panel_selectByTyping(&mut p, 0x08);
        assert_eq!(r, HandlerResult::IGNORED);
        assert_eq!(search_buf(&p), "");
    }

    #[test]
    fn enter_clears_buffer_and_breaks_loop() {
        let mut p = with_items(&["apple"]);
        assert_eq!(
            Panel_selectByTyping(&mut p, 'a' as i32),
            HandlerResult::HANDLED
        );
        let r = Panel_selectByTyping(&mut p, 13);
        assert_eq!(r, HandlerResult::BREAK_LOOP);
        assert_eq!(search_buf(&p), ""); // buffer cleared by the non-graphic branch
    }

    #[test]
    fn err_is_ignored_and_leaves_buffer_intact() {
        let mut p = with_items(&["apple"]);
        assert_eq!(
            Panel_selectByTyping(&mut p, 'a' as i32),
            HandlerResult::HANDLED
        );
        let r = Panel_selectByTyping(&mut p, ERR);
        assert_eq!(r, HandlerResult::IGNORED);
        assert_eq!(search_buf(&p), "a"); // ERR does not clear the buffer
    }
}
