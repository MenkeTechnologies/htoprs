//! Partial port of `Header.c` — htop's header (the columns of meters
//! drawn across the top of the screen).
//!
//! The functions ported below are the ones whose behavior is faithfully
//! reproducible without the parts of htop that are not modeled yet:
//!
//! * `Header_setLayout` — resizes the column vector and migrates meters
//!   from removed columns into the last kept one.
//! * `calcColumnWidthCount` / `Header_calculateHeight` — the layout
//!   height/column-span arithmetic.
//! * `Header_draw` — clears the header rows and dispatches each meter's
//!   `draw` slot across its computed column span.
//! * `Header_writeBackToSettings` — copies each column's meter name/mode
//!   list back into the [`Settings`] `hColumns`.
//!
//! Everything that constructs meters through the `MeterClass` vtable
//! (`Meter_new`, `Header_addMeterByName`, `Header_addMeterByClass`,
//! `Header_populateFromSettings`),
//! pulls live values (`Header_updateData`), or needs the `Machine` host
//! allocation (`Header_new`/`Header_delete`) stays a `todo!()` stub — that
//! substrate is not ported.
//!
//! ## Modeled structs
//!
//! htop's `Header` holds `Vector** columns` where each `Vector` owns
//! `Meter*`; this ports to `Vec<Vec<Meter>>` — one inner vec per column,
//! each holding the live [`Meter`] directly
//! (its `h`, `columnWidthCount`, `isMultiColumn`, `mode`, `param`, `name`,
//! and `draw` slot back every function here). The two settings values
//! `Header_calculateHeight` reads — `headerMargin` and `screenTabs` — live
//! on [`Header`] directly (the C reads them via `this->host->settings`),
//! because the `Machine`/`Settings` substrate that carries them is not
//! modeled.
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use std::io::Write;

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::functionbar::Ncurses;
use crate::ported::machine::Machine;
use crate::ported::meter::{
    BlankMeter_class, Meter, MeterClass, Meter_new, Meter_setMode, TEXT_METERMODE,
};
use crate::ported::settings::{
    HeaderLayout, HeaderLayout_getColumns, HeaderLayout_layouts, MeterModeId, Settings,
    Settings_setHeaderLayout,
};
// Platform dispatch (darwin-first): the available-meter registry comes from
// this build's platform, mirroring htop linking one platform's `Platform.c`.
#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_meterTypes;
#[cfg(not(target_os = "macos"))]
use crate::ported::linux::platform::Platform_meterTypes;

/// Model of htop's `Header` (`Header.h:20`). The C fields `columns`,
/// `headerLayout`, `pad`, and `height` are reproduced. `Vector** columns`
/// becomes `Vec<Vec<Meter>>` (one inner vec per column). The C
/// `Machine* host` is stored as a raw `*const Machine` back-pointer (the
/// crate's pointer-graph ownership model). The two `host->settings` values
/// the ported height arithmetic reads — `headerMargin` and `screenTabs` —
/// are additionally cached on the struct: zeroed at construction (like C's
/// `xCalloc`) and refreshed when the header is populated/recalculated.
///
/// Invariant (as in C): `columns.len() == HeaderLayout_getColumns(headerLayout)`.
pub struct Header {
    /// C `struct Machine_* host` — the owning machine (borrowed).
    pub host: *const Machine,
    pub columns: Vec<Vec<Meter>>,
    pub headerLayout: HeaderLayout,
    pub pad: i32,
    pub height: i32,
    /// C `this->host->settings->headerMargin` — when set, each column is
    /// padded by 2 rows.
    pub headerMargin: bool,
    /// C `this->host->settings->screenTabs` — when set, one row is added
    /// to the total height for the screen-tabs line.
    pub screenTabs: bool,
}

/// Port of `Header* Header_new(Machine* host, HeaderLayout hLayout)` from
/// `Header.c:31`. C `xCalloc`s the `Header` (zeroing `pad`/`height` and,
/// here, the cached `headerMargin`/`screenTabs`), stores `hLayout` and the
/// `Machine* host`, then `Vector_new`s one empty meter column per
/// `HeaderLayout_getColumns(hLayout)` (`Header_forEachColumn`). The
/// `Class(Meter)` object-class tag is dropped — column element identity is
/// the Rust `HeaderMeter` type.
pub fn Header_new(host: *const Machine, hLayout: HeaderLayout) -> Header {
    let ncol = HeaderLayout_getColumns(hLayout);
    let mut columns = Vec::with_capacity(ncol);
    for _ in 0..ncol {
        columns.push(Vec::new());
    }

    Header {
        host,
        columns,
        headerLayout: hLayout,
        pad: 0,
        height: 0,
        headerMargin: false,
        screenTabs: false,
    }
}

/// Port of `void Header_delete(Header* this)` from `Header.c:44`.
///
/// C `Vector_delete`s each column, then `free(this->columns)` and
/// `free(this)`. The [`Header`] owns its `Vec<Vec<Meter>>` columns and every
/// [`Meter`] in them; taking `this` by value so it drops at end of scope IS
/// that free chain (the same by-value idiom `FunctionBar_delete` uses for its
/// `free(this)`). Nothing the header owned outlives the call.
pub fn Header_delete(this: Header) {
    let _ = this;
}

/// Port of `Header.c:53`.
///
/// Sets the new layout, then reconciles the column vector to the new
/// column count. Growing appends fresh empty columns (C `Vector_new`).
/// Shrinking migrates the meters of each removed column into the last kept
/// column, in reverse index order — the C loop takes element `j` from
/// `items-1` down to `0` (`Vector_take`) and `Vector_add`s it to
/// `columns[newColumns-1]`, which is exactly `pop()` then `push()`. Then
/// recomputes the height.
pub fn Header_setLayout(this: &mut Header, hLayout: HeaderLayout) {
    let oldColumns = HeaderLayout_getColumns(this.headerLayout);
    let newColumns = HeaderLayout_getColumns(hLayout);

    this.headerLayout = hLayout;

    if newColumns == oldColumns {
        return;
    }

    if newColumns > oldColumns {
        for _ in oldColumns..newColumns {
            this.columns.push(Vec::new());
        }
    } else {
        // move meters from to-be-deleted columns into last one
        for i in newColumns..oldColumns {
            let mut removed = std::mem::take(&mut this.columns[i]);
            while let Some(meter) = removed.pop() {
                this.columns[newColumns - 1].push(meter);
            }
        }
        this.columns.truncate(newColumns);
    }

    Header_calculateHeight(this);
}

