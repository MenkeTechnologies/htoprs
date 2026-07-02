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
//! * `Header_writeBackToSettings` — copies each column's meter name/mode
//!   list back into the [`Settings`] `hColumns` (see the note on its doc
//!   about the `(param)` formatting that is not reproduced).
//!
//! Everything that constructs meters through the `MeterClass` vtable
//! (`Meter_new`, `Header_addMeterByName`, `Header_addMeterByClass`,
//! `Header_populateFromSettings`), draws to ncurses (`Header_draw`),
//! reinitializes meters (`Header_reinit`), pulls live values
//! (`Header_updateData`), or needs the `Machine` host allocation
//! (`Header_new`/`Header_delete`) stays a `todo!()` stub — that substrate
//! is not ported.
//!
//! ## Modeled structs
//!
//! htop's `Header` holds `Vector** columns` where each `Vector` owns
//! `Meter*`. The full `Meter` (`meter.rs`) deliberately omits the `h`,
//! `columnWidthCount`, and `param` fields (see its struct doc) and cannot
//! be constructed without the `MeterClass` vtable, so it cannot back the
//! header arithmetic here. [`HeaderMeter`] models exactly the `Meter`
//! fields the ported functions read or write. Likewise, the two settings
//! values `Header_calculateHeight` reads — `headerMargin` and
//! `screenTabs` — live on [`Header`] directly (the C reads them via
//! `this->host->settings`), because the `Machine`/`Settings` substrate
//! that carries them is not modeled.
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use crate::ported::settings::{
    HeaderLayout, HeaderLayout_getColumns, MeterModeId, Settings, Settings_setHeaderLayout,
};

/// Header-local model of the `Meter` (`Meter.h`) fields that the ported
/// header functions touch. The full `Meter` in `meter.rs` omits `h` and
/// `columnWidthCount` and has no vtable-free constructor, so it cannot be
/// reused for the layout arithmetic; this carries only what is read/written:
///
/// * `name` — the meter's serialized name as it appears in the config
///   `meters` line. In C the string is rebuilt at write time from
///   `As_Meter(meter)->name` plus any `(param)` suffix; here it is stored
///   already serialized (see [`Header_writeBackToSettings`]).
/// * `mode` — `meter->mode`, copied verbatim into the settings mode list.
/// * `h` — `meter->h`, the meter's height in rows; drives every height sum.
/// * `columnWidthCount` — `meter->columnWidthCount`, written by
///   [`Header_calculateHeight`] via [`calcColumnWidthCount`].
/// * `isBlank` — models the C `Object_isA(meter, &BlankMeter_class)` test
///   in `calcColumnWidthCount`: `true` iff the meter is a `BlankMeter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderMeter {
    pub name: String,
    pub mode: MeterModeId,
    pub h: i32,
    pub columnWidthCount: i32,
    pub isBlank: bool,
}

