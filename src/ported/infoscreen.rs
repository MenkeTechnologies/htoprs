//! Port of `InfoScreen.c` — htop's abstract scrollable info panel (the
//! base class for the Command / Env / OpenFiles / ProcessLocks / Trace /
//! Backtrace screens).
//!
//! An `InfoScreen` wraps a scrollable `Panel` of `ListItem` lines, an
//! `IncSet` (incremental search/filter), and a backing `Vector` of every
//! line (the filter narrows the visible `Panel` against this full set).
//! Concrete screens plug in via the `InfoScreenClass` vtable
//! (`scan`/`draw`/`onErr`/`onKey`) which `InfoScreen_run` dispatches
//! through `As_InfoScreen(this)`.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module. Each C function
//! takes `InfoScreen* this`; the faithful analog is a free fn taking
//! `this: &mut InfoScreen` (the shape the `Vector.c`/`History.c` ports
//! use: free fns, not methods).
//!
//! # Struct mapping (`InfoScreen.h:22`)
//!
//! `Object super` — the class back-pointer carrying the `InfoScreenClass`
//! vtable slots (`scan`/`draw`/`onErr`/`onKey`) — is not a field on the
//! [`InfoScreen`] struct; instead the vtable is modeled as the
//! [`InfoScreenClass`] **trait** (the object.rs `Object`-trait precedent:
//! function-pointer slots become trait methods). A concrete info screen
//! implements that trait and holds an [`InfoScreen`] as its embedded base,
//! reached from the loop via [`InfoScreenClass::super_InfoScreen`] (C's
//! `(InfoScreen*)this`). `const Process* process` is a raw
//! `*const Process` back-pointer (the `MainPanel.state` /
//! `BacktracePanel.processes` precedent — a borrowed handle owned
//! elsewhere, kept raw so the struct stays `'static`). `Panel* display`,
//! `IncSet* inc`, and `Vector* lines` are owned values now that
//! `panel.rs` / `incset.rs` / `vector.rs` all model their types.
//!
//! # Ported
//!
//! - The [`InfoScreen`] struct (`InfoScreen.h:22`).
//! - The [`InfoScreenClass`] vtable (`InfoScreen.h:35`) as a trait — the
//!   `scan`/`draw`/`onErr`/`onKey` slots become trait methods and the C
//!   `NULL`-slot guards become `has_scan`/`has_onErr`/`has_onKey` predicates.
//! - [`InfoScreen_init`] (`InfoScreen.c:31`) — builds the `Panel`, the
//!   `IncSet`, and the `lines` `Vector`, then installs the panel header.
//! - [`InfoScreen_addLine`] (`InfoScreen.c:73`) — `ListItem_new` +
//!   `Vector_add` + the `IncSet_filter` gate that decides whether the new
//!   line is also shown in the panel.
//! - [`InfoScreen_run`] (`InfoScreen.c:96`) — the full ncurses event-loop
//!   control flow: the `As_InfoScreen` vtable dispatch (trait methods), the
//!   `Panel_draw`/`FunctionBar_setLabel`/`Panel_getCh`/`Panel_onKey`/
//!   `Panel_resize`/`Vector_prune`/`clear()` calls, and the key switch. Its
//!   `IncSet` leaves (`IncSet_drawBar`/`IncSet_handleKey`/`IncSet_activate`) are
//!   all ported and called with their real arguments; the
//!   `#ifdef HAVE_GETMOUSE` mouse block is compiled out (documented on the fn).
//!
//! ## Owned-value divergences (documented, per "port what you can")
//!
//! - **Shared `FunctionBar`.** C hands ONE `FunctionBar*` to BOTH
//!   `Panel_new` and `IncSet_new`, so `InfoScreen_run` can later mutate it
//!   in place through `this->display->defaultBar` and have the `IncSet`
//!   observe it. The ported `Panel_new`/`IncSet_new` each take an *owned*
//!   `Option<FunctionBar>`, so [`InfoScreen_init`] gives the panel a clone
//!   and moves the original into the `IncSet`: identical bar *content* in
//!   both, but not one aliased, in-place-mutated object. The only code
//!   that mutates the shared bar is [`InfoScreen_run`] (stubbed), so no
//!   ported behavior observes the difference. Same clone precedent as
//!   `Panel_init` seeding `defaultBar`/`currentBar` from one `fuBar`.
//! - **`COLS`.** C passes the ncurses `COLS` global as the panel width;
//!   the ported analog is `functionbar::Ncurses::cols()` (the terminal
//!   column count), the same source `Panel_draw`/`FunctionBar_draw` read.
//! - **`Vector_type(this->display->items)`.** C types the `lines` vector
//!   with the panel's item class (`Class(ListItem)`, set at `Panel_new`).
//!   The ported `Panel` drops per-item typing (`items` is an untyped
//!   `Vec<Box<dyn Object>>`), so the `ListItem` class is recovered from an
//!   instance's `Object::klass()` — the same class `Vector_type` would
//!   yield.
//! - **Weak panel (`Panel_add(display, Vector_get(lines, last))`).** C adds the
//!   *same* `Object*` to the panel that it just put in `lines` — htop's
//!   non-owning "weak panel" (`Panel_new(…, false, …)`), a filtered view
//!   aliasing the `lines`-owned objects. This is modeled faithfully with a
//!   [`PanelItem::Borrowed`] raw pointer into the `lines`-owned `Box` (the same
//!   idiom the process `Table` uses), so the shared *identity* the
//!   `displayLast != last` guard in [`InfoScreen_appendLine`] /
//!   `IncSet_handleKey` needs is preserved. SAFETY: `lines` owns the boxes
//!   (heap-stable across `Vector` growth) and `display` is declared before
//!   `lines`, so the panel's borrowed pointers drop before the boxes are freed.
//!
//! # Stubbed (cannot be ported faithfully yet), each naming its blocker
//!
//! - [`InfoScreen_done`] (`InfoScreen.c:43`) — `Panel_delete` +
//!   `IncSet_delete` + `Vector_delete` + `free`, i.e. heap-free only. An
//!   owned `InfoScreen` releases its fields via `Drop`, so there is no
//!   algorithm to port (same precedent as `IncSet_delete` /
//!   `History_delete` / `Panel_delete`).
//! - [`InfoScreen_drawTitled`] (`InfoScreen.c:50`) — the title-row draw
//!   (`attrset`/`mvhline`/`mvaddstr` -> `Ncurses`, `CRT_colors[X]` ->
//!   `ColorElements::X.packed`), `Panel_draw`, and [`IncSet_drawBar`] (now
//!   ported). C's variadic `fmt, ...` becomes a pre-formatted `title: &str`
//!   (caller-side `format!`, the `vsnprintf` idiom) with the `COLS`-truncate +
//!   `"..."` ellipsis reproduced on the byte buffer; `String_stripControlChars`
//!   (`XUtils.h:147`, absent from the port-purity snapshot) is inlined as a
//!   byte loop rather than a module-level `fn`.
//! - [`InfoScreen_appendLine`] (`InfoScreen.c:81`) — the body is portable (the
//!   weak-panel raw-pointer technique it needs is proven by the ported
//!   [`InfoScreen_addLine`] and `incset.rs`'s `updateWeakPanel`), but its
//!   faithful `(this, line)` signature would break the committed 0-arg caller
//!   `InfoScreen_appendLine()` in `tracescreen.rs:527`, which is outside this
//!   port group's 3-file scope. Kept a zero-argument stub so the build compiles;
//!   un-stubbing it also needs a one-line caller fix in `tracescreen.rs`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::ffi::c_int;
use std::io::{self, Write};