/// Port of `static void Header_addMeterByName(Header* this, const char*
/// name, MeterModeId mode, size_t column)` from `Header.c:80`.
///
/// Splits an optional `(param)` suffix off the serialized `name`, then finds
/// the matching class in [`Platform_meterTypes`] (`type.name` equals the base
/// name exactly, C `strncmp(...) == 0 && type->name[nameLen] == '\0'`),
/// constructs it with [`Meter_new`], applies `mode` (when non-zero) via
/// [`Meter_setMode`], and appends it to `column`. Unknown names add nothing
/// (the C loop simply falls through).
///
/// Suffix parsing mirrors the C: `sscanf(paren, "(%10u)", &param)` — a
/// leading unsigned int (≤10 digits) is the `CPUMeter`/single-meter param.
/// The `DynamicMeter` branch (a non-numeric `(name)`) needs
/// `DynamicMeter_search(settings->dynamicMeters, ...)`; `Settings` models no
/// `dynamicMeters`, so — exactly as the C `return`s on lookup failure — a
/// non-numeric suffix adds no meter.
pub fn Header_addMeterByName(this: &mut Header, name: &str, mode: MeterModeId, column: usize) {
    debug_assert!(column < HeaderLayout_getColumns(this.headerLayout));

    let mut param: u32 = 0;
    let nameLen = if let Some(parenPos) = name.find('(') {
        // C: sscanf(paren, "(%10u)", &param) — up to 10 leading digits after
        // '('. A non-empty digit run is the numeric param; anything else is
        // the (unmodeled) DynamicMeter path, which the C abandons.
        let digits: String = name[parenPos + 1..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .take(10)
            .collect();
        if digits.is_empty() {
            return; // DynamicMeter lookup — see doc; adds nothing
        }
        param = digits.parse().unwrap_or(0);
        parenPos
    } else {
        name.len()
    };

    let baseName = &name[..nameLen];
    for &type_ in Platform_meterTypes.iter() {
        if type_.name == baseName {
            let mut meter = Meter_new(this.host, param, type_);
            if mode != 0 {
                Meter_setMode(&mut meter, mode);
            }
            this.columns[column].push(meter);
            break;
        }
    }
}

/// Port of `void Header_populateFromSettings(Header* this)` from
/// `Header.c:120`.
///
/// Applies `settings.hLayout` via [`Header_setLayout`], then for each column
/// prunes it (C `Vector_prune` → [`Vec::clear`]) and re-adds every meter
/// named in `settings.hColumns[col]` through [`Header_addMeterByName`], and
/// finally recomputes the height ([`Header_calculateHeight`]).
///
/// C reads `settings` via `this->host->settings`; here it is passed
/// explicitly (the same deviation as [`Header_writeBackToSettings`], because
/// the host→settings substrate is not the modeled path). The two cached
/// layout inputs [`Header::headerMargin`]/[`Header::screenTabs`] — which the
/// C `Header_calculateHeight` reads live from `host->settings` — are
/// refreshed here from `settings` so the recomputed height matches.
pub fn Header_populateFromSettings(this: &mut Header, settings: &Settings) {
    Header_setLayout(this, settings.hLayout);
    this.headerMargin = settings.headerMargin;
    this.screenTabs = settings.screenTabs;

    let numColumns = HeaderLayout_getColumns(this.headerLayout);
    for col in 0..numColumns {
        this.columns[col].clear();
        let colSettings = &settings.hColumns[col];
        for i in 0..colSettings.len {
            // Present only when len != 0 (see Header_writeBackToSettings).
            let name = colSettings.names.as_ref().unwrap()[i].clone();
            let mode = colSettings.modes.as_ref().unwrap()[i];
            Header_addMeterByName(this, &name, mode, col);
        }
    }

    Header_calculateHeight(this);
}

/// Port of `Header.c:135`.
///
/// Writes each column's meter list back into `settings->hColumns`: sets
/// the header layout (which resizes `hColumns`), then for every column
/// copies the meters' `name`/`mode` into the column's `names`/`modes` and
/// records the count in `len`. An empty column stores `None`
/// names/modes (C `NULL`) with `len == 0`, matching the C
/// `len ? xCalloc(...) : NULL`.
///
/// The C reconstructs each serialized name at write time (`Header.c:163`):
/// `"%s(%u)"` for a `CPUMeter` with a `param`, `"%s(%s)"` for a
/// `DynamicMeter` (via `DynamicMeter_lookup(settings->dynamicMeters,
/// param)`), else plain `As_Meter(meter)->name`. The class-identity test
/// `As_Meter(meter) == &CPUMeter_class` collapses to `meter.name == "CPU"`
/// (`CPUMeter_class.name`, the unique class with that name). The
/// `DynamicMeter` branch is elided: that meter type cannot be constructed
/// here (`Header_addMeterByName` is a stub) and its lookup substrate
/// (`Settings.dynamicMeters` / `DynamicMeter_lookup`) is not modeled.
pub fn Header_writeBackToSettings(this: &Header, settings: &mut Settings) {
    Settings_setHeaderLayout(settings, this.headerLayout);

    let numColumns = HeaderLayout_getColumns(this.headerLayout);
    for col in 0..numColumns {
        let vec = &this.columns[col];
        let len = vec.len();

        let colSettings = &mut settings.hColumns[col];
        if len != 0 {
            colSettings.names = Some(
                vec.iter()
                    .map(|meter| {
                        if meter.param != 0 && meter.name == "CPU" {
                            format!("{}({})", meter.name, meter.param)
                        } else {
                            meter.name.to_string()
                        }
                    })
                    .collect(),
            );
            colSettings.modes = Some(vec.iter().map(|m| m.mode).collect());
        } else {
            colSettings.names = None;
            colSettings.modes = None;
        }
        colSettings.len = len;
    }
}

/// Port of `Meter* Header_addMeterByClass(Header* this, const MeterClass*
/// type, unsigned int param, size_t column)` from `Header.c:173`.
///
/// Constructs a meter of `type` with `Meter_new(this->host, param, type)`
/// and appends it to `column`'s vector (C `Vector_add`), returning a
/// mutable handle to the freshly-added meter (C returns the `Meter*`, used
/// by the caller to `Meter_setMode`). The `assert(column < numColumns)`
/// precondition is honored — an out-of-range column panics via the `Vec`
/// index.
pub fn Header_addMeterByClass<'a>(
    this: &'a mut Header,
    type_: &'static MeterClass,
    param: u32,
    column: usize,
) -> &'a mut Meter {
    debug_assert!(column < HeaderLayout_getColumns(this.headerLayout));

    let meter = Meter_new(this.host, param, type_);
    this.columns[column].push(meter);
    this.columns[column].last_mut().unwrap()
}

