//! Port of `RichString.c` — htop's in-memory styled-character buffer.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Data model
//!
//! htop's `RichString` (`RichString.h:42`) is
//! `{ int chlen; CharType* chptr; CharType chstr[RICHSTRING_MAXLEN + 1];
//! int highlightAttr; }` where `CharType` is a curses cell carrying a
//! character plus its ncurses attribute. `chptr` points into the inline
//! `chstr` buffer while the string fits, and at a heap allocation once it
//! outgrows `RICHSTRING_MAXLEN`.
//!
//! In safe Rust the inline-vs-heap split collapses: [`RichString::chptr`]
//! is a single owned `Vec<RichCell>` that subsumes both `chstr` and the
//! overflow allocation. `Vec` owns its storage, so the `xMalloc` /
//! `xRealloc` / `free` / `memcpy` bookkeeping in `RichString_extendLen`
//! and the heap release in `RichString_delete` become plain `Vec`
//! operations (and `Vec`'s own `Drop` is the real deallocator). The
//! logical contract is preserved exactly: cells `[0, chlen)` are valid
//! and index `chlen` holds a null-terminator cell (`chars = '\0'`,
//! `attr = 0`), matching the `RichString_setChar(this, len, 0)` the C
//! code writes after every length change.
//!
//! Each [`RichCell`] stores `chars` — the primary code point, i.e.
//! `cchar_t.chars[0]` in the `HAVE_LIBNCURSESW` build — plus the opaque
//! ncurses `attr: i32` (a packed color-pair | attribute-bits value). The
//! attribute is stored verbatim and is NOT resolved to a color here; the
//! draw layer resolves it later. htop's two build variants (ncursesw
//! `cchar_t` vs. plain `chtype`) have identical observable behavior — the
//! wide variant is the primary and is what is ported.
//!
//! # Unicode substrate
//!
//! htop leans on three libc primitives:
//!
//! - `wcwidth`: display-column width, needed by
//!   [`RichString_appendnWideColumns`]. Ported faithfully in [`wcwidth`]
//!   via the `unicode-width` crate (a pure-Rust, vendorable Unicode
//!   width-table implementation): wide CJK/emoji code points count as 2
//!   columns, combining / zero-width marks as 0, normal characters as 1,
//!   and control characters as `-1` — reproducing libc `wcwidth(3)` in a
//!   UTF-8 (non-CJK) locale. So `RichString_appendnWideColumns` breaks on
//!   the real column budget for wide text, matching htop: a `'世'`
//!   (`wcwidth == 2`) consumes two columns and is skipped when only one
//!   column remains, exactly as the C loop's `cwidth > *columns` test
//!   dictates.
//! - `iswprint` / `isprint`: character-printability classification used to
//!   replace non-printable input with `U+FFFD`. `isprint` is ported
//!   exactly (C-locale printable range `0x20..=0x7e`). `iswprint` is
//!   approximated as `!char::is_control()`. The `unicode-width` crate was
//!   evaluated as a tightening: its `width()` returns `None` for exactly
//!   the Unicode `Cc` control range — the same set as `char::is_control()`
//!   — so `width().is_some()` is identical to `!char::is_control()` and
//!   offers no fidelity gain (verified against v0.2.2: NUL, C0, and C1
//!   controls all map to `None`; space, ASCII, combining marks, PUA, wide
//!   CJK, emoji, and noncharacters all map to `Some(_)`). Both
//!   approximations agree with glibc for the C0/C1 control range htop's
//!   replacement targets and diverge only for exotic unassigned /
//!   noncharacter code points that do not occur in htop's real input
//!   paths. `iswprint` is therefore left unchanged (no regression), not
//!   switched to a table that would classify identically.
//!
//! `mbstowcs_nonfatal` (the multibyte→wide decode) is ported faithfully:
//! it decodes UTF-8, emits exactly one `U+FFFD` per contiguous run of
//! invalid bytes, and stops at a NUL — mirroring the `mbrtowc` loop.
//!
//! # Terminal blit
//!
//! [`RichString_printoffnVal`] is the `RichString_printoffnVal` macro
//! (`RichString.h:28`, `HAVE_LIBNCURSESW` variant) — `mvadd_wchnstr(y, x,
//! chptr + off, n)` — a pure ncurses blit that paints `n` styled cells to
//! the screen. It has no locale/string logic to port; it is a behavioral
//! crossterm port through the crate's [`Ncurses`] emit shim, per-cell
//! `attrset` + `mvaddch`, exactly mirroring the established
//! `Panel::print_offset` blit (`Panel.c` draw path). The companion
//! whole-string macro `RichString_printVal` (`RichString.h:27`,
//! `mvadd_wchstr`) is NOT ported: it is absent from the htop C-name
//! snapshot (no call site records it), so exposing it as a `pub fn` would
//! be a non-htop name that the port-purity gate rejects. `RichString_printAt`
//! does not exist in htop 3.5.1.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::io::Write;

use unicode_width::UnicodeWidthChar;

use crate::ported::functionbar::Ncurses;

/// `RICHSTRING_MAXLEN` from `RichString.h:40`. The size (minus the
/// terminator slot) of htop's inline `chstr` buffer and the threshold at
/// which the C code switches `chptr` to a heap allocation.
pub const RICHSTRING_MAXLEN: usize = 350;

/// One styled character cell — the safe-Rust analog of htop's `CharType`
/// (`cchar_t` in the `HAVE_LIBNCURSESW` build, `chtype` otherwise).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct RichCell {
    /// The primary code point — `cchar_t.chars[0]`. The null cell (`'\0'`)
    /// is the string terminator.
    pub chars: char,
    /// The opaque ncurses attribute (packed color-pair | attribute bits),
    /// stored verbatim; resolved to a color by the draw layer, not here.
    pub attr: i32,
}

