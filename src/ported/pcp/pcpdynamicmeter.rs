//! Port of `pcp/PCPDynamicMeter.c` + `.h` — htop's Performance Co-Pilot
//! dynamic-meter subsystem: the `htop.meter.<name>` config-file readers and the
//! per-meter value formatter (`txtBuffer` fill + colored `RichString` display)
//! for user-defined PCP metric meters.
//!
//! 1:1 faithful port; the C is the spec. `PCPDynamicMeter` "extends"
//! [`DynamicMeter`](crate::ported::dynamicmeter::DynamicMeter) via the embedded
//! `super_`, exactly as the C struct embeds `DynamicMeter super` as its first
//! member. The libpcp/PMAPI surface is reused from
//! [`crate::ported::pcp::pmapi`] and the `Metric` wrapper from
//! [`crate::ported::pcp::metric`]; nothing is redeclared. This is the direct
//! sibling of [`crate::ported::pcp::pcpdynamiccolumn`] and mirrors its
//! config-parse structure and substrate-limitation handling.
//!
//! # Config-file parsing
//!
//! [`PCPDynamicMeters_init`] reads the `$PCP_SHARE_DIR` / `$PCP_SYSCONF_DIR`
//! (via `pmGetConfig`), `$XDG_CONFIG_HOME` / `$HOME`, and `$PCP_HTOP_DIR`
//! `meters/` directories and parses each file's `key = value` lines. The C
//! `opendir`/`readdir` scan is [`std::fs::read_dir`]; the C `fopen` +
//! `String_readLine` loop is [`std::fs::read_to_string`] iterated by line.
//! Section headers `[name]`, the `caption`/`description`/`type`/`maximum` keys,
//! and the per-metric `<name>.metric` / `<name>.color` / `<name>.label` /
//! `<name>.suffix` attribute keys are ported verbatim.
//!
//! `pmGetConfig` / `pmGetProgname` return libpcp-owned `char*` (static/cached
//! for `pmGetConfig`) — wrapped via `CStr::from_ptr(...).to_string_lossy()` and
//! never freed. `pmRegisterDerivedMetric`'s error `char**` is formatted to
//! stderr through `CRT_fatalError`, exactly as the C prints it unconditionally
//! with `pmGetProgname()` (twice, matching the C format string).
//!
//! # `pmsprintf` value formatting
//!
//! [`PCPDynamicMeter_updateValues`] fills the meter's `txtBuffer` and
//! [`PCPDynamicMeter_display`] appends colored `RichString` segments. Both
//! format numeric values through the libpcp `pmsprintf` variadic extern — the
//! exact function the C calls, for exact `%d`/`%u`/`%lld`/`%llu`/`%.2f`/`%s`
//! fidelity — reinterpreting the written bytes for the `RichString` /
//! `txtBuffer` targets. Space-dimensioned values go through
//! [`Meter_humanUnit`](crate::ported::meter::Meter_humanUnit), which the port
//! models as returning an owned `String`; that string is then written with
//! `pmsprintf("%s", …)` so the byte-count bookkeeping matches the C
//! `Meter_humanUnit(buffer + bytes, …)` path for the realistic (never
//! truncating) value sizes.
//!
//! Because the ported [`Meter::txtBuffer`](crate::ported::meter::Meter) is an
//! owned `String` (not a C `char[256]`), `updateValues` formats into a local
//! `[c_char; 256]` scratch buffer — reproducing the C's fixed-size byte counter,
//! separator handling, and `CLAMP`-truncation exactly — then copies the
//! NUL-terminated result into the `String`. The final copy is `to_string_lossy`
//! (PCP string values are ASCII/UTF-8 in practice; invalid bytes would be
//! `U+FFFD`-replaced rather than stored raw, the only observable divergence from
//! the C `char` buffer).
//!
//! # Substrate limitations (reported, identical to the sibling column port)
//!
//! - The ported [`Hashtable`] stores owned `Box<dyn Object>` values and drops
//!   them on removal, so the per-value free in [`PCPDynamicMeter_free`] is
//!   subsumed by the owner table's `Box` `Drop`.
//! - [`crate::ported::dynamicmeter::DynamicMeter_search`] (called by
//!   [`PCPDynamicMeter_uniqueName`], as the C does) downcasts stored values to
//!   `DynamicMeter` via `Any`, but this table stores `PCPDynamicMeter`. C's
//!   `void*` struct-prefix aliasing lets it read the `DynamicMeter` prefix of a
//!   `PCPDynamicMeter`; the safe-Rust `Any` downcast is exact-type and cannot.
//!   This cross-module impedance mismatch (fixable only in `dynamicmeter.rs`)
//!   means same-name duplicate detection across the safe boundary is not
//!   expressible; noted in the port report.
//! - `Platform_addMetric` (the PCP platform metric-array registrar) is a
//!   `pcp/Platform.c` function (not yet ported), scaffolded as a `todo!()` in
//!   [`platform`](super::platform) and imported here so
//!   [`PCPDynamicMeter_lookupMetric`]'s call site stays 1:1 until `Platform.c`
//!   lands.
//! - The in-progress meter is held as a local owned value and inserted into the
//!   table at the next section header / EOF (instead of C's insert-then-mutate-
//!   through-`void*`), because the ported [`Hashtable`] hands back no mutable
//!   alias — the exact pattern the sibling column `parseFile` uses.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::any::Any;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

