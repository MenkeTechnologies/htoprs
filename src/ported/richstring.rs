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
//! # Unicode substrate (documented approximations, no new dependency)
//!
//! htop leans on three libc primitives this crate has no dependency for:
//!
//! - `iswprint` / `isprint`: character-printability classification used to
//!   replace non-printable input with `U+FFFD`. `isprint` is ported
//!   exactly (C-locale printable range `0x20..=0x7e`). `iswprint` is
//!   approximated as `!char::is_control()`, which matches glibc for the
//!   C0/C1 control range that htop's replacement targets (control bytes in
//!   process names, meter text, etc.); it diverges from glibc only for
//!   exotic unassigned / format code points, which do not occur in htop's
//!   real input paths.
//! - `wcwidth`: display-column width, needed by
//!   [`RichString_appendnWideColumns`]. A faithful `wcwidth` requires
//!   Unicode East-Asian-Width tables that are unavailable without adding a
//!   dependency, so — per the port brief — the char-append structure is
//!   ported faithfully and the column count uses a width of 1 per
//!   (post-replacement, printable) character. This is exact for narrow
//!   text (the overwhelming majority of htop's meter/column output) and
//!   diverges only for wide (CJK/emoji, `wcwidth == 2`) and combining
//!   (`wcwidth == 0`) code points. Nothing is invented: the column count
//!   is precisely "characters written", documented here and at the call.
//!
//! `mbstowcs_nonfatal` (the multibyte→wide decode) is ported faithfully:
//! it decodes UTF-8, emits exactly one `U+FFFD` per contiguous run of
//! invalid bytes, and stops at a NUL — mirroring the `mbrtowc` loop.
#![allow(non_snake_case)]
#![allow(dead_code)]

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

    /// Rust analog of the `RichString_setChar(this, at, ch)` macro
    /// (`RichString.h:30`): `chptr[at] = { .chars = { ch, 0 } }`, i.e. the
    /// cell's attribute is zeroed. Grows the owned buffer so index `at`
    /// exists — in C this is always in-bounds because `chstr` is physically
    /// `RICHSTRING_MAXLEN + 1` cells and `extendLen` sizes the heap buffer;
    /// here the buffer grows on demand, filling any gap with null cells
    /// (which callers overwrite before reading).
    fn set_char(&mut self, at: usize, ch: char) {
        if at >= self.chptr.len() {
            self.chptr.resize(at + 1, RichCell::default());
        }
        self.chptr[at] = RichCell { chars: ch, attr: 0 };
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

    /// Column width used by [`RichString_appendnWideColumns`] in place of
    /// libc `wcwidth(3)` (`RichString.c:131`). Returns 1 for every
    /// character: after the `iswprint` replacement all cells hold printable
    /// code points, and a faithful multi-width `wcwidth` needs Unicode
    /// East-Asian-Width tables unavailable without a dependency. Exact for
    /// narrow text; see the module docs for the wide/combining divergence.
    fn column_width(_c: char) -> i32 {
        1
    }
}

impl Default for RichString {
    fn default() -> Self {
        Self::new()
    }
}

/// Port of `RichString.c:24` (`static void RichString_extendLen`). Ensures
/// the buffer can hold `len` characters plus a terminator, writes the null
/// terminator at index `len`, and sets `chlen = len`. In C this chooses
/// between the inline `chstr` buffer and a `xMalloc`/`xRealloc` heap buffer
/// (with `memcpy`/`free` bookkeeping); the owned `Vec` subsumes both, so
/// those branches collapse into the single buffer growth done by
/// [`RichString::set_char`].
pub fn RichString_extendLen(this: &mut RichString, len: usize) {
    this.set_char(len, '\0');
    this.chlen = len as i32;
}

/// Port of `RichString.c:52` (`static void RichString_setLen`). Fast path
/// when both the new and current lengths are below `RICHSTRING_MAXLEN`
/// (in C, the inline buffer always suffices): write the terminator and set
/// `chlen`. Otherwise defer to [`RichString_extendLen`].
pub fn RichString_setLen(this: &mut RichString, len: usize) {
    if len < RICHSTRING_MAXLEN && (this.chlen as usize) < RICHSTRING_MAXLEN {
        this.set_char(len, '\0');
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
/// The `wcwidth`-based column accounting is ported structurally, but the
/// per-character width uses [`RichString::column_width`] (a fixed 1) rather
/// than libc `wcwidth` — a faithful multi-width `wcwidth` needs Unicode
/// East-Asian-Width tables unavailable without a dependency. So `*columns`
/// is exactly "characters written" here: identical to htop for narrow
/// text, diverging only for wide/combining code points. See the module
/// docs.
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
        let cwidth = RichString::column_width(c);
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
        this.chptr[i] = RichCell { chars: c, attr: attrs };
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

    // ── appendnWideColumns (char-width fallback: 1 col per char) ───────

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
}