/// Port of `void Header_reinit(Header* this)` from `Header.c:183`.
///
/// For every meter in every column runs `if (Meter_initFn(meter))
/// Meter_init(meter)` — dispatch through the class `init` slot
/// (`As_Meter(meter)->init`) when it is non-`NULL`. The `init` slot is
/// mirrored onto the [`Meter`] instance as the `init` field (seeded from the
/// class in [`Meter_new`], like `updateValues`), so `if let Some(init)` is the
/// faithful `if (Meter_initFn(meter))` guard and `init(meter)` the
/// `Meter_init(meter)` call.
pub fn Header_reinit(this: &mut Header) {
    let numColumns = HeaderLayout_getColumns(this.headerLayout);
    for col in 0..numColumns {
        let items = this.columns[col].len();
        for i in 0..items {
            if let Some(init) = this.columns[col][i].init {
                init(&mut this.columns[col][i]);
            }
        }
    }
}

/// Port of `void Header_draw(const Header* this)` from `Header.c:194`.
///
/// Clears every header row to blanks (`attrset(RESET_COLOR)` + `mvhline`
/// across `COLS`), then lays the columns out left-to-right: each column's
/// pixel width is `width * HeaderLayout_layouts[layout].widths[col] / 100`,
/// with fractional remainders accumulated in `roundingLoss` and folded back
/// in a whole column at a time (so the columns tile `width` exactly). Within
/// a column each meter is drawn at `(x, y)` via its `draw` slot and `y`
/// advances by `meter.h`. A text-mode, non-multi-column meter is allowed to
/// expand rightward across `columnWidthCount - 1` empty neighbor columns
/// (each contributes a separator column plus that neighbor's share of
/// `width`). The `assert(meter->draw)` is honored by the `Option::expect`.
///
/// The C reads/writes the ncurses global cursor; here the terminal side
/// effects go through the crossterm `Ncurses` shim into `out` (the
/// `Panel_draw`/`Meter`-draw precedent). `COLS` is `Ncurses::cols`;
/// `floorf` is [`f32::floor`]. `this` is `&mut` because the per-meter
/// `draw` slot takes `&mut Meter` (it updates `drawData`).
pub fn Header_draw(this: &mut Header, mut out: &mut dyn Write) {
    let scheme = ColorScheme::active();
    let height = this.height;
    let pad = this.pad;
    let cols = Ncurses::cols();

    Ncurses::attrset(&mut out, ColorElements::RESET_COLOR.packed(scheme));
    for y in 0..height {
        Ncurses::mvhline(&mut out, y, 0, ' ', cols);
    }

    let numCols = HeaderLayout_getColumns(this.headerLayout);
    let width = cols - 2 * pad - (numCols as i32 - 1);
    let widths = HeaderLayout_layouts[this.headerLayout as usize].widths;
    let mut x = pad;
    let mut roundingLoss = 0.0f32;

    for col in 0..numCols {
        let mut colWidth = width as f32 * widths[col] as f32 / 100.0;

        roundingLoss += colWidth - colWidth.floor();
        if roundingLoss >= 1.0 {
            colWidth += 1.0;
            roundingLoss -= 1.0;
        }

        let mut y = pad / 2;
        let colLen = this.columns[col].len();
        for i in 0..colLen {
            // Read the scalar fields the layout math needs before taking the
            // `&mut` borrow that `draw` requires (no aliasing).
            let (mode, columnWidthCount, isMultiColumn, h, draw) = {
                let meter = &this.columns[col][i];
                (
                    meter.mode,
                    meter.columnWidthCount,
                    meter.isMultiColumn,
                    meter.h,
                    meter.draw,
                )
            };

            let mut actualWidth = colWidth;

            // Let meters in text mode expand to the right on empty neighbors;
            // except for multi column meters.
            if mode == TEXT_METERMODE && !isMultiColumn {
                for j in 1..columnWidthCount {
                    actualWidth += 1.0; // separator column
                    actualWidth += width as f32 * widths[col + j as usize] as f32 / 100.0;
                }
            }

            // C `assert(meter->draw)`.
            let draw = draw.expect("Header_draw: meter->draw is NULL");
            draw(
                &mut *out,
                &mut this.columns[col][i],
                x,
                y,
                actualWidth.floor() as i32,
            );
            y += h;
        }

        x += colWidth.floor() as i32;
        x += 1; // separator column
    }
}