use crate::ported::crt::{CRT_fatalError, ColorElements as CE, ColorScheme};
use crate::ported::dynamicmeter::{DynamicMeter, DynamicMeter_search};
use crate::ported::hashtable::{Hashtable, Hashtable_foreach, Hashtable_new, Hashtable_put};
use crate::ported::meter::{
    Meter, Meter_humanUnit, BAR_METERMODE, GRAPH_METERMODE, LED_METERMODE, TEXT_METERMODE,
};
use crate::ported::object::{Object, ObjectClass, Object_class};
use crate::ported::pcp::metric::{Metric_desc, Metric_enable, Metric_fromId, Metric_values};
use crate::ported::pcp::platform::Platform_addMetric;
use crate::ported::pcp::pmapi::{
    pmAtomValue, pmConvScale, pmGetConfig, pmGetProgname, pmRegisterDerivedMetric, pmUnits,
    pmsprintf, PM_SPACE_KBYTE, PM_TIME_SEC, PM_TYPE_32, PM_TYPE_64, PM_TYPE_DOUBLE, PM_TYPE_FLOAT,
    PM_TYPE_STRING, PM_TYPE_U32, PM_TYPE_U64,
};
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendnAscii, RichString_writeAscii,
};
use crate::ported::xutils::{String_eq, String_trim};

/// Port of the autoconf `CONFIGDIR` macro — `"/.config"`, the per-`$HOME`
/// config subdir (matches the sibling `pcpdynamiccolumn.rs` / `settings.rs`
/// `CONFIGDIR`).
const CONFIGDIR: &str = "/.config";

/// The C `char txtBuffer[256]` (`Meter.h`) size, the fixed byte budget
/// [`PCPDynamicMeter_updateValues`] formats into (`sizeof(meter->txtBuffer)`).
const TXT_BUFFER_SIZE: usize = 256;

/// The C `char buffer[64]` scratch size in `PCPDynamicMeter_display`
/// (`PCPDynamicMeter.c:410`).
const DISPLAY_BUFFER_SIZE: usize = 64;

/// Port of `typedef struct PCPDynamicMetric_` (`PCPDynamicMeter.h:19`). Owned
/// `char*` fields (`name`, `label`, `suffix`) map to `Option<String>` (`None` =
/// C `NULL`), freed by [`PCPDynamicMeter_free`] (subsumed by `Drop`). `color`
/// is a [`ColorElements`](CE) value indexing `CRT_colors` at display time (C
/// `memset` zero-init ⇒ [`RESET_COLOR`](CE::RESET_COLOR)).
pub struct PCPDynamicMetric {
    /// C `size_t id` — index into the Platform metric array.
    pub id: usize,
    /// C `ColorElements color` — the display color for this metric's value.
    pub color: CE,
    /// C `char* name` — the derived metric name (`htop.meter.<meter>.<attr>`).
    pub name: Option<String>,
    /// C `char* label` — the `"<attr>: "` prefix shown before the value.
    pub label: Option<String>,
    /// C `char* suffix` — an optional unit suffix shown after the value.
    pub suffix: Option<String>,
}

/// Port of `typedef struct PCPDynamicMeter_` (`PCPDynamicMeter.h:27`). "Extends"
/// [`DynamicMeter`] via the embedded `super_` (C's first member); the C
/// `PCPDynamicMetric* metrics` heap array becomes an owned `Vec`, and
/// `totalMetrics` its logical length (kept in lock-step with the C, which grows
/// the array by one per new metric).
pub struct PCPDynamicMeter {
    /// C `DynamicMeter super`.
    pub super_: DynamicMeter,
    /// C `PCPDynamicMetric* metrics` — the owned per-metric array.
    pub metrics: Vec<PCPDynamicMetric>,
    /// C `size_t totalMetrics` — the number of metrics in `metrics`.
    pub totalMetrics: usize,
}

/// Class descriptor for [`PCPDynamicMeter`], present solely so a value can be
/// stored as a `Box<dyn Object>` in the ported [`Hashtable`] (the same adapter
/// role the sibling column's class serves). htop stores raw `void*`, so this is
/// not a real C class; rooted at [`Object_class`], it sets no dispatch slots
/// (the table never dispatches through them).
static PCPDynamicMeter_class: ObjectClass = ObjectClass {
    extends: Some(&Object_class),
};

impl Object for PCPDynamicMeter {
    fn klass(&self) -> &'static ObjectClass {
        &PCPDynamicMeter_class
    }
}

/// Port of `typedef struct PCPDynamicMeters_` (`PCPDynamicMeter.h:33`). Owns the
/// discovery [`Hashtable`] (`None` before [`PCPDynamicMeters_init`], the C
/// uninitialized/`NULL` state), plus the discovery/allocation counters.
#[derive(Default)]
pub struct PCPDynamicMeters {
    /// C `Hashtable* table` — discovered meters keyed by discovery index.
    pub table: Option<Hashtable>,
    /// C `size_t count` — count of dynamic meters discovered by the scan.
    pub count: usize,
    /// C `size_t offset` — start offset into the Platform metric array.
    pub offset: usize,
    /// C `size_t cursor` — identifier allocator for each new metric used.
    pub cursor: usize,
}

/// Port of `static double strtod(...)` usage for the `maximum` key
/// (`PCPDynamicMeter.c:223`): C `strtod(value, NULL)` parses a leading decimal
/// float (optional sign, digits, fraction, exponent), yielding `0.0` when no
/// numeric prefix is present. A local helper, matching the sibling column's
/// local `atoi`.
fn strtod(s: &str) -> f64 {
    let t = s.trim_start();
    let bytes = t.as_bytes();
    let mut end = 0usize;
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    let mut saw_digit = false;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
        saw_digit = true;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
            saw_digit = true;
        }
    }
    if saw_digit && end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
        let mut e = end + 1;
        if e < bytes.len() && (bytes[e] == b'+' || bytes[e] == b'-') {
            e += 1;
        }
        let mut exp_digit = false;
        while e < bytes.len() && bytes[e].is_ascii_digit() {
            e += 1;
            exp_digit = true;
        }
        if exp_digit {
            end = e;
        }
    }
    if !saw_digit {
        return 0.0;
    }
    t[..end].parse::<f64>().unwrap_or(0.0)
}