/// Port of htop's `RichString` struct (`RichString.h:42`). The inline
/// `chstr` buffer and the overflow `chptr` allocation are collapsed into
/// the single owned [`RichString::chptr`] `Vec` (see the module docs);
/// `chlen` and `highlightAttr` are preserved as-is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RichString {
    /// Number of valid characters (`this->chlen`). Cells `[0, chlen)` are
    /// valid; index `chlen` is the null terminator.
    pub chlen: i32,
    /// The character/attribute cells. Owns both what C splits across the
    /// inline `chstr` buffer and the heap overflow buffer.
    pub chptr: Vec<RichCell>,
    /// The highlight attribute (`this->highlightAttr`), set by the display
    /// layer; carried verbatim.
    pub highlightAttr: i32,
}

impl RichString {
    /// Rust analog of the `RichString_beginAllocated` macro
    /// (`RichString.h:18`): `chlen = 0`, `chptr` points at a buffer holding
    /// a single null terminator (`RichString_setChar(this, 0, 0)`), and
    /// `highlightAttr = 0`.
    pub fn new() -> Self {
        RichString {
            chlen: 0,
            chptr: vec![RichCell::default()],
            highlightAttr: 0,
        }
    }

    /// Approximation of the libc `iswprint(3)` call sites in
    /// `RichString.c` (lines 112, 130): treats every non-control character
    /// as printable. See the module docs for the precise divergence from
    /// glibc. Used only to select the `U+FFFD` replacement.
    fn iswprint(c: char) -> bool {
        !c.is_control()
    }

    /// Port of the libc `isprint((unsigned char)…)` call site in
    /// `RichString.c:153` for the C locale: printable iff in the range
    /// `0x20..=0x7e`.
    fn isprint(b: u8) -> bool {
        (0x20..=0x7e).contains(&b)
    }
}

impl Default for RichString {
    fn default() -> Self {
        Self::new()
    }
}

/// Port of the `RichString_setChar(this, at, ch)` macro (`RichString.h:30`,
/// `HAVE_LIBNCURSESW` variant): `this->chptr[at] = (CharType){ .chars = {
/// ch, 0 } }`, i.e. the cell's attribute is zeroed. Exposed as a free fn
/// because `RichString_setChar` appears in the C-name snapshot (invoked in
/// `RichString.c` and `Meter.c`), so the meter/panel draw layer calls it by
/// this exact name.
///
/// Grows the owned buffer so index `at` exists — in C this is always
/// in-bounds because `chstr` is physically `RICHSTRING_MAXLEN + 1` cells and
/// `extendLen` sizes the heap buffer; here the buffer grows on demand,
/// filling any gap with null cells (which callers overwrite before reading).
pub fn RichString_setChar(this: &mut RichString, at: usize, ch: char) {
    if at >= this.chptr.len() {
        this.chptr.resize(at + 1, RichCell::default());
    }
    this.chptr[at] = RichCell { chars: ch, attr: 0 };
}

/// Port of the `RichString_getCharVal(this, i)` macro (`RichString.h:29`,
/// `HAVE_LIBNCURSESW` variant): `(this).chptr[i].chars[0]` — the primary
/// code point of cell `i`. Exposed as a free fn because `RichString_getCharVal`
/// appears in the C-name snapshot (invoked in `Meter.c` and `Table.c`).
pub fn RichString_getCharVal(this: &RichString, i: usize) -> char {
    this.chptr[i].chars
}

/// Port of the `RichString_size(this)` macro (`RichString.h:14`):
/// `(this)->chlen`. Exposed as a free fn because `RichString_size` appears in
/// the C-name snapshot (invoked in `Panel.c`, `Process.c`, `Row.c`,
/// `Table.c`). In C this macro takes a pointer; the value-taking companion is
/// [`RichString_sizeVal`]. That pointer-vs-value distinction has no analog in
/// safe Rust, so both take `&RichString` and return `chlen`.
pub fn RichString_size(this: &RichString) -> i32 {
    this.chlen
}

/// Port of the `RichString_sizeVal(this)` macro (`RichString.h:15`):
/// `(this).chlen`. Exposed as a free fn because `RichString_sizeVal` appears
/// in the C-name snapshot (invoked in `Meter.c` and `Panel.c`). Identical
/// body to [`RichString_size`]; see that fn for the pointer-vs-value note.
pub fn RichString_sizeVal(this: &RichString) -> i32 {
    this.chlen
}

/// Port of the libc `wcwidth(3)` call at `RichString.c:131`
/// (`int cwidth = wcwidth(c);`). Reproduces `wcwidth` in a UTF-8 (non-CJK)
/// locale via the `unicode-width` crate's Unicode width tables:
/// `UnicodeWidthChar::width` returns `None` for control characters (mapped
/// to `-1`, as libc does for non-printable input), `Some(0)` for combining /
/// zero-width marks, `Some(2)` for wide CJK/emoji code points, and `Some(1)`
/// for normal characters. Exposed as a free fn because `wcwidth` appears in
/// the C-name snapshot (invoked in `RichString.c`).
pub fn wcwidth(wc: char) -> i32 {
    match UnicodeWidthChar::width(wc) {
        Some(w) => w as i32,
        None => -1,
    }
}

/// Port of `RichString.c:24` (`static void RichString_extendLen`). Ensures
/// the buffer can hold `len` characters plus a terminator, writes the null
/// terminator at index `len`, and sets `chlen = len`. In C this chooses
/// between the inline `chstr` buffer and a `xMalloc`/`xRealloc` heap buffer
/// (with `memcpy`/`free` bookkeeping); the owned `Vec` subsumes both, so
/// those branches collapse into the single buffer growth done by
/// [`RichString_setChar`].
pub fn RichString_extendLen(this: &mut RichString, len: usize) {
    RichString_setChar(this, len, '\0');
    this.chlen = len as i32;
}

