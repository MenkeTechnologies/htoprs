//! Port of `pcp/PCPDynamicColumn.c` + `.h` — htop's Performance Co-Pilot
//! dynamic-column subsystem: the `htop.column.<name>` config-file readers and
//! the per-row value formatter for user-defined PCP metric columns.
//!
//! 1:1 faithful port; the C is the spec. `PCPDynamicColumn` "extends"
//! [`DynamicColumn`] via the
//! embedded `super_`, exactly as the C struct embeds `DynamicColumn super` as
//! its first member. The libpcp/PMAPI surface is reused from
//! [`crate::ported::pcp::pmapi`] and the `Metric` wrapper from
//! [`crate::ported::pcp::metric`]; nothing is redeclared.
//!
//! # Config-file parsing
//!
//! [`PCPDynamicColumns_init`] reads the `$PCP_SHARE_DIR` / `$PCP_SYSCONF_DIR`
//! (via `pmGetConfig`), `$XDG_CONFIG_HOME` / `$HOME`, and `$PCP_HTOP_DIR`
//! `columns/` directories and parses each file's `key = value` lines. The C
//! `opendir`/`readdir` scan is [`std::fs::read_dir`]; the C `fopen` +
//! `String_readLine` loop is [`std::fs::read_to_string`] iterated by line.
//! Section headers `[name]`, the `metric`/`width`/`format`/`caption`/… keys,
//! and comment/blank skipping are ported verbatim.
//!
//! `pmGetConfig` / `pmGetProgname` return libpcp-owned `char*` (static/cached
//! for `pmGetConfig`) — wrapped via `CStr::from_ptr(...).to_string_lossy()` and
//! never freed. `pmRegisterDerivedMetric`'s error `char**` is formatted to
//! stderr through `CRT_fatalError`, exactly as the C prints it unconditionally
//! with `pmGetProgname()`.
//!
//! # `pmsprintf` formatting
//!
//! [`PCPDynamicColumn_writeAtomValue`] formats into a `[c_char; …]` buffer by
//! calling the libpcp `pmsprintf` variadic extern (option **(a)** in the port
//! brief) — the exact function the C calls, for exact `%*.*s`/`%*d`/`%*.2f`/
//! `%*llu` fidelity — then reinterprets the written bytes for
//! [`RichString_appendnAscii`]. Format strings are C-string literals; widths
//! are passed as `c_int`, strings as `*const c_char`, the double as `f64`, and
//! the unsigned as `u64` (`c_ulonglong`) with the same C default-argument
//! promotions.
//!
//! # Substrate note
//!
//! [`PCPDynamicColumns_setupWidths`] drives its per-column `super.width` write
//! (C `Hashtable_foreach(columns->table, PCPDynamicColumn_setupWidth, NULL)`)
//! through `Hashtable::foreach_value_mut` — the `&mut` analog of the shared
//! [`Hashtable_foreach`], added for this one mutating callback. The per-value
//! free in [`PCPDynamicColumns_free`] is subsumed by the owner table's `Box`
//! `Drop`.
//!
//! [`PCPDynamicColumn_uniqueName`] calls
//! [`crate::ported::dynamiccolumn::DynamicColumn_search`] over this table (which
//! stores `PCPDynamicColumn`), as the C does. The search reads each value's
//! `DynamicColumn` base through [`Object::as_dynamic_column`] — the safe analog
//! of C's `(DynamicColumn*)value` struct-prefix cast, which
//! [`PCPDynamicColumn`] overrides to return its `super_`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::any::Any;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::atomic::Ordering;

use crate::ported::crt::{CRT_fatalError, ColorElements as CE, ColorScheme};
use crate::ported::dynamiccolumn::{
    DynamicColumn, DynamicColumn_done, DynamicColumn_search, DYNAMIC_DEFAULT_COLUMN_WIDTH,
    DYNAMIC_MAX_COLUMN_WIDTH,
};
use crate::ported::hashtable::{
    Hashtable, Hashtable_foreach, Hashtable_get, Hashtable_new, Hashtable_put,
};
use crate::ported::linux::cgrouputils::CGroup_filterName;
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass, Object_class};
use crate::ported::pcp::metric::{
    Metric_desc, Metric_externalName, Metric_fromId, Metric_instance, Metric_lookupText,
    Metric_type,
};
use crate::ported::pcp::pcpprocess::{PCPProcess, LAST_PROCESSFIELD};
use crate::ported::pcp::platform::Platform_addMetric;
use crate::ported::pcp::pmapi::{
    pmAtomValue, pmConvScale, pmDesc, pmGetConfig, pmGetProgname, pmRegisterDerivedMetric, pmUnits,
    pmsprintf, PM_SPACE_BYTE, PM_TIME_SEC, PM_TYPE_32, PM_TYPE_64, PM_TYPE_DOUBLE, PM_TYPE_FLOAT,
    PM_TYPE_STRING, PM_TYPE_U32, PM_TYPE_U64,
};
use crate::ported::process::spaceship_nullstr;
use crate::ported::process::Process_getPid;
use crate::ported::richstring::{RichString, RichString_appendnAscii};
use crate::ported::row::spaceship_number;
use crate::ported::row::{
    PercentageAttr, Row_pidDigits, Row_printBytes, Row_printCount, Row_printPercentage,
    Row_printRate, Row_printTime,
};
use crate::ported::settings::{RowField, Settings};
use crate::ported::xutils::{compareRealNumbers, String_eq, String_trim};

/// Port of `#define PM_COUNT_ONE 0` (`pmapi.h`). The base event-count scale;
/// not re-exported by [`crate::ported::pcp::pmapi`], so declared locally.
const PM_COUNT_ONE: c_int = 0;

/// Port of `#define PM_ERR_CONV (-PM_ERR_BASE-17)` (`pmapi.h`) with
/// `PM_ERR_BASE == 12345` ⇒ `-12362` (the same base that yields the ported
/// `PM_ERR_IPC == -12366` at offset `-21`). Not re-exported by
/// [`crate::ported::pcp::pmapi`], so declared locally. Used only as a `< 0`
/// error sentinel returned from [`PCPDynamicColumn_normalize`].
const PM_ERR_CONV: c_int = -12362;

