//! Themed keyboard-help overlay, theme chooser, and theme editor.
//!
//! Ported from iftoprs (`src/ui/render.rs` + `src/main.rs` + `src/ui/app.rs`),
//! which in turn took the storageshower-style modal design. The overlays draw
//! into a [`ratatui::buffer::Buffer`]; unlike iftoprs (a full ratatui app),
//! htoprs draws on crossterm, so this is a standalone extension module: the
//! render functions, the [`OverlayState`] container, and [`OverlayState::handle_key`]
//! reproduce the iftoprs behavior 1:1 but are not yet wired into htoprs's live
//! draw loop.
//!
//! [`super::theme::Theme`] stores `crossterm::style::Color` values; ratatui
//! styles want `ratatui::style::Color`, so [`tr`] converts at the boundary —
//! the same adaptation the theme port made for `Color::Indexed` → `AnsiValue`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;

use crossterm::cursor::MoveTo;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::{
    Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::{queue, terminal};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::theme::{CustomThemeColors, Theme, ThemeName};

// ncurses key constants, mirrored from `crate::ported::crt` so this module
// stays self-contained (and testable in isolation). Values are octal exactly
// as in `crt.rs`.
const KEY_DOWN: i32 = 0o402;
const KEY_UP: i32 = 0o403;
const KEY_LEFT: i32 = 0o404;
const KEY_RIGHT: i32 = 0o405;
const KEY_BACKSPACE: i32 = 0o407;
const KEY_ENTER: i32 = 0o527;
const KEY_F1: i32 = 0o411; // KEY_F(1) == 265; htop's F1 = Help

/// Convert a [`Theme`] color (`crossterm::style::Color`) into the
/// `ratatui::style::Color` the buffer styling API expects. `Theme` only ever
/// produces `AnsiValue` and `Black` (see `Theme::from_palette_raw`); the other
/// arms cover the full crossterm enum for robustness.
fn tr(c: crossterm::style::Color) -> Color {
    use crossterm::style::Color as C;
    match c {
        C::Reset => Color::Reset,
        C::Black => Color::Black,
        C::DarkGrey => Color::DarkGray,
        C::Red => Color::Red,
        C::DarkRed => Color::LightRed,
        C::Green => Color::Green,
        C::DarkGreen => Color::LightGreen,
        C::Yellow => Color::Yellow,
        C::DarkYellow => Color::LightYellow,
        C::Blue => Color::Blue,
        C::DarkBlue => Color::LightBlue,
        C::Magenta => Color::Magenta,
        C::DarkMagenta => Color::LightMagenta,
        C::Cyan => Color::Cyan,
        C::DarkCyan => Color::LightCyan,
        C::White => Color::White,
        C::Grey => Color::Gray,
        C::Rgb { r, g, b } => Color::Rgb(r, g, b),
        C::AnsiValue(n) => Color::Indexed(n),
    }
}

// ─── Theme chooser state (iftoprs app.rs ThemeChooser) ─────────────────────────

/// Theme chooser popup state.
pub struct ThemeChooser {
    /// Whether the chooser is open.
    pub active: bool,
    /// Index into [`ThemeName::ALL`] of the highlighted row.
    pub selected: usize,
}

impl Default for ThemeChooser {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeChooser {
    /// A closed chooser positioned at the first theme.
    pub fn new() -> Self {
        Self {
            active: false,
            selected: 0,
        }
    }
    /// Open the chooser with the row for `current` pre-selected.
    pub fn open(&mut self, current: ThemeName) {
        self.active = true;
        self.selected = ThemeName::ALL
            .iter()
            .position(|&t| t == current)
            .unwrap_or(0);
    }
}

// ─── Theme editor state (iftoprs app.rs ThemeEditState) ────────────────────────

/// Theme editor popup state.
#[derive(Default)]
pub struct ThemeEditState {
    /// Whether the editor is open.
    pub active: bool,
    /// The six palette channels being edited.
    pub colors: [u8; 6],
    /// The channel row (0..=5) currently selected.
    pub slot: usize,
    /// Whether the name-entry sub-prompt is showing.
    pub naming: bool,
    /// The custom-theme name being typed.
    pub name: String,
    /// Byte cursor into `name`.
    pub cursor: usize,
}

impl ThemeEditState {
    /// A closed editor.
    pub fn new() -> Self {
        Self::default()
    }
    /// Open the editor seeded with `current_palette`.
    pub fn open(&mut self, current_palette: [u8; 6]) {
        self.active = true;
        self.colors = current_palette;
        self.slot = 0;
        self.naming = false;
        self.name.clear();
        self.cursor = 0;
    }
}

// ─── Overlay subsystem state ───────────────────────────────────────────────────

/// The theme/help subsystem state extracted from iftoprs's `AppState`. Holds
/// only the fields the overlays and their key handling touch; the rest of
/// iftoprs's `AppState` (flows, interfaces, capture toggles) has no htoprs
/// analog and is intentionally omitted.
pub struct OverlayState {
    /// Whether the help overlay is showing.
    pub show_help: bool,
    /// Whether the UI border is shown (`x` toggles).
    pub show_border: bool,
    /// Whether the header is shown (`g` toggles).
    pub show_header: bool,
    /// The active built-in theme name.
    pub theme_name: ThemeName,
    /// The resolved colors for `theme_name` (or the live custom palette).
    pub theme: Theme,
    /// Theme chooser popup state.
    pub theme_chooser: ThemeChooser,
    /// Theme editor popup state.
    pub theme_edit: ThemeEditState,
    /// Saved custom palettes, keyed by user-chosen name.
    pub custom_themes: HashMap<String, CustomThemeColors>,
    /// The name of the applied custom theme, if any.
    pub active_custom_theme: Option<String>,
    /// Last status line set by a toggle/save (iftoprs `set_status` analog).
    pub status: Option<String>,
    /// True once the user has engaged the theme UI (`c`/`C`). While set, the
    /// live palette is pushed to [`super::colors`] so the htoprs UI recolors.
    pub themed: bool,
    /// Set when a selection/save should be persisted to the prefs file; the
    /// I/O happens in [`dispatch_key`] (kept out of `handle_key` so that stays
    /// pure and testable).
    pub dirty: bool,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayState {
    /// Fresh state on the default theme with border and header shown.
    pub fn new() -> Self {
        let theme_name = ThemeName::default();
        Self {
            show_help: false,
            // Off by default so htoprs looks like htop until `b`/`g` are pressed.
            show_border: false,
            show_header: false,
            theme_name,
            theme: Theme::from_name(theme_name),
            theme_chooser: ThemeChooser::new(),
            theme_edit: ThemeEditState::new(),
            custom_themes: HashMap::new(),
            active_custom_theme: None,
            status: None,
            themed: false,
            dirty: false,
        }
    }

    /// The palette currently driving the theme: the active custom palette if
    /// one is applied, otherwise the built-in theme's palette.
    pub fn current_palette(&self) -> [u8; 6] {
        self.active_custom_theme
            .as_ref()
            .and_then(|name| self.custom_themes.get(name))
            .map(|ct| [ct.c1, ct.c2, ct.c3, ct.c4, ct.c5, ct.c6])
            .unwrap_or_else(|| Theme::palette_values(self.theme_name))
    }

    /// Switch to a built-in theme, clearing any active custom palette.
    /// Port of `AppState::set_theme` (iftoprs app.rs).
    pub fn set_theme(&mut self, name: ThemeName) {
        self.theme_name = name;
        self.theme = Theme::from_name(name);
        self.active_custom_theme = None;
    }

    /// Re-derive the live theme from a raw six-channel palette (editor preview).
    /// Port of `AppState::apply_custom_palette` (iftoprs app.rs).
    pub fn apply_custom_palette(&mut self, colors: [u8; 6]) {
        self.theme = Theme::from_palette_raw(
            colors[0], colors[1], colors[2], colors[3], colors[4], colors[5],
        );
    }

    /// Set the transient status line. Port of `AppState::set_status`.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some(msg.into());
    }

    /// Dispatch a key while an overlay is active or for the overlay hotkeys.
    /// Returns `true` if the key was consumed. Ports the chooser/editor/toggle
    /// arms of iftoprs's `main.rs` event loop (the flow/interface/capture arms
    /// are omitted — they have no htoprs analog). `save_prefs()` calls become
    /// no-ops here because this standalone module owns no prefs file.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Theme chooser mode.
        if self.theme_chooser.active {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    let len = ThemeName::ALL.len();
                    self.theme_chooser.selected = (self.theme_chooser.selected + 1) % len;
                    let name = ThemeName::ALL[self.theme_chooser.selected];
                    self.set_theme(name);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    let len = ThemeName::ALL.len();
                    self.theme_chooser.selected = (self.theme_chooser.selected + len - 1) % len;
                    let name = ThemeName::ALL[self.theme_chooser.selected];
                    self.set_theme(name);
                }
                KeyCode::Enter => {
                    self.theme_chooser.active = false;
                    self.dirty = true; // persist the chosen theme
                }
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('c') => {
                    self.theme_chooser.active = false;
                }
                _ => {}
            }
            return true;
        }

        // Theme editor mode.
        if self.theme_edit.active {
            if self.theme_edit.naming {
                match key.code {
                    KeyCode::Enter => {
                        let name = self.theme_edit.name.trim().to_string();
                        if !name.is_empty() {
                            let c = self.theme_edit.colors;
                            self.custom_themes.insert(
                                name.clone(),
                                CustomThemeColors {
                                    c1: c[0],
                                    c2: c[1],
                                    c3: c[2],
                                    c4: c[3],
                                    c5: c[4],
                                    c6: c[5],
                                },
                            );
                            self.active_custom_theme = Some(name.clone());
                            self.set_status(format!("Saved theme: {}", name));
                            self.dirty = true; // persist the new custom theme
                        }
                        self.theme_edit.active = false;
                        self.theme_edit.naming = false;
                        self.theme_edit.name.clear();
                        self.theme_edit.cursor = 0;
                    }
                    KeyCode::Esc => {
                        self.theme_edit.naming = false;
                        self.theme_edit.name.clear();
                        self.theme_edit.cursor = 0;
                    }
                    KeyCode::Backspace if self.theme_edit.cursor > 0 => {
                        self.theme_edit.cursor -= 1;
                        self.theme_edit.name.remove(self.theme_edit.cursor);
                    }
                    KeyCode::Left => {
                        self.theme_edit.cursor = self.theme_edit.cursor.saturating_sub(1);
                    }
                    KeyCode::Right => {
                        self.theme_edit.cursor =
                            (self.theme_edit.cursor + 1).min(self.theme_edit.name.len());
                    }
                    KeyCode::Char(c) if self.theme_edit.name.len() < 20 => {
                        self.theme_edit.name.insert(self.theme_edit.cursor, c);
                        self.theme_edit.cursor += 1;
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        self.theme_edit.active = false;
                        // Restore the theme as it was before editing.
                        let restore = self
                            .active_custom_theme
                            .as_ref()
                            .and_then(|name| self.custom_themes.get(name))
                            .map(|ct| [ct.c1, ct.c2, ct.c3, ct.c4, ct.c5, ct.c6]);
                        match restore {
                            Some(colors) => self.apply_custom_palette(colors),
                            None => self.set_theme(self.theme_name),
                        }
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.theme_edit.slot = (self.theme_edit.slot + 1).min(5);
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.theme_edit.slot = self.theme_edit.slot.saturating_sub(1);
                    }
                    KeyCode::Char('l') | KeyCode::Right => {
                        self.theme_edit.colors[self.theme_edit.slot] =
                            self.theme_edit.colors[self.theme_edit.slot].wrapping_add(1);
                        self.apply_custom_palette(self.theme_edit.colors);
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        self.theme_edit.colors[self.theme_edit.slot] =
                            self.theme_edit.colors[self.theme_edit.slot].wrapping_sub(1);
                        self.apply_custom_palette(self.theme_edit.colors);
                    }
                    KeyCode::Char('L') => {
                        self.theme_edit.colors[self.theme_edit.slot] =
                            self.theme_edit.colors[self.theme_edit.slot].wrapping_add(10);
                        self.apply_custom_palette(self.theme_edit.colors);
                    }
                    KeyCode::Char('H') => {
                        self.theme_edit.colors[self.theme_edit.slot] =
                            self.theme_edit.colors[self.theme_edit.slot].wrapping_sub(10);
                        self.apply_custom_palette(self.theme_edit.colors);
                    }
                    KeyCode::Enter | KeyCode::Char('s') | KeyCode::Char('S') => {
                        self.theme_edit.naming = true;
                        self.theme_edit.name.clear();
                        self.theme_edit.cursor = 0;
                    }
                    _ => {}
                }
            }
            return true;
        }

        // Top-level overlay hotkeys (iftoprs main.rs, theme-relevant subset).
        match key.code {
            KeyCode::Char('h') | KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Char('c') => {
                self.show_help = false;
                self.themed = true;
                self.theme_chooser.open(self.theme_name);
            }
            KeyCode::Char('C') => {
                self.show_help = false;
                self.themed = true;
                let palette = self
                    .active_custom_theme
                    .as_ref()
                    .and_then(|name| self.custom_themes.get(name))
                    .map(|ct| [ct.c1, ct.c2, ct.c3, ct.c4, ct.c5, ct.c6])
                    .unwrap_or_else(|| Theme::palette_values(self.theme_name));
                self.theme_edit.open(palette);
            }
            // Border toggle is on `b` (not `x`): htop binds `x` to the
            // file-locks screen (`action.rs` actionShowLocks), and the overlay
            // intercepts keys before htop's handler, so using `x` here shadowed
            // that feature. `b` (border) is free in htop's key map.
            KeyCode::Char('b') => {
                self.show_border = !self.show_border;
                self.set_status(if self.show_border {
                    "Border: on"
                } else {
                    "Border: off"
                });
            }
            KeyCode::Char('g') => {
                self.show_header = !self.show_header;
                self.set_status(if self.show_header {
                    "Header: on"
                } else {
                    "Header: off"
                });
            }
            _ => return false,
        }
        true
    }

    /// Draw whichever overlay is currently active into `buf`. Mirrors the
    /// overlay dispatch block of iftoprs `render.rs`.
    pub fn render(&self, buf: &mut Buffer, area: Rect) {
        if self.show_help {
            draw_help(buf, area, self);
        }
        if self.theme_chooser.active {
            draw_theme_chooser(buf, area, self);
        }
        if self.theme_edit.active {
            draw_theme_editor(buf, area, self);
        }
    }

    /// Route a raw ncurses key int (as read by `Panel_getCh`) into
    /// [`OverlayState::handle_key`]. Returns `true` if consumed.
    pub fn handle_ncurses_key(&mut self, ch: i32) -> bool {
        // htop's F1 = Help, but the ported `actionHelp` is an unfinished
        // `todo!()` that panics; route F1 to the themed help overlay instead.
        if ch == KEY_F1 {
            return self.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        }
        match ncurses_to_keycode(ch) {
            Some(code) => self.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            None => false,
        }
    }

    /// Whether any overlay (help/chooser/editor) is currently visible.
    pub fn any_active(&self) -> bool {
        self.show_help || self.theme_chooser.active || self.theme_edit.active
    }
}