/// Port of `RichString.c:52` (`static void RichString_setLen`). Fast path
/// when both the new and current lengths are below `RICHSTRING_MAXLEN`
/// (in C, the inline buffer always suffices): write the terminator and set
/// `chlen`. Otherwise defer to [`RichString_extendLen`].
pub fn RichString_setLen(this: &mut RichString, len: usize) {
    if len < RICHSTRING_MAXLEN && (this.chlen as usize) < RICHSTRING_MAXLEN {
        RichString_setChar(this, len, '\0');
        this.chlen = len as i32;
    } else {
        RichString_extendLen(this, len);
    }
}

/// Port of `RichString.c:61`. Shortens the string by `count` characters via
/// `RichString_setLen(this, this->chlen - count)`. As in C, `chlen - count`
/// is computed in signed arithmetic and passed as an (unsigned) length;
/// callers must keep `count <= chlen`.
pub fn RichString_rewind(this: &mut RichString, count: i32) {
    RichString_setLen(this, (this.chlen - count) as usize);
}

/// Port of `RichString.c:67` (`static size_t mbstowcs_nonfatal`). Decodes
/// the multibyte (UTF-8) bytes in `src` to wide characters, emitting
/// exactly one `U+FFFD` per contiguous run of invalid bytes and stopping at
/// a NUL byte — mirroring the `mbrtowc` loop with its `broken` flag and its
/// `ret == 0` break. Returns the decoded characters (the C out-param
/// `dest`; the C return value is this `Vec`'s length).
pub fn mbstowcs_nonfatal(src: &[u8]) -> Vec<char> {
    /// Decode a single UTF-8 sequence at the start of `b`. Returns the
    /// character and its byte length, or `None` for an invalid/incomplete
    /// sequence (`mbrtowc` returning `(size_t)-1` / `(size_t)-2`). Rejects
    /// overlong encodings, surrogates, and code points above `U+10FFFF`,
    /// matching a UTF-8 locale's `mbrtowc`.
    fn utf8_decode_one(b: &[u8]) -> Option<(char, usize)> {
        let b0 = b[0];
        if b0 < 0x80 {
            return Some((b0 as char, 1));
        }
        let (len, min, init) = if (0xc2..=0xdf).contains(&b0) {
            (2usize, 0x80u32, (b0 as u32) & 0x1f)
        } else if (0xe0..=0xef).contains(&b0) {
            (3, 0x800, (b0 as u32) & 0x0f)
        } else if (0xf0..=0xf4).contains(&b0) {
            (4, 0x10000, (b0 as u32) & 0x07)
        } else {
            return None; // continuation byte, overlong lead (0xc0/0xc1), or > 0xf4
        };
        if b.len() < len {
            return None;
        }
        let mut cp = init;
        for &bk in &b[1..len] {
            if !(0x80..=0xbf).contains(&bk) {
                return None;
            }
            cp = (cp << 6) | ((bk as u32) & 0x3f);
        }
        if cp < min || cp > 0x10_ffff || (0xd800..=0xdfff).contains(&cp) {
            return None;
        }
        char::from_u32(cp).map(|c| (c, len))
    }

    let mut out: Vec<char> = Vec::new();
    let mut broken = false;
    let mut i = 0;
    while i < src.len() {
        match utf8_decode_one(&src[i..]) {
            Some((c, adv)) => {
                if c == '\0' {
                    break; // ret == 0: NUL terminates the decode
                }
                broken = false;
                out.push(c);
                i += adv;
            }
            None => {
                if !broken {
                    broken = true;
                    out.push('\u{FFFD}');
                }
                i += 1;
            }
        }
    }
    out
}

/// Port of `RichString.c:100` (`static inline int
/// RichString_writeFromWide`, `HAVE_LIBNCURSESW` variant). Decodes `len`
/// bytes of `data_c` via [`mbstowcs_nonfatal`], grows the string to
/// `from + decoded`, and writes each cell as the printable code point (or
/// `U+FFFD`) with `attrs & 0xffffff`. Returns the number of characters
/// written (the decoded wide count). `data_c` is htop's `const char*`
/// modeled as a byte slice; `len` bytes are read (clamped to the slice to
/// stay memory-safe — C reads exactly `len`).
pub fn RichString_writeFromWide(
    this: &mut RichString,
    attrs: i32,
    data_c: &[u8],
    from: i32,
    len: usize,
) -> i32 {
    if len < 1 {
        return 0;
    }
    let data = mbstowcs_nonfatal(&data_c[..len.min(data_c.len())]);
    let wlen = data.len();
    if wlen == 0 {
        return 0;
    }
    let new_len = from as usize + wlen;
    RichString_setLen(this, new_len);
    let mut j = 0usize;
    for i in (from as usize)..new_len {
        let c = if RichString::iswprint(data[j]) {
            data[j]
        } else {
            '\u{FFFD}'
        };
        this.chptr[i] = RichCell {
            chars: c,
            attr: attrs & 0xffffff,
        };
        j += 1;
    }
    wlen as i32
}

