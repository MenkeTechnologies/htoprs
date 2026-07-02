//! Partial port of `Settings.c` — htop's config-file settings layer.
//!
//! Ported (full behavior reproducible in safe Rust with the substrate
//! that exists today):
//!
//! * `Settings_splitLineToIDs` — pure string work over the ported `XUtils`.
//! * The meter readers `Settings_readMeters` / `Settings_readMeterModes`
//!   and `Settings_validateMeters` — string + `HeaderLayout` only.
//! * `Settings_defaultMeters` — builds the default two-column meter
//!   layout from `Machine.activeCPUs` (ported in `machine.rs`) and the
//!   `BAR_METERMODE`/`TEXT_METERMODE` constants (ported in `meter.rs`);
//!   touches only `hLayout`/`hColumns`.
//! * `Settings_setHeaderLayout` — resizes the `hColumns` array.
//! * The meter writers `writeList` / `writeMeters` / `writeMeterModes` —
//!   string building into a buffer (the C `OutputFunc`/`FILE*` sink is
//!   modeled as a `&mut String`, since the config text is identical).
//! * `ScreenSettings_invertSortOrder` and the `readonly` latch pair.
//!
//! Stubbed (cannot be ported faithfully yet — the specific blocker is
//! named on each stub below):
//!
//! * The field-name/index family `toFieldName` / `toFieldIndex` /
//!   `ScreenSettings_readFields` / `ScreenSettings_setSortKey` and the
//!   `writeFields` writer all index the platform `Process_fields[]`
//!   `ProcessFieldData` table (its `.name`/`.flags`/`.defaultSortDesc`)
//!   and `LAST_PROCESSFIELD`/`ROW_DYNAMIC_FIELDS`/`RowField`, none of
//!   which is ported (`process.rs` has the `ProcessField` enum but not
//!   the data table), plus `DynamicColumn_lookup`/`DynamicColumn_search`
//!   which are still `todo!()` stubs in `dynamiccolumn.rs`.
//! * The screen constructors `Settings_newScreen` /
//!   `Settings_newDynamicScreen` / `Settings_initScreenSettings` /
//!   `Settings_defaultScreens` sit on top of that field family plus the
//!   full `ScreenSettings`/`screens[]` array model and `Platform_*`
//!   (`Platform_defaultScreens`, `Platform_addDynamicScreen`).
//! * `Settings_read` / `Settings_write` / `Settings_new` /
//!   `signal_safe_fprintf` are the file-I/O layer (`open`/`fstat`/
//!   `mkstemp`/`rename`/`realpath`, env reads, `Platform_*`) sitting on
//!   top of the above.
//! * The heap-free `Settings_deleteColumns` / `Settings_deleteScreens` /
//!   `Settings_delete` / `ScreenSettings_delete` free the owned arrays and
//!   the struct; `Settings`/`ScreenSettings`/`MeterColumnSetting` own
//!   their fields, so `Drop` frees them and there is no faithful body to
//!   port (same call as `History_delete` in `history.rs`).
//!
//! `HeaderLayout` and `HeaderLayout_getColumns` are ports of the pure
//! `HeaderLayout.h` `static inline` helpers, inlined here because the
//! meter functions above fundamentally need the per-layout column count
//! and `HeaderLayout.c` has no ported module yet.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use std::sync::atomic::{AtomicBool, Ordering};

use crate::ported::machine::Machine;
use crate::ported::meter::{BAR_METERMODE, TEXT_METERMODE};
use crate::ported::xutils::{String_split, String_trim};

/// Port of `MeterMode.h:20` — `typedef unsigned int MeterModeId`.
pub type MeterModeId = u32;

/// Port of the `HeaderLayout` enum from `HeaderLayout.h:18`. Discriminants
/// match the C enum: `HF_INVALID = -1`, `HF_ONE_100 = 0`, then ascending.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeaderLayout {
    HF_INVALID = -1,
    HF_ONE_100 = 0,
    HF_TWO_50_50,
    HF_TWO_33_67,
    HF_TWO_67_33,
    HF_THREE_33_34_33,
    HF_THREE_25_25_50,
    HF_THREE_25_50_25,
    HF_THREE_50_25_25,
    HF_THREE_40_30_30,
    HF_THREE_30_40_30,
    HF_THREE_30_30_40,
    HF_THREE_40_20_40,
    HF_FOUR_25_25_25_25,
    LAST_HEADER_LAYOUT,
}

