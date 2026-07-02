//! Partial port of `Row.c`.
//!
//! Ported: the `RichString` number formatters
//! `Row_print{KBytes,Bytes,Count,Time,Nanoseconds,Rate,LeftAlignedField}`
//! (they write styled digits into a [`RichString`], choosing a
//! `CRT_colors[...]` attribute per magnitude band), plus the pure
//! `Row_printPercentage` (writes into a `char* buffer`). These sit on
//! the merged `richstring` + `crt` substrate.
//!
//! Still stubbed (`todo!()`, named after their real htop C function so
//! the port-purity gate accepts the module): `Row_init` / `Row_done` /
//! `Row_display` / `Row_compare` / the field-title and width helpers —
//! they need the unported `Row` / `Machine` / `Settings` structs and the
//! ncurses draw layer. Replace each stub with a faithful port of the C
//! body, updating the signature and the doc comment to
//! `Port of `Row.c`:<line>.` as you go. `gen_port_report.py` counts
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::crt::ColorElements::*;
use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendChr, RichString_appendnAscii,
    RichString_appendnWideColumns,
};

// Unit-size constants from `Row.h:107-116`.
/// `#define ONE_K 1024UL` (`Row.h:107`).
const ONE_K: u64 = 1024;
/// `#define ONE_M (ONE_K * ONE_K)` (`Row.h:108`).
const ONE_M: u64 = ONE_K * ONE_K;
/// `#define ONE_G (ONE_M * ONE_K)` (`Row.h:109`).
const ONE_G: u64 = ONE_M * ONE_K;
/// `#define ONE_T (1ULL * ONE_G * ONE_K)` (`Row.h:110`).
const ONE_T: u64 = ONE_G * ONE_K;
/// `#define ONE_P (1ULL * ONE_T * ONE_K)` (`Row.h:111`).
const ONE_P: u64 = ONE_T * ONE_K;
/// `#define ONE_DECIMAL_K 1000UL` (`Row.h:113`).
const ONE_DECIMAL_K: u64 = 1000;
/// `#define ONE_DECIMAL_M (ONE_DECIMAL_K * ONE_DECIMAL_K)` (`Row.h:114`).
const ONE_DECIMAL_M: u64 = ONE_DECIMAL_K * ONE_DECIMAL_K;
/// `#define ONE_DECIMAL_G (ONE_DECIMAL_M * ONE_DECIMAL_K)` (`Row.h:115`).
const ONE_DECIMAL_G: u64 = ONE_DECIMAL_M * ONE_DECIMAL_K;
/// `#define ONE_DECIMAL_T (1ULL * ONE_DECIMAL_G * ONE_DECIMAL_K)`
/// (`Row.h:116`).
const ONE_DECIMAL_T: u64 = ONE_DECIMAL_G * ONE_DECIMAL_K;

/// `static const char unitPrefixes[]` from `XUtils.h:160` — the
/// unit-prefix letters `Row_printKBytes` indexes by magnitude. `XUtils.h`
/// is not yet a ported module; this data is reproduced verbatim here (the
/// same values `meter.rs` also copies from `XUtils.h:160`).
const unitPrefixes: [u8; 10] = [b'K', b'M', b'G', b'T', b'P', b'E', b'Z', b'Y', b'R', b'Q'];

/// The discrete color selection [`Row_printPercentage`] makes for its
/// `int* attr` out-param.
///
/// The C writes `*attr = CRT_colors[PROCESS_SHADOW]` or
/// `*attr = CRT_colors[PROCESS_MEGABYTES]` in two branches, and leaves
/// `*attr` untouched otherwise (the caller's prior value survives). CRT
/// colors are not ported — the `CRT_colors[...]` palette lookup lives in
/// the unported CRT layer — so this enum mirrors *which* branch the C
/// took, not a color value. The unported CRT layer applies the actual
/// `CRT_colors[PROCESS_SHADOW]` / `CRT_colors[PROCESS_MEGABYTES]` mapping
/// when it consumes this. Variants correspond 1:1 to the C branches; no
/// color constants are invented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PercentageAttr {
    /// C: `*attr = CRT_colors[PROCESS_SHADOW];` — taken when
    /// `val < 0.05` (nonnegative branch) or when `val` is negative/NaN.
    Shadow,
    /// C: `*attr = CRT_colors[PROCESS_MEGABYTES];` — taken when
    /// `val >= 99.9`.
    Megabytes,
    /// Neutral sentinel modeling "C leaves `*attr` at the caller's prior
    /// value" — the branch taken when `0.05 <= val < 99.9`. The C only
    /// *assigns* `*attr` in the `PROCESS_SHADOW` / `PROCESS_MEGABYTES`
    /// cases, so [`Row_printPercentage`] likewise never writes this
    /// variant; it exists as a neutral initial value the caller can pass
    /// and read back to detect that no branch fired.
    Unchanged,
}


/// TODO: port of `void Row_init(Row* this, const Machine* host` from `Row.c:35`.
pub fn Row_init() {
    todo!("port of Row.c:35")
}

/// TODO: port of `void Row_done(Row* this` from `Row.c:44`.
pub fn Row_done() {
    todo!("port of Row.c:44")
}

/// TODO: port of `static inline bool Row_isNew(const Row* this` from `Row.c:49`.
pub fn Row_isNew() {
    todo!("port of Row.c:49")
}

/// TODO: port of `static inline bool Row_isTomb(const Row* this` from `Row.c:58`.
pub fn Row_isTomb() {
    todo!("port of Row.c:58")
}

/// TODO: port of `void Row_display(const Object* cast, RichString* out` from `Row.c:62`.
pub fn Row_display() {
    todo!("port of Row.c:62")
}

/// TODO: port of `void Row_setPidColumnWidth(pid_t maxPid` from `Row.c:86`.
pub fn Row_setPidColumnWidth() {
    todo!("port of Row.c:86")
}

/// TODO: port of `void Row_setUidColumnWidth(uid_t maxUid` from `Row.c:96`.
pub fn Row_setUidColumnWidth() {
    todo!("port of Row.c:96")
}

/// TODO: port of `void Row_resetFieldWidths(void` from `Row.c:108`.
pub fn Row_resetFieldWidths() {
    todo!("port of Row.c:108")
}

