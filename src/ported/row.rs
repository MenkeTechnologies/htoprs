//! Partial port of `Row.c` + `Row.h`.
//!
//! Ported data model: the [`Row`] struct (every `Row.h` field), its
//! [`Object`] trait impl (the C `Row_class` vtable, whose only non-NULL
//! slot is `.compare = Row_compare`), and the pure lifecycle /
//! comparison / predicate logic — [`Row_init`], [`Row_done`],
//! [`Row_isTomb`], [`Row_toggleTag`], [`Row_compare`],
//! [`Row_compareByParent_Base`], [`Row_getGroupOrParent`],
//! [`Row_isChildOf`].
//!
//! Ported formatters: the `RichString` number formatters
//! `Row_print{KBytes,Bytes,Count,Time,Nanoseconds,Rate,LeftAlignedField}`
//! (they write styled digits into a [`RichString`], choosing a
//! `CRT_colors[...]` attribute per magnitude band), plus the pure
//! `Row_printPercentage` (writes into a `char* buffer`). These sit on
//! the merged `richstring` + `crt` substrate.
//!
//! Ported column-width setters: [`Row_setPidColumnWidth`] /
//! [`Row_setUidColumnWidth`] and their `Row_pidDigits` / `Row_uidDigits`
//! globals (modeled as `AtomicI32`, matching `crt.rs`'s `CRT_colorScheme`
//! pattern). They depend only on the ported `countDigits` (`xutils.rs`).
//!
//! Still stubbed (`todo!()`, named after their real htop C function so
//! the port-purity gate accepts the module); each names its blocker in
//! its own doc-comment:
//! - [`Row_display`] — dereferences `host` into `Settings`, walks
//!   `settings->ss->fields` and dispatches through the `RowClass.writeField`
//!   / `Row_isHighlighted` vtable slots, none of which are modeled (only
//!   `ObjectClass` is realized, not `RowClass`).
//! - [`Row_resetFieldWidths`], [`alignedTitleProcessField`] — need the
//!   unported platform `Process_fields[]` (`ProcessFieldData`) table.
//! - [`Row_updateFieldWidth`], [`Row_resetFieldWidths`] — need the
//!   `Row_fieldWidths[LAST_RESERVED_FIELD]` global, sized by a platform
//!   constant not modeled in the port.
//! - [`alignedTitleDynamicColumn`] — needs `Settings.dynamicColumns`
//!   (`Hashtable_get`, `DynamicColumn.{width,heading}` and
//!   `DYNAMIC_*_COLUMN_WIDTH` have since landed, but the `Settings` struct
//!   in `settings.rs` still carries no `dynamicColumns` `Hashtable` field
//!   to look the column up in).
//! - [`RowField_alignedTitle`], [`RowField_keyAt`] — blocked transitively
//!   on the two `alignedTitle*` helpers (and `Settings.ss`).
//!
//! `gen_port_report.py` counts `todo!()` bodies as *stubbed*, not
//! *ported*, so scaffolding does not inflate coverage.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::ported::crt::ColorElements::*;
use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::linux::linuxprocess::{Process_fields, LAST_PROCESSFIELD};
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass, Object_class};
use crate::ported::process::ProcessField;
use crate::ported::settings::{RowField, Settings};
use crate::ported::table::Table;
use core::ops::Deref;
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendChr, RichString_appendnAscii,
    RichString_appendnWideColumns, RichString_setAttr, RichString_size,
};
use crate::ported::xutils::countDigits;
use core::any::Any;
use core::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};

/// Port of `#define SPACESHIP_NUMBER(a, b) (((a) > (b)) - ((a) < (b)))`
/// from `Macros.h:33`. A three-way comparison collapsing to `-1`/`0`/`1`
/// (kept as a macro, matching the C text-substitution macro; it is not a
/// function in C either, so no port-gate fn name is introduced).
macro_rules! spaceship_number {
    ($a:expr, $b:expr) => {
        (($a > $b) as i32 - ($a < $b) as i32)
    };
}
pub(crate) use spaceship_number;

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

// PID/UID column-width bounds from `Row.h:22-25`.
/// `#define ROW_MIN_PID_DIGITS 5` (`Row.h:22`).
const ROW_MIN_PID_DIGITS: i32 = 5;
/// `#define ROW_MAX_PID_DIGITS 19` (`Row.h:23`).
const ROW_MAX_PID_DIGITS: i32 = 19;
/// `#define ROW_MIN_UID_DIGITS 5` (`Row.h:24`).
const ROW_MIN_UID_DIGITS: i32 = 5;
/// `#define ROW_MAX_UID_DIGITS 20` (`Row.h:25`).
const ROW_MAX_UID_DIGITS: i32 = 20;

/// Port of the global `int Row_pidDigits = ROW_MIN_PID_DIGITS;`
/// (`Row.c:32`). Mutable process-wide state, so it is modeled as an
/// `AtomicI32` — the same pattern `crt.rs` uses for the `CRT_colorScheme`
/// global. Written by [`Row_setPidColumnWidth`].
pub static Row_pidDigits: AtomicI32 = AtomicI32::new(ROW_MIN_PID_DIGITS);

/// Port of the global `int Row_uidDigits = ROW_MIN_UID_DIGITS;`
/// (`Row.c:33`). Mutable process-wide state; see [`Row_pidDigits`].
/// Written by [`Row_setUidColumnWidth`].
pub static Row_uidDigits: AtomicI32 = AtomicI32::new(ROW_MIN_UID_DIGITS);

