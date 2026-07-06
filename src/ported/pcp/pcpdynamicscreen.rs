//! Port of `pcp/PCPDynamicScreen.c` + `.h` — htop's Performance Co-Pilot
//! dynamic-screen subsystem: the `screens/` config-file readers that build a
//! per-screen [`InDomTable`] over a PCP instance domain and wire user-defined
//! `Dynamic(...)` columns into htop's [`Settings`]/[`ScreenSettings`]/[`Panel`].
//!
//! 1:1 faithful port; the C is the spec. `PCPDynamicScreen` "extends"
//! [`DynamicScreen`](crate::ported::dynamicscreen::DynamicScreen) via the
//! embedded `super_`, exactly as the C struct embeds `DynamicScreen super` as
//! its first member. This is the direct sibling of
//! [`crate::ported::pcp::pcpdynamiccolumn`] and
//! [`crate::ported::pcp::pcpdynamicmeter`] and mirrors their config-parse
//! structure, local `atoi`, `std::fs` config reading, and substrate-limitation
//! handling. The per-screen `Dynamic(...)` columns are the sibling
//! [`PCPDynamicColumn`]s (imported, not redeclared); the instance-domain table
//! is [`InDomTable`] (imported); `Platform_addMetric` is imported from
//! [`platform`](super::platform).
//!
//! # Config-file parsing
//!
//! [`PCPDynamicScreens_init`] reads the `$PCP_SHARE_DIR` / `$PCP_SYSCONF_DIR`
//! (via `pmGetConfig`), `$XDG_CONFIG_HOME` / `$HOME`, and `$PCP_HTOP_DIR`
//! `screens/` directories and parses each file's `key = value` lines. The C
//! `opendir`/`readdir` scan is [`std::fs::read_dir`]; the C `fopen` +
//! `String_readLine` loop is [`std::fs::read_to_string`] iterated by line.
//! Section headers `[name]`, the screen keys (`heading`/`caption`/`sortKey`/
//! `sortDirection`/`default`/`enabled`), and the per-column
//! `<name>.metric`/`<name>.caption`/… attribute keys are ported verbatim.
//! Unlike the column/meter `init`, the C screen `init` has **no** `getpwuid`
//! `$HOME` fallback (`PCPDynamicScreen.c:299`), so none is added here.
//!
//! `pmGetConfig` / `pmGetProgname` return libpcp-owned `char*` (static/cached
//! for `pmGetConfig`) — wrapped via `CStr::from_ptr(...).to_string_lossy()` and
//! never freed. `pmRegisterDerivedMetric`'s error `char**` is formatted to
//! stderr through `CRT_fatalError`, exactly as the C prints it unconditionally
//! with `pmGetProgname()`.
//!
//! # Substrate limitations (reported)
//!
//! - **Reduced base `DynamicScreen`.** The ported
//!   [`DynamicScreen`](crate::ported::dynamicscreen::DynamicScreen) carries only
//!   `name`/`heading`/`columnKeys`/`direction`; the C `super.caption` and
//!   `super.sortKey` fields the C `parseFile` writes were dropped from that
//!   reduced struct (no ported reader). Since `dynamicscreen.rs` is not editable
//!   here, they are carried locally as [`PCPDynamicScreen::caption`] /
//!   [`PCPDynamicScreen::sortKey`]; only `caption` is read within this module
//!   (the `instances` column attribute), so the relocation is behavior-faithful.
//! - **Owner-`Hashtable`, shared-only `get`.** The screens live in an owner
//!   [`Hashtable`] (`Hashtable_new(0, true)`) that exposes only
//!   [`Hashtable_get`] → `&dyn Object` and *drops* values on
//!   `Hashtable_remove`. So the C pattern of inserting a screen at `[name]`
//!   and then mutating it in place through the returned `void*` on subsequent
//!   lines is not expressible; parsing instead holds the in-progress screen as a
//!   local owned value and flushes it into the table at the next section / EOF
//!   (the sibling `parseFile` pattern). For the fields the C mutates *after* the
//!   screen is already in the table ([`PCPDynamicScreens_appendDynamicColumns`]
//!   sets `indom`/`key`; [`PCPDynamicScreen_appendTables`] sets `table`), the
//!   ported struct uses [`Cell`] interior mutability so those writes go through a
//!   shared `&PCPDynamicScreen` soundly — reproducing C's in-place mutation of a
//!   table-resident object without the `invalid_reference_casting`
//!   (`&T`→`&mut T`) that a raw const-cast would trip.
//!   The one field C also mutates post-insert that lives on the *reduced base*
//!   struct, `super.columnKeys = formatFields(screen)`, cannot be wrapped in a
//!   `Cell` (the base struct is in the uneditable `dynamicscreen.rs`), so that
//!   single write in `appendDynamicColumns` cannot persist through the owner
//!   table; the value is still computed ([`formatFields`]) but not stored, and
//!   this is the reported gap (the `PCPDynamicColumns_setupWidths` precedent).
//! - **`DynamicScreen_search` over `PCPDynamicScreen` values.** The screens
//!   table stores `PCPDynamicScreen`, and
//!   [`DynamicScreen_search`](crate::ported::dynamicscreen::DynamicScreen_search)
//!   (used by [`PCPDynamicScreen_uniqueName`] and
//!   [`PCPDynamicScreens_addAvailableColumns`], as the C does) reads each
//!   value's `DynamicScreen` base through [`Object::as_dynamic_screen`] — the
//!   safe analog of C's `(DynamicScreen*)value` struct-prefix cast, which
//!   [`PCPDynamicScreen`] overrides to return its `super_`.
//! - **Immutable-foreach free subsumed by `Drop`.** [`PCPDynamicScreens_free`]
//!   receives a shared `&dyn Object` and cannot drive the owning teardown; the
//!   owner table's `Box<dyn Object>` `Drop` frees each screen's owned fields, and
//!   [`PCPDynamicScreen`]'s [`Drop`] reclaims the leaked [`InDomTable`] box
//!   (`Object_delete(ds->table)`). The `columns` array holds `PCPDynamicColumn**`
//!   aliases whose owning `Box`es are handed to the [`PCPDynamicColumns`] table
//!   in `appendDynamicColumns`, so the screen frees only the pointer array
//!   (C's `free(ds->columns)`), never the columns themselves.
//! - **`Platform_addMetric`** (the PCP platform metric-array registrar) is a
//!   `pcp/Platform.c` function (not yet ported), scaffolded as a `todo!()` in
//!   [`platform`](super::platform) and imported here so `appendDynamicColumns`'s
//!   call site stays 1:1 until `Platform.c` lands.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::any::Any;
use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_uint};
use std::ptr;