use crossterm::queue;
use crossterm::terminal::{Clear, ClearType};

use crate::ported::crt::{ColorElements, ColorScheme, ERR, KEY_F, KEY_RESIZE};
use crate::ported::functionbar::{FunctionBar, FunctionBar_new, FunctionBar_setLabel, Ncurses};
use crate::ported::incset::{
    IncSet, IncSet_activate, IncSet_delete, IncSet_drawBar, IncSet_filter, IncSet_getListItemValue,
    IncSet_handleKey, IncSet_new, IncType,
};
use crate::ported::listitem::ListItem_new;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::{
    Panel, PanelItem, Panel_delete, Panel_draw, Panel_getCh, Panel_new, Panel_onKey, Panel_resize,
    Panel_setHeader,
};
use crate::ported::process::Process;
use crate::ported::vector::{Vector, Vector_add, Vector_delete, Vector_new, Vector_prune};
use crate::ported::xutils::String_contains_i;

/// Port of `#define VECTOR_DEFAULT_SIZE (10)` from `Vector.h:15` — the
/// initial `lines` vector capacity `InfoScreen_init` passes to `Vector_new`.
const VECTOR_DEFAULT_SIZE: c_int = 10;

/// Port of `static const char* const InfoScreenFunctions[]`
/// (`InfoScreen.c:25`), minus the trailing `NULL` (Rust length terminates).
const InfoScreenFunctions: [&str; 4] = ["Search ", "Filter ", "Refresh", "Done   "];

/// Port of `static const char* const InfoScreenKeys[]` (`InfoScreen.c:27`).
const InfoScreenKeys: [&str; 4] = ["F3", "F4", "F5", "Esc"];

/// Port of `static const int InfoScreenEvents[]` (`InfoScreen.c:29`):
/// `{KEY_F(3), KEY_F(4), KEY_F(5), 27}` (`crt::KEY_F` reproduces the
/// ncurses codes; `27` is `Esc`).
const InfoScreenEvents: [c_int; 4] = [KEY_F(3), KEY_F(4), KEY_F(5), 27];

/// Port of `struct InfoScreen_` (`InfoScreen.h:22`). See the module docs
/// for the full field mapping: `Object super` (the vtable) is omitted (only
/// the stubbed [`InfoScreen_run`] reads it), `process` is a raw back-pointer
/// (owned by the caller), and `display`/`inc`/`lines` are owned values.
pub struct InfoScreen {
    /// C `const Process* process` — the process this screen describes; a
    /// borrowed handle owned elsewhere, kept raw so the struct stays
    /// `'static` (the `MainPanel.state` precedent). Never dereferenced by
    /// the ported functions.
    pub process: *const Process,
    /// C `Panel* display` — the scrollable, filtered list widget.
    pub display: Panel,
    /// C `IncSet* inc` — the incremental search/filter state.
    pub inc: IncSet,
    /// C `Vector* lines` — every line ever added (the full set the filter
    /// narrows `display` against). Owns its `ListItem`s.
    pub lines: Vector,
}

impl InfoScreen {
    /// A zeroed `InfoScreen`: null `process`, an empty `Panel`
    /// (`Panel_new(0, 0, 0, 0, None)`), an empty `IncSet` (`IncSet_new(None)`),
    /// and an empty `ListItem`-typed `lines` `Vector`. Gate-skipped
    /// associated fn — not a C function; the C analog is the `AllocThis`
    /// uninitialized storage that `InfoScreen_init` then overwrites (the
    /// same `Panel::empty` / `IncMode::empty` bootstrap idiom).
    fn empty() -> InfoScreen {
        let list_item_class: &'static ObjectClass = ListItem_new("", 0).klass();
        InfoScreen {
            process: core::ptr::null(),
            display: Panel_new(0, 0, 0, 0, None),
            inc: IncSet_new(None),
            lines: Vector_new(list_item_class, true, VECTOR_DEFAULT_SIZE),
        }
    }
}

