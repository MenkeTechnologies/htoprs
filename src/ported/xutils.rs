//! Port of `XUtils.c` — htop's string and math utility layer.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port.
//!
//! The C allocation wrappers (`xMalloc`/`xCalloc`/`xRealloc`/…), the
//! string-duplication helpers (`xStrdup`/`xStrndup`/
//! `free_and_xStrdup`), the varargs formatters (`xAsprintf`,
//! `xSnprintf`), `full_write`, `String_readLine`, and
//! `String_freeArray` have no faithful safe-Rust analog (Rust owns its
//! allocation, bounds, and lifetimes), so they are intentionally not
//! ported here. `fail` (aborts after [`crate::ported::crt::CRT_done`])
//! and `strnlen` (a pure byte scan) are ported below.
//!
//! Deferred (blocked, not faithfully addable yet): the `static inline`
//! header helpers `String_stripControlChars`, `Char_isControl`, and
//! `Char_isC1Control` (`XUtils.h:137`-`156`) — the cited blocker for
//! `InfoScreen_drawTitled`. Their C names are absent from the
//! checked-in port-purity snapshot
//! (`tests/data/htop_c_fn_names.txt`): the extractor scans `.c`
//! sources and the `InfoScreen.c`/`RichString.c` call sites post-date
//! the snapshot, so a module-level `fn` with any of those names trips
//! the `build.rs` port-purity gate. Regenerating the snapshot and
//! editing the allowlist are out of scope for this port, so these
//! stay deferred rather than break `cargo build`.
#![allow(non_snake_case)]

/// Port of `void fail(void)` from `XUtils.c:27`. Restores the terminal
/// via [`crate::ported::crt::CRT_done`] and aborts the process. The C
/// `_exit(1)` after `abort()` is unreachable; the `-> !` return type
/// captures the no-return contract.
pub fn fail() -> ! {
    crate::ported::crt::CRT_done();
    std::process::abort();
}

/// Port of `String_cat(const char* s1, const char* s2)` from
/// `XUtils.c:125`. Returns the concatenation of `s1` and `s2`. The C
/// `SIZE_MAX` overflow guard is unnecessary — Rust's allocator faults
/// on overflow.
pub fn String_cat(s1: &str, s2: &str) -> String {
    let mut out = String::with_capacity(s1.len() + s2.len());
    out.push_str(s1);
    out.push_str(s2);
    out
}

/// Port of `String_trim(const char* in)` from `XUtils.c:138`. Strips
/// leading and trailing ` `, `\t`, and `\n` (only those three — not
/// the full ASCII whitespace set, matching the C loop).
pub fn String_trim(input: &str) -> String {
    input
        .trim_matches(|c| c == ' ' || c == '\t' || c == '\n')
        .to_string()
}

