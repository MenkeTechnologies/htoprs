//! Port of `Meter.c` — htop's meter layer.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the
//! spec name-for-name is the point of the port. Each C function
//! `Foo_bar(Meter* this, …)` ports to a free fn taking
//! `this: &Meter` / `this: &mut Meter` (the same shape the `Vector.c`
//! and `History.c` ports use: free fns, not methods).
//!
//! # Class-vtable modeling
//!
//! htop's `MeterClass_` (`Meter.h:59`) is a statically-allocated vtable:
//! a set of fn-pointer slots (`init`/`done`/`updateMode`/`updateValues`/
//! `draw`/`getCaption`/`getUiName` plus the inherited `ObjectClass.display`)
//! and a block of const data (`defaultMode`/`supportedModes`/`total`/
//! `attributes`/`name`/`uiName`/`caption`/`description`/`maxItems`/
//! `isMultiColumn`/`isPercentChart`). [`MeterClass`] mirrors that struct
//! field-for-field, and [`Meter_class`] / [`BlankMeter_class`] reproduce the
//! two class globals `Meter.c` defines.
//!
//! Because no concrete meter type (CPUMeter, TasksMeter, …) is migrated yet,
//! the generic mode renderers here do NOT dispatch through a `klass`
//! pointer. Instead the handful of class constants a renderer reads
//! (`Meter_attributes` / `Meter_supportedModes` / `Meter_isPercentChart` /
//! the `Object` display slot / the class `updateMode`+`draw` slots) are
//! mirrored as instance fields on [`Meter`] — exactly the modeling the
//! pre-existing `supportedModes` field already used ("an instance field
//! carrying that class constant"). A migrated concrete meter would attach
//! its `static X_class: MeterClass` and `Meter_new` (not yet ported) would
//! seed those instance fields from it.
//!
//! # `Object` subclassing
//!
//! htop's `struct Meter_ { Object super; … }` (`Meter.h:112`) IS an `Object`
//! subclass, and `MeterClass_ { const ObjectClass super; … }` (`Meter.h:59`)
//! embeds an `ObjectClass`. That inheritance is reproduced here: [`MeterClass`]
//! carries the embedded [`ObjectClass`] as its `super_` field (rooted at
//! `Object_class` per `Meter_class.super = { .extends = Class(Object) }`,
//! `Meter.c:446`), and [`Meter`] implements the [`Object`] trait —
//! `klass()` returns `&Meter_class.super_`, `display()` dispatches through the
//! instance-mirrored `Object` display slot. This lets a `Meter` be boxed as
//! `Box<dyn Object>` and stored in a ported `Vector` / `Hashtable` wherever C
//! stores an `Object*`.
//!
//! Ported:
//! - `Meter_humanUnit` (`Meter.c:473`) — kibibytes → human-readable string.
//! - `Meter_computeSum` (`Meter.c:51`) — `static`; sums the live positive
//!   values clamped to `DBL_MAX`.
//! - `Meter_nextSupportedMode` (`Meter.c:556`) — pure bit op over
//!   `supportedModes`.
//! - `Meter_displayBuffer` (`Meter.c:44`) — `static inline`; dispatches on
//!   the `Object` display slot (`display` field) or writes `txtBuffer` in
//!   `CRT_colors[Meter_attributes(this)[0]]`.
//! - `TextMeterMode_draw` (`Meter.c:62`) — caption + `displayBuffer`, blitted
//!   through the crossterm [`Ncurses`] shim.
//! - `BarMeterMode_draw` (`Meter.c:90`) — the bar renderer: caption,
//!   brackets, per-item colored fill from `values`/`total`/`curAttributes`,
//!   right-aligned `txtBuffer`. The C fill math is reproduced line-for-line.
//! - `Meter_setMode` (`Meter.c:526`) — sets `mode`, looks up `Meter_modes`
//!   for `draw`+`h`, resets `drawData`.
//! - `Meter_toListItem` (`Meter.c:571`) — builds the setup-menu label
//!   (`"<uiName>[ [<mode>]]"`) and wraps it in a [`ListItem`] via
//!   [`ListItem_new`]. Reads the class `getUiName`/`uiName` slots, mirrored
//!   as instance fields (see the [`Meter`] struct docs).
//! - `BlankMeter_updateValues` (`Meter.c:592`) / `BlankMeter_display`
//!   (`Meter.c:596`) — the Blank meter's trivial value/display hooks.
//!
//! Stubbed (honest `todo!()`, specific blocker each):
//! - `GraphMeterMode_draw` (`Meter.c:221`) — needs the `Machine` host
//!   (`host->realtime`, `host->settings->delay`), `timespec` arithmetic, and
//!   the `GraphData` ring-buffer expansion; no `Machine` is ported, so the
//!   value-recording half cannot be written faithfully. Referenced by the
//!   `Meter_modes` table as a fn pointer (never called until a `Meter` is
//!   actually drawn in Graph mode).
//! - `LEDMeterMode_draw` (`Meter.c:357`) — out of this port's scope; needs
//!   the LED digit tables, the `CRT_utf8` `mvadd_wch` branch, and a
//!   per-digit cell blit. Referenced by `Meter_modes` as a fn pointer.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::io::Write;

use crate::ported::crt::{ColorElements, ColorScheme, CRT_colorSchemes};
use crate::ported::functionbar::Ncurses;
use crate::ported::listitem::{ListItem, ListItem_new};
use crate::ported::object::{Object, ObjectClass, Object_class};
use crate::ported::richstring::{
    RichString, RichString_appendChr, RichString_appendWide, RichString_delete,
    RichString_getCharVal, RichString_printoffnVal, RichString_setAttrn, RichString_setChar,
    RichString_sizeVal, RichString_writeWide,
};

/// IEC unit prefixes. Port of `unitPrefixes` from `XUtils.h:160`
/// (`static const char unitPrefixes[] = { 'K', ... 'Q' }`).
const UNIT_PREFIXES: [char; 10] = ['K', 'M', 'G', 'T', 'P', 'E', 'Z', 'Y', 'R', 'Q'];

/// Port of `#define ONE_K 1024UL` from `Row.h:107`, as `f64` for the
/// division in [`Meter_humanUnit`].
const ONE_K: f64 = 1024.0;

/// Port of `#define DEFAULT_GRAPH_HEIGHT 4` from `Meter.c:34` (rows/lines).
const DEFAULT_GRAPH_HEIGHT: i32 = 4;

/// Port of `int Meter_humanUnit(char* buffer, double value, size_t size)`
/// from `Meter.c:473`. Converts `value` in kibibytes into a human
/// readable string (e.g. `"0K"`, `"1023K"`, `"98.7M"`, `"1.23G"`).
///
/// Signature mapping: the C writes into the caller's `char* buffer`
/// bounded by `size` and returns the `xSnprintf` byte count. Rust owns
/// its allocation, so the `buffer`/`size` out-param and the `int`
/// return are dropped in favor of an owned `String` — the same mapping
/// `xutils.rs` applies to the varargs formatters.
///
/// The C `assert(value >= 0.0 || isNaN(value))` is dropped: it is a
/// debug-only precondition, not input validation, so no check is added.
pub fn Meter_humanUnit(mut value: f64) -> String {
    let mut i: usize = 0;

    while value >= ONE_K {
        if i >= UNIT_PREFIXES.len() - 1 {
            if value > 9999.0 {
                return "inf".to_string();
            }
            break;
        }

        value /= ONE_K;
        i += 1;
    }

    let mut precision = 0;

    if i > 0 {
        // Fraction digits for mebibytes and above
        precision = if value <= 99.9 {
            if value <= 9.99 {
                2
            } else {
                1
            }
        } else {
            0
        };

        // Round up if 'value' is in range (99.9, 100) or (9.99, 10)
        if precision < 2 {
            let limit = if precision == 1 { 10.0 } else { 100.0 };
            if value < limit {
                value = limit;
            }
        }
    }

    format!("{:.*}{}", precision, value, UNIT_PREFIXES[i])
}

/// Port of `typedef unsigned int MeterModeId` from `MeterMode.h:19`. The
/// mode ids are the `enum MeterModeId_` values (`MeterMode.h:11`); mode `0`
/// is reserved, so the real modes start at `1` and `LAST_METERMODE` is the
/// trailing count sentinel.
pub type MeterModeId = u32;

