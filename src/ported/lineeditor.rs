//! Port of `LineEditor.c` — htop's fixed-buffer inline text editor.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake` and
//! lowerCamelCase statics), so `non_snake_case` is allowed for the
//! whole module — matching the spec name-for-name is the point of the
//! port.
//!
//! The pure text-buffer editing functions are ported: init/reset/
//! setText, cursor movement (single char and word boundaries), the
//! three edit primitives (`deleteCharBefore`, `deleteCharAt`,
//! `insertChar`), and the scroll-offset arithmetic in
//! `LineEditor_updateScroll`. They operate only on the `LineEditor`
//! struct's `buffer`/`len`/`cursor`/`scroll`/`maxLen` fields and have
//! faithful safe-Rust analogs (`memmove` → `slice::copy_within`,
//! `strncpy`/`strnlen` inlined against the fixed byte array).
//!
//! The pure accessors `LineEditor_getText` (LineEditor.h:37) and
//! `LineEditor_getCursor` (LineEditor.h:42) are ported as struct reads,
//! and `LineEditor_handleKey` (:118) dispatches on the ncurses `KEY_*`
//! integers (reproduced verbatim in `crt.rs`) to the cursor-movement and
//! edit primitives above, so it needs no drawing substrate.
//!
//! `LineEditor_draw` (:209) is ported through the `Ncurses` crossterm
//! emit shim owned by `functionbar.rs` (the same shim `panel.rs` and
//! `screenmanager.rs` draw through): `attrset`/`mvaddnstr`/`mvaddch` map
//! to the shim methods, `CRT_colors[FUNCTION_BAR]` to
//! `ColorElements::FUNCTION_BAR.packed(ColorScheme::active())`, and
//! `LINES` to `Ncurses::lines()`. `LineEditor_click` (:235) is pure
//! cursor arithmetic over `scroll`/`len` and needs no draw substrate.
//!
//! Nothing in `LineEditor.c`/`.h` remains stubbed.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::io::{self, Write};

use crate::ported::crt::{
    ColorElements, ColorScheme, KEY_BACKSPACE, KEY_CTRL, KEY_DC, KEY_END, KEY_HOME, KEY_LEFT,
    KEY_RIGHT, KEY_SLEFT, KEY_SRIGHT,
};
use crate::ported::functionbar::Ncurses;

/// Port of `#define LINEEDITOR_MAX 128` from `LineEditor.h:14`.
pub const LINEEDITOR_MAX: usize = 128;

/// Port of `struct LineEditor_` from `LineEditor.h:16`.
///
/// `buffer` is the fixed `char buffer[LINEEDITOR_MAX + 1]` (holds a
/// trailing `'\0'`), `len` the current text length, `cursor` the byte
/// position in `0..=len`, `scroll` the display scroll offset, and
/// `maxLen` the maximum allowed input length (`0` = uninitialized;
/// `LineEditor_initWithMax` clamps it to `LINEEDITOR_MAX`).
pub struct LineEditor {
    buffer: [u8; LINEEDITOR_MAX + 1],
    len: usize,
    cursor: usize,
    scroll: usize,
    maxLen: usize,
}

impl Default for LineEditor {
    /// Zeroed backing store so a `LineEditor` value can exist before
    /// `LineEditor_init` / `LineEditor_initWithMax` runs on it — the C
    /// code always operates on a caller-owned `LineEditor*`.
    fn default() -> Self {
        LineEditor {
            buffer: [0; LINEEDITOR_MAX + 1],
            len: 0,
            cursor: 0,
            scroll: 0,
            maxLen: 0,
        }
    }
}

// C's `isspace` on an `unsigned char` matches space, `\t`, `\n`, `\v`,
// `\f`, `\r`. The word-boundary loops below inline that test as
// `matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')` — Rust's
// `is_ascii_whitespace` omits `\v` (0x0B), which would diverge.

/// Port of `LineEditor_init` from `LineEditor.c:20`.
pub fn LineEditor_init(this: &mut LineEditor) {
    LineEditor_initWithMax(this, LINEEDITOR_MAX);
}

/// Port of `LineEditor_initWithMax` from `LineEditor.c:24`.
pub fn LineEditor_initWithMax(this: &mut LineEditor, maxLen: usize) {
    this.buffer[0] = b'\0';
    this.len = 0;
    this.cursor = 0;
    this.scroll = 0;
    this.maxLen = if maxLen > 0 && maxLen <= LINEEDITOR_MAX {
        maxLen
    } else {
        LINEEDITOR_MAX
    };
}