/// Port of the global `uint8_t Row_fieldWidths[LAST_PROCESSFIELD]`
/// (`Row.c:106`) — the process-wide auto-adjusted column widths, indexed by
/// [`RowField`]. Modeled as an array of `AtomicU8` (the interior-mutability
/// pattern [`Row_pidDigits`] uses), sized by the Linux `LAST_PROCESSFIELD`.
/// Reset by [`Row_resetFieldWidths`] and grown by [`Row_updateFieldWidth`].
pub static Row_fieldWidths: [AtomicU8; LAST_PROCESSFIELD] =
    [const { AtomicU8::new(0) }; LAST_PROCESSFIELD];

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

/// Port of `struct Row_` from `Row.h:41`. htop's base class for every
/// entity displayable in the process-table half of the screen (Process,
/// and platform rows). Field names, order, and widths mirror the C
/// struct exactly.
///
/// Field-type mapping:
/// - C `Object super;` — modeled by the [`Object`] trait impl below
///   (`Row_class`), not a struct field; htop's emulated-OOP `super` is a
///   vtable pointer, which in Rust is the trait + [`klass`](Object::klass).
/// - C `const struct Machine_* host;` — [`host`](Row::host), a raw
///   `*const c_void`. `Machine` is not a ported module; the field is
///   kept as an opaque pointer (the faithful representation of a
///   `const Machine*` back-reference) so lifecycle code can store it
///   without the unported `Machine` struct. [`Row_isNew`] dereferences it
///   via the crate's `host as *const Machine` cast; [`Row_display`], which
///   also needs the `RowClass` vtable, stays stubbed.
/// - `pid_t`/`uid_t` widths are handled by the callers; the struct's
///   `id`/`group`/`parent` are `int` as in C.
#[derive(Debug, Clone)]
pub struct Row {
    /// C `const struct Machine_* host` — opaque back-reference to the
    /// owning `Machine` (unported; see struct docs).
    pub host: *const c_void,
    /// C `int id`.
    pub id: i32,
    /// C `int group`.
    pub group: i32,
    /// C `int parent`.
    pub parent: i32,
    /// C `bool isRoot` — has no known parent.
    pub isRoot: bool,
    /// C `bool tag` — tagged by the user.
    pub tag: bool,
    /// C `bool show` — whether to display this row.
    pub show: bool,
    /// C `bool wasShown` — shown last cycle.
    pub wasShown: bool,
    /// C `bool showChildren` — show children in tree-mode.
    pub showChildren: bool,
    /// C `bool updated` — updated during the last scan.
    pub updated: bool,
    /// C `int32_t indent` — tree-mode internal state.
    pub indent: i32,
    /// C `unsigned int tree_depth`.
    pub tree_depth: u32,
    /// C `uint64_t seenStampMs` — for showing new processes.
    pub seenStampMs: u64,
    /// C `uint64_t tombStampMs` — for showing exited processes.
    pub tombStampMs: u64,
}

impl Default for Row {
    /// Models htop's `calloc`-zeroed `Row` allocation: all fields zero /
    /// `false`, `host` a null pointer. [`Row_init`] then overwrites the
    /// subset the C initializer sets.
    fn default() -> Self {
        Row {
            host: core::ptr::null(),
            id: 0,
            group: 0,
            parent: 0,
            isRoot: false,
            tag: false,
            show: false,
            wasShown: false,
            showChildren: false,
            updated: false,
            indent: 0,
            tree_depth: 0,
            seenStampMs: 0,
            tombStampMs: 0,
        }
    }
}

/// Port of `typedef void (*Row_WriteField)(const Row*, RichString*,
/// RowField)` (`Row.h:80`). The C `const Row*` receiver is a `&dyn Object`
/// here; the slot downcasts to its concrete type via `Any`.
pub type Row_WriteField = fn(&dyn Object, &mut RichString, RowField);
/// Port of `typedef bool (*Row_IsHighlighted)(const Row*)` (`Row.h:81`).
pub type Row_IsHighlighted = fn(&dyn Object) -> bool;
/// Port of `typedef bool (*Row_IsVisible)(const Row*, const Table*)`
/// (`Row.h:82`).
pub type Row_IsVisible = fn(&dyn Object, &Table) -> bool;
/// Port of `typedef bool (*Row_MatchesFilter)(const Row*, const Table*)`
/// (`Row.h:83`).
pub type Row_MatchesFilter = fn(&dyn Object, &Table) -> bool;
/// Port of `typedef const char* (*Row_SortKeyString)(Row*)` (`Row.h:84`);
/// the C `const char*` becomes an owned `String`.
pub type Row_SortKeyString = fn(&dyn Object) -> String;
/// Port of `typedef int (*Row_CompareByParent)(const Row*, const Row*)`
/// (`Row.h:85`).
pub type Row_CompareByParent = fn(&dyn Object, &dyn Object) -> i32;

/// Port of `typedef struct RowClass_` (`Row.h:88`) — the `Row` vtable. It
/// embeds [`ObjectClass`] (`super_`, the first field, so [`Deref`] and the
/// class-identity pointers coincide) and adds the Row-level virtual slots.
/// `Deref<Target = ObjectClass>` lets a `&RowClass` coerce to `&ObjectClass`
/// wherever the class-identity API ([`Object_isA`]) expects one, so the C
/// `As_Row`/`(ObjectClass*)` casts need no call-site changes.
pub struct RowClass {
    pub super_: ObjectClass,
    pub isHighlighted: Option<Row_IsHighlighted>,
    pub isVisible: Option<Row_IsVisible>,
    pub writeField: Option<Row_WriteField>,
    pub matchesFilter: Option<Row_MatchesFilter>,
    pub sortKeyString: Option<Row_SortKeyString>,
    pub compareByParent: Option<Row_CompareByParent>,
}