/// Port of `void Header_updateData(Header* this)` from `Header.c:240`.
///
/// Calls `Meter_updateValues(meter)` on every meter in every column. The C
/// `Meter_updateValues(this_)` macro (`Meter.h:94`) expands to
/// `As_Meter(this_)->updateValues(this_)` — the class value-update slot,
/// mirrored onto the [`Meter`] instance as `updateValues` and dispatched
/// here. The slot is non-`NULL` for every real meter class (the C macro has
/// no guard and would fault otherwise), so `Option::expect` mirrors that.
pub fn Header_updateData(this: &mut Header) {
    let numColumns = HeaderLayout_getColumns(this.headerLayout);
    for col in 0..numColumns {
        let items = this.columns[col].len();
        for i in 0..items {
            let updateValues = this.columns[col][i]
                .updateValues
                .expect("Meter_updateValues: updateValues slot is NULL");
            updateValues(&mut this.columns[col][i]);
        }
    }
}

/// Port of `Header.c:256`.
///
/// Counts how many columns the meter at (`curColumn`, `curHeight`) may
/// span, by walking the columns to its right and stopping at the first one
/// that has a non-`BlankMeter` meter overlapping the current meter's
/// vertical band `[curHeight, curHeight + curMeter.h)`. For each right
/// column it accumulates `height` from `pad`, skipping meters entirely
/// above `curHeight` (`height <= curHeight` → continue) and stopping once
/// past the band (`height >= curHeight + curMeter.h` → break). Returns the
/// column distance to the first blocking column, or the full remaining
/// span if none blocks.
fn calcColumnWidthCount(
    this: &Header,
    curMeter: &Meter,
    pad: i32,
    curColumn: usize,
    curHeight: i32,
) -> i32 {
    let numColumns = HeaderLayout_getColumns(this.headerLayout);
    for i in (curColumn + 1)..numColumns {
        let meters = &this.columns[i];

        let mut height = pad;
        for j in 0..meters.len() {
            let meter = &meters[j];

            if height >= curHeight + curMeter.h {
                break;
            }

            height += meter.h;
            if height <= curHeight {
                continue;
            }

            // C: `!Object_isA(meter, &BlankMeter_class)`. The Rust `Meter` is
            // a single struct whose class identity is carried by `name`
            // (mirrored from its `MeterClass`), so the "is a BlankMeter" test
            // collapses to a name comparison against `BlankMeter_class.name`.
            if meter.name != BlankMeter_class.name {
                return (i - curColumn) as i32;
            }
        }
    }

    (numColumns - curColumn) as i32
}

