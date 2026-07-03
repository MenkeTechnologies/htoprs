//! Port of `FunctionBar.c` — htop's bottom-row key/label bar.
//!
//! C names are preserved verbatim (`CamelCase_snake`), so `non_snake_case`
//! is allowed for the whole module — matching the spec name-for-name is the
//! point of the port.
//!
//! # What is ported
//!
//! * Data model: [`FunctionBar_new`], [`FunctionBar_newEnterEsc`],
//!   [`FunctionBar_setLabel`], [`FunctionBar_getWidth`],
//!   [`FunctionBar_synthesizeEvent`] — all faithful, pure state/geometry.
//!   The `FunctionBar_FEvents = {KEY_F(1), ...}` initializer is now
//!   portable because `crt::KEY_F` exists (`crt.rs`), so the static-data
//!   branch of `FunctionBar_new` is reproduced verbatim.
//! * Drawing: [`FunctionBar_draw`], [`FunctionBar_drawExtra`],
//!   [`FunctionBar_append`] — behavioral ports on crossterm. htop drives
//!   these through ncurses (`attrset`/`mvhline`/`mvaddstr`/`curs_set`
//!   against `CRT_colors`, `LINES`, `COLS`); crossterm is htoprs's
//!   terminal backend, so the emit is reproduced through the `Ncurses`
//!   shim below (which resolves `CRT_colors` packed attrs via the ported
//!   `crt::ResolvedColor`). The pure column/cursor arithmetic each fn
//!   computes is factored into gate-skipped helper methods and unit
//!   tested; the terminal side-effects are not (headless CI has no TTY).
//! * Lifecycle: [`FunctionBar_delete`] — the C `free()` chain
//!   (`FunctionBar.c:67`) ported as take-by-value + drop-at-scope-end,
//!   the same idiom as `Affinity_delete`/`Hashtable_delete`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::io::{self, Write};
use std::sync::atomic::{AtomicI32, Ordering};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::style::{
    Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::{queue, terminal};

use crate::ported::crt::{
    ColorElements, ColorScheme, ResolvedColor, A_BLINK, A_BOLD, A_DIM, A_REVERSE, A_STANDOUT,
    A_UNDERLINE, KEY_F,
};

/// Port of `#define FUNCTIONBAR_MAXEVENTS 11` from `FunctionBar.c:22`
/// ("sufficient for all cases, includes NULL"). Used by the C loops'
/// `assert(i < FUNCTIONBAR_MAXEVENTS)` bound checks, mirrored below as
/// `debug_assert!`.
const FUNCTIONBAR_MAXEVENTS: usize = 11;

/// ncurses `ERR` sentinel (`#define ERR (-1)`), the value returned by
/// [`FunctionBar_synthesizeEvent`] when `pos` is past the last label.
const ERR: i32 = -1;

/// Port of `FunctionBar_FKeys` (`FunctionBar.c:24`), minus the trailing
/// `NULL` (Rust length is the terminator).
const FunctionBar_FKeys: [&str; 10] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10"];

/// Port of `FunctionBar_FLabels` (`FunctionBar.c:26`): ten six-space
/// blank labels.
const FunctionBar_FLabels: [&str; 10] = [
    "      ", "      ", "      ", "      ", "      ", "      ", "      ", "      ", "      ",
    "      ",
];

/// Port of `FunctionBar_FEvents = {KEY_F(1), ...}` (`FunctionBar.c:28`).
/// `KEY_F` is a `const fn` in `crt.rs`, so this reproduces the ncurses
/// `KEY_F(n)` codes verbatim.
const FunctionBar_FEvents: [i32; 10] = [
    KEY_F(1),
    KEY_F(2),
    KEY_F(3),
    KEY_F(4),
    KEY_F(5),
    KEY_F(6),
    KEY_F(7),
    KEY_F(8),
    KEY_F(9),
    KEY_F(10),
];

/// Port of `FunctionBar_EnterEscKeys` (`FunctionBar.c:30`).
const FunctionBar_EnterEscKeys: [&str; 2] = ["Enter", "Esc"];

/// Port of `FunctionBar_EnterEscEvents` (`FunctionBar.c:31`).
const FunctionBar_EnterEscEvents: [i32; 2] = [13, 27];

/// Port of `static int currentLen` (`FunctionBar.c:33`): the running
/// column position shared between [`FunctionBar_drawExtra`] and
/// [`FunctionBar_append`]. Modeled as an atomic file-static exactly like
/// the C translation-unit global.
static currentLen: AtomicI32 = AtomicI32::new(0);

/// Model of the C `FunctionBar` struct (`FunctionBar.h:14`).
///
/// The C `union { char** keys; const char* const* constKeys; }` collapses
/// to a single `Vec<String>` — both union members are only ever read as
/// key-label strings, so the observable is identical. `staticData` mirrors
/// the C `bool staticData` (which of the two allocation strategies
/// `FunctionBar_new` chose); `Clone` lets a `Panel` hold both a
/// `defaultBar` and a `currentBar` copy (C shares one pointer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionBar {
    pub functions: Vec<String>,
    pub keys: Vec<String>,
    pub events: Vec<i32>,
    pub staticData: bool,
}