/// Port of `LineEditor_reset` from `LineEditor.c:32`.
pub fn LineEditor_reset(this: &mut LineEditor) {
    this.buffer[0] = b'\0';
    this.len = 0;
    this.cursor = 0;
    this.scroll = 0;
}

/// Port of `LineEditor_setText` from `LineEditor.c:39`.
///
/// Faithful `strncpy(buffer, text, maxLen)` + `buffer[maxLen] = '\0'`
/// + `len = strnlen(buffer, maxLen)`: copies up to `maxLen` bytes from
/// `text` (stopping at the first embedded `'\0'`, like a C string) and
/// zero-fills the remainder, then measures the length.
pub fn LineEditor_setText(this: &mut LineEditor, text: &str) {
    let copyLen = this.maxLen;
    let src = text.as_bytes();
    // strncpy treats `text` as a C string: stop at the first NUL.
    let srcEnd = src.iter().position(|&b| b == 0).unwrap_or(src.len());
    for i in 0..copyLen {
        this.buffer[i] = if i < srcEnd { src[i] } else { 0 };
    }
    this.buffer[copyLen] = b'\0';
    // strnlen(buffer, maxLen)
    this.len = this.buffer[..this.maxLen]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(this.maxLen);
    this.cursor = this.len;
    this.scroll = 0;
}

/// Port of `LineEditor_getText` from `LineEditor.h:37`
/// (`static inline char* LineEditor_getText(LineEditor* this) { return this->buffer; }`).
///
/// The C returns the raw `char*` to the NUL-terminated buffer; the Rust
/// analog returns the live text region `buffer[..len]` as `&str`. The
/// editor only ever stores ASCII (`insertChar` gates on `isprint`), so the
/// UTF-8 decode never fails in practice; a non-UTF-8 buffer set via
/// `LineEditor_setText` degrades to `""` rather than panicking.
pub fn LineEditor_getText(this: &LineEditor) -> &str {
    std::str::from_utf8(&this.buffer[..this.len]).unwrap_or("")
}

/// Port of `LineEditor_getCursor` from `LineEditor.h:42`
/// (`static inline size_t LineEditor_getCursor(LineEditor* this) { return this->cursor; }`).
///
/// Pure struct read of the `cursor` byte position (`0..=len`). Used by
/// `ScreenTabsPanel`/`ScreensPanel` rename handlers to place the cursor.
pub fn LineEditor_getCursor(this: &LineEditor) -> usize {
    this.cursor
}

/// Port of `moveCursorLeft` from `LineEditor.c:51`.
pub fn moveCursorLeft(this: &mut LineEditor) {
    if this.cursor > 0 {
        this.cursor -= 1;
    }
}

/// Port of `moveCursorRight` from `LineEditor.c:57`.
pub fn moveCursorRight(this: &mut LineEditor) {
    if this.cursor < this.len {
        this.cursor += 1;
    }
}

/// Port of `moveCursorWordLeft` from `LineEditor.c:63`.
pub fn moveCursorWordLeft(this: &mut LineEditor) {
    let mut pos = this.cursor;
    // skip whitespace before cursor
    while pos > 0
        && matches!(
            this.buffer[pos - 1],
            b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'
        )
    {
        pos -= 1;
    }
    // skip non-whitespace (the word itself)
    while pos > 0
        && !matches!(
            this.buffer[pos - 1],
            b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'
        )
    {
        pos -= 1;
    }
    this.cursor = pos;
}

/// Port of `moveCursorWordRight` from `LineEditor.c:75`.
pub fn moveCursorWordRight(this: &mut LineEditor) {
    let mut pos = this.cursor;
    let len = this.len;
    // skip non-whitespace
    while pos < len && !matches!(this.buffer[pos], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        pos += 1;
    }
    // skip whitespace
    while pos < len && matches!(this.buffer[pos], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        pos += 1;
    }
    this.cursor = pos;
}

/// Port of `deleteCharBefore` from `LineEditor.c:88`.
pub fn deleteCharBefore(this: &mut LineEditor) -> bool {
    if this.cursor == 0 {
        return false;
    }
    let pos = this.cursor - 1;
    // memmove(buffer + pos, buffer + cursor, len - cursor + 1)
    this.buffer.copy_within(this.cursor..this.len + 1, pos);
    this.len -= 1;
    this.cursor = pos;
    true
}