/// Port of `String_split(const char* s, char sep, size_t* n)` from
/// `XUtils.c:151`. Splits `s` on `sep`. Interior empty fields (from
/// consecutive separators) are kept; a trailing empty field (when `s`
/// ends in `sep`) is dropped — the C loop only pushes the final
/// segment `when s[0] != '\0'`. The out-param `n` is the returned
/// `Vec`'s length.
pub fn String_split(s: &str, sep: char) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = s;
    while let Some(idx) = rest.find(sep) {
        out.push(rest[..idx].to_string());
        rest = &rest[idx + sep.len_utf8()..];
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// Port of `String_splitFirst(const char* s, char sep, size_t* n)`
/// from `XUtils.c:181`. Like [`String_split`] but splits only on the
/// first occurrence of `sep`.
pub fn String_splitFirst(s: &str, sep: char) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = s;
    if let Some(idx) = rest.find(sep) {
        out.push(rest[..idx].to_string());
        rest = &rest[idx + sep.len_utf8()..];
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// Port of `String_contains_i(const char* s1, const char* s2, bool
/// multi)` from `XUtils.c:107`. Case-insensitive substring test. When
/// `multi` is set and `s2` contains `|`, `s2` is treated as a set of
/// `|`-separated needles and the result is true if any needle matches.
/// The C `strcasestr` (ASCII case-insensitive `strstr`) is inlined as
/// an ASCII-lowercased `contains`.
pub fn String_contains_i(s1: &str, s2: &str, multi: bool) -> bool {
    if multi && s2.contains('|') {
        let hay = s1.to_ascii_lowercase();
        for needle in String_split(s2, '|') {
            if hay.contains(&needle.to_ascii_lowercase()) {
                return true;
            }
        }
        false
    } else {
        s1.to_ascii_lowercase().contains(&s2.to_ascii_lowercase())
    }
}

/// Port of `String_startsWith(const char* s, const char* match)` from
/// `XUtils.h:54` (`static inline`). True iff `s` begins with the byte
/// prefix `match`. The C `strncmp(s, match, strlen(match)) == 0` is a
/// byte-prefix test; `str::starts_with` is the same on UTF-8 bytes.
pub fn String_startsWith(s: &str, match_: &str) -> bool {
    s.starts_with(match_)
}

/// Port of `String_eq(const char* s1, const char* s2)` from
/// `XUtils.h:61` (`static inline`). Byte-exact string equality
/// (`strcmp(s1, s2) == 0`).
pub fn String_eq(s1: &str, s2: &str) -> bool {
    s1 == s2
}

/// Port of `String_eq_nullable(const char* s1, const char* s2)` from
/// `XUtils.h:65` (`static inline`). The C code returns true when the
/// pointers are identical (covers both-`NULL`), true when both are
/// non-`NULL` and equal, and false otherwise (exactly one `NULL`).
/// `None` models the C `NULL` pointer.
pub fn String_eq_nullable(s1: Option<&str>, s2: Option<&str>) -> bool {
    match (s1, s2) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Port of `String_strchrnul(const char* s, int c)` from `XUtils.h:93`
/// (`static inline`, the `!HAVE_STRCHRNUL` fallback branch). Returns
/// the byte index of the first occurrence of `c` in `s`, or `s.len()`
/// (the position of the terminating NUL) when `c` is not found — the
/// index analog of the C pointer return.
pub fn String_strchrnul(s: &str, c: u8) -> usize {
    match s.bytes().position(|b| b == c) {
        Some(i) => i,
        None => s.len(),
    }
}

/// Port of `String_safeStrncpy(char* restrict dest, const char*
/// restrict src, size_t size)` from `XUtils.c:241`. Cited blocker for
/// `DynamicMeter_getUiName`. Copies bytes from `src` into the fixed
/// buffer `dest` (whose length plays the role of the C `size` arg),
/// always NUL-terminating, and returns the number of bytes copied
/// (excluding the terminator). Stops at `dest.len() - 1` bytes, at a
/// NUL byte in `src`, or at the end of `src` — the last replacing the
/// C reliance on a trailing NUL, so byte semantics match for a C
/// string carried as bytes. Truncation is byte-level (may split a
/// multi-byte UTF-8 sequence), matching the C copy.
pub fn String_safeStrncpy(dest: &mut [u8], src: &[u8]) -> usize {
    let size = dest.len();
    assert!(size > 0);

    let mut i = 0;
    while i < size - 1 && i < src.len() && src[i] != 0 {
        dest[i] = src[i];
        i += 1;
    }

    dest[i] = 0;

    i
}

/// Port of `size_t strnlen(const char* str, size_t maxLen)` from
/// `XUtils.c:252` (the `!HAVE_STRNLEN` fallback). Returns the length of
/// the NUL-terminated C string `str` (modeled as a byte slice, as
/// [`String_safeStrncpy`] does), stopping at the first NUL byte or at
/// `max_len`, whichever comes first.
pub fn strnlen(str: &[u8], max_len: usize) -> usize {
    for len in 0..max_len {
        if str[len] == 0 {
            return len;
        }
    }
    max_len
}

/// Port of `compareRealNumbers(double a, double b)` from
/// `XUtils.c:345`. Orders `a` and `b` treating NaN as less than any
/// value (regardless of sign) and two NaNs as equal. Rust's `>` is
/// quiet for NaN, matching C's `isgreater`.
pub fn compareRealNumbers(a: f64, b: f64) -> i32 {
    let result = (a > b) as i32 - (b > a) as i32;
    if result != 0 {
        return result;
    }
    (!a.is_nan()) as i32 - (!b.is_nan()) as i32
}

/// Port of `sumPositiveValues(const double* array, size_t count)` from
/// `XUtils.c:355`. Sums the strictly-positive values, skipping NaN
/// (`isPositive(x)` is `x > 0.0`, false for NaN). The result is always
/// nonnegative.
pub fn saturatingSub(a: u64, b: u64) -> u64 {
    // Port of `saturatingSub` (`Macros.h`): `a > b ? a - b : 0`.
    if a > b {
        a - b
    } else {
        0
    }
}

pub fn sumPositiveValues(array: &[f64]) -> f64 {
    let mut sum = 0.0;
    for &v in array {
        if v > 0.0 {
            sum += v;
        }
    }
    sum
}

/// Port of `countDigits(size_t n, size_t base)` from `XUtils.c:367`.
/// Number of digits needed to print `n` in `base`; returns 1 for zero.
/// O(log n) with the same overflow guard on `limit *= base`.
pub fn countDigits(n: usize, base: usize) -> usize {
    assert!(base > 1);
    let mut res = 1;
    let mut limit = base;
    while n >= limit {
        res += 1;
        if limit > usize::MAX / base {
            break;
        }
        limit *= base;
    }
    res
}

// map a bit value mod 37 to its position (XUtils.c:381)
const MOD37_BIT_POSITION: [u8; 37] = [
    32, 0, 1, 26, 2, 23, 27, 0, 3, 16, 24, 30, 28, 11, 0, 13, 4, 7, 17, 0, 25, 22, 31, 15, 29, 10,
    12, 6, 0, 21, 14, 9, 5, 20, 8, 19, 18,
];

/// Port of `countTrailingZeros(unsigned int x)` from `XUtils.c:388`
/// (the `!HAVE_BUILTIN_CTZ` fallback). Isolates the lowest set bit
/// (`-x & x`, wrapping negation on the unsigned value) and maps it to
/// its position via the mod-37 table.
pub fn countTrailingZeros(x: u32) -> u32 {
    MOD37_BIT_POSITION[((x.wrapping_neg() & x) % 37) as usize] as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_cat_concatenates() {
        assert_eq!(String_cat("foo", "bar"), "foobar");
        assert_eq!(String_cat("", "x"), "x");
        assert_eq!(String_cat("x", ""), "x");
    }

    #[test]
    fn string_trim_only_space_tab_newline() {
        assert_eq!(String_trim("  \t hi \n\t"), "hi");
        assert_eq!(String_trim("\n\nabc"), "abc");
        // \r is NOT trimmed (not in the C set)
        assert_eq!(String_trim("\rhi\r"), "\rhi\r");
    }

    #[test]
    fn string_split_keeps_interior_drops_trailing_empty() {
        assert_eq!(String_split("a,b,c", ','), vec!["a", "b", "c"]);
        // interior empty from "a,,b" kept, matches C ["a","","b"]
        assert_eq!(String_split("a,,b", ','), vec!["a", "", "b"]);
        // trailing sep drops the empty final field: C returns ["a"], n=1
        assert_eq!(String_split("a,", ','), vec!["a"]);
        // leading sep keeps the empty first field
        assert_eq!(String_split(",a", ','), vec!["", "a"]);
        // empty input -> empty result, n=0
        assert!(String_split("", ',').is_empty());
        // no separator -> whole string
        assert_eq!(String_split("abc", ','), vec!["abc"]);
    }

    #[test]
    fn string_split_first_only_first_sep() {
        assert_eq!(String_splitFirst("a,b,c", ','), vec!["a", "b,c"]);
        assert_eq!(String_splitFirst("nocomma", ','), vec!["nocomma"]);
        assert_eq!(String_splitFirst("a,", ','), vec!["a"]);
    }

    #[test]
    fn string_contains_i_case_insensitive_and_multi() {
        assert!(String_contains_i("Hello World", "hello", false));
        assert!(!String_contains_i("Hello", "xyz", false));
        // multi: any needle matches
        assert!(String_contains_i("firefox", "chrome|FOX|edge", true));
        assert!(!String_contains_i("safari", "chrome|fox|edge", true));
        // multi=false ignores '|' — treats it literally
        assert!(!String_contains_i("firefox", "chrome|fox", false));
    }

    #[test]
    fn string_starts_with_byte_prefix() {
        assert!(String_startsWith("firefox", "fire"));
        assert!(String_startsWith("abc", "")); // empty match always true
        assert!(String_startsWith("abc", "abc")); // equal is a prefix
        assert!(!String_startsWith("abc", "abcd")); // longer than s
        assert!(!String_startsWith("abc", "b"));
        // byte-level, not codepoint-level: partial UTF-8 prefix
        assert!(String_startsWith("áb", "\u{e1}")); // full 'á'
    }

    #[test]
    fn string_eq_byte_exact() {
        assert!(String_eq("abc", "abc"));
        assert!(!String_eq("abc", "abd"));
        assert!(!String_eq("abc", "ab"));
        assert!(String_eq("", ""));
        assert!(!String_eq("", "x"));
    }

    #[test]
    fn string_eq_nullable_null_semantics() {
        // both NULL -> true (C pointer identity)
        assert!(String_eq_nullable(None, None));
        // both non-null and equal
        assert!(String_eq_nullable(Some("x"), Some("x")));
        // both non-null, unequal
        assert!(!String_eq_nullable(Some("x"), Some("y")));
        // exactly one NULL -> false
        assert!(!String_eq_nullable(Some("x"), None));
        assert!(!String_eq_nullable(None, Some("x")));
        // empty-string is non-null: equal to itself, not to None
        assert!(String_eq_nullable(Some(""), Some("")));
        assert!(!String_eq_nullable(Some(""), None));
    }

    #[test]
    fn string_strchrnul_index_or_len() {
        assert_eq!(String_strchrnul("a=b", b'='), 1);
        assert_eq!(String_strchrnul("abc", b'a'), 0);
        // not found -> len (position of terminating NUL)
        assert_eq!(String_strchrnul("abc", b'z'), 3);
        assert_eq!(String_strchrnul("", b'x'), 0);
        // first occurrence only
        assert_eq!(String_strchrnul("a=b=c", b'='), 1);
    }

    #[test]
    fn string_safe_strncpy_truncates_and_terminates() {
        // exact-fit: 5 bytes + NUL into a 6-byte buffer
        let mut buf = [0u8; 6];
        assert_eq!(String_safeStrncpy(&mut buf, b"hello"), 5);
        assert_eq!(&buf, b"hello\0");

        // truncation boundary: only size-1 == 3 bytes fit
        let mut buf = [0xFFu8; 4];
        assert_eq!(String_safeStrncpy(&mut buf, b"hello"), 3);
        assert_eq!(&buf, b"hel\0");

        // size == 1: nothing copied, just the terminator
        let mut buf = [0xFFu8; 1];
        assert_eq!(String_safeStrncpy(&mut buf, b"x"), 0);
        assert_eq!(&buf, b"\0");

        // empty src: terminator only
        let mut buf = [0xFFu8; 8];
        assert_eq!(String_safeStrncpy(&mut buf, b""), 0);
        assert_eq!(buf[0], 0);

        // embedded NUL in src stops the copy (C src[i] truthiness)
        let mut buf = [0xFFu8; 8];
        assert_eq!(String_safeStrncpy(&mut buf, b"ab\0cd"), 2);
        assert_eq!(&buf[..3], b"ab\0");

        // byte-level truncation may split a UTF-8 sequence, matching C:
        // 'á' is 0xC3 0xA1; a 2-byte buffer copies only 0xC3
        let mut buf = [0u8; 2];
        assert_eq!(String_safeStrncpy(&mut buf, "á".as_bytes()), 1);
        assert_eq!(&buf, &[0xC3u8, 0x00]);
    }

    #[test]
    fn strnlen_stops_at_nul_or_cap() {
        // NUL before the cap -> length up to the NUL
        assert_eq!(strnlen(b"hello\0world", 11), 5);
        // no NUL within the cap -> the cap
        assert_eq!(strnlen(b"hello", 3), 3);
        // NUL exactly at the cap boundary is not scanned -> cap
        assert_eq!(strnlen(b"abc\0", 3), 3);
        // NUL just inside the cap
        assert_eq!(strnlen(b"abc\0", 4), 3);
        // empty scan (cap 0) -> 0
        assert_eq!(strnlen(b"abc", 0), 0);
        // leading NUL -> 0
        assert_eq!(strnlen(b"\0abc", 4), 0);
    }

    #[test]
    fn compare_real_numbers_orders_and_nan_last() {
        assert_eq!(compareRealNumbers(1.0, 2.0), -1);
        assert_eq!(compareRealNumbers(2.0, 1.0), 1);
        assert_eq!(compareRealNumbers(1.0, 1.0), 0);
        // NaN < any value
        assert_eq!(compareRealNumbers(f64::NAN, 1.0), -1);
        assert_eq!(compareRealNumbers(1.0, f64::NAN), 1);
        // two NaNs are equal
        assert_eq!(compareRealNumbers(f64::NAN, f64::NAN), 0);
    }

    #[test]
    fn sum_positive_values_skips_nan_and_nonpositive() {
        assert_eq!(sumPositiveValues(&[1.0, 2.0, 3.0]), 6.0);
        assert_eq!(sumPositiveValues(&[1.0, -2.0, 3.0]), 4.0);
        assert_eq!(sumPositiveValues(&[f64::NAN, 5.0, -1.0]), 5.0);
        assert_eq!(sumPositiveValues(&[-1.0, -2.0]), 0.0);
        assert_eq!(sumPositiveValues(&[]), 0.0);
    }

    #[test]
    fn count_digits_base10_and_base2() {
        assert_eq!(countDigits(0, 10), 1);
        assert_eq!(countDigits(9, 10), 1);
        assert_eq!(countDigits(10, 10), 2);
        assert_eq!(countDigits(999, 10), 3);
        assert_eq!(countDigits(1000, 10), 4);
        assert_eq!(countDigits(0, 2), 1);
        assert_eq!(countDigits(1, 2), 1);
        assert_eq!(countDigits(2, 2), 2);
        assert_eq!(countDigits(255, 16), 2);
    }

    #[test]
    fn count_trailing_zeros_matches_intrinsic() {
        // cross-check the mod-37 table against the hardware intrinsic
        // for every single-bit value and a few composites
        for shift in 0..31u32 {
            let x = 1u32 << shift;
            assert_eq!(countTrailingZeros(x), x.trailing_zeros(), "x={x:#x}");
        }
        assert_eq!(countTrailingZeros(0b1011000), 3);
        assert_eq!(countTrailingZeros(12), 2);
    }
}