/// Map a raw ncurses key int to the crossterm [`KeyCode`] the overlay handlers
/// expect. Printable ASCII becomes `Char`; the rest map from the `KEY_*`
/// constants (mirrored from `crt.rs`).
fn ncurses_to_keycode(ch: i32) -> Option<KeyCode> {
    match ch {
        KEY_UP => Some(KeyCode::Up),
        KEY_DOWN => Some(KeyCode::Down),
        KEY_LEFT => Some(KeyCode::Left),
        KEY_RIGHT => Some(KeyCode::Right),
        KEY_ENTER | 10 | 13 => Some(KeyCode::Enter),
        27 => Some(KeyCode::Esc),
        KEY_BACKSPACE | 127 | 8 => Some(KeyCode::Backspace),
        c if (32..=126).contains(&c) => Some(KeyCode::Char(c as u8 as char)),
        _ => None,
    }
}

thread_local! {
    /// The live overlay state for the running TUI. Thread-local because the TUI
    /// draws and reads keys on a single thread (`ScreenManager_run`).
    static OVERLAY: RefCell<OverlayState> = RefCell::new(OverlayState::new());
}

/// True if any overlay is currently visible — the run loop uses this to keep
/// the panels frozen and to know whether a redraw must repaint the overlay.
pub fn overlay_active() -> bool {
    OVERLAY.with(|o| o.borrow().any_active())
}