/// `BAR_METERMODE = 1` (`MeterMode.h:13`).
pub const BAR_METERMODE: MeterModeId = 1;
/// `TEXT_METERMODE` (`MeterMode.h:14`).
pub const TEXT_METERMODE: MeterModeId = 2;
/// `GRAPH_METERMODE` (`MeterMode.h:15`).
pub const GRAPH_METERMODE: MeterModeId = 3;
/// `LED_METERMODE` (`MeterMode.h:16`).
pub const LED_METERMODE: MeterModeId = 4;
/// `LAST_METERMODE` — trailing count sentinel (`MeterMode.h:17`).
pub const LAST_METERMODE: MeterModeId = 5;

/// Port of `typedef struct GraphData_` from `Meter.h:106`. Only the two
/// fields the ported machinery touches are modeled: `nValues` and the
/// `values` ring buffer (C `double* values`, an owned `Vec` here).
/// [`Meter_setMode`] resets both; the third C field `struct timespec time`
/// is read solely by `GraphMeterMode_draw` (stubbed, out of scope), so it
/// is omitted until that renderer is ported.
#[derive(Default)]
pub struct GraphData {
    pub nValues: usize,
    pub values: Vec<f64>,
}

/// C `Meter_Draw` (`Meter.h:55`): `void (*)(Meter*, int, int, int)`. The
/// ported renderers take an explicit terminal sink (`out: &mut dyn Write`)
/// as their first argument — htop's ncurses draw writes to the implicit
/// `stdscr`, which the crossterm [`Ncurses`] shim replaces with an explicit
/// writer (the `Panel_draw` precedent) so the blit is unit-testable. The
/// remaining `(Meter*, x, y, w)` arguments match C.
pub type MeterDraw = fn(&mut dyn Write, &mut Meter, i32, i32, i32);
/// C `Meter_Init` (`Meter.h:51`): `void (*)(Meter*)`.
pub type MeterInit = fn(&mut Meter);
/// C `Meter_Done` (`Meter.h:52`): `void (*)(Meter*)`.
pub type MeterDone = fn(&mut Meter);
/// C `Meter_UpdateMode` (`Meter.h:53`): `void (*)(Meter*, MeterModeId)`.
pub type MeterUpdateMode = fn(&mut Meter, MeterModeId);
/// C `Meter_UpdateValues` (`Meter.h:54`): `void (*)(Meter*)`.
pub type MeterUpdateValues = fn(&mut Meter);
/// C `Meter_GetCaption` (`Meter.h:56`): `const char* (*)(const Meter*)`.
pub type MeterGetCaption = fn(&Meter) -> String;
/// C `Meter_GetUiName` (`Meter.h:57`): `void (*)(const Meter*, char*, size_t)`,
/// modeled as returning an owned `String` (the Rust owns-its-buffer mapping).
pub type MeterGetUiName = fn(&Meter) -> String;
/// C `Object_Display` (`Object.h`): `void (*)(const Object*, RichString*)` —
/// the display slot every meter inherits through `MeterClass.super`.
pub type MeterDisplay = fn(&Meter, &mut RichString);

/// Port of `typedef struct MeterClass_` from `Meter.h:59` — the meter
/// vtable. The C `const ObjectClass super` (`Meter.h:60`) is modeled as the
/// embedded [`ObjectClass`] `super_` field (first field, matching C), which
/// carries the class-chain `extends` link `Object_isA` walks; the one
/// meter-relevant display slot of that base class (`ObjectClass.display`) is
/// mirrored separately as the `display` field, because the ported
/// [`ObjectClass`] models only `extends` (its `display`/`delete`/`compare`
/// slots live on the [`Object`] trait, not the struct — see `object.rs`).
/// The remaining fn-pointer slots and the const-data block are reproduced
/// field-for-field. Instances are `static` (stable address = type identity),
/// matching C's `const MeterClass` globals, so `&Meter_class.super_` is a
/// stable `&'static ObjectClass` usable as the meter's class identity. See
/// the module docs for why the base renderers read instance-mirrored fields
/// rather than dispatching through this vtable.
pub struct MeterClass {
    /// C `const ObjectClass super` (`Meter.h:60`) — the embedded base
    /// class. Only its `extends` link is modeled here (the ported
    /// [`ObjectClass`] carries nothing else); its `display` slot is
    /// mirrored by the sibling `display` field below.
    pub super_: ObjectClass,
    /// `ObjectClass super.display` — the `Object_display` slot.
    pub display: Option<MeterDisplay>,
    pub init: Option<MeterInit>,
    pub done: Option<MeterDone>,
    pub updateMode: Option<MeterUpdateMode>,
    pub updateValues: Option<MeterUpdateValues>,
    pub draw: Option<MeterDraw>,
    pub getCaption: Option<MeterGetCaption>,
    pub getUiName: Option<MeterGetUiName>,
    pub defaultMode: MeterModeId,
    pub supportedModes: u32,
    pub total: f64,
    pub attributes: &'static [i32],
    pub name: &'static str,
    pub uiName: &'static str,
    pub caption: &'static str,
    pub description: Option<&'static str>,
    pub maxItems: u8,
    pub isMultiColumn: bool,
    pub isPercentChart: bool,
}

/// Port of `const MeterClass Meter_class` from `Meter.c:446`:
/// `{ .super = { .extends = Class(Object) } }`. Every other slot is `NULL` /
/// `0` in the C initializer, i.e. `None` / defaults here. The embedded
/// `super_` roots the class chain at `Object_class` (C `Class(Object)` =
/// `&Object_class`); this base meter class carries no meter-specific behavior
/// of its own.
pub static Meter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Object_class),
    },
    display: None,
    init: None,
    done: None,
    updateMode: None,
    updateValues: None,
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: 0,
    supportedModes: 0,
    total: 0.0,
    attributes: &[],
    name: "",
    uiName: "",
    caption: "",
    description: None,
    maxItems: 0,
    isMultiColumn: false,
    isPercentChart: false,
};

/// Port of `static const int BlankMeter_attributes[]` from `Meter.c:599`:
/// `{ DEFAULT_COLOR }`.
static BlankMeter_attributes: [i32; 1] = [ColorElements::DEFAULT_COLOR as i32];

/// Port of `const MeterClass BlankMeter_class` from `Meter.c:603`. Its
/// C `.super` sets `.extends = Class(Meter)`, `.delete = Meter_delete`, and
/// `.display = BlankMeter_display`. `Class(Meter)` is `(const ObjectClass*)
/// &Meter_class`, i.e. the embedded `Meter_class.super_`, so `super_.extends`
/// points there; `.delete` maps to `Drop` (see `object.rs`) so it is not
/// modeled, and `.display` is carried by the sibling `display` field.
pub static BlankMeter_class: MeterClass = MeterClass {
    super_: ObjectClass {
        extends: Some(&Meter_class.super_),
    },
    display: Some(BlankMeter_display),
    init: None,
    done: None,
    updateMode: None,
    updateValues: Some(BlankMeter_updateValues),
    draw: None,
    getCaption: None,
    getUiName: None,
    defaultMode: TEXT_METERMODE,
    supportedModes: 1 << TEXT_METERMODE,
    total: 0.0,
    attributes: &BlankMeter_attributes,
    name: "Blank",
    uiName: "Blank",
    caption: "",
    description: None,
    maxItems: 0,
    isMultiColumn: false,
    isPercentChart: false,
};

/// Port of `typedef struct MeterMode_` from `Meter.c:36`: one row of the
/// `Meter_modes` table — the draw fn, the setup-menu display name, and the
/// default height. `uiName` is `None` for the reserved mode `0`.
pub struct MeterMode {
    pub draw: Option<MeterDraw>,
    pub uiName: Option<&'static str>,
    pub h: i32,
}