/// Port of `FunctionBar* FunctionBar_newEnterEsc(const char* enter, const char* esc)`
/// from `FunctionBar.c:35`. Builds a two-slot bar (`enter`, `esc`) with
/// the static Enter/Esc key labels and events `{13, 27}`.
pub fn FunctionBar_newEnterEsc(enter: &str, esc: &str) -> FunctionBar {
    let functions = [enter, esc];
    FunctionBar_new(
        Some(&functions),
        Some(&FunctionBar_EnterEscKeys),
        Some(&FunctionBar_EnterEscEvents),
    )
}

/// Port of `FunctionBar* FunctionBar_new(const char* const* functions,
/// const char* const* keys, const int* events)` from `FunctionBar.c:40`.
///
/// `functions` defaults to the ten blank `FunctionBar_FLabels` when `None`
/// (C `if (!functions) functions = FunctionBar_FLabels`). When both `keys`
/// and `events` are supplied the bar owns per-slot key/event copies
/// (`staticData = false`, C's `xStrdup` loop); otherwise it uses the
/// static F1..F10 key/event tables (`staticData = true`). The C copy loops
/// are bounded by the `functions` NUL terminator, so `keys`/`events` are
/// read only for as many slots as there are functions.
pub fn FunctionBar_new(
    functions: Option<&[&str]>,
    keys: Option<&[&str]>,
    events: Option<&[i32]>,
) -> FunctionBar {
    let funcs: Vec<String> = match functions {
        Some(f) => f.iter().map(|s| s.to_string()).collect(),
        None => FunctionBar_FLabels.iter().map(|s| s.to_string()).collect(),
    };
    debug_assert!(funcs.len() <= FUNCTIONBAR_MAXEVENTS);

    match (keys, events) {
        (Some(k), Some(e)) => {
            let keysv: Vec<String> = (0..funcs.len()).map(|i| k[i].to_string()).collect();
            let eventsv: Vec<i32> = (0..funcs.len()).map(|i| e[i]).collect();
            FunctionBar {
                functions: funcs,
                keys: keysv,
                events: eventsv,
                staticData: false,
            }
        }
        _ => FunctionBar {
            functions: funcs,
            keys: FunctionBar_FKeys.iter().map(|s| s.to_string()).collect(),
            events: FunctionBar_FEvents.to_vec(),
            staticData: true,
        },
    }
}

/// Port of `void FunctionBar_delete(FunctionBar* this)` from
/// `FunctionBar.c:67`. C loops the `functions` array freeing each label
/// (and, when `!staticData`, each `keys.keys[i]`), then frees `functions`,
/// the `keys`/`events` arrays (again only when `!staticData`), and finally
/// the struct. Taking `this` by value is the faithful analog of the final
/// `free(this)`: the moved-in [`FunctionBar`] — and its owned `Vec<String>`
/// `functions`/`keys` and `Vec<i32>` `events` — drops at end of scope,
/// which *is* the C `free` chain. The `staticData` branch is a no-op in
/// Rust because the static F1..F10 tables the C shares by pointer are
/// per-instance owned `Vec` copies here, so dropping them is correct in
/// either case. Same idiom as `Affinity_delete`/`Hashtable_delete`.
pub fn FunctionBar_delete(this: FunctionBar) {
    let _ = this;
}