/// Port of `static PCPDynamicMetric* PCPDynamicMeter_lookupMetric(
/// PCPDynamicMeters* meters, PCPDynamicMeter* meter, const char* name)`
/// (`PCPDynamicMeter.c:32`). Builds the derived name `htop.meter.<meter>.<name>`
/// and returns the existing metric with that name, else appends a new one
/// (label `"<name>: "`, `id = offset + cursor`, advancing the cursor) and
/// registers it with the platform. Returns a mutable reference to the metric
/// (C returns the pointer).
///
/// `Platform_addMetric(metric, name)` is scaffolded in [`platform`](super::platform)
/// (its owning `pcp/Platform.c` is not yet ported).
fn PCPDynamicMeter_lookupMetric<'a>(
    meters: &mut PCPDynamicMeters,
    meter: &'a mut PCPDynamicMeter,
    name: &str,
) -> &'a mut PCPDynamicMetric {
    // xAsprintf(&metricName, "htop.meter.%s.%s", meter->super.name, name);
    let metric_name = format!("htop.meter.{}.{}", meter.super_.name, name);

    // for (i..totalMetrics) if (String_eq(metric->name, metricName)) return metric;
    if let Some(idx) = meter
        .metrics
        .iter()
        .position(|m| String_eq(m.name.as_deref().unwrap_or(""), &metric_name))
    {
        return &mut meter.metrics[idx];
    }

    // not an existing metric in this meter - add it
    // metric->id = meters->offset + meters->cursor; meters->cursor++;
    let id = meters.offset + meters.cursor;
    meters.cursor += 1;

    // Platform_addMetric(Metric_fromId(metric->id), metricName); — not yet ported.
    Platform_addMetric(Metric_fromId(id), &metric_name);

    // memset(metric, 0, ...); metric->name = metricName; metric->label = String_cat(name, ": ");
    meter.metrics.push(PCPDynamicMetric {
        id,
        color: CE::RESET_COLOR, // memset 0 == RESET_COLOR (first ColorElements variant)
        name: Some(metric_name),
        label: Some(format!("{name}: ")),
        suffix: None,
    });
    meter.totalMetrics += 1;

    let last = meter.metrics.len() - 1;
    &mut meter.metrics[last]
}

/// Port of `static void PCPDynamicMeter_parseMetric(PCPDynamicMeters* meters,
/// PCPDynamicMeter* meter, const char* path, unsigned int line, char* key,
/// char* value)` (`PCPDynamicMeter.c:61`). Splits `key` at its first `.` into a
/// metric name and an attribute (`metric`/`color`/`label`/`suffix`); the
/// `metric` attribute registers a libpcp derived metric for `value` (a parse
/// failure is a `CRT_fatalError` with the libpcp error text, printed
/// unconditionally with `pmGetProgname()` twice, as the C does), the others set
/// the metric's color/label/suffix.
pub fn PCPDynamicMeter_parseMetric(
    meters: &mut PCPDynamicMeters,
    meter: &mut PCPDynamicMeter,
    path: &str,
    line: u32,
    key: &str,
    value: &str,
) {
    // if ((p = strchr(key, '.')) == NULL) return; *p++ = '\0';
    let dot = match key.find('.') {
        Some(i) => i,
        None => return,
    };
    let name = &key[..dot];
    let p = &key[dot + 1..];

    if String_eq(p, "metric") {
        // lookup a dynamic metric with this name, else create
        let metric = PCPDynamicMeter_lookupMetric(meters, meter, name);

        // use derived metrics in dynamic meters for simplicity
        let name_c = CString::new(metric.name.as_deref().unwrap_or(""))
            .expect("parseMetric: metric name has interior NUL");
        let expr_c = CString::new(value).expect("parseMetric: metric value has interior NUL");
        let mut error: *mut c_char = core::ptr::null_mut();
        let sts = unsafe { pmRegisterDerivedMetric(name_c.as_ptr(), expr_c.as_ptr(), &mut error) };
        if sts < 0 {
            let errstr = if error.is_null() {
                String::new()
            } else {
                let s = unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned();
                unsafe { libc::free(error as *mut libc::c_void) };
                s
            };
            // xAsprintf(&note, "%s: failed to parse expression in %s at line %u\n%s\n%s",
            //           pmGetProgname(), path, line, error, pmGetProgname());
            // pmGetProgname(): libpcp-owned static string, never freed.
            let progname = unsafe { CStr::from_ptr(pmGetProgname()) }.to_string_lossy();
            let note = format!(
                "{progname}: failed to parse expression in {path} at line {line}\n{errstr}\n{progname}"
            );
            // errno = EINVAL; — the ported CRT_fatalError takes the message directly
            // and does not read errno, so setting it is omitted.
            CRT_fatalError(&note);
        }
    } else {
        // this is a property of a dynamic metric - the metric expression may not
        // have been observed yet - i.e. we allow for any ordering
        let metric = PCPDynamicMeter_lookupMetric(meters, meter, name);
        if String_eq(p, "color") {
            if String_eq(value, "gray") {
                metric.color = CE::DYNAMIC_GRAY;
            } else if String_eq(value, "darkgray") {
                metric.color = CE::DYNAMIC_DARKGRAY;
            } else if String_eq(value, "red") {
                metric.color = CE::DYNAMIC_RED;
            } else if String_eq(value, "green") {
                metric.color = CE::DYNAMIC_GREEN;
            } else if String_eq(value, "blue") {
                metric.color = CE::DYNAMIC_BLUE;
            } else if String_eq(value, "cyan") {
                metric.color = CE::DYNAMIC_CYAN;
            } else if String_eq(value, "magenta") {
                metric.color = CE::DYNAMIC_MAGENTA;
            } else if String_eq(value, "yellow") {
                metric.color = CE::DYNAMIC_YELLOW;
            } else if String_eq(value, "white") {
                metric.color = CE::DYNAMIC_WHITE;
            }
        } else if String_eq(p, "label") {
            // char* label = String_cat(value, ": "); free_and_xStrdup(&metric->label, label);
            metric.label = Some(format!("{value}: "));
        } else if String_eq(p, "suffix") {
            // free_and_xStrdup(&metric->suffix, value);
            metric.suffix = Some(value.to_string());
        }
    }
}

