//! Port of `XUtils.c` — htop's string and math utility layer.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port.
//!
//! The C allocation wrappers (`xMalloc`/`xCalloc`/`xRealloc`/… and
//! `fail`), the null-terminated helpers (`strnlen`,
//! `String_safeStrncpy`), the varargs formatters (`xAsprintf`,
//! `xSnprintf`), `full_write`, and `String_freeArray` have no
//! faithful safe-Rust analog (Rust owns its allocation, bounds, and
//! lifetimes), so they are intentionally not ported here.
#![allow(non_snake_case)]

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