/// Port of `void FunctionBar_setLabel(FunctionBar* this, int event, const char* text)`
/// from `FunctionBar.c:83`.
///
/// Walks the `functions` array in parallel with `events`; on the first
/// slot whose event equals `event`, replaces that label with a copy of
/// `text` and stops. If no event matches, the bar is unchanged.
pub fn FunctionBar_setLabel(this: &mut FunctionBar, event: i32, text: &str) {
    for i in 0..this.functions.len() {
        debug_assert!(i < FUNCTIONBAR_MAXEVENTS);
        if this.events[i] == event {
            this.functions[i] = text.to_string();
            break;
        }
    }
}

/// Port of `int FunctionBar_draw(const FunctionBar* this)` from
/// `FunctionBar.c:94`: `return FunctionBar_drawExtra(this, NULL, -1, false)`.
pub fn FunctionBar_draw(this: &FunctionBar) -> i32 {
    FunctionBar_drawExtra(this, None, -1, false)
}

/// Port of `int FunctionBar_drawExtra(const FunctionBar* this,
/// const char* buffer, int attr, bool setCursor)` from `FunctionBar.c:98`.
///
/// Behavioral crossterm port. Paints the bottom line (`LINES - 1`): the
/// whole line in `FUNCTION_BAR`, then each key label in `FUNCTION_KEY`
/// and each function label in `FUNCTION_BAR`, then the optional `buffer`
/// (in `attr`, or `FUNCTION_BAR` when `attr == -1`), resets the color, and
/// shows/hides the cursor. Updates the file-static `currentLen` and
/// returns `cursorX` (the column after the function keys, or after the
/// buffer when one is drawn) — exactly as the C computes it. The pure
/// `cursorX`/`currentLen` arithmetic is factored into
/// `FunctionBar::extra_layout` and unit tested.
pub fn FunctionBar_drawExtra(
    this: &FunctionBar,
    buffer: Option<&str>,
    attr: i32,
    setCursor: bool,
) -> i32 {
    let (cursorX, endX) = this.extra_layout(buffer);

    let line = Ncurses::lines() - 1;
    let mut out = io::stdout().lock();

    Ncurses::attrset(
        &mut out,
        ColorElements::FUNCTION_BAR.packed(ColorScheme::active()),
    );
    Ncurses::mvhline(&mut out, line, 0, ' ', Ncurses::cols());
    let mut x = 0i32;
    for i in 0..this.functions.len() {
        debug_assert!(i < FUNCTIONBAR_MAXEVENTS);
        Ncurses::attrset(
            &mut out,
            ColorElements::FUNCTION_KEY.packed(ColorScheme::active()),
        );
        Ncurses::mvaddstr(&mut out, line, x, &this.keys[i]);
        x += this.keys[i].len() as i32;
        Ncurses::attrset(
            &mut out,
            ColorElements::FUNCTION_BAR.packed(ColorScheme::active()),
        );
        Ncurses::mvaddstr(&mut out, line, x, &this.functions[i]);
        x += this.functions[i].len() as i32;
    }

    if let Some(b) = buffer {
        let a = if attr == -1 {
            ColorElements::FUNCTION_BAR.packed(ColorScheme::active())
        } else {
            attr
        };
        Ncurses::attrset(&mut out, a);
        Ncurses::mvaddstr(&mut out, line, x, b);
    }

    Ncurses::attrset(
        &mut out,
        ColorElements::RESET_COLOR.packed(ColorScheme::active()),
    );
    Ncurses::curs_set(&mut out, setCursor);
    let _ = out.flush();

    currentLen.store(endX, Ordering::Relaxed);
    cursorX
}