/// Port of the autoconf `CONFIGDIR` macro — `"/.config"`, the per-`$HOME`
/// config subdir (matches `settings.rs`'s private `CONFIGDIR`).
const CONFIGDIR: &str = "/.config";

/// The `char buffer[DYNAMIC_MAX_COLUMN_WIDTH + 1 + 1]` from
/// `PCPDynamicColumn_writeAtomValue` (`PCPDynamicColumn.c:358`): the max column
/// width plus a trailing space plus a NUL terminator.
const BUFSIZE: usize = DYNAMIC_MAX_COLUMN_WIDTH as usize + 1 + 1;

/// Port of `typedef struct PCPDynamicColumn_` (`PCPDynamicColumn.h:22`).
/// "Extends" [`DynamicColumn`] via the embedded `super_` (C's first member);
/// owned `char*` fields (`metricName`, `format`) map to `Option<String>`
/// (`None` = C `NULL`), freed by [`PCPDynamicColumn_done`].
pub struct PCPDynamicColumn {
    /// C `DynamicColumn super`.
    pub super_: DynamicColumn,
    /// C `char* metricName` — the `htop.column.<name>` derived-metric name.
    pub metricName: Option<String>,
    /// C `char* format` — optional value format from the config file.
    pub format: Option<String>,
    /// C `size_t id` — identifier for metric-array lookups (`offset + cursor`).
    pub id: usize,
    /// C `int width` — optional width from the configuration file.
    pub width: i32,
    /// C `bool defaultEnabled` — default enabled in a dynamic screen.
    pub defaultEnabled: bool,
    /// C `bool percent`.
    pub percent: bool,
    /// C `bool instances` — an instance-*names* column, not values.
    pub instances: bool,
}

/// Class descriptor for [`PCPDynamicColumn`], present solely so a value can be
/// stored as a `Box<dyn Object>` in the ported [`Hashtable`] (the same adapter
/// role [`DynamicColumn`]'s class serves). htop stores raw `void*`, so this is
/// not a real C class; rooted at [`Object_class`], it sets no dispatch slots
/// (the table never dispatches through them).
static PCPDynamicColumn_class: ObjectClass = ObjectClass {
    extends: Some(&Object_class),
};

impl Object for PCPDynamicColumn {
    fn klass(&self) -> &'static ObjectClass {
        &PCPDynamicColumn_class
    }

    /// The `DynamicColumn` base is the `super_` prefix (C's
    /// `(DynamicColumn*)pcpColumn` cast) — lets `DynamicColumn_search` read a
    /// `PCPDynamicColumn` stored in the shared table.
    fn as_dynamic_column(&self) -> Option<&DynamicColumn> {
        Some(&self.super_)
    }
}

/// Port of `typedef struct PCPDynamicColumns_` (`PCPDynamicColumn.h:33`). Owns
/// the discovery [`Hashtable`] (`None` before [`PCPDynamicColumns_init`], the
/// C uninitialized/`NULL` state), plus the discovery/allocation counters.
#[derive(Default)]
pub struct PCPDynamicColumns {
    /// C `Hashtable* table` — discovered columns keyed by field id.
    pub table: Option<Hashtable>,
    /// C `size_t count` — count of dynamic columns discovered by the scan.
    pub count: usize,
    /// C `size_t offset` — start offset into the Platform metric array.
    pub offset: usize,
    /// C `size_t cursor` — identifier allocator for each new metric used.
    pub cursor: usize,
}

// ── pmUnits bitfield notes ────────────────────────────────────────────────
//
// The ported `pmUnits` is opaque (`{ bits: u32 }`) and exposes only
// `scaleSpace`/`scaleTime`, so the `dim*` reads and `scale*` writes the C does
// are computed inline on the raw word. On the little-endian targets
// (`HAVE_BITFIELDS_LTOR` undefined) `pmUnits` packs LSB-first: `pad:8,
// scaleCount:4, scaleTime:4, scaleSpace:4, dimCount:4, dimTime:4, dimSpace:4`
// (`pmapi.h`) — matching the ported `scaleTime`/`scaleSpace` shifts (bits 12–15
// / 16–19). `dimSpace`/`dimTime`/`dimCount` are the top three nibbles (bits
// 28–31 / 24–27 / 20–23); they are only tested for non-zero, so no sign
// extension is needed:
//   dimSpace != 0  ⇔  (units.bits >> 28) & 0xF != 0
//   dimTime  != 0  ⇔  (units.bits >> 24) & 0xF != 0
//   dimCount != 0  ⇔  (units.bits >> 20) & 0xF != 0

/// Port of `static bool PCPDynamicColumn_addMetric(PCPDynamicColumns* columns,
/// PCPDynamicColumn* column)` (`PCPDynamicColumn.c:35`). Names the derived
/// metric `htop.column.<name>`, assigns `column->id = offset + cursor`,
/// advances the cursor, and registers the metric with the platform. Returns
/// `false` for a column with an empty name (the C `!column->super.name[0]`).
///
/// `Platform_addMetric(metric, metricName)` is scaffolded in
/// [`platform`](super::platform) (its owning `pcp/Platform.c` is not yet ported).
pub fn PCPDynamicColumn_addMetric(
    columns: &mut PCPDynamicColumns,
    column: &mut PCPDynamicColumn,
) -> bool {
    // if (!column->super.name[0]) return false;
    if column.super_.name.is_empty() {
        return false;
    }

    // xAsprintf(&metricName, "htop.column.%s", column->super.name);
    let metric_name = format!("htop.column.{}", column.super_.name);
    column.metricName = Some(metric_name.clone());

    // column->id = columns->offset + columns->cursor; columns->cursor++;
    column.id = columns.offset + columns.cursor;
    columns.cursor += 1;

    // Metric metric = Metric_fromId(column->id);
    let metric = Metric_fromId(column.id);
    // Platform_addMetric(metric, metricName); — not yet ported.
    Platform_addMetric(metric, &metric_name);
    true
}