use crate::ported::crt::CRT_fatalError;
use crate::ported::dynamicscreen::{DynamicScreen, DynamicScreen_search};
use crate::ported::hashtable::{
    Hashtable, Hashtable_foreach, Hashtable_get, Hashtable_new, Hashtable_put,
};
use crate::ported::listitem::ListItem_new;
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass, Object_class};
use crate::ported::panel::{Panel, Panel_add};
use crate::ported::pcp::indomtable::{InDomTable, InDomTable_new};
use crate::ported::pcp::metric::{Metric_desc, Metric_fromId, Metric_lookupText};
use crate::ported::pcp::pcpdynamiccolumn::{PCPDynamicColumn, PCPDynamicColumns};
use crate::ported::pcp::pcpprocess::LAST_PROCESSFIELD;
use crate::ported::pcp::platform::Platform_addMetric;
use crate::ported::pcp::pmapi::{pmGetProgname, pmInDom, pmRegisterDerivedMetric, PM_INDOM_NULL};
use crate::ported::settings::{ScreenSettings, Settings, Settings_newDynamicScreen};
use crate::ported::table::Table;
use crate::ported::xutils::{String_eq, String_trim};

use crate::ported::dynamiccolumn::DynamicColumn;

/// Port of the autoconf `CONFIGDIR` macro — `"/.config"`, the per-`$HOME`
/// config subdir (matches the sibling `pcpdynamiccolumn.rs` / `pcpdynamicmeter.rs`).
const CONFIGDIR: &str = "/.config";

/// `sizeof(column->super.name)` — the C `DynamicColumn.name[32]` buffer
/// (`DynamicColumn.h:22`), the length budget checked in
/// [`PCPDynamicScreen_lookupMetric`].
const COLUMN_NAME_SIZE: usize = 32;

/// Port of `typedef struct PCPDynamicScreen_` (`PCPDynamicScreen.h:25`).
/// "Extends" [`DynamicScreen`] via the embedded `super_` (C's first member).
///
/// The C `super.caption` / `super.sortKey` fields the parser writes are absent
/// from the reduced ported base struct, so they are carried locally as
/// [`caption`](Self::caption) / [`sortKey`](Self::sortKey) (see the module
/// note). `table`, `indom`, and `key` are wrapped in [`Cell`] because the C
/// mutates them *after* the screen is stored in the owner `Hashtable`
/// (`appendTables` / `appendDynamicColumns`); the `Cell` reproduces that
/// in-place mutation soundly through a shared reference. `columns` is the C
/// `PCPDynamicColumn** columns` array — raw-pointer aliases whose owning `Box`es
/// are handed to the [`PCPDynamicColumns`] table in `appendDynamicColumns`.
pub struct PCPDynamicScreen {
    /// C `DynamicScreen super`.
    pub super_: DynamicScreen,
    /// C `super.caption` — carried locally (reduced base struct omits it).
    pub caption: Option<String>,
    /// C `super.sortKey` — carried locally (reduced base struct omits it).
    pub sortKey: Option<String>,
    /// C `struct InDomTable_* table` — the per-screen instance-domain table,
    /// set by [`PCPDynamicScreen_appendTables`]. A leaked [`Box`] pointer (null
    /// until built); reclaimed by [`Drop`] (`Object_delete(ds->table)`).
    pub table: Cell<*mut InDomTable>,
    /// C `struct PCPDynamicColumn_** columns` — the screen's dynamic columns
    /// (raw-pointer aliases; owned by the [`PCPDynamicColumns`] table after
    /// `appendDynamicColumns`).
    pub columns: Vec<*mut PCPDynamicColumn>,
    /// C `size_t totalColumns`.
    pub totalColumns: usize,
    /// C `unsigned int indom` — instance-domain number (set post-insert).
    pub indom: Cell<pmInDom>,
    /// C `unsigned int key` — representative `PCPMetric` identifier (set
    /// post-insert).
    pub key: Cell<c_uint>,
    /// C `bool defaultEnabled` — enabled setting from the configuration file.
    pub defaultEnabled: bool,
}

impl Drop for PCPDynamicScreen {
    /// `Object_delete(ds->table)` (`PCPDynamicScreen.c:344`) — reclaim and free
    /// the leaked [`InDomTable`] box. The `columns` `Vec` drops the pointer
    /// array only (C's `free(ds->columns)`); the pointed-to columns are owned by
    /// the [`PCPDynamicColumns`] table.
    fn drop(&mut self) {
        let t = self.table.get();
        if !t.is_null() {
            // SAFETY: a non-null `table` is a `Box::into_raw` leak from
            // `appendTables`, reclaimed exactly once (here or in `_done`).
            drop(unsafe { Box::from_raw(t) });
        }
    }
}