/// Port of `RichString.c:118` (`RichString_appendnWideColumns`,
/// `HAVE_LIBNCURSESW` variant). Appends up to `*columns` display columns of
/// decoded text, stopping before any character that would overflow the
/// budget. Sets `*columns` to the number of columns written and returns the
/// number of characters written.
///
/// The `wcwidth`-based column accounting is ported line-for-line: each
/// decoded (post-`iswprint`-replacement) code point's display width comes
/// from [`wcwidth`], the loop breaks before a character whose width exceeds
/// the remaining budget (`cwidth > *columns`), the string is truncated to
/// what fit, and `*columns` returns the number of columns written. Wide
/// (CJK/emoji, width 2) and combining (width 0) code points are accounted
/// exactly as htop does.
pub fn RichString_appendnWideColumns(
    this: &mut RichString,
    attrs: i32,
    data_c: &[u8],
    len: usize,
    columns: &mut i32,
) -> i32 {
    let data = mbstowcs_nonfatal(&data_c[..len.min(data_c.len())]);
    let wlen = data.len();
    if wlen == 0 {
        return 0;
    }
    let from = this.chlen;
    let new_len = from as usize + wlen;
    RichString_setLen(this, new_len);
    let mut columns_written = 0;
    let mut pos = from as usize;
    for j in 0..wlen {
        let c = if RichString::iswprint(data[j]) {
            data[j]
        } else {
            '\u{FFFD}'
        };
        let cwidth = wcwidth(c);
        if cwidth > *columns {
            break;
        }
        *columns -= cwidth;
        columns_written += cwidth;
        this.chptr[pos] = RichCell {
            chars: c,
            attr: attrs & 0xffffff,
        };
        pos += 1;
    }
    RichString_setLen(this, pos);
    *columns = columns_written;
    (pos - from as usize) as i32
}

/// Port of `RichString.c:148` (`static inline int
/// RichString_writeFromAscii`, `HAVE_LIBNCURSESW` variant). Grows the
/// string to `from + len` and writes each byte as its ASCII character (or
/// `U+FFFD` when not printable) with `attrs & 0xffffff`. The C
/// `assert((unsigned char)data[j] <= SCHAR_MAX)` becomes a `debug_assert`.
/// Returns `len`.
pub fn RichString_writeFromAscii(
    this: &mut RichString,
    attrs: i32,
    data: &[u8],
    from: i32,
    len: usize,
) -> i32 {
    let new_len = from as usize + len;
    RichString_setLen(this, new_len);
    let mut j = 0usize;
    for i in (from as usize)..new_len {
        let b = data[j];
        debug_assert!(b <= 0x7f, "RichString ASCII input byte > SCHAR_MAX");
        let c = if RichString::isprint(b) {
            b as char
        } else {
            '\u{FFFD}'
        };
        this.chptr[i] = RichCell {
            chars: c,
            attr: attrs & 0xffffff,
        };
        j += 1;
    }
    len as i32
}

/// Port of `RichString.c:159` (`RichString_setAttrn`, `HAVE_LIBNCURSESW`
/// variant). Applies `attrs` to the cells in `[start, start + charcount)`,
/// clamped to `[0, chlen)`. The attribute is stored verbatim (no
/// `0xffffff` mask — matching the C body). `CLAMP` with a lower bound of 0
/// on the unsigned sum reduces to `min(start + charcount, chlen)`.
pub fn RichString_setAttrn(this: &mut RichString, attrs: i32, start: usize, charcount: usize) {
    let end = (start + charcount).min(this.chlen as usize);
    for i in start..end {
        this.chptr[i].attr = attrs;
    }
}

/// Port of `RichString.c:166` (`RichString_appendChr`, `HAVE_LIBNCURSESW`
/// variant). Appends the character `c` `count` times with attribute `attrs`
/// stored verbatim (no `0xffffff` mask, and no printability replacement —
/// matching the C body).
pub fn RichString_appendChr(this: &mut RichString, attrs: i32, c: char, count: i32) {
    let from = this.chlen;
    let new_len = from + count;
    RichString_setLen(this, new_len as usize);
    for i in (from as usize)..(new_len as usize) {
        this.chptr[i] = RichCell {
            chars: c,
            attr: attrs,
        };
    }
}

/// Port of `RichString.c:175` (`HAVE_LIBNCURSESW` variant). Searches the
/// character sequence for `c` starting at index `start`, returning the
/// first matching index or `-1` when absent. The C code widens the byte
/// with `btowc(c)` and compares against `chars[0]`; the non-ncursesw
/// variant (`RichString.c:226`, `(*ch & 0xff) == (chtype)c`) has identical
/// search behavior. The loop runs over `[start, chlen)`, so a `start` at or
/// past `chlen` — and an empty string — returns `-1` immediately.
pub fn RichString_findChar(this: &RichString, c: char, start: i32) -> i32 {
    // `const wchar_t wc = btowc(c)` — widen the search byte.
    let wc = c;
    let mut i = start;
    while i < this.chlen {
        if this.chptr[i as usize].chars == wc {
            return i;
        }
        i += 1;
    }
    -1
}

/// Port of `RichString.c:238`. In C this releases the heap overflow buffer
/// (`if (chlen > RICHSTRING_MAXLEN) { free(chptr); chptr = chstr; }`) and
/// repoints `chptr` at the inline `chstr`. The owned [`RichString::chptr`]
/// `Vec` is released by its own `Drop`, so no manual free is needed; this
/// faithful analog drops the overflow buffer back to the begin state
/// (`chlen = 0`, a single terminator cell) when the string had outgrown
/// `RICHSTRING_MAXLEN`. C leaves `chlen` stale after `delete`, but the
/// object is being discarded either way.
pub fn RichString_delete(this: &mut RichString) {
    if this.chlen > RICHSTRING_MAXLEN as i32 {
        this.chptr = vec![RichCell::default()];
        this.chlen = 0;
    }
}

/// Port of `RichString.c:245`. Applies `attrs` to the whole string via
/// [`RichString_setAttrn`]`(this, attrs, 0, chlen)`.
pub fn RichString_setAttr(this: &mut RichString, attrs: i32) {
    RichString_setAttrn(this, attrs, 0, this.chlen as usize);
}

/// Port of `RichString.c:249`. Appends the NUL-terminated wide string
/// `data` (all its bytes) at the current end. Returns the number of
/// characters written.
pub fn RichString_appendWide(this: &mut RichString, attrs: i32, data: &[u8]) -> i32 {
    let from = this.chlen;
    RichString_writeFromWide(this, attrs, data, from, data.len())
}