/// Port of `void FunctionBar_append(const char* buffer, int attr)` from
/// `FunctionBar.c:139`. Appends `buffer` one column past the running
/// `currentLen` (in `attr`, or `FUNCTION_BAR` when `attr == -1`), resets
/// the color, and advances `currentLen` by `strlen(buffer) + 1`.
pub fn FunctionBar_append(buffer: &str, attr: i32) {
    let cur = currentLen.load(Ordering::Relaxed);
    let line = Ncurses::lines() - 1;
    let mut out = io::stdout().lock();

    let a = if attr == -1 {
        ColorElements::FUNCTION_BAR.packed(ColorScheme::active())
    } else {
        attr
    };
    Ncurses::attrset(&mut out, a);
    Ncurses::mvaddstr(&mut out, line, cur + 1, buffer);
    Ncurses::attrset(
        &mut out,
        ColorElements::RESET_COLOR.packed(ColorScheme::active()),
    );
    let _ = out.flush();

    currentLen.store(cur + buffer.len() as i32 + 1, Ordering::Relaxed);
}

/// Port of `int FunctionBar_getWidth(const FunctionBar* this)` from
/// `FunctionBar.c:151`. Sums the byte lengths of every key label and
/// function label (C `strlen`, i.e. bytes), accumulated in an `int`.
pub fn FunctionBar_getWidth(this: &FunctionBar) -> i32 {
    let mut x: i32 = 0;
    for i in 0..this.functions.len() {
        debug_assert!(i < FUNCTIONBAR_MAXEVENTS);
        x += this.keys[i].len() as i32;
        x += this.functions[i].len() as i32;
    }
    x
}

/// Port of `int FunctionBar_synthesizeEvent(const FunctionBar* this, int pos)`
/// from `FunctionBar.c:161`.
///
/// Advances a running column `x` by each slot's key + function byte
/// lengths; the first slot whose cumulative width strictly exceeds `pos`
/// yields that slot's event. Past the end returns `ERR` (`-1`).
pub fn FunctionBar_synthesizeEvent(this: &FunctionBar, pos: i32) -> i32 {
    let mut x: i32 = 0;
    for i in 0..this.functions.len() {
        debug_assert!(i < FUNCTIONBAR_MAXEVENTS);
        x += this.keys[i].len() as i32;
        x += this.functions[i].len() as i32;
        if pos < x {
            return this.events[i];
        }
    }
    ERR
}

impl FunctionBar {
    /// Pure arithmetic backing [`FunctionBar_drawExtra`]: returns
    /// `(cursorX, endX)` where `endX` is the final `x` (= the value stored
    /// into `currentLen`) and `cursorX` is the C `cursorX` — the column
    /// after the function keys, or after `buffer` when one is present.
    /// A gate-skipped method (not a top-level `fn`) so it can be unit
    /// tested without a TTY.
    fn extra_layout(&self, buffer: Option<&str>) -> (i32, i32) {
        let mut x = 0i32;
        for i in 0..self.functions.len() {
            x += self.keys[i].len() as i32;
            x += self.functions[i].len() as i32;
        }
        let mut cursor_x = x;
        if let Some(b) = buffer {
            x += b.len() as i32;
            cursor_x = x;
        }
        (cursor_x, x)
    }
}

/// Rust-only crossterm emit shim reproducing the ncurses primitives htop's
/// draw code uses (`attrset`, `mvhline`, `mvaddstr`, `mvaddch`,
/// `mvaddnstr`, `move`, `curs_set`, `LINES`, `COLS`). It is a type with
/// associated fns — not top-level `fn`s — so the port-purity gate ignores
/// it, the same pattern `crt.rs` uses for its pure-logic helpers. Colors
/// come from the ported `crt::ResolvedColor`/`ColorScheme`; no color table
/// is re-implemented here. `pub(crate)` so `panel.rs` and
/// `screenmanager.rs` share the one shim (they draw the same way).
pub(crate) struct Ncurses;

impl Ncurses {
    /// htoprs extension: the border inset in cells (0 or 1). When the themed
    /// border is on, the usable area shrinks by `2*margin` and every positioned
    /// draw shifts in by `margin`, so htop lays its whole UI out *inside* the
    /// border instead of underneath it (a real inset, matching iftoprs's
    /// `margin`, not an overdraw). 0 by default → zero effect when off.
    fn margin() -> i32 {
        crate::extensions::overlay::border_margin() as i32
    }