/// Class descriptor for [`PCPDynamicScreen`], present solely so a value can be
/// stored as a `Box<dyn Object>` in the ported [`Hashtable`] (the same adapter
/// role the sibling column/meter classes serve). htop stores raw `void*`, so
/// this is not a real C class; rooted at [`Object_class`], it sets no dispatch
/// slots (the table never dispatches through them).
static PCPDynamicScreen_class: ObjectClass = ObjectClass {
    extends: Some(&Object_class),
};

impl Object for PCPDynamicScreen {
    fn klass(&self) -> &'static ObjectClass {
        &PCPDynamicScreen_class
    }

    /// The `DynamicScreen` base is the `super_` prefix (C's
    /// `(DynamicScreen*)pcpScreen` cast) — lets `DynamicScreen_search` read a
    /// `PCPDynamicScreen` stored in the shared table.
    fn as_dynamic_screen(&self) -> Option<&DynamicScreen> {
        Some(&self.super_)
    }
}

/// Port of `typedef struct PCPDynamicScreens_` (`PCPDynamicScreen.h:39`). Owns
/// the discovery [`Hashtable`] (`None` before [`PCPDynamicScreens_init`], the C
/// uninitialized/`NULL` state) plus the discovery counter.
#[derive(Default)]
pub struct PCPDynamicScreens {
    /// C `Hashtable* table` — discovered screens keyed by discovery index.
    pub table: Option<Hashtable>,
    /// C `size_t count` — count of dynamic screens discovered by the scan.
    pub count: usize,
}

/// C `atoi` as used for the `sortDirection` / `width` keys
/// (`PCPDynamicScreen.c:266`): optional sign then base-10 digits, `0` when none.
/// Mirrors the sibling `pcpdynamiccolumn.rs` local `atoi`.
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

/// Port of `static char* formatFields(PCPDynamicScreen* screen)`
/// (`PCPDynamicScreen.c:28`). Builds the space-separated `Dynamic(<name>)` field
/// list from the screen's enabled columns (C `strdup("")` seed + `xAsprintf`
/// append per enabled column). The columns are read through their
/// `PCPDynamicColumn*` aliases.
pub fn formatFields(screen: &PCPDynamicScreen) -> String {
    // char* columns = strdup("");
    let mut columns = String::new();
    for j in 0..screen.totalColumns {
        let column = screen.columns[j];
        // if (column->super.enabled == false) continue;
        // SAFETY: `column` is a live alias into a `PCPDynamicColumn` box.
        if !unsafe { (*column).super_.enabled } {
            continue;
        }
        let name = unsafe { (*column).super_.name.clone() };
        // xAsprintf(&columns, "%s Dynamic(%s)", prefix, column->super.name);
        columns = format!("{columns} Dynamic({name})");
    }
    columns
}

/// Port of `static void PCPDynamicScreens_appendDynamicColumns(
/// PCPDynamicScreens* screens, PCPDynamicColumns* columns)`
/// (`PCPDynamicScreen.c:43`). For every screen, assigns each column a metric id
/// (`offset + cursor`), registers it with the platform, and inserts it into the
/// shared `columns` table keyed by `count + LAST_PROCESSFIELD`; the first column
/// seeds the screen's `indom`/`key` from the metric descriptor.
///
/// The `column->id` writes go through the `PCPDynamicColumn*` aliases (heap
/// boxes, not the screen), and the box ownership is handed to the `columns`
/// table via [`Box::from_raw`] (C's aliasing insert). The `indom`/`key` writes
/// go through the screen's [`Cell`]s. The final `screen->super.columnKeys =
/// formatFields(screen)` write cannot persist through the owner table (the
/// reduced base `columnKeys` is not `Cell`-wrapped) — see the module note; the
/// value is computed but discarded.
pub fn PCPDynamicScreens_appendDynamicColumns(
    screens: &PCPDynamicScreens,
    columns: &mut PCPDynamicColumns,
) {
    let table = match screens.table.as_ref() {
        Some(t) => t,
        None => return,
    };
    for i in 0..screens.count as u32 {
        // PCPDynamicScreen* screen = Hashtable_get(screens->table, i);
        // if (!screen) return;   /* C returns, not continue */
        let screen = match Hashtable_get(table, i) {
            Some(o) => o,
            None => return,
        };
        let screen = match (screen as &dyn Any).downcast_ref::<PCPDynamicScreen>() {
            Some(s) => s,
            None => return,
        };

        for j in 0..screen.totalColumns {
            let column = screen.columns[j];

            // column->id = columns->offset + columns->cursor;
            // SAFETY: `column` aliases a live `PCPDynamicColumn` box.
            unsafe { (*column).id = columns.offset + columns.cursor };
            // Metric metric = Metric_fromId(column->id);
            let metric = Metric_fromId(unsafe { (*column).id });
            // columns->cursor++;
            columns.cursor += 1;
            // Platform_addMetric(metric, column->metricName);
            let metric_name = unsafe { (*column).metricName.clone() }.unwrap_or_default();
            Platform_addMetric(metric, &metric_name);

            // ht_key_t id = (ht_key_t) columns->count + LAST_PROCESSFIELD;
            let id = columns.count as u32 + LAST_PROCESSFIELD as u32;
            // Hashtable_put(columns->table, id, column);  — hand box ownership over.
            if let Some(ctable) = columns.table.as_mut() {
                // SAFETY: `column` is the sole owning pointer for this box until
                // now (created by lookupMetric); ownership transfers to the
                // columns table. The screen keeps the raw alias (C's aliasing).
                Hashtable_put(ctable, id, unsafe { Box::from_raw(column) });
            }
            // columns->count++;
            columns.count += 1;

            if j == 0 {
                // const pmDesc* desc = Metric_desc(metric);
                let desc = Metric_desc(metric);
                // assert(desc->indom != PM_INDOM_NULL);
                debug_assert!(unsafe { (*desc).indom } != PM_INDOM_NULL);
                // screen->indom = desc->indom;  screen->key = metric;
                screen.indom.set(unsafe { (*desc).indom });
                screen.key.set(metric as usize as c_uint);
            }
        }

        // screen->super.columnKeys = formatFields(screen);
        // The reduced base `columnKeys` is not Cell-wrapped, so this write cannot
        // persist through the owner table (see the module note). Computed for
        // fidelity, discarded.
        let _column_keys = formatFields(screen);
    }
}