/// TODO: port of `void Row_updateFieldWidth(RowField key, size_t width` from `Row.c:119`.
pub fn Row_updateFieldWidth() {
    todo!("port of Row.c:119")
}

/// TODO: port of `static const char* alignedTitleDynamicColumn(const Settings* settings, int key, char* titleBuffer, size_t titleBufferSize` from `Row.c:127`.
pub fn alignedTitleDynamicColumn() {
    todo!("port of Row.c:127")
}

/// TODO: port of `static const char* alignedTitleProcessField(ProcessField field, char* titleBuffer, size_t titleBufferSize` from `Row.c:141`.
pub fn alignedTitleProcessField() {
    todo!("port of Row.c:141")
}

/// TODO: port of `const char* RowField_alignedTitle(const Settings* settings, RowField field` from `Row.c:168`.
pub fn RowField_alignedTitle() {
    todo!("port of Row.c:168")
}

/// TODO: port of `RowField RowField_keyAt(const Settings* settings, int at` from `Row.c:179`.
pub fn RowField_keyAt() {
    todo!("port of Row.c:179")
}

/// Port of `Row.c:193`.
///
/// C signature:
/// `void Row_printKBytes(RichString* str, unsigned long long number, bool coloring)`.
///
/// Formats `number` (in KiB) into a fixed-width styled field, promoting
/// through unit prefixes (`K`, `M`, `G`, …) as magnitude grows and
/// coloring the digits by band (`PROCESS`, `PROCESS_MEGABYTES`,
/// `PROCESS_GIGABYTES`, `LARGE_NUMBER`). `ULLONG_MAX` renders `"  N/A "`.
///
/// Signature mapping: `str` → `&mut RichString`; the C `char buffer[16]`
/// + `xSnprintf`/`RichString_appendnAscii(str, color, buffer, len)` pairs
/// become owned `String`s appended via [`RichString_appendnAscii`] with
/// `s.len()` as the byte count (ASCII, so bytes == chars). `CRT_colors[X]`
/// reads become `X.packed(scheme)` for the active scheme. The two
/// `goto invalidNumber` sites are inlined at both jump points.
pub fn Row_printKBytes(str: &mut RichString, mut number: u64, coloring: bool) {
    let scheme = ColorScheme::active();
    let color_of = |e: ColorElements| e.packed(scheme);

    let mut color = color_of(PROCESS);
    let mut next_unit_color = color_of(PROCESS);

    // const int colors[4] = { PROCESS, PROCESS_MEGABYTES, PROCESS_GIGABYTES, LARGE_NUMBER }
    let colors = [
        color_of(PROCESS),
        color_of(PROCESS_MEGABYTES),
        color_of(PROCESS_GIGABYTES),
        color_of(LARGE_NUMBER),
    ];

    if number == u64::MAX {
        // invalidNumber:
        if coloring {
            color = color_of(PROCESS_SHADOW);
        }
        RichString_appendAscii(str, color, b"  N/A ");
        return;
    }

    if coloring {
        color = colors[0];
        next_unit_color = colors[1];
    }

    if number < 1000 {
        // Plain number, no markings
        let buf = format!("{:5} ", number as u32);
        RichString_appendnAscii(str, color, buf.as_bytes(), buf.len());
        return;
    }

    if number < 100000 {
        // 2 digits for M, 3 digits for K
        let buf = format!("{:2}", (number / 1000) as u32);
        RichString_appendnAscii(str, next_unit_color, buf.as_bytes(), buf.len());
        let buf = format!("{:03} ", (number % 1000) as u32);
        RichString_appendnAscii(str, color, buf.as_bytes(), buf.len());
        return;
    }

    // 100000 KiB (97.6 MiB) or greater. A unit prefix would be added.
    // maxUnitIndex = (sizeof(number) * CHAR_BIT - 1) / 10 + 1 with
    // sizeof(u64) == 8, CHAR_BIT == 8.
    let max_unit_index: usize = (8 * 8 - 1) / 10 + 1;
    let can_overflow = max_unit_index >= unitPrefixes.len();

    let mut i: usize = 1;
    // C: `int prevUnitColor;` — assigned in the loop before it is read.
    let mut prev_unit_color: i32;
    // Convert KiB to (1/100) of MiB
    let mut hundredths: u64 = (number / 256) * 25 + (number % 256) * 25 / 256;
    loop {
        if can_overflow && i >= unitPrefixes.len() {
            // invalidNumber:
            if coloring {
                color = color_of(PROCESS_SHADOW);
            }
            RichString_appendAscii(str, color, b"  N/A ");
            return;
        }

        prev_unit_color = color;
        color = next_unit_color;

        if coloring && i + 1 < colors.len() {
            next_unit_color = colors[i + 1];
        }

        if hundredths < 1000000 {
            break;
        }

        hundredths /= ONE_K;
        i += 1;
    }

    number = hundredths / 100;
    hundredths %= 100;
    let tail: String;
    if number < 100 {
        let buf = format!("{}", number as u32);
        RichString_appendnAscii(str, color, buf.as_bytes(), buf.len());
        let buf = if number < 10 {
            // 1 digit + decimal point + 2 digits: "9.76G", "9.99T", etc.
            format!(".{:02}", hundredths as u32)
        } else {
            // 2 digits + decimal point + 1 digit: "97.6M", "10.0G", etc.
            format!(".{}", (hundredths as u32) / 10)
        };
        RichString_appendnAscii(str, prev_unit_color, buf.as_bytes(), buf.len());
        tail = format!("{} ", unitPrefixes[i] as char);
    } else if number < 1000 {
        // 3 digits: "100M", "999G", etc.
        tail = format!("{:4}{} ", number as u32, unitPrefixes[i] as char);
    } else {
        // 1 digit + 3 digits: "1000M", "9999G", etc.
        debug_assert!(number < 10000);
        let buf = format!("{}", (number as u32) / 1000);
        RichString_appendnAscii(str, next_unit_color, buf.as_bytes(), buf.len());
        tail = format!("{:03}{} ", (number as u32) % 1000, unitPrefixes[i] as char);
    }
    RichString_appendnAscii(str, color, tail.as_bytes(), tail.len());
}