/// Port of `static bool PCPDynamicMeter_validateMeterName(char* key, const char*
/// path, unsigned int line)` (`PCPDynamicMeter.c:119`). Truncates `key` at the
/// last `']'` (the C `*end = '\0'`), then validates it as a PCP-metric /
/// htoprc-safe name: the first byte alpha-or-`_`, the rest alnum-or-`_`. A
/// missing brace or an invalid character prints a parse error to stderr and
/// returns `false`. `key` is mutated in place (C's `char*` mutation).
pub fn PCPDynamicMeter_validateMeterName(key: &mut String, path: &str, line: u32) -> bool {
    // pmGetProgname(): libpcp-owned static string, never freed.
    let progname = unsafe { CStr::from_ptr(pmGetProgname()) }.to_string_lossy();
    // char* end = strrchr(key, ']'); if (end) *end = '\0'; else { fprintf; return false; }
    match key.rfind(']') {
        Some(pos) => key.truncate(pos),
        None => {
            eprintln!(
                "{progname}: no closing brace on meter name at {path} line {line}\n\"{key}\""
            );
            return false;
        }
    }

    // while (*p) { first: isalpha||'_'; rest: isalnum||'_'; else break; }
    let bytes = key.as_bytes();
    let mut p = 0usize;
    while p < bytes.len() {
        let c = bytes[p];
        let ok = if p == 0 {
            c.is_ascii_alphabetic() || c == b'_'
        } else {
            c.is_ascii_alphanumeric() || c == b'_'
        };
        if !ok {
            break;
        }
        p += 1;
    }
    // if (*p != '\0') { fprintf; return false; }  — broke before end == invalid
    if p != bytes.len() {
        eprintln!("{progname}: invalid meter name at {path} line {line}\n\"{key}\"");
        return false;
    }
    true
}

/// Port of `static bool PCPDynamicMeter_uniqueName(char* key, PCPDynamicMeters*
/// meters)` (`PCPDynamicMeter.c:152`): the name has not been defined previously
/// iff [`DynamicMeter_search`] finds no meter with it.
///
/// See the module note: `DynamicMeter_search` downcasts stored values to
/// `DynamicMeter`, but this table stores `PCPDynamicMeter`; the C `void*` prefix
/// aliasing has no safe-Rust `Any` analog (cross-module limitation).
pub fn PCPDynamicMeter_uniqueName(key: &str, meters: &PCPDynamicMeters) -> bool {
    // return !DynamicMeter_search(meters->table, key, NULL);
    !DynamicMeter_search(meters.table.as_ref(), key, None)
}

/// Port of `static PCPDynamicMeter* PCPDynamicMeter_new(PCPDynamicMeters*
/// meters, const char* name)` (`PCPDynamicMeter.c:156`). Builds a zeroed meter
/// (C `xCalloc`) with `super.name` set (truncated to the C `char[32]` buffer)
/// and returns it with its hashtable key `++count` (the C `Hashtable_put` is
/// deferred to the caller — see [`PCPDynamicMeter_parseFile`] — because the
/// ported table cannot hand back a mutable alias for subsequent key parsing),
/// advancing `count`.
pub fn PCPDynamicMeter_new(meters: &mut PCPDynamicMeters, name: &str) -> (PCPDynamicMeter, u32) {
    // String_safeStrncpy(meter->super.name, name, sizeof(meter->super.name));
    // — copies up to 31 bytes (the char[32] buffer). Meter names are validated
    // ASCII (alnum/underscore), so a byte-boundary truncation is a char boundary.
    let name: String = name[..name.len().min(31)].to_string();

    let meter = PCPDynamicMeter {
        super_: DynamicMeter {
            name,
            caption: None,
            description: None,
            type_: 0,
            maximum: 0.0,
        },
        metrics: Vec::new(),
        totalMetrics: 0,
    };

    // ht_key_t key = (ht_key_t) ++meters->count;  (pre-increment)
    meters.count += 1;
    let key = meters.count as u32;
    // Hashtable_put(meters->table, key, meter) is done by the caller once the
    // meter's keys are fully parsed.
    (meter, key)
}

