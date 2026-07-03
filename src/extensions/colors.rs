//! 256-color theming for htoprs's htop UI, via an ANSI palette remap.
//!
//! htoprs faithfully ports htop's 8-color scheme model: every entry in
//! [`crate::ported::crt::CRT_colorSchemes`] is built from the eight basic
//! ncurses colors, and all on-screen color flows through
//! `Ncurses::to_color(n: i16)` (`functionbar.rs`), which maps an ncurses color
//! number `0..=8`/`-1` to a `crossterm::style::Color`.
//!
//! A theme here is a **base16-style ANSI remap**: it redefines what those eight
//! ANSI slots *look like* by pointing each at a 256-color palette value. htop's
//! semantic color assignments (e.g. `PROCESS_BASENAME` is "cyan") are untouched;
//! only the concrete color cyan resolves to changes. This recolors the entire
//! UI in true 256-color per theme without altering the ported scheme table or
//! `ResolvedColor::from_attr` — `to_color` consults [`remap`] and, when a theme
//! is active, returns `Color::AnsiValue(idx)` instead of the fixed ANSI color.
//!
//! The palette→ANSI mapping in [`remap_from_palette`] is authored htoprs design
//! (there is no upstream to port): it distributes a theme's six palette channels
//! across the eight ANSI slots.

use std::cell::RefCell;

use super::theme::{Theme, ThemeName};

/// Remap of ncurses color numbers `0..=8` to 256-color palette indices.
/// Index `i` holds the `Color::AnsiValue` that ncurses color `i` resolves to
/// while this remap is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnsiRemap(pub [u8; 9]);

/// Build an ANSI remap from a theme's six-channel palette
/// `(primary, accent, c3, c4, c5, c6)`. `c6` is the palette's darkest channel
/// (htoprs `Theme::from_palette_raw` uses it as the bar background), so it
/// anchors the black slot; the brighter channels drive the foreground slots
/// htop uses most (cyan/green for meter text, process basenames, running
/// tasks). Authored htoprs mapping — not a port.
pub fn remap_from_palette(p: [u8; 6]) -> AnsiRemap {
    let [c1, c2, c3, c4, c5, c6] = p;
    AnsiRemap([
        c6, // 0 Black   — darkest channel (backgrounds / black-on-color text)
        c5, // 1 Red
        c2, // 2 Green   — accent (running tasks, OK values)
        c4, // 3 Yellow
        c3, // 4 Blue
        c3, // 5 Magenta
        c2, // 6 Cyan    — accent (meter text, process basenames — heavily used)
        c1, // 7 White   — primary (bright text)
        c6, // 8 gray    — darkest channel (shadows)
    ])
}

thread_local! {
    /// The active ANSI remap, or `None` when htoprs uses its native htop scheme.
    /// Thread-local because the TUI draws on a single thread (`ScreenManager_run`).
    static ACTIVE: RefCell<Option<AnsiRemap>> = const { RefCell::new(None) };
}

/// Install (or clear, with `None`) the active ANSI remap.
pub fn set(remap: Option<AnsiRemap>) {
    ACTIVE.with(|a| *a.borrow_mut() = remap);
}

/// Clear the active remap — the UI returns to htop's native scheme colors.
pub fn clear() {
    set(None);
}

/// Activate a remap derived from a raw six-channel palette (editor previews).
pub fn apply_palette(palette: [u8; 6]) {
    set(Some(remap_from_palette(palette)));
}

/// Activate a remap derived from a built-in [`ThemeName`].
pub fn apply_theme(name: ThemeName) {
    apply_palette(Theme::palette_values(name));
}

/// Whether a theme remap is currently active.
pub fn is_active() -> bool {
    ACTIVE.with(|a| a.borrow().is_some())
}

/// The 256-color index ncurses color `n` should resolve to under the active
/// remap, or `None` when no theme is active or `n` is outside `0..=8` (the
/// terminal-default `-1` and any out-of-range value keep their native color).
/// Called by the ported `Ncurses::to_color`.
pub fn remap(n: i16) -> Option<u8> {
    if !(0..=8).contains(&n) {
        return None;
    }
    ACTIVE.with(|a| a.borrow().map(|r| r.0[n as usize]))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ACTIVE is thread-local; each #[test] runs on its own thread, so these
    // never observe each other's remap state.

    #[test]
    fn no_remap_by_default() {
        assert!(!is_active());
        assert_eq!(remap(2), None);
    }

    #[test]
    fn apply_theme_activates_and_remaps() {
        apply_theme(ThemeName::BladeRunner);
        assert!(is_active());
        let pal = Theme::palette_values(ThemeName::BladeRunner);
        // Cyan (6) -> accent channel (c2 = pal[1]).
        assert_eq!(remap(6), Some(pal[1]));
        // White (7) -> primary (c1 = pal[0]).
        assert_eq!(remap(7), Some(pal[0]));
    }

    #[test]
    fn clear_deactivates() {
        apply_theme(ThemeName::NeonSprawl);
        assert!(is_active());
        clear();
        assert!(!is_active());
        assert_eq!(remap(3), None);
    }

    #[test]
    fn out_of_range_never_remapped() {
        apply_theme(ThemeName::NeonSprawl);
        assert_eq!(remap(-1), None); // terminal default
        assert_eq!(remap(9), None);
        assert_eq!(remap(255), None);
    }

    #[test]
    fn remap_from_palette_layout() {
        let r = remap_from_palette([10, 20, 30, 40, 50, 60]);
        assert_eq!(r.0[0], 60); // black -> c6
        assert_eq!(r.0[2], 20); // green -> accent (c2)
        assert_eq!(r.0[6], 20); // cyan  -> accent (c2)
        assert_eq!(r.0[7], 10); // white -> primary (c1)
        assert_eq!(r.0[8], 60); // gray  -> c6
    }

    #[test]
    fn apply_palette_matches_remap_from_palette() {
        apply_palette([1, 2, 3, 4, 5, 6]);
        assert_eq!(remap(0), Some(6));
        assert_eq!(remap(7), Some(1));
        clear();
    }
}