impl Deref for RowClass {
    type Target = ObjectClass;
    fn deref(&self) -> &ObjectClass {
        &self.super_
    }
}

/// Port of `const RowClass Row_class` from `Row.c:560`:
/// `{ .super = { .extends = Class(Object), .compare = Row_compare } }`. The
/// `.compare = Row_compare` slot is realized by the [`Object::compare`] impl
/// below; every Row-level virtual slot is `NULL` in the C initializer
/// (`None` here). Declared `static` for stable-address class identity.
pub static Row_class: RowClass = RowClass {
    super_: ObjectClass {
        extends: Some(&Object_class),
    },
    isHighlighted: None,
    isVisible: None,
    writeField: None,
    matchesFilter: None,
    sortKeyString: None,
    compareByParent: None,
};

impl Object for Row {
    /// C `this->super.klass` set to `&Row_class` by `Object_setClass`; here
    /// the embedded [`ObjectClass`] of the [`RowClass`] vtable.
    fn klass(&self) -> &'static ObjectClass {
        &Row_class.super_
    }

    /// C `As_Row(this)` — the concrete [`RowClass`] vtable for a base `Row`.
    fn row_class(&self) -> Option<&'static RowClass> {
        Some(&Row_class)
    }

    /// C `(const Row*)this` — a base `Row` is its own embedded `Row`.
    fn as_row(&self) -> Option<&Row> {
        Some(self)
    }

    /// C `Row_class.super.compare = Row_compare`. Dispatches to
    /// [`Row_compare`]; the `const void*` args in C become a downcast of
    /// the trait object back to `Row` (via `Any`), the safe-Rust analog
    /// of the C cast.
    fn compare(&self, other: &dyn Object) -> i32 {
        let any: &dyn Any = other;
        let o = any
            .downcast_ref::<Row>()
            .expect("Row_compare called across incompatible classes");
        Row_compare(self, o)
    }
}

/// Port of `void Row_init(Row* this, const Machine* host)` from
/// `Row.c:35`. Stores the `host` back-reference and sets the display
/// defaults. Only these fields are assigned (matching the C body); the
/// rest keep their zero-init values (see [`Row::default`]).
pub fn Row_init(this: &mut Row, host: *const c_void) {
    this.host = host;
    this.tag = false;
    this.showChildren = true;
    this.show = true;
    this.wasShown = false;
    this.updated = false;
}

/// Port of `int Row_getGroupOrParent(const Row* this)` from `Row.h:172`.
/// Returns `parent` when the row is its own group leader
/// (`group == id`), otherwise `group`. Used by the tree view and by
/// [`Row_compareByParent_Base`].
pub fn Row_getGroupOrParent(this: &Row) -> i32 {
    if this.group == this.id {
        this.parent
    } else {
        this.group
    }
}

/// Port of `bool Row_isChildOf(const Row* this, int id)` from
/// `Row.h:176`. True when `id` is this row's group-or-parent.
pub fn Row_isChildOf(this: &Row, id: i32) -> bool {
    id == Row_getGroupOrParent(this)
}

/// Port of `void Row_done(Row* this)` from `Row.c:44`. The C body is
/// `assert(this != NULL); (void) this;` — a no-op teardown; the
/// non-null precondition is guaranteed by the `&Row` reference.
pub fn Row_done(this: &Row) {
    let _ = this;
}

/// Port of `static inline bool Row_isNew(const Row* this)` from
/// `Row.c:49`. True when the row was first seen within the last
/// `highlightDelaySecs` seconds (used to flash newly-appeared rows).
///
/// Signature mapping: the opaque `host` (`*const c_void`) is cast back to
/// `*const Machine` — the same cast the crate uses elsewhere on a `Row`'s
/// `host` (`linuxprocesstable.rs:1465`). The C reads `host->monotonicMs`
/// and `host->settings` (a `const Settings*`, unconditionally
/// dereferenced); `Machine.settings` is an `Option`, so the non-null
/// precondition is realized with `.expect`, matching `Table_cleanupRow`'s
/// pattern (`table.rs`). The C `1000 * (uint64_t)settings->highlightDelaySecs`
/// keeps the `int → uint64_t` cast semantics as `i32 as u64` (identical
/// wrapping on a negative value, though the flag is nonnegative in
/// practice).
pub fn Row_isNew(this: &Row) -> bool {
    let host = unsafe { &*(this.host as *const Machine) };
    if host.monotonicMs < this.seenStampMs {
        return false;
    }

    let settings = host
        .settings
        .as_ref()
        .expect("Row_isNew: host->settings is NULL");
    host.monotonicMs - this.seenStampMs <= 1000 * settings.highlightDelaySecs as u64
}

/// Port of `static inline bool Row_isTomb(const Row* this)` from
/// `Row.c:58`. True once the row has an exit timestamp
/// (`tombStampMs > 0`). Pure — no substrate dereference.
pub fn Row_isTomb(this: &Row) -> bool {
    this.tombStampMs > 0
}