/// Port of `static PCPDynamicColumn* PCPDynamicScreen_lookupMetric(
/// PCPDynamicScreen* screen, const char* name)` (`PCPDynamicScreen.c:73`).
/// Returns the existing column whose derived `metricName` matches
/// `htop.screen.<screen>.<name>`, else allocates a fresh [`PCPDynamicColumn`]
/// (named `<screen>:<name>`, enabled, back-pointer to the screen's table),
/// appends its pointer to `screen->columns`, and returns it. `None` (C `NULL`)
/// when the combined name would overflow the `char[32]` buffer.
///
/// The new column is a `Box::into_raw` leak owned by the screen's `columns`
/// array until `appendDynamicColumns` hands it to the `PCPDynamicColumns` table.
pub fn PCPDynamicScreen_lookupMetric(
    screen: &mut PCPDynamicScreen,
    name: &str,
) -> Option<*mut PCPDynamicColumn> {
    // if ((strlen(name) + strlen(screen->super.name) + 1) >= sizeof(column->super.name))
    //    return NULL;  /* colon */
    if name.len() + screen.super_.name.len() + 1 >= COLUMN_NAME_SIZE {
        return None;
    }

    // xAsprintf(&metricName, "htop.screen.%s.%s", screen->super.name, name);
    let metric_name = format!("htop.screen.{}.{}", screen.super_.name, name);

    // for (i..totalColumns) if (String_eq(column->metricName, metricName)) return column;
    for j in 0..screen.totalColumns {
        let column = screen.columns[j];
        if String_eq(
            unsafe { (*column).metricName.as_deref() }.unwrap_or(""),
            &metric_name,
        ) {
            return Some(column);
        }
    }

    // not an existing column in this screen - create it and add to the list
    let tp = screen.table.get();
    // column->super.table = &screen->table->super;  (NULL screen->table ⇒ NULL Table*)
    let super_table: *const Table = if tp.is_null() {
        ptr::null()
    } else {
        // SAFETY: `tp` is a live InDomTable box; `super_` is its base Table.
        unsafe { &(*tp).super_ as *const Table }
    };

    let column = PCPDynamicColumn {
        super_: DynamicColumn {
            // pmsprintf(column->super.name, sizeof, "%s:%s", screen->super.name, name);
            // — fits (checked above), so no truncation.
            name: format!("{}:{}", screen.super_.name, name),
            heading: None,
            caption: None,
            description: None,
            width: 0,
            enabled: true, // column->super.enabled = true;
            table: super_table,
        },
        metricName: Some(metric_name),
        format: None,
        id: 0,
        width: 0,
        defaultEnabled: false, // xCalloc zero-init (only the `default` key sets it)
        percent: false,
        instances: false,
    };

    let ptr = Box::into_raw(Box::new(column));
    // screen->columns = xReallocArray(...); screen->columns[n-1] = column; totalColumns = n;
    screen.columns.push(ptr);
    screen.totalColumns += 1;
    Some(ptr)
}