/// Port of `Row.c:295`.
///
/// C signature:
/// `void Row_printBytes(RichString* str, unsigned long long number, bool coloring)`.
///
/// Converts a raw byte count to KiB (`number / ONE_K`) and defers to
/// [`Row_printKBytes`]; `ULLONG_MAX` is forwarded unchanged as the
/// invalid sentinel.
pub fn Row_printBytes(str: &mut RichString, number: u64, coloring: bool) {
    if number == u64::MAX {
        Row_printKBytes(str, u64::MAX, coloring);
    } else {
        Row_printKBytes(str, number / ONE_K, coloring);
    }
}

/// Port of `Row.c:302`.
///
/// C signature:
/// `void Row_printCount(RichString* str, unsigned long long number, bool coloring)`.
///
/// Formats a decimal count into a 12-column field (`"%11llu "`), coloring
/// leading groups of digits by decimal magnitude
/// (`LARGE_NUMBER` / `PROCESS_MEGABYTES` / `PROCESS` / `PROCESS_SHADOW`).
/// `ULLONG_MAX` renders `"        N/A "`.
///
/// Signature mapping: the C `char buffer[13]` holding the 12-char
/// `"%11llu "` render (always 12 chars — the divided value fits 11
/// digits in every branch) is an owned `String`; the C's
/// `RichString_appendnAscii(str, color, buffer + off, len)` byte-offset
/// appends become subslices `&buf[off..]` with the same `len`.
pub fn Row_printCount(str: &mut RichString, number: u64, coloring: bool) {
    let scheme = ColorScheme::active();
    let color_of = |e: ColorElements| e.packed(scheme);

    let large_number_color = if coloring { color_of(LARGE_NUMBER) } else { color_of(PROCESS) };
    let megabytes_color = if coloring { color_of(PROCESS_MEGABYTES) } else { color_of(PROCESS) };
    let shadow_color = if coloring { color_of(PROCESS_SHADOW) } else { color_of(PROCESS) };
    let base_color = color_of(PROCESS);

    if number == u64::MAX {
        RichString_appendAscii(str, color_of(PROCESS_SHADOW), b"        N/A ");
    } else if number >= 100000 * ONE_DECIMAL_T {
        let buf = format!("{:11} ", number / ONE_DECIMAL_G);
        let b = buf.as_bytes();
        RichString_appendnAscii(str, large_number_color, b, 12);
    } else if number >= 100 * ONE_DECIMAL_T {
        let buf = format!("{:11} ", number / ONE_DECIMAL_M);
        let b = buf.as_bytes();
        RichString_appendnAscii(str, large_number_color, b, 8);
        RichString_appendnAscii(str, megabytes_color, &b[8..], 4);
    } else if number >= 10 * ONE_DECIMAL_G {
        let buf = format!("{:11} ", number / ONE_DECIMAL_K);
        let b = buf.as_bytes();
        RichString_appendnAscii(str, large_number_color, b, 5);
        RichString_appendnAscii(str, megabytes_color, &b[5..], 3);
        RichString_appendnAscii(str, base_color, &b[8..], 4);
    } else {
        let buf = format!("{:11} ", number);
        let b = buf.as_bytes();
        RichString_appendnAscii(str, large_number_color, b, 2);
        RichString_appendnAscii(str, megabytes_color, &b[2..], 3);
        RichString_appendnAscii(str, base_color, &b[5..], 3);
        RichString_appendnAscii(str, shadow_color, &b[8..], 4);
    }
}

/// Port of `Row.c:333`.
///
/// C signature:
/// `void Row_printTime(RichString* str, unsigned long long totalHundredths, bool coloring)`.
///
/// Formats a duration given in hundredths of a second, switching layout
/// by magnitude (`MM:SS.hh`, `HHhMM:SS`, `Nd HHh MMm`, `DDDd HHh`,
/// `YYYy DDDd`, `NNNNNNNy`, and finally `"eternity "`) and coloring the
/// year/day/hour groups (`LARGE_NUMBER` / `PROCESS_GIGABYTES` /
/// `PROCESS_MEGABYTES` / `PROCESS`). `0` renders `" 0:00.00 "`.
pub fn Row_printTime(str: &mut RichString, total_hundredths: u64, coloring: bool) {
    let scheme = ColorScheme::active();
    let color_of = |e: ColorElements| e.packed(scheme);

    if total_hundredths == 0 {
        let shadow_color = if coloring { color_of(PROCESS_SHADOW) } else { color_of(PROCESS) };
        RichString_appendAscii(str, shadow_color, b" 0:00.00 ");
        return;
    }

    let year_color = if coloring { color_of(LARGE_NUMBER) } else { color_of(PROCESS) };
    let day_color = if coloring { color_of(PROCESS_GIGABYTES) } else { color_of(PROCESS) };
    let hour_color = if coloring { color_of(PROCESS_MEGABYTES) } else { color_of(PROCESS) };
    let base_color = color_of(PROCESS);

    let total_seconds = total_hundredths / 100;
    let total_minutes = total_seconds / 60;
    let total_hours = total_minutes / 60;
    let seconds = (total_seconds % 60) as u32;
    let minutes = (total_minutes % 60) as u32;

    if total_minutes < 60 {
        let hundredths = (total_hundredths % 100) as u32;
        let buf = format!("{:2}:{:02}.{:02} ", total_minutes as u32, seconds, hundredths);
        RichString_appendnAscii(str, base_color, buf.as_bytes(), buf.len());
        return;
    }
    if total_hours < 24 {
        let buf = format!("{:2}h", total_hours as u32);
        RichString_appendnAscii(str, hour_color, buf.as_bytes(), buf.len());
        let buf = format!("{:02}:{:02} ", minutes, seconds);
        RichString_appendnAscii(str, base_color, buf.as_bytes(), buf.len());
        return;
    }

    let total_days = total_hours / 24;
    let hours = (total_hours % 24) as u32;
    if total_days < 10 {
        let buf = format!("{}d", total_days as u32);
        RichString_appendnAscii(str, day_color, buf.as_bytes(), buf.len());
        let buf = format!("{:02}h", hours);
        RichString_appendnAscii(str, hour_color, buf.as_bytes(), buf.len());
        let buf = format!("{:02}m ", minutes);
        RichString_appendnAscii(str, base_color, buf.as_bytes(), buf.len());
        return;
    }
    if total_days < /* Ignore leap years */ 365 {
        let buf = format!("{:4}d", total_days as u32);
        RichString_appendnAscii(str, day_color, buf.as_bytes(), buf.len());
        let buf = format!("{:02}h ", hours);
        RichString_appendnAscii(str, hour_color, buf.as_bytes(), buf.len());
        return;
    }

    let years = total_days / 365;
    let days = (total_days % 365) as u32;
    if years < 1000 {
        let buf = format!("{:3}y", years as u32);
        RichString_appendnAscii(str, year_color, buf.as_bytes(), buf.len());
        let buf = format!("{:03}d ", days);
        RichString_appendnAscii(str, day_color, buf.as_bytes(), buf.len());
    } else if years < 10000000 {
        let buf = format!("{:7}y ", years);
        RichString_appendnAscii(str, year_color, buf.as_bytes(), buf.len());
    } else {
        RichString_appendAscii(str, year_color, b"eternity ");
    }
}