/// Port of `Header.c:279`.
///
/// Computes and stores the header's height. `pad` is 2 when
/// `headerMargin` is set, else 0. For each column it sums the meter
/// heights (starting from `pad`) and, as a side effect, records each
/// meter's `columnWidthCount` via `calcColumnWidthCount`. The tallest
/// column wins. If no column has any meters (`maxHeight == pad`) the
/// header collapses to height 0 and `pad` 0; otherwise `pad` is kept. When
/// `screenTabs` is set one extra row is added. Returns the final height.
pub fn Header_calculateHeight(this: &mut Header) -> i32 {
    let pad = if this.headerMargin { 2 } else { 0 };
    let mut maxHeight = pad;

    let numColumns = HeaderLayout_getColumns(this.headerLayout);
    for col in 0..numColumns {
        let mut height = pad;
        let colLen = this.columns[col].len();
        for i in 0..colLen {
            let columnWidthCount = {
                let meter = &this.columns[col][i];
                calcColumnWidthCount(this, meter, pad, col, height)
            };
            this.columns[col][i].columnWidthCount = columnWidthCount;
            height += this.columns[col][i].h;
        }
        maxHeight = maxHeight.max(height);
    }

    if maxHeight == pad {
        maxHeight = 0;
        this.pad = 0;
    } else {
        this.pad = pad;
    }

    if this.screenTabs {
        maxHeight += 1;
    }

    this.height = maxHeight;

    maxHeight
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::settings::{MeterColumnSetting, MeterModeId};

    /// A live [`Meter`] of the given name and height, mode fixed at 1. Uses
    /// [`Meter::empty`] for the vtable-free fields the header math ignores.
    fn meter(name: &'static str, h: i32) -> Meter {
        Meter {
            name,
            mode: 1,
            h,
            ..Meter::empty()
        }
    }

    /// A meter whose `name` matches `BlankMeter_class.name` — the header
    /// blank test keys on the name (see [`calcColumnWidthCount`]).
    fn blank(h: i32) -> Meter {
        Meter {
            name: BlankMeter_class.name,
            mode: 1,
            h,
            ..Meter::empty()
        }
    }

    /// Column meter names, for structural comparisons where the mutated
    /// `columnWidthCount` (set by `Header_calculateHeight`) is irrelevant.
    /// `Meter` is not `PartialEq` (raw pointers / fn slots), so tests compare
    /// by name.
    fn names(col: &[Meter]) -> Vec<&str> {
        col.iter().map(|m| m.name).collect()
    }

    /// A test `MeterDraw` slot (`fn`, so it cannot capture) that records its
    /// dispatch coordinates into the sink as a newline-delimited `MARK` line.
    /// crossterm's `attrset`/`mvhline` output carries no newlines, so these
    /// lines survive `str::lines()` filtering intact.
    fn record_draw(out: &mut dyn Write, m: &mut Meter, x: i32, y: i32, w: i32) {
        let _ = write!(out, "\nMARK name={} x={} y={} w={}\n", m.name, x, y, w);
    }

    /// [`meter`] with the recording [`record_draw`] slot wired in, so
    /// [`Header_draw`]'s per-meter dispatch is observable.
    fn draw_meter(name: &'static str, h: i32) -> Meter {
        Meter {
            name,
            mode: 1,
            h,
            draw: Some(record_draw),
            ..Meter::empty()
        }
    }

    /// The `MARK` lines emitted into `buf`, in dispatch order.
    fn marks(buf: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(buf)
            .lines()
            .filter(|l| l.starts_with("MARK"))
            .map(|l| l.to_string())
            .collect()
    }

    /// Build a `Header` from per-column meter lists. The layout's column
    /// count must equal `columns.len()`.
    #[test]
    fn new_allocates_one_empty_column_per_layout_column() {
        let host = 0xF00D as *const Machine;
        for hLayout in [
            HeaderLayout::HF_ONE_100,
            HeaderLayout::HF_TWO_50_50,
            HeaderLayout::HF_THREE_33_34_33,
        ] {
            let h = Header_new(host, hLayout);
            // Invariant: one empty column vector per layout column.
            assert_eq!(h.columns.len(), HeaderLayout_getColumns(hLayout));
            assert!(h.columns.iter().all(|c| c.is_empty()));
            // xCalloc-zeroed scalars + stored host/layout.
            assert_eq!(h.host, host);
            assert_eq!(h.headerLayout, hLayout);
            assert_eq!((h.pad, h.height), (0, 0));
            assert!(!h.headerMargin && !h.screenTabs);
        }
    }

    fn header(hLayout: HeaderLayout, columns: Vec<Vec<Meter>>) -> Header {
        assert_eq!(HeaderLayout_getColumns(hLayout), columns.len());
        Header {
            host: core::ptr::null(),
            columns,
            headerLayout: hLayout,
            pad: 0,
            height: 0,
            headerMargin: false,
            screenTabs: false,
        }
    }

    // ---- Header_calculateHeight -------------------------------------

    #[test]
    fn calculate_height_one_column() {
        // HF_ONE_100, meters h=[2,2], no margin, no tabs -> height 4.
        let mut h = header(
            HeaderLayout::HF_ONE_100,
            vec![vec![meter("A", 2), meter("B", 2)]],
        );
        assert_eq!(Header_calculateHeight(&mut h), 4);
        assert_eq!(h.height, 4);
        assert_eq!(h.pad, 0);
    }

    #[test]
    fn calculate_height_two_columns_takes_max() {
        // col0 sums to 4, col1 to 2 -> max 4.
        let mut h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![vec![meter("A", 2), meter("B", 2)], vec![meter("C", 2)]],
        );
        assert_eq!(Header_calculateHeight(&mut h), 4);
        assert_eq!(h.height, 4);
    }

    #[test]
    fn calculate_height_margin_adds_pad_to_each_column() {
        // pad=2: col0 = 2+2+2 = 6, col1 = 2+2 = 4 -> max 6, pad kept.
        let mut h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![vec![meter("A", 2), meter("B", 2)], vec![meter("C", 2)]],
        );
        h.headerMargin = true;
        assert_eq!(Header_calculateHeight(&mut h), 6);
        assert_eq!(h.pad, 2);
    }

    #[test]
    fn calculate_height_three_columns_varying_counts() {
        // heights: col0=2, col1=6, col2=4 -> max 6.
        let mut h = header(
            HeaderLayout::HF_THREE_33_34_33,
            vec![
                vec![meter("A", 2)],
                vec![meter("B", 2), meter("C", 2), meter("D", 2)],
                vec![meter("E", 2), meter("F", 2)],
            ],
        );
        assert_eq!(Header_calculateHeight(&mut h), 6);
    }

    #[test]
    fn calculate_height_empty_collapses_to_zero() {
        // No meters: maxHeight == pad(0) -> height 0, pad 0.
        let mut h = header(HeaderLayout::HF_TWO_50_50, vec![vec![], vec![]]);
        assert_eq!(Header_calculateHeight(&mut h), 0);
        assert_eq!(h.pad, 0);

        // Even with margin: maxHeight == pad(2) -> collapses to 0, pad 0.
        h.headerMargin = true;
        assert_eq!(Header_calculateHeight(&mut h), 0);
        assert_eq!(h.pad, 0);
    }

    #[test]
    fn calculate_height_screen_tabs_adds_one() {
        // Non-empty height 4, +1 for tabs = 5.
        let mut h = header(
            HeaderLayout::HF_ONE_100,
            vec![vec![meter("A", 2), meter("B", 2)]],
        );
        h.screenTabs = true;
        assert_eq!(Header_calculateHeight(&mut h), 5);

        // Empty + tabs: collapses to 0 then +1 = 1.
        let mut e = header(HeaderLayout::HF_ONE_100, vec![vec![]]);
        e.screenTabs = true;
        assert_eq!(Header_calculateHeight(&mut e), 1);
    }

    #[test]
    fn calculate_height_writes_column_width_count() {
        // col0 meter has an occupied neighbor in col1 -> span 1.
        // col1 meter has no column to its right -> full span (2 - 1 = 1).
        let mut h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![vec![meter("A", 2)], vec![meter("B", 2)]],
        );
        Header_calculateHeight(&mut h);
        assert_eq!(h.columns[0][0].columnWidthCount, 1);
        assert_eq!(h.columns[1][0].columnWidthCount, 1);
    }

    // ---- calcColumnWidthCount ---------------------------------------

    #[test]
    fn width_count_occupied_neighbor_is_one() {
        // col1 has a non-blank meter overlapping col0's meter band -> 1.
        let h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![vec![meter("A", 2)], vec![meter("B", 2)]],
        );
        let cur = &h.columns[0][0];
        assert_eq!(calcColumnWidthCount(&h, cur, 0, 0, 0), 1);
    }

    #[test]
    fn width_count_blank_then_occupied_spans_two() {
        // col1 blank, col2 occupied -> spans across the blank to col2 => 2.
        let h = header(
            HeaderLayout::HF_THREE_33_34_33,
            vec![vec![meter("A", 2)], vec![blank(1)], vec![meter("C", 1)]],
        );
        let cur = &h.columns[0][0];
        assert_eq!(calcColumnWidthCount(&h, cur, 0, 0, 0), 2);
    }

    #[test]
    fn width_count_all_empty_right_is_full_span() {
        // Both right columns empty -> full remaining span = numCols - curCol = 3.
        let h = header(
            HeaderLayout::HF_THREE_33_34_33,
            vec![vec![meter("A", 2)], vec![], vec![]],
        );
        let cur = &h.columns[0][0];
        assert_eq!(calcColumnWidthCount(&h, cur, 0, 0, 0), 3);
    }

    #[test]
    fn width_count_break_when_neighbor_below_band_with_pad() {
        // pad=2 so height starts at 2, and curMeter.h=1 means the band is
        // [0,1): the very first neighbor check breaks immediately
        // (2 >= 0+1), no return -> full span (2 - 0 = 2).
        let h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![vec![meter("A", 1)], vec![meter("B", 5)]],
        );
        let cur = &h.columns[0][0];
        assert_eq!(calcColumnWidthCount(&h, cur, 2, 0, 0), 2);
    }

    #[test]
    fn width_count_skips_meter_above_current_height() {
        // curHeight=3: col1's first meter (h=2) ends at height 2 <= 3, so
        // it is skipped (continue); the second meter (h=2, ends at 4 > 3)
        // is non-blank and overlaps -> return 1.
        let h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![vec![meter("A", 4)], vec![blank(2), meter("C", 2)]],
        );
        // curMeter.h large enough that band extends past height 3.
        let cur = &h.columns[0][0];
        assert_eq!(calcColumnWidthCount(&h, cur, 0, 0, 3), 1);
    }

    // ---- Header_draw ------------------------------------------------

    #[test]
    fn draw_dispatches_each_meter_at_stacked_y() {
        // Two columns: col0 has two h=2 meters (drawn at y=0, y=2), col1 has
        // one h=1 meter (y=0). `pad` is 0 so `y` starts at 0 in every column.
        // The exact x/width depend on terminal COLS, but the dispatch count,
        // order, and y-offsets are COLS-independent.
        let mut h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![
                vec![draw_meter("A", 2), draw_meter("B", 2)],
                vec![draw_meter("C", 1)],
            ],
        );
        Header_calculateHeight(&mut h); // seeds height/columnWidthCount

        let mut buf: Vec<u8> = Vec::new();
        Header_draw(&mut h, &mut buf);

        let m = marks(&buf);
        assert_eq!(m.len(), 3, "one dispatch per meter");
        // Dispatch order is column-major, top-to-bottom within a column.
        assert!(m[0].contains("name=A") && m[0].contains("y=0"));
        assert!(m[1].contains("name=B") && m[1].contains("y=2"));
        assert!(m[2].contains("name=C") && m[2].contains("y=0"));
    }

    #[test]
    fn draw_margin_starts_y_at_half_pad() {
        // headerMargin -> pad=2, so each column's first meter draws at
        // y = pad/2 = 1, the next at 1 + h.
        let mut h = header(
            HeaderLayout::HF_ONE_100,
            vec![vec![draw_meter("A", 2), draw_meter("B", 2)]],
        );
        h.headerMargin = true;
        Header_calculateHeight(&mut h);

        let mut buf: Vec<u8> = Vec::new();
        Header_draw(&mut h, &mut buf);

        let m = marks(&buf);
        assert_eq!(m.len(), 2);
        assert!(m[0].contains("name=A") && m[0].contains("y=1"));
        assert!(m[1].contains("name=B") && m[1].contains("y=3"));
    }

    #[test]
    #[should_panic(expected = "meter->draw")]
    fn draw_panics_on_missing_draw_slot() {
        // `meter()` uses `Meter::empty()` -> `draw: None`; the C
        // `assert(meter->draw)` maps to the `Option::expect` panic.
        let mut h = header(HeaderLayout::HF_ONE_100, vec![vec![meter("A", 1)]]);
        Header_calculateHeight(&mut h);
        let mut buf: Vec<u8> = Vec::new();
        Header_draw(&mut h, &mut buf);
    }

    // ---- Header_reinit ----------------------------------------------

    /// A `MeterInit` slot (`fn`, so it coerces to the fn pointer) that marks
    /// the meter as having run its init by writing `txtBuffer`.
    fn mark_init(m: &mut Meter) {
        m.txtBuffer = "inited".to_string();
    }

    #[test]
    fn reinit_runs_init_slot_when_present_and_skips_when_absent() {
        // col0's meter carries an `init` slot; col1's does not (C `NULL`).
        let mut h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![
                vec![Meter {
                    name: "A",
                    init: Some(mark_init),
                    ..Meter::empty()
                }],
                vec![Meter {
                    name: "B",
                    ..Meter::empty()
                }],
            ],
        );
        Header_reinit(&mut h);
        // C `if (Meter_initFn(meter)) Meter_init(meter)`: dispatched for A,
        // skipped for B (its slot is None).
        assert_eq!(h.columns[0][0].txtBuffer, "inited");
        assert!(h.columns[1][0].txtBuffer.is_empty());
    }

    // ---- Header_addMeterByClass / Header_updateData -----------------

    #[test]
    fn add_meter_by_class_appends_to_column_and_returns_it() {
        let mut h = header(HeaderLayout::HF_TWO_50_50, vec![vec![], vec![]]);
        let m = Header_addMeterByClass(&mut h, &BlankMeter_class, 7, 1);
        // Returned handle is the freshly-built meter (name/param seeded by
        // Meter_new).
        assert_eq!(m.name, "Blank");
        assert_eq!(m.param, 7);
        // Appended to column 1 only.
        assert_eq!(h.columns[0].len(), 0);
        assert_eq!(h.columns[1].len(), 1);
        assert_eq!(h.columns[1][0].name, "Blank");
    }

    #[test]
    fn update_data_dispatches_update_slot_per_meter() {
        // BlankMeter_updateValues clears txtBuffer; pre-dirty every meter and
        // assert the dispatch reached each one.
        let mut h = header(HeaderLayout::HF_TWO_50_50, vec![vec![], vec![]]);
        Header_addMeterByClass(&mut h, &BlankMeter_class, 0, 0);
        Header_addMeterByClass(&mut h, &BlankMeter_class, 0, 1);
        h.columns[0][0].txtBuffer = "dirty".to_string();
        h.columns[1][0].txtBuffer = "dirty".to_string();

        Header_updateData(&mut h);

        assert!(h.columns[0][0].txtBuffer.is_empty());
        assert!(h.columns[1][0].txtBuffer.is_empty());
    }

    // ---- Header_addMeterByName --------------------------------------

    #[test]
    fn add_meter_by_name_looks_up_class_and_appends() {
        let mut h = header(HeaderLayout::HF_TWO_50_50, vec![vec![], vec![]]);
        Header_addMeterByName(&mut h, "Blank", 0, 1);
        assert_eq!(h.columns[0].len(), 0);
        assert_eq!(h.columns[1].len(), 1);
        assert_eq!(h.columns[1][0].name, "Blank");
        assert_eq!(h.columns[1][0].param, 0);
    }

    #[test]
    fn add_meter_by_name_parses_numeric_param_suffix() {
        // "Blank(7)" -> base name "Blank", CPUMeter-style numeric param 7.
        let mut h = header(HeaderLayout::HF_ONE_100, vec![vec![]]);
        Header_addMeterByName(&mut h, "Blank(7)", 0, 0);
        assert_eq!(h.columns[0].len(), 1);
        assert_eq!(h.columns[0][0].name, "Blank");
        assert_eq!(h.columns[0][0].param, 7);
    }

    #[test]
    fn add_meter_by_name_unknown_name_adds_nothing() {
        let mut h = header(HeaderLayout::HF_ONE_100, vec![vec![]]);
        Header_addMeterByName(&mut h, "NoSuchMeter", 0, 0);
        assert!(h.columns[0].is_empty());
    }

    #[test]
    fn add_meter_by_name_non_numeric_suffix_is_dynamic_and_adds_nothing() {
        // A non-numeric "(name)" is the DynamicMeter path; its lookup
        // substrate is unmodeled, so — as the C `return`s on lookup failure —
        // nothing is added, even though "Blank" is otherwise a known class.
        let mut h = header(HeaderLayout::HF_ONE_100, vec![vec![]]);
        Header_addMeterByName(&mut h, "Blank(cpu)", 0, 0);
        assert!(h.columns[0].is_empty());
    }

    #[test]
    fn add_meter_by_name_applies_mode() {
        let mut h = header(HeaderLayout::HF_ONE_100, vec![vec![]]);
        Header_addMeterByName(&mut h, "Blank", TEXT_METERMODE, 0);
        assert_eq!(h.columns[0][0].mode, TEXT_METERMODE);
    }

    // ---- Header_populateFromSettings --------------------------------

    #[test]
    fn populate_from_settings_relayouts_and_builds_columns() {
        // Header starts one-column; settings ask for two -> relaid out, then
        // each column's named meters constructed via Header_addMeterByName.
        let mut h = header(HeaderLayout::HF_ONE_100, vec![vec![]]);
        let settings = Settings {
            hLayout: HeaderLayout::HF_TWO_50_50,
            hColumns: vec![
                MeterColumnSetting {
                    len: 2,
                    names: Some(vec!["Blank".to_string(), "Blank".to_string()]),
                    modes: Some(vec![TEXT_METERMODE, TEXT_METERMODE]),
                },
                MeterColumnSetting {
                    len: 1,
                    names: Some(vec!["Blank".to_string()]),
                    modes: Some(vec![TEXT_METERMODE]),
                },
            ],
            ..Default::default()
        };

        Header_populateFromSettings(&mut h, &settings);

        assert_eq!(h.headerLayout, HeaderLayout::HF_TWO_50_50);
        assert_eq!(h.columns.len(), 2);
        assert_eq!(names(&h.columns[0]), ["Blank", "Blank"]);
        assert_eq!(names(&h.columns[1]), ["Blank"]);
        // Height was recomputed (BlankMeter is 1 row; col0 has two).
        assert!(h.height >= 1);
    }

    #[test]
    fn populate_from_settings_resolves_real_meter_classes() {
        // End-to-end: real class names resolve through Platform_meterTypes to
        // Meter_new, which seeds each class's maxItems-sized values vector.
        let mut h = header(HeaderLayout::HF_TWO_50_50, vec![vec![], vec![]]);
        let settings = Settings {
            hLayout: HeaderLayout::HF_TWO_50_50,
            hColumns: vec![
                MeterColumnSetting {
                    len: 2,
                    names: Some(vec!["Memory".to_string(), "Swap".to_string()]),
                    modes: Some(vec![TEXT_METERMODE, TEXT_METERMODE]),
                },
                MeterColumnSetting {
                    len: 1,
                    names: Some(vec!["Tasks".to_string()]),
                    modes: Some(vec![TEXT_METERMODE]),
                },
            ],
            ..Default::default()
        };

        Header_populateFromSettings(&mut h, &settings);

        assert_eq!(names(&h.columns[0]), ["Memory", "Swap"]);
        assert_eq!(names(&h.columns[1]), ["Tasks"]);
        // Meter_new sized values[] to each class's maxItems.
        assert_eq!(h.columns[0][0].values.len(), 6); // Memory: MEMORY_1..6
        assert_eq!(h.columns[0][1].values.len(), 3); // Swap: USED/CACHE/FRONTSWAP
        assert_eq!(h.columns[1][0].values.len(), 4); // Tasks
    }

    #[test]
    fn populate_from_settings_resolves_load_uptime_clock_hostname() {
        // The text/load meters registered this batch also resolve.
        let mut h = header(HeaderLayout::HF_ONE_100, vec![vec![]]);
        let settings = Settings {
            hLayout: HeaderLayout::HF_ONE_100,
            hColumns: vec![MeterColumnSetting {
                len: 6,
                names: Some(vec![
                    "LoadAverage".to_string(),
                    "Uptime".to_string(),
                    "Clock".to_string(),
                    "Hostname".to_string(),
                    "Battery".to_string(),
                    "System".to_string(),
                ]),
                modes: Some(vec![TEXT_METERMODE; 6]),
            }],
            ..Default::default()
        };

        Header_populateFromSettings(&mut h, &settings);

        assert_eq!(
            names(&h.columns[0]),
            [
                "LoadAverage",
                "Uptime",
                "Clock",
                "Hostname",
                "Battery",
                "System"
            ]
        );
        assert_eq!(h.columns[0][0].values.len(), 3); // LoadAverage: 1/5/15
        assert_eq!(h.columns[0][1].values.len(), 0); // Uptime: maxItems 0
        assert_eq!(h.columns[0][4].values.len(), 1); // Battery: maxItems 1
    }

    #[test]
    fn populate_from_settings_prunes_existing_meters_first() {
        // Pre-seed a stale meter; populate must clear it before re-adding.
        let mut h = header(HeaderLayout::HF_ONE_100, vec![vec![draw_meter("stale", 2)]]);
        let settings = Settings {
            hLayout: HeaderLayout::HF_ONE_100,
            hColumns: vec![MeterColumnSetting {
                len: 1,
                names: Some(vec!["Blank".to_string()]),
                modes: Some(vec![0]),
            }],
            ..Default::default()
        };

        Header_populateFromSettings(&mut h, &settings);

        assert_eq!(names(&h.columns[0]), ["Blank"]); // stale gone
    }

    // ---- Header_setLayout -------------------------------------------

    #[test]
    fn set_layout_grow_appends_empty_columns() {
        let mut h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![vec![meter("A", 2)], vec![meter("B", 2)]],
        );
        Header_setLayout(&mut h, HeaderLayout::HF_FOUR_25_25_25_25);
        assert_eq!(h.columns.len(), 4);
        assert_eq!(h.headerLayout, HeaderLayout::HF_FOUR_25_25_25_25);
        // existing columns preserved, new ones empty (columnWidthCount is
        // recomputed by the calculateHeight inside setLayout, so compare
        // by name)
        assert_eq!(names(&h.columns[0]), ["A"]);
        assert_eq!(names(&h.columns[1]), ["B"]);
        assert!(h.columns[2].is_empty());
        assert!(h.columns[3].is_empty());
    }

    #[test]
    fn set_layout_shrink_migrates_meters_in_reverse() {
        // 3 -> 1: col1 = [B,C], col2 = [D] migrate into col0.
        // C order: iterate removed cols left->right, within each pop from
        // the end. col1 popped C then B, col2 popped D. So col0 gains
        // [A, C, B, D].
        let mut h = header(
            HeaderLayout::HF_THREE_33_34_33,
            vec![
                vec![meter("A", 2)],
                vec![meter("B", 2), meter("C", 2)],
                vec![meter("D", 2)],
            ],
        );
        Header_setLayout(&mut h, HeaderLayout::HF_ONE_100);
        assert_eq!(h.columns.len(), 1);
        assert_eq!(names(&h.columns[0]), ["A", "C", "B", "D"]);
    }

    #[test]
    fn set_layout_same_count_is_noop_on_columns() {
        // 2 -> 2 (different ratio, same column count): columns untouched.
        let mut h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![vec![meter("A", 2)], vec![meter("B", 2)]],
        );
        Header_setLayout(&mut h, HeaderLayout::HF_TWO_33_67);
        assert_eq!(h.headerLayout, HeaderLayout::HF_TWO_33_67);
        assert_eq!(h.columns.len(), 2);
        // same column count -> early return, no calculateHeight, columns
        // untouched (Meter is not PartialEq; compare by name + height).
        assert_eq!(names(&h.columns[0]), ["A"]);
        assert_eq!(names(&h.columns[1]), ["B"]);
        assert_eq!(h.columns[0][0].h, 2);
        assert_eq!(h.columns[1][0].h, 2);
    }

    // ---- Header_writeBackToSettings ---------------------------------

    #[test]
    fn write_back_copies_names_modes_and_sets_layout() {
        // Header has 2 columns; settings starts at 1 column and must be
        // resized by the layout write.
        let hm = |n: &'static str, mode: MeterModeId| Meter {
            name: n,
            mode,
            h: 2,
            ..Meter::empty()
        };
        let h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![
                vec![hm("AllCPUs", 1), hm("Memory", 2)],
                vec![hm("Tasks", 1)],
            ],
        );
        let mut settings = Settings {
            hLayout: HeaderLayout::HF_ONE_100,
            hColumns: vec![MeterColumnSetting::default()],
            screens: Vec::new(),
            ssIndex: 0,
            changed: false,
            lastUpdate: 0,
            ..Default::default()
        };

        Header_writeBackToSettings(&h, &mut settings);

        assert_eq!(settings.hLayout, HeaderLayout::HF_TWO_50_50);
        assert_eq!(settings.hColumns.len(), 2);
        assert_eq!(
            settings.hColumns[0].names.as_deref().unwrap(),
            ["AllCPUs", "Memory"]
        );
        assert_eq!(settings.hColumns[0].modes.as_deref().unwrap(), [1u32, 2]);
        assert_eq!(settings.hColumns[0].len, 2);
        assert_eq!(settings.hColumns[1].names.as_deref().unwrap(), ["Tasks"]);
        assert_eq!(settings.hColumns[1].len, 1);
    }

    #[test]
    fn write_back_empty_column_is_none() {
        let h = header(
            HeaderLayout::HF_TWO_50_50,
            vec![vec![meter("AllCPUs", 2)], vec![]],
        );
        let mut settings = Settings {
            hLayout: HeaderLayout::HF_TWO_50_50,
            hColumns: vec![MeterColumnSetting::default(), MeterColumnSetting::default()],
            screens: Vec::new(),
            ssIndex: 0,
            changed: false,
            lastUpdate: 0,
            ..Default::default()
        };

        Header_writeBackToSettings(&h, &mut settings);

        assert_eq!(settings.hColumns[0].len, 1);
        // empty column -> C NULL == None, len 0
        assert!(settings.hColumns[1].names.is_none());
        assert!(settings.hColumns[1].modes.is_none());
        assert_eq!(settings.hColumns[1].len, 0);
    }
}