/// Port of `void Row_display(const Object* cast, RichString* out)` from
/// `Row.c:62` — the `Object_display` slot for every `Row`-derived type.
/// Renders each active-screen field through the concrete row's `writeField`
/// [`RowClass`] slot, then applies the highlight/tag/tomb/new attribute
/// overlays. The C `(const Row*)cast` becomes [`Object::as_row`]; the
/// `As_Row(this)->writeField`/`isHighlighted` vtable dispatch goes through
/// [`Object::row_class`].
pub fn Row_display(cast: &dyn Object, out: &mut RichString) {
    let this = cast.as_row().expect("Row_display: object is not a Row");
    // C `this->host->settings`.
    let host = unsafe { &*(this.host as *const Machine) };
    let settings = host
        .settings
        .as_ref()
        .expect("Row_display: host->settings is NULL");
    let scheme = ColorScheme::active();

    let rc = cast
        .row_class()
        .expect("Row_display: object has no RowClass vtable");
    let write_field = rc
        .writeField
        .expect("Row_display: RowClass has no writeField slot");

    // C `for (int i = 0; fields[i]; i++) As_Row(this)->writeField(...)`.
    let fields = &settings.screens[settings.ssIndex as usize].fields;
    for &field in fields {
        if field == 0 {
            break; // NULL_FIELD terminator
        }
        write_field(cast, out, field);
    }

    // C `Row_isHighlighted(this)` — the `isHighlighted` slot, or false.
    if rc.isHighlighted.map_or(false, |f| f(cast)) {
        RichString_setAttr(out, PROCESS_SHADOW.packed(scheme));
    }

    if this.tag {
        RichString_setAttr(out, PROCESS_TAG.packed(scheme));
    }

    if settings.highlightChanges {
        if Row_isTomb(this) {
            out.highlightAttr = PROCESS_TOMB.packed(scheme);
        } else if Row_isNew(this) {
            out.highlightAttr = PROCESS_NEW.packed(scheme);
        }
    }

    debug_assert!(RichString_size(out) > 0);
}

/// Port of `void Row_setPidColumnWidth(pid_t maxPid)` from `Row.c:86`.
/// Sets [`Row_pidDigits`] from the largest PID seen: the minimum when
/// `maxPid` still fits in `ROW_MIN_PID_DIGITS` digits, otherwise the exact
/// digit count.
///
/// Signature mapping: `pid_t` → `i32` (the port's PID type, per
/// `machine.rs`'s `maxProcessId: i32`). The C `(int)pow(10,
/// ROW_MIN_PID_DIGITS)` is the compile-time constant `10^5 == 100000`,
/// computed with integer `pow` here (no float round-trip needed).
/// `countDigits((size_t)maxPid, 10)` reuses the ported [`countDigits`];
/// the cast is safe because this branch is only reached when
/// `maxPid >= 100000 > 0`. The C `assert` becomes a `debug_assert!`.
pub fn Row_setPidColumnWidth(maxPid: i32) {
    if maxPid < 10_i32.pow(ROW_MIN_PID_DIGITS as u32) {
        Row_pidDigits.store(ROW_MIN_PID_DIGITS, Ordering::Relaxed);
        return;
    }

    let digits = countDigits(maxPid as usize, 10) as i32;
    Row_pidDigits.store(digits, Ordering::Relaxed);
    debug_assert!(digits <= ROW_MAX_PID_DIGITS);
}

/// Port of `void Row_setUidColumnWidth(uid_t maxUid)` from `Row.c:96`.
/// Sets [`Row_uidDigits`] from the largest UID seen; mirrors
/// [`Row_setPidColumnWidth`].
///
/// Signature mapping: `uid_t` → `u32` (the port's UID type, per
/// `process.rs`'s `st_uid: u32`). The C `(uid_t)pow(10,
/// ROW_MIN_UID_DIGITS)` is the constant `10^5 == 100000`. `maxUid` is
/// unsigned, so no sign concern on the `usize` cast.
pub fn Row_setUidColumnWidth(maxUid: u32) {
    if maxUid < 10_u32.pow(ROW_MIN_UID_DIGITS as u32) {
        Row_uidDigits.store(ROW_MIN_UID_DIGITS, Ordering::Relaxed);
        return;
    }

    let digits = countDigits(maxUid as usize, 10) as i32;
    Row_uidDigits.store(digits, Ordering::Relaxed);
    debug_assert!(digits <= ROW_MAX_UID_DIGITS);
}

/// Port of `void Row_resetFieldWidths(void)` from `Row.c:107`. Reseeds each
/// auto-width field's tracked width in [`Row_fieldWidths`] from its title
/// length.
pub fn Row_resetFieldWidths() {
    for i in 0..LAST_PROCESSFIELD {
        if !Process_fields[i].autoWidth {
            continue;
        }
        // C `strlen(Process_fields[i].title)`; autoWidth fields always have a
        // title, so `None` (defensively → 0) never occurs in practice.
        let len = Process_fields[i].title.map_or(0, |t| t.len());
        debug_assert!(len <= u8::MAX as usize);
        Row_fieldWidths[i].store(len as u8, Ordering::Relaxed);
    }
}

/// Port of `void Row_updateFieldWidth(RowField key, size_t width)` from
/// `Row.c:119`. Grows the tracked width for `key` toward `width`, capped at
/// `u8::MAX`.
pub fn Row_updateFieldWidth(key: RowField, width: usize) {
    if width > u8::MAX as usize {
        Row_fieldWidths[key as usize].store(u8::MAX, Ordering::Relaxed);
    } else if width > Row_fieldWidths[key as usize].load(Ordering::Relaxed) as usize {
        Row_fieldWidths[key as usize].store(width as u8, Ordering::Relaxed);
    }
}