/// Port of `deleteCharAt` from `LineEditor.c:99`.
pub fn deleteCharAt(this: &mut LineEditor) -> bool {
    if this.cursor >= this.len {
        return false;
    }
    // memmove(buffer + cursor, buffer + cursor + 1, len - cursor)
    this.buffer
        .copy_within(this.cursor + 1..this.len + 1, this.cursor);
    this.len -= 1;
    true
}

/// Port of `insertChar` from `LineEditor.c:108`.
pub fn insertChar(this: &mut LineEditor, ch: u8) -> bool {
    if this.len >= this.maxLen {
        return false;
    }
    // memmove(buffer + cursor + 1, buffer + cursor, len - cursor + 1)
    this.buffer
        .copy_within(this.cursor..this.len + 1, this.cursor + 1);
    this.buffer[this.cursor] = ch;
    this.cursor += 1;
    this.len += 1;
    true
}

// Ctrl-key codes `LineEditor_handleKey` matches. `crt::KEY_CTRL` is a
// `const fn`; binding its results as `const`s makes them usable as match
// patterns (a const-fn call is not itself a pattern) without adding any
// top-level `fn` (mirrors the same idiom in `panel.rs`).
const LE_CTRL_B: i32 = KEY_CTRL(b'B' as i32);
const LE_CTRL_F: i32 = KEY_CTRL(b'F' as i32);
const LE_CTRL_A: i32 = KEY_CTRL(b'A' as i32);
const LE_CTRL_E: i32 = KEY_CTRL(b'E' as i32);
const LE_CTRL_W: i32 = KEY_CTRL(b'W' as i32);
const LE_CTRL_K: i32 = KEY_CTRL(b'K' as i32);
const LE_CTRL_U: i32 = KEY_CTRL(b'U' as i32);

/// Port of `LineEditor_handleKey` from `LineEditor.c:118`.
///
/// Faithful transcription of the C `switch (ch)`. The ncurses `KEY_*`
/// integers are the ones reproduced verbatim in `crt.rs`. `isspace` is
/// inlined as the same C-locale test used by the word-movement helpers
/// above; `isprint` is inlined as the C-locale ASCII printable range
/// `0x20..=0x7e` (bytes `>= 0x80` are locale-dependent in C and rejected
/// here, matching the C locale). Returns `true` when the text content
/// changed.
pub fn LineEditor_handleKey(this: &mut LineEditor, ch: i32) -> bool {
    match ch {
        KEY_LEFT | LE_CTRL_B => {
            moveCursorLeft(this);
            false
        }

        KEY_RIGHT | LE_CTRL_F => {
            moveCursorRight(this);
            false
        }

        KEY_HOME | LE_CTRL_A => {
            this.cursor = 0;
            false
        }

        KEY_END | LE_CTRL_E => {
            this.cursor = this.len;
            false
        }

        KEY_SLEFT => {
            // Shift+Left (ncurses stock) and Ctrl+Left (htop home grown)
            moveCursorWordLeft(this);
            false
        }

        KEY_SRIGHT => {
            // Shift+Right (ncurses stock) and Ctrl+Right (htop home grown)
            moveCursorWordRight(this);
            false
        }

        KEY_DC => deleteCharAt(this), // Delete

        KEY_BACKSPACE | 127 => deleteCharBefore(this), // DEL / Backspace in some terminals

        LE_CTRL_W => {
            // Delete word before cursor (like bash Ctrl-W)
            let end = this.cursor;
            // skip whitespace before cursor
            while this.cursor > 0
                && matches!(
                    this.buffer[this.cursor - 1],
                    b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'
                )
            {
                this.cursor -= 1;
            }
            // skip non-whitespace
            while this.cursor > 0
                && !matches!(
                    this.buffer[this.cursor - 1],
                    b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'
                )
            {
                this.cursor -= 1;
            }
            if this.cursor == end {
                return false;
            }
            let deleted = end - this.cursor;
            // memmove(buffer + cursor, buffer + end, len - end + 1)
            this.buffer.copy_within(end..this.len + 1, this.cursor);
            this.len -= deleted;
            true
        }

        LE_CTRL_K => {
            // Delete from cursor to end of line
            if this.cursor >= this.len {
                return false;
            }
            this.buffer[this.cursor] = b'\0';
            this.len = this.cursor;
            true
        }

        LE_CTRL_U => {
            // Delete from start of line to cursor
            if this.cursor == 0 {
                return false;
            }
            // memmove(buffer, buffer + cursor, len - cursor + 1)
            this.buffer.copy_within(this.cursor..this.len + 1, 0);
            this.len -= this.cursor;
            this.cursor = 0;
            true
        }

        _ => {
            if ch > 0 && ch < 256 && matches!(ch as u8, 0x20..=0x7e) {
                insertChar(this, ch as u8)
            } else {
                false
            }
        }
    }
}