/// Port of `HeaderLayout_getColumns` (`HeaderLayout.h:57`). Returns the
/// `columns` count of the layout's `HeaderLayout_layouts[]` row. The C
/// asserts `0 <= hLayout < LAST_HEADER_LAYOUT`; the two uninitialized
/// variants panic here to mirror that debug assertion.
pub fn HeaderLayout_getColumns(hLayout: HeaderLayout) -> usize {
    use HeaderLayout::*;
    match hLayout {
        HF_ONE_100 => 1,
        HF_TWO_50_50 | HF_TWO_33_67 | HF_TWO_67_33 => 2,
        HF_THREE_33_34_33
        | HF_THREE_25_25_50
        | HF_THREE_25_50_25
        | HF_THREE_50_25_25
        | HF_THREE_40_30_30
        | HF_THREE_30_40_30
        | HF_THREE_30_30_40
        | HF_THREE_40_20_40 => 3,
        HF_FOUR_25_25_25_25 => 4,
        HF_INVALID | LAST_HEADER_LAYOUT => {
            panic!("HeaderLayout_getColumns: uninitialized layout {hLayout:?}")
        }
    }
}

/// A subset of htop's `MeterColumnSetting` (`Settings.h:36`). The C
/// `char** names` is a NUL-terminated array; here it is an owned
/// `Vec<String>` wrapped in `Option` to distinguish "never set" (C
/// `NULL`) from "set to the empty list". `len` still counts *modes*, as
/// in C — it is written by `Settings_readMeterModes`, not by
/// `Settings_readMeters`.
#[derive(Default, Clone, Debug)]
pub struct MeterColumnSetting {
    pub len: usize,
    pub names: Option<Vec<String>>,
    pub modes: Option<Vec<MeterModeId>>,
}

/// A subset of htop's `Settings` (`Settings.h:57`) holding only the
/// fields the ported meter/layout functions touch: the header layout and
/// its per-column meter settings, plus the `changed` dirty flag that
/// `Settings_setHeaderLayout` sets. Every other field (filenames,
/// display toggles, screens, dynamic-column hashtables, …) is omitted
/// because no ported function reads or writes it.
pub struct Settings {
    pub hLayout: HeaderLayout,
    pub hColumns: Vec<MeterColumnSetting>,
    pub changed: bool,
}

/// C `atoi` semantics as used throughout `Settings.c` (e.g. the meter
/// mode tokens at `Settings.c:83`): skip leading whitespace, an optional
/// `+`/`-` sign, then base-10 digits; stop at the first non-digit and
/// return `0` when no digits are present. Overflow wraps (C leaves it
/// undefined; wrapping avoids a panic).
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

/// Port of `Settings.c:59`. Trims leading/trailing ` `/`\t`/`\n` from
/// `line`, then splits the remainder on single spaces. Interior empty
/// fields (consecutive spaces) are kept and a trailing empty field is
/// dropped, exactly as htop's `String_split(trim, ' ', NULL)` does. The
/// C `free(trim)` is handled by Rust ownership.
pub fn Settings_splitLineToIDs(line: &str) -> Vec<String> {
    let trim = String_trim(line);
    String_split(&trim, ' ')
}

/// Port of `Settings.c:66`. Clamps `column` to the last header column,
/// then stores the space-split IDs of `line` as that column's meter
/// `names`. As in C, this does *not* touch `len` (which counts modes).
pub fn Settings_readMeters(this: &mut Settings, line: &str, column: usize) {
    let column = column.min(HeaderLayout_getColumns(this.hLayout) - 1);
    this.hColumns[column].names = Some(Settings_splitLineToIDs(line));
}

/// Port of `Settings.c:71`. Parses the space-split IDs of `line` as
/// `MeterModeId` integers (via `atoi`), records their count in the
/// column's `len`, and stores the modes. When there are no IDs the C
/// sets `modes = NULL`; here that is `None`.
pub fn Settings_readMeterModes(this: &mut Settings, line: &str, column: usize) {
    let ids = Settings_splitLineToIDs(line);

    let len = ids.len();

    let column = column.min(HeaderLayout_getColumns(this.hLayout) - 1);
    this.hColumns[column].len = len;
    let modes = if len != 0 {
        Some(ids.iter().map(|id| atoi(id) as MeterModeId).collect())
    } else {
        None
    };
    this.hColumns[column].modes = modes;
}

/// Port of `Settings.c:90`. Returns `true` iff at least one column has
/// meters and every populated column is internally consistent: `names`
/// and `modes` both present, one name per mode, and no extra name past
/// `len` (the C NUL-terminator check `names[len]`). A column with
/// `len == 0` is skipped.
pub fn Settings_validateMeters(this: &Settings) -> bool {
    let colCount = HeaderLayout_getColumns(this.hLayout);

    let mut anyMeter = false;

    for column in 0..colCount {
        let names = &this.hColumns[column].names;
        let modes = &this.hColumns[column].modes;
        let len = this.hColumns[column].len;

        if len == 0 {
            continue;
        }

        if names.is_none() || modes.is_none() {
            return false;
        }

        anyMeter |= len != 0;

        let names = names.as_ref().unwrap();

        // Check for each mode there is an entry with a non-NULL name
        for meterIdx in 0..len {
            if meterIdx >= names.len() {
                return false;
            }
        }

        if names.len() > len {
            return false;
        }
    }

    anyMeter
}