    /// ncurses `LINES` — terminal row count (falls back to 24 with no TTY),
    /// less the border inset so panels lay out inside the frame.
    pub(crate) fn lines() -> i32 {
        let raw = terminal::size().map(|(_c, r)| r as i32).unwrap_or(24);
        (raw - 2 * Self::margin()).max(1)
    }

    /// ncurses `COLS` — terminal column count (falls back to 80 with no TTY),
    /// less the border inset.
    pub(crate) fn cols() -> i32 {
        let raw = terminal::size().map(|(c, _r)| c as i32).unwrap_or(80);
        (raw - 2 * Self::margin()).max(1)
    }

    /// Map an ncurses color number to a crossterm [`Color`]: `0..=7` are
    /// the eight ANSI colors, `8` is gray, `-1` is the terminal default
    /// (`Color::Reset`).
    ///
    /// When an htoprs theme is active, [`crate::extensions::colors::remap`]
    /// redirects the eight ANSI slots (`0..=8`) to 256-color palette values —
    /// this is the single choke point that recolors the whole UI per theme.
    fn to_color(n: i16) -> Color {
        if let Some(idx) = crate::extensions::colors::remap(n) {
            return Color::AnsiValue(idx);
        }
        match n {
            0 => Color::Black,
            1 => Color::DarkRed,
            2 => Color::DarkGreen,
            3 => Color::DarkYellow,
            4 => Color::DarkBlue,
            5 => Color::DarkMagenta,
            6 => Color::DarkCyan,
            7 => Color::Grey,
            8 => Color::DarkGrey,
            _ => Color::Reset,
        }
    }

    /// ncurses `attrset(CRT_colors[...])`: resolve the packed attribute via
    /// [`ResolvedColor::from_attr`] and emit the fg/bg + `A_*` attributes.
    /// `Attribute::Reset` first clears any prior run, matching `attrset`
    /// replacing (not OR-ing) the current attribute.
    pub(crate) fn attrset<W: Write>(out: &mut W, attr: i32) {
        let rc = ResolvedColor::from_attr(attr, ColorScheme::active(), true);
        let _ = queue!(out, SetAttribute(Attribute::Reset));
        let _ = queue!(
            out,
            SetForegroundColor(Self::to_color(rc.fg)),
            SetBackgroundColor(Self::to_color(rc.bg))
        );
        if rc.attributes & A_BOLD != 0 {
            let _ = queue!(out, SetAttribute(Attribute::Bold));
        }
        if rc.attributes & A_DIM != 0 {
            let _ = queue!(out, SetAttribute(Attribute::Dim));
        }
        if rc.attributes & (A_REVERSE | A_STANDOUT) != 0 {
            let _ = queue!(out, SetAttribute(Attribute::Reverse));
        }
        if rc.attributes & A_UNDERLINE != 0 {
            let _ = queue!(out, SetAttribute(Attribute::Underlined));
        }
        if rc.attributes & A_BLINK != 0 {
            let _ = queue!(out, SetAttribute(Attribute::SlowBlink));
        }
    }

    /// ncurses `mvaddstr(y, x, s)`. Off-screen (negative) coordinates are
    /// dropped, matching ncurses' silent failure.
    pub(crate) fn mvaddstr<W: Write>(out: &mut W, y: i32, x: i32, s: &str) {
        if y < 0 || x < 0 {
            return;
        }
        let m = Self::margin();
        let _ = queue!(out, MoveTo((x + m) as u16, (y + m) as u16), Print(s));
    }

    /// ncurses `mvaddnstr(y, x, s, n)`: at most `n` bytes of `s`.
    pub(crate) fn mvaddnstr<W: Write>(out: &mut W, y: i32, x: i32, s: &str, n: i32) {
        if y < 0 || x < 0 || n <= 0 {
            return;
        }
        let end = (n as usize).min(s.len());
        let m = Self::margin();
        let _ = queue!(out, MoveTo((x + m) as u16, (y + m) as u16), Print(&s[..end]));
    }

    /// ncurses `mvaddch(y, x, ch)`.
    pub(crate) fn mvaddch<W: Write>(out: &mut W, y: i32, x: i32, ch: char) {
        if y < 0 || x < 0 {
            return;
        }
        let m = Self::margin();
        let _ = queue!(out, MoveTo((x + m) as u16, (y + m) as u16), Print(ch));
    }