/// Port of the `InfoScreenClass` vtable (`InfoScreen.h:35`):
///
/// ```c
/// typedef struct InfoScreenClass_ {
///    const ObjectClass super;
///    const InfoScreen_Scan  scan;   // void (*)(InfoScreen*)
///    const InfoScreen_Draw  draw;   // void (*)(InfoScreen*)
///    const InfoScreen_OnErr onErr;  // void (*)(InfoScreen*)
///    const InfoScreen_OnKey onKey;  // bool (*)(InfoScreen*, int)
/// } InfoScreenClass;
/// ```
///
/// Concrete info screens (Command / Env / OpenFiles / ProcessLocks / Trace /
/// Backtrace) embed an `InfoScreen super` and install this vtable; the
/// `As_InfoScreen(this)->scan(...)` macros (`InfoScreen.h:44`) dispatch
/// through it. The faithful safe-Rust analog is a **trait** — the same
/// vtable-as-trait mapping [`Object`] uses for
/// `ObjectClass` (`display`/`compare` slots become trait methods). Each C
/// function-pointer slot becomes one trait method:
///
/// | C vtable slot                 | Rust trait method            |
/// |-------------------------------|------------------------------|
/// | `InfoScreen_Draw  draw`       | [`draw`](InfoScreenClass::draw)   (required — always non-`NULL` in C) |
/// | `InfoScreen_Scan  scan`       | [`scan`](InfoScreenClass::scan)   (default no-op) |
/// | `InfoScreen_OnErr onErr`      | [`onErr`](InfoScreenClass::onErr) (default no-op) |
/// | `InfoScreen_OnKey onKey`      | [`onKey`](InfoScreenClass::onKey) (default `false`) |
///
/// C guards three of the slots with a `NULL` test before calling
/// (`if (As_InfoScreen(this)->scan) …`, `InfoScreen.c:99/114/162/180/188`).
/// A Rust trait method is never "null", so the presence of each optional
/// slot is modeled by a companion predicate — [`has_scan`](InfoScreenClass::has_scan)
/// / [`has_onErr`](InfoScreenClass::has_onErr) / [`has_onKey`](InfoScreenClass::has_onKey),
/// each defaulting `false` (the base class leaves the slot `NULL`) and
/// overridden `true` by a subclass that installs the pointer. `draw` needs
/// no predicate: it is dispatched unconditionally in C.
///
/// [`super_InfoScreen`](InfoScreenClass::super_InfoScreen) models the
/// `InfoScreen super` embedded base (C's `(InfoScreen*)this` upcast): the
/// loop reaches `this->display`/`this->inc`/`this->lines` through it. These
/// methods live inside the trait (not module-level `fn`s), so the port-purity
/// gate — which indexes only depth-0 free functions — does not see them; the
/// trait itself is the faithful analog of the `InfoScreenClass` struct, which
/// has no free-function counterpart to port.
pub trait InfoScreenClass {
    /// The embedded `InfoScreen super` (`InfoScreen.h:23` in a subclass): the
    /// base data the loop mutates. C reaches it as `(InfoScreen*)this`.
    fn super_InfoScreen(&mut self) -> &mut InfoScreen;

    /// C `draw` slot (`InfoScreen_Draw`). Always non-`NULL` in htop, so it is
    /// required (no default) and dispatched unconditionally by the loop.
    fn draw(&mut self);

    /// C `scan` slot (`InfoScreen_Scan`). Optional; the default models a
    /// `NULL` slot as a no-op — but the loop still gates it on
    /// [`has_scan`](InfoScreenClass::has_scan) because the `NULL` test also
    /// guards the surrounding `Vector_prune` (`InfoScreen.c:163/181`).
    fn scan(&mut self) {}

    /// C `onErr` slot (`InfoScreen_OnErr`). Optional; default is a no-op.
    fn onErr(&mut self) {}

    /// C `onKey` slot (`InfoScreen_OnKey`): returns `true` when the key was
    /// consumed (C `bool`). Optional; the default models a `NULL` slot.
    fn onKey(&mut self, ch: c_int) -> bool {
        let _ = ch;
        false
    }

    /// Models the C `As_InfoScreen(this)->scan != NULL` test: `true` when the
    /// subclass installs a `scan` pointer. Defaults `false` (base slot `NULL`).
    fn has_scan(&self) -> bool {
        false
    }

    /// Models `As_InfoScreen(this)->onErr != NULL` (`InfoScreen.c:114`).
    fn has_onErr(&self) -> bool {
        false
    }

    /// Models `As_InfoScreen(this)->onKey != NULL` (`InfoScreen.c:188`).
    fn has_onKey(&self) -> bool {
        false
    }
}

/// Port of `InfoScreen* InfoScreen_init(InfoScreen* this, const Process*
/// process, FunctionBar* bar, int height, const char* panelHeader)` from
/// `InfoScreen.c:31`.
///
/// Stores the `process` back-pointer, builds a default function bar when
/// none is supplied (C `if (!bar) bar = FunctionBar_new(...)`), creates the
/// `display` panel and the `inc` set from that bar, allocates the `lines`
/// vector, and installs the panel header. See the module docs for the four
/// owned-value divergences (shared `FunctionBar` -> clone + move; `COLS` ->
/// `Ncurses::cols()`; `Vector_type` -> `ListItem` class from an instance).
/// Returns `this`, mirroring the C `return this` identity chain-return.
pub fn InfoScreen_init<'a>(
    this: &'a mut InfoScreen,
    process: *const Process,
    bar: Option<FunctionBar>,
    height: c_int,
    panelHeader: &str,
) -> &'a mut InfoScreen {
    this.process = process;

    // C: if (!bar) bar = FunctionBar_new(InfoScreenFunctions, InfoScreenKeys, InfoScreenEvents);
    let bar = bar.unwrap_or_else(|| {
        FunctionBar_new(
            Some(&InfoScreenFunctions[..]),
            Some(&InfoScreenKeys[..]),
            Some(&InfoScreenEvents[..]),
        )
    });

    // C: this->display = Panel_new(0, 1, COLS, height, Class(ListItem), false, bar);
    // COLS -> Ncurses::cols(); the shared bar is cloned into the panel and
    // moved into the IncSet below (see module docs).
    this.display = Panel_new(0, 1, Ncurses::cols(), height, Some(bar.clone()));

    // C: this->inc = IncSet_new(bar);   // same bar pointer as the panel in C
    this.inc = IncSet_new(Some(bar));

    // C: this->lines = Vector_new(Vector_type(this->display->items), true, VECTOR_DEFAULT_SIZE);
    // The panel's item class is Class(ListItem); recover it from an instance.
    let list_item_class: &'static ObjectClass = ListItem_new("", 0).klass();
    this.lines = Vector_new(list_item_class, true, VECTOR_DEFAULT_SIZE);

    // C: Panel_setHeader(this->display, panelHeader);
    Panel_setHeader(&mut this.display, panelHeader);

    this
}