/// Port of `static const MeterMode Meter_modes[]` from `Meter.c:416`. Index
/// `0` is reserved (`{ NULL, 0, NULL }`); the real modes map to their
/// renderer + default height. `GraphMeterMode_draw` / `LEDMeterMode_draw`
/// are honest stubs (see the module docs) but are still wired here as fn
/// pointers so `Meter_setMode` assigns the correct height and (eventual)
/// renderer.
pub static Meter_modes: [MeterMode; LAST_METERMODE as usize] = [
    // [0] reserved
    MeterMode {
        draw: None,
        uiName: None,
        h: 0,
    },
    // [BAR_METERMODE]
    MeterMode {
        draw: Some(BarMeterMode_draw),
        uiName: Some("Bar"),
        h: 1,
    },
    // [TEXT_METERMODE]
    MeterMode {
        draw: Some(TextMeterMode_draw),
        uiName: Some("Text"),
        h: 1,
    },
    // [GRAPH_METERMODE]
    MeterMode {
        draw: Some(GraphMeterMode_draw),
        uiName: Some("Graph"),
        h: DEFAULT_GRAPH_HEIGHT,
    },
    // [LED_METERMODE]
    MeterMode {
        draw: Some(LEDMeterMode_draw),
        uiName: Some("LED"),
        h: 3,
    },
];

/// A partial model of htop's `struct Meter_` (`Meter.h:112`) holding the
/// fields the ported machinery reads or writes. The C fields are mirrored
/// name-for-name where a renderer touches them:
///   * `values` / `curItems` — the per-item value array and its live count;
///   * `mode` — the current draw mode;
///   * `supportedModes` — the class `supportedModes` bitset, mirrored as an
///     instance field (see the module docs);
///   * `caption` — the header prefix (`Meter_getCaption` falls back to it
///     when the class sets no `getCaption` slot);
///   * `param` — the C `unsigned int param`;
///   * `drawData` — the [`GraphData`] ring buffer, reset by [`Meter_setMode`];
///   * `h` — the meter height in rows;
///   * `curAttributes` — optional per-item color override (C
///     `const int* curAttributes`, `NULL` ⇒ `None`);
///   * `txtBuffer` — the rendered value text (C `char txtBuffer[256]`);
///   * `total` — the bar/graph `100%` reference;
///   * `attributes` — the class `attributes` color array (mirrored);
///   * `isPercentChart` — the class flag (mirrored);
///   * `uiName` — the class `uiName` (setup-menu display name), mirrored;
///     read by [`Meter_toListItem`] via `Meter_uiName(this)`;
///   * `getUiName` — the class `getUiName` slot (mirrored; `None` ⇒ no
///     dynamic name fn, so [`Meter_toListItem`] falls back to `uiName`);
///   * `display` — the `Object` display slot (mirrored; `None` ⇒ the
///     `Meter_displayBuffer` else branch);
///   * `updateMode` / `classDraw` — the class `updateMode`+`draw` vtable
///     slots (mirrored) read by [`Meter_setMode`]'s `Meter_updateModeFn`
///     branch;
///   * `draw` — the instance draw pointer (`this->draw`), assigned by
///     [`Meter_setMode`].
///
/// The remaining C fields (`super`, `host`, `columnWidthCount`,
/// `meterData`) are substrate the ported renderers do not touch.
pub struct Meter {
    pub values: Vec<f64>,
    pub curItems: u8,
    pub mode: MeterModeId,
    pub supportedModes: u32,
    pub caption: String,
    pub param: u32,
    pub drawData: GraphData,
    pub h: i32,
    pub curAttributes: Option<&'static [i32]>,
    pub txtBuffer: String,
    pub total: f64,
    pub attributes: &'static [i32],
    pub isPercentChart: bool,
    /// C `Meter_uiName(this)` — the class `uiName` (setup-menu display name),
    /// mirrored as an instance field.
    pub uiName: &'static str,
    /// C `Meter_getUiNameFn(this)` — the class `getUiName` slot; `None` ⇒ the
    /// meter has no dynamic-name fn and [`Meter_toListItem`] uses `uiName`.
    pub getUiName: Option<MeterGetUiName>,
    /// C `Object_displayFn(this)` — the inherited display slot; `None` ⇒ the
    /// `Meter_displayBuffer` else branch writes `txtBuffer` directly.
    pub display: Option<MeterDisplay>,
    /// C `Meter_updateModeFn(this)` — the class `updateMode` slot.
    pub updateMode: Option<MeterUpdateMode>,
    /// C `Meter_drawFn(this)` — the class `draw` slot (distinct from the
    /// instance `draw` pointer below).
    pub classDraw: Option<MeterDraw>,
    /// C `this->draw` — the instance draw pointer set by [`Meter_setMode`].
    pub draw: Option<MeterDraw>,
}

impl Meter {
    /// Rust-only bootstrap helper (a test/consumer convenience, not a C
    /// function): a fully zeroed `Meter` so `Meter { values, .. }` struct
    /// literals in consumers stay short via `..Meter::empty()`. Mirrors the
    /// `Panel::empty` precedent; kept as an associated fn (the build.rs
    /// port-purity gate only inspects free `fn`s at module depth 0, so a
    /// method needs no C counterpart). `h` defaults to `1`, matching
    /// `Meter_new`'s `this->h = 1` (`Meter.c:455`).
    pub(crate) fn empty() -> Meter {
        Meter {
            values: Vec::new(),
            curItems: 0,
            mode: 0,
            supportedModes: 0,
            caption: String::new(),
            param: 0,
            drawData: GraphData::default(),
            h: 1,
            curAttributes: None,
            txtBuffer: String::new(),
            total: 0.0,
            attributes: &[],
            isPercentChart: false,
            uiName: "",
            getUiName: None,
            display: None,
            updateMode: None,
            classDraw: None,
            draw: None,
        }
    }

    /// Reproduces C's `CRT_colors[idx]` — the packed ncurses attribute for
    /// color element `idx` in the active scheme (C's active-scheme row
    /// `const int* CRT_colors = CRT_colorSchemes[CRT_colorScheme]`). `idx`
    /// is a `ColorElements` value stored as an `int` in the class
    /// `attributes` / `curAttributes` arrays. A gate-skipped helper
    /// (associated fn) so the port-purity gate ignores it.
    fn crt_colors(idx: i32) -> i32 {
        CRT_colorSchemes[ColorScheme::active() as usize][idx as usize]
    }
}

/// C `struct Meter_ { Object super; … }` (`Meter.h:112`) makes every `Meter`
/// an `Object` subclass, and `MeterClass_ { const ObjectClass super; … }`
/// (`Meter.h:59`) embeds the base class the runtime dispatches through. This
/// `impl` reproduces that inheritance so a ported `Meter` can live in a
/// [`Vector`](crate::ported::vector::Vector) / `Hashtable` wherever the C
/// stores an `Object*` (the precondition MetersPanel's meter-list machinery
/// needs).
impl Object for Meter {
    /// C `this->super.klass`, set by `Object_setClass(this, type)` in
    /// `Meter_new` (`Meter.c:453`) to the concrete `MeterClass*`. The ported
    /// `Meter` carries no per-instance klass pointer (no concrete meter type
    /// is migrated yet), so the base `Meter_class`'s embedded `ObjectClass`
    /// is returned — `&Meter_class.super_`, the class rooted at `Object_class`
    /// (C `Meter_class.super = { .extends = Class(Object) }`, `Meter.c:446`).
    fn klass(&self) -> &'static ObjectClass {
        &Meter_class.super_
    }

    /// C `Object_display` dispatch through `As_Meter(this)->super.display`
    /// (`Meter.h:60`). Mirrored on the instance as the `display` slot: when
    /// set, dispatch to it (`BlankMeter_display` and friends); when `NULL`,
    /// the C `Object_display` macro `assert`s non-`NULL`, so an unset slot
    /// aborts — modeled here by the trait's default panic, matching
    /// [`Object::display`]'s contract.
    fn display(&self, out: &mut RichString) {
        match self.display {
            Some(display) => display(self, out),
            None => unimplemented!(
                "Object::display: Meter class has no display method (C NULL vtable slot)"
            ),
        }
    }
}