/// Port of `static void PCPDynamicColumn_parseMetric(PCPDynamicColumns*,
/// PCPDynamicColumn*, const char* path, unsigned int line, char* value)`
/// (`PCPDynamicColumn.c:51`). Looks up one-line help text (once) into the
/// column description, registers the column, then registers a libpcp derived
/// metric for `value`; a parse failure is a `CRT_fatalError` with the libpcp
/// error text (as the C prints unconditionally with `pmGetProgname()`).
pub fn PCPDynamicColumn_parseMetric(
    columns: &mut PCPDynamicColumns,
    column: &mut PCPDynamicColumn,
    path: &str,
    line: u32,
    value: &str,
) {
    // pmLookupText: if (!column->super.description) Metric_lookupText(value, &column->super.description);
    if column.super_.description.is_none() {
        let name_c = CString::new(value).expect("parseMetric: metric value has interior NUL");
        let mut descp: *mut c_char = ptr::null_mut();
        Metric_lookupText(name_c.as_c_str(), &mut descp);
        if !descp.is_null() {
            // The libpcp-malloc'd help text becomes an owned Rust String (C keeps
            // the char* directly; DynamicColumn_done frees it). Copy then free.
            let s = unsafe { CStr::from_ptr(descp) }
                .to_string_lossy()
                .into_owned();
            column.super_.description = Some(s);
            unsafe { libc::free(descp as *mut libc::c_void) };
        }
    }

    // if (PCPDynamicColumn_addMetric(columns, column) == false) return;
    if !PCPDynamicColumn_addMetric(columns, column) {
        return;
    }

    // if (pmRegisterDerivedMetric(column->metricName, value, &error) < 0) { ... CRT_fatalError }
    let name_c = CString::new(column.metricName.as_deref().unwrap_or(""))
        .expect("parseMetric: metricName has interior NUL");
    let expr_c = CString::new(value).expect("parseMetric: metric value has interior NUL");
    let mut error: *mut c_char = ptr::null_mut();
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
        // xAsprintf(&note, "%s: failed to parse expression in %s at line %u\n%s\n", ...)
        // pmGetProgname(): libpcp-owned static string, never freed.
        let progname = unsafe { CStr::from_ptr(pmGetProgname()) }.to_string_lossy();
        let note =
            format!("{progname}: failed to parse expression in {path} at line {line}\n{errstr}\n");
        // errno = EINVAL; — the ported CRT_fatalError takes the message directly
        // and does not read errno, so setting it is omitted.
        CRT_fatalError(&note);
    }
}