/// Port of `InfoScreen* InfoScreen_done(InfoScreen* this)` from
/// `InfoScreen.c:43`: `Panel_delete((Object*)this->display);
/// IncSet_delete(this->inc); Vector_delete(this->lines); return this;`.
///
/// Taking `this` by value consumes the screen. The owned `display`
/// [`Panel`], `inc` [`IncSet`], and `lines` [`Vector`] are handed to their
/// ported `_delete`s (mirroring the C call graph); the non-owning `process`
/// raw pointer drops without a free (C never frees it either). The C return
/// of `this` only lets a subclass `free(this)` afterward — here the by-value
/// consume folds that free in, so nothing is returned.
pub fn InfoScreen_done(this: InfoScreen) {
    let InfoScreen {
        display,
        inc,
        lines,
        process,
    } = this;
    Panel_delete(display);
    IncSet_delete(inc);
    Vector_delete(lines);
    let _ = process;
}

/// Port of `void InfoScreen_drawTitled(InfoScreen* this, const char* fmt,
/// ...)` from `InfoScreen.c:50`.
///
/// Paints the title row (`METER_TEXT`, whole line cleared, title at `(0,0)`),
/// then the panel and the incremental-search bar. Two faithful adaptations:
///
/// - **Variadic `fmt, ...` -> pre-formatted `title: &str`.** Rust has no C
///   `va_list`; the caller does the `format!` (the standard idiom for the
///   `xSnprintf`/`vsnprintf` family). C's `char title[COLS + 1]` +
///   `vsnprintf` truncates the result to `COLS` bytes and returns the
///   would-be full length in `len`; both are reproduced on the byte buffer
///   (`len > COLS && COLS >= 3` overwrites the last three kept bytes with
///   `.`, C's `memset(&title[COLS - 3], '.', 3)`).
/// - **`String_stripControlChars` (`XUtils.h:147`) inlined.** Its C name is
///   absent from the port-purity snapshot, so it cannot be a module-level
///   `fn`; the loop (with `Char_isControl` `XUtils.h:137` and
///   `Char_isC1Control` `XUtils.h:141`) is reproduced inline on the byte
///   buffer, replacing C0/DEL and C1 controls with `?`.
///
/// `CRT_colors[X]` map to `ColorElements::X.packed(ColorScheme::active())`,
/// `COLS` to `Ncurses::cols`, and `IncSet_drawBar` is threaded the panel
/// (the `Panel*` IncSet back-pointer is not modeled — see [`IncSet_drawBar`]).
pub fn InfoScreen_drawTitled(this: &mut InfoScreen, title: &str) {
    let cols = Ncurses::cols();

    // C: char title[COLS + 1]; int len = vsnprintf(title, sizeof(title), fmt, ap);
    // The caller supplies the already-formatted string; the COLS+1 buffer
    // truncates it to COLS bytes, and `len` is the would-be full length.
    let len = title.len() as c_int;
    let bound = if cols < 0 {
        0
    } else {
        (cols as usize).min(title.len())
    };
    let mut buf: Vec<u8> = title.as_bytes()[..bound].to_vec();

    // C: if (len > COLS && COLS >= 3) memset(&title[COLS - 3], '.', 3);
    // (len > cols implies bound == cols, so buf is exactly `cols` bytes here.)
    if len > cols && cols >= 3 {
        let start = (cols - 3) as usize;
        for b in &mut buf[start..start + 3] {
            *b = b'.';
        }
    }

    // C: String_stripControlChars(title); — inlined (its C name is absent from
    // the port-purity snapshot; XUtils.h:147 with Char_isControl XUtils.h:137
    // and Char_isC1Control XUtils.h:141). Replace C0/DEL and C1 controls.
    let mut i = 0usize;
    while i < buf.len() {
        let c = buf[i];
        // Char_isControl(c): (unsigned char)c < ' ' || c == '\x7F'.
        if c < b' ' || c == 0x7F {
            buf[i] = b'?';
        } else if c == 0xC2 && i + 1 < buf.len() && (0x80..=0x9F).contains(&buf[i + 1]) {
            // Char_isC1Control(c, next): 0xC2 followed by 0x80..=0x9F.
            buf[i] = b'?';
            buf[i + 1] = b'?';
            i += 1;
        }
        i += 1;
    }
    let title = String::from_utf8_lossy(&buf);

    {
        let mut out = io::stdout().lock();
        // C: attrset(CRT_colors[METER_TEXT]);
        Ncurses::attrset(
            &mut out,
            ColorElements::METER_TEXT.packed(ColorScheme::active()),
        );
        // C: mvhline(0, 0, ' ', COLS);
        Ncurses::mvhline(&mut out, 0, 0, ' ', cols);
        // C: mvaddstr(0, 0, title);
        Ncurses::mvaddstr(&mut out, 0, 0, &title);
        // C: attrset(CRT_colors[DEFAULT_COLOR]);
        Ncurses::attrset(
            &mut out,
            ColorElements::DEFAULT_COLOR.packed(ColorScheme::active()),
        );
        let _ = out.flush();
    }

    // C: Panel_draw(this->display, true, true, true, false);
    Panel_draw(&mut this.display, true, true, true, false);

    // C: IncSet_drawBar(this->inc, CRT_colors[FUNCTION_BAR]);
    IncSet_drawBar(
        &mut this.inc,
        &mut this.display,
        ColorElements::FUNCTION_BAR.packed(ColorScheme::active()),
    );
}