/// TODO: port of `static void Settings_deleteColumns(Settings* this` from
/// `Settings.c:35`. Heap-free only (frees each column's `names` array +
/// `modes`, then the `hColumns` array); `MeterColumnSetting` owns its
/// `Vec`s, so `Drop` frees them and there is no faithful body to port.
/// Left as a stub.
pub fn Settings_deleteColumns() {
    todo!("port of Settings.c:35")
}

/// TODO: port of `static void Settings_deleteScreens(Settings* this` from
/// `Settings.c:43`. Heap-free only (`ScreenSettings_delete` each screen,
/// then free the `screens` array); the owned screen model frees via
/// `Drop`, so there is no faithful body to port. Left as a stub.
pub fn Settings_deleteScreens() {
    todo!("port of Settings.c:43")
}

/// TODO: port of `void Settings_delete(Settings* this` from
/// `Settings.c:51`. Heap-free only (frees `filename`/`initialFilename`,
/// the columns, the screens, and the struct); `Settings` owns its fields
/// and frees them via `Drop`, so there is no faithful body to port. Left
/// as a stub (same call as `History_delete`).
pub fn Settings_delete() {
    todo!("port of Settings.c:51")
}

/// Port of `Settings.c:120`. Installs the built-in two-column
/// (`HF_TWO_50_50`) header meter layout scaled to the host's CPU count:
/// column 0 is always `<CPU-variant> Memory Swap` (all `BAR`), column 1
/// is `[<RightCPUs>] Tasks LoadAverage Uptime` (`Tasks`/`LoadAverage`/
/// `Uptime` are `TEXT`). The CPU meter name is chosen by `activeCPUs`
/// exactly as the C if/else chain: `>128` a single averaged `CPU`,
/// `>32/>16/>8/>4` the `Left/RightCPUs{8,4,2,}` split pair, else a single
/// `AllCPUs`. The right column gains the extra `RightCPUs*` slot only for
/// `4 < activeCPUs <= 128`, matching the C `sizes[1]++` guard.
///
/// The C first calls `Settings_deleteColumns` to release the previous
/// `hColumns`; here reassigning `this.hColumns` drops the old columns via
/// `Drop`. The C `char** names` NUL-terminator slot is not modeled — the
/// `Vec<String>` carries exactly `len` names. `xStrdup` becomes
/// `String::to_string`.
pub fn Settings_defaultMeters(this: &mut Settings, host: &Machine) {
    let initialCpuCount = host.activeCPUs;
    let mut sizes: [usize; 2] = [3, 3];

    if initialCpuCount > 4 && initialCpuCount <= 128 {
        sizes[1] += 1;
    }

    // Release any previously allocated memory (C `Settings_deleteColumns`);
    // reassigning `this.hColumns` below drops the old columns via `Drop`.

    this.hLayout = HeaderLayout::HF_TWO_50_50;

    let mut names0: Vec<String> = Vec::new();
    let mut modes0: Vec<MeterModeId> = Vec::new();
    let mut names1: Vec<String> = Vec::new();
    let mut modes1: Vec<MeterModeId> = Vec::new();

    if initialCpuCount > 128 {
        // Just show the average, ricers need to config for impressive screenshots
        names0.push("CPU".to_string());
        modes0.push(BAR_METERMODE);
    } else if initialCpuCount > 32 {
        names0.push("LeftCPUs8".to_string());
        modes0.push(BAR_METERMODE);
        names1.push("RightCPUs8".to_string());
        modes1.push(BAR_METERMODE);
    } else if initialCpuCount > 16 {
        names0.push("LeftCPUs4".to_string());
        modes0.push(BAR_METERMODE);
        names1.push("RightCPUs4".to_string());
        modes1.push(BAR_METERMODE);
    } else if initialCpuCount > 8 {
        names0.push("LeftCPUs2".to_string());
        modes0.push(BAR_METERMODE);
        names1.push("RightCPUs2".to_string());
        modes1.push(BAR_METERMODE);
    } else if initialCpuCount > 4 {
        names0.push("LeftCPUs".to_string());
        modes0.push(BAR_METERMODE);
        names1.push("RightCPUs".to_string());
        modes1.push(BAR_METERMODE);
    } else {
        names0.push("AllCPUs".to_string());
        modes0.push(BAR_METERMODE);
    }
    names0.push("Memory".to_string());
    modes0.push(BAR_METERMODE);
    names0.push("Swap".to_string());
    modes0.push(BAR_METERMODE);
    names1.push("Tasks".to_string());
    modes1.push(TEXT_METERMODE);
    names1.push("LoadAverage".to_string());
    modes1.push(TEXT_METERMODE);
    names1.push("Uptime".to_string());
    modes1.push(TEXT_METERMODE);

    this.hColumns = vec![
        MeterColumnSetting {
            len: sizes[0],
            names: Some(names0),
            modes: Some(modes0),
        },
        MeterColumnSetting {
            len: sizes[1],
            names: Some(names1),
            modes: Some(modes1),
        },
    ];
}