/// Port of `static void PCPDynamicScreen_parseColumn(PCPDynamicScreen* screen,
/// const char* path, unsigned int line, char* key, char* value)`
/// (`PCPDynamicScreen.c:104`). Splits `key` at its first `.` into a column name
/// and an attribute; `metric` registers a libpcp derived metric for `value` (a
/// parse failure is a `CRT_fatalError` with the libpcp error text, printed
/// unconditionally with `pmGetProgname()`), the others set the column's
/// caption/heading/description/width/format/instances/default.
pub fn PCPDynamicScreen_parseColumn(
    screen: &mut PCPDynamicScreen,
    path: &str,
    line: u32,
    key: &str,
    value: &str,
) {
    // char* p = strchr(key, '.'); if (!p) return; *p++ = '\0';
    let dot = match key.find('.') {
        Some(i) => i,
        None => return,
    };
    let name = &key[..dot];
    let p = &key[dot + 1..];

    // PCPDynamicColumn* column = PCPDynamicScreen_lookupMetric(screen, key);
    // if (!column) return;
    let col_ptr = match PCPDynamicScreen_lookupMetric(screen, name) {
        Some(c) => c,
        None => return,
    };
    // SAFETY: a freshly-created or existing alias into a live PCPDynamicColumn
    // box; no tracked borrow of `screen` is held across this raw deref.
    let col = unsafe { &mut *col_ptr };

    if String_eq(p, "metric") {
        // if (pmRegisterDerivedMetric(column->metricName, value, &error) < 0) { ... CRT_fatalError }
        let name_c = CString::new(col.metricName.as_deref().unwrap_or(""))
            .expect("parseColumn: metricName has interior NUL");
        let expr_c = CString::new(value).expect("parseColumn: metric value has interior NUL");
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
            let note = format!(
                "{progname}: failed to parse expression in {path} at line {line}\n{errstr}\n"
            );
            // errno = EINVAL; — the ported CRT_fatalError takes the message directly.
            CRT_fatalError(&note);
        }

        // pmLookupText - add optional metric help text
        // if (!column->super.description && !column->instances) Metric_lookupText(value, &column->super.description);
        if col.super_.description.is_none() && !col.instances {
            let value_c = CString::new(value).expect("parseColumn: metric value has interior NUL");
            let mut descp: *mut c_char = ptr::null_mut();
            Metric_lookupText(value_c.as_c_str(), &mut descp);
            if !descp.is_null() {
                let s = unsafe { CStr::from_ptr(descp) }
                    .to_string_lossy()
                    .into_owned();
                col.super_.description = Some(s);
                unsafe { libc::free(descp as *mut libc::c_void) };
            }
        }
    } else {
        // property of a dynamic column - the column expression may not have been
        // observed yet; i.e. we allow for any ordering.
        if String_eq(p, "caption") {
            col.super_.caption = Some(value.to_string());
        } else if String_eq(p, "heading") {
            col.super_.heading = Some(value.to_string());
        } else if String_eq(p, "description") {
            col.super_.description = Some(value.to_string());
        } else if String_eq(p, "width") {
            col.width = atoi(value);
        } else if String_eq(p, "format") {
            col.format = Some(value.to_string());
        } else if String_eq(p, "instances") {
            col.instances = false;
            if String_eq(value, "True") || String_eq(value, "true") {
                col.instances = true;
            }
            // free_and_xStrdup(&column->super.description, screen->super.caption);
            col.super_.description = screen.caption.clone();
        } else if String_eq(p, "default") {
            // displayed by default
            col.defaultEnabled = true;
            col.super_.enabled = true;
            if String_eq(value, "False") || String_eq(value, "false") {
                col.defaultEnabled = false;
                col.super_.enabled = false;
            }
        }
    }
}