/// Port of `Row.c:403`.
///
/// C signature:
/// `void Row_printNanoseconds(RichString* str, unsigned long long totalNanoseconds, bool coloring)`.
///
/// Formats a nanosecond duration with a magnitude-dependent layout
/// (`"%6luns "`, `"%u.%04ums "`, a variable-precision seconds form, and
/// `"M:SS.mmm "`); at ≥ 600 s it converts to hundredths and defers to
/// [`Row_printTime`]. `0` renders `"     0ns "`. Only the zero case is
/// colored specially (`PROCESS_SHADOW`); every other branch uses
/// `PROCESS` (`base_color`).
pub fn Row_printNanoseconds(str: &mut RichString, total_nanoseconds: u64, coloring: bool) {
    let scheme = ColorScheme::active();
    let color_of = |e: ColorElements| e.packed(scheme);

    if total_nanoseconds == 0 {
        let shadow_color = if coloring { color_of(PROCESS_SHADOW) } else { color_of(PROCESS) };
        RichString_appendAscii(str, shadow_color, b"     0ns ");
        return;
    }

    let base_color = color_of(PROCESS);

    if total_nanoseconds < 1000000 {
        let buf = format!("{:6}ns ", total_nanoseconds);
        RichString_appendnAscii(str, base_color, buf.as_bytes(), buf.len());
        return;
    }

    if total_nanoseconds < 10000000 {
        // The precision is 0.1 microseconds here. We print the unit in
        // "ms" rather than microseconds to avoid the Greek "mu" choice.
        let mut fraction = (total_nanoseconds as u32) / 100;
        let milliseconds = fraction / 10000;
        fraction %= 10000;
        let buf = format!("{}.{:04}ms ", milliseconds, fraction);
        RichString_appendnAscii(str, base_color, buf.as_bytes(), buf.len());
        return;
    }

    let total_microseconds = total_nanoseconds / 1000;
    let total_seconds = total_microseconds / 1000000;
    let microseconds = (total_microseconds % 1000000) as u32;
    if total_seconds < 60 {
        let mut width: i32 = 6;
        let mut fraction = microseconds;
        let mut limit: u64 = 1;
        while total_seconds >= limit {
            width -= 1;
            fraction /= 10;
            limit *= 10;
        }
        // "%.u" prints no digits if (totalSeconds == 0).
        let secs = if total_seconds == 0 {
            String::new()
        } else {
            format!("{}", total_seconds as u32)
        };
        let buf = format!("{}.{:0w$}s ", secs, fraction, w = width as usize);
        RichString_appendnAscii(str, base_color, buf.as_bytes(), buf.len());
        return;
    }

    if total_seconds < 600 {
        let minutes = (total_seconds as u32) / 60;
        let seconds = (total_seconds as u32) % 60;
        let milliseconds = microseconds / 1000;
        let buf = format!("{}:{:02}.{:03} ", minutes, seconds, milliseconds);
        RichString_appendnAscii(str, base_color, buf.as_bytes(), buf.len());
        return;
    }

    let total_hundredths = total_microseconds / 1000 / 10;
    Row_printTime(str, total_hundredths, coloring);
}

/// Port of `Row.c:462`.
///
/// C signature:
/// `void Row_printRate(RichString* str, double rate, bool coloring)`.
///
/// Formats a transfer rate (bytes/second) as `"%7.2f X/s "` with a
/// magnitude-scaled unit (`B`, `K`, `M`, `G`, `T`, `P`), coloring by band
/// (`PROCESS_SHADOW` for sub-0.005 and invalid, `PROCESS` for B/K,
/// `PROCESS_MEGABYTES` for M, `LARGE_NUMBER` for G and above). A negative
/// or NaN rate renders `"        N/A "`.
///
/// `isNonnegative(rate)` (`Macros.h`) is `isgreaterequal(rate, 0.0)`,
/// false for NaN; inlined as `rate >= 0.0` (Rust `>=` is quiet for NaN).
/// The `ONE_*` size constants are `u64`; the C compares/divides `rate`
/// (a `double`) against them with the usual float promotion, so they are
/// cast to `f64` here.
pub fn Row_printRate(str: &mut RichString, rate: f64, coloring: bool) {
    let scheme = ColorScheme::active();
    let color_of = |e: ColorElements| e.packed(scheme);

    let mut large_number_color = color_of(LARGE_NUMBER);
    let mut megabytes_color = color_of(PROCESS_MEGABYTES);
    let shadow_color = color_of(PROCESS_SHADOW);
    let base_color = color_of(PROCESS);

    if !coloring {
        large_number_color = color_of(PROCESS);
        megabytes_color = color_of(PROCESS);
    }

    if !(rate >= 0.0) {
        RichString_appendAscii(str, shadow_color, b"        N/A ");
    } else if rate < 0.005 {
        let buf = format!("{:7.2} B/s ", rate);
        RichString_appendnAscii(str, shadow_color, buf.as_bytes(), buf.len());
    } else if rate < ONE_K as f64 {
        let buf = format!("{:7.2} B/s ", rate);
        RichString_appendnAscii(str, base_color, buf.as_bytes(), buf.len());
    } else if rate < ONE_M as f64 {
        let buf = format!("{:7.2} K/s ", rate / ONE_K as f64);
        RichString_appendnAscii(str, base_color, buf.as_bytes(), buf.len());
    } else if rate < ONE_G as f64 {
        let buf = format!("{:7.2} M/s ", rate / ONE_M as f64);
        RichString_appendnAscii(str, megabytes_color, buf.as_bytes(), buf.len());
    } else if rate < ONE_T as f64 {
        let buf = format!("{:7.2} G/s ", rate / ONE_G as f64);
        RichString_appendnAscii(str, large_number_color, buf.as_bytes(), buf.len());
    } else if rate < ONE_P as f64 {
        let buf = format!("{:7.2} T/s ", rate / ONE_T as f64);
        RichString_appendnAscii(str, large_number_color, buf.as_bytes(), buf.len());
    } else {
        let buf = format!("{:7.2} P/s ", rate / ONE_P as f64);
        RichString_appendnAscii(str, large_number_color, buf.as_bytes(), buf.len());
    }
}