/// Port of `RichString.c:253`. Appends the first `len` bytes of `data` (as
/// wide/UTF-8) at the current end. Returns the number of characters
/// written.
pub fn RichString_appendnWide(this: &mut RichString, attrs: i32, data: &[u8], len: usize) -> i32 {
    let from = this.chlen;
    RichString_writeFromWide(this, attrs, data, from, len)
}

/// Port of `RichString.c:257`. Overwrites the string from index 0 with the
/// wide/UTF-8 decode of all of `data`. Returns the number of characters
/// written.
pub fn RichString_writeWide(this: &mut RichString, attrs: i32, data: &[u8]) -> i32 {
    RichString_writeFromWide(this, attrs, data, 0, data.len())
}

/// Port of `RichString.c:261`. Appends the NUL-terminated ASCII string
/// `data` (all its bytes) at the current end. Returns the number of
/// characters written.
pub fn RichString_appendAscii(this: &mut RichString, attrs: i32, data: &[u8]) -> i32 {
    let from = this.chlen;
    RichString_writeFromAscii(this, attrs, data, from, data.len())
}

/// Port of `RichString.c:265`. Appends the first `len` bytes of `data` (as
/// ASCII) at the current end. Returns the number of characters written.
pub fn RichString_appendnAscii(this: &mut RichString, attrs: i32, data: &[u8], len: usize) -> i32 {
    let from = this.chlen;
    RichString_writeFromAscii(this, attrs, data, from, len)
}

/// Port of `RichString.c:269`. Overwrites the string from index 0 with the
/// ASCII bytes of all of `data`. Returns the number of characters written.
pub fn RichString_writeAscii(this: &mut RichString, attrs: i32, data: &[u8]) -> i32 {
    RichString_writeFromAscii(this, attrs, data, 0, data.len())
}