    /// ncurses `mvhline(y, x, ch, n)`: `n` copies of `ch` from `(y, x)`.
    pub(crate) fn mvhline<W: Write>(out: &mut W, y: i32, x: i32, ch: char, n: i32) {
        if y < 0 || x < 0 || n <= 0 {
            return;
        }
        let run: String = std::iter::repeat_n(ch, n as usize).collect();
        let m = Self::margin();
        let _ = queue!(out, MoveTo((x + m) as u16, (y + m) as u16), Print(run));
    }

    /// ncurses `mvvline(y, x, ch, n)`: `n` copies of `ch` down from `(y, x)`.
    pub(crate) fn mvvline<W: Write>(out: &mut W, y: i32, x: i32, ch: char, n: i32) {
        if y < 0 || x < 0 || n <= 0 {
            return;
        }
        let m = Self::margin();
        for k in 0..n {
            let _ = queue!(out, MoveTo((x + m) as u16, (y + k + m) as u16), Print(ch));
        }
    }

    /// ncurses `move(y, x)`.
    pub(crate) fn move_to<W: Write>(out: &mut W, y: i32, x: i32) {
        if y < 0 || x < 0 {
            return;
        }
        let m = Self::margin();
        let _ = queue!(out, MoveTo((x + m) as u16, (y + m) as u16));
    }