/// Model of htop's `Header` (`Header.h:20`). The C fields `columns`,
/// `headerLayout`, `pad`, and `height` are reproduced. `Vector** columns`
/// becomes `Vec<Vec<HeaderMeter>>` (one inner vec per column). The C
/// `Machine* host` is replaced by the two `host->settings` values the
/// ported arithmetic actually reads — `headerMargin` and `screenTabs` —
/// because the `Machine`/`Settings` substrate is not modeled here.
///
/// Invariant (as in C): `columns.len() == HeaderLayout_getColumns(headerLayout)`.
pub struct Header {
    pub columns: Vec<Vec<HeaderMeter>>,
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

/// TODO: port of `Header* Header_new(Machine* host, HeaderLayout hLayout)`
/// from `Header.c:31`. Stubbed: allocates the `Header`, stores the
/// `Machine* host`, and `Vector_new`s each column via `Class(Meter)`. The
/// `Machine` host and the `Meter` `ObjectClass` are not modeled, so there
/// is no faithful constructor; tests build [`Header`] via its public
/// fields.
pub fn Header_new() {
    todo!("port of Header.c:31")
}

/// TODO: port of `void Header_delete(Header* this)` from `Header.c:44`.
/// Stubbed: the body is `Vector_delete` per column then `free(columns)` /
/// `free(this)`. Rust `Drop` reclaims a [`Header`] automatically, so there
/// is no manual free to port.
pub fn Header_delete() {
    todo!("port of Header.c:44")
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

/// TODO: port of `static void Header_addMeterByName(Header* this, const
/// char* name, MeterModeId mode, size_t column)` from `Header.c:80`.
/// Stubbed: parses the `(param)` / `(dynamic)` suffix, then looks the name
/// up in `Platform_meterTypes` and constructs the meter with `Meter_new`
/// (the `MeterClass` vtable) and `Meter_setMode`; the `DynamicMeter`
/// branch also needs `this->host->settings->dynamicMeters`
/// (`DynamicMeter_search`). None of that substrate is ported.
pub fn Header_addMeterByName() {
    todo!("port of Header.c:80")
}

/// TODO: port of `void Header_populateFromSettings(Header* this)` from
/// `Header.c:120`. Stubbed: after `Header_setLayout` and pruning each
/// column, its only work is a loop of [`Header_addMeterByName`] to
/// construct the meters named in `settings->hColumns`. That constructor
/// needs the `MeterClass` vtable, so the read direction (settings → live
/// meters) cannot be ported faithfully; only the write direction
/// ([`Header_writeBackToSettings`]) is.
pub fn Header_populateFromSettings() {
    todo!("port of Header.c:120")
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
/// Not reproduced: the C rebuilds each name at write time as
/// `"%s(%s)"`/`"%s(%u)"` for `DynamicMeter`/`CPUMeter` meters with a
/// `param` (`As_Meter(meter)` vtable comparison plus `DynamicMeter_lookup`
/// over `settings->dynamicMeters`). That vtable/host substrate is not
/// modeled, so [`HeaderMeter::name`] is taken as the already-serialized
/// name and copied verbatim.
pub fn Header_writeBackToSettings(this: &Header, settings: &mut Settings) {
    Settings_setHeaderLayout(settings, this.headerLayout);

    let numColumns = HeaderLayout_getColumns(this.headerLayout);
    for col in 0..numColumns {
        let vec = &this.columns[col];
        let len = vec.len();

        let colSettings = &mut settings.hColumns[col];
        if len != 0 {
            colSettings.names = Some(vec.iter().map(|m| m.name.clone()).collect());
            colSettings.modes = Some(vec.iter().map(|m| m.mode).collect());
        } else {
            colSettings.names = None;
            colSettings.modes = None;
        }
        colSettings.len = len;
    }
}

/// TODO: port of `Meter* Header_addMeterByClass(Header* this, const
/// MeterClass* type, unsigned int param, size_t column)` from
/// `Header.c:173`. Stubbed: constructs a meter with `Meter_new(this->host,
/// param, type)` — the `MeterClass` vtable and `Machine` host are not
/// modeled.
pub fn Header_addMeterByClass() {
    todo!("port of Header.c:173")
}

/// TODO: port of `void Header_reinit(Header* this)` from `Header.c:183`.
/// Stubbed: calls `Meter_init(meter)` when the meter class defines an init
/// fn (`Meter_initFn`) — the `MeterClass` vtable is not ported.
pub fn Header_reinit() {
    todo!("port of Header.c:183")
}

/// TODO: port of `void Header_draw(const Header* this)` from
/// `Header.c:194`. Stubbed: ncurses cursor drawing (`attrset`, `mvhline`)
/// plus per-meter `meter->draw(...)` vtable dispatch and the
/// `HeaderLayout_layouts[].widths[]` column-width table — none of the
/// terminal/vtable substrate is ported.
pub fn Header_draw() {
    todo!("port of Header.c:194")
}

/// TODO: port of `void Header_updateData(Header* this)` from
/// `Header.c:240`. Stubbed: calls `Meter_updateValues(meter)` on every
/// meter — the `MeterClass` vtable update path is not ported.
pub fn Header_updateData() {
    todo!("port of Header.c:240")
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
    curMeter: &HeaderMeter,
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

            if !meter.isBlank {
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
/// meter's `columnWidthCount` via [`calcColumnWidthCount`]. The tallest
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
    use crate::ported::settings::MeterColumnSetting;

    /// A meter of the given height and blankness, with a fixed mode. Name
    /// is derived from `h` so writers have something to copy.
    fn meter(name: &str, h: i32) -> HeaderMeter {
        HeaderMeter {
            name: name.to_string(),
            mode: 1,
            h,
            columnWidthCount: 0,
            isBlank: false,
        }
    }

    fn blank(h: i32) -> HeaderMeter {
        HeaderMeter {
            name: "Blank".to_string(),
            mode: 1,
            h,
            columnWidthCount: 0,
            isBlank: true,
        }
    }

    /// Column meter names, for structural comparisons where the mutated
    /// `columnWidthCount` (set by `Header_calculateHeight`) is irrelevant.
    fn names(col: &[HeaderMeter]) -> Vec<&str> {
        col.iter().map(|m| m.name.as_str()).collect()
    }

    /// Build a `Header` from per-column meter lists. The layout's column
    /// count must equal `columns.len()`.
    fn header(hLayout: HeaderLayout, columns: Vec<Vec<HeaderMeter>>) -> Header {
        assert_eq!(HeaderLayout_getColumns(hLayout), columns.len());
        Header {
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
        // untouched
        assert_eq!(h.columns[0], vec![meter("A", 2)]);
        assert_eq!(h.columns[1], vec![meter("B", 2)]);
    }

    // ---- Header_writeBackToSettings ---------------------------------

    #[test]
    fn write_back_copies_names_modes_and_sets_layout() {
        // Header has 2 columns; settings starts at 1 column and must be
        // resized by the layout write.
        let hm = |n: &str, mode: MeterModeId| HeaderMeter {
            name: n.to_string(),
            mode,
            h: 2,
            columnWidthCount: 0,
            isBlank: false,
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
        };

        Header_writeBackToSettings(&h, &mut settings);

        assert_eq!(settings.hColumns[0].len, 1);
        // empty column -> C NULL == None, len 0
        assert!(settings.hColumns[1].names.is_none());
        assert!(settings.hColumns[1].modes.is_none());
        assert_eq!(settings.hColumns[1].len, 0);
    }
}