/// Port of `static bool PCPDynamicScreen_validateScreenName(char* key, const
/// char* path, unsigned int line)` (`PCPDynamicScreen.c:168`). Truncates `key`
/// at the last `']'`, then validates it (first byte alpha-or-`_`, the rest
/// alnum-or-`_`); a missing brace or an invalid character prints a parse error
/// to stderr and returns `false`. `key` is mutated in place (C's `char*`).
pub fn PCPDynamicScreen_validateScreenName(key: &mut String, path: &str, line: u32) -> bool {
    // pmGetProgname(): libpcp-owned static string, never freed.
    let progname = unsafe { CStr::from_ptr(pmGetProgname()) }.to_string_lossy();
    // char* end = strrchr(key, ']'); if (end) *end = '\0'; else { fprintf; return false; }
    match key.rfind(']') {
        Some(pos) => key.truncate(pos),
        None => {
            eprint!("{progname}: no closing brace on screen name at {path} line {line}\n\"{key}\"");
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
    // if (*p != '\0') { fprintf; return false; }
    if p != bytes.len() {
        eprint!("{progname}: invalid screen name at {path} line {line}\n\"{key}\"");
        return false;
    }
    true
}

/// Port of `static bool PCPDynamicScreen_uniqueName(char* key,
/// PCPDynamicScreens* screens)` (`PCPDynamicScreen.c:201`): the name has not been
/// defined previously iff [`DynamicScreen_search`] finds no screen with it.
///
/// `DynamicScreen_search` reads each stored `PCPDynamicScreen`'s `DynamicScreen`
/// base through `Object::as_dynamic_screen` (the safe analog of C's `void*`
/// prefix cast), so it matches a name defined via this table.
pub fn PCPDynamicScreen_uniqueName(key: &str, screens: &PCPDynamicScreens) -> bool {
    // return !DynamicScreen_search(screens->table, key, NULL);
    !DynamicScreen_search(screens.table.as_ref(), key, None)
}

/// Port of `static PCPDynamicScreen* PCPDynamicScreen_new(PCPDynamicScreens*
/// screens, const char* name)` (`PCPDynamicScreen.c:205`). Builds a zeroed screen
/// (C `xCalloc`) with `super.name` set (truncated to the C `char[32]` buffer)
/// and `defaultEnabled = true`, returning it with its hashtable key
/// `screens->count` (the C `Hashtable_put` is deferred to the caller — see
/// [`PCPDynamicScreen_parseFile`] — because the ported owner table hands back no
/// mutable alias for subsequent key parsing), then advances `count`.
pub fn PCPDynamicScreen_new(
    screens: &mut PCPDynamicScreens,
    name: &str,
) -> (PCPDynamicScreen, u32) {
    // String_safeStrncpy(screen->super.name, name, sizeof(screen->super.name));
    // — copies up to 31 bytes (the char[32] buffer). Screen names are validated
    // ASCII (alnum/underscore), so a byte-boundary truncation is a char boundary.
    let name: String = name[..name.len().min(31)].to_string();

    let screen = PCPDynamicScreen {
        super_: DynamicScreen {
            name,
            heading: None,
            columnKeys: None,
            direction: 0,
        },
        caption: None,
        sortKey: None,
        table: Cell::new(ptr::null_mut()),
        columns: Vec::new(),
        totalColumns: 0,
        indom: Cell::new(0),
        key: Cell::new(0),
        defaultEnabled: true, // screen->defaultEnabled = true;
    };

    // ht_key_t id = (ht_key_t) screens->count; ... screens->count++;
    let key = screens.count as u32;
    screens.count += 1;
    (screen, key)
}

/// Port of `static void PCPDynamicScreen_parseFile(PCPDynamicScreens* screens,
/// const char* path)` (`PCPDynamicScreen.c:217`). Reads `path` and parses each
/// `key = value` line: a `[name]` section header validates/uniquifies the name
/// and starts a new screen; the `heading`/`caption`/`sortKey`/`sortDirection`/
/// `default`/`enabled` keys populate the current screen, and any other key is a
/// per-column attribute forwarded to [`PCPDynamicScreen_parseColumn`]. Comment
/// (`#`) and blank lines are skipped.
///
/// The in-progress screen is held as a local owned value and inserted into the
/// table at the next section / EOF (instead of C's insert-then-mutate-through-
/// `void*`); the previous screen is flushed before the next header's uniqueness
/// check, preserving same-file duplicate detection.
pub fn PCPDynamicScreen_parseFile(screens: &mut PCPDynamicScreens, path: &str) {
    // FILE* file = fopen(path, "r"); if (!file) return;
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    // PCPDynamicScreen* screen = NULL; unsigned int lineno = 0;
    let mut screen: Option<PCPDynamicScreen> = None;
    let mut pending_key: u32 = 0;
    let mut lineno: u32 = 0;

    // Insert the current in-progress screen into the table (owner frees on drop).
    let flush =
        |screens: &mut PCPDynamicScreens, screen: &mut Option<PCPDynamicScreen>, key: u32| {
            if let Some(s) = screen.take() {
                if let Some(table) = screens.table.as_mut() {
                    Hashtable_put(table, key, Box::new(s));
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
        let (key_raw, value_raw) = match trimmed.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (trimmed.as_str(), None),
        };
        let key = String_trim(key_raw);
        let value = value_raw.map(String_trim);

        if key.as_bytes().first() == Some(&b'[') {
            // new section name - i.e. new screen; flush the previous so uniqueName sees it.
            flush(screens, &mut screen, pending_key);
            screen = None;

            // bool ok = validateScreenName(key + 1, path, lineno);
            let mut sname = key[1..].to_string();
            let mut ok = PCPDynamicScreen_validateScreenName(&mut sname, path, lineno);
            // if (ok) ok = uniqueName(key + 1, screens);
            if ok {
                ok = PCPDynamicScreen_uniqueName(&sname, screens);
            }
            // if (ok) screen = PCPDynamicScreen_new(screens, key + 1);
            if ok {
                let (s, k) = PCPDynamicScreen_new(screens, &sname);
                screen = Some(s);
                pending_key = k;
            }
        } else if let Some(s) = screen.as_mut() {
            // else if (!screen) skip; else if (!value) skip; else dispatch.
            if let Some(value) = value.as_deref() {
                if String_eq(&key, "heading") {
                    s.super_.heading = Some(value.to_string());
                } else if String_eq(&key, "caption") {
                    s.caption = Some(value.to_string());
                } else if String_eq(&key, "sortKey") {
                    s.sortKey = Some(value.to_string());
                } else if String_eq(&key, "sortDirection") {
                    s.super_.direction = atoi(value);
                } else if String_eq(&key, "default") || String_eq(&key, "enabled") {
                    if String_eq(value, "False") || String_eq(value, "false") {
                        s.defaultEnabled = false;
                    } else if String_eq(value, "True") || String_eq(value, "true") {
                        s.defaultEnabled = true; // also default
                    }
                } else {
                    PCPDynamicScreen_parseColumn(s, path, lineno, &key, value);
                }
            }
        }
    }

    // Insert the final in-progress screen. (fclose(file): `contents` is owned.)
    flush(screens, &mut screen, pending_key);
}

/// Port of `static void PCPDynamicScreen_scanDir(PCPDynamicScreens* screens,
/// char* path)` (`PCPDynamicScreen.c:282`). Opens `path` and parses every entry
/// whose name does not begin with `.` (skipping `.`/`..`/hidden), via
/// [`PCPDynamicScreen_parseFile`]. The C `String_cat(path, d_name)` is a plain
/// concatenation (`path` already ends with `/`).
pub fn PCPDynamicScreen_scanDir(screens: &mut PCPDynamicScreens, path: &str) {
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
        PCPDynamicScreen_parseFile(screens, &file);
    }
}

/// Port of `void PCPDynamicScreens_init(PCPDynamicScreens* screens,
/// PCPDynamicColumns* columns)` (`PCPDynamicScreen.c:299`). Creates the owning
/// discovery table and scans, in order, the `$PCP_HTOP_DIR/screens/` developer
/// path, the `$XDG_CONFIG_HOME` (else `$HOME/.config`) `htop/screens/`, the
/// system `$PCP_SYSCONF_DIR` `htop/screens/`, and the read-only `$PCP_SHARE_DIR`
/// `htop/screens/`, then wires the discovered columns' metric ids via
/// [`PCPDynamicScreens_appendDynamicColumns`]. Unlike the column/meter `init`,
/// there is no `getpwuid` `$HOME` fallback (faithful to the C).
pub fn PCPDynamicScreens_init(screens: &mut PCPDynamicScreens, columns: &mut PCPDynamicColumns) {
    // pmGetConfig(name): libpcp returns a static/cached string (never freed); a
    // NULL return (C assumes non-NULL for these keys) yields the empty string.
    let pm_get_config = |name: &str| -> String {
        let c = CString::new(name).expect("pmGetConfig: name has interior NUL");
        let p = unsafe { crate::ported::pcp::pmapi::pmGetConfig(c.as_ptr()) };
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
    let home = std::env::var("HOME").ok();

    // screens->table = Hashtable_new(0, true);
    screens.table = Some(Hashtable_new(0, true));

    // developer paths - PCP_HTOP_DIR=./pcp ./pcp-htop
    if let Some(ov) = &override_ {
        let path = format!("{ov}/screens/");
        PCPDynamicScreen_scanDir(screens, &path);
    }

    // next, search in home directory alongside htoprc
    let path = if let Some(x) = &xdg_config_home {
        Some(format!("{x}/htop/screens/"))
    } else {
        home.as_ref()
            .map(|h| format!("{h}{CONFIGDIR}/htop/screens/"))
    };
    if let Some(p) = path {
        PCPDynamicScreen_scanDir(screens, &p);
    }

    // next, search in the system screens directory
    let path = format!("{sysconf}/htop/screens/");
    PCPDynamicScreen_scanDir(screens, &path);

    // next, try the readonly system screens directory
    let path = format!("{share}/htop/screens/");
    PCPDynamicScreen_scanDir(screens, &path);

    // establish internal metric identifier mappings
    PCPDynamicScreens_appendDynamicColumns(screens, columns);
}

/// Port of `static void PCPDynamicScreen_done(PCPDynamicScreen* ds)`
/// (`PCPDynamicScreen.c:342`). C body: `DynamicScreen_done(&ds->super);
/// Object_delete(ds->table); free(ds->columns);`.
///
/// The ported base [`DynamicScreen_done`](crate::ported::dynamicscreen::DynamicScreen_done)
/// consumes by value and cannot be driven from a `&mut super_` field; the base
/// string frees are subsumed by the screen `Box`'s [`Drop`]. `Object_delete`
/// reclaims the leaked [`InDomTable`] box (nulling the [`Cell`] so [`Drop`] does
/// not double-free); `free(ds->columns)` clears the pointer array (the columns
/// themselves are owned by the [`PCPDynamicColumns`] table).
pub fn PCPDynamicScreen_done(ds: &mut PCPDynamicScreen) {
    // Object_delete(ds->table);
    let t = ds.table.get();
    if !t.is_null() {
        // SAFETY: reclaim the leaked InDomTable box exactly once; null the Cell
        // so Drop does not reclaim it again.
        drop(unsafe { Box::from_raw(t) });
        ds.table.set(ptr::null_mut());
    }
    // free(ds->columns);
    ds.columns.clear();
    ds.totalColumns = 0;
}

/// Port of `static void PCPDynamicScreens_free(ht_key_t key, void* value, void*
/// data)` (`PCPDynamicScreen.c:348`): the [`Hashtable_foreach`] callback that
/// runs [`PCPDynamicScreen_done`] on each screen. The callback matches the ported
/// foreach signature (`key: u32, value: &dyn Object`).
///
/// The shared `&dyn Object` cannot drive the `&mut` `PCPDynamicScreen_done`, but
/// the owner table's `Box<dyn Object>` `Drop` (plus [`PCPDynamicScreen`]'s
/// [`Drop`]) already frees each screen's owned fields and table box, so this
/// per-value free is subsumed by `Drop` (see the module note).
pub fn PCPDynamicScreens_free(_key: u32, value: &dyn Object) {
    let _screen = (value as &dyn Any)
        .downcast_ref::<PCPDynamicScreen>()
        .expect("PCPDynamicScreens_free: hashtable value is not a PCPDynamicScreen");
    // C: PCPDynamicScreen_done(ds); — subsumed by the owner Box's Drop.
}

/// Port of `void PCPDynamicScreens_done(Hashtable* table)`
/// (`PCPDynamicScreen.c:353`). Runs [`PCPDynamicScreens_free`] over every entry
/// via [`Hashtable_foreach`] (freeing each screen's owned fields, subsumed by
/// `Drop` here). Does not delete the table itself (the table is deleted
/// separately by its owner).
pub fn PCPDynamicScreens_done(table: &Hashtable) {
    // Hashtable_foreach(table, PCPDynamicScreens_free, NULL);
    Hashtable_foreach(table, &mut PCPDynamicScreens_free);
}

/// Port of `void PCPDynamicScreen_appendTables(PCPDynamicScreens* screens,
/// Machine* host)` (`PCPDynamicScreen.c:357`). Builds each screen's per-screen
/// [`InDomTable`] from its `indom`/`key` and stores it (through the screen's
/// `table` [`Cell`]).
pub fn PCPDynamicScreen_appendTables(screens: &PCPDynamicScreens, host: *const Machine) {
    let table = match screens.table.as_ref() {
        Some(t) => t,
        None => return,
    };
    for i in 0..screens.count as u32 {
        // if ((ds = Hashtable_get(screens->table, i)) == NULL) continue;
        let ds = match Hashtable_get(table, i) {
            Some(o) => o,
            None => continue,
        };
        let ds = match (ds as &dyn Any).downcast_ref::<PCPDynamicScreen>() {
            Some(s) => s,
            None => continue,
        };
        // ds->table = InDomTable_new(host, ds->indom, ds->key);
        // Reclaim any prior table (C overwrites a NULL first-time pointer).
        let old = ds.table.get();
        if !old.is_null() {
            drop(unsafe { Box::from_raw(old) });
        }
        let boxed = InDomTable_new(host, ds.indom.get(), ds.key.get() as c_int);
        ds.table.set(Box::into_raw(boxed));
    }
}

/// Port of `void PCPDynamicScreen_appendScreens(PCPDynamicScreens* screens,
/// Settings* settings)` (`PCPDynamicScreen.c:367`). Registers a runtime
/// [`ScreenSettings`] for every default-enabled screen via
/// [`Settings_newDynamicScreen`], passing the screen's heading tab, its base
/// [`DynamicScreen`], and its [`InDomTable`]'s base [`Table`] handle.
pub fn PCPDynamicScreen_appendScreens(screens: &PCPDynamicScreens, settings: &mut Settings) {
    let table = match screens.table.as_ref() {
        Some(t) => t,
        None => return,
    };
    for i in 0..screens.count as u32 {
        // if ((ds = Hashtable_get(screens->table, i)) == NULL) continue;
        let ds = match Hashtable_get(table, i) {
            Some(o) => o,
            None => continue,
        };
        let ds = match (ds as &dyn Any).downcast_ref::<PCPDynamicScreen>() {
            Some(s) => s,
            None => continue,
        };
        // if (ds->defaultEnabled == false) continue;
        if !ds.defaultEnabled {
            continue;
        }
        // const char* tab = ds->super.heading;
        let tab = ds.super_.heading.as_deref().unwrap_or("");
        // Settings_newDynamicScreen(settings, tab, &ds->super, &ds->table->super);
        let tp = ds.table.get();
        let table_handle: Option<*mut Table> = if tp.is_null() {
            None
        } else {
            // SAFETY: `tp` is a live InDomTable box; its base Table is the handle.
            Some(unsafe { &mut (*tp).super_ as *mut Table })
        };
        Settings_newDynamicScreen(settings, tab, &ds.super_, table_handle);
    }
}

/// Port of `void PCPDynamicScreen_addDynamicScreen(PCPDynamicScreens* screens,
/// ScreenSettings* ss)` (`PCPDynamicScreen.c:381`) — called when an htoprc
/// `.dynamic` line names a dynamic screen. Finds the screen whose name matches
/// `ss->dynamic` and points `ss->table` at that screen's [`InDomTable`] base
/// [`Table`].
pub fn PCPDynamicScreen_addDynamicScreen(screens: &PCPDynamicScreens, ss: &mut ScreenSettings) {
    let table = match screens.table.as_ref() {
        Some(t) => t,
        None => return,
    };
    for i in 0..screens.count as u32 {
        // if ((ds = Hashtable_get(screens->table, i)) == NULL) continue;
        let ds = match Hashtable_get(table, i) {
            Some(o) => o,
            None => continue,
        };
        let ds = match (ds as &dyn Any).downcast_ref::<PCPDynamicScreen>() {
            Some(s) => s,
            None => continue,
        };
        // if (String_eq(ss->dynamic, ds->super.name) == false) continue;
        if !String_eq(ss.dynamic.as_deref().unwrap_or(""), &ds.super_.name) {
            continue;
        }
        // ss->table = &ds->table->super;
        let tp = ds.table.get();
        ss.table = if tp.is_null() {
            None
        } else {
            // SAFETY: `tp` is a live InDomTable box; its base Table is the handle.
            Some(unsafe { &mut (*tp).super_ as *mut Table })
        };
    }
}

/// Port of `void PCPDynamicScreens_addAvailableColumns(Panel* availableColumns,
/// Hashtable* screens, const char* screen)` (`PCPDynamicScreen.c:393`). Clears
/// the panel and repopulates it with one [`ListItem`](crate::ported::listitem)
/// per column of the named screen (`"<heading> - <description>"`, or just the
/// title when no description/caption exists).
///
/// The C `Vector_prune(availableColumns->items)` maps to clearing the panel's
/// owned item `Vec`. The [`DynamicScreen_search`] over the screens table reads
/// each stored `PCPDynamicScreen`'s base through `Object::as_dynamic_screen`
/// (the safe analog of C's `void*` prefix cast) — see the module note.
pub fn PCPDynamicScreens_addAvailableColumns(
    availableColumns: &mut Panel,
    screens: &Hashtable,
    screen: &str,
) {
    // Vector_prune(availableColumns->items); — owner Vec, dropping frees items.
    availableColumns.items.clear();

    // bool success = DynamicScreen_search(screens, screen, &key); if (!success) return;
    let mut key: u32 = 0;
    if !DynamicScreen_search(Some(screens), screen, Some(&mut key)) {
        return;
    }

    // PCPDynamicScreen* dynamicScreen = Hashtable_get(screens, key); if (!dynamicScreen) return;
    let dynamic_screen = match Hashtable_get(screens, key) {
        Some(o) => o,
        None => return,
    };
    let dynamic_screen = match (dynamic_screen as &dyn Any).downcast_ref::<PCPDynamicScreen>() {
        Some(s) => s,
        None => return,
    };

    for j in 0..dynamic_screen.totalColumns {
        let column = dynamic_screen.columns[j];
        // SAFETY: `column` aliases a live PCPDynamicColumn box.
        let col: &DynamicColumn = unsafe { &(*column).super_ };
        // const char* title = column->super.heading ? column->super.heading : column->super.name;
        let title = col.heading.as_deref().unwrap_or(&col.name);
        // const char* text = column->super.description ? column->super.description : column->super.caption;
        let text = col.description.as_deref().or(col.caption.as_deref());
        // if (text) "%s - %s" else "%s"
        let description = match text {
            Some(t) => format!("{title} - {t}"),
            None => title.to_string(),
        };
        // Panel_add(availableColumns, (Object*) ListItem_new(description, j));
        Panel_add(
            availableColumns,
            Box::new(ListItem_new(&description, j as i32)),
        );
    }
}