    /// ncurses `napms(ms)`: sleep for `ms` milliseconds (used by `actionKill`
    /// for the brief "Sending..." pause). Negative values are a no-op.
    pub(crate) fn napms(ms: i32) {
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
        }
    }

    /// ncurses `curs_set(0/1)`.
    pub(crate) fn curs_set<W: Write>(out: &mut W, on: bool) {
        let _ = if on {
            queue!(out, Show)
        } else {
            queue!(out, Hide)
        };
    }

    /// ncurses `clear()` — erase the whole screen and home the cursor.
    pub(crate) fn clear<W: Write>(out: &mut W) {
        let _ = queue!(out, terminal::Clear(terminal::ClearType::All), MoveTo(0, 0));
    }

    /// ncurses `addstr()` — write the string at the current cursor position (in
    /// the current attribute), advancing the cursor. No `MoveTo`, so it
    /// continues from wherever the last `mvaddstr`/`addstr` left off.
    pub(crate) fn addstr<W: Write>(out: &mut W, s: &str) {
        let _ = queue!(out, Print(s));
    }

    /// ncurses `beep()` — the audible terminal bell (BEL, `\a`).
    pub(crate) fn beep<W: Write>(out: &mut W) {
        let _ = out.write_all(b"\x07");
    }

    /// ncurses `refresh()` — flush the queued output to the terminal.
    pub(crate) fn refresh<W: Write>(out: &mut W) {
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(keys: &[&str], functions: &[&str], events: &[i32]) -> FunctionBar {
        FunctionBar {
            functions: functions.iter().map(|s| s.to_string()).collect(),
            keys: keys.iter().map(|s| s.to_string()).collect(),
            events: events.to_vec(),
            staticData: false,
        }
    }

    #[test]
    fn new_static_data_uses_fkeys_and_fevents() {
        // No keys/events -> static F1..F10 tables (staticData = true).
        let b = FunctionBar_new(None, None, None);
        assert!(b.staticData);
        assert_eq!(b.functions.len(), 10);
        assert_eq!(b.functions[0], "      "); // blank FLabels
        assert_eq!(b.keys[0], "F1");
        assert_eq!(b.keys[9], "F10");
        assert_eq!(b.events[0], KEY_F(1));
        assert_eq!(b.events[9], KEY_F(10));
    }

    #[test]
    fn new_with_keys_events_is_dynamic() {
        let funcs = ["Help", "Quit"];
        let keys = ["F1", "F10"];
        let events = [1, 2];
        let b = FunctionBar_new(Some(&funcs), Some(&keys), Some(&events));
        assert!(!b.staticData);
        assert_eq!(b.functions, vec!["Help".to_string(), "Quit".to_string()]);
        assert_eq!(b.keys, vec!["F1".to_string(), "F10".to_string()]);
        assert_eq!(b.events, vec![1, 2]);
    }

    #[test]
    fn new_enter_esc_builds_two_slots() {
        let b = FunctionBar_newEnterEsc("Done  ", "Cancel");
        assert!(!b.staticData);
        assert_eq!(
            b.functions,
            vec!["Done  ".to_string(), "Cancel".to_string()]
        );
        assert_eq!(b.keys, vec!["Enter".to_string(), "Esc".to_string()]);
        assert_eq!(b.events, vec![13, 27]);
    }

    #[test]
    fn setlabel_replaces_matching_event() {
        let mut b = bar(&["F1", "F2", "F3"], &["a", "b", "c"], &[1, 2, 3]);
        FunctionBar_setLabel(&mut b, 2, "xyz");
        assert_eq!(
            b.functions,
            vec!["a".to_string(), "xyz".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn setlabel_no_match_is_noop() {
        let mut b = bar(&["F1", "F2"], &["a", "b"], &[1, 2]);
        FunctionBar_setLabel(&mut b, 99, "xyz");
        assert_eq!(b.functions, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn setlabel_stops_at_first_match() {
        let mut b = bar(&["F1", "F2"], &["a", "b"], &[5, 5]);
        FunctionBar_setLabel(&mut b, 5, "z");
        assert_eq!(b.functions, vec!["z".to_string(), "b".to_string()]);
    }

    #[test]
    fn getwidth_sums_key_and_function_bytes() {
        // "F1"(2)+"  "(2) + "F2"(2)+"abc"(3) = 9
        let b = bar(&["F1", "F2"], &["  ", "abc"], &[1, 2]);
        assert_eq!(FunctionBar_getWidth(&b), 9);
    }

    #[test]
    fn getwidth_empty_is_zero() {
        let b = bar(&[], &[], &[]);
        assert_eq!(FunctionBar_getWidth(&b), 0);
    }

    #[test]
    fn synthesize_event_boundaries() {
        // widths: after slot0 x=2+4=6, after slot1 x=6+2+4=12
        let b = bar(&["F1", "F2"], &["Help", "Quit"], &[10, 20]);
        assert_eq!(FunctionBar_synthesizeEvent(&b, 0), 10);
        assert_eq!(FunctionBar_synthesizeEvent(&b, 5), 10);
        assert_eq!(FunctionBar_synthesizeEvent(&b, 6), 20);
        assert_eq!(FunctionBar_synthesizeEvent(&b, 11), 20);
        assert_eq!(FunctionBar_synthesizeEvent(&b, 12), ERR);
        assert_eq!(FunctionBar_synthesizeEvent(&b, 100), -1);
    }

    #[test]
    fn synthesize_event_negative_pos_hits_first_slot() {
        let b = bar(&["F1"], &["Help"], &[10]);
        assert_eq!(FunctionBar_synthesizeEvent(&b, -1), 10);
    }

    #[test]
    fn extra_layout_without_buffer_matches_getwidth() {
        let b = bar(&["F1", "F2"], &["Help", "Quit"], &[10, 20]);
        let (cursor_x, end_x) = b.extra_layout(None);
        // No buffer: cursorX == endX == getWidth == 12.
        assert_eq!(cursor_x, FunctionBar_getWidth(&b));
        assert_eq!(end_x, 12);
        assert_eq!(cursor_x, 12);
    }

    #[test]
    fn extra_layout_with_buffer_advances_by_buffer_len() {
        let b = bar(&["F1"], &["Help"], &[10]); // width 6
        let (cursor_x, end_x) = b.extra_layout(Some("query"));
        // cursorX and endX both include the 5-byte buffer -> 11.
        assert_eq!(end_x, 11);
        assert_eq!(cursor_x, 11);
    }

    #[test]
    fn to_color_maps_ansi_and_default() {
        assert_eq!(Ncurses::to_color(-1), Color::Reset);
        assert_eq!(Ncurses::to_color(0), Color::Black);
        assert_eq!(Ncurses::to_color(1), Color::DarkRed);
        assert_eq!(Ncurses::to_color(7), Color::Grey);
        assert_eq!(Ncurses::to_color(8), Color::DarkGrey);
    }
}