/// Port of `static bool PCPDynamicColumn_validateColumnName(char* key, const
/// char* path, unsigned int line)` (`PCPDynamicColumn.c:75`). Truncates `key` at
/// the last `']'` (the C `*end = '\0'`), then validates it as a PCP-metric /
/// htoprc-safe name: the first byte alpha-or-`_`, the rest alnum-or-`_`. A
/// missing brace or an invalid character prints a parse error to stderr and
/// returns `false`. `key` is mutated in place (C's `char*` mutation).
pub fn PCPDynamicColumn_validateColumnName(key: &mut String, path: &str, line: u32) -> bool {
    // pmGetProgname(): libpcp-owned static string, never freed.
    let progname = unsafe { CStr::from_ptr(pmGetProgname()) }.to_string_lossy();
    // char* end = strrchr(key, ']'); if (end) *end = '\0'; else { fprintf; return false; }
    match key.rfind(']') {
        Some(pos) => key.truncate(pos),
        None => {
            eprint!("{progname}: no closing brace on column name at {path} line {line}\n\"{key}\"");
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
        eprint!("{progname}: invalid column name at {path} line {line}\n\"{key}\"");
        return false;
    }
    true
}

/// Port of `static bool PCPDynamicColumn_uniqueName(char* key,
/// PCPDynamicColumns* columns)` (`PCPDynamicColumn.c:108`): the name has not been
/// defined previously iff [`DynamicColumn_search`] finds no column with it.
///
/// `DynamicColumn_search` reads each stored `PCPDynamicColumn`'s `DynamicColumn`
/// base through `Object::as_dynamic_column` (the safe analog of C's `void*`
/// prefix cast), so it matches a name defined via this table.
pub fn PCPDynamicColumn_uniqueName(key: &str, columns: &PCPDynamicColumns) -> bool {
    // return DynamicColumn_search(columns->table, key, NULL) == NULL;
    DynamicColumn_search(columns.table.as_ref(), key, None).is_none()
}

/// Port of `static PCPDynamicColumn* PCPDynamicColumn_new(PCPDynamicColumns*
/// columns, const char* name)` (`PCPDynamicColumn.c:112`). Builds a zeroed
/// column (C `xCalloc`) with `super.name` set (truncated to the C `char[32]`
/// buffer), `defaultEnabled = true`, and the other flags cleared. Returns the
/// new column and its hashtable key `count + LAST_PROCESSFIELD` (the C
/// `Hashtable_put` is deferred to the caller — see [`PCPDynamicColumn_parseFile`]
/// — because the ported table cannot hand back a mutable alias for subsequent
/// key parsing), and advances `count`.
pub fn PCPDynamicColumn_new(
    columns: &mut PCPDynamicColumns,
    name: &str,
) -> (PCPDynamicColumn, u32) {
    // String_safeStrncpy(column->super.name, name, sizeof(column->super.name));
    // — copies up to 31 bytes (the char[32] buffer). Column names are validated
    // ASCII (alnum/underscore), so a byte-boundary truncation is a char boundary.
    let name: String = name[..name.len().min(31)].to_string();

    let column = PCPDynamicColumn {
        super_: DynamicColumn {
            name,
            heading: None,
            caption: None,
            description: None,
            width: 0,
            enabled: false, // column->super.enabled = false;
            table: ptr::null(),
        },
        metricName: None,
        format: None,
        id: 0,
        width: 0,
        defaultEnabled: true, // column->defaultEnabled = true;
        percent: false,       // column->percent = false;
        instances: false,     // column->instances = false;
    };

    // ht_key_t id = (ht_key_t) columns->count + LAST_PROCESSFIELD;
    let key = columns.count as u32 + LAST_PROCESSFIELD as u32;
    // Hashtable_put(columns->table, id, column) is done by the caller once the
    // column's keys are fully parsed; columns->count++ happens here so the next
    // column and uniqueName bookkeeping stay consistent with the C.
    columns.count += 1;

    (column, key)
}

/// Port of `static void PCPDynamicColumn_parseFile(PCPDynamicColumns* columns,
/// const char* path)` (`PCPDynamicColumn.c:127`). Reads `path` and parses each
/// `key = value` line: a `[name]` section header validates/uniquifies the name
/// and starts a new column; the `caption`/`heading`/`description`/`width`/
/// `format`/`instances`/`default`/`enabled`/`metric` keys populate the current
/// column. Comment (`#`) and blank lines are skipped.
///
/// The in-progress column is held as a local owned value and inserted into the
/// table when the next section starts (or at EOF), instead of C's insert-then-
/// mutate-through-`void*` — the ported [`Hashtable`] hands back no mutable
/// alias. Because each prior column is inserted before the next header's
/// uniqueness check runs, same-file duplicate detection is preserved.
pub fn PCPDynamicColumn_parseFile(columns: &mut PCPDynamicColumns, path: &str) {
    // FILE* file = fopen(path, "r"); if (!file) return;
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    // PCPDynamicColumn* column = NULL; unsigned int lineno = 0;
    let mut column: Option<PCPDynamicColumn> = None;
    let mut pending_key: u32 = 0;
    let mut lineno: u32 = 0;

    // Insert the current in-progress column into the table (owner frees on drop).
    let flush =
        |columns: &mut PCPDynamicColumns, column: &mut Option<PCPDynamicColumn>, key: u32| {
            if let Some(c) = column.take() {
                if let Some(table) = columns.table.as_mut() {
                    Hashtable_put(table, key, Box::new(c));
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

        // String_splitFirst(trimmed, '='): key = config[0], value = config[1] (or NULL).
        // A trailing '=' with nothing after it yields NO value in C (its
        // `if (s[0] != '\0')` guard leaves n == 1, value NULL), so the line is
        // skipped — NOT applied with an empty string, which `str::split_once`
        // would otherwise return as `Some("")`.
        let (key_raw, value_opt) = match trimmed.split_once('=') {
            Some((k, "")) => (k, None),
            Some((k, v)) => (k, Some(v)),
            None => (trimmed.as_str(), None),
        };
        let key = String_trim(key_raw);
        let value = value_opt.map(String_trim);

        if key.as_bytes().first() == Some(&b'[') {
            // new section heading — flush the previous column so uniqueName sees it.
            flush(columns, &mut column, pending_key);
            column = None;

            // bool ok = validateColumnName(key + 1, path, lineno);
            let mut name = key[1..].to_string();
            let mut ok = PCPDynamicColumn_validateColumnName(&mut name, path, lineno);
            // if (ok) ok = uniqueName(key + 1, columns);
            if ok {
                ok = PCPDynamicColumn_uniqueName(&name, columns);
            }
            // if (ok) column = PCPDynamicColumn_new(columns, key + 1);
            if ok {
                let (c, k) = PCPDynamicColumn_new(columns, &name);
                column = Some(c);
                pending_key = k;
            }
        } else if let (Some(value), Some(c)) = (value.as_deref(), column.as_mut()) {
            // value && column && String_eq(key, ...)
            if String_eq(&key, "caption") {
                c.super_.caption = Some(value.to_string());
            } else if String_eq(&key, "heading") {
                c.super_.heading = Some(value.to_string());
            } else if String_eq(&key, "description") {
                c.super_.description = Some(value.to_string());
            } else if String_eq(&key, "width") {
                c.super_.width = atoi(value);
            } else if String_eq(&key, "format") {
                c.format = Some(value.to_string());
            } else if String_eq(&key, "instances") {
                if String_eq(value, "True") || String_eq(value, "true") {
                    c.instances = true;
                }
            } else if String_eq(&key, "default") || String_eq(&key, "enabled") {
                if String_eq(value, "False") || String_eq(value, "false") {
                    c.defaultEnabled = false;
                }
            } else if String_eq(&key, "metric") {
                PCPDynamicColumn_parseMetric(columns, c, path, lineno, value);
            }
        }
    }

    // Insert the final in-progress column. (fclose(file): `contents` is owned.)
    flush(columns, &mut column, pending_key);
}

/// C `atoi` as used for the `width` key (`PCPDynamicColumn.c:170`): optional
/// sign then base-10 digits, `0` when none. Mirrors `settings.rs`'s private
/// `atoi`.
fn atoi(s: &str) -> i32 {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut sign: i32 = 1;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        if bytes[i] == b'-' {
            sign = -1;
        }
        i += 1;
    }
    let mut n: i32 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        n = n.wrapping_mul(10).wrapping_add((bytes[i] - b'0') as i32);
        i += 1;
    }
    sign.wrapping_mul(n)
}

/// Port of `static void PCPDynamicColumn_scanDir(PCPDynamicColumns* columns,
/// char* path)` (`PCPDynamicColumn.c:189`). Opens `path` and parses every entry
/// whose name does not begin with `.` (skipping `.`/`..`/hidden), via
/// [`PCPDynamicColumn_parseFile`]. The C `String_cat(path, d_name)` is a plain
/// concatenation (`path` already ends with `/`).
pub fn PCPDynamicColumn_scanDir(columns: &mut PCPDynamicColumns, path: &str) {
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
        PCPDynamicColumn_parseFile(columns, &file);
    }
}

