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
//! `LineEditor_handleKey` (:118), `LineEditor_draw` (:209), and
//! `LineEditor_click` (:235) depend on ncurses drawing / ncurses key
//! constants (`KEY_LEFT`, `attrset`, `mvaddnstr`, `LINES`, …) — that
//! substrate is not ported, so those three remain `todo!()` stubs.
#![allow(non_snake_case)]
#![allow(dead_code)]

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

/// TODO: port of `bool LineEditor_handleKey(LineEditor* this, int ch` from `LineEditor.c:118`.
pub fn LineEditor_handleKey() {
    todo!("port of LineEditor.c:118")
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

/// TODO: port of `int LineEditor_draw(LineEditor* this, int startX, int fieldWidth, int attr` from `LineEditor.c:209`.
pub fn LineEditor_draw() {
    todo!("port of LineEditor.c:209")
}

/// TODO: port of `void LineEditor_click(LineEditor* this, int clickX, int fieldStartX` from `LineEditor.c:235`.
pub fn LineEditor_click() {
    todo!("port of LineEditor.c:235")
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
}