/// Port of `LineEditor_updateScroll` from `LineEditor.c:197`.
///
/// Pure scroll-offset arithmetic: nudge `scroll` so `cursor` stays
/// within the `[scroll, scroll + fieldWidth)` visible window.
pub fn LineEditor_updateScroll(this: &mut LineEditor, fieldWidth: i32) {
    if fieldWidth <= 0 {
        return;
    }
    let fw = fieldWidth as usize;
    // Ensure cursor is visible
    if this.cursor < this.scroll {
        this.scroll = this.cursor;
    } else if this.cursor >= this.scroll + fw {
        this.scroll = this.cursor - fw + 1;
    }
}

/// Port of `LineEditor_draw` from `LineEditor.c:209`.
///
/// Emits through the `Ncurses` crossterm shim (`functionbar.rs`): the
/// C `attrset(CRT_colors[FUNCTION_BAR])` for `attr == -1` maps to
/// `ColorElements::FUNCTION_BAR.packed(ColorScheme::active())`, an
/// explicit `attr` is passed straight through, `LINES - 1` is
/// `Ncurses::lines() - 1`, and the bottom line is painted with
/// `mvaddnstr` (the visible buffer window) then `mvaddch` space-padding.
/// Returns the screen column `startX + (cursor - scroll)` where the
/// caller should place the cursor.
pub fn LineEditor_draw(this: &LineEditor, startX: i32, fieldWidth: i32, attr: i32) -> i32 {
    let mut out = io::stdout().lock();

    if attr == -1 {
        Ncurses::attrset(
            &mut out,
            ColorElements::FUNCTION_BAR.packed(ColorScheme::active()),
        );
    } else {
        Ncurses::attrset(&mut out, attr);
    }

    let line = Ncurses::lines() - 1;

    // Display the visible portion of the buffer.
    let mut visibleLen = this.len as i32 - this.scroll as i32;
    if visibleLen < 0 {
        visibleLen = 0;
    }
    if visibleLen > fieldWidth {
        visibleLen = fieldWidth;
    }
    // `visibleStart = buffer + scroll`, bounded to `visibleLen` bytes;
    // the clamp above keeps `scroll + visibleLen <= len`, so the slice is
    // always within the live text region.
    let end = this.scroll + visibleLen as usize;
    let visibleStart = std::str::from_utf8(&this.buffer[this.scroll..end]).unwrap_or("");
    Ncurses::mvaddnstr(&mut out, line, startX, visibleStart, visibleLen);

    // Pad remaining field with spaces.
    for i in visibleLen..fieldWidth {
        Ncurses::mvaddch(&mut out, line, startX + i, ' ');
    }

    let _ = out.flush();

    startX + (this.cursor as i32 - this.scroll as i32)
}