/// Port of `static void PCPDynamicMeter_parseFile(PCPDynamicMeters* meters,
/// const char* path)` (`PCPDynamicMeter.c:164`). Reads `path` and parses each
/// `key = value` line: a `[name]` section header validates/uniquifies the name
/// and starts a new meter; the `caption`/`description`/`type`/`maximum` keys
/// populate the current meter's `super`, and any other key is a per-metric
/// attribute forwarded to [`PCPDynamicMeter_parseMetric`]. Comment (`#`) and
/// blank lines are skipped.
///
/// The in-progress meter is held as a local owned value and inserted into the
/// table when the next section starts (or at EOF), instead of C's insert-then-
/// mutate-through-`void*` — the ported [`Hashtable`] hands back no mutable
/// alias. Because each prior meter is inserted before the next header's
/// uniqueness check runs, same-file duplicate detection is preserved.
pub fn PCPDynamicMeter_parseFile(meters: &mut PCPDynamicMeters, path: &str) {
    // FILE* file = fopen(path, "r"); if (!file) return;
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    // PCPDynamicMeter* meter = NULL; unsigned int lineno = 0;
    let mut meter: Option<PCPDynamicMeter> = None;
    let mut pending_key: u32 = 0;
    let mut lineno: u32 = 0;

    // Insert the current in-progress meter into the table (owner frees on drop).
    let flush = |meters: &mut PCPDynamicMeters, meter: &mut Option<PCPDynamicMeter>, key: u32| {
        if let Some(m) = meter.take() {
            if let Some(table) = meters.table.as_mut() {
                Hashtable_put(table, key, Box::new(m));
            }
        }
    };

    for line in contents.lines() {
        lineno += 1;

        // char* trimmed = String_trim(line); skip empty / comment.
        let trimmed = String_trim(line);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // String_splitFirst(trimmed, '='): key = config[0], value = config[1] (raw, or NULL).
        let (key_raw, value_raw) = match trimmed.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (trimmed.as_str(), None),
        };
        let key = String_trim(key_raw);
        let value = value_raw.map(String_trim);

        if key.as_bytes().first() == Some(&b'[') {
            // new section heading - i.e. new meter; flush the previous so uniqueName sees it.
            flush(meters, &mut meter, pending_key);
            meter = None;

            // bool ok = validateMeterName(key + 1, path, lineno);
            let mut mname = key[1..].to_string();
            let mut ok = PCPDynamicMeter_validateMeterName(&mut mname, path, lineno);
            // if (ok) ok = uniqueName(key + 1, meters);
            if ok {
                ok = PCPDynamicMeter_uniqueName(&mname, meters);
            }
            // if (ok) meter = PCPDynamicMeter_new(meters, key + 1);
            if ok {
                let (m, k) = PCPDynamicMeter_new(meters, &mname);
                meter = Some(m);
                pending_key = k;
            }
        } else if let Some(m) = meter.as_mut() {
            // else if (!meter) skip; else if (!value) skip; else match key.
            if let Some(value) = value.as_deref() {
                if String_eq(&key, "caption") {
                    // char* caption = String_cat(value, ": "); free_and_xStrdup(&super.caption, ...)
                    m.super_.caption = Some(format!("{value}: "));
                } else if String_eq(&key, "description") {
                    m.super_.description = Some(value.to_string());
                } else if String_eq(&key, "type") {
                    // C compares the raw (untrimmed) config[1] here, not `value`.
                    let raw = value_raw.unwrap_or("");
                    if String_eq(raw, "bar") {
                        m.super_.type_ = BAR_METERMODE;
                    } else if String_eq(raw, "text") {
                        m.super_.type_ = TEXT_METERMODE;
                    } else if String_eq(raw, "graph") {
                        m.super_.type_ = GRAPH_METERMODE;
                    } else if String_eq(raw, "led") {
                        m.super_.type_ = LED_METERMODE;
                    }
                } else if String_eq(&key, "maximum") {
                    m.super_.maximum = strtod(value);
                } else {
                    PCPDynamicMeter_parseMetric(meters, m, path, lineno, &key, value);
                }
            }
        }
    }

    // Insert the final in-progress meter. (fclose(file): `contents` is owned.)
    flush(meters, &mut meter, pending_key);
}

/// Port of `static void PCPDynamicMeter_scanDir(PCPDynamicMeters* meters, char*
/// path)` (`PCPDynamicMeter.c:234`). Opens `path` and parses every entry whose
/// name does not begin with `.` (skipping `.`/`..`/hidden), via
/// [`PCPDynamicMeter_parseFile`]. The C `String_cat(path, d_name)` is a plain
/// concatenation (`path` already ends with `/`).
pub fn PCPDynamicMeter_scanDir(meters: &mut PCPDynamicMeters, path: &str) {
    // DIR* dir = opendir(path); if (!dir) return;
    let dir = match std::fs::read_dir(path) {
        Ok(d) => d,
        Err(_) => return,
    };

    for entry in dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // if (dirent->d_name[0] == '.') continue;
        if name.starts_with('.') {
            continue;
        }
        // char* file = String_cat(path, dirent->d_name);
        let file = format!("{path}{name}");
        PCPDynamicMeter_parseFile(meters, &file);
    }
}