/// Port of `void PCPDynamicColumns_init(PCPDynamicColumns* columns)`
/// (`PCPDynamicColumn.c:206`). Creates the owning discovery table and scans, in
/// order, the `$PCP_HTOP_DIR/columns/` developer path, the `$XDG_CONFIG_HOME`
/// (else `$HOME/.config`) `htop/columns/`, the system `$PCP_SYSCONF_DIR`
/// `htop/columns/`, and the read-only `$PCP_SHARE_DIR` `htop/columns/`.
pub fn PCPDynamicColumns_init(columns: &mut PCPDynamicColumns) {
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

    // columns->table = Hashtable_new(0, true);
    columns.table = Some(Hashtable_new(0, true));

    // developer paths - PCP_HTOP_DIR=./pcp ./pcp-htop
    if let Some(ov) = &override_ {
        let path = format!("{ov}/columns/");
        PCPDynamicColumn_scanDir(columns, &path);
    }

    // next, search in home directory alongside htoprc
    let path = if let Some(x) = &xdg_config_home {
        Some(format!("{x}/htop/columns/"))
    } else {
        home.as_ref()
            .map(|h| format!("{h}{CONFIGDIR}/htop/columns/"))
    };
    if let Some(p) = path {
        PCPDynamicColumn_scanDir(columns, &p);
    }

    // next, search in the system columns directory
    let path = format!("{sysconf}/htop/columns/");
    PCPDynamicColumn_scanDir(columns, &path);

    // next, try the readonly system columns directory
    let path = format!("{share}/htop/columns/");
    PCPDynamicColumn_scanDir(columns, &path);
}

/// Port of `void PCPDynamicColumn_done(PCPDynamicColumn* this)`
/// (`PCPDynamicColumn.c:252`). Runs the base [`DynamicColumn_done`] (frees
/// `heading`/`caption`/`description`) then frees `metricName`/`format` (dropping
/// the owned `Option<String>`s). Does not free the struct storage — the `_done`
/// contract, matching the base port.
pub fn PCPDynamicColumn_done(this: &mut PCPDynamicColumn) {
    // DynamicColumn_done(&this->super);
    DynamicColumn_done(&mut this.super_);
    // free(this->metricName);
    this.metricName = None;
    // free(this->format);
    this.format = None;
}

/// Port of `static void PCPDynamicColumns_free(ht_key_t key, void* value, void*
/// data)` (`PCPDynamicColumn.c:258`): the [`Hashtable_foreach`] callback that
/// runs [`PCPDynamicColumn_done`] on each column. The callback matches the
/// ported foreach signature (`key: u32, value: &dyn Object`).
///
/// The shared `&dyn Object` cannot drive the `&mut` `PCPDynamicColumn_done`, but
/// the owner table's `Box<dyn Object>` `Drop` already frees each column's owned
/// strings when the table is cleared/deleted, so this per-value free is
/// subsumed by `Drop` (see the module note).
pub fn PCPDynamicColumns_free(_key: u32, value: &dyn Object) {
    let _column = (value as &dyn Any)
        .downcast_ref::<PCPDynamicColumn>()
        .expect("PCPDynamicColumns_free: hashtable value is not a PCPDynamicColumn");
    // C: PCPDynamicColumn_done(column); — subsumed by the owner Box's Drop.
}

/// Port of `void PCPDynamicColumns_done(Hashtable* table)`
/// (`PCPDynamicColumn.c:263`). Runs [`PCPDynamicColumns_free`] over every entry
/// via [`Hashtable_foreach`] (freeing each column's owned fields, subsumed by
/// `Drop` here). Does not delete the table itself (C frees the contents, not the
/// table — the table is deleted separately by its owner).
pub fn PCPDynamicColumns_done(table: &Hashtable) {
    // Hashtable_foreach(table, PCPDynamicColumns_free, NULL);
    Hashtable_foreach(table, &mut PCPDynamicColumns_free);
}

/// Port of `static void PCPDynamicColumn_setupWidth(ht_key_t key, void* value,
/// void* data)` (`PCPDynamicColumn.c:267`). Computes `column->super.width` from
/// the config file and the metric's descriptor/units: instance-name and
/// string-typed columns use the config width (default `-16`); `percent`/`process`
/// formats use fixed widths; an explicit config width wins; otherwise the width
/// is derived from the metric's space/time/count dimensions.
///
/// Takes `&mut PCPDynamicColumn` to perform the faithful in-place width write;
/// see [`PCPDynamicColumns_setupWidths`] and the module note on why the ported
/// table cannot drive this callback via `Hashtable_foreach`.
pub fn PCPDynamicColumn_setupWidth(column: &mut PCPDynamicColumn) {
    // Metric metric = Metric_fromId(column->id); const pmDesc* desc = Metric_desc(metric);
    let metric = Metric_fromId(column.id);
    let desc = Metric_desc(metric);
    let dtype = unsafe { (*desc).type_ };

    // if (column->instances || desc->type == PM_TYPE_STRING) { ... }
    if column.instances || dtype == PM_TYPE_STRING {
        column.super_.width = column.width;
        if column.super_.width == 0 {
            column.super_.width = -16;
        }
        return;
    }

    // if (column->format) { "percent" -> 5; "process" -> Process_pidDigits; }
    if let Some(fmt) = column.format.as_deref() {
        if fmt == "percent" {
            column.super_.width = 5;
            return;
        }
        if fmt == "process" {
            // C uses `Process_pidDigits`; the ported symbol is `Row_pidDigits`
            // (htop renamed `Process_pidDigits` -> `Row_pidDigits`).
            column.super_.width = Row_pidDigits.load(Ordering::Relaxed);
            return;
        }
    }

    // if (column->width) { column->super.width = column->width; return; }
    if column.width != 0 {
        column.super_.width = column.width;
        return;
    }

    // width from metric dimensions (dim* nibbles of the opaque units word).
    let units = unsafe { (*desc).units };
    let ds = (units.bits >> 28) & 0xF != 0; // dimSpace
    let dt = (units.bits >> 24) & 0xF != 0; // dimTime
    let dc = (units.bits >> 20) & 0xF != 0; // dimCount
    column.super_.width = if ds && dt {
        11 // Row_printRate
    } else if ds {
        5 // Row_printBytes
    } else if dc && dt {
        11 // Row_printCount
    } else if dt {
        8 // Row_printTime
    } else {
        11 // Row_printCount
    };
}