/// Route an ncurses key int into the live overlay. Returns `true` if the
/// overlay consumed it (an overlay hotkey, or any key while an overlay is
/// open). On a theme change, pushes the live palette to [`super::colors`] so
/// the htoprs UI recolors immediately.
pub fn dispatch_key(ch: i32) -> bool {
    OVERLAY.with(|o| {
        let mut s = o.borrow_mut();
        let consumed = s.handle_ncurses_key(ch);
        if consumed && s.themed {
            super::colors::apply_palette(s.current_palette());
        }
        if s.dirty {
            s.dirty = false;
            super::prefs::save(&super::prefs::Prefs {
                theme: s.theme_name,
                active_custom_theme: s.active_custom_theme.clone(),
                custom_themes: s.custom_themes.clone(),
            });
        }
        consumed
    })
}

/// Load the saved theme prefs (if any) into the live overlay state and apply
/// the colors, so a previously-chosen theme is active from the first frame.
/// A no-op when no prefs file exists. Called once at TUI startup, before the
/// first panel draw.
pub fn init_from_prefs() {
    let Some(p) = super::prefs::load() else {
        return;
    };
    OVERLAY.with(|o| {
        let mut s = o.borrow_mut();
        s.theme_name = p.theme;
        s.custom_themes = p.custom_themes;
        s.active_custom_theme = p.active_custom_theme;
        s.themed = true;
        let palette = s.current_palette();
        s.theme = Theme::from_palette_raw(
            palette[0], palette[1], palette[2], palette[3], palette[4], palette[5],
        );
        super::colors::apply_palette(palette);
    });
}

/// Draw the active overlay (if any) over the current screen, then flush.
/// A no-op when no overlay is visible. Called from the run loop right after
/// the panels are drawn.
pub fn draw_active<W: Write>(out: &mut W) {
    let (cols, rows) = terminal::size().unwrap_or((80, 24));
    if cols == 0 || rows == 0 {
        return;
    }
    OVERLAY.with(|o| {
        let s = o.borrow();
        if !s.any_active() {
            return;
        }
        let area = Rect::new(0, 0, cols, rows);
        let mut b = Buffer::empty(area);
        s.render(&mut b, area);
        blit(out, &b);
    });
    let _ = out.flush();
}

/// Draw the themed border chrome (ported from iftoprs `draw`): a box around
/// the whole screen with a centered title, in the active theme's `scale_line`
/// color, when `show_border` is set. Drawn after the panels and before the
/// modal overlays. A no-op when the border is off.
///
/// Unlike iftoprs (which insets its content by a 1-cell margin), htoprs's htop
/// panels fill the screen, so the border overdraws the outermost row/column of
/// panel content — insetting htop's layout is separate, deeper work.
pub fn draw_chrome<W: Write>(out: &mut W) {
    let (cols, rows) = terminal::size().unwrap_or((80, 24));
    if cols < 2 || rows < 2 {
        return;
    }
    OVERLAY.with(|o| {
        let s = o.borrow();
        if !s.show_border {
            return;
        }
        // `Theme` colors are already `crossterm::style::Color`.
        let bc = s.theme.scale_line;
        let x1 = cols - 1;
        let y1 = rows - 1;
        let _ = queue!(out, SetAttribute(Attribute::Reset), SetForegroundColor(bc));
        let _ = queue!(out, MoveTo(0, 0), Print("┌"));
        let _ = queue!(out, MoveTo(x1, 0), Print("┐"));
        let _ = queue!(out, MoveTo(0, y1), Print("└"));
        let _ = queue!(out, MoveTo(x1, y1), Print("┘"));
        for x in 1..x1 {
            let _ = queue!(out, MoveTo(x, 0), Print("─"));
            let _ = queue!(out, MoveTo(x, y1), Print("─"));
        }
        for y in 1..y1 {
            let _ = queue!(out, MoveTo(0, y), Print("│"));
            let _ = queue!(out, MoveTo(x1, y), Print("│"));
        }
        // Centered title in the top border.
        let ver = env!("CARGO_PKG_VERSION");
        let title = format!(" ▶▶▶ HTOPRS v{} ◀◀◀ ", ver);
        let title_cw = title.chars().count() as u16;
        if title_cw < cols {
            let tx = (cols - title_cw) / 2;
            let _ = queue!(out, MoveTo(tx, 0), SetAttribute(Attribute::Bold), Print(&title));
        }
        let _ = queue!(out, SetAttribute(Attribute::Reset), ResetColor);
    });
    let _ = out.flush();
}