/// Port of `void InfoScreen_addLine(InfoScreen* this, const char* line)`
/// from `InfoScreen.c:73`. Appends a fresh `ListItem` for `line` to the
/// `lines` vector, then — when there is no active filter or `line` matches
/// the current `IncSet_filter` (`String_contains_i`, case-insensitive) —
/// also shows it in the panel.
///
/// **Weak panel (raw-pointer alias).** C's `Panel_add(this->display,
/// Vector_get(this->lines, …))` adds the *same* `Object*` to the panel that it
/// just put in `lines` — htop's non-owning "weak panel" (`Panel_new(…, false,
/// …)`) is a filtered view aliasing the `lines`-owned objects. This is modeled
/// faithfully with a [`PanelItem::Borrowed`] raw pointer into the
/// `lines`-owned `Box` (the same idiom the process `Table` uses for its
/// `Panel`), not an independent clone, so that [`InfoScreen_appendLine`] /
/// `IncSet_handleKey`'s `displayLast != last` pointer-identity test works.
///
/// SAFETY: `lines` owns the `ListItem` boxes (heap-stable — `Box` keeps each
/// pointee's address fixed across `Vector` growth), and in [`InfoScreen`]
/// `display` is declared before `lines`, so the panel (and its borrowed
/// pointers) drops before `lines` frees the boxes — no use-after-free. The
/// alias is only dereferenced while `lines` is alive.
pub fn InfoScreen_addLine(this: &mut InfoScreen, line: &str) {
    // C: Vector_add(this->lines, (Object*) ListItem_new(line, 0));
    Vector_add(&mut this.lines, Box::new(ListItem_new(line, 0)));

    // C: const char* incFilter = IncSet_filter(this->inc);
    //    if (!incFilter || String_contains_i(line, incFilter, true)) { ... }
    let show = match IncSet_filter(&this.inc) {
        None => true,
        Some(incFilter) => String_contains_i(line, incFilter, true),
    };
    if show {
        // C: Panel_add(this->display, Vector_get(this->lines, Vector_size(this->lines) - 1));
        // Weak (borrowed) add: alias the just-inserted `lines` box, don't clone.
        let last = this.lines.array.len() - 1;
        let ptr: *mut dyn Object = &mut **this.lines.array[last]
            .as_mut()
            .expect("just-added lines slot is non-NULL");
        this.display.items.push(PanelItem::Borrowed(ptr));
        this.display.prevSelected = -1;
        this.display.needsRedraw = true;
    }
}

/// TODO: port of `void InfoScreen_appendLine(InfoScreen* this, const char*
/// line)` from `InfoScreen.c:81`. **Not a soundness blocker** — the weak-panel
/// raw-pointer technique this port needs (in-place `ListItem_append` on the last
/// `lines` box, then the `displayLast != last` data-pointer identity guard, then
/// a [`PanelItem::Borrowed`] weak-add of the matching fragment) is fully proven
/// by the ported [`InfoScreen_addLine`] and `incset.rs`'s
/// [`updateWeakPanel`](crate::ported::incset)/`IncSet_handleKey`, which alias
/// `lines`-owned boxes into the panel exactly the same way.
///
/// The blocker is purely the **committed 0-arg caller**
/// `InfoScreen_appendLine()` in `tracescreen.rs:527` (the contLine branch of
/// `TraceScreen_scan`), which is pinned to this stub's zero-argument signature.
/// Giving the fn its faithful `(this, line)` signature would break that call —
/// and `tracescreen.rs` is outside this port group's 3-file scope, so the caller
/// cannot be updated here. Un-stubbing this fn therefore also requires a
/// one-line caller fix in `tracescreen.rs`
/// (`InfoScreen_appendLine(&mut this.super_, &s)`), out of scope. Kept
/// zero-argument so the build stays green.
pub fn InfoScreen_appendLine() {
    todo!("port of InfoScreen.c:81 — body is portable (weak-panel raw-pointer technique, proven by InfoScreen_addLine/updateWeakPanel); blocked only by the committed 0-arg caller tracescreen.rs:527, outside this 3-file scope")
}