/// Port of the `RichString_printoffnVal(this, y, x, off, n)` macro
/// (`RichString.h:28`, `HAVE_LIBNCURSESW` variant):
/// `mvadd_wchnstr(y, x, (this).chptr + (off), n)`. Blits `n` styled cells
/// starting at cell `off` to screen position `(y, x)`, each cell carrying
/// its own attribute (as `mvadd_wchnstr` writes `cchar_t`s that embed their
/// attributes rather than using the current `attrset`). Exposed as a free
/// fn because `RichString_printoffnVal` appears in the C-name snapshot
/// (invoked in `Meter.c` and `Panel.c`).
///
/// Behavioral crossterm port: the ncurses blit has no locale/string logic,
/// so it is reproduced through the crate's [`Ncurses`] emit shim exactly as
/// the `Panel::print_offset` draw helper does — set each cell's own
/// attribute, then print its character. `out` is the crossterm draw target
/// standing in for ncurses' implicit `stdscr`. Cells past the end of the
/// backing buffer stop the blit (`mvadd_wchnstr` stops at the string's
/// terminating null cell; callers pass `n` bounded by `chlen`).
pub fn RichString_printoffnVal<W: Write>(
    out: &mut W,
    this: &RichString,
    y: i32,
    x: i32,
    off: i32,
    n: i32,
) {
    for k in 0..n {
        let idx = (off + k) as usize;
        if idx >= this.chptr.len() {
            break;
        }
        let cell = this.chptr[idx];
        Ncurses::attrset(out, cell.attr);
        Ncurses::mvaddch(out, y, x + k, cell.chars);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a RichString from an ASCII string (via the ported append).
    fn ascii(s: &str) -> RichString {
        let mut r = RichString::new();
        RichString_appendAscii(&mut r, 0, s.as_bytes());
        r
    }

    /// The visible characters of the valid `[0, chlen)` range.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    // ── invariants ────────────────────────────────────────────────────

    #[test]
    fn new_is_empty_with_terminator() {
        let r = RichString::new();
        assert_eq!(r.chlen, 0);
        assert_eq!(r.highlightAttr, 0);
        assert_eq!(r.chptr[0], RichCell::default());
        assert_eq!(r.chptr[0].chars, '\0');
    }

    #[test]
    fn terminator_follows_valid_range() {
        let mut r = RichString::new();
        RichString_appendAscii(&mut r, 0, b"abc");
        assert_eq!(r.chlen, 3);
        // cell at index chlen is the null terminator
        assert_eq!(r.chptr[r.chlen as usize].chars, '\0');
    }

    // ── ASCII append/write ────────────────────────────────────────────

    #[test]
    fn append_ascii_stores_each_byte_verbatim() {
        let mut r = RichString::new();
        let n = RichString_appendAscii(&mut r, 7, b"Hi!");
        assert_eq!(n, 3);
        assert_eq!(text(&r), "Hi!");
        for i in 0..3 {
            assert_eq!(r.chptr[i].chars, "Hi!".as_bytes()[i] as char);
            assert_eq!(r.chptr[i].attr, 7);
        }
    }

    #[test]
    fn append_ascii_replaces_nonprintable_with_replacement_char() {
        let mut r = RichString::new();
        // 0x1f (unit separator) and 0x7f (DEL) are not printable -> U+FFFD
        RichString_appendAscii(&mut r, 0, &[b'a', 0x1f, b'b', 0x7f]);
        assert_eq!(text(&r), "a\u{FFFD}b\u{FFFD}");
    }

    #[test]
    fn append_ascii_appends_after_existing() {
        let mut r = ascii("foo");
        RichString_appendAscii(&mut r, 0, b"bar");
        assert_eq!(text(&r), "foobar");
        assert_eq!(r.chlen, 6);
    }

    #[test]
    fn write_ascii_overwrites_from_zero() {
        let mut r = ascii("longtext");
        let n = RichString_writeAscii(&mut r, 0, b"hi");
        assert_eq!(n, 2);
        assert_eq!(text(&r), "hi");
        assert_eq!(r.chlen, 2);
    }

    #[test]
    fn appendn_ascii_honors_len() {
        let mut r = RichString::new();
        let n = RichString_appendnAscii(&mut r, 0, b"abcdef", 3);
        assert_eq!(n, 3);
        assert_eq!(text(&r), "abc");
    }

    // ── attribute masking (writeFrom* mask 0xffffff; appendChr/setAttr do not)

    #[test]
    fn write_from_ascii_masks_attr_to_24_bits() {
        let mut r = RichString::new();
        RichString_appendAscii(&mut r, 0x1234_5678, b"x");
        assert_eq!(r.chptr[0].attr, 0x0034_5678); // 0x12345678 & 0xffffff
    }

    #[test]
    fn append_chr_stores_attr_verbatim_no_mask() {
        let mut r = RichString::new();
        RichString_appendChr(&mut r, 0x1234_5678, '=', 3);
        assert_eq!(text(&r), "===");
        for i in 0..3 {
            assert_eq!(r.chptr[i].attr, 0x1234_5678); // unmasked
            assert_eq!(r.chptr[i].chars, '=');
        }
    }

    #[test]
    fn append_chr_does_not_filter_nonprintable() {
        let mut r = RichString::new();
        // appendChr has no isprint check — the raw char is stored
        RichString_appendChr(&mut r, 0, '\u{1}', 2);
        assert_eq!(r.chptr[0].chars, '\u{1}');
        assert_eq!(r.chptr[1].chars, '\u{1}');
    }

    // ── setAttrn / setAttr ────────────────────────────────────────────

    #[test]
    fn set_attrn_changes_only_the_range_and_keeps_chars() {
        let mut r = ascii("abcdef");
        RichString_setAttrn(&mut r, 0x1234_5678, 2, 3); // indices 2,3,4
        for i in 0..6 {
            let expect = if (2..5).contains(&i) { 0x1234_5678 } else { 0 };
            assert_eq!(r.chptr[i].attr, expect, "attr at {i}");
        }
        assert_eq!(text(&r), "abcdef"); // characters untouched
    }

    #[test]
    fn set_attrn_clamps_range_to_chlen() {
        let mut r = ascii("abc");
        // start+charcount overruns chlen -> clamped to [2, 3)
        RichString_setAttrn(&mut r, 9, 2, 100);
        assert_eq!(r.chptr[2].attr, 9);
        // start past chlen -> empty range, no panic
        RichString_setAttrn(&mut r, 5, 10, 4);
    }

    #[test]
    fn set_attr_applies_to_whole_string() {
        let mut r = ascii("abcd");
        RichString_setAttr(&mut r, 42);
        for i in 0..4 {
            assert_eq!(r.chptr[i].attr, 42);
        }
    }

    // ── wide / UTF-8 decode ───────────────────────────────────────────

    #[test]
    fn append_wide_decodes_multibyte_codepoints() {
        let mut r = RichString::new();
        // "héllo" — é is U+00E9 (2 bytes); "日本" — CJK (3 bytes each)
        let n = RichString_appendWide(&mut r, 0, "héllo日本".as_bytes());
        assert_eq!(text(&r), "héllo日本");
        assert_eq!(n, 7); // 7 code points, not bytes
        assert_eq!(r.chlen, 7);
    }

    #[test]
    fn write_wide_empty_is_noop_does_not_clear() {
        // C returns early on len < 1 without calling setLen, so prior
        // content is preserved.
        let mut r = ascii("keep");
        let n = RichString_writeWide(&mut r, 0, b"");
        assert_eq!(n, 0);
        assert_eq!(text(&r), "keep");
    }

    #[test]
    fn wide_replaces_control_chars_with_replacement() {
        let mut r = RichString::new();
        RichString_appendWide(&mut r, 0, &[b'a', 0x0a, b'b']); // \n is control
        assert_eq!(text(&r), "a\u{FFFD}b");
    }

    // ── mbstowcs_nonfatal ─────────────────────────────────────────────

    #[test]
    fn mbstowcs_decodes_valid_utf8() {
        assert_eq!(mbstowcs_nonfatal("aé日".as_bytes()), vec!['a', 'é', '日']);
    }

    #[test]
    fn mbstowcs_one_replacement_per_broken_run() {
        // two invalid bytes in a row -> exactly one U+FFFD
        assert_eq!(
            mbstowcs_nonfatal(&[b'a', 0xff, 0xfe, b'b']),
            vec!['a', '\u{FFFD}', 'b']
        );
        // separated invalid bytes -> one U+FFFD each
        assert_eq!(
            mbstowcs_nonfatal(&[0xff, b'x', 0xfe]),
            vec!['\u{FFFD}', 'x', '\u{FFFD}']
        );
    }

    #[test]
    fn mbstowcs_stops_at_nul() {
        assert_eq!(mbstowcs_nonfatal(&[b'a', b'b', 0x00, b'c']), vec!['a', 'b']);
    }

    #[test]
    fn mbstowcs_rejects_overlong_and_surrogate() {
        // overlong encoding of '/' (0xc0 0xaf) -> one broken run
        assert_eq!(mbstowcs_nonfatal(&[0xc0, 0xaf]), vec!['\u{FFFD}']);
        // surrogate U+D800 encoded as ed a0 80 -> broken run
        assert_eq!(mbstowcs_nonfatal(&[0xed, 0xa0, 0x80]), vec!['\u{FFFD}']);
    }

    // ── wcwidth (faithful, via unicode-width) ─────────────────────────

    #[test]
    fn wcwidth_narrow_wide_combining_control() {
        assert_eq!(wcwidth('A'), 1); // ASCII narrow
        assert_eq!(wcwidth(' '), 1);
        assert_eq!(wcwidth('é'), 1); // precomposed, narrow
        assert_eq!(wcwidth('世'), 2); // wide CJK
        assert_eq!(wcwidth('本'), 2);
        assert_eq!(wcwidth('\u{1F600}'), 2); // emoji, wide
        assert_eq!(wcwidth('\u{0301}'), 0); // combining acute accent
        assert_eq!(wcwidth('\u{200B}'), 0); // zero-width space
        assert_eq!(wcwidth('\n'), -1); // control -> non-printable
        assert_eq!(wcwidth('\0'), -1);
    }

    // ── appendnWideColumns (faithful wcwidth column accounting) ────────

    #[test]
    fn append_wide_columns_respects_budget_narrow_text() {
        let mut r = RichString::new();
        let mut cols = 3;
        let n = RichString_appendnWideColumns(&mut r, 5, b"abcde", 5, &mut cols);
        assert_eq!(n, 3); // only 3 chars fit in 3 columns
        assert_eq!(cols, 3); // columns written
        assert_eq!(text(&r), "abc");
        assert_eq!(r.chlen, 3);
        assert_eq!(r.chptr[0].attr, 5);
    }

    #[test]
    fn append_wide_columns_zero_budget_writes_nothing() {
        let mut r = RichString::new();
        let mut cols = 0;
        let n = RichString_appendnWideColumns(&mut r, 0, b"abc", 3, &mut cols);
        assert_eq!(n, 0);
        assert_eq!(cols, 0);
        assert_eq!(r.chlen, 0);
    }

    #[test]
    fn append_wide_columns_all_fit() {
        let mut r = RichString::new();
        let mut cols = 100;
        let n = RichString_appendnWideColumns(&mut r, 0, b"hello", 5, &mut cols);
        assert_eq!(n, 5);
        assert_eq!(cols, 5);
        assert_eq!(text(&r), "hello");
    }

    #[test]
    fn append_wide_columns_wide_char_costs_two_columns() {
        // "世" is 2 columns wide; a 4-column budget fits "世界" (2 chars, 4 cols)
        let mut r = RichString::new();
        let mut cols = 4;
        let n = RichString_appendnWideColumns(&mut r, 0, "世界".as_bytes(), 6, &mut cols);
        assert_eq!(n, 2); // 2 characters written
        assert_eq!(cols, 4); // 4 columns consumed
        assert_eq!(text(&r), "世界");
    }

    #[test]
    fn append_wide_columns_wide_char_breaks_on_odd_budget() {
        // 3-column budget: "世" (2) fits leaving 1, next "界" (2) > 1 -> break.
        let mut r = RichString::new();
        let mut cols = 3;
        let n = RichString_appendnWideColumns(&mut r, 0, "世界".as_bytes(), 6, &mut cols);
        assert_eq!(n, 1); // only the first wide char fit
        assert_eq!(cols, 2); // 2 columns written (not 3)
        assert_eq!(text(&r), "世");
        assert_eq!(r.chlen, 1);
    }

    #[test]
    fn append_wide_columns_wide_char_needs_two_columns_min() {
        // 1-column budget can't hold a width-2 char at all.
        let mut r = RichString::new();
        let mut cols = 1;
        let n = RichString_appendnWideColumns(&mut r, 0, "世".as_bytes(), 3, &mut cols);
        assert_eq!(n, 0);
        assert_eq!(cols, 0);
        assert_eq!(r.chlen, 0);
    }

    #[test]
    fn append_wide_columns_combining_mark_costs_zero() {
        // "e" (1) + U+0301 combining acute (0) = 1 column for 2 chars.
        let mut r = RichString::new();
        let mut cols = 1;
        let n = RichString_appendnWideColumns(&mut r, 0, "e\u{0301}".as_bytes(), 3, &mut cols);
        assert_eq!(n, 2); // both code points written
        assert_eq!(cols, 1); // only 1 column consumed
        assert_eq!(text(&r), "e\u{0301}");
    }

    // ── setChar / getCharVal / size / sizeVal accessors ───────────────

    #[test]
    fn set_char_writes_cell_and_zeroes_attr() {
        let mut r = ascii("abc");
        r.chptr[1].attr = 99; // dirty the attr first
        RichString_setChar(&mut r, 1, 'Z');
        assert_eq!(r.chptr[1].chars, 'Z');
        assert_eq!(r.chptr[1].attr, 0); // setChar zeroes the attribute
    }

    #[test]
    fn set_char_grows_buffer_on_demand() {
        let mut r = RichString::new();
        RichString_setChar(&mut r, 500, 'q'); // index past current buffer
        assert!(r.chptr.len() >= 501);
        assert_eq!(r.chptr[500].chars, 'q');
    }

    #[test]
    fn get_char_val_returns_primary_codepoint() {
        let mut r = RichString::new();
        RichString_appendWide(&mut r, 0, "a世b".as_bytes());
        assert_eq!(RichString_getCharVal(&r, 0), 'a');
        assert_eq!(RichString_getCharVal(&r, 1), '世');
        assert_eq!(RichString_getCharVal(&r, 2), 'b');
    }

    #[test]
    fn size_and_size_val_return_chlen() {
        let r = ascii("hello");
        assert_eq!(RichString_size(&r), 5);
        assert_eq!(RichString_sizeVal(&r), 5);
        assert_eq!(RichString_size(&RichString::new()), 0);
    }

    // ── rewind / length management ────────────────────────────────────

    #[test]
    fn rewind_shortens_string() {
        let mut r = ascii("abcdef");
        RichString_rewind(&mut r, 2);
        assert_eq!(r.chlen, 4);
        assert_eq!(text(&r), "abcd");
        assert_eq!(r.chptr[r.chlen as usize].chars, '\0'); // terminator moved back
    }

    #[test]
    fn extend_len_beyond_maxlen_grows_buffer() {
        let mut r = RichString::new();
        let big = RICHSTRING_MAXLEN + 10;
        RichString_extendLen(&mut r, big);
        assert_eq!(r.chlen, big as i32);
        assert!(r.chptr.len() >= big + 1);
        assert_eq!(r.chptr[big].chars, '\0'); // terminator at index len
    }

    #[test]
    fn set_len_switches_to_extend_at_maxlen() {
        let mut r = RichString::new();
        // fill past MAXLEN via a wide append, then verify chlen tracks it
        let data = vec![b'z'; RICHSTRING_MAXLEN + 5];
        let n = RichString_appendAscii(&mut r, 0, &data);
        assert_eq!(n, (RICHSTRING_MAXLEN + 5) as i32);
        assert_eq!(r.chlen, (RICHSTRING_MAXLEN + 5) as i32);
        assert_eq!(text(&r).len(), RICHSTRING_MAXLEN + 5);
    }

    // ── delete ────────────────────────────────────────────────────────

    #[test]
    fn delete_releases_overflow_buffer() {
        let mut r = RichString::new();
        RichString_appendAscii(&mut r, 0, &vec![b'x'; RICHSTRING_MAXLEN + 20]);
        assert!(r.chlen > RICHSTRING_MAXLEN as i32);
        RichString_delete(&mut r);
        assert_eq!(r.chlen, 0);
        assert_eq!(r.chptr, vec![RichCell::default()]);
    }

    #[test]
    fn delete_noop_when_within_internal_buffer() {
        let mut r = ascii("small");
        RichString_delete(&mut r);
        // not an overflow string: left as-is (C only frees the heap case)
        assert_eq!(r.chlen, 5);
        assert_eq!(text(&r), "small");
    }

    // ── findChar (kept from the original port, adapted to the cell model)

    #[test]
    fn find_char_first_match_from_start_zero() {
        let s = ascii("hello");
        assert_eq!(RichString_findChar(&s, 'l', 0), 2);
        assert_eq!(RichString_findChar(&s, 'h', 0), 0);
        assert_eq!(RichString_findChar(&s, 'o', 0), 4);
    }

    #[test]
    fn find_char_start_offset_skips_earlier() {
        let s = ascii("hello");
        assert_eq!(RichString_findChar(&s, 'l', 3), 3);
        assert_eq!(RichString_findChar(&s, 'l', 4), -1);
    }

    #[test]
    fn find_char_absent_and_empty_return_minus_one() {
        assert_eq!(RichString_findChar(&ascii("hello"), 'z', 0), -1);
        assert_eq!(RichString_findChar(&RichString::new(), 'a', 0), -1);
    }

    #[test]
    fn find_char_start_at_or_past_end() {
        let s = ascii("abc");
        assert_eq!(RichString_findChar(&s, 'c', 3), -1);
        assert_eq!(RichString_findChar(&s, 'a', 10), -1);
        assert_eq!(RichString_findChar(&s, 'c', 2), 2);
    }

    #[test]
    fn find_char_matches_wide_codepoint() {
        let mut r = RichString::new();
        RichString_appendWide(&mut r, 0, "a日b".as_bytes());
        assert_eq!(RichString_findChar(&r, '日', 0), 1);
        assert_eq!(RichString_findChar(&r, 'b', 0), 2);
    }

    // ── printoffnVal (behavioral crossterm blit) ──────────────────────
    //
    // The blit emits crossterm escape sequences (MoveTo/SetColor/Print)
    // into a `Vec<u8>` sink. The tests assert on the *printed characters*
    // that survive in the byte stream — the observable payload of the
    // `mvadd_wchnstr` blit — not on the exact escape encoding.

    /// The printable (non-escape) characters emitted into a crossterm sink,
    /// in order. Strips CSI escape sequences (`ESC [ … final-byte`) so only
    /// the `Print`ed glyphs remain.
    fn printed_chars(buf: &[u8]) -> String {
        let s = String::from_utf8(buf.to_vec()).unwrap();
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                // ESC — skip a CSI: the `[` introducer (0x5b), then
                // parameter/intermediate bytes up to and including the final
                // byte in 0x40..=0x7e (`m` for SGR, `H` for cursor move…).
                let intro = chars.next();
                if intro != Some('[') {
                    continue;
                }
                for e in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&e) {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn printoffn_blits_full_range() {
        let r = ascii("hello");
        let mut buf: Vec<u8> = Vec::new();
        RichString_printoffnVal(&mut buf, &r, 0, 0, 0, 5);
        assert_eq!(printed_chars(&buf), "hello");
    }

    #[test]
    fn printoffn_honors_offset_and_count() {
        let r = ascii("abcdef");
        let mut buf: Vec<u8> = Vec::new();
        // off=2, n=3 -> cells 2,3,4 == "cde"
        RichString_printoffnVal(&mut buf, &r, 0, 0, 2, 3);
        assert_eq!(printed_chars(&buf), "cde");
    }

    #[test]
    fn printoffn_stops_at_buffer_end() {
        let r = ascii("abc");
        let mut buf: Vec<u8> = Vec::new();
        // n overruns the 3 valid + 1 terminator cells; blit stops at the
        // end of the backing buffer instead of reading past it.
        RichString_printoffnVal(&mut buf, &r, 0, 0, 0, 100);
        // valid chars plus the terminating null cell (index 3) are emitted;
        // no panic, no read past the Vec.
        assert!(printed_chars(&buf).starts_with("abc"));
    }

    #[test]
    fn printoffn_zero_count_emits_nothing() {
        let r = ascii("abc");
        let mut buf: Vec<u8> = Vec::new();
        RichString_printoffnVal(&mut buf, &r, 0, 0, 0, 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn printoffn_blits_wide_codepoints() {
        let mut r = RichString::new();
        RichString_appendWide(&mut r, 0, "a世b".as_bytes());
        let mut buf: Vec<u8> = Vec::new();
        RichString_printoffnVal(&mut buf, &r, 0, 0, 0, 3);
        assert_eq!(printed_chars(&buf), "a世b");
    }
}