/// TODO: port of `static const char* alignedTitleDynamicColumn(const
/// Settings* settings, int key, char* titleBuffer, size_t
/// titleBufferSize)` from `Row.c:127`. Looks up `settings->dynamicColumns`
/// (a `Hashtable`) via `Hashtable_get` and reads `column->width` /
/// `column->heading`. `Hashtable_get`, `DynamicColumn.{width,heading}` and
/// `DYNAMIC_{MAX,DEFAULT}_COLUMN_WIDTH` all exist, but the `Settings` struct
/// still carries no `dynamicColumns` field (the `Hashtable` to look the
/// column up in). Stays a stub until that field is modeled.
pub fn alignedTitleDynamicColumn(_settings: &Settings, _field: RowField) -> String {
    todo!("port of Row.c:127 — needs Settings.dynamicColumns + Hashtable_get + DynamicColumn")
}

/// Port of `static const char* alignedTitleProcessField(ProcessField field,
/// char* titleBuffer, size_t titleBufferSize)` from `Row.c:141`. Formats a
/// reserved process field's column title: right-aligned to the pid/uid digit
/// width for pid/`ST_UID` columns, auto-width-aligned (right or left+truncate)
/// for auto-width columns, else the raw title. Returns an owned `String`
/// (C fills a caller buffer or returns a static string).
pub fn alignedTitleProcessField(field: RowField) -> String {
    let fd = &Process_fields[field as usize];
    let title = match fd.title {
        Some(t) => t,
        None => return "- ".to_string(),
    };

    if fd.pidColumn {
        let w = Row_pidDigits.load(Ordering::Relaxed) as usize;
        return format!("{title:>w$} ");
    }

    if field == ProcessField::ST_UID as RowField {
        let w = Row_uidDigits.load(Ordering::Relaxed) as usize;
        return format!("{title:>w$} ");
    }

    if fd.autoWidth {
        let w = Row_fieldWidths[field as usize].load(Ordering::Relaxed) as usize;
        if fd.autoTitleRightAlign {
            return format!("{title:>w$} ");
        }
        return format!("{title:<w$.w$} ");
    }

    title.to_string()
}

/// Port of `const char* RowField_alignedTitle(const Settings* settings,
/// RowField field)` from `Row.c:168`. Reserved fields (`< LAST_PROCESSFIELD`)
/// go through [`alignedTitleProcessField`]; dynamic fields through
/// [`alignedTitleDynamicColumn`].
pub fn RowField_alignedTitle(settings: &Settings, field: RowField) -> String {
    if (field as usize) < LAST_PROCESSFIELD {
        alignedTitleProcessField(field)
    } else {
        alignedTitleDynamicColumn(settings, field)
    }
}

/// Port of `RowField RowField_keyAt(const Settings* settings, int at)` from
/// `Row.c:179`. Walks the active screen's field list
/// (`settings->ss->fields`, i.e. `screens[ssIndex].fields`) measuring each
/// column title with [`RowField_alignedTitle`] to find which field the
/// horizontal offset `at` lands on; defaults to `COMM`.
pub fn RowField_keyAt(settings: &Settings, at: i32) -> RowField {
    let fields = &settings.screens[settings.ssIndex as usize].fields;
    let mut rem = at;
    for &field in fields {
        // C loop terminates at the `NULL_FIELD` (0) sentinel.
        if field == 0 {
            break;
        }
        let len = if rem > 0 {
            RowField_alignedTitle(settings, field).len().min(rem as usize) as i32
        } else {
            0
        };
        if rem <= len {
            return field;
        }
        rem -= len;
    }
    ProcessField::COMM as RowField
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

    let large_number_color = if coloring {
        color_of(LARGE_NUMBER)
    } else {
        color_of(PROCESS)
    };
    let megabytes_color = if coloring {
        color_of(PROCESS_MEGABYTES)
    } else {
        color_of(PROCESS)
    };
    let shadow_color = if coloring {
        color_of(PROCESS_SHADOW)
    } else {
        color_of(PROCESS)
    };
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
        let shadow_color = if coloring {
            color_of(PROCESS_SHADOW)
        } else {
            color_of(PROCESS)
        };
        RichString_appendAscii(str, shadow_color, b" 0:00.00 ");
        return;
    }

    let year_color = if coloring {
        color_of(LARGE_NUMBER)
    } else {
        color_of(PROCESS)
    };
    let day_color = if coloring {
        color_of(PROCESS_GIGABYTES)
    } else {
        color_of(PROCESS)
    };
    let hour_color = if coloring {
        color_of(PROCESS_MEGABYTES)
    } else {
        color_of(PROCESS)
    };
    let base_color = color_of(PROCESS);

    let total_seconds = total_hundredths / 100;
    let total_minutes = total_seconds / 60;
    let total_hours = total_minutes / 60;
    let seconds = (total_seconds % 60) as u32;
    let minutes = (total_minutes % 60) as u32;

    if total_minutes < 60 {
        let hundredths = (total_hundredths % 100) as u32;
        let buf = format!(
            "{:2}:{:02}.{:02} ",
            total_minutes as u32, seconds, hundredths
        );
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
        let shadow_color = if coloring {
            color_of(PROCESS_SHADOW)
        } else {
            color_of(PROCESS)
        };
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
/// magnitude-scaled unit. The scale index `i` is found by a general loop
/// dividing by `ONE_K` until below `ONE_K` (or `i` reaches the end of
/// `unitPrefixes`), and the prefix is `'B'` for `i == 0` else
/// `unitPrefixes[i - 1]` (`K`, `M`, `G`, `T`, `P`, `E`, `Z`, `Y`, `R`,
/// `Q`). Coloring by band: `PROCESS_SHADOW` for sub-0.005 and invalid,
/// `PROCESS` for `i` of 0/1 (B/K), `PROCESS_MEGABYTES` for `i == 2` (M),
/// `LARGE_NUMBER` for `i >= 3` (G and above). A negative or NaN rate
/// renders `"        N/A "`.
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
        return;
    }

    let mut i: usize = 0;
    let mut scaled = rate;
    while scaled >= ONE_K as f64 && i < unitPrefixes.len() {
        scaled /= ONE_K as f64;
        i += 1;
    }

    let mut color = base_color;
    if rate < 0.005 {
        color = shadow_color;
    } else if i == 2 {
        color = megabytes_color;
    } else if i >= 3 {
        color = large_number_color;
    }

    let prefix = if i == 0 { b'B' } else { unitPrefixes[i - 1] };
    let buf = format!("{:7.2} {}/s ", scaled, prefix as char);
    RichString_appendnAscii(str, color, buf.as_bytes(), buf.len());
}