/// Port of `void PCPDynamicColumns_setupWidths(PCPDynamicColumns* columns)`
/// (`PCPDynamicColumn.c:310`): C runs `Hashtable_foreach(columns->table,
/// PCPDynamicColumn_setupWidth, NULL)` to set every column's `super.width` in
/// place.
///
/// The owning `columns.table` is held by value (`Option<Hashtable>`), so
/// `setupWidths` has exclusive `&mut` access and drives the per-column write
/// through `Hashtable::foreach_value_mut` — the `&mut` analog of
/// `Hashtable_foreach` for the one mutating C callback. Each stored value is a
/// `PCPDynamicColumn` (the exact type the C casts `value` to), so the callback
/// resolves it with an exact-type `downcast_mut`.
pub fn PCPDynamicColumns_setupWidths(columns: &mut PCPDynamicColumns) {
    // Hashtable_foreach(columns->table, PCPDynamicColumn_setupWidth, NULL);
    if let Some(table) = columns.table.as_mut() {
        table.foreach_value_mut(&mut |_key, value| {
            // PCPDynamicColumn* column = (PCPDynamicColumn*) value;
            let any: &mut dyn core::any::Any = value;
            if let Some(column) = any.downcast_mut::<PCPDynamicColumn>() {
                PCPDynamicColumn_setupWidth(column);
            }
        });
    }
}

/// Port of `static int PCPDynamicColumn_normalize(const pmDesc* desc, const
/// pmAtomValue* ap, double* value)` (`PCPDynamicColumn.c:315`). Rescales the
/// value to normalized units (bytes / seconds / count-one) via `pmConvScale`,
/// then widens the result to a `double`. Returns `0` on success, a negative
/// `pmConvScale` status or `PM_ERR_CONV` (unhandled type) otherwise.
pub fn PCPDynamicColumn_normalize(
    desc: *const pmDesc,
    ap: *const pmAtomValue,
    value: &mut f64,
) -> c_int {
    // Form normalized units from the original metric units.
    let src_units = unsafe { (*desc).units };
    let mut bits = src_units.bits;
    // if (units.dimTime) units.scaleTime = PM_TIME_SEC;   (bits 12-15)
    if (bits >> 24) & 0xF != 0 {
        bits = (bits & !(0xF << 12)) | (((PM_TIME_SEC as u32) & 0xF) << 12);
    }
    // if (units.dimSpace) units.scaleSpace = PM_SPACE_BYTE; (bits 16-19)
    if (bits >> 28) & 0xF != 0 {
        bits = (bits & !(0xF << 16)) | (((PM_SPACE_BYTE as u32) & 0xF) << 16);
    }
    // if (units.dimCount) units.scaleCount = PM_COUNT_ONE;  (bits 8-11)
    if (bits >> 20) & 0xF != 0 {
        bits = (bits & !(0xF << 8)) | (((PM_COUNT_ONE as u32) & 0xF) << 8);
    }
    let units = pmUnits { bits };

    let type_ = unsafe { (*desc).type_ };
    let mut atom: pmAtomValue = unsafe { core::mem::zeroed() };
    // if ((sts = pmConvScale(type, ap, &desc->units, &atom, &units)) < 0) return sts;
    let sts = unsafe { pmConvScale(type_, ap, &(*desc).units, &mut atom, &units) };
    if sts < 0 {
        return sts;
    }

    unsafe {
        match type_ {
            PM_TYPE_32 => *value = atom.l as f64,
            PM_TYPE_U32 => *value = atom.ul as f64,
            PM_TYPE_64 => *value = atom.ll as f64,
            PM_TYPE_U64 => *value = atom.ull as f64,
            PM_TYPE_FLOAT => *value = atom.f as f64,
            PM_TYPE_DOUBLE => *value = atom.d,
            _ => return PM_ERR_CONV,
        }
    }
    0
}