/// Convert a ratatui [`Color`] to its `crossterm::style::Color` equivalent
/// (inverse of [`tr`]). `Indexed` → `AnsiValue` preserves 256-color themes.
fn ct(c: Color) -> crossterm::style::Color {
    use crossterm::style::Color as X;
    match c {
        Color::Reset => X::Reset,
        Color::Black => X::Black,
        Color::Red => X::DarkRed,
        Color::Green => X::DarkGreen,
        Color::Yellow => X::DarkYellow,
        Color::Blue => X::DarkBlue,
        Color::Magenta => X::DarkMagenta,
        Color::Cyan => X::DarkCyan,
        Color::Gray => X::Grey,
        Color::DarkGray => X::DarkGrey,
        Color::LightRed => X::Red,
        Color::LightGreen => X::Green,
        Color::LightYellow => X::Yellow,
        Color::LightBlue => X::Blue,
        Color::LightMagenta => X::Magenta,
        Color::LightCyan => X::Cyan,
        Color::White => X::White,
        Color::Indexed(n) => X::AnsiValue(n),
        Color::Rgb(r, g, b) => X::Rgb { r, g, b },
    }
}

/// Blit only the cells an overlay actually painted onto `out` via crossterm.
/// Untouched backdrop cells (blank, default style) are skipped so the panels
/// drawn underneath show through around the modal.
fn blit<W: Write>(out: &mut W, buf: &Buffer) {
    let area = buf.area();
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            if cell.symbol() == " "
                && cell.fg == Color::Reset
                && cell.bg == Color::Reset
                && cell.modifier.is_empty()
            {
                continue;
            }
            let _ = queue!(
                out,
                MoveTo(x, y),
                SetAttribute(Attribute::Reset),
                SetForegroundColor(ct(cell.fg)),
                SetBackgroundColor(ct(cell.bg))
            );
            if cell.modifier.contains(Modifier::BOLD) {
                let _ = queue!(out, SetAttribute(Attribute::Bold));
            }
            if cell.modifier.contains(Modifier::DIM) {
                let _ = queue!(out, SetAttribute(Attribute::Dim));
            }
            if cell.modifier.contains(Modifier::REVERSED) {
                let _ = queue!(out, SetAttribute(Attribute::Reverse));
            }
            if cell.modifier.contains(Modifier::UNDERLINED) {
                let _ = queue!(out, SetAttribute(Attribute::Underlined));
            }
            let _ = queue!(out, Print(cell.symbol()));
        }
    }
    let _ = queue!(out, SetAttribute(Attribute::Reset), ResetColor);
}

// ─── Buffer helpers (iftoprs render.rs) ────────────────────────────────────────

fn set_cell(buf: &mut Buffer, x: u16, y: u16, ch: &str, s: Style) {
    let a = buf.area();
    if x < a.x + a.width && y < a.y + a.height {
        let c = &mut buf[(x, y)];
        c.set_symbol(ch);
        c.set_style(s);
    }
}

fn set_str(buf: &mut Buffer, x: u16, y: u16, s: &str, st: Style, mw: u16) {
    let aw = buf.area().x + buf.area().width;
    let ah = buf.area().y + buf.area().height;
    if y >= ah {
        return;
    }
    let mut char_buf = [0u8; 4];
    for (i, ch) in s.chars().enumerate() {
        let cx = x + i as u16;
        if cx >= x + mw || cx >= aw {
            break;
        }
        let c = &mut buf[(cx, y)];
        c.set_symbol(ch.encode_utf8(&mut char_buf));
        c.set_style(st);
    }
}

/// Draw a filled box with a double-line border. Returns the top-left `(x0, y0)`
/// of the box, centered in `area`.
fn draw_box(buf: &mut Buffer, area: Rect, bw: u16, bh: u16, bg: Color, border_style: Style) -> (u16, u16) {
    let x0 = (area.width.saturating_sub(bw)) / 2;
    let y0 = (area.height.saturating_sub(bh)) / 2;
    let x1 = x0 + bw - 1;
    let y1 = y0 + bh - 1;
    let fill = Style::default().bg(bg);
    for y in y0..y0 + bh {
        for x in x0..x0 + bw {
            set_cell(buf, x, y, " ", fill);
        }
    }
    set_cell(buf, x0, y0, "╔", border_style);
    set_cell(buf, x1, y0, "╗", border_style);
    set_cell(buf, x0, y1, "╚", border_style);
    set_cell(buf, x1, y1, "╝", border_style);
    for x in x0 + 1..x1 {
        set_cell(buf, x, y0, "═", border_style);
        set_cell(buf, x, y1, "═", border_style);
    }
    for y in y0 + 1..y1 {
        set_cell(buf, x0, y, "║", border_style);
        set_cell(buf, x1, y, "║", border_style);
    }
    (x0, y0)
}

// ─── Help modal (storageshower-style, htoprs keys) ─────────────────────────────