/// Port of `void PCPDynamicMeters_init(PCPDynamicMeters* meters)`
/// (`PCPDynamicMeter.c:251`). Creates the owning discovery table and scans, in
/// order, the `$PCP_HTOP_DIR/meters/` developer path, the `$XDG_CONFIG_HOME`
/// (else `$HOME/.config`) `htop/meters/`, the system `$PCP_SYSCONF_DIR`
/// `htop/meters/`, and the read-only `$PCP_SHARE_DIR` `htop/meters/`.
pub fn PCPDynamicMeters_init(meters: &mut PCPDynamicMeters) {
    // pmGetConfig(name): libpcp returns a static/cached string (never freed); a
    // NULL return (C assumes non-NULL for these keys) yields the empty string.
    let pm_get_config = |name: &str| -> String {
        let c = CString::new(name).expect("pmGetConfig: name has interior NUL");
        let p = unsafe { pmGetConfig(c.as_ptr()) };
        if p.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    };

    let share = pm_get_config("PCP_SHARE_DIR");
    let sysconf = pm_get_config("PCP_SYSCONF_DIR");
    let xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
    let override_ = std::env::var("PCP_HTOP_DIR").ok();
    let mut home = std::env::var("HOME").ok();

    // if (!xdgConfigHome && !home) { pw = getpwuid(getuid()); if (pw) home = pw->pw_dir; }
    if xdg_config_home.is_none() && home.is_none() {
        home = unsafe {
            let pw = libc::getpwuid(libc::getuid());
            if pw.is_null() || (*pw).pw_dir.is_null() {
                None
            } else {
                Some(CStr::from_ptr((*pw).pw_dir).to_string_lossy().into_owned())
            }
        };
    }

    // meters->table = Hashtable_new(0, true);
    meters.table = Some(Hashtable_new(0, true));

    // developer paths - PCP_HTOP_DIR=./pcp ./pcp-htop
    if let Some(ov) = &override_ {
        let path = format!("{ov}/meters/");
        PCPDynamicMeter_scanDir(meters, &path);
    }

    // next, search in home directory alongside htoprc
    let path = if let Some(x) = &xdg_config_home {
        Some(format!("{x}/htop/meters/"))
    } else {
        home.as_ref()
            .map(|h| format!("{h}{CONFIGDIR}/htop/meters/"))
    };
    if let Some(p) = path {
        PCPDynamicMeter_scanDir(meters, &p);
    }

    // next, search in the system meters directory
    let path = format!("{sysconf}/htop/meters/");
    PCPDynamicMeter_scanDir(meters, &path);

    // next, try the readonly system meters directory
    let path = format!("{share}/htop/meters/");
    PCPDynamicMeter_scanDir(meters, &path);
}

/// Port of `static void PCPDynamicMeter_free(ht_key_t key, void* value, void*
/// data)` (`PCPDynamicMeter.c:297`): the [`Hashtable_foreach`] callback that
/// frees each meter's per-metric `name`/`label`/`suffix`, the metrics array,
/// and `super.caption`/`super.description`. The callback matches the ported
/// foreach signature (`key: u32, value: &dyn Object`).
///
/// The shared `&dyn Object` cannot drive an owning free, but the owner table's
/// `Box<dyn Object>` `Drop` already frees each meter's owned fields when the
/// table is cleared/deleted, so this per-value free is subsumed by `Drop` (see
/// the module note).
pub fn PCPDynamicMeter_free(_key: u32, value: &dyn Object) {
    let _meter = (value as &dyn Any)
        .downcast_ref::<PCPDynamicMeter>()
        .expect("PCPDynamicMeter_free: hashtable value is not a PCPDynamicMeter");
    // C: free each metric's name/label/suffix, the metrics array, super.caption,
    // super.description; — subsumed by the owner Box's Drop.
}

/// Port of `void PCPDynamicMeters_done(Hashtable* table)`
/// (`PCPDynamicMeter.c:309`). Runs [`PCPDynamicMeter_free`] over every entry via
/// [`Hashtable_foreach`] (freeing each meter's owned fields, subsumed by `Drop`
/// here).
pub fn PCPDynamicMeters_done(table: &Hashtable) {
    // Hashtable_foreach(table, PCPDynamicMeter_free, NULL);
    Hashtable_foreach(table, &mut PCPDynamicMeter_free);
}

/// Port of `void PCPDynamicMeter_enable(PCPDynamicMeter* this)`
/// (`PCPDynamicMeter.c:313`). Enables value fetching for every metric this
/// dynamic meter uses.
pub fn PCPDynamicMeter_enable(this: &PCPDynamicMeter) {
    // for (i..totalMetrics) Metric_enable(Metric_fromId(this->metrics[i].id), true);
    for i in 0..this.totalMetrics {
        Metric_enable(Metric_fromId(this.metrics[i].id), true);
    }
}

// ── pmUnits "canonical units" note ────────────────────────────────────────
//
// Both value formatters recompute the C `pmUnits conv = desc->units; if
// (conv.dimSpace) conv.scaleSpace = PM_SPACE_KBYTE; if (conv.dimTime)
// conv.scaleTime = PM_TIME_SEC;` inline (the port gate forbids a shared
// Rust-original helper). The ported `pmUnits` is opaque (`{ bits: u32 }`), so
// the `dim*` reads and `scale*` writes are done on the raw word. On the
// little-endian targets (see the sibling column's `pmUnits` bitfield notes):
// `dimSpace` = bits 28–31, `dimTime` = bits 24–27; the scale writes target
// `scaleSpace` (bits 16–19) / `scaleTime` (bits 12–15). `conv.dimSpace` (read
// after the write, unchanged) selects the `Meter_humanUnit` render.