/// TODO: port of `static const char* toFieldName(Hashtable* columns, int id, bool* enabled` from `Settings.c:181`.
/// Needs the platform `Process_fields[]` table and the `DynamicColumn`
/// `Hashtable` — left stubbed.
pub fn toFieldName() {
    todo!("port of Settings.c:181")
}

/// TODO: port of `static int toFieldIndex(Hashtable* columns, const char* str` from `Settings.c:198`.
/// Needs `toFieldName` (`Process_fields[]`) and `DynamicColumn_search`
/// over the `Hashtable` — left stubbed.
pub fn toFieldIndex() {
    todo!("port of Settings.c:198")
}

/// TODO: port of `static void ScreenSettings_readFields(ScreenSettings* ss, Hashtable* columns, const char* line` from `Settings.c:230`.
/// Needs `toFieldIndex` (still stubbed) plus the platform
/// `Process_fields[id].flags` table and `LAST_PROCESSFIELD`, neither
/// ported — left stubbed.
pub fn ScreenSettings_readFields() {
    todo!("port of Settings.c:230")
}

/// TODO: port of `static ScreenSettings* Settings_initScreenSettings(ScreenSettings* ss, Settings* this, const char* columns` from `Settings.c:254`.
/// Calls the stubbed `ScreenSettings_readFields` and manages the full
/// `screens[]` array, which is not modeled in this minimal `Settings` —
/// left stubbed.
pub fn Settings_initScreenSettings() {
    todo!("port of Settings.c:254")
}

/// TODO: port of `ScreenSettings* Settings_newScreen(Settings* this, const ScreenDefaults* defaults` from `Settings.c:263`.
/// Needs `toFieldIndex`, `Process_fields[sortKey].defaultSortDesc`, the
/// full `ScreenSettings`/`screens[]` model, and `Settings_initScreenSettings`
/// — all stubbed/unported. Left stubbed.
pub fn Settings_newScreen() {
    todo!("port of Settings.c:263")
}

/// TODO: port of `ScreenSettings* Settings_newDynamicScreen(Settings* this, const char* tab, const DynamicScreen* screen, Table* table` from `Settings.c:286`.
/// Needs `toFieldIndex`, the `DynamicScreen`/`Table` substrate, and the
/// `screens[]` model — left stubbed.
pub fn Settings_newDynamicScreen() {
    todo!("port of Settings.c:286")
}

/// TODO: port of `void ScreenSettings_delete(ScreenSettings* this` from
/// `Settings.c:302`. Heap-free only (frees `heading`/`dynamic`/`fields`
/// and the struct); the owned `ScreenSettings` model frees via `Drop`, so
/// there is no faithful body to port. Left as a stub.
pub fn ScreenSettings_delete() {
    todo!("port of Settings.c:302")
}

/// TODO: port of `static ScreenSettings* Settings_defaultScreens(Settings* this` from `Settings.c:309`.
/// Needs `Platform_numberOfDefaultScreens`/`Platform_defaultScreens`/
/// `Platform_defaultDynamicScreens` and the stubbed `Settings_newScreen`
/// — left stubbed.
pub fn Settings_defaultScreens() {
    todo!("port of Settings.c:309")
}

/// TODO: port of `static bool Settings_read(Settings* this, const char* fileName, const Machine* host, bool checkWritability` from `Settings.c:320`.
/// The config-file reader: `open`/`fstat` writability probe, line
/// parsing, then dispatch into `ScreenSettings_readFields` /
/// `Settings_newScreen` / `toFieldIndex` (all stubbed) and the full
/// `Settings` field set (display toggles, screens) not modeled here.
/// Left stubbed.
pub fn Settings_read() {
    todo!("port of Settings.c:320")
}

/// TODO: port of `static void writeFields(OutputFunc of, FILE* fp,` from `Settings.c:575`.
/// Needs `toFieldName` (`Process_fields[]` / `DynamicColumn`) for its
/// by-name branches — left stubbed.
pub fn writeFields() {
    todo!("port of Settings.c:575")
}

/// Port of `Settings.c:597`. Appends `list[0..len]` to `out`,
/// space-separated, followed by `separator`. Models the C `OutputFunc
/// of` / `FILE* fp` sink as a `&mut String` buffer since the produced
/// text is identical.
pub fn writeList(out: &mut String, list: &[String], len: usize, separator: char) {
    let mut sep = "";
    for i in 0..len {
        out.push_str(sep);
        out.push_str(&list[i]);
        sep = " ";
    }
    out.push(separator);
}