/// Draw the keyboard-shortcuts help overlay. Ported from iftoprs `draw_help`;
/// the section/key content is populated with htoprs (htop) bindings — the
/// function-bar keys (`F1`..`F10`) and the theme-extension keys (`c`/`C`/`x`/`g`)
/// — rather than iftop's network-flow keys.
pub fn draw_help(buf: &mut Buffer, area: Rect, state: &OverlayState) {
    let t = &state.theme;
    let bw = 90u16.min(area.width.saturating_sub(4));
    let bh = 31u16.min(area.height.saturating_sub(4));
    let bg = tr(t.help_bg);
    let bs = Style::default().fg(tr(t.help_border));
    let bgs = Style::default().fg(Color::White).bg(bg);
    let ks = Style::default().fg(tr(t.help_key)).bg(bg);
    let ts = Style::default()
        .fg(tr(t.help_title))
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let ss = Style::default()
        .fg(tr(t.help_section))
        .bg(bg)
        .add_modifier(Modifier::BOLD);

    let (x0, y0) = draw_box(buf, area, bw, bh, bg, bs);

    let ver = env!("CARGO_PKG_VERSION");
    let title = format!("⌨ HTOPRS v{} — KEYBOARD SHORTCUTS", ver);
    let title_cw = title.chars().count() as u16;
    set_str(
        buf,
        x0 + (bw.saturating_sub(title_cw)) / 2,
        y0 + 1,
        &title,
        ts,
        bw - 2,
    );
    let subtitle = "interactive process viewer";
    set_str(
        buf,
        x0 + (bw.saturating_sub(subtitle.len() as u16)) / 2,
        y0 + 2,
        subtitle,
        Style::default().fg(Color::Indexed(240)).bg(bg),
        bw - 2,
    );

    let entries: [(&str, &[(&str, &str)]); 7] = [
        (
            "GENERAL",
            &[("F1 h ?", "Help"), ("F2 S", "Setup"), ("F10 q", "Quit")],
        ),
        (
            "SEARCH",
            &[("F3 /", "Search"), ("F4 \\", "Filter"), ("Esc", "Clear")],
        ),
        (
            "SORT",
            &[
                ("F6 >", "Sort by"),
                ("<", "Sort column"),
                ("I", "Invert order"),
                ("P", "Sort CPU%"),
                ("M", "Sort MEM%"),
                ("T", "Sort TIME+"),
            ],
        ),
        (
            "NAV",
            &[
                ("k ↑", "Move up"),
                ("j ↓", "Move down"),
                ("^U", "Half page up"),
                ("^D", "Half page down"),
                ("Home", "Jump to top"),
                ("End", "Jump to end"),
            ],
        ),
        (
            "PROCESS",
            &[
                ("F9", "Kill"),
                ("F7", "Nice -"),
                ("F8", "Nice +"),
                ("Space", "Tag"),
                ("U", "Untag all"),
                ("F5 t", "Tree view"),
            ],
        ),
        (
            "THEME",
            &[
                ("c", "Theme chooser"),
                ("C", "Theme editor"),
                ("b", "Toggle border"),
                ("g", "Toggle header"),
                ("h ?", "Toggle help"),
                ("q", "Quit"),
            ],
        ),
        ("", &[]),
    ];

    let cw = ((bw as usize).saturating_sub(4)) / 3;
    let mut col = 0usize;
    let mut row = 0usize;
    for (section, keys) in &entries {
        if section.is_empty() {
            continue;
        }
        // `bh as usize - 6` underflows (panics in debug) when the terminal is
        // shorter than 6 rows — a latent bug carried over from the iftoprs
        // original. `saturating_sub` keeps the column-break logic intact while
        // rendering harmlessly (all sections wrap immediately) in a tiny area.
        if row + keys.len() + 2 > (bh as usize).saturating_sub(6) {
            col += 1;
            row = 0;
            if col >= 3 {
                break;
            }
        }
        let cx = x0 + 2 + (col as u16) * cw as u16;
        let sy = y0 + 5 + row as u16;
        set_str(buf, cx, sy, section, ss, cw as u16);
        row += 1;
        for &(k, d) in *keys {
            let ey = y0 + 5 + row as u16;
            if ey >= y0 + bh - 2 {
                break;
            }
            set_str(buf, cx, ey, k, ks, 8);
            set_str(buf, cx + 9, ey, d, bgs, 18);
            row += 1;
        }
        row += 1;
    }

    let tl = format!("theme: {} | c=chooser", state.theme_name.display_name());
    set_str(
        buf,
        x0 + (bw.saturating_sub(tl.len() as u16)) / 2,
        y0 + bh - 3,
        &tl,
        Style::default().fg(tr(t.help_val)).bg(bg),
        bw - 4,
    );
    set_str(
        buf,
        x0 + (bw.saturating_sub(16)) / 2,
        y0 + bh - 2,
        "press h to close",
        Style::default().fg(Color::Indexed(240)).bg(bg),
        bw - 4,
    );
}

// ─── Theme chooser (iftoprs draw_theme_chooser) ────────────────────────────────

/// Draw the theme chooser popup: every [`ThemeName`] with its 6-cell swatch,
/// highlighting the selected row and marking the active theme.
pub fn draw_theme_chooser(buf: &mut Buffer, area: Rect, state: &OverlayState) {
    let t = &state.theme;
    let ch = &state.theme_chooser;
    let bw = 50u16.min(area.width.saturating_sub(4));
    let bh = (ThemeName::ALL.len() as u16 + 6).min(area.height.saturating_sub(4));
    let bg = tr(t.help_bg);
    let bs = Style::default().fg(tr(t.help_border));
    let ts = Style::default()
        .fg(tr(t.help_title))
        .bg(bg)
        .add_modifier(Modifier::BOLD);

    let (x0, y0) = draw_box(buf, area, bw, bh, bg, bs);
    set_str(buf, x0 + 2, y0 + 1, "THEME CHOOSER", ts, bw - 4);

    let help_key = tr(t.help_key);
    for (i, &tn) in ThemeName::ALL.iter().enumerate() {
        let ey = y0 + 3 + i as u16;
        if ey >= y0 + bh - 2 {
            break;
        }
        let sel = i == ch.selected;
        let act = tn == state.theme_name;
        let mk = if act { "▸ " } else { "  " };
        let rs = if sel {
            Style::default().fg(Color::Black).bg(help_key)
        } else {
            Style::default().fg(Color::White).bg(bg)
        };
        set_str(
            buf,
            x0 + 2,
            ey,
            &format!("{}{:<20}", mk, tn.display_name()),
            rs,
            24,
        );
        let swatch = Theme::swatch(tn);
        let sx = x0 + 26;
        for (si, (color, block)) in swatch.iter().enumerate() {
            let ss = if sel {
                Style::default().fg(tr(*color)).bg(help_key)
            } else {
                Style::default().fg(tr(*color)).bg(bg)
            };
            set_str(buf, sx + (si as u16) * 2, ey, block, ss, 2);
        }
    }

    let ft = "j/k:nav  Enter:select  Esc:cancel";
    set_str(
        buf,
        x0 + (bw.saturating_sub(ft.len() as u16)) / 2,
        y0 + bh - 2,
        ft,
        Style::default().fg(Color::Indexed(240)).bg(bg),
        bw - 4,
    );
}

// ─── Theme editor (iftoprs draw_theme_editor) ──────────────────────────────────