/// Port of `void PCPDynamicColumn_writeAtomValue(PCPDynamicColumn* column,
/// RichString* str, const struct Settings_* settings, int metric, int instance,
/// const pmDesc* desc, const void* atom)` (`PCPDynamicColumn.c:356`). Formats
/// one metric value into `str`: `N/A` for a null atom, instance-name / string
/// values (with `command`/`process`/`device`/`cgroup` formats), or a normalized
/// numeric value (`percent`/`process` format, explicit-width float/int, or a
/// dimension-driven `Row_print*` renderer).
///
/// Uses the libpcp `pmsprintf` variadic extern for the `%*.*s`/`%*d`/`%*.2f`/
/// `%*llu` renders (option (a); exact C fidelity — see the module note).
pub fn PCPDynamicColumn_writeAtomValue(
    column: &PCPDynamicColumn,
    str: &mut RichString,
    settings: &Settings,
    metric: c_int,
    instance: c_int,
    desc: *const pmDesc,
    atom: *const pmAtomValue,
) {
    // const pmAtomValue* atomvalue = (const pmAtomValue*) atom;
    let atomvalue = atom;
    let scheme = ColorScheme::active();
    let mut buffer = [0 as c_char; BUFSIZE];

    // RichString_appendnAscii(str, attr, buffer, n): reinterpret the `pmsprintf`
    // c_char output as ASCII bytes, `n` clamped to the buffer for memory safety.
    let append_buf = |str: &mut RichString, attr: i32, buffer: &[c_char], n: c_int| {
        let n = if n < 0 {
            0
        } else {
            (n as usize).min(buffer.len())
        };
        let bytes = unsafe { core::slice::from_raw_parts(buffer.as_ptr() as *const u8, n) };
        RichString_appendnAscii(str, attr, bytes, n);
    };

    // int attr = CRT_colors[DEFAULT_COLOR];
    let mut attr = CE::DEFAULT_COLOR.packed(scheme);
    // int width = column->super.width;
    let mut width = column.super_.width;

    // if (!width || abs(width) > DYNAMIC_MAX_COLUMN_WIDTH) width = DYNAMIC_DEFAULT_COLUMN_WIDTH;
    if width == 0 || width.abs() > DYNAMIC_MAX_COLUMN_WIDTH {
        width = DYNAMIC_DEFAULT_COLUMN_WIDTH;
    }
    let mut abswidth = width.abs();
    if abswidth > DYNAMIC_MAX_COLUMN_WIDTH {
        abswidth = DYNAMIC_MAX_COLUMN_WIDTH;
        width = -abswidth;
    }

    // if (atomvalue == NULL) { "N/A" }
    if atomvalue.is_null() {
        let n = unsafe {
            pmsprintf(
                buffer.as_mut_ptr(),
                buffer.len(),
                c"%*.*s ".as_ptr(),
                width,
                abswidth,
                c"N/A".as_ptr(),
            )
        };
        append_buf(str, CE::PROCESS_SHADOW.packed(scheme), &buffer, n);
        return;
    }

    let dtype = unsafe { (*desc).type_ };

    // instance names and metrics with string values first.
    if column.instances || dtype == PM_TYPE_STRING {
        // char* value = NULL; — assigned in both branches below (C `value = ...`).
        let mut value: *const c_char;
        let mut dupd1: *mut c_char = ptr::null_mut();
        if column.instances {
            // attr = CRT_colors[DYNAMIC_GRAY]; Metric_externalName(metric, instance, &dupd1);
            attr = CE::DYNAMIC_GRAY.packed(scheme);
            Metric_externalName(Metric_fromId(metric as usize), instance, &mut dupd1);
            value = dupd1;
        } else {
            // attr = CRT_colors[DYNAMIC_GREEN]; value = atomvalue->cp;
            attr = CE::DYNAMIC_GREEN.packed(scheme);
            value = unsafe { (*atomvalue).cp };
        }

        // Holds a CGroup_filterName result alive while `value` points into it.
        let mut _cgroup_hold: Option<CString> = None;
        let n;

        if column.format.is_some() && !value.is_null() {
            let fmt = column.format.as_deref().unwrap();
            if fmt == "command" {
                attr = CE::PROCESS_COMM.packed(scheme);
            } else if fmt == "process" {
                attr = CE::PROCESS_SHADOW.packed(scheme);
            } else if fmt == "device" && unsafe { libc::strncmp(value, c"/dev/".as_ptr(), 5) } == 0
            {
                // value += 5;
                value = unsafe { value.add(5) };
            } else if fmt == "cgroup" {
                // dupd2 = CGroup_filterName(value); if (dupd2) value = dupd2;
                let vs = unsafe { CStr::from_ptr(value) }.to_string_lossy();
                if let Some(filtered) = CGroup_filterName(&vs) {
                    let cs = CString::new(filtered)
                        .expect("writeAtomValue: filtered cgroup has interior NUL");
                    value = cs.as_ptr();
                    _cgroup_hold = Some(cs);
                }
            }
            n = unsafe {
                pmsprintf(
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    c"%*.*s ".as_ptr(),
                    width,
                    abswidth,
                    value,
                )
            };
        } else if !value.is_null() {
            n = unsafe {
                pmsprintf(
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    c"%*.*s ".as_ptr(),
                    width,
                    abswidth,
                    value,
                )
            };
        } else {
            n = unsafe {
                pmsprintf(
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    c"%*.*s ".as_ptr(),
                    width,
                    abswidth,
                    c"N/A".as_ptr(),
                )
            };
        }

        // if (dupd1) free(dupd1);
        if !dupd1.is_null() {
            unsafe { libc::free(dupd1 as *mut libc::c_void) };
        }
        append_buf(str, attr, &buffer, n);
        return;
    }

    // numeric value - normalize units to bytes/seconds first.
    let mut value: f64 = 0.0;
    if PCPDynamicColumn_normalize(desc, atomvalue, &mut value) < 0 {
        let n = unsafe {
            pmsprintf(
                buffer.as_mut_ptr(),
                buffer.len(),
                c"%*.*s ".as_ptr(),
                width,
                abswidth,
                c"no conv".as_ptr(),
            )
        };
        append_buf(str, CE::METER_VALUE_ERROR.packed(scheme), &buffer, n);
        return;
    }

    if let Some(fmt) = column.format.as_deref() {
        if fmt == "percent" {
            // n = Row_printPercentage(value, buffer, sizeof(buffer), (uint8_t)width, &attr);
            let mut pa = PercentageAttr::Unchanged;
            let buf = Row_printPercentage(value as f32, buffer.len(), width as u8, &mut pa);
            let a = match pa {
                PercentageAttr::Shadow => CE::PROCESS_SHADOW.packed(scheme),
                PercentageAttr::Megabytes => CE::PROCESS_MEGABYTES.packed(scheme),
                PercentageAttr::Unchanged => attr,
            };
            RichString_appendnAscii(str, a, buf.as_bytes(), buf.len());
            return;
        }
        if fmt == "process" {
            // n = pmsprintf(buffer, sizeof(buffer), "%*d ", Row_pidDigits, (int)value);
            let n = unsafe {
                pmsprintf(
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    c"%*d ".as_ptr(),
                    Row_pidDigits.load(Ordering::Relaxed),
                    value as c_int,
                )
            };
            append_buf(str, attr, &buffer, n);
            return;
        }
    }

    // width overrides unit suffix and coloring.
    if column.width != 0 {
        let truncated = value as u64;
        let n = if value - (truncated as f64) > 0.0 {
            // "%*.2f " with the double
            unsafe {
                pmsprintf(
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    c"%*.2f ".as_ptr(),
                    width,
                    value,
                )
            }
        } else {
            // "%*llu " with the unsigned truncation
            unsafe {
                pmsprintf(
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    c"%*llu ".as_ptr(),
                    width,
                    truncated,
                )
            }
        };
        append_buf(str, CE::PROCESS.packed(scheme), &buffer, n);
        return;
    }

    // bool coloring = settings->highlightMegabytes;
    let coloring = settings.highlightMegabytes;
    let units = unsafe { (*desc).units };
    let ds = (units.bits >> 28) & 0xF != 0; // dimSpace
    let dt = (units.bits >> 24) & 0xF != 0; // dimTime
    let dc = (units.bits >> 20) & 0xF != 0; // dimCount
    if ds && dt {
        Row_printRate(str, value, coloring);
    } else if ds {
        Row_printBytes(str, value as u64, coloring);
    } else if dc {
        Row_printCount(str, value as u64, coloring);
    } else if dt {
        Row_printTime(str, (value / 10.0) as u64, coloring); // hundredths of a second
    } else {
        Row_printCount(str, value as u64, false); // e.g. PID
    }
}