/// Port of `Settings.c:607`. Writes column `column`'s meter names via
/// [`writeList`] when it has meters, otherwise writes `!` then the
/// separator.
pub fn writeMeters(this: &Settings, out: &mut String, separator: char, column: usize) {
    let col = &this.hColumns[column];
    if col.len != 0 {
        writeList(
            out,
            col.names.as_ref().expect("names set when len != 0"),
            col.len,
            separator,
        );
    } else {
        out.push('!');
        out.push(separator);
    }
}

/// Port of `Settings.c:616`. Writes column `column`'s meter modes as
/// space-separated unsigned integers when it has meters, otherwise `!`;
/// then the separator.
pub fn writeMeterModes(this: &Settings, out: &mut String, separator: char, column: usize) {
    let col = &this.hColumns[column];
    if col.len != 0 {
        let modes = col.modes.as_ref().expect("modes set when len != 0");
        let mut sep = "";
        for i in 0..col.len {
            out.push_str(sep);
            out.push_str(&modes[i].to_string());
            sep = " ";
        }
    } else {
        out.push('!');
    }

    out.push(separator);
}

/// TODO: port of `static int signal_safe_fprintf(FILE* stream, const char* fmt, ...` from `Settings.c:632`.
/// The signal-safe `vsnprintf` + `full_write_str(fileno(stream), buf)`
/// crash-path writer. The ported meter/field writers model their sink as
/// a `&mut String`, not a `FILE*`/fd; there is no fd-write substrate to
/// port this against. Left stubbed.
pub fn signal_safe_fprintf() {
    todo!("port of Settings.c:632")
}

/// TODO: port of `int Settings_write(const Settings* this, bool onCrash` from `Settings.c:647`.
/// Needs `writeFields` (`Process_fields[]` / `DynamicColumn`), the
/// screens array, `HeaderLayout_getName`, and `mkstemp`/`rename` file
/// I/O — left stubbed.
pub fn Settings_write() {
    todo!("port of Settings.c:647")
}

/// TODO: port of `Settings* Settings_new(const Machine* host, Hashtable* dynamicMeters, Hashtable* dynamicColumns, Hashtable* dynamicScreens` from `Settings.c:794`.
/// The top-level constructor: reads `HTOPRC`/`HOME`/`XDG_CONFIG_HOME`
/// from the environment, `mkdir`s the config dir, `realpath`s the
/// filename, then drives `Settings_read`/`Settings_defaultScreens`/
/// `Settings_write` (all stubbed) over the full `Settings` field set not
/// modeled here. Left stubbed.
pub fn Settings_new() {
    todo!("port of Settings.c:794")
}

/// A subset of htop's `ScreenSettings` (`Settings.h:42`) holding only
/// the three fields `ScreenSettings_invertSortOrder` touches. The other
/// fields (`heading`, `dynamic`, `table`, `fields`, `flags`, `sortKey`,
/// `treeSortKey`, `treeViewAlwaysByPID`, `allBranchesCollapsed`) are
/// omitted because this ported function never reads or writes them.
pub struct ScreenSettings {
    pub treeView: bool,
    pub direction: i32,
    pub treeDirection: i32,
}

/// Port of `Settings.c:913`. Flips the active sort direction between `1`
/// and `-1`: `treeDirection` when `treeView` is set, otherwise
/// `direction`. Faithful to the C `(*attr == 1) ? -1 : 1`, so any value
/// other than `1` becomes `1` (not negated).
pub fn ScreenSettings_invertSortOrder(this: &mut ScreenSettings) {
    let attr = if this.treeView {
        &mut this.treeDirection
    } else {
        &mut this.direction
    };
    *attr = if *attr == 1 { -1 } else { 1 };
}

/// TODO: port of `void ScreenSettings_setSortKey(ScreenSettings* this, ProcessField sortKey` from `Settings.c:918`.
/// Needs `Process_fields[sortKey].defaultSortDesc` from the platform
/// field table — left stubbed.
pub fn ScreenSettings_setSortKey() {
    todo!("port of Settings.c:918")
}

/// The file-static `bool readonly` from `Settings.c:929`. A process-wide
/// latch, so it is a `static` `AtomicBool` here rather than a passed
/// value.
static READONLY: AtomicBool = AtomicBool::new(false);

/// Port of `Settings.c:931`. Sets the process-wide `readonly` latch. The
/// C `readonly = true` becomes an atomic store.
pub fn Settings_enableReadonly() {
    READONLY.store(true, Ordering::Relaxed);
}

/// Port of `Settings.c:935`. Returns the current value of the
/// process-wide `readonly` latch.
pub fn Settings_isReadonly() -> bool {
    READONLY.load(Ordering::Relaxed)
}