/// Draw the theme editor popup: the six palette channels with live values,
/// swatches, arrow previews, a gradient preview bar, and a name-entry prompt.
pub fn draw_theme_editor(buf: &mut Buffer, area: Rect, state: &OverlayState) {
    let t = &state.theme;
    let te = &state.theme_edit;
    let bw = 56u16.min(area.width.saturating_sub(4));
    let bh: u16 = if te.naming { 16 } else { 15 };
    let bh = bh.min(area.height.saturating_sub(4));
    let bg = tr(t.help_bg);
    let bs = Style::default().fg(tr(t.help_border));
    let bgs = Style::default().fg(Color::White).bg(bg);
    let ts = Style::default()
        .fg(tr(t.help_title))
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let hint_s = Style::default().fg(Color::Indexed(240)).bg(bg);
    let sel_s = Style::default().fg(Color::White).bg(Color::Indexed(237));

    let (x0, y0) = draw_box(buf, area, bw, bh, bg, bs);

    // Title
    let title = "\u{1F3A8} THEME EDITOR";
    let tlen = title.chars().count() as u16;
    set_str(
        buf,
        x0 + (bw.saturating_sub(tlen)) / 2,
        y0 + 1,
        title,
        ts,
        bw - 2,
    );

    // Color channel labels
    let labels = ["primary", "accent", "c3", "c4", "c5", "c6"];
    let colors = te.colors;

    for (i, label) in labels.iter().enumerate() {
        let row_y = y0 + 3 + i as u16;
        if row_y >= y0 + bh - 2 {
            break;
        }
        let is_sel = i == te.slot;

        let row_style = if is_sel { sel_s } else { bgs };
        if is_sel {
            for x in x0 + 1..x0 + bw - 1 {
                set_cell(buf, x, row_y, " ", sel_s);
            }
        }

        let marker = if is_sel { "\u{25B8} " } else { "  " };
        set_str(buf, x0 + 2, row_y, marker, row_style, 2);

        let label_str = format!("{:<10}", label);
        set_str(buf, x0 + 4, row_y, &label_str, row_style, 10);

        let val_str = format!("{:>3}", colors[i]);
        set_str(buf, x0 + 15, row_y, &val_str, row_style, 3);

        // Color swatch
        let swatch_s = Style::default().fg(Color::Indexed(colors[i])).bg(bg);
        set_str(
            buf,
            x0 + 20,
            row_y,
            "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}",
            swatch_s,
            5,
        );

        // Arrow preview
        let arrow_s = Style::default().fg(Color::Indexed(colors[i])).bg(bg);
        set_str(
            buf,
            x0 + 26,
            row_y,
            " \u{25C0}\u{2500}\u{2500}\u{25B6}",
            arrow_s,
            5,
        );
    }

    // Preview bar using the full palette
    let preview_y = y0 + 10;
    if preview_y < y0 + bh - 2 {
        set_str(buf, x0 + 2, preview_y, "preview:", hint_s, 8);
        let preview_w = (bw as usize).saturating_sub(13);
        for j in 0..preview_w {
            let frac = j as f64 / preview_w as f64;
            let c = if frac < 0.20 {
                Color::Indexed(colors[0]) // primary
            } else if frac < 0.40 {
                Color::Indexed(colors[1]) // accent
            } else if frac < 0.55 {
                Color::Indexed(colors[2]) // c3
            } else if frac < 0.70 {
                Color::Indexed(colors[3]) // c4
            } else if frac < 0.85 {
                Color::Indexed(colors[4]) // c5
            } else {
                Color::Indexed(colors[5]) // c6
            };
            set_cell(
                buf,
                x0 + 11 + j as u16,
                preview_y,
                "\u{2588}",
                Style::default().fg(c).bg(bg),
            );
        }
    }

    // Naming prompt or keybind hints
    if te.naming {
        let name_y = y0 + 12;
        if name_y < y0 + bh - 1 {
            let input_s = Style::default()
                .fg(Color::Indexed(48))
                .bg(Color::Indexed(235));
            set_str(buf, x0 + 2, name_y, "Theme name:", bgs, 11);
            let name_display = format!("{}_", te.name);
            set_str(buf, x0 + 14, name_y, &name_display, input_s, bw - 16);
            set_str(
                buf,
                x0 + 2,
                name_y + 1,
                "Enter:save  Esc:back",
                hint_s,
                bw - 4,
            );
        }
    } else {
        let hint_y = y0 + 12;
        if hint_y < y0 + bh - 1 {
            set_str(
                buf,
                x0 + 2,
                hint_y,
                "j/k:select  h/l:\u{00B1}1  H/L:\u{00B1}10",
                hint_s,
                bw - 4,
            );
            set_str(
                buf,
                x0 + 2,
                hint_y + 1,
                "Enter/s:save  Esc/q:cancel",
                hint_s,
                bw - 4,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn buf() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 100, 40))
    }

    // ── State construction ──

    #[test]
    fn new_state_defaults() {
        let s = OverlayState::new();
        assert!(!s.show_help);
        assert!(!s.show_border);
        assert!(!s.show_header);
        assert_eq!(s.theme_name, ThemeName::default());
        assert!(!s.theme_chooser.active);
        assert!(!s.theme_edit.active);
    }

    #[test]
    fn set_theme_changes_and_clears_custom() {
        let mut s = OverlayState::new();
        s.active_custom_theme = Some("mine".into());
        s.set_theme(ThemeName::BladeRunner);
        assert_eq!(s.theme_name, ThemeName::BladeRunner);
        assert!(s.active_custom_theme.is_none());
    }

    // ── Top-level hotkeys ──

    #[test]
    fn h_toggles_help() {
        let mut s = OverlayState::new();
        assert!(s.handle_key(key(KeyCode::Char('h'))));
        assert!(s.show_help);
        assert!(s.handle_key(key(KeyCode::Char('?'))));
        assert!(!s.show_help);
    }

    #[test]
    fn c_opens_chooser_selecting_current() {
        let mut s = OverlayState::new();
        s.set_theme(ThemeName::BladeRunner);
        s.show_help = true;
        assert!(s.handle_key(key(KeyCode::Char('c'))));
        assert!(s.theme_chooser.active);
        assert!(!s.show_help);
        let idx = ThemeName::ALL
            .iter()
            .position(|&t| t == ThemeName::BladeRunner)
            .unwrap();
        assert_eq!(s.theme_chooser.selected, idx);
    }

    #[test]
    fn capital_c_opens_editor_with_current_palette() {
        let mut s = OverlayState::new();
        s.set_theme(ThemeName::BladeRunner);
        assert!(s.handle_key(key(KeyCode::Char('C'))));
        assert!(s.theme_edit.active);
        assert_eq!(s.theme_edit.colors, Theme::palette_values(ThemeName::BladeRunner));
    }

    #[test]
    fn b_toggles_border_with_status() {
        let mut s = OverlayState::new();
        assert!(!s.show_border); // off by default (htop-like)
        assert!(s.handle_key(key(KeyCode::Char('b'))));
        assert!(s.show_border);
        assert_eq!(s.status.as_deref(), Some("Border: on"));
        s.handle_key(key(KeyCode::Char('b')));
        assert!(!s.show_border);
        assert_eq!(s.status.as_deref(), Some("Border: off"));
    }

    #[test]
    fn draw_chrome_emits_border_when_on() {
        // Fresh thread → fresh OVERLAY (border off): nothing emitted.
        let mut out: Vec<u8> = Vec::new();
        draw_chrome(&mut out);
        assert!(out.is_empty());
        // Turn the border on, then it must emit box-drawing bytes + title.
        dispatch_key(b'b' as i32);
        let mut out2: Vec<u8> = Vec::new();
        draw_chrome(&mut out2);
        let s = String::from_utf8_lossy(&out2);
        assert!(s.contains('┌') && s.contains('┘'));
        assert!(out2.contains(&b'H')); // HTOPRS title
    }

    #[test]
    fn x_is_not_consumed_by_overlay_when_idle() {
        // htop binds `x` (file locks); the overlay must let it pass through.
        let mut s = OverlayState::new();
        assert!(!s.handle_key(key(KeyCode::Char('x'))));
    }

    #[test]
    fn g_toggles_header_with_status() {
        let mut s = OverlayState::new();
        assert!(!s.show_header); // off by default
        assert!(s.handle_key(key(KeyCode::Char('g'))));
        assert!(s.show_header);
        assert_eq!(s.status.as_deref(), Some("Header: on"));
    }

    #[test]
    fn unhandled_key_returns_false() {
        let mut s = OverlayState::new();
        assert!(!s.handle_key(key(KeyCode::Char('z'))));
    }

    // ── Chooser navigation ──

    #[test]
    fn chooser_j_advances_and_wraps() {
        let mut s = OverlayState::new();
        s.theme_chooser.open(ThemeName::default());
        let start = s.theme_chooser.selected;
        s.handle_key(key(KeyCode::Char('j')));
        assert_eq!(s.theme_chooser.selected, (start + 1) % ThemeName::ALL.len());
        // theme follows selection
        assert_eq!(s.theme_name, ThemeName::ALL[s.theme_chooser.selected]);
    }

    #[test]
    fn chooser_k_wraps_backwards_from_zero() {
        let mut s = OverlayState::new();
        s.theme_chooser.active = true;
        s.theme_chooser.selected = 0;
        s.handle_key(key(KeyCode::Char('k')));
        assert_eq!(s.theme_chooser.selected, ThemeName::ALL.len() - 1);
    }

    #[test]
    fn chooser_enter_closes() {
        let mut s = OverlayState::new();
        s.theme_chooser.open(ThemeName::default());
        assert!(s.handle_key(key(KeyCode::Enter)));
        assert!(!s.theme_chooser.active);
    }

    #[test]
    fn chooser_c_closes() {
        let mut s = OverlayState::new();
        s.theme_chooser.open(ThemeName::default());
        s.handle_key(key(KeyCode::Char('c')));
        assert!(!s.theme_chooser.active);
    }

    // ── Editor ──

    #[test]
    fn editor_slot_moves_and_clamps() {
        let mut s = OverlayState::new();
        s.theme_edit.open([1, 2, 3, 4, 5, 6]);
        for _ in 0..10 {
            s.handle_key(key(KeyCode::Char('j')));
        }
        assert_eq!(s.theme_edit.slot, 5);
        for _ in 0..10 {
            s.handle_key(key(KeyCode::Char('k')));
        }
        assert_eq!(s.theme_edit.slot, 0);
    }

    #[test]
    fn editor_l_and_h_adjust_channel_by_one() {
        let mut s = OverlayState::new();
        s.theme_edit.open([100, 0, 0, 0, 0, 0]);
        s.handle_key(key(KeyCode::Char('l')));
        assert_eq!(s.theme_edit.colors[0], 101);
        s.handle_key(key(KeyCode::Char('h')));
        assert_eq!(s.theme_edit.colors[0], 100);
    }

    #[test]
    fn editor_capital_l_h_adjust_by_ten_wrapping() {
        let mut s = OverlayState::new();
        s.theme_edit.open([5, 0, 0, 0, 0, 0]);
        s.handle_key(key(KeyCode::Char('H'))); // 5 - 10 wraps
        assert_eq!(s.theme_edit.colors[0], 251);
        s.handle_key(key(KeyCode::Char('L'))); // 251 + 10 wraps
        assert_eq!(s.theme_edit.colors[0], 5);
    }

    #[test]
    fn editor_enter_starts_naming_then_saves_custom_theme() {
        let mut s = OverlayState::new();
        s.theme_edit.open([10, 20, 30, 40, 50, 60]);
        s.handle_key(key(KeyCode::Enter)); // enter naming
        assert!(s.theme_edit.naming);
        for ch in "cool".chars() {
            s.handle_key(key(KeyCode::Char(ch)));
        }
        assert_eq!(s.theme_edit.name, "cool");
        s.handle_key(key(KeyCode::Enter)); // save
        assert!(!s.theme_edit.active);
        let saved = s.custom_themes.get("cool").expect("saved");
        assert_eq!(
            [saved.c1, saved.c2, saved.c3, saved.c4, saved.c5, saved.c6],
            [10, 20, 30, 40, 50, 60]
        );
        assert_eq!(s.active_custom_theme.as_deref(), Some("cool"));
        assert_eq!(s.status.as_deref(), Some("Saved theme: cool"));
    }

    #[test]
    fn editor_naming_backspace_removes_char() {
        let mut s = OverlayState::new();
        s.theme_edit.open([1, 2, 3, 4, 5, 6]);
        s.handle_key(key(KeyCode::Enter));
        for ch in "ab".chars() {
            s.handle_key(key(KeyCode::Char(ch)));
        }
        s.handle_key(key(KeyCode::Backspace));
        assert_eq!(s.theme_edit.name, "a");
        assert_eq!(s.theme_edit.cursor, 1);
    }

    #[test]
    fn editor_esc_cancels_and_restores_builtin() {
        let mut s = OverlayState::new();
        s.set_theme(ThemeName::BladeRunner);
        s.theme_edit.open(Theme::palette_values(ThemeName::BladeRunner));
        s.handle_key(key(KeyCode::Char('l'))); // perturb the live palette
        s.handle_key(key(KeyCode::Esc));
        assert!(!s.theme_edit.active);
        // restored to the built-in theme's colors
        let expected = Theme::from_name(ThemeName::BladeRunner);
        assert_eq!(s.theme.bar_color, expected.bar_color);
    }

    // ── Rendering (into a headless Buffer) ──

    #[test]
    fn draw_help_writes_title_and_does_not_panic() {
        let mut b = buf();
        let s = OverlayState::new();
        let area = *b.area();
        draw_help(&mut b, area, &s);
        let joined: String = b.content().iter().map(|c| c.symbol()).collect();
        assert!(joined.contains("HTOPRS"));
        assert!(joined.contains("KEYBOARD SHORTCUTS"));
        assert!(joined.contains("THEME"));
    }

    #[test]
    fn draw_help_small_area_no_panic() {
        let mut b = Buffer::empty(Rect::new(0, 0, 20, 8));
        let s = OverlayState::new();
        let area = *b.area();
        draw_help(&mut b, area, &s);
    }

    #[test]
    fn draw_theme_chooser_renders_names() {
        let mut b = buf();
        let mut s = OverlayState::new();
        s.theme_chooser.open(ThemeName::default());
        let area = *b.area();
        draw_theme_chooser(&mut b, area, &s);
        let joined: String = b.content().iter().map(|c| c.symbol()).collect();
        assert!(joined.contains("THEME CHOOSER"));
        assert!(joined.contains(ThemeName::default().display_name()));
    }

    #[test]
    fn draw_theme_editor_renders_channels() {
        let mut b = buf();
        let mut s = OverlayState::new();
        s.theme_edit.open([1, 2, 3, 4, 5, 6]);
        let area = *b.area();
        draw_theme_editor(&mut b, area, &s);
        let joined: String = b.content().iter().map(|c| c.symbol()).collect();
        assert!(joined.contains("THEME EDITOR"));
        assert!(joined.contains("primary"));
        assert!(joined.contains("preview:"));
    }

    #[test]
    fn draw_theme_editor_naming_prompt() {
        let mut b = buf();
        let mut s = OverlayState::new();
        s.theme_edit.open([1, 2, 3, 4, 5, 6]);
        s.theme_edit.naming = true;
        s.theme_edit.name = "abc".into();
        let area = *b.area();
        draw_theme_editor(&mut b, area, &s);
        let joined: String = b.content().iter().map(|c| c.symbol()).collect();
        assert!(joined.contains("Theme name:"));
    }

    #[test]
    fn render_dispatches_active_overlay() {
        let mut b = buf();
        let mut s = OverlayState::new();
        s.show_help = true;
        let area = *b.area();
        s.render(&mut b, area);
        let joined: String = b.content().iter().map(|c| c.symbol()).collect();
        assert!(joined.contains("HTOPRS"));
    }

    // ── Color conversion ──

    #[test]
    fn tr_maps_ansi_and_black() {
        assert_eq!(tr(crossterm::style::Color::AnsiValue(99)), Color::Indexed(99));
        assert_eq!(tr(crossterm::style::Color::Black), Color::Black);
        assert_eq!(tr(crossterm::style::Color::White), Color::White);
        assert_eq!(tr(crossterm::style::Color::Reset), Color::Reset);
    }

    // ── ncurses key mapping ──

    #[test]
    fn ncurses_key_mapping() {
        assert_eq!(ncurses_to_keycode(b'h' as i32), Some(KeyCode::Char('h')));
        assert_eq!(ncurses_to_keycode(b'C' as i32), Some(KeyCode::Char('C')));
        assert_eq!(ncurses_to_keycode(27), Some(KeyCode::Esc));
        assert_eq!(ncurses_to_keycode(13), Some(KeyCode::Enter));
        assert_eq!(ncurses_to_keycode(KEY_UP), Some(KeyCode::Up));
        assert_eq!(ncurses_to_keycode(KEY_DOWN), Some(KeyCode::Down));
        assert_eq!(ncurses_to_keycode(KEY_BACKSPACE), Some(KeyCode::Backspace));
        assert_eq!(ncurses_to_keycode(-1), None); // ERR
    }

    #[test]
    fn handle_ncurses_key_toggles_help() {
        let mut s = OverlayState::new();
        assert!(s.handle_ncurses_key(b'h' as i32));
        assert!(s.show_help);
    }

    #[test]
    fn engaging_theme_sets_themed_flag() {
        let mut s = OverlayState::new();
        assert!(!s.themed);
        s.handle_ncurses_key(b'c' as i32);
        assert!(s.themed);
        assert!(s.theme_chooser.active);
    }

    #[test]
    fn current_palette_tracks_builtin_then_custom() {
        let mut s = OverlayState::new();
        s.set_theme(ThemeName::BladeRunner);
        assert_eq!(s.current_palette(), Theme::palette_values(ThemeName::BladeRunner));
        s.custom_themes.insert(
            "x".into(),
            CustomThemeColors { c1: 1, c2: 2, c3: 3, c4: 4, c5: 5, c6: 6 },
        );
        s.active_custom_theme = Some("x".into());
        assert_eq!(s.current_palette(), [1, 2, 3, 4, 5, 6]);
    }

    // ── crossterm color conversion / blit ──

    #[test]
    fn ct_maps_indexed_to_ansi_value() {
        assert_eq!(ct(Color::Indexed(200)), crossterm::style::Color::AnsiValue(200));
        assert_eq!(ct(Color::Reset), crossterm::style::Color::Reset);
        assert_eq!(ct(Color::White), crossterm::style::Color::White);
    }

    #[test]
    fn blit_skips_blank_backdrop_and_emits_modal() {
        // A buffer with one styled cell surrounded by default blanks: the blit
        // must emit the styled cell's symbol but not the blank backdrop.
        let mut b = Buffer::empty(Rect::new(0, 0, 4, 2));
        b[(1u16, 0u16)].set_symbol("X");
        b[(1u16, 0u16)].set_style(Style::default().fg(Color::Indexed(99)));
        let mut out: Vec<u8> = Vec::new();
        blit(&mut out, &b);
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains('X'));
        // The blank cell at (0,0) must not have been moved-to/printed as content.
        assert!(!s.contains("  ")); // no run of emitted blanks
    }

    #[test]
    fn thread_local_dispatch_consumes_hotkey() {
        // Runs on its own test thread → fresh OVERLAY.
        assert!(!overlay_active());
        assert!(dispatch_key(b'h' as i32)); // help toggles on, consumed
        assert!(overlay_active());
        assert!(dispatch_key(b'h' as i32)); // toggles off
        assert!(!overlay_active());
    }

    #[test]
    fn thread_local_non_hotkey_not_consumed_when_idle() {
        assert!(!dispatch_key(b'z' as i32));
        assert!(!overlay_active());
    }

    #[test]
    fn dispatch_then_draw_active_emits_overlay_bytes() {
        // End-to-end through the thread-local state: open help, render+blit.
        // (blit interleaves per-cell escape sequences, so the emitted title is
        // not contiguous in the byte stream — the buffer-level content is
        // asserted by `draw_help_writes_title_and_does_not_panic`. Here we just
        // confirm the thread-local pipeline emits the modal when open.)
        assert!(dispatch_key(b'h' as i32));
        let mut out: Vec<u8> = Vec::new();
        draw_active(&mut out);
        assert!(!out.is_empty());
        // The 'H' of the HTOPRS title is Print'd as a bare byte.
        assert!(out.contains(&b'H'));
    }

    #[test]
    fn draw_active_noop_when_no_overlay() {
        // Fresh thread → no overlay open → nothing emitted.
        let mut out: Vec<u8> = Vec::new();
        draw_active(&mut out);
        assert!(out.is_empty());
    }
}
