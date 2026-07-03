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

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::theme::{CustomThemeColors, Theme, ThemeName};

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
            show_border: true,
            show_header: true,
            theme_name,
            theme: Theme::from_name(theme_name),
            theme_chooser: ThemeChooser::new(),
            theme_edit: ThemeEditState::new(),
            custom_themes: HashMap::new(),
            active_custom_theme: None,
            status: None,
        }
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
                self.theme_chooser.open(self.theme_name);
            }
            KeyCode::Char('C') => {
                self.show_help = false;
                let palette = self
                    .active_custom_theme
                    .as_ref()
                    .and_then(|name| self.custom_themes.get(name))
                    .map(|ct| [ct.c1, ct.c2, ct.c3, ct.c4, ct.c5, ct.c6])
                    .unwrap_or_else(|| Theme::palette_values(self.theme_name));
                self.theme_edit.open(palette);
            }
            KeyCode::Char('x') => {
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
                ("x", "Toggle border"),
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
        assert!(s.show_border);
        assert!(s.show_header);
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
    fn x_toggles_border_with_status() {
        let mut s = OverlayState::new();
        assert!(s.handle_key(key(KeyCode::Char('x'))));
        assert!(!s.show_border);
        assert_eq!(s.status.as_deref(), Some("Border: off"));
        s.handle_key(key(KeyCode::Char('x')));
        assert!(s.show_border);
        assert_eq!(s.status.as_deref(), Some("Border: on"));
    }

    #[test]
    fn g_toggles_header_with_status() {
        let mut s = OverlayState::new();
        assert!(s.handle_key(key(KeyCode::Char('g'))));
        assert!(!s.show_header);
        assert_eq!(s.status.as_deref(), Some("Header: off"));
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
}