/// Port of `Row.c:500`.
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

/// Port of `Row.c:506`.
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
    debug_assert!(
        n >= 6 && width >= 4,
        "Invalid width in Row_printPercentage()"
    );
    // truncate in favour of abort in xSnprintf()
    // CLAMP(x, low, high) = (assert(low <= high), x > high ? high : MAXIMUM(x, low))
    let high = n - 2;
    debug_assert!(4 <= high); // CLAMP's embedded assert(low <= high)
    let w = width as usize;
    let width = (if w > high { high } else { w.max(4) }) as u8;
    debug_assert!(
        (width as usize) < n - 1,
        "Insufficient space to print column"
    );

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
        return format!(
            "{:>width$.precision$} ",
            val,
            width = width as usize,
            precision = precision
        );
    }

    *attr = PercentageAttr::Shadow;
    // C: xSnprintf(buffer, n, "%*.*s ", width, width, "N/A")
    let w = width as usize;
    format!("{:>width$.precision$} ", "N/A", width = w, precision = w)
}

/// Port of `void Row_toggleTag(Row* this)` from `Row.c:533`. Flips the
/// user-tag flag.
pub fn Row_toggleTag(this: &mut Row) {
    this.tag = !this.tag;
}

/// Port of `int Row_compare(const void* v1, const void* v2)` from
/// `Row.c:537`. Orders rows by `id` (the default row comparator; the
/// C `const void*` args become `&Row`).
pub fn Row_compare(v1: &Row, v2: &Row) -> i32 {
    spaceship_number!(v1.id, v2.id)
}

