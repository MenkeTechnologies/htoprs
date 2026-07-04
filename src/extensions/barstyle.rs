//! htoprs-original: cycle the glyph the header bar meters are filled with.
//!
//! Ported from storageshower: the `BarStyle` enum (`src/types.rs`), its `b`
//! key handler that steps the style and persists it (`src/keys.rs:422`), and
//! its per-style fill rendering (`src/ui.rs:727`). htop's native fill is a
//! single `'|'` (or the `|#*@$%&.` table in monochrome); this lets `b` swap it
//! for storageshower's block / thin / ascii / gradient glyphs while keeping
//! each bar segment's semantic color.
//!
//! The live style is a thread-local (the TUI draws on one thread), consulted by
//! the ported [`crate::ported::meter::BarMeterMode_draw`] fill loop through
//! [`fill_glyph`], and persisted to `~/.config/htoprs/prefs.json` alongside the
//! theme selection. The `b` key is wired into the ported keybinding table
//! ([`crate::ported::action::Action_setBindings`]) as an
//! [`crate::ported::action::Htop_Action`]; the slot is free on this build
//! (upstream binds `keys['b']` to `actionBacktrace` only under
//! `HAVE_BACKTRACE_SCREEN`, which is not compiled here).

use std::cell::Cell;

use serde::{Deserialize, Serialize};

use crate::ported::action::{
    State, HTOP_KEEP_FOLLOWING, HTOP_REDRAW_BAR, HTOP_REFRESH, Htop_Reaction,
};

/// The bar fill glyph style. `Classic` is htop's native fill; the other four
/// are storageshower's (`types.rs:24`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BarStyle {
    /// htop's native glyph: `'|'` in color mode, the `|#*@$%&.` table in
    /// monochrome. The resting default so the out-of-box look is unchanged.
    #[default]
    Classic,
    /// Position-shaded blocks `█▓▒░` with a `▸` tip.
    Gradient,
    /// Full block `█`.
    Solid,
    /// Thin bar `▬` with a `▸` tip.
    Thin,
    /// ASCII `#` with a `>` tip.
    Ascii,
}

impl BarStyle {
    /// storageshower's cycle order (`keys.rs:422`), with `Classic` threaded in
    /// first so htop's default look is the resting state and one press enters
    /// the storageshower styles.
    fn next(self) -> BarStyle {
        match self {
            BarStyle::Classic => BarStyle::Gradient,
            BarStyle::Gradient => BarStyle::Solid,
            BarStyle::Solid => BarStyle::Thin,
            BarStyle::Thin => BarStyle::Ascii,
            BarStyle::Ascii => BarStyle::Classic,
        }
    }

    /// Lower-case display name, matching storageshower's `-b`/config spelling.
    pub fn label(self) -> &'static str {
        match self {
            BarStyle::Classic => "classic",
            BarStyle::Gradient => "gradient",
            BarStyle::Solid => "solid",
            BarStyle::Thin => "thin",
            BarStyle::Ascii => "ascii",
        }
    }
}

thread_local! {
    static CURRENT: Cell<BarStyle> = const { Cell::new(BarStyle::Classic) };
}

/// The live bar style.
pub fn current() -> BarStyle {
    CURRENT.with(Cell::get)
}

/// Set the live bar style.
pub fn set(style: BarStyle) {
    CURRENT.with(|c| c.set(style));
}

/// The fill glyph for cell `cell` (0-based within a bar of width `bar_w`, with
/// `total_filled` cells lit), or `None` for `Classic` — in which case the
/// ported caller keeps htop's native glyph. The last lit cell
/// (`cell == total_filled - 1`) is the bar tip. Mirrors storageshower's
/// per-style rendering (`ui.rs:727`); the empty-cell decoration there is
/// dropped because htop overlays right-aligned value text on the same cells.
pub fn fill_glyph(cell: i32, total_filled: i32, bar_w: i32) -> Option<char> {
    let is_tip = cell == total_filled - 1;
    let frac = if bar_w > 0 {
        cell as f64 / bar_w as f64
    } else {
        0.0
    };
    match current() {
        BarStyle::Classic => None,
        BarStyle::Solid => Some('\u{2588}'), // █
        BarStyle::Thin => Some(if is_tip { '\u{25B8}' } else { '\u{25AC}' }), // ▸ / ▬
        BarStyle::Ascii => Some(if is_tip { '>' } else { '#' }),
        BarStyle::Gradient => Some(if is_tip {
            '\u{25B8}' // ▸
        } else if frac < 0.33 {
            '\u{2588}' // █
        } else if frac < 0.55 {
            '\u{2593}' // ▓
        } else if frac < 0.80 {
            '\u{2592}' // ▒
        } else {
            '\u{2591}' // ░
        }),
    }
}