/// Port of `Row.c:501`.
///
/// C signature:
/// `void Row_printLeftAlignedField(RichString* str, int attr, const char* content, unsigned int width)`.
///
/// Appends up to `width` display columns of `content` (via
/// [`RichString_appendnWideColumns`]) then pads with spaces to fill the
/// field, `width + 1 - columns` of them.
///
/// Signature mapping: `content` (C `const char*`, measured with
/// `strlen`) is a `&[u8]`; its length is `content.len()`. The column
/// out-param `columns` starts at `width`, is overwritten by the append
/// with the columns actually written, and the pad count is
/// `width + 1 - columns`.
pub fn Row_printLeftAlignedField(str: &mut RichString, attr: i32, content: &[u8], width: u32) {
    let mut columns: i32 = width as i32;
    RichString_appendnWideColumns(str, attr, content, content.len(), &mut columns);
    RichString_appendChr(str, attr, ' ', width as i32 + 1 - columns);
}

/// Port of `Row.c:507`.
///
/// C signature:
/// `int Row_printPercentage(float val, char* buffer, size_t n, uint8_t width, int* attr)`.
///
/// Formats `val` as a fixed-width percentage (e.g. `" 50.0 "`) and
/// selects a color branch for the caller.
///
/// Signature mapping:
/// - `buffer`/`n` + the `int` (byte-count) return → an owned `String`,
///   the same mapping `meter.rs` / `xutils.rs` apply. Rust owns its
///   allocation, so the `xSnprintf` truncate-or-abort size cap is
///   dropped. `n` is retained as a parameter because the C clamps the
///   field width with `CLAMP(width, 4, n - 2)`, which is load-bearing on
///   the output; `n` no longer bounds the returned buffer.
/// - `int* attr` out-param → the `attr: &mut PercentageAttr` out-param,
///   kept as a by-reference out-param exactly like the C so the
///   "leave `*attr` unchanged" branch is preserved (the fn writes only
///   in the branches where the C assigns). The C writes a
///   `CRT_colors[...]` index into `*attr`; CRT colors are not ported, so
///   the enum mirrors the discrete branch the C selects. See
///   [`PercentageAttr`].
///
/// The two C `assert(...)` preconditions are ported as `debug_assert!`
/// (C `assert` compiles out under `NDEBUG`, Rust `debug_assert!` under
/// release — same behavior): they are debug-only preconditions, not
/// input validation, and the second is the assert embedded in `CLAMP`.
pub fn Row_printPercentage(mut val: f32, n: usize, width: u8, attr: &mut PercentageAttr) -> String {
    debug_assert!(n >= 6 && width >= 4, "Invalid width in Row_printPercentage()");
    // truncate in favour of abort in xSnprintf()
    // CLAMP(x, low, high) = (assert(low <= high), x > high ? high : MAXIMUM(x, low))
    let high = n - 2;
    debug_assert!(4 <= high); // CLAMP's embedded assert(low <= high)
    let w = width as usize;
    let width = (if w > high { high } else { w.max(4) }) as u8;
    debug_assert!((width as usize) < n - 1, "Insufficient space to print column");

    // isNonnegative(val) from Macros.h:141 (isgreaterequal(x, 0.0)):
    // val >= 0.0, false for NaN. Rust's `>=` is quiet for NaN. Inlined
    // because Macros.h is not a ported module (no free fn introduced —
    // the port gate rejects non-C fn names).
    if val >= 0.0 {
        if val < 0.05_f32 {
            *attr = PercentageAttr::Shadow;
        } else if val >= 99.9_f32 {
            *attr = PercentageAttr::Megabytes;
        }

        let mut precision: usize = 1;

        // Display "val" as "100" for columns like "MEM%".
        if width == 4 && val > 99.9_f32 {
            precision = 0;
            val = 100.0_f32;
        }

        // C: xSnprintf(buffer, n, "%*.*f ", width, precision, val)
        return format!("{:>width$.precision$} ", val, width = width as usize, precision = precision);
    }

    *attr = PercentageAttr::Shadow;
    // C: xSnprintf(buffer, n, "%*.*s ", width, width, "N/A")
    let w = width as usize;
    format!("{:>width$.precision$} ", "N/A", width = w, precision = w)
}

/// TODO: port of `void Row_toggleTag(Row* this` from `Row.c:534`.
pub fn Row_toggleTag() {
    todo!("port of Row.c:534")
}

/// TODO: port of `int Row_compare(const void* v1, const void* v2` from `Row.c:538`.
pub fn Row_compare() {
    todo!("port of Row.c:538")
}