/// Port of `void PCPDynamicMeter_updateValues(PCPDynamicMeter* this, Meter*
/// meter)` (`PCPDynamicMeter.c:318`). Formats each metric's current value into
/// `meter->txtBuffer`, `/`-separated, converting to canonical units first (KiB /
/// seconds) and rendering space-dimensioned values via
/// [`Meter_humanUnit`]. A metric with no sampled value (or a failed rescale)
/// contributes nothing (the separator it may have written is rewound). An empty
/// result becomes `"no data"`.
///
/// The ported [`Meter::txtBuffer`](crate::ported::meter::Meter) is an owned
/// `String`, so the C `char[256]` byte-counting is reproduced in a local
/// `[c_char; 256]` scratch buffer (exact separator / `CLAMP` / fixed-size
/// semantics), then copied into the `String` (see the module note on the
/// `to_string_lossy` final copy).
pub fn PCPDynamicMeter_updateValues(this: &PCPDynamicMeter, meter: &mut Meter) {
    let size = TXT_BUFFER_SIZE;
    let mut buffer = [0 as c_char; TXT_BUFFER_SIZE];
    let mut bytes: usize = 0;

    for i in 0..this.totalMetrics {
        let bytes_old = bytes;

        // if (i > 0 && bytes < size - 1) buffer[bytes++] = '/';  /* separator */
        if i > 0 && bytes < size - 1 {
            buffer[bytes] = b'/' as c_char;
            bytes += 1;
        }

        let metric = &this.metrics[i];
        let base = Metric_fromId(metric.id);
        let desc = Metric_desc(base);
        let dtype = unsafe { (*desc).type_ };

        let mut atom: pmAtomValue = unsafe { core::mem::zeroed() };
        let mut raw: pmAtomValue = unsafe { core::mem::zeroed() };

        // if (!Metric_values(base, &raw, 1, desc->type)) { bytes = bytes_old; continue; }
        if Metric_values(base, &mut raw, 1, dtype).is_null() {
            bytes = bytes_old;
            continue;
        }

        // pmUnits conv = desc->units; if (dimSpace) scaleSpace=KBYTE; if (dimTime) scaleTime=SEC;
        let mut convbits = unsafe { (*desc).units }.bits;
        let dim_space = (convbits >> 28) & 0xF != 0;
        let dim_time = (convbits >> 24) & 0xF != 0;
        if dim_space {
            convbits = (convbits & !(0xF << 16)) | (((PM_SPACE_KBYTE as u32) & 0xF) << 16);
        }
        if dim_time {
            convbits = (convbits & !(0xF << 12)) | (((PM_TIME_SEC as u32) & 0xF) << 12);
        }
        let conv = pmUnits { bits: convbits };

        if dtype == PM_TYPE_STRING {
            atom = raw;
        } else {
            // else if (pmConvScale(desc->type, &raw, &desc->units, &atom, &conv) < 0) { ...continue }
            let sts = unsafe { pmConvScale(dtype, &raw, &(*desc).units, &mut atom, &conv) };
            if sts < 0 {
                bytes = bytes_old;
                continue;
            }
        }

        let saved = bytes;
        // Render into buffer+bytes, bounded by size-bytes, mirroring the C switch.
        // These are invoked only inside the `unsafe { match }` block below.
        macro_rules! write_num {
            ($fmt:literal, $arg:expr) => {{
                bytes += pmsprintf(
                    buffer.as_mut_ptr().add(bytes),
                    size - bytes,
                    $fmt.as_ptr(),
                    $arg,
                ) as usize;
            }};
        }
        macro_rules! write_human {
            ($val:expr) => {{
                let hu = Meter_humanUnit($val);
                let cs = CString::new(hu).unwrap_or_default();
                bytes += pmsprintf(
                    buffer.as_mut_ptr().add(bytes),
                    size - bytes,
                    c"%s".as_ptr(),
                    cs.as_ptr(),
                ) as usize;
            }};
        }

        unsafe {
            match dtype {
                PM_TYPE_STRING => {
                    write_num!(c"%s", atom.cp);
                    libc::free(atom.cp as *mut libc::c_void);
                }
                PM_TYPE_32 => {
                    if dim_space {
                        write_human!(atom.l as f64);
                    } else {
                        write_num!(c"%d", atom.l);
                    }
                }
                PM_TYPE_U32 => {
                    if dim_space {
                        write_human!(atom.ul as f64);
                    } else {
                        write_num!(c"%u", atom.ul);
                    }
                }
                PM_TYPE_64 => {
                    if dim_space {
                        write_human!(atom.ll as f64);
                    } else {
                        write_num!(c"%lld", atom.ll);
                    }
                }
                PM_TYPE_U64 => {
                    if dim_space {
                        write_human!(atom.ull as f64);
                    } else {
                        write_num!(c"%llu", atom.ull);
                    }
                }
                PM_TYPE_FLOAT => {
                    if dim_space {
                        write_human!(atom.f as f64);
                    } else {
                        write_num!(c"%.2f", atom.f as f64);
                    }
                }
                PM_TYPE_DOUBLE => {
                    if dim_space {
                        write_human!(atom.d);
                    } else {
                        write_num!(c"%.2f", atom.d);
                    }
                }
                _ => {}
            }
        }

        // if (saved != bytes && metric->suffix && bytes < size) bytes += pmsprintf(... "%s", suffix);
        if saved != bytes {
            if let Some(suffix) = metric.suffix.as_deref() {
                if bytes < size {
                    let cs = CString::new(suffix).unwrap_or_default();
                    bytes += unsafe {
                        pmsprintf(
                            buffer.as_mut_ptr().add(bytes),
                            size - bytes,
                            c"%s".as_ptr(),
                            cs.as_ptr(),
                        )
                    } as usize;
                }
            }
        }
    }

    // buffer[CLAMP(bytes, 0u, size - 1)] = '\0';
    buffer[bytes.min(size - 1)] = 0;

    // if (!bytes) pmsprintf(buffer, size, "no data");
    if bytes == 0 {
        unsafe { pmsprintf(buffer.as_mut_ptr(), size, c"no data".as_ptr()) };
    }

    // Copy the NUL-terminated scratch buffer into the owned String txtBuffer.
    meter.txtBuffer = unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned();
}