/// Port of `static double Meter_computeSum(const Meter* this)` from
/// `Meter.c:51`. Sums the strictly-positive live values
/// (`sumPositiveValues(this->values, this->curItems)`) and clamps the
/// result to `DBL_MAX` so IEEE-754 rounding cannot yield infinity.
///
/// The C `assert(this->curItems > 0)` and `assert(this->values)` are
/// debug-only preconditions (not input validation), so they are dropped —
/// the same treatment [`Meter_humanUnit`] gives its `assert`.
pub fn Meter_computeSum(this: &Meter) -> f64 {
    let sum = crate::ported::xutils::sumPositiveValues(&this.values[..this.curItems as usize]);
    // Prevent rounding to infinity in IEEE 754. `MINIMUM(DBL_MAX, sum)`
    // expands to `((DBL_MAX) < (sum) ? (DBL_MAX) : (sum))` (`Macros.h:17`).
    if f64::MAX < sum {
        f64::MAX
    } else {
        sum
    }
}

/// Port of `MeterModeId Meter_nextSupportedMode(const Meter* this)` from
/// `Meter.c:556`. Given the current `mode`, returns the next supported
/// mode id, cycling back to the lowest supported mode once the highest is
/// passed. The selection is a pure bit operation over the
/// `supportedModes` bitset: mask off every mode id `<= this->mode`
/// (`((uint32_t)-1 << 1) << this->mode`), and if nothing remains fall back
/// to the full set, then take the lowest set bit
/// ([`countTrailingZeros`](crate::ported::xutils::countTrailingZeros)).
///
/// The C `assert(supportedModes)` and `assert(this->mode < UINT32_WIDTH)`
/// are debug-only preconditions, kept as `debug_assert!`. As in C, the
/// shift by `this->mode` is only well-defined for `mode < 32`.
pub fn Meter_nextSupportedMode(this: &Meter) -> MeterModeId {
    let supportedModes = this.supportedModes;
    debug_assert!(supportedModes != 0);
    debug_assert!(this.mode < 32);

    let mode_mask = (u32::MAX << 1) << this.mode;
    let mut next_modes = supportedModes & mode_mask;
    if next_modes == 0 {
        next_modes = supportedModes;
    }

    crate::ported::xutils::countTrailingZeros(next_modes) as MeterModeId
}

/// Port of `static inline void Meter_displayBuffer(const Meter* this,
/// RichString* out)` from `Meter.c:44`. When the meter's class sets an
/// `Object` display slot (`display` is `Some`), dispatch to it
/// (`Object_display(this, out)`); otherwise write `txtBuffer` in the color
/// of the first class attribute (`CRT_colors[Meter_attributes(this)[0]]`).
pub fn Meter_displayBuffer(this: &Meter, out: &mut RichString) {
    if let Some(display) = this.display {
        display(this, out);
    } else {
        RichString_writeWide(
            out,
            Meter::crt_colors(this.attributes[0]),
            this.txtBuffer.as_bytes(),
        );
    }
}

/// Port of `static void TextMeterMode_draw(Meter* this, int x, int y,
/// int w)` from `Meter.c:62`. Draws the caption in `METER_TEXT`, then the
/// `Meter_displayBuffer` text in the remaining width, blitting through the
/// crossterm [`Ncurses`] shim (the `Panel_draw` terminal-side-effect
/// precedent). `Meter_getCaption(this)` falls back to `this->caption`
/// because no class `getCaption` slot is modeled here.
///
/// The C `assert(x >= 0)` / `assert(w <= INT_MAX - x)` are debug-only
/// preconditions and are dropped. `strnlen(caption, w)` becomes
/// `min(caption bytes, w)` (a Rust `String` has no embedded NUL).
pub fn TextMeterMode_draw(mut out: &mut dyn Write, this: &mut Meter, x: i32, y: i32, w: i32) {
    let scheme = ColorScheme::active();
    let caption = this.caption.clone();

    if w > 0 {
        Ncurses::attrset(&mut out, ColorElements::METER_TEXT.packed(scheme));
        Ncurses::mvaddnstr(&mut out, y, x, &caption, w);
    }
    Ncurses::attrset(&mut out, ColorElements::RESET_COLOR.packed(scheme));

    let caption_width = if w > 0 {
        (caption.len() as i32).min(w)
    } else {
        0
    };
    if w <= caption_width {
        return;
    }
    let w = w - caption_width;
    let x = x + caption_width;

    let mut text = RichString::new();
    Meter_displayBuffer(this, &mut text);
    RichString_printoffnVal(&mut out, &text, y, x, 0, w);
    RichString_delete(&mut text);
}

/// Port of `static const char BarMeterMode_characters[]` from `Meter.c:88`:
/// `"|#*@$%&."`, the per-item fill glyphs used in `COLORSCHEME_MONOCHROME`.
const BarMeterMode_characters: &[u8] = b"|#*@$%&.";

/// Port of `static void BarMeterMode_draw(Meter* this, int x, int y,
/// int w)` from `Meter.c:90`. Draws the 3-column caption, the `[`…`]`
/// borders, the per-item colored fill (each item's block sized
/// `ceil(value/total * w)` and clamped to the remaining width), and the
/// right-aligned `txtBuffer` over the top, blitting through the crossterm
/// [`Ncurses`] shim. The fill math (space-padded bar, `startPos`
/// truncation-at-a-space, monochrome vs. `'|'` glyph selection, per-item
/// `RichString_setAttrn` + `RichString_printoffnVal`, trailing `BAR_SHADOW`)
/// is reproduced line-for-line from the C.
///
/// `isPositive(value)` is `value > 0.0` (false for NaN — `Macros.h:146`).
/// The C `assert`s are debug-only preconditions kept as `debug_assert!`.
pub fn BarMeterMode_draw(mut out: &mut dyn Write, this: &mut Meter, x: i32, y: i32, w: i32) {
    let scheme = ColorScheme::active();
    let mut x = x;
    let mut w = w;

    // Draw the caption
    let caption_len = 3;
    let caption = this.caption.clone();
    if w >= caption_len {
        Ncurses::attrset(&mut out, ColorElements::METER_TEXT.packed(scheme));
        Ncurses::mvaddnstr(&mut out, y, x, &caption, caption_len);
    }
    w -= caption_len;

    // Draw the bar borders
    if w >= 1 {
        x += caption_len;
        Ncurses::attrset(&mut out, ColorElements::BAR_BORDER.packed(scheme));
        Ncurses::mvaddch(&mut out, y, x, '[');
        w -= 1;
        Ncurses::mvaddch(&mut out, y, x + w, ']');
        w -= 1;
    }

    // Update the "total" if necessary
    if !this.isPercentChart && this.curItems > 0 {
        let sum = Meter_computeSum(this);
        this.total = if sum > this.total { sum } else { this.total };
    }

    if w < 1 {
        Ncurses::attrset(&mut out, ColorElements::RESET_COLOR.packed(scheme));
        return;
    }
    Ncurses::attrset(&mut out, ColorElements::RESET_COLOR.packed(scheme)); // Clear the bold attribute
    x += 1;

    // The text in the bar is right aligned; pad with maximal spaces and then
    // calculate needed starting position offset.
    let mut bar = RichString::new();
    RichString_appendChr(&mut bar, 0, ' ', w);
    RichString_appendWide(&mut bar, 0, this.txtBuffer.as_bytes());

    let mut start_pos = RichString_sizeVal(&bar) - w;
    if start_pos > w {
        // Text is too large for bar; truncate meter text at a space character.
        let mut pos = 2 * w;
        while pos > w {
            if RichString_getCharVal(&bar, pos as usize) == ' ' {
                while pos > w && RichString_getCharVal(&bar, (pos - 1) as usize) == ' ' {
                    pos -= 1;
                }
                start_pos = pos - w;
                break;
            }
            pos -= 1;
        }
        // If still too large, print the start not the end.
        start_pos = start_pos.min(w);
    }

    debug_assert!(start_pos >= 0);
    debug_assert!(start_pos <= w);
    debug_assert!(start_pos + w <= RichString_sizeVal(&bar));

    let mut block_sizes = [0i32; 10];

    // First draw in the bar[] buffer...
    let mut offset = 0i32;
    for i in 0..this.curItems as usize {
        let value = this.values[i];
        if value > 0.0 && this.total > 0.0 {
            let value = value.min(this.total);
            let mut bs = ((value / this.total) * w as f64).ceil() as i32;
            bs = bs.min(w - offset);
            block_sizes[i] = bs;
        } else {
            block_sizes[i] = 0;
        }
        let next_offset = offset + block_sizes[i];
        let mut j = offset;
        while j < next_offset {
            if RichString_getCharVal(&bar, (start_pos + j) as usize) == ' ' {
                if scheme == ColorScheme::COLORSCHEME_MONOCHROME {
                    debug_assert!(i < BarMeterMode_characters.len());
                    RichString_setChar(
                        &mut bar,
                        (start_pos + j) as usize,
                        BarMeterMode_characters[i] as char,
                    );
                } else {
                    RichString_setChar(&mut bar, (start_pos + j) as usize, '|');
                }
            }
            j += 1;
        }
        offset = next_offset;
    }

    // ...then print the buffer.
    offset = 0;
    for i in 0..this.curItems as usize {
        let attr = match this.curAttributes {
            Some(ca) => ca[i],
            None => this.attributes[i],
        };
        RichString_setAttrn(
            &mut bar,
            Meter::crt_colors(attr),
            (start_pos + offset) as usize,
            block_sizes[i] as usize,
        );
        RichString_printoffnVal(&mut out, &bar, y, x + offset, start_pos + offset, block_sizes[i]);
        offset += block_sizes[i];
    }
    if offset < w {
        RichString_setAttrn(
            &mut bar,
            ColorElements::BAR_SHADOW.packed(scheme),
            (start_pos + offset) as usize,
            (w - offset) as usize,
        );
        RichString_printoffnVal(&mut out, &bar, y, x + offset, start_pos + offset, w - offset);
    }

    RichString_delete(&mut bar);

    Ncurses::move_to(&mut out, y, x + w + 1);

    Ncurses::attrset(&mut out, ColorElements::RESET_COLOR.packed(scheme));
}