/// TODO: port of `int Row_compareByParent_Base(const void* v1, const void* v2` from `Row.c:545`.
pub fn Row_compareByParent_Base() {
    todo!("port of Row.c:545")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: run with a fresh Unchanged sentinel so the returned attr
    // reflects exactly which branch fired (Unchanged = no write).
    fn run(val: f32, n: usize, width: u8) -> (String, PercentageAttr) {
        let mut attr = PercentageAttr::Unchanged;
        let s = Row_printPercentage(val, n, width, &mut attr);
        (s, attr)
    }

    #[test]
    fn zero_is_shadow_and_zero_point_zero() {
        // 0.0 < 0.05 => Shadow; width 5, precision 1; "%5.1f " of 0.0.
        assert_eq!(run(0.0, 7, 5), ("  0.0 ".to_string(), PercentageAttr::Shadow));
    }

    #[test]
    fn below_shadow_threshold_is_shadow() {
        // 0.04 < 0.05 => Shadow; rounds to "0.0" at precision 1.
        assert_eq!(run(0.04, 7, 5), ("  0.0 ".to_string(), PercentageAttr::Shadow));
    }

    #[test]
    fn mid_range_leaves_attr_unchanged() {
        // 0.05 <= 50.0 < 99.9 => no branch fires, attr stays Unchanged.
        // "%5.1f " of 50.0 => " 50.0 ".
        assert_eq!(run(50.0, 7, 5), (" 50.0 ".to_string(), PercentageAttr::Unchanged));
    }

    #[test]
    fn unchanged_branch_preserves_caller_attr() {
        // C never touches *attr in the mid-range branch: a pre-set value
        // must survive the call.
        let mut attr = PercentageAttr::Megabytes;
        let s = Row_printPercentage(50.0, 7, 5, &mut attr);
        assert_eq!(s, " 50.0 ");
        assert_eq!(attr, PercentageAttr::Megabytes);
    }

    #[test]
    fn at_ninety_nine_nine_is_megabytes_precision_one() {
        // 99.9 >= 99.9 => Megabytes; width 4 but 99.9 > 99.9 is FALSE,
        // so precision stays 1, val stays 99.9 => "99.9 ".
        assert_eq!(run(99.9, 6, 4), ("99.9 ".to_string(), PercentageAttr::Megabytes));
    }

    #[test]
    fn hundred_at_width_five_keeps_one_decimal() {
        // >= 99.9 => Megabytes; width != 4 so precision 1, val 100.0.
        // "%5.1f " of 100.0 => "100.0 ".
        assert_eq!(run(100.0, 7, 5), ("100.0 ".to_string(), PercentageAttr::Megabytes));
    }

    #[test]
    fn mem_percent_width_four_collapses_to_integer() {
        // MEM% column: width == 4 && val > 99.9 => precision 0, val=100.
        // "%4.0f " of 100.0 => " 100 ". Also >= 99.9 => Megabytes.
        assert_eq!(run(100.0, 6, 4), (" 100 ".to_string(), PercentageAttr::Megabytes));
    }

    #[test]
    fn negative_is_na_and_shadow() {
        // val < 0.0 (not nonnegative) => Shadow; "%5.5s " of "N/A".
        assert_eq!(run(-1.0, 7, 5), ("  N/A ".to_string(), PercentageAttr::Shadow));
    }

    #[test]
    fn nan_is_na_and_shadow() {
        // isNonnegative(NaN) is false => Shadow + N/A path.
        assert_eq!(run(f32::NAN, 7, 5), ("  N/A ".to_string(), PercentageAttr::Shadow));
    }

    #[test]
    fn width_clamped_to_n_minus_two() {
        // width 200 clamped to CLAMP(200, 4, n-2) = 4 (n=6). 50.0 is
        // mid-range, width==4 but not > 99.9 => precision 1.
        // "%4.1f " of 50.0 => "50.0 ".
        assert_eq!(run(50.0, 6, 200), ("50.0 ".to_string(), PercentageAttr::Unchanged));
    }

    // ── RichString number formatters ──────────────────────────────────
    //
    // Color assertions compare each cell's stored attr against the
    // active scheme's `ColorElements::X.packed(...)` masked to 24 bits
    // (the `RichString` append masks `attrs & 0xffffff`; `crt.rs` packs
    // the fg/bg/attr into that low range). Reading `active()` here
    // mirrors what the formatter read internally on the same call.

    /// The visible characters of the valid `[0, chlen)` range.
    fn text(r: &RichString) -> String {
        r.chptr
            .iter()
            .take(r.chlen as usize)
            .map(|c| c.chars)
            .collect()
    }

    /// The stored attr of every valid cell.
    fn cell_attrs(r: &RichString) -> Vec<i32> {
        r.chptr.iter().take(r.chlen as usize).map(|c| c.attr).collect()
    }

    /// `CRT_colors[element]` as the append layer stores it (masked to the
    /// low 24 bits, matching `RichString_writeFromAscii`).
    fn col(element: ColorElements) -> i32 {
        element.packed(ColorScheme::active()) & 0xffffff
    }

    fn kbytes(n: u64, coloring: bool) -> RichString {
        let mut r = RichString::new();
        Row_printKBytes(&mut r, n, coloring);
        r
    }

    #[test]
    fn kbytes_zero_plain_process() {
        let r = kbytes(0, true);
        assert_eq!(text(&r), "    0 ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(PROCESS)));
    }

    #[test]
    fn kbytes_under_1000_plain() {
        let r = kbytes(999, true);
        assert_eq!(text(&r), "  999 ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(PROCESS)));
    }

    #[test]
    fn kbytes_k_m_split_colors_the_thousands_group() {
        // 1500 KiB: "%2u" of 1 (next-unit color) + "%03u " of 500 (base).
        let r = kbytes(1500, true);
        assert_eq!(text(&r), " 1500 ");
        // " 1" -> PROCESS_MEGABYTES, "500 " -> PROCESS
        let a = cell_attrs(&r);
        assert_eq!(&a[0..2], &[col(PROCESS_MEGABYTES), col(PROCESS_MEGABYTES)]);
        assert!(a[2..6].iter().all(|&x| x == col(PROCESS)));
    }

    #[test]
    fn kbytes_k_m_split_no_coloring_all_process() {
        let r = kbytes(1500, false);
        assert_eq!(text(&r), " 1500 ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(PROCESS)));
    }

    #[test]
    fn kbytes_97_6_mib_two_digit_decimal() {
        // 100000 KiB = 97.65625 MiB -> "97.6M ".
        let r = kbytes(100000, true);
        assert_eq!(text(&r), "97.6M ");
        // "97" -> M color, ".6" -> prev(PROCESS), "M " -> M color.
        let a = cell_attrs(&r);
        assert_eq!(&a[0..2], &[col(PROCESS_MEGABYTES); 2]); // 97
        assert_eq!(&a[2..4], &[col(PROCESS); 2]); // .6
        assert_eq!(&a[4..6], &[col(PROCESS_MEGABYTES); 2]); // M-space
    }

    #[test]
    fn kbytes_9_76_gib_one_digit_decimal() {
        // 10240000 KiB -> one promotion to GiB, units 9.76 -> "9.76G ".
        let r = kbytes(10240000, true);
        assert_eq!(text(&r), "9.76G ");
        // "9" -> GIGABYTES, ".76" -> prev(MEGABYTES), "G " -> GIGABYTES.
        let a = cell_attrs(&r);
        assert_eq!(a[0], col(PROCESS_GIGABYTES));
        assert_eq!(&a[1..4], &[col(PROCESS_MEGABYTES); 3]);
        assert_eq!(&a[4..6], &[col(PROCESS_GIGABYTES); 2]);
    }

    #[test]
    fn kbytes_10_0_gib() {
        // 10 GiB = 10485760 KiB -> "10.0G ".
        let r = kbytes(10485760, true);
        assert_eq!(text(&r), "10.0G ");
        let a = cell_attrs(&r);
        assert_eq!(&a[0..2], &[col(PROCESS_GIGABYTES); 2]); // 10
        assert_eq!(&a[2..4], &[col(PROCESS_MEGABYTES); 2]); // .0
        assert_eq!(&a[4..6], &[col(PROCESS_GIGABYTES); 2]); // G-space
    }

    #[test]
    fn kbytes_four_digit_megabytes() {
        // 2 GiB = 2097152 KiB stays in M (units 2048 < 10000) -> "2048M ".
        let r = kbytes(2097152, true);
        assert_eq!(text(&r), "2048M ");
        // "2" -> next-unit (GIGABYTES), "048M " -> color (MEGABYTES).
        let a = cell_attrs(&r);
        assert_eq!(a[0], col(PROCESS_GIGABYTES));
        assert!(a[1..6].iter().all(|&x| x == col(PROCESS_MEGABYTES)));
    }

    #[test]
    fn kbytes_invalid_na_shadow_when_coloring() {
        let r = kbytes(u64::MAX, true);
        assert_eq!(text(&r), "  N/A ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(PROCESS_SHADOW)));
    }

    #[test]
    fn kbytes_invalid_na_process_when_not_coloring() {
        let r = kbytes(u64::MAX, false);
        assert_eq!(text(&r), "  N/A ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(PROCESS)));
    }

    #[test]
    fn bytes_divides_by_one_k_then_formats() {
        // 1500 bytes / 1024 = 1 KiB -> "    1 ".
        let mut r = RichString::new();
        Row_printBytes(&mut r, 1500, true);
        assert_eq!(text(&r), "    1 ");
        // ULLONG_MAX forwards as the invalid sentinel.
        let mut r2 = RichString::new();
        Row_printBytes(&mut r2, u64::MAX, true);
        assert_eq!(text(&r2), "  N/A ");
    }

    fn count(n: u64, coloring: bool) -> RichString {
        let mut r = RichString::new();
        Row_printCount(&mut r, n, coloring);
        r
    }

    #[test]
    fn count_small_four_color_bands() {
        // 0 -> "%11llu " -> "          0 " (12 chars), else-branch splits
        // 2/3/3/4 across large/mega/base/shadow.
        let r = count(0, true);
        assert_eq!(text(&r), "          0 ");
        let a = cell_attrs(&r);
        assert!(a[0..2].iter().all(|&x| x == col(LARGE_NUMBER)));
        assert!(a[2..5].iter().all(|&x| x == col(PROCESS_MEGABYTES)));
        assert!(a[5..8].iter().all(|&x| x == col(PROCESS)));
        assert!(a[8..12].iter().all(|&x| x == col(PROCESS_SHADOW)));
    }

    #[test]
    fn count_invalid_na() {
        let r = count(u64::MAX, true);
        assert_eq!(text(&r), "        N/A ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(PROCESS_SHADOW)));
    }

    #[test]
    fn count_hundred_peta_decimal_all_large() {
        // >= 100000 * ONE_DECIMAL_T -> number / ONE_DECIMAL_G, all large.
        let r = count(100_000 * 1_000_000_000_000, true); // 1e17
        assert_eq!(text(&r), "  100000000 ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(LARGE_NUMBER)));
    }

    fn time(h: u64, coloring: bool) -> RichString {
        let mut r = RichString::new();
        Row_printTime(&mut r, h, coloring);
        r
    }

    #[test]
    fn time_zero_shadow() {
        let r = time(0, true);
        assert_eq!(text(&r), " 0:00.00 ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(PROCESS_SHADOW)));
    }

    #[test]
    fn time_sub_minute_base_color() {
        // 100 hundredths = 1 s -> " 0:01.00 ".
        let r = time(100, true);
        assert_eq!(text(&r), " 0:01.00 ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(PROCESS)));
    }

    #[test]
    fn time_one_minute() {
        // 6000 hundredths = 60 s -> " 1:00.00 ".
        assert_eq!(text(&time(6000, true)), " 1:00.00 ");
    }

    #[test]
    fn time_hour_layout() {
        // 1 h = 360000 hundredths -> " 1h00:00 ".
        let r = time(360000, true);
        assert_eq!(text(&r), " 1h00:00 ");
        let a = cell_attrs(&r);
        assert!(a[0..3].iter().all(|&x| x == col(PROCESS_MEGABYTES))); // " 1h"
        assert!(a[3..9].iter().all(|&x| x == col(PROCESS))); // "00:00 "
    }

    #[test]
    fn time_day_under_ten() {
        // 1 day = 8640000 hundredths -> "1d00h00m ".
        let r = time(8640000, true);
        assert_eq!(text(&r), "1d00h00m ");
        let a = cell_attrs(&r);
        assert_eq!(&a[0..2], &[col(PROCESS_GIGABYTES); 2]); // "1d"
        assert_eq!(&a[2..5], &[col(PROCESS_MEGABYTES); 3]); // "00h"
        assert_eq!(&a[5..9], &[col(PROCESS); 4]); // "00m "
    }

    #[test]
    fn time_day_under_year() {
        // 10 days = 86400000 hundredths -> "  10d00h ".
        let r = time(86_400_000, true);
        assert_eq!(text(&r), "  10d00h ");
        let a = cell_attrs(&r);
        assert!(a[0..5].iter().all(|&x| x == col(PROCESS_GIGABYTES))); // "  10d"
        assert!(a[5..9].iter().all(|&x| x == col(PROCESS_MEGABYTES))); // "00h "
    }

    #[test]
    fn time_year_under_thousand() {
        // 365 days = 3153600000 hundredths -> 1 year, 0 days -> "  1y000d ".
        let r = time(3_153_600_000, true);
        assert_eq!(text(&r), "  1y000d ");
        let a = cell_attrs(&r);
        assert!(a[0..4].iter().all(|&x| x == col(LARGE_NUMBER))); // "  1y"
        assert!(a[4..9].iter().all(|&x| x == col(PROCESS_GIGABYTES))); // "000d "
    }

    #[test]
    fn time_thousand_years_wide() {
        // 1000 years -> "%7luy " -> "   1000y ".
        let r = time(1000 * 365 * 8_640_000, true);
        assert_eq!(text(&r), "   1000y ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(LARGE_NUMBER)));
    }

    #[test]
    fn time_eternity() {
        let r = time(u64::MAX, true);
        assert_eq!(text(&r), "eternity ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(LARGE_NUMBER)));
    }

    fn nanos(n: u64) -> RichString {
        let mut r = RichString::new();
        Row_printNanoseconds(&mut r, n, true);
        r
    }

    #[test]
    fn nanoseconds_zero_shadow() {
        let r = nanos(0);
        assert_eq!(text(&r), "     0ns ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(PROCESS_SHADOW)));
    }

    #[test]
    fn nanoseconds_sub_microsecond_range() {
        // 500 ns -> "   500ns ".
        let r = nanos(500);
        assert_eq!(text(&r), "   500ns ");
        assert!(cell_attrs(&r).iter().all(|&a| a == col(PROCESS)));
    }

    #[test]
    fn nanoseconds_millisecond_range() {
        // 5_000_000 ns = 5 ms -> "5.0000ms ".
        assert_eq!(text(&nanos(5_000_000)), "5.0000ms ");
    }

    #[test]
    fn nanoseconds_sub_second_zero_seconds() {
        // 1e8 ns = 0.1 s, totalSeconds == 0 -> "%.u" empty -> ".100000s ".
        assert_eq!(text(&nanos(100_000_000)), ".100000s ");
    }

    #[test]
    fn nanoseconds_five_seconds_variable_precision() {
        // 5e9 ns = 5 s -> width shrinks to 5 -> "5.00000s ".
        assert_eq!(text(&nanos(5_000_000_000)), "5.00000s ");
    }

    #[test]
    fn nanoseconds_minute_range() {
        // 6e10 ns = 60 s -> "1:00.000 ".
        assert_eq!(text(&nanos(60_000_000_000)), "1:00.000 ");
    }

    #[test]
    fn nanoseconds_defers_to_print_time_at_ten_minutes() {
        // 6e11 ns = 600 s -> defers to Row_printTime(60000) -> "10:00.00 ".
        assert_eq!(text(&nanos(600_000_000_000)), "10:00.00 ");
    }

    fn rate(r: f64, coloring: bool) -> RichString {
        let mut s = RichString::new();
        Row_printRate(&mut s, r, coloring);
        s
    }

    #[test]
    fn rate_negative_and_nan_are_na_shadow() {
        for r in [-1.0, f64::NAN] {
            let s = rate(r, true);
            assert_eq!(text(&s), "        N/A ");
            assert!(cell_attrs(&s).iter().all(|&a| a == col(PROCESS_SHADOW)));
        }
    }

    #[test]
    fn rate_tiny_is_shadow_bytes() {
        // 0.0 < 0.005 -> shadow-colored B/s.
        let s = rate(0.0, true);
        assert_eq!(text(&s), "   0.00 B/s ");
        assert!(cell_attrs(&s).iter().all(|&a| a == col(PROCESS_SHADOW)));
    }

    #[test]
    fn rate_bytes_base_color() {
        let s = rate(1.0, true);
        assert_eq!(text(&s), "   1.00 B/s ");
        assert!(cell_attrs(&s).iter().all(|&a| a == col(PROCESS)));
    }

    #[test]
    fn rate_kilobytes_base_color() {
        // 2048 B/s = 2.00 K/s.
        let s = rate(2048.0, true);
        assert_eq!(text(&s), "   2.00 K/s ");
        assert!(cell_attrs(&s).iter().all(|&a| a == col(PROCESS)));
    }

    #[test]
    fn rate_megabytes_mega_color() {
        let s = rate(ONE_M as f64, true);
        assert_eq!(text(&s), "   1.00 M/s ");
        assert!(cell_attrs(&s).iter().all(|&a| a == col(PROCESS_MEGABYTES)));
    }

    #[test]
    fn rate_gigabytes_large_color() {
        let s = rate(ONE_G as f64, true);
        assert_eq!(text(&s), "   1.00 G/s ");
        assert!(cell_attrs(&s).iter().all(|&a| a == col(LARGE_NUMBER)));
    }

    #[test]
    fn rate_tera_and_peta() {
        assert_eq!(text(&rate(ONE_T as f64, true)), "   1.00 T/s ");
        assert_eq!(text(&rate(ONE_P as f64, true)), "   1.00 P/s ");
    }

    #[test]
    fn rate_no_coloring_uses_process_for_scaled_units() {
        // Without coloring, mega/large collapse to PROCESS; shadow stays.
        let s = rate(ONE_G as f64, false);
        assert_eq!(text(&s), "   1.00 G/s ");
        assert!(cell_attrs(&s).iter().all(|&a| a == col(PROCESS)));
    }

    #[test]
    fn left_aligned_field_pads_to_width_plus_one() {
        // "abc" in width 5 -> 3 columns written, pad = 5+1-3 = 3 spaces.
        let mut r = RichString::new();
        Row_printLeftAlignedField(&mut r, 0, b"abc", 5);
        assert_eq!(text(&r), "abc   ");
    }

    #[test]
    fn left_aligned_field_truncates_to_width() {
        // "abcdefgh" in width 3 -> 3 columns, pad = 3+1-3 = 1 space.
        let mut r = RichString::new();
        Row_printLeftAlignedField(&mut r, 0, b"abcdefgh", 3);
        assert_eq!(text(&r), "abc ");
    }

    #[test]
    fn left_aligned_field_short_content() {
        // "x" in width 4 -> 1 column, pad = 4+1-1 = 4 spaces.
        let mut r = RichString::new();
        Row_printLeftAlignedField(&mut r, 0, b"x", 4);
        assert_eq!(text(&r), "x    ");
    }
}