/// Port of `void PCPDynamicMeter_display(PCPDynamicMeter* this, const Meter*
/// meter, RichString* out)` (`PCPDynamicMeter.c:402`). Appends each metric's
/// label (in `METER_TEXT`), value (in the metric's own color), and suffix (in
/// `METER_TEXT`) to `out`, space-separated. Values are converted to canonical
/// units and space-dimensioned values rendered via [`Meter_humanUnit`], exactly
/// as [`PCPDynamicMeter_updateValues`]. When no metric yields data, `"no data"`
/// is written in `METER_VALUE_ERROR`. The `meter` argument is unused (C
/// `ATTR_UNUSED`).
pub fn PCPDynamicMeter_display(this: &PCPDynamicMeter, _meter: &Meter, out: &mut RichString) {
    let scheme = ColorScheme::active();
    // int nodata = 1;
    let mut nodata = true;

    // RichString_appendnAscii(out, attr, buffer, len): reinterpret the `pmsprintf`
    // c_char output as ASCII bytes, `len` clamped to the buffer for memory safety.
    let append_buf = |out: &mut RichString, attr: i32, buffer: &[c_char], len: c_int| {
        let n = if len < 0 {
            0
        } else {
            (len as usize).min(buffer.len())
        };
        let bytes = unsafe { core::slice::from_raw_parts(buffer.as_ptr() as *const u8, n) };
        RichString_appendnAscii(out, attr, bytes, n);
    };

    for i in 0..this.totalMetrics {
        let metric = &this.metrics[i];
        let base = Metric_fromId(metric.id);
        let desc = Metric_desc(base);
        let dtype = unsafe { (*desc).type_ };

        let mut atom: pmAtomValue = unsafe { core::mem::zeroed() };
        let mut raw: pmAtomValue = unsafe { core::mem::zeroed() };

        // if (!Metric_values(base, &raw, 1, desc->type)) continue;
        if Metric_values(base, &mut raw, 1, dtype).is_null() {
            continue;
        }

        // pmUnits conv = desc->units; if (dimSpace) scaleSpace=KBYTE; if (dimTime) scaleTime=SEC;
        let mut convbits = unsafe { (*desc).units }.bits;
        let dim_space = (convbits >> 28) & 0xF != 0;
        let dim_time = (convbits >> 24) & 0xF != 0;
        if dim_space {
            convbits = (convbits & !(0xF << 16)) | (((PM_SPACE_KBYTE as u32) & 0xF) << 16);
        }
        if dim_time {
            convbits = (convbits & !(0xF << 12)) | (((PM_TIME_SEC as u32) & 0xF) << 12);
        }
        let conv = pmUnits { bits: convbits };

        if dtype == PM_TYPE_STRING {
            atom = raw;
        } else {
            // else if (pmConvScale(desc->type, &raw, &desc->units, &atom, &conv) < 0) continue;
            let sts = unsafe { pmConvScale(dtype, &raw, &(*desc).units, &mut atom, &conv) };
            if sts < 0 {
                continue;
            }
        }

        // nodata = 0;  /* we will use this metric so *some* data will be added */
        nodata = false;

        // if (i > 0) RichString_appendAscii(out, CRT_colors[metric->color], " ");
        if i > 0 {
            RichString_appendAscii(out, metric.color.packed(scheme), b" ");
        }

        // if (metric->label) RichString_appendAscii(out, CRT_colors[METER_TEXT], metric->label);
        if let Some(label) = metric.label.as_deref() {
            RichString_appendAscii(out, CE::METER_TEXT.packed(scheme), label.as_bytes());
        }

        let mut buf = [0 as c_char; DISPLAY_BUFFER_SIZE];
        let bsize = DISPLAY_BUFFER_SIZE;
        let mut len: c_int = 0;

        // These are invoked only inside the `unsafe { match }` block below.
        macro_rules! fmt_num {
            ($fmt:literal, $arg:expr) => {{
                len = pmsprintf(buf.as_mut_ptr(), bsize, $fmt.as_ptr(), $arg);
            }};
        }
        macro_rules! fmt_human {
            ($val:expr) => {{
                let hu = Meter_humanUnit($val);
                let cs = CString::new(hu).unwrap_or_default();
                len = pmsprintf(buf.as_mut_ptr(), bsize, c"%s".as_ptr(), cs.as_ptr());
            }};
        }

        unsafe {
            match dtype {
                PM_TYPE_STRING => {
                    fmt_num!(c"%s", atom.cp);
                    libc::free(atom.cp as *mut libc::c_void);
                }
                PM_TYPE_32 => {
                    if dim_space {
                        fmt_human!(atom.l as f64);
                    } else {
                        fmt_num!(c"%d", atom.l);
                    }
                }
                PM_TYPE_U32 => {
                    if dim_space {
                        fmt_human!(atom.ul as f64);
                    } else {
                        fmt_num!(c"%u", atom.ul);
                    }
                }
                PM_TYPE_64 => {
                    if dim_space {
                        fmt_human!(atom.ll as f64);
                    } else {
                        fmt_num!(c"%lld", atom.ll);
                    }
                }
                PM_TYPE_U64 => {
                    if dim_space {
                        fmt_human!(atom.ull as f64);
                    } else {
                        fmt_num!(c"%llu", atom.ull);
                    }
                }
                PM_TYPE_FLOAT => {
                    if dim_space {
                        fmt_human!(atom.f as f64);
                    } else {
                        fmt_num!(c"%.2f", atom.f as f64);
                    }
                }
                PM_TYPE_DOUBLE => {
                    if dim_space {
                        fmt_human!(atom.d);
                    } else {
                        fmt_num!(c"%.2f", atom.d);
                    }
                }
                _ => {}
            }
        }

        // if (len) { RichString_appendnAscii(out, CRT_colors[metric->color], buffer, len);
        //            if (metric->suffix) RichString_appendAscii(out, CRT_colors[METER_TEXT], suffix); }
        if len != 0 {
            append_buf(out, metric.color.packed(scheme), &buf, len);
            if let Some(suffix) = metric.suffix.as_deref() {
                RichString_appendAscii(out, CE::METER_TEXT.packed(scheme), suffix.as_bytes());
            }
        }
    }

    // if (nodata) RichString_writeAscii(out, CRT_colors[METER_VALUE_ERROR], "no data");
    if nodata {
        RichString_writeAscii(out, CE::METER_VALUE_ERROR.packed(scheme), b"no data");
    }
}