/// The `b` handler (storageshower `keys.rs:422`): advance to the next style,
/// persist it, and request a bar redraw. The glyph swap does not change meter
/// height, so no resize is needed.
pub fn cycle_bar_style(_st: &mut State) -> Htop_Reaction {
    let next = current().next();
    set(next);
    crate::extensions::prefs::update(|p| p.bar_style = next);
    // Show the iftoprs-style status toast naming the new style.
    crate::extensions::overlay::set_status(format!("Bar style: {}", next.label()));
    HTOP_REFRESH | HTOP_REDRAW_BAR | HTOP_KEEP_FOLLOWING
}

/// Load the saved bar style (if any) into the thread-local so a prior choice is
/// active from the first frame. A no-op when no prefs file exists. Called once
/// at TUI startup.
pub fn init_from_prefs() {
    if let Some(p) = crate::extensions::prefs::load() {
        set(p.bar_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cycle order wraps through all five styles, starting and ending at
    /// Classic (storageshower's four plus the threaded-in Classic).
    #[test]
    fn cycle_order_wraps_through_all_five() {
        let seq = [
            BarStyle::Classic,
            BarStyle::Gradient,
            BarStyle::Solid,
            BarStyle::Thin,
            BarStyle::Ascii,
        ];
        let mut s = BarStyle::Classic;
        for expected_next in seq.iter().skip(1).chain(std::iter::once(&BarStyle::Classic)) {
            s = s.next();
            assert_eq!(s, *expected_next);
        }
    }

    /// Classic yields no override (caller keeps htop's native glyph).
    #[test]
    fn classic_is_none() {
        set(BarStyle::Classic);
        assert_eq!(fill_glyph(0, 5, 10), None);
        assert_eq!(fill_glyph(4, 5, 10), None);
    }

    /// Each non-Classic style renders its body glyph and a distinct tip on the
    /// last lit cell.
    #[test]
    fn styles_render_body_and_tip() {
        set(BarStyle::Solid);
        assert_eq!(fill_glyph(0, 5, 10), Some('\u{2588}'));
        assert_eq!(fill_glyph(4, 5, 10), Some('\u{2588}')); // solid has no special tip

        set(BarStyle::Thin);
        assert_eq!(fill_glyph(0, 5, 10), Some('\u{25AC}'));
        assert_eq!(fill_glyph(4, 5, 10), Some('\u{25B8}')); // tip

        set(BarStyle::Ascii);
        assert_eq!(fill_glyph(0, 5, 10), Some('#'));
        assert_eq!(fill_glyph(4, 5, 10), Some('>')); // tip

        set(BarStyle::Classic); // restore for other tests on this thread
    }

    /// Gradient shades by fractional position and caps with the tip glyph.
    #[test]
    fn gradient_shades_by_position() {
        set(BarStyle::Gradient);
        // bar_w = 100 so frac == cell/100.
        assert_eq!(fill_glyph(10, 60, 100), Some('\u{2588}')); // frac .10 < .33
        assert_eq!(fill_glyph(40, 60, 100), Some('\u{2593}')); // frac .40 < .55
        assert_eq!(fill_glyph(60, 80, 100), Some('\u{2592}')); // frac .60 < .80
        assert_eq!(fill_glyph(90, 100, 100), Some('\u{2591}')); // frac .90
        assert_eq!(fill_glyph(59, 60, 100), Some('\u{25B8}')); // tip (last lit)
        set(BarStyle::Classic);
    }

    /// A zero-width bar does not divide by zero.
    #[test]
    fn zero_width_is_safe() {
        set(BarStyle::Gradient);
        assert_eq!(fill_glyph(0, 0, 0), Some('\u{2588}')); // frac 0.0 branch
        set(BarStyle::Classic);
    }
}