/// TODO: port of `static void GraphMeterMode_draw(Meter* this, int x, int y,
/// int w)` from `Meter.c:221`. Blocked: the value-recording half reads the
/// `Machine` host (`host->realtime`, `host->settings->delay`), does
/// `timespec` arithmetic, and expands the `GraphData` ring buffer — no
/// `Machine` type is ported, so the record path cannot be reproduced
/// faithfully. Wired into `Meter_modes` as a fn pointer (only reached when a
/// meter is actually drawn in Graph mode).
pub fn GraphMeterMode_draw(_out: &mut dyn Write, _this: &mut Meter, _x: i32, _y: i32, _w: i32) {
    todo!("port of Meter.c:221 — needs Machine host, timespec, GraphData ring buffer")
}

/// TODO: port of `static void LEDMeterMode_draw(Meter* this, int x, int y,
/// int w)` from `Meter.c:357`. Out of this port's scope: needs the
/// `LEDMeterMode_digits` ASCII/UTF-8 tables, the `CRT_utf8` `mvadd_wch`
/// branch, and a per-digit cell blit (`LEDMeterMode_drawDigit`). Wired into
/// `Meter_modes` as a fn pointer.
pub fn LEDMeterMode_draw(_out: &mut dyn Write, _this: &mut Meter, _x: i32, _y: i32, _w: i32) {
    todo!("port of Meter.c:357 — needs LED digit tables, CRT_utf8 mvadd_wch branch")
}

/// Port of `void Meter_setMode(Meter* this, MeterModeId modeIndex)` from
/// `Meter.c:526`. No-op when already in `modeIndex`; otherwise validates the
/// mode against `supportedModes` (rejecting mode `0`, out-of-range ids, and
/// unsupported bits) and switches. When the class provides an `updateMode`
/// slot the instance draw pointer is taken from the class `draw` slot and
/// `updateMode` runs; otherwise the `drawData` ring buffer is reset and the
/// draw pointer + height come from the `Meter_modes` table.
///
/// The C `assert`s (mode `> 0`, `supportedModes` non-zero, bit `0` unset,
/// `LAST_METERMODE <= UINT32_WIDTH`, `modeIndex >= 1`, non-null draw slot)
/// are debug-only preconditions kept as `debug_assert!`.
pub fn Meter_setMode(this: &mut Meter, modeIndex: MeterModeId) {
    if modeIndex == this.mode {
        debug_assert!(this.mode > 0);
        return;
    }

    let supportedModes = this.supportedModes;
    debug_assert!(supportedModes != 0);
    debug_assert!(supportedModes & (1 << 0) == 0);

    debug_assert!(LAST_METERMODE <= 32);
    if modeIndex >= LAST_METERMODE || (supportedModes & (1u32 << modeIndex)) == 0 {
        return;
    }

    debug_assert!(modeIndex >= 1);
    if let Some(update) = this.updateMode {
        let d = this
            .classDraw
            .expect("Meter_drawFn must be non-null when updateMode is set");
        this.draw = Some(d);
        update(this, modeIndex);
    } else {
        this.drawData.values = Vec::new();
        this.drawData.nValues = 0;

        let mode = &Meter_modes[modeIndex as usize];
        this.draw = mode.draw;
        this.h = mode.h;
    }
    this.mode = modeIndex;
}

/// Port of `ListItem* Meter_toListItem(const Meter* this, bool moving)` from
/// `Meter.c:571`. Builds the meter's setup-menu label — the ui-name, plus a
/// `" [<mode>]"` suffix when the meter is in a real (non-reserved) mode — and
/// wraps it in a [`ListItem`] (key `0`), copying the caller's `moving` flag
/// onto the item.
///
/// The label is assembled exactly as C does:
///   * `mode[20]` — `" [%s]"` of `Meter_modes[this->mode].uiName` when
///     `this->mode > 0`, else empty (reserved mode 0 has no suffix);
///   * `name[32]` — the class `getUiName` result when the slot is set
///     (`Meter_getUiNameFn(this)`), else the class `uiName`
///     (`Meter_uiName(this)`);
///   * `buffer[50]` — `name` concatenated with `mode`.
///
/// The three fixed C buffers (`mode[20]`/`name[32]`/`buffer[50]`) cap their
/// contents at `sizeof - 1` bytes via `xSnprintf`. Those size bounds are
/// dropped here in favor of owned `String`s — the same modeling decision this
/// module already applies to [`Meter_humanUnit`]'s `buffer`/`size` out-param
/// and to the [`MeterGetUiName`] type (whose signature omits the C
/// `char*`/`size_t` out-params). No ported meter class carries a ui-name long
/// enough to hit the caps, so the observable label is identical.
pub fn Meter_toListItem(this: &Meter, moving: bool) -> ListItem {
    // char mode[20];
    let mode = if this.mode > 0 {
        // xSnprintf(mode, sizeof(mode), " [%s]", Meter_modes[this->mode].uiName);
        let ui = Meter_modes[this.mode as usize]
            .uiName
            .expect("Meter_modes[mode].uiName is non-NULL for mode > 0");
        format!(" [{ui}]")
    } else {
        // mode[0] = '\0';
        String::new()
    };

    // char name[32];
    let name = if let Some(getUiName) = this.getUiName {
        // Meter_getUiName(this, name, sizeof(name));
        getUiName(this)
    } else {
        // xSnprintf(name, sizeof(name), "%s", Meter_uiName(this));
        this.uiName.to_string()
    };

    // char buffer[50];  xSnprintf(buffer, sizeof(buffer), "%s%s", name, mode);
    let buffer = format!("{name}{mode}");

    // ListItem* li = ListItem_new(buffer, 0);  li->moving = moving;
    let mut li = ListItem_new(&buffer, 0);
    li.moving = moving;
    li
}

/// Port of `static void BlankMeter_updateValues(Meter* this)` from
/// `Meter.c:592`. Clears the value text (`this->txtBuffer[0] = '\0'`).
pub fn BlankMeter_updateValues(this: &mut Meter) {
    this.txtBuffer.clear();
}