/// Port of `int Row_compareByParent_Base(const void* v1, const void* v2)`
/// from `Row.c:544`. Orders by group-or-parent (roots sort as `0`),
/// tie-breaking with [`Row_compare`] — the stable tree-mode ordering.
pub fn Row_compareByParent_Base(v1: &Row, v2: &Row) -> i32 {
    let result = spaceship_number!(
        if v1.isRoot {
            0
        } else {
            Row_getGroupOrParent(v1)
        },
        if v2.isRoot {
            0
        } else {
            Row_getGroupOrParent(v2)
        }
    );

    if result != 0 {
        return result;
    }

    Row_compare(v1, v2)
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
        assert_eq!(
            run(0.0, 7, 5),
            ("  0.0 ".to_string(), PercentageAttr::Shadow)
        );
    }

    #[test]
    fn below_shadow_threshold_is_shadow() {
        // 0.04 < 0.05 => Shadow; rounds to "0.0" at precision 1.
        assert_eq!(
            run(0.04, 7, 5),
            ("  0.0 ".to_string(), PercentageAttr::Shadow)
        );
    }

    #[test]
    fn mid_range_leaves_attr_unchanged() {
        // 0.05 <= 50.0 < 99.9 => no branch fires, attr stays Unchanged.
        // "%5.1f " of 50.0 => " 50.0 ".
        assert_eq!(
            run(50.0, 7, 5),
            (" 50.0 ".to_string(), PercentageAttr::Unchanged)
        );
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
        assert_eq!(
            run(99.9, 6, 4),
            ("99.9 ".to_string(), PercentageAttr::Megabytes)
        );
    }

    #[test]
    fn hundred_at_width_five_keeps_one_decimal() {
        // >= 99.9 => Megabytes; width != 4 so precision 1, val 100.0.
        // "%5.1f " of 100.0 => "100.0 ".
        assert_eq!(
            run(100.0, 7, 5),
            ("100.0 ".to_string(), PercentageAttr::Megabytes)
        );
    }

    #[test]
    fn mem_percent_width_four_collapses_to_integer() {
        // MEM% column: width == 4 && val > 99.9 => precision 0, val=100.
        // "%4.0f " of 100.0 => " 100 ". Also >= 99.9 => Megabytes.
        assert_eq!(
            run(100.0, 6, 4),
            (" 100 ".to_string(), PercentageAttr::Megabytes)
        );
    }

    #[test]
    fn negative_is_na_and_shadow() {
        // val < 0.0 (not nonnegative) => Shadow; "%5.5s " of "N/A".
        assert_eq!(
            run(-1.0, 7, 5),
            ("  N/A ".to_string(), PercentageAttr::Shadow)
        );
    }

    #[test]
    fn nan_is_na_and_shadow() {
        // isNonnegative(NaN) is false => Shadow + N/A path.
        assert_eq!(
            run(f32::NAN, 7, 5),
            ("  N/A ".to_string(), PercentageAttr::Shadow)
        );
    }

    #[test]
    fn width_clamped_to_n_minus_two() {
        // width 200 clamped to CLAMP(200, 4, n-2) = 4 (n=6). 50.0 is
        // mid-range, width==4 but not > 99.9 => precision 1.
        // "%4.1f " of 50.0 => "50.0 ".
        assert_eq!(
            run(50.0, 6, 200),
            ("50.0 ".to_string(), PercentageAttr::Unchanged)
        );
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
        r.chptr
            .iter()
            .take(r.chlen as usize)
            .map(|c| c.attr)
            .collect()
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

    // ── Row data model: lifecycle, comparison, predicates ─────────────

    /// A `Row` with a given `id`; all else defaulted.
    fn row_id(id: i32) -> Row {
        Row {
            id,
            ..Row::default()
        }
    }

    #[test]
    fn row_init_sets_display_defaults_and_host() {
        let mut r = Row::default();
        let host = 0x1234_usize as *const c_void;
        Row_init(&mut r, host);
        assert_eq!(r.host, host);
        assert!(!r.tag);
        assert!(r.showChildren); // C sets this true
        assert!(r.show); // C sets this true
        assert!(!r.wasShown);
        assert!(!r.updated);
        // Fields the C body does NOT touch keep their zero-init values.
        assert_eq!(r.id, 0);
        assert_eq!(r.tombStampMs, 0);
    }

    #[test]
    fn row_done_is_a_noop() {
        // Faithful to C `(void) this;` — no observable effect.
        let r = row_id(7);
        Row_done(&r);
        assert_eq!(r.id, 7);
    }

    #[test]
    fn row_toggle_tag_flips() {
        let mut r = Row::default();
        assert!(!r.tag);
        Row_toggleTag(&mut r);
        assert!(r.tag);
        Row_toggleTag(&mut r);
        assert!(!r.tag);
    }

    #[test]
    fn row_is_tomb_tracks_exit_stamp() {
        let mut r = Row::default();
        assert!(!Row_isTomb(&r)); // 0 => not a tomb
        r.tombStampMs = 1;
        assert!(Row_isTomb(&r));
        r.tombStampMs = u64::MAX;
        assert!(Row_isTomb(&r));
    }

    #[test]
    fn row_compare_orders_by_id() {
        assert_eq!(Row_compare(&row_id(1), &row_id(2)), -1);
        assert_eq!(Row_compare(&row_id(2), &row_id(1)), 1);
        assert_eq!(Row_compare(&row_id(5), &row_id(5)), 0);
        // Negative ids order correctly (SPACESHIP is signed).
        assert_eq!(Row_compare(&row_id(-3), &row_id(4)), -1);
    }

    #[test]
    fn row_compare_dispatches_through_object_trait() {
        // The Row_class.compare slot: Object::compare must reach
        // Row_compare via the trait, ordering identically.
        let a = row_id(1);
        let b = row_id(2);
        assert_eq!(a.compare(&b), -1);
        assert_eq!(b.compare(&a), 1);
        assert_eq!(a.compare(&row_id(1)), 0);
    }

    #[test]
    fn row_get_group_or_parent_picks_parent_when_own_leader() {
        // group == id => use parent; else use group.
        let r = Row {
            id: 10,
            group: 10,
            parent: 3,
            ..Row::default()
        };
        assert_eq!(Row_getGroupOrParent(&r), 3);
        let r2 = Row {
            id: 10,
            group: 7,
            parent: 3,
            ..Row::default()
        };
        assert_eq!(Row_getGroupOrParent(&r2), 7);
    }

    #[test]
    fn row_is_child_of_matches_group_or_parent() {
        let r = Row {
            id: 10,
            group: 7,
            parent: 3,
            ..Row::default()
        };
        assert!(Row_isChildOf(&r, 7));
        assert!(!Row_isChildOf(&r, 3)); // parent not used (group != id)
        assert!(!Row_isChildOf(&r, 10));
    }

    #[test]
    fn row_compare_by_parent_orders_by_group_then_id() {
        // Distinct group-or-parent: ordered by it, ignoring id.
        let a = Row {
            id: 99,
            group: 1,
            parent: 1,
            ..Row::default()
        };
        let b = Row {
            id: 2,
            group: 5,
            parent: 5,
            ..Row::default()
        };
        assert_eq!(Row_compareByParent_Base(&a, &b), -1); // 1 < 5
        assert_eq!(Row_compareByParent_Base(&b, &a), 1);
    }

    #[test]
    fn row_compare_by_parent_ties_break_on_id() {
        // Same group-or-parent (both 4) => tie-break by id.
        let a = Row {
            id: 10,
            group: 4,
            parent: 4,
            ..Row::default()
        };
        let b = Row {
            id: 20,
            group: 4,
            parent: 4,
            ..Row::default()
        };
        assert_eq!(Row_compareByParent_Base(&a, &b), -1); // ids 10 < 20
    }

    #[test]
    fn row_compare_by_parent_roots_sort_as_zero() {
        // isRoot => group-or-parent treated as 0.
        let root = Row {
            id: 50,
            group: 999,
            parent: 999,
            isRoot: true,
            ..Row::default()
        };
        let child = Row {
            id: 3,
            group: 7,
            parent: 7,
            ..Row::default()
        };
        // root's key = 0, child's key = 7 => root sorts first.
        assert_eq!(Row_compareByParent_Base(&root, &child), -1);
    }

    // ── Column-width setters (mutate the Row_*Digits globals) ──────────
    // Each test drives one global only (pid vs uid are distinct atomics),
    // exercising both branches sequentially so no cross-test race exists.

    #[test]
    fn set_pid_column_width_min_then_grows() {
        // < 10^5 keeps the minimum; the exact digit count is used above it.
        Row_setPidColumnWidth(0);
        assert_eq!(Row_pidDigits.load(Ordering::Relaxed), ROW_MIN_PID_DIGITS);
        Row_setPidColumnWidth(99999);
        assert_eq!(Row_pidDigits.load(Ordering::Relaxed), ROW_MIN_PID_DIGITS);
        // 100000 has 6 digits.
        Row_setPidColumnWidth(100000);
        assert_eq!(Row_pidDigits.load(Ordering::Relaxed), 6);
        // 4194304 (a common pid_max) has 7 digits.
        Row_setPidColumnWidth(4194304);
        assert_eq!(Row_pidDigits.load(Ordering::Relaxed), 7);
        // Reset the global so the module's steady state is the C default.
        Row_setPidColumnWidth(0);
        assert_eq!(Row_pidDigits.load(Ordering::Relaxed), ROW_MIN_PID_DIGITS);
    }

    #[test]
    fn set_uid_column_width_min_then_grows() {
        Row_setUidColumnWidth(0);
        assert_eq!(Row_uidDigits.load(Ordering::Relaxed), ROW_MIN_UID_DIGITS);
        Row_setUidColumnWidth(99999);
        assert_eq!(Row_uidDigits.load(Ordering::Relaxed), ROW_MIN_UID_DIGITS);
        // 4294967295 (uid_t max) has 10 digits, <= ROW_MAX_UID_DIGITS.
        Row_setUidColumnWidth(u32::MAX);
        assert_eq!(Row_uidDigits.load(Ordering::Relaxed), 10);
        Row_setUidColumnWidth(0);
        assert_eq!(Row_uidDigits.load(Ordering::Relaxed), ROW_MIN_UID_DIGITS);
    }

    /// [`Row_resetFieldWidths`] seeds an auto-width field to its title length;
    /// [`Row_updateFieldWidth`] grows the width upward only and caps at
    /// `u8::MAX`. (All `Row_fieldWidths` mutation lives in this one test to
    /// avoid racing the shared global across the parallel suite.)
    #[test]
    fn field_widths_reset_grow_and_cap() {
        Row_resetFieldWidths();
        let cgroup = ProcessField::CGROUP as usize;
        // CGROUP is autoWidth with title "CGROUP (raw)".
        assert_eq!(
            Row_fieldWidths[cgroup].load(Ordering::Relaxed),
            "CGROUP (raw)".len() as u8
        );

        let key = ProcessField::CGROUP as RowField;
        Row_updateFieldWidth(key, 40);
        assert_eq!(Row_fieldWidths[cgroup].load(Ordering::Relaxed), 40);
        // Smaller width never shrinks.
        Row_updateFieldWidth(key, 5);
        assert_eq!(Row_fieldWidths[cgroup].load(Ordering::Relaxed), 40);
        // Capped at u8::MAX.
        Row_updateFieldWidth(key, 1000);
        assert_eq!(Row_fieldWidths[cgroup].load(Ordering::Relaxed), u8::MAX);
    }

    /// [`alignedTitleProcessField`] on non-pid, non-uid, non-auto fields (no
    /// dependence on the process-wide digit/width globals): a `None` title
    /// (a gap index) becomes `"- "`, and a plain field returns its title
    /// verbatim (trailing spaces preserved).
    #[test]
    fn aligned_title_reserved_plain_fields() {
        // Index 9 is a gap in the field table → EMPTY_FIELD (title None).
        assert_eq!(alignedTitleProcessField(9), "- ");
        assert_eq!(
            alignedTitleProcessField(ProcessField::PRIORITY as RowField),
            "PRI "
        );
        assert_eq!(
            alignedTitleProcessField(ProcessField::COMM as RowField),
            "Command "
        );
    }

    /// [`RowField_keyAt`] walks the active screen's field list, measuring each
    /// column title, to map a horizontal offset to a field; past the end it
    /// falls back to `COMM`. Uses non-pid/non-auto fields (`PRIORITY`/`NICE`,
    /// both 4-char titles) so the result is independent of the digit globals.
    #[test]
    fn rowfield_keyat_walks_active_screen_fields() {
        use crate::ported::settings::ScreenSettings;
        let mut s = Settings::default();
        s.screens = vec![ScreenSettings {
            fields: vec![
                ProcessField::PRIORITY as RowField,
                ProcessField::NICE as RowField,
            ],
            ..Default::default()
        }];
        // ssIndex defaults to 0.
        assert_eq!(RowField_keyAt(&s, 0), ProcessField::PRIORITY as RowField);
        assert_eq!(RowField_keyAt(&s, 4), ProcessField::PRIORITY as RowField);
        assert_eq!(RowField_keyAt(&s, 5), ProcessField::NICE as RowField);
        assert_eq!(RowField_keyAt(&s, 100), ProcessField::COMM as RowField);
    }
}