/// Port of `Settings.c:939`. Resizes `hColumns` to the new layout's
/// column count: grows with default (C `memset`-zeroed) columns, or
/// drops trailing columns (Rust `Drop` frees their names/modes, matching
/// the C `free` loop). Then updates `hLayout` and sets `changed`.
pub fn Settings_setHeaderLayout(this: &mut Settings, hLayout: HeaderLayout) {
    let oldColumns = HeaderLayout_getColumns(this.hLayout);
    let newColumns = HeaderLayout_getColumns(hLayout);

    if newColumns > oldColumns {
        this.hColumns
            .resize_with(newColumns, MeterColumnSetting::default);
    } else if newColumns < oldColumns {
        this.hColumns.truncate(newColumns);
    }

    this.hLayout = hLayout;
    this.changed = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A default 2-column (`HF_TWO_50_50`) `Settings` with empty meter
    /// columns, matching what `Settings_new` sets up before reading a
    /// config file.
    fn two_column_settings() -> Settings {
        Settings {
            hLayout: HeaderLayout::HF_TWO_50_50,
            hColumns: vec![MeterColumnSetting::default(), MeterColumnSetting::default()],
            changed: false,
        }
    }

    #[test]
    fn split_line_to_ids_trims_then_splits_on_space() {
        // leading/trailing whitespace stripped, then space-split
        assert_eq!(
            Settings_splitLineToIDs("  cpu mem swap  "),
            vec!["cpu", "mem", "swap"]
        );
        // interior double-space keeps an empty field (C String_split
        // keeps interior empties)
        assert_eq!(Settings_splitLineToIDs("a b  c"), vec!["a", "b", "", "c"]);
        // trim also strips tabs and newlines (the XUtils trim set)
        assert_eq!(Settings_splitLineToIDs("\tx y\n"), vec!["x", "y"]);
        // all-whitespace trims to empty -> no ids
        assert!(Settings_splitLineToIDs("   ").is_empty());
        assert!(Settings_splitLineToIDs("").is_empty());
    }

    #[test]
    fn read_meters_stores_names_and_clamps_column() {
        let mut s = two_column_settings();
        Settings_readMeters(&mut s, "AllCPUs Memory Swap", 0);
        Settings_readMeters(&mut s, "Tasks LoadAverage Uptime", 1);
        assert_eq!(
            s.hColumns[0].names.as_deref().unwrap(),
            ["AllCPUs", "Memory", "Swap"]
        );
        assert_eq!(
            s.hColumns[1].names.as_deref().unwrap(),
            ["Tasks", "LoadAverage", "Uptime"]
        );
        // readMeters does not set len (that is readMeterModes' job)
        assert_eq!(s.hColumns[0].len, 0);

        // column beyond the last is clamped to HeaderLayout_getColumns-1
        Settings_readMeters(&mut s, "OnlyOne", 5);
        assert_eq!(s.hColumns[1].names.as_deref().unwrap(), ["OnlyOne"]);
    }

    #[test]
    fn read_meter_modes_parses_ints_and_sets_len() {
        let mut s = two_column_settings();
        Settings_readMeterModes(&mut s, "1 1 1", 0);
        assert_eq!(s.hColumns[0].len, 3);
        assert_eq!(s.hColumns[0].modes.as_deref().unwrap(), [1u32, 1, 1]);

        // non-numeric token -> atoi returns 0; interior empty field (from
        // "2  4") is also atoi("") == 0
        Settings_readMeterModes(&mut s, "2 x  4", 1);
        assert_eq!(s.hColumns[1].len, 4);
        assert_eq!(s.hColumns[1].modes.as_deref().unwrap(), [2u32, 0, 0, 4]);

        // empty line -> len 0, modes None (C: modes = NULL)
        Settings_readMeterModes(&mut s, "", 0);
        assert_eq!(s.hColumns[0].len, 0);
        assert!(s.hColumns[0].modes.is_none());
    }

    /// Build a `Machine` whose only meaningful field for
    /// `Settings_defaultMeters` is `activeCPUs`.
    fn host_with_cpus(activeCPUs: u32) -> Machine {
        Machine {
            activeCPUs,
            ..Default::default()
        }
    }

    #[test]
    fn default_meters_small_cpu_uses_allcpus_three_and_three() {
        // activeCPUs <= 4: single AllCPUs meter, no RightCPUs, both
        // columns len 3.
        let mut s = two_column_settings();
        Settings_defaultMeters(&mut s, &host_with_cpus(4));

        assert_eq!(s.hLayout, HeaderLayout::HF_TWO_50_50);
        assert_eq!(s.hColumns.len(), 2);
        assert_eq!(s.hColumns[0].len, 3);
        assert_eq!(s.hColumns[1].len, 3);
        assert_eq!(
            s.hColumns[0].names.as_deref().unwrap(),
            ["AllCPUs", "Memory", "Swap"]
        );
        assert_eq!(
            s.hColumns[0].modes.as_deref().unwrap(),
            [BAR_METERMODE, BAR_METERMODE, BAR_METERMODE]
        );
        assert_eq!(
            s.hColumns[1].names.as_deref().unwrap(),
            ["Tasks", "LoadAverage", "Uptime"]
        );
        assert_eq!(
            s.hColumns[1].modes.as_deref().unwrap(),
            [TEXT_METERMODE, TEXT_METERMODE, TEXT_METERMODE]
        );
        // The resulting layout must satisfy the validator.
        assert!(Settings_validateMeters(&s));
    }

    #[test]
    fn default_meters_midrange_cpu_adds_right_cpus_split() {
        // 4 < activeCPUs <= 8: Left/RightCPUs pair, right column len 4.
        let mut s = two_column_settings();
        Settings_defaultMeters(&mut s, &host_with_cpus(6));

        assert_eq!(s.hColumns[0].len, 3);
        assert_eq!(s.hColumns[1].len, 4);
        assert_eq!(
            s.hColumns[0].names.as_deref().unwrap(),
            ["LeftCPUs", "Memory", "Swap"]
        );
        assert_eq!(
            s.hColumns[1].names.as_deref().unwrap(),
            ["RightCPUs", "Tasks", "LoadAverage", "Uptime"]
        );
        assert_eq!(
            s.hColumns[1].modes.as_deref().unwrap(),
            [BAR_METERMODE, TEXT_METERMODE, TEXT_METERMODE, TEXT_METERMODE]
        );
        assert!(Settings_validateMeters(&s));
    }

    #[test]
    fn default_meters_cpu_bucket_names_track_thresholds() {
        // Each threshold selects a distinct Left/RightCPUs variant.
        for (cpus, left, right) in [
            (9u32, "LeftCPUs2", "RightCPUs2"),
            (17, "LeftCPUs4", "RightCPUs4"),
            (33, "LeftCPUs8", "RightCPUs8"),
        ] {
            let mut s = two_column_settings();
            Settings_defaultMeters(&mut s, &host_with_cpus(cpus));
            assert_eq!(s.hColumns[0].names.as_deref().unwrap()[0], left);
            assert_eq!(s.hColumns[1].names.as_deref().unwrap()[0], right);
            assert_eq!(s.hColumns[1].len, 4);
        }
    }

    #[test]
    fn default_meters_huge_cpu_shows_single_averaged_cpu() {
        // activeCPUs > 128: single averaged "CPU", no RightCPUs, len 3/3.
        let mut s = two_column_settings();
        Settings_defaultMeters(&mut s, &host_with_cpus(256));

        assert_eq!(s.hColumns[0].len, 3);
        assert_eq!(s.hColumns[1].len, 3);
        assert_eq!(s.hColumns[0].names.as_deref().unwrap()[0], "CPU");
        assert_eq!(
            s.hColumns[1].names.as_deref().unwrap(),
            ["Tasks", "LoadAverage", "Uptime"]
        );
        assert!(Settings_validateMeters(&s));
    }

    #[test]
    fn default_meters_replaces_prior_columns() {
        // Prior custom columns are dropped and replaced by the defaults.
        let mut s = two_column_settings();
        Settings_readMeters(&mut s, "Custom1 Custom2", 0);
        Settings_readMeterModes(&mut s, "1 1", 0);
        Settings_defaultMeters(&mut s, &host_with_cpus(2));
        assert_eq!(
            s.hColumns[0].names.as_deref().unwrap(),
            ["AllCPUs", "Memory", "Swap"]
        );
        assert_eq!(s.hColumns[0].len, 3);
    }

    #[test]
    fn validate_meters_true_for_consistent_columns() {
        let mut s = two_column_settings();
        Settings_readMeters(&mut s, "AllCPUs Memory Swap", 0);
        Settings_readMeterModes(&mut s, "1 1 1", 0);
        Settings_readMeters(&mut s, "Tasks LoadAverage Uptime", 1);
        Settings_readMeterModes(&mut s, "2 2 2", 1);
        assert!(Settings_validateMeters(&s));
    }

    #[test]
    fn validate_meters_false_when_names_missing_or_mismatched() {
        // modes/len set but names never read -> names None -> false
        let mut s = two_column_settings();
        Settings_readMeterModes(&mut s, "1 1 1", 0);
        assert!(!Settings_validateMeters(&s));

        // fewer names than modes -> false (a mode with no name)
        let mut s = two_column_settings();
        Settings_readMeters(&mut s, "AllCPUs Memory", 0);
        Settings_readMeterModes(&mut s, "1 1 1", 0);
        assert!(!Settings_validateMeters(&s));

        // more names than modes -> false (C names[len] != NULL)
        let mut s = two_column_settings();
        Settings_readMeters(&mut s, "AllCPUs Memory Swap Extra", 0);
        Settings_readMeterModes(&mut s, "1 1 1", 0);
        assert!(!Settings_validateMeters(&s));
    }

    #[test]
    fn validate_meters_false_when_no_column_has_meters() {
        // all columns len 0 -> anyMeter stays false
        let s = two_column_settings();
        assert!(!Settings_validateMeters(&s));
    }

    #[test]
    fn write_list_space_joins_then_separator() {
        let list = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut out = String::new();
        writeList(&mut out, &list, list.len(), '\n');
        assert_eq!(out, "a b c\n");

        // empty list -> just the separator
        let mut out = String::new();
        writeList(&mut out, &[], 0, ';');
        assert_eq!(out, ";");
    }

    #[test]
    fn write_meters_and_modes_roundtrip_read() {
        // read a meters config, then write it back — the text between the
        // "=" and the separator must reproduce exactly
        let mut s = two_column_settings();
        Settings_readMeters(&mut s, "AllCPUs Memory Swap", 0);
        Settings_readMeterModes(&mut s, "1 1 1", 0);

        let mut names_out = String::new();
        writeMeters(&s, &mut names_out, '\n', 0);
        assert_eq!(names_out, "AllCPUs Memory Swap\n");

        let mut modes_out = String::new();
        writeMeterModes(&s, &mut modes_out, '\n', 0);
        assert_eq!(modes_out, "1 1 1\n");
    }

    #[test]
    fn write_meters_empty_column_writes_bang() {
        // an empty column: names writer emits "!<sep>", modes writer "!<sep>"
        let s = two_column_settings();
        let mut names_out = String::new();
        writeMeters(&s, &mut names_out, '\n', 0);
        assert_eq!(names_out, "!\n");

        let mut modes_out = String::new();
        writeMeterModes(&s, &mut modes_out, '\n', 0);
        assert_eq!(modes_out, "!\n");
    }

    #[test]
    fn set_header_layout_grows_shrinks_and_marks_changed() {
        let mut s = two_column_settings();

        // grow 2 -> 4: appends default columns, sets changed
        Settings_setHeaderLayout(&mut s, HeaderLayout::HF_FOUR_25_25_25_25);
        assert_eq!(s.hColumns.len(), 4);
        assert_eq!(s.hLayout, HeaderLayout::HF_FOUR_25_25_25_25);
        assert!(s.changed);

        // shrink 4 -> 1: drops trailing columns
        s.changed = false;
        Settings_setHeaderLayout(&mut s, HeaderLayout::HF_ONE_100);
        assert_eq!(s.hColumns.len(), 1);
        assert_eq!(s.hLayout, HeaderLayout::HF_ONE_100);
        assert!(s.changed);

        // equal count -> array untouched, still marks changed
        s.changed = false;
        Settings_setHeaderLayout(&mut s, HeaderLayout::HF_ONE_100);
        assert_eq!(s.hColumns.len(), 1);
        assert!(s.changed);
    }

    #[test]
    fn header_layout_get_columns_counts() {
        use HeaderLayout::*;
        assert_eq!(HeaderLayout_getColumns(HF_ONE_100), 1);
        assert_eq!(HeaderLayout_getColumns(HF_TWO_50_50), 2);
        assert_eq!(HeaderLayout_getColumns(HF_TWO_33_67), 2);
        assert_eq!(HeaderLayout_getColumns(HF_THREE_33_34_33), 3);
        assert_eq!(HeaderLayout_getColumns(HF_THREE_40_20_40), 3);
        assert_eq!(HeaderLayout_getColumns(HF_FOUR_25_25_25_25), 4);
    }

    #[test]
    fn invert_sort_order_toggles_direction_when_not_treeview() {
        let mut ss = ScreenSettings {
            treeView: false,
            direction: 1,
            treeDirection: 1,
        };
        ScreenSettings_invertSortOrder(&mut ss);
        assert_eq!(ss.direction, -1);
        assert_eq!(ss.treeDirection, 1); // untouched

        ScreenSettings_invertSortOrder(&mut ss);
        assert_eq!(ss.direction, 1);
    }

    #[test]
    fn invert_sort_order_uses_tree_direction_when_treeview() {
        let mut ss = ScreenSettings {
            treeView: true,
            direction: 1,
            treeDirection: 1,
        };
        ScreenSettings_invertSortOrder(&mut ss);
        assert_eq!(ss.treeDirection, -1);
        assert_eq!(ss.direction, 1); // untouched
    }

    #[test]
    fn invert_sort_order_non_one_becomes_one() {
        // C is `(*attr == 1) ? -1 : 1`, so any value != 1 collapses to 1
        let mut ss = ScreenSettings {
            treeView: false,
            direction: 5,
            treeDirection: 0,
        };
        ScreenSettings_invertSortOrder(&mut ss);
        assert_eq!(ss.direction, 1);
    }

    #[test]
    fn readonly_latch_starts_false_then_latches_true() {
        // single test owns the global latch to avoid cross-test races
        assert!(!Settings_isReadonly());
        Settings_enableReadonly();
        assert!(Settings_isReadonly());
    }
}