/// Port of `void PCPDynamicColumn_writeField(PCPDynamicColumn* this, const
/// Process* proc, RichString* str)` (`PCPDynamicColumn.c:458`). Fetches this
/// column's metric instance for `proc` and formats it via
/// [`PCPDynamicColumn_writeAtomValue`]. The C `const Process* proc` (cast to
/// `PCPProcess*`) is taken directly as `&PCPProcess` (the caller has the
/// concrete type); the base `Process` is `proc.super_`.
pub fn PCPDynamicColumn_writeField(
    this: &PCPDynamicColumn,
    proc: &PCPProcess,
    str: &mut RichString,
) {
    // const Settings* settings = proc->super.host->settings;
    let host = unsafe { &*(proc.super_.super_.host as *const Machine) };
    let settings = host
        .settings
        .as_ref()
        .expect("PCPDynamicColumn_writeField: host->settings is NULL");

    // Metric metric = Metric_fromId(this->id); const pmDesc* desc = Metric_desc(metric);
    let metric = Metric_fromId(this.id);
    let desc = Metric_desc(metric);
    // pid_t pid = Process_getPid(proc);
    let pid = Process_getPid(&proc.super_);

    // pmAtomValue atom; pmAtomValue* ap = &atom;
    // if (!Metric_instance(metric, pid, pp->offset, ap, desc->type)) ap = NULL;
    let mut atom: pmAtomValue = unsafe { core::mem::zeroed() };
    let dtype = unsafe { (*desc).type_ };
    let ap: *const pmAtomValue =
        if Metric_instance(metric, pid, proc.offset as c_int, &mut atom, dtype).is_null() {
            ptr::null()
        } else {
            &atom
        };

    // PCPDynamicColumn_writeAtomValue(this, str, settings, metric, pid, desc, ap);
    PCPDynamicColumn_writeAtomValue(this, str, settings, metric as c_int, pid, desc, ap);
}

/// Port of `int PCPDynamicColumn_compareByKey(const PCPProcess* p1, const
/// PCPProcess* p2, ProcessField key)` (`PCPDynamicColumn.c:474`). Looks up the
/// dynamic column for `key` in `settings->dynamicColumns`, fetches each process's
/// metric instance, and three-way-compares by the metric's type (reversed
/// operand order, matching the C `SPACESHIP_*(atom2, atom1)`). Returns `-1` when
/// the column or either instance is missing.
pub fn PCPDynamicColumn_compareByKey(p1: &PCPProcess, p2: &PCPProcess, key: RowField) -> c_int {
    // const Settings* settings = proc->super.host->settings;
    let host = unsafe { &*(p1.super_.super_.host as *const Machine) };
    let settings = host
        .settings
        .as_ref()
        .expect("PCPDynamicColumn_compareByKey: host->settings is NULL");

    // const PCPDynamicColumn* column = Hashtable_get(settings->dynamicColumns, key);
    let dyn_cols = match settings.dynamicColumns {
        Some(p) => unsafe { &*p },
        None => return -1,
    };
    let column = match Hashtable_get(dyn_cols, key as u32) {
        Some(o) => match (o as &dyn Any).downcast_ref::<PCPDynamicColumn>() {
            Some(c) => c,
            None => return -1,
        },
        None => return -1, // if (!column) return -1;
    };

    // Metric metric = Metric_fromId(column->id); unsigned int type = Metric_type(metric);
    let metric = Metric_fromId(column.id);
    let type_ = Metric_type(metric);

    // pmAtomValue atom1 = {0}, atom2 = {0};
    let mut atom1: pmAtomValue = unsafe { core::mem::zeroed() };
    let mut atom2: pmAtomValue = unsafe { core::mem::zeroed() };
    let pid1 = Process_getPid(&p1.super_);
    let pid2 = Process_getPid(&p2.super_);
    if Metric_instance(metric, pid1, p1.offset as c_int, &mut atom1, type_).is_null()
        || Metric_instance(metric, pid2, p2.offset as c_int, &mut atom2, type_).is_null()
    {
        if type_ == PM_TYPE_STRING {
            unsafe {
                libc::free(atom1.cp as *mut libc::c_void);
                libc::free(atom2.cp as *mut libc::c_void);
            }
        }
        return -1;
    }

    unsafe {
        match type_ {
            PM_TYPE_STRING => {
                // int cmp = SPACESHIP_NULLSTR(atom2.cp, atom1.cp); free(atom2.cp); free(atom1.cp);
                let s2 = if atom2.cp.is_null() {
                    None
                } else {
                    Some(CStr::from_ptr(atom2.cp).to_bytes())
                };
                let s1 = if atom1.cp.is_null() {
                    None
                } else {
                    Some(CStr::from_ptr(atom1.cp).to_bytes())
                };
                let cmp = spaceship_nullstr!(s2, s1);
                libc::free(atom2.cp as *mut libc::c_void);
                libc::free(atom1.cp as *mut libc::c_void);
                cmp
            }
            PM_TYPE_32 => spaceship_number!(atom2.l, atom1.l),
            PM_TYPE_U32 => spaceship_number!(atom2.ul, atom1.ul),
            PM_TYPE_64 => spaceship_number!(atom2.ll, atom1.ll),
            PM_TYPE_U64 => spaceship_number!(atom2.ull, atom1.ull),
            PM_TYPE_FLOAT => compareRealNumbers(atom2.f as f64, atom1.f as f64),
            PM_TYPE_DOUBLE => compareRealNumbers(atom2.d, atom1.d),
            _ => -1,
        }
    }
}
