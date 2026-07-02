//! Port of `FunctionBar.c` — htop's bottom-row key/label bar.
//!
//! C names are preserved verbatim (`CamelCase_snake`), so `non_snake_case`
//! is allowed for the whole module — matching the spec name-for-name is the
//! point of the port.
//!
//! Only the three pure data-model functions are ported: [`FunctionBar_setLabel`],
//! [`FunctionBar_getWidth`], and [`FunctionBar_synthesizeEvent`]. The remaining
//! functions stay `todo!()` stubs because their behavior is inseparable from
//! unported substrate:
//!   * `FunctionBar_new` / `FunctionBar_newEnterEsc` — the static-data branch
//!     copies `FunctionBar_FEvents = {KEY_F(1), ...}`, which are ncurses
//!     `KEY_F` constants (`FunctionBar.c:28`); reproducing them would mean
//!     inventing ncurses values.
//!   * `FunctionBar_delete` — pure `free()` chain (`FunctionBar.c:67`); in Rust
//!     this is `Drop`, so there is no algorithm to port.
//!   * `FunctionBar_draw` / `FunctionBar_drawExtra` / `FunctionBar_append` —
//!     `attrset`/`mvaddstr`/`mvhline`/`curs_set` against `CRT_colors`, `LINES`,
//!     `COLS` (curses + CRT color substrate).
#![allow(non_snake_case)]
#![allow(dead_code)]

/// Port of `#define FUNCTIONBAR_MAXEVENTS 11` from `FunctionBar.c:22`
/// ("sufficient for all cases, includes NULL"). Used by the C loops'
/// `assert(i < FUNCTIONBAR_MAXEVENTS)` bound checks, mirrored below as
/// `debug_assert!`.
const FUNCTIONBAR_MAXEVENTS: usize = 11;

/// ncurses `ERR` sentinel (`#define ERR (-1)`), the value returned by
/// [`FunctionBar_synthesizeEvent`] when `pos` is past the last label.
/// Modeled as the plain integer it is, not an ncurses call.
const ERR: i32 = -1;

/// Minimal model of the C `FunctionBar` struct (`FunctionBar.h:14`).
///
/// Only the fields the ported functions read are modeled:
///   * `functions` — the `char** functions` array (NUL-terminated in C;
///     a `Vec<String>` here).
///   * `keys` — the `union { char** keys; const char* const* constKeys; }`.
///     The ported functions only ever read key labels as strings, so the
///     union collapses to a single `Vec<String>` (the observable is identical
///     for both union members).
///   * `events` — the `int* events` array, parallel to `functions`.
///
/// Omitted: `staticData` (`bool`) — none of the ported functions read it
/// (only `FunctionBar_delete` and `FunctionBar_new`, which stay stubs, do).
pub struct FunctionBar {
    pub functions: Vec<String>,
    pub keys: Vec<String>,
    pub events: Vec<i32>,
}

/// TODO: port of `FunctionBar* FunctionBar_newEnterEsc(const char* enter, const char* esc` from `FunctionBar.c:35`.
pub fn FunctionBar_newEnterEsc() {
    todo!("port of FunctionBar.c:35")
}

/// TODO: port of `FunctionBar* FunctionBar_new(const char* const* functions, const char* const* keys, const int* events` from `FunctionBar.c:40`.
pub fn FunctionBar_new() {
    todo!("port of FunctionBar.c:40")
}

/// TODO: port of `void FunctionBar_delete(FunctionBar* this` from `FunctionBar.c:67`.
pub fn FunctionBar_delete() {
    todo!("port of FunctionBar.c:67")
}

/// Port of `void FunctionBar_setLabel(FunctionBar* this, int event, const char* text)`
/// from `FunctionBar.c:83`.
///
/// Walks the NUL-terminated `functions` array in parallel with `events`;
/// on the first slot whose event equals `event`, replaces that label with a
/// copy of `text` and stops. If no event matches, the bar is unchanged.
/// The C `free`/`xStrdup` pair is a plain owned-string assignment in Rust.
pub fn FunctionBar_setLabel(this: &mut FunctionBar, event: i32, text: &str) {
    for i in 0..this.functions.len() {
        debug_assert!(i < FUNCTIONBAR_MAXEVENTS);
        if this.events[i] == event {
            this.functions[i] = text.to_string();
            break;
        }
    }
}

/// Port of `int FunctionBar_getWidth(const FunctionBar* this)` from
/// `FunctionBar.c:151`.
///
/// Sums, over every function slot, the byte lengths of the key label and the
/// function label (C `strlen`, i.e. bytes). Accumulated in an `int` as in C.
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
/// Advances a running column `x` by each slot's key + function byte lengths;
/// the first slot whose cumulative width strictly exceeds `pos` yields that
/// slot's event. If `pos` lands at or past the end of the bar, returns `ERR`
/// (`-1`).
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

/// TODO: port of `int FunctionBar_draw(const FunctionBar* this` from `FunctionBar.c:94`.
pub fn FunctionBar_draw() {
    todo!("port of FunctionBar.c:94")
}

/// TODO: port of `int FunctionBar_drawExtra(const FunctionBar* this, const char* buffer, int attr, bool setCursor` from `FunctionBar.c:98`.
pub fn FunctionBar_drawExtra() {
    todo!("port of FunctionBar.c:98")
}

/// TODO: port of `void FunctionBar_append(const char* buffer, int attr` from `FunctionBar.c:139`.
pub fn FunctionBar_append() {
    todo!("port of FunctionBar.c:139")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(keys: &[&str], functions: &[&str], events: &[i32]) -> FunctionBar {
        FunctionBar {
            functions: functions.iter().map(|s| s.to_string()).collect(),
            keys: keys.iter().map(|s| s.to_string()).collect(),
            events: events.to_vec(),
        }
    }

    #[test]
    fn setlabel_replaces_matching_event() {
        let mut b = bar(&["F1", "F2", "F3"], &["a", "b", "c"], &[1, 2, 3]);
        FunctionBar_setLabel(&mut b, 2, "xyz");
        assert_eq!(b.functions, vec!["a".to_string(), "xyz".to_string(), "c".to_string()]);
    }

    #[test]
    fn setlabel_no_match_is_noop() {
        let mut b = bar(&["F1", "F2"], &["a", "b"], &[1, 2]);
        FunctionBar_setLabel(&mut b, 99, "xyz");
        assert_eq!(b.functions, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn setlabel_stops_at_first_match() {
        // Duplicate event value: only the first slot is replaced (C `break`).
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
        // pos == 6 is NOT < 6 for slot0, falls to slot1 (6 < 12)
        assert_eq!(FunctionBar_synthesizeEvent(&b, 6), 20);
        assert_eq!(FunctionBar_synthesizeEvent(&b, 11), 20);
        // past the end -> ERR (-1)
        assert_eq!(FunctionBar_synthesizeEvent(&b, 12), ERR);
        assert_eq!(FunctionBar_synthesizeEvent(&b, 100), -1);
    }

    #[test]
    fn synthesize_event_negative_pos_hits_first_slot() {
        // pos = -1 < first cumulative width -> first event.
        let b = bar(&["F1"], &["Help"], &[10]);
        assert_eq!(FunctionBar_synthesizeEvent(&b, -1), 10);
    }
}