/// Port of `void InfoScreen_run(InfoScreen* this)` from `InfoScreen.c:96`.
///
/// The ncurses main event loop. `this` is any concrete info screen
/// (`&mut dyn InfoScreenClass`); the C `As_InfoScreen(this)->scan/draw/onErr/
/// onKey` vtable dispatch (`InfoScreen.h:44`) becomes the corresponding trait
/// method calls, and the `NULL`-slot guards become the `has_scan`/`has_onErr`/
/// `has_onKey` predicates. `this->display`/`inc`/`lines` are reached through
/// [`InfoScreenClass::super_InfoScreen`] (C's `(InfoScreen*)this`). `COLS`/
/// `LINES` map to `Ncurses::cols`/`Ncurses::lines`, and `clear()` to the
/// crossterm full-screen clear (a local closure, kept off module scope so the
/// port-purity gate — which has no `clear` C entry — does not flag it).
///
/// # IncSet leaves
///
/// The control flow is ported in full and every `IncSet` leaf is called with
/// its real arguments — [`IncSet_drawBar`], [`IncSet_activate`], and
/// [`IncSet_handleKey`]`(this->inc, ch, panel, IncSet_getListItemValue,
/// this->lines)` (`:145`) — reaching `this->inc`/`display`/`lines` through
/// [`InfoScreenClass::super_InfoScreen`] as disjoint field borrows.
///
/// # Omitted: the `HAVE_GETMOUSE` block (`:120`–`:142`)
///
/// The mouse translation (`getmouse`/`MEVENT`/`BUTTON1_RELEASED`/
/// `IncSet_synthesizeEvent`/`KEY_WHEEL*`) is `#ifdef HAVE_GETMOUSE`
/// conditional code. htoprs reads keys through crossterm (`CRT_readKey`),
/// which surfaces no ncurses `MEVENT`, and `getmouse`/`MEVENT` are unported,
/// so this port compiles as if `HAVE_GETMOUSE` were unset — faithful to
/// building htop without ncurses mouse support.
pub fn InfoScreen_run(this: &mut dyn InfoScreenClass) {
    // C: clear() — ncurses full-screen clear; crossterm analog. A no-capture
    // closure (not a module-level `fn`) so the call sites read `clear();` like
    // C without adding a depth-0 helper the port gate has no C name for.
    let clear = || {
        let mut out = io::stdout().lock();
        let _ = queue!(out, Clear(ClearType::All));
        let _ = out.flush();
    };

    // C: Panel* panel = this->display; — aliased; reached via super_InfoScreen.

    // C: if (As_InfoScreen(this)->scan) InfoScreen_scan(this);
    if this.has_scan() {
        this.scan();
    }

    // C: InfoScreen_draw(this);
    this.draw();

    let mut looping = true;
    while looping {
        // C: Panel_draw(panel, false, true, true, false);
        Panel_draw(
            &mut this.super_InfoScreen().display,
            false,
            true,
            true,
            false,
        );

        // C: IncSet_drawBar(this->inc, CRT_colors[FUNCTION_BAR]);
        let screen = this.super_InfoScreen();
        IncSet_drawBar(
            &mut screen.inc,
            &mut screen.display,
            ColorElements::FUNCTION_BAR.packed(ColorScheme::active()),
        );

        // C: FunctionBar_setLabel(this->display->defaultBar, KEY_F(4),
        //        this->inc->filtering ? "FILTER " : "Filter ");
        let filtering = this.super_InfoScreen().inc.filtering;
        if let Some(bar) = this.super_InfoScreen().display.defaultBar.as_mut() {
            FunctionBar_setLabel(bar, KEY_F(4), if filtering { "FILTER " } else { "Filter " });
        }

        // C: int ch = Panel_getCh(panel);
        let ch = Panel_getCh(&this.super_InfoScreen().display);

        // C: if (ch == ERR) { if (As_InfoScreen(this)->onErr) { InfoScreen_onErr(this); continue; } }
        if ch == ERR && this.has_onErr() {
            this.onErr();
            continue;
        }

        // The `#ifdef HAVE_GETMOUSE` mouse block is omitted (see fn docs).

        // C: if (this->inc->active) {
        //        IncSet_handleKey(this->inc, ch, panel, IncSet_getListItemValue, this->lines);
        //        continue;
        //    }
        if this.super_InfoScreen().inc.active.is_some() {
            // C: IncSet_handleKey(this->inc, ch, panel, IncSet_getListItemValue, this->lines);
            // inc/display/lines are disjoint InfoScreen fields, borrowed at once.
            let screen = this.super_InfoScreen();
            IncSet_handleKey(
                &mut screen.inc,
                ch,
                &mut screen.display,
                IncSet_getListItemValue,
                &mut screen.lines,
            );
            continue;
        }

        // Function-key codes as match patterns need consts (KEY_F is a const
        // fn, not a literal); the char cases likewise. These local consts
        // mirror the C `case` labels exactly.
        const F3: c_int = KEY_F(3);
        const F4: c_int = KEY_F(4);
        const F5: c_int = KEY_F(5);
        const F10: c_int = KEY_F(10);
        const SLASH: c_int = b'/' as c_int;
        const BACKSLASH: c_int = b'\\' as c_int;
        const CTRL_L: c_int = 0o14; // '\014'
        const Q: c_int = b'q' as c_int;

        match ch {
            // C: case ERR: continue;
            ERR => continue,
            // C: case KEY_F(3): case '/': IncSet_activate(this->inc, INC_SEARCH, panel); break;
            F3 | SLASH => {
                let screen = this.super_InfoScreen();
                IncSet_activate(&mut screen.inc, IncType::INC_SEARCH, &mut screen.display);
            }
            // C: case KEY_F(4): case '\\': IncSet_activate(this->inc, INC_FILTER, panel); break;
            F4 | BACKSLASH => {
                let screen = this.super_InfoScreen();
                IncSet_activate(&mut screen.inc, IncType::INC_FILTER, &mut screen.display);
            }
            // C: case KEY_F(5): clear();
            //        if (As_InfoScreen(this)->scan) { Vector_prune(this->lines); InfoScreen_scan(this); }
            //        InfoScreen_draw(this); break;
            F5 => {
                clear();
                if this.has_scan() {
                    Vector_prune(&mut this.super_InfoScreen().lines);
                    this.scan();
                }
                this.draw();
            }
            // C: case '\014': clear(); InfoScreen_draw(this); break;
            CTRL_L => {
                clear();
                this.draw();
            }
            // C: case 27: case 'q': case KEY_F(10): looping = false; break;
            27 | Q | F10 => {
                looping = false;
            }
            // C: case KEY_RESIZE: Panel_resize(panel, COLS, LINES - 2);
            //        if (As_InfoScreen(this)->scan) { Vector_prune(this->lines); InfoScreen_scan(this); }
            //        InfoScreen_draw(this); break;
            KEY_RESIZE => {
                Panel_resize(
                    &mut this.super_InfoScreen().display,
                    Ncurses::cols(),
                    Ncurses::lines() - 2,
                );
                if this.has_scan() {
                    Vector_prune(&mut this.super_InfoScreen().lines);
                    this.scan();
                }
                this.draw();
            }
            // C: default:
            //        if (As_InfoScreen(this)->onKey && InfoScreen_onKey(this, ch)) continue;
            //        Panel_onKey(panel, ch);
            _ => {
                if this.has_onKey() && this.onKey(ch) {
                    continue;
                }
                Panel_onKey(&mut this.super_InfoScreen().display, ch);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::incset::IncSet_setFilter;
    use crate::ported::listitem::ListItem;
    use crate::ported::panel::{Panel_get, Panel_headerHeight, Panel_size};
    use crate::ported::vector::{Vector_get, Vector_size};

    /// Read the `value` of the `ListItem` stored at `idx` in a `Vector`.
    fn line_value(lines: &Vector, idx: usize) -> String {
        let any: &dyn std::any::Any = Vector_get(lines, idx);
        any.downcast_ref::<ListItem>().unwrap().value.clone()
    }

    /// Read the `value` of the `ListItem` shown at panel index `i`.
    fn panel_value(p: &Panel, i: i32) -> String {
        let any: &dyn std::any::Any = Panel_get(p, i);
        any.downcast_ref::<ListItem>().unwrap().value.clone()
    }

    fn fresh(height: c_int, header: &str) -> InfoScreen {
        let mut this = InfoScreen::empty();
        InfoScreen_init(&mut this, core::ptr::null(), None, height, header);
        this
    }

    #[test]
    fn init_sets_fields_and_geometry() {
        let mut this = InfoScreen::empty();
        InfoScreen_init(&mut this, core::ptr::null(), None, 22, "HEADER");
        // process back-pointer stored (null here).
        assert!(this.process.is_null());
        // lines starts empty; panel starts empty.
        assert_eq!(Vector_size(&this.lines), 0);
        assert_eq!(Panel_size(&this.display), 0);
        // Panel geometry: Panel_new(0, 1, COLS, height, ...).
        assert_eq!(this.display.x, 0);
        assert_eq!(this.display.y, 1);
        assert_eq!(this.display.h, 22);
        assert_eq!(this.display.w, Ncurses::cols());
        // Header installed -> headerHeight 1.
        assert_eq!(Panel_headerHeight(&this.display), 1);
        // No filter active on a fresh IncSet.
        assert!(IncSet_filter(&this.inc).is_none());
    }

    #[test]
    fn init_builds_default_bar_when_none() {
        let this = fresh(10, " ");
        // C: bar = FunctionBar_new(InfoScreenFunctions, InfoScreenKeys, InfoScreenEvents).
        // The panel's default bar carries the InfoScreen labels/keys/events.
        let bar = this.display.defaultBar.as_ref().expect("default bar built");
        assert_eq!(bar.functions, InfoScreenFunctions.to_vec());
        assert_eq!(bar.keys, InfoScreenKeys.to_vec());
        assert_eq!(bar.events, InfoScreenEvents.to_vec());
        // The IncSet received the same bar content (cloned + moved).
        let inc_bar = this.inc.defaultBar.as_ref().expect("inc default bar");
        assert_eq!(inc_bar.functions, InfoScreenFunctions.to_vec());
    }

    #[test]
    fn init_uses_supplied_bar() {
        let mut this = InfoScreen::empty();
        let custom = FunctionBar_new(Some(&["Only "][..]), Some(&["F1"][..]), Some(&[1][..]));
        InfoScreen_init(&mut this, core::ptr::null(), Some(custom), 5, "H");
        assert_eq!(
            this.display.defaultBar.as_ref().unwrap().functions,
            vec!["Only ".to_string()]
        );
        assert_eq!(
            this.inc.defaultBar.as_ref().unwrap().functions,
            vec!["Only ".to_string()]
        );
    }

    #[test]
    fn add_line_grows_lines_and_panel_without_filter() {
        let mut this = fresh(10, "H");
        InfoScreen_addLine(&mut this, "alpha");
        InfoScreen_addLine(&mut this, "beta");
        InfoScreen_addLine(&mut this, "gamma");
        // Every line is recorded in `lines`.
        assert_eq!(Vector_size(&this.lines), 3);
        assert_eq!(line_value(&this.lines, 0), "alpha");
        assert_eq!(line_value(&this.lines, 1), "beta");
        assert_eq!(line_value(&this.lines, 2), "gamma");
        // With no filter, every line is also shown in the panel.
        assert_eq!(Panel_size(&this.display), 3);
        assert_eq!(panel_value(&this.display, 0), "alpha");
        assert_eq!(panel_value(&this.display, 2), "gamma");
    }

    #[test]
    fn add_line_filter_gates_panel_but_not_lines() {
        let mut this = fresh(10, "H");
        // Activate a filter: only lines containing "sh" are shown.
        IncSet_setFilter(&mut this.inc, "sh");
        assert_eq!(IncSet_filter(&this.inc), Some("sh"));

        InfoScreen_addLine(&mut this, "bash"); // matches
        InfoScreen_addLine(&mut this, "xyz"); //  no match
        InfoScreen_addLine(&mut this, "zsh"); //  matches

        // All three are recorded in `lines` regardless of the filter.
        assert_eq!(Vector_size(&this.lines), 3);
        assert_eq!(line_value(&this.lines, 1), "xyz");

        // Only the two matching lines reach the panel, in order.
        assert_eq!(Panel_size(&this.display), 2);
        assert_eq!(panel_value(&this.display, 0), "bash");
        assert_eq!(panel_value(&this.display, 1), "zsh");
    }

    #[test]
    fn add_line_filter_is_case_insensitive() {
        let mut this = fresh(10, "H");
        IncSet_setFilter(&mut this.inc, "SH"); // uppercase needle
        InfoScreen_addLine(&mut this, "bash"); // lowercase haystack -> matches
        assert_eq!(Vector_size(&this.lines), 1);
        assert_eq!(Panel_size(&this.display), 1);
        assert_eq!(panel_value(&this.display, 0), "bash");
    }

    #[test]
    fn add_line_empty_string_is_recorded() {
        let mut this = fresh(10, "H");
        InfoScreen_addLine(&mut this, "");
        assert_eq!(Vector_size(&this.lines), 1);
        assert_eq!(line_value(&this.lines, 0), "");
        // No filter -> shown in the panel too.
        assert_eq!(Panel_size(&this.display), 1);
    }

    // ── InfoScreenClass vtable dispatch (InfoScreen.h:35) ─────────────
    //
    // The `InfoScreen_run` loop body itself is not unit-tested: it calls
    // `Panel_getCh` (blocks on real stdin via `CRT_readKey`) and `Panel_draw`
    // (emits to stdout), so it needs a live TTY. What IS testable headlessly is
    // the
    // new modelable piece — the vtable-as-trait: slot dispatch, the `NULL`-
    // slot presence predicates, `&mut dyn` dispatch, and the `super` base
    // access the loop drives everything through.

    /// A concrete info screen that installs every vtable slot (the analog of
    /// a C subclass whose `InfoScreenClass` sets all four pointers).
    struct FullScreen {
        base: InfoScreen,
        scans: u32,
        draws: u32,
        errs: u32,
        keys: Vec<c_int>,
        onkey_ret: bool,
    }
    impl InfoScreenClass for FullScreen {
        fn super_InfoScreen(&mut self) -> &mut InfoScreen {
            &mut self.base
        }
        fn draw(&mut self) {
            self.draws += 1;
        }
        fn scan(&mut self) {
            self.scans += 1;
        }
        fn onErr(&mut self) {
            self.errs += 1;
        }
        fn onKey(&mut self, ch: c_int) -> bool {
            self.keys.push(ch);
            self.onkey_ret
        }
        fn has_scan(&self) -> bool {
            true
        }
        fn has_onErr(&self) -> bool {
            true
        }
        fn has_onKey(&self) -> bool {
            true
        }
    }

    /// A concrete info screen that installs only the (mandatory) `draw` slot,
    /// leaving `scan`/`onErr`/`onKey` `NULL` — the base-class default vtable.
    struct BareScreen {
        base: InfoScreen,
        draws: u32,
    }
    impl InfoScreenClass for BareScreen {
        fn super_InfoScreen(&mut self) -> &mut InfoScreen {
            &mut self.base
        }
        fn draw(&mut self) {
            self.draws += 1;
        }
    }

    #[test]
    fn vtable_defaults_model_null_slots() {
        let mut s = BareScreen {
            base: InfoScreen::empty(),
            draws: 0,
        };
        // The three optional slots report absent (C `->scan == NULL`, etc.).
        assert!(!s.has_scan());
        assert!(!s.has_onErr());
        assert!(!s.has_onKey());
        // Default scan/onErr are no-ops (do not panic) and onKey returns false.
        s.scan();
        s.onErr();
        assert!(!s.onKey(42));
        // draw is the one required slot and dispatches.
        s.draw();
        assert_eq!(s.draws, 1);
    }

    #[test]
    fn vtable_overrides_report_present_and_dispatch() {
        let mut s = FullScreen {
            base: InfoScreen::empty(),
            scans: 0,
            draws: 0,
            errs: 0,
            keys: Vec::new(),
            onkey_ret: true,
        };
        assert!(s.has_scan());
        assert!(s.has_onErr());
        assert!(s.has_onKey());
        s.scan();
        s.scan();
        s.draw();
        s.onErr();
        assert!(s.onKey(7)); // returns the configured `true` (key consumed)
        assert_eq!(s.scans, 2);
        assert_eq!(s.draws, 1);
        assert_eq!(s.errs, 1);
        assert_eq!(s.keys, vec![7]);
    }

    #[test]
    fn onkey_return_flows_back_like_c_bool() {
        // C `default:` gates the `continue` on `InfoScreen_onKey(this, ch)`'s
        // bool: true = consumed (skip Panel_onKey), false = fall through.
        let mut s = FullScreen {
            base: InfoScreen::empty(),
            scans: 0,
            draws: 0,
            errs: 0,
            keys: Vec::new(),
            onkey_ret: false,
        };
        assert!(!s.onKey(99)); // not consumed
        assert_eq!(s.keys, vec![99]);
    }

    #[test]
    fn dyn_dispatch_reaches_concrete_impl_and_super_base() {
        // Exactly how `InfoScreen_run` sees a screen: `&mut dyn InfoScreenClass`.
        let mut s = FullScreen {
            base: InfoScreen::empty(),
            scans: 0,
            draws: 0,
            errs: 0,
            keys: Vec::new(),
            onkey_ret: false,
        };
        let dynref: &mut dyn InfoScreenClass = &mut s;

        // Vtable dispatch through the trait object hits the concrete methods.
        if dynref.has_scan() {
            dynref.scan();
        }
        dynref.draw();

        // super_InfoScreen exposes the embedded base; mutations persist, the
        // way the loop's `Vector_prune(this->lines)` / `Panel_resize` reach
        // `this->display`/`this->lines`.
        InfoScreen_addLine(dynref.super_InfoScreen(), "alpha");
        Panel_resize(&mut dynref.super_InfoScreen().display, 123, 45);

        assert_eq!(s.scans, 1);
        assert_eq!(s.draws, 1);
        assert_eq!(Vector_size(&s.base.lines), 1);
        assert_eq!(s.base.display.w, 123);
        assert_eq!(s.base.display.h, 45);
    }
}