/// Port of `LineEditor_click` from `LineEditor.c:235`.
///
/// Pure cursor arithmetic: translate a screen-column click to a byte
/// position by adding the display `scroll` offset, clamping the negative
/// pre-field region to `0` and the tail past the text to `len`.
pub fn LineEditor_click(this: &mut LineEditor, clickX: i32, fieldStartX: i32) {
    let mut offset = clickX - fieldStartX;
    if offset < 0 {
        offset = 0;
    }
    let mut newCursor = this.scroll + offset as usize;
    if newCursor > this.len {
        newCursor = this.len;
    }
    this.cursor = newCursor;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read the live text region as a `String` for assertions.
    fn text(e: &LineEditor) -> String {
        String::from_utf8_lossy(&e.buffer[..e.len]).into_owned()
    }

    #[test]
    fn init_and_initWithMax() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        assert_eq!(e.maxLen, LINEEDITOR_MAX);
        assert_eq!(e.len, 0);
        assert_eq!(e.cursor, 0);
        assert_eq!(e.scroll, 0);
        assert_eq!(e.buffer[0], b'\0');

        // In-range custom max is kept.
        LineEditor_initWithMax(&mut e, 10);
        assert_eq!(e.maxLen, 10);
        // 0 clamps up to LINEEDITOR_MAX.
        LineEditor_initWithMax(&mut e, 0);
        assert_eq!(e.maxLen, LINEEDITOR_MAX);
        // Over-max clamps down to LINEEDITOR_MAX.
        LineEditor_initWithMax(&mut e, LINEEDITOR_MAX + 5);
        assert_eq!(e.maxLen, LINEEDITOR_MAX);
    }

    #[test]
    fn reset_clears_state() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "hello");
        e.scroll = 3;
        LineEditor_reset(&mut e);
        assert_eq!(e.len, 0);
        assert_eq!(e.cursor, 0);
        assert_eq!(e.scroll, 0);
        assert_eq!(e.buffer[0], b'\0');
    }

    #[test]
    fn setText_puts_cursor_at_end() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "hello");
        assert_eq!(text(&e), "hello");
        assert_eq!(e.len, 5);
        assert_eq!(e.cursor, 5);
        assert_eq!(e.scroll, 0);
    }

    #[test]
    fn setText_truncates_to_maxLen() {
        let mut e = LineEditor::default();
        LineEditor_initWithMax(&mut e, 3);
        LineEditor_setText(&mut e, "abcdef");
        assert_eq!(text(&e), "abc");
        assert_eq!(e.len, 3);
        assert_eq!(e.cursor, 3);
    }

    #[test]
    fn moveCursor_left_right_bounds() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "ab"); // cursor at 2
        moveCursorRight(&mut e); // clamped at len
        assert_eq!(e.cursor, 2);
        moveCursorLeft(&mut e);
        assert_eq!(e.cursor, 1);
        moveCursorLeft(&mut e);
        assert_eq!(e.cursor, 0);
        moveCursorLeft(&mut e); // clamped at 0
        assert_eq!(e.cursor, 0);
        moveCursorRight(&mut e);
        assert_eq!(e.cursor, 1);
    }

    #[test]
    fn moveCursorWordLeft_jumps_over_spaces_and_word() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "foo bar"); // cursor at 7 (end)
        moveCursorWordLeft(&mut e); // to start of "bar"
        assert_eq!(e.cursor, 4);
        moveCursorWordLeft(&mut e); // over the space then over "foo"
        assert_eq!(e.cursor, 0);
        moveCursorWordLeft(&mut e); // already at start
        assert_eq!(e.cursor, 0);
    }

    #[test]
    fn moveCursorWordLeft_from_within_trailing_spaces() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "ab   "); // trailing spaces, cursor at 5
        moveCursorWordLeft(&mut e); // skip 3 spaces, then skip "ab"
        assert_eq!(e.cursor, 0);
    }

    #[test]
    fn moveCursorWordRight_jumps_over_word_and_spaces() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "foo bar");
        e.cursor = 0;
        moveCursorWordRight(&mut e); // skip "foo" then the space -> start of "bar"
        assert_eq!(e.cursor, 4);
        moveCursorWordRight(&mut e); // skip "bar", no trailing space -> end
        assert_eq!(e.cursor, 7);
        moveCursorWordRight(&mut e); // at end, stays
        assert_eq!(e.cursor, 7);
    }

    #[test]
    fn deleteCharBefore_at_start_is_noop() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "ab");
        e.cursor = 0;
        assert!(!deleteCharBefore(&mut e));
        assert_eq!(text(&e), "ab");
        assert_eq!(e.len, 2);
    }

    #[test]
    fn deleteCharBefore_removes_and_moves_cursor() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "abc"); // cursor at 3
        assert!(deleteCharBefore(&mut e));
        assert_eq!(text(&e), "ab");
        assert_eq!(e.len, 2);
        assert_eq!(e.cursor, 2);
        assert_eq!(e.buffer[e.len], b'\0'); // NUL preserved by memmove
    }

    #[test]
    fn deleteCharBefore_in_middle() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "abc");
        e.cursor = 1; // char before cursor is 'a' (index 0)
        assert!(deleteCharBefore(&mut e)); // deletes 'a'
        assert_eq!(text(&e), "bc");
        assert_eq!(e.cursor, 0);
    }

    #[test]
    fn deleteCharAt_at_end_is_noop() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "ab"); // cursor at end == len
        assert!(!deleteCharAt(&mut e));
        assert_eq!(text(&e), "ab");
    }

    #[test]
    fn deleteCharAt_removes_current() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "abc");
        e.cursor = 1;
        assert!(deleteCharAt(&mut e)); // deletes 'b'
        assert_eq!(text(&e), "ac");
        assert_eq!(e.len, 2);
        assert_eq!(e.cursor, 1); // cursor unchanged
        assert_eq!(e.buffer[e.len], b'\0');
    }

    #[test]
    fn insertChar_in_middle() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "ac");
        e.cursor = 1;
        assert!(insertChar(&mut e, b'b'));
        assert_eq!(text(&e), "abc");
        assert_eq!(e.len, 3);
        assert_eq!(e.cursor, 2);
        assert_eq!(e.buffer[e.len], b'\0'); // trailing NUL shifted correctly
    }

    #[test]
    fn insertChar_at_maxLen_is_rejected() {
        let mut e = LineEditor::default();
        LineEditor_initWithMax(&mut e, 3);
        LineEditor_setText(&mut e, "abc"); // len == maxLen
        assert!(!insertChar(&mut e, b'd'));
        assert_eq!(text(&e), "abc");
        assert_eq!(e.len, 3);
    }

    #[test]
    fn insertChar_up_to_maxLen_boundary() {
        let mut e = LineEditor::default();
        LineEditor_initWithMax(&mut e, 3);
        LineEditor_setText(&mut e, "ab"); // len 2, room for one more
        assert!(insertChar(&mut e, b'c')); // now len 3 == maxLen
        assert_eq!(text(&e), "abc");
        assert!(!insertChar(&mut e, b'd')); // full
        assert_eq!(e.len, 3);
    }

    #[test]
    fn updateScroll_noop_on_nonpositive_width() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        e.cursor = 10;
        e.scroll = 4;
        LineEditor_updateScroll(&mut e, 0);
        assert_eq!(e.scroll, 4);
        LineEditor_updateScroll(&mut e, -1);
        assert_eq!(e.scroll, 4);
    }

    #[test]
    fn updateScroll_scrolls_right_when_cursor_past_window() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        e.cursor = 10;
        e.scroll = 0;
        LineEditor_updateScroll(&mut e, 5); // cursor >= scroll + 5
        assert_eq!(e.scroll, 6); // 10 - 5 + 1
    }

    #[test]
    fn updateScroll_scrolls_left_when_cursor_before_window() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        e.cursor = 3;
        e.scroll = 6;
        LineEditor_updateScroll(&mut e, 5); // cursor < scroll
        assert_eq!(e.scroll, 3);
    }

    #[test]
    fn updateScroll_no_change_when_cursor_visible() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        e.cursor = 4;
        e.scroll = 2;
        LineEditor_updateScroll(&mut e, 5); // 2 <= 4 < 7
        assert_eq!(e.scroll, 2);
    }

    #[test]
    fn getText_returns_buffer_text() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        assert_eq!(LineEditor_getText(&e), "");
        LineEditor_setText(&mut e, "hello world");
        assert_eq!(LineEditor_getText(&e), "hello world");
        // Reflects edits: delete the trailing 'd'.
        e.cursor = e.len;
        assert!(deleteCharBefore(&mut e));
        assert_eq!(LineEditor_getText(&e), "hello worl");
    }

    #[test]
    fn handleKey_printable_inserts_and_advances_cursor() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        // A printable char inserts and returns true (content changed).
        assert!(LineEditor_handleKey(&mut e, b'a' as i32));
        assert!(LineEditor_handleKey(&mut e, b'b' as i32));
        assert_eq!(text(&e), "ab");
        assert_eq!(e.len, 2);
        assert_eq!(e.cursor, 2); // cursor advanced past each insert
                                 // A non-printable control byte (that has no dedicated arm) is ignored.
        assert!(!LineEditor_handleKey(&mut e, 0x01_000)); // out of 0..256 range
        assert_eq!(text(&e), "ab");
    }

    #[test]
    fn handleKey_backspace_deletes_before_cursor() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "abc"); // cursor at end (3)
        assert!(LineEditor_handleKey(&mut e, KEY_BACKSPACE));
        assert_eq!(text(&e), "ab");
        assert_eq!(e.cursor, 2);
        // 127 (DEL) is the same backspace path.
        assert!(LineEditor_handleKey(&mut e, 127));
        assert_eq!(text(&e), "a");
        // At start, backspace is a no-op returning false.
        e.cursor = 0;
        assert!(!LineEditor_handleKey(&mut e, KEY_BACKSPACE));
        assert_eq!(text(&e), "a");
    }

    #[test]
    fn handleKey_delete_removes_char_at_cursor() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "abc");
        e.cursor = 1;
        assert!(LineEditor_handleKey(&mut e, KEY_DC)); // deletes 'b'
        assert_eq!(text(&e), "ac");
        assert_eq!(e.cursor, 1);
    }

    #[test]
    fn handleKey_arrows_move_cursor_without_changing_text() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "abc"); // cursor 3
        assert!(!LineEditor_handleKey(&mut e, KEY_LEFT)); // false: no content change
        assert_eq!(e.cursor, 2);
        assert!(!LineEditor_handleKey(&mut e, KEY_LEFT));
        assert_eq!(e.cursor, 1);
        assert!(!LineEditor_handleKey(&mut e, KEY_RIGHT));
        assert_eq!(e.cursor, 2);
        // HOME / END jump to the ends.
        assert!(!LineEditor_handleKey(&mut e, KEY_HOME));
        assert_eq!(e.cursor, 0);
        assert!(!LineEditor_handleKey(&mut e, KEY_END));
        assert_eq!(e.cursor, 3);
        assert_eq!(text(&e), "abc"); // text untouched by movement
    }

    #[test]
    fn handleKey_ctrl_w_deletes_word_before_cursor() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "foo bar"); // cursor at end
        assert!(LineEditor_handleKey(&mut e, KEY_CTRL(b'W' as i32)));
        assert_eq!(text(&e), "foo ");
        assert_eq!(e.cursor, 4);
    }

    #[test]
    fn getCursor_reads_cursor_position() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        assert_eq!(LineEditor_getCursor(&e), 0);
        LineEditor_setText(&mut e, "hello"); // cursor moves to end
        assert_eq!(LineEditor_getCursor(&e), 5);
        moveCursorWordLeft(&mut e); // back to start of "hello"
        assert_eq!(LineEditor_getCursor(&e), 0);
    }

    #[test]
    fn click_maps_column_to_cursor_with_scroll() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "abcdef"); // len 6
        e.scroll = 0;
        // Click at column 3 in a field starting at column 0 -> cursor 3.
        LineEditor_click(&mut e, 3, 0);
        assert_eq!(e.cursor, 3);
        // Click before the field start clamps offset to 0 -> cursor = scroll.
        e.scroll = 2;
        LineEditor_click(&mut e, 1, 5); // offset -4 -> 0
        assert_eq!(e.cursor, 2); // scroll + 0
                                 // Scroll offset is added to the in-field offset.
        LineEditor_click(&mut e, 8, 5); // offset 3, scroll 2 -> 5
        assert_eq!(e.cursor, 5);
        // Clicking past the text clamps to len.
        LineEditor_click(&mut e, 100, 5);
        assert_eq!(e.cursor, 6); // len
    }

    #[test]
    fn draw_returns_cursor_screen_column() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "hello"); // cursor at 5
        e.scroll = 0;
        // cursorX = startX + (cursor - scroll) = 10 + (5 - 0).
        let cursor_x = LineEditor_draw(&e, 10, 20, -1);
        assert_eq!(cursor_x, 15);
        // With a scroll offset the returned column shifts left accordingly.
        e.scroll = 2;
        let cursor_x = LineEditor_draw(&e, 10, 20, 0);
        assert_eq!(cursor_x, 13); // 10 + (5 - 2)
    }

    #[test]
    fn handleKey_ctrl_k_and_ctrl_u_kill_line_regions() {
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "abcdef");
        e.cursor = 3;
        // Ctrl-K: delete from cursor to end -> "abc".
        assert!(LineEditor_handleKey(&mut e, KEY_CTRL(b'K' as i32)));
        assert_eq!(text(&e), "abc");
        assert_eq!(e.len, 3);
        // Ctrl-U: delete from start to cursor -> "" (cursor still at 3).
        assert!(LineEditor_handleKey(&mut e, KEY_CTRL(b'U' as i32)));
        assert_eq!(text(&e), "");
        assert_eq!(e.cursor, 0);
    }
}