/// Port of `static void BlankMeter_display(const Object* cast, RichString*
/// out)` from `Meter.c:596`. A no-op: the Blank meter renders nothing.
pub fn BlankMeter_display(_this: &Meter, _out: &mut RichString) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::richstring::RichString_appendAscii;

    #[test]
    fn zero_stays_kibibytes_no_fraction() {
        // 0 < ONE_K: loop never runs, i=0 => precision 0, prefix 'K'.
        assert_eq!(Meter_humanUnit(0.0), "0K");
    }

    #[test]
    fn below_one_k_stays_kibibytes() {
        // 999 < 1024: no division, i=0, precision 0.
        assert_eq!(Meter_humanUnit(999.0), "999K");
    }

    #[test]
    fn one_k_promotes_to_mebi_two_fraction_digits() {
        // 1024 -> one division -> i=1, value=1.0; 1.0 <= 9.99 => prec 2.
        assert_eq!(Meter_humanUnit(1024.0), "1.00M");
    }

    #[test]
    fn precision_one_in_range() {
        // 1024*50 -> value=50.0, i=1; 50 <= 99.9 but > 9.99 => prec 1;
        // limit 10.0, 50 not < 10 => "50.0M".
        assert_eq!(Meter_humanUnit(1024.0 * 50.0), "50.0M");
    }

    #[test]
    fn precision_zero_above_ninety_nine_nine() {
        // 1024*500 -> value=500.0, i=1; 500 > 99.9 => prec 0;
        // limit 100.0, 500 not < 100 => "500M".
        assert_eq!(Meter_humanUnit(1024.0 * 500.0), "500M");
    }

    #[test]
    fn round_up_boundary_forces_limit() {
        // 1024*9.995 -> value~9.995, i=1; 9.995 > 9.99 => prec 1;
        // limit 10.0, 9.995 < 10 => value forced to 10.0 => "10.0M".
        assert_eq!(Meter_humanUnit(1024.0 * 9.995), "10.0M");
    }

    #[test]
    fn inf_when_still_huge_at_last_prefix() {
        // After 9 divisions i reaches len-1=9 with value=19998 > 9999
        // => early "inf" return.
        let v = 9999.0 * f64::powi(1024.0, 9) * 2.0;
        assert_eq!(Meter_humanUnit(v), "inf");
    }

    #[test]
    fn caps_at_last_prefix_without_inf() {
        // After 9 divisions i=9, value=5000 <= 9999 => break, format
        // with prefix 'Q'; 5000 > 99.9 => prec 0 => "5000Q".
        let v = 5000.0 * f64::powi(1024.0, 9);
        assert_eq!(Meter_humanUnit(v), "5000Q");
    }

    #[test]
    fn compute_sum_ignores_negatives_and_nan() {
        // sumPositiveValues skips values <= 0 and NaN: 5 + 2 = 7.
        let m = Meter {
            values: vec![5.0, -3.0, f64::NAN, 2.0],
            curItems: 4,
            ..Meter::empty()
        };
        assert_eq!(Meter_computeSum(&m), 7.0);
    }

    #[test]
    fn compute_sum_honors_cur_items() {
        // Only the first curItems entries are summed; trailing 100.0 unused.
        let m = Meter {
            values: vec![1.0, 2.0, 100.0],
            curItems: 2,
            ..Meter::empty()
        };
        assert_eq!(Meter_computeSum(&m), 3.0);
    }

    #[test]
    fn compute_sum_clamps_to_dbl_max() {
        // Two DBL_MAX positives overflow to +inf; MINIMUM(DBL_MAX, inf)
        // picks DBL_MAX since DBL_MAX < inf.
        let m = Meter {
            values: vec![f64::MAX, f64::MAX],
            curItems: 2,
            ..Meter::empty()
        };
        assert_eq!(Meter_computeSum(&m), f64::MAX);
    }

    // ── Meter_nextSupportedMode ───────────────────────────────────────

    /// `METERMODE_DEFAULT_SUPPORTED` (`MeterMode.h:21`): all four real
    /// modes supported = bits 1..4 set.
    const ALL_MODES: u32 = (1 << BAR_METERMODE)
        | (1 << TEXT_METERMODE)
        | (1 << GRAPH_METERMODE)
        | (1 << LED_METERMODE);

    fn mode_meter(mode: MeterModeId, supportedModes: u32) -> Meter {
        Meter {
            mode,
            supportedModes,
            ..Meter::empty()
        }
    }

    #[test]
    fn next_supported_mode_cycles_through_all_modes() {
        // With every mode supported, cycling advances 1->2->3->4 and wraps
        // 4->1 (LED back to BAR).
        assert_eq!(
            Meter_nextSupportedMode(&mode_meter(BAR_METERMODE, ALL_MODES)),
            TEXT_METERMODE
        );
        assert_eq!(
            Meter_nextSupportedMode(&mode_meter(TEXT_METERMODE, ALL_MODES)),
            GRAPH_METERMODE
        );
        assert_eq!(
            Meter_nextSupportedMode(&mode_meter(GRAPH_METERMODE, ALL_MODES)),
            LED_METERMODE
        );
        // highest mode wraps to the lowest supported mode
        assert_eq!(
            Meter_nextSupportedMode(&mode_meter(LED_METERMODE, ALL_MODES)),
            BAR_METERMODE
        );
    }

    #[test]
    fn next_supported_mode_skips_unsupported_modes() {
        // Only BAR and LED supported: BAR -> LED (skips TEXT/GRAPH),
        // LED wraps back to BAR.
        let supported = (1 << BAR_METERMODE) | (1 << LED_METERMODE);
        assert_eq!(
            Meter_nextSupportedMode(&mode_meter(BAR_METERMODE, supported)),
            LED_METERMODE
        );
        assert_eq!(
            Meter_nextSupportedMode(&mode_meter(LED_METERMODE, supported)),
            BAR_METERMODE
        );
    }

    #[test]
    fn next_supported_mode_single_mode_stays_put() {
        // Only TEXT supported: the mask above TEXT is empty, so it falls
        // back to the full set and returns TEXT again.
        let supported = 1 << TEXT_METERMODE;
        assert_eq!(
            Meter_nextSupportedMode(&mode_meter(TEXT_METERMODE, supported)),
            TEXT_METERMODE
        );
    }

    #[test]
    fn next_supported_mode_from_lower_than_all_supported() {
        // mode below the lowest supported bit: BAR (1) current, but only
        // GRAPH and LED supported -> next is GRAPH.
        let supported = (1 << GRAPH_METERMODE) | (1 << LED_METERMODE);
        assert_eq!(
            Meter_nextSupportedMode(&mode_meter(BAR_METERMODE, supported)),
            GRAPH_METERMODE
        );
    }

    // ── printed-output helper (crossterm sink) ────────────────────────
    //
    // The renderers emit crossterm escape sequences into a `Vec<u8>` sink;
    // the tests assert on the *printed characters* that survive in the byte
    // stream (the observable glyph payload), not the exact escape encoding.

    /// The printable (non-escape, non-NUL) characters emitted into a
    /// crossterm sink, in order. Strips CSI escape sequences and the
    /// `RichString` terminator-cell NUL that a blit may emit past the end.
    fn printed_chars(buf: &[u8]) -> String {
        let s = String::from_utf8(buf.to_vec()).unwrap();
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                let intro = chars.next();
                if intro != Some('[') {
                    continue;
                }
                for e in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&e) {
                        break;
                    }
                }
            } else if c != '\0' {
                out.push(c);
            }
        }
        out
    }

    /// The visible characters of a `RichString`'s valid `[0, chlen)` range.
    fn rich_text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    // ── Meter_displayBuffer ───────────────────────────────────────────

    #[test]
    fn display_buffer_else_branch_writes_txt_buffer() {
        // No display slot: writes txtBuffer in CRT_colors[attributes[0]].
        static ATTRS: [i32; 1] = [ColorElements::METER_VALUE as i32];
        let m = Meter {
            txtBuffer: "hi".to_string(),
            attributes: &ATTRS,
            ..Meter::empty()
        };
        let mut out = RichString::new();
        Meter_displayBuffer(&m, &mut out);
        assert_eq!(rich_text(&out), "hi");
        // colored with the resolved attribute for METER_VALUE.
        let expect = CRT_colorSchemes[ColorScheme::active() as usize]
            [ColorElements::METER_VALUE as usize];
        assert_eq!(out.chptr[0].attr, expect & 0xffffff);
    }

    #[test]
    fn display_buffer_dispatches_to_display_slot() {
        // A class display slot overrides the else branch.
        fn disp(_this: &Meter, out: &mut RichString) {
            RichString_appendAscii(out, 0, b"XY");
        }
        static ATTRS: [i32; 1] = [ColorElements::METER_VALUE as i32];
        let m = Meter {
            txtBuffer: "ignored".to_string(),
            attributes: &ATTRS,
            display: Some(disp),
            ..Meter::empty()
        };
        let mut out = RichString::new();
        Meter_displayBuffer(&m, &mut out);
        assert_eq!(rich_text(&out), "XY");
    }

    // ── TextMeterMode_draw ────────────────────────────────────────────

    #[test]
    fn text_meter_draw_caption_then_value() {
        static ATTRS: [i32; 1] = [ColorElements::METER_VALUE as i32];
        let mut m = Meter {
            caption: "CPU".to_string(),
            txtBuffer: "50%".to_string(),
            attributes: &ATTRS,
            ..Meter::empty()
        };
        let mut buf: Vec<u8> = Vec::new();
        TextMeterMode_draw(&mut buf, &mut m, 0, 0, 20);
        // caption printed first, then the value text.
        assert!(printed_chars(&buf).starts_with("CPU50%"));
    }

    #[test]
    fn text_meter_draw_zero_width_prints_nothing() {
        let mut m = Meter {
            caption: "CPU".to_string(),
            txtBuffer: "50%".to_string(),
            ..Meter::empty()
        };
        let mut buf: Vec<u8> = Vec::new();
        TextMeterMode_draw(&mut buf, &mut m, 0, 0, 0);
        // w == 0 <= captionWidth(0) -> early return, no glyphs.
        assert_eq!(printed_chars(&buf), "");
    }

    #[test]
    fn text_meter_draw_caption_only_when_width_equals_caption() {
        static ATTRS: [i32; 1] = [ColorElements::METER_VALUE as i32];
        let mut m = Meter {
            caption: "CPU".to_string(),
            txtBuffer: "50%".to_string(),
            attributes: &ATTRS,
            ..Meter::empty()
        };
        let mut buf: Vec<u8> = Vec::new();
        // w == 3 == captionWidth -> caption drawn, value skipped.
        TextMeterMode_draw(&mut buf, &mut m, 0, 0, 3);
        assert_eq!(printed_chars(&buf), "CPU");
    }

    // ── BarMeterMode_draw ─────────────────────────────────────────────

    #[test]
    fn bar_meter_draw_borders_fill_and_text() {
        static ATTRS: [i32; 1] = [ColorElements::CPU_NORMAL as i32];
        let mut m = Meter {
            caption: "CPU".to_string(),
            txtBuffer: "50%".to_string(),
            values: vec![50.0],
            curItems: 1,
            total: 100.0,
            isPercentChart: true,
            attributes: &ATTRS,
            ..Meter::empty()
        };
        let mut buf: Vec<u8> = Vec::new();
        BarMeterMode_draw(&mut buf, &mut m, 0, 0, 20);
        let printed = printed_chars(&buf);
        // caption, both brackets, a run of fill glyphs, and the value text.
        assert!(printed.contains("CPU"), "printed: {printed:?}");
        assert!(printed.contains('['), "printed: {printed:?}");
        assert!(printed.contains(']'), "printed: {printed:?}");
        assert!(printed.contains('|'), "printed: {printed:?}");
        assert!(printed.contains("50%"), "printed: {printed:?}");
    }

    #[test]
    fn bar_meter_draw_percent_chart_keeps_total() {
        // isPercentChart: total is NOT auto-grown from the sum.
        static ATTRS: [i32; 1] = [ColorElements::CPU_NORMAL as i32];
        let mut m = Meter {
            caption: "CPU".to_string(),
            txtBuffer: "".to_string(),
            values: vec![50.0],
            curItems: 1,
            total: 100.0,
            isPercentChart: true,
            attributes: &ATTRS,
            ..Meter::empty()
        };
        let mut buf: Vec<u8> = Vec::new();
        BarMeterMode_draw(&mut buf, &mut m, 0, 0, 20);
        assert_eq!(m.total, 100.0);
    }

    #[test]
    fn bar_meter_draw_non_percent_grows_total() {
        // Non-percent chart with sum > total: total grows to the sum.
        static ATTRS: [i32; 2] = [ColorElements::CPU_NORMAL as i32, ColorElements::CPU_SYSTEM as i32];
        let mut m = Meter {
            caption: "IO ".to_string(),
            txtBuffer: "".to_string(),
            values: vec![120.0, 30.0],
            curItems: 2,
            total: 100.0,
            isPercentChart: false,
            attributes: &ATTRS,
            ..Meter::empty()
        };
        let mut buf: Vec<u8> = Vec::new();
        BarMeterMode_draw(&mut buf, &mut m, 0, 0, 20);
        // sum = 150 > 100 -> total becomes 150.
        assert_eq!(m.total, 150.0);
    }

    #[test]
    fn bar_meter_draw_curattributes_override() {
        // curAttributes, when set, take priority over class attributes.
        static CLASS_ATTRS: [i32; 1] = [ColorElements::CPU_NORMAL as i32];
        static CUR_ATTRS: [i32; 1] = [ColorElements::CPU_SYSTEM as i32];
        let mut m = Meter {
            caption: "CPU".to_string(),
            txtBuffer: "".to_string(),
            values: vec![100.0],
            curItems: 1,
            total: 100.0,
            isPercentChart: true,
            attributes: &CLASS_ATTRS,
            curAttributes: Some(&CUR_ATTRS),
            ..Meter::empty()
        };
        let mut buf: Vec<u8> = Vec::new();
        // Full-width fill (value==total) — exercises the setAttrn/print path
        // with the override without panicking.
        BarMeterMode_draw(&mut buf, &mut m, 0, 0, 20);
        assert!(printed_chars(&buf).contains('|'));
    }

    // ── Meter_setMode ─────────────────────────────────────────────────

    #[test]
    fn set_mode_assigns_height_from_table() {
        let mut m = Meter {
            mode: BAR_METERMODE,
            supportedModes: ALL_MODES,
            ..Meter::empty()
        };
        Meter_setMode(&mut m, TEXT_METERMODE);
        assert_eq!(m.mode, TEXT_METERMODE);
        assert_eq!(m.h, 1);
        assert!(m.draw.is_some());

        Meter_setMode(&mut m, GRAPH_METERMODE);
        assert_eq!(m.mode, GRAPH_METERMODE);
        assert_eq!(m.h, DEFAULT_GRAPH_HEIGHT);

        Meter_setMode(&mut m, LED_METERMODE);
        assert_eq!(m.mode, LED_METERMODE);
        assert_eq!(m.h, 3);
    }

    #[test]
    fn set_mode_resets_draw_data() {
        let mut m = Meter {
            mode: BAR_METERMODE,
            supportedModes: ALL_MODES,
            ..Meter::empty()
        };
        m.drawData.values = vec![1.0, 2.0, 3.0];
        m.drawData.nValues = 3;
        Meter_setMode(&mut m, TEXT_METERMODE);
        assert!(m.drawData.values.is_empty());
        assert_eq!(m.drawData.nValues, 0);
    }

    #[test]
    fn set_mode_same_mode_is_noop() {
        let mut m = Meter {
            mode: BAR_METERMODE,
            supportedModes: ALL_MODES,
            h: 42, // sentinel: unchanged when mode doesn't switch
            ..Meter::empty()
        };
        Meter_setMode(&mut m, BAR_METERMODE);
        assert_eq!(m.mode, BAR_METERMODE);
        assert_eq!(m.h, 42);
    }

    #[test]
    fn set_mode_unsupported_mode_is_rejected() {
        // Only TEXT supported: a switch to GRAPH is ignored.
        let mut m = Meter {
            mode: TEXT_METERMODE,
            supportedModes: 1 << TEXT_METERMODE,
            h: 7,
            ..Meter::empty()
        };
        Meter_setMode(&mut m, GRAPH_METERMODE);
        assert_eq!(m.mode, TEXT_METERMODE);
        assert_eq!(m.h, 7);
    }

    #[test]
    fn set_mode_out_of_range_is_rejected() {
        let mut m = Meter {
            mode: BAR_METERMODE,
            supportedModes: ALL_MODES,
            h: 9,
            ..Meter::empty()
        };
        Meter_setMode(&mut m, LAST_METERMODE);
        assert_eq!(m.mode, BAR_METERMODE);
        assert_eq!(m.h, 9);
    }

    #[test]
    fn set_mode_uses_class_update_mode_branch() {
        // When the class sets an updateMode slot, the instance draw pointer
        // comes from classDraw and updateMode runs (drawData untouched).
        fn upd(this: &mut Meter, mode: MeterModeId) {
            // record that it ran by stashing the mode into h.
            this.h = 100 + mode as i32;
        }
        let mut m = Meter {
            mode: BAR_METERMODE,
            supportedModes: ALL_MODES,
            updateMode: Some(upd),
            classDraw: Some(BarMeterMode_draw),
            ..Meter::empty()
        };
        m.drawData.nValues = 5;
        Meter_setMode(&mut m, TEXT_METERMODE);
        assert_eq!(m.mode, TEXT_METERMODE);
        assert_eq!(m.h, 100 + TEXT_METERMODE as i32);
        assert!(m.draw.is_some());
        // updateMode branch does not reset drawData.
        assert_eq!(m.drawData.nValues, 5);
    }

    // ── BlankMeter hooks ──────────────────────────────────────────────

    #[test]
    fn blank_meter_update_values_clears_text() {
        let mut m = Meter {
            txtBuffer: "stale".to_string(),
            ..Meter::empty()
        };
        BlankMeter_updateValues(&mut m);
        assert_eq!(m.txtBuffer, "");
    }

    #[test]
    fn blank_meter_display_is_noop() {
        let m = Meter::empty();
        let mut out = RichString::new();
        BlankMeter_display(&m, &mut out);
        assert_eq!(out.chlen, 0);
    }

    #[test]
    fn blank_meter_class_wires_blank_hooks() {
        assert_eq!(BlankMeter_class.defaultMode, TEXT_METERMODE);
        assert_eq!(BlankMeter_class.supportedModes, 1 << TEXT_METERMODE);
        assert_eq!(BlankMeter_class.name, "Blank");
        assert!(BlankMeter_class.updateValues.is_some());
        assert!(BlankMeter_class.display.is_some());
        assert_eq!(
            BlankMeter_class.attributes,
            &[ColorElements::DEFAULT_COLOR as i32]
        );
    }

    // ── Meter_toListItem ──────────────────────────────────────────────

    #[test]
    fn to_list_item_reserved_mode_has_no_suffix() {
        // mode == 0 (reserved): label is just the uiName, no " [..]" suffix.
        let m = Meter {
            uiName: "CPU",
            mode: 0,
            ..Meter::empty()
        };
        let li = Meter_toListItem(&m, false);
        assert_eq!(li.value, "CPU");
        assert_eq!(li.key, 0);
        assert!(!li.moving);
    }

    #[test]
    fn to_list_item_real_mode_appends_mode_uiname() {
        // mode > 0: suffix " [<Meter_modes[mode].uiName>]" is appended.
        let bar = Meter {
            uiName: "CPU",
            mode: BAR_METERMODE,
            ..Meter::empty()
        };
        assert_eq!(Meter_toListItem(&bar, false).value, "CPU [Bar]");

        let text = Meter {
            uiName: "Memory",
            mode: TEXT_METERMODE,
            ..Meter::empty()
        };
        assert_eq!(Meter_toListItem(&text, false).value, "Memory [Text]");

        let graph = Meter {
            uiName: "Swap",
            mode: GRAPH_METERMODE,
            ..Meter::empty()
        };
        assert_eq!(Meter_toListItem(&graph, false).value, "Swap [Graph]");
    }

    #[test]
    fn to_list_item_propagates_moving_flag() {
        // The caller's `moving` bool is copied onto the returned item.
        let m = Meter {
            uiName: "CPU",
            mode: 0,
            ..Meter::empty()
        };
        assert!(Meter_toListItem(&m, true).moving);
        assert!(!Meter_toListItem(&m, false).moving);
    }

    #[test]
    fn to_list_item_prefers_getuiname_slot_over_uiname() {
        // When the class sets a getUiName slot, it supplies the name and the
        // static uiName is ignored (C `Meter_getUiNameFn(this)` branch).
        fn dyn_name(_this: &Meter) -> String {
            "CPU average".to_string()
        }
        let m = Meter {
            uiName: "STATIC-IGNORED",
            getUiName: Some(dyn_name),
            mode: BAR_METERMODE,
            ..Meter::empty()
        };
        assert_eq!(Meter_toListItem(&m, false).value, "CPU average [Bar]");
    }

    #[test]
    fn to_list_item_getuiname_passes_through_full_name() {
        // The C `xSnprintf` fixed-buffer size bounds are dropped (owned
        // Strings, per this module's convention), so a long getUiName result
        // is carried through in full rather than clipped at name[32].
        fn long_name(_this: &Meter) -> String {
            "a".repeat(40)
        }
        let m = Meter {
            getUiName: Some(long_name),
            mode: 0,
            ..Meter::empty()
        };
        assert_eq!(Meter_toListItem(&m, false).value, "a".repeat(40));
    }

    // ── Object subclassing ────────────────────────────────────────────

    #[test]
    fn meter_klass_is_rooted_at_object() {
        // A Meter's class identity is Meter_class.super_, whose extends chain
        // walks up to Object_class (C Meter_class.super = { .extends =
        // Class(Object) }).
        let m = Meter::empty();
        let k = m.klass();
        assert!(core::ptr::eq(k, &Meter_class.super_));
        assert!(crate::ported::object::Object_isA(
            Some(&m as &dyn Object),
            &Meter_class.super_
        ));
        assert!(crate::ported::object::Object_isA(
            Some(&m as &dyn Object),
            &Object_class
        ));
    }

    #[test]
    fn meter_display_dispatches_through_object_trait() {
        // Object::display routes to the instance display slot (BlankMeter_display
        // here — a no-op), not a panic.
        let m = Meter {
            display: Some(BlankMeter_display),
            ..Meter::empty()
        };
        let mut out = RichString::new();
        Object::display(&m, &mut out);
        assert_eq!(out.chlen, 0);
    }

    #[test]
    fn meter_roundtrips_through_ported_vector_as_object() {
        // The unblocking invariant: a Meter boxes as Box<dyn Object> and lives
        // in a ported Vector wherever C stores an Object*. Round-trip: add,
        // get, downcast back to the concrete Meter, read a mirrored field.
        use crate::ported::vector::{Vector_add, Vector_get, Vector_new, Vector_size};

        let mut v = Vector_new(&Meter_class.super_, true, 10);

        let m = Meter {
            caption: "CPU".to_string(),
            param: 7,
            ..Meter::empty()
        };
        Vector_add(&mut v, Box::new(m));
        assert_eq!(Vector_size(&v), 1);

        let got: &dyn Object = Vector_get(&v, 0);
        // klass identity survives the Object* round-trip.
        assert!(core::ptr::eq(got.klass(), &Meter_class.super_));
        // downcast the trait object back to the concrete Meter (C casts the
        // Object* back to Meter*).
        let any: &dyn core::any::Any = got;
        let back = any
            .downcast_ref::<Meter>()
            .expect("stored object downcasts back to Meter");
        assert_eq!(back.caption, "CPU");
        assert_eq!(back.param, 7);
    }

    #[test]
    fn meter_modes_table_matches_c() {
        // Index 0 reserved; real modes carry their C ui-name + height.
        assert!(Meter_modes[0].draw.is_none());
        assert_eq!(Meter_modes[BAR_METERMODE as usize].uiName, Some("Bar"));
        assert_eq!(Meter_modes[BAR_METERMODE as usize].h, 1);
        assert_eq!(Meter_modes[TEXT_METERMODE as usize].uiName, Some("Text"));
        assert_eq!(Meter_modes[TEXT_METERMODE as usize].h, 1);
        assert_eq!(Meter_modes[GRAPH_METERMODE as usize].uiName, Some("Graph"));
        assert_eq!(Meter_modes[GRAPH_METERMODE as usize].h, DEFAULT_GRAPH_HEIGHT);
        assert_eq!(Meter_modes[LED_METERMODE as usize].uiName, Some("LED"));
        assert_eq!(Meter_modes[LED_METERMODE as usize].h, 3);
    }
}
