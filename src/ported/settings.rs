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
//! * `ScreenSettings_getActiveSortKey` / `ScreenSettings_getActiveDirection`
//!   — the pure `static inline` accessors from `Settings.h`, field reads
//!   over the now-modeled `ScreenSettings` (they never touch
//!   `Process_fields[]`, only branch on `treeView`/`treeViewAlwaysByPID`).
//!
//! Ported field family (now that the platform `Process_fields[]` table
//! exists in `linux/linuxprocess.rs`): `toFieldName` / `toFieldIndex` /
//! `ScreenSettings_readFields` / `ScreenSettings_setSortKey` index the
//! `ProcessFieldData` table (`.name`/`.flags`/`.defaultSortDesc`) and resolve
//! dynamic columns through `DynamicColumn_lookup`/`DynamicColumn_search`.
//!
//! Now ported (with the borrowed `Hashtable*` and `filename` fields added
//! to `Settings`):
//!
//! * The screen constructors `Settings_initScreenSettings` /
//!   `Settings_newScreen` (drive `ScreenSettings_readFields` / `toFieldIndex`
//!   through the borrowed `dynamicColumns` pointer and append to
//!   `Settings.screens`).
//! * The heap-free destructors `Settings_deleteColumns` /
//!   `Settings_deleteScreens` / `Settings_delete` / `ScreenSettings_delete`
//!   (modeled as by-value consume / `Vec::clear`, mirroring the C free order;
//!   the borrowed `Hashtable*` are not freed — owned by the `Machine`).
//! * `signal_safe_fprintf` (a signal-safe `full_write` to a raw fd) and
//!   `Settings_write` (builds the config text via `writeFields`/`writeMeters`,
//!   `mkstemp`s a `0600` tempfile, then `rename`s into place; the crash path
//!   writes to `stderr`).
//! * `Settings_new` — the top-level constructor (env/`getpwuid`/`realpath`
//!   config-path resolution, `mkdir`, defaults). Its body is a faithful
//!   standalone port; it chain-calls `Settings_read` / `Settings_defaultScreens`
//!   (below), both now ported, so it boots without panicking.
//! * `Settings_defaultScreens` — builds the platform default screens. The
//!   darwin `Platform_defaultScreens` table (one `Main` entry) is inlined
//!   (the darwin `Platform` screens array is not separately ported); the
//!   empty-on-darwin `Platform_defaultDynamicScreens` is skipped.
//! * `Settings_read` — opens the config file (writability probe + read-only
//!   fallback), parses each `key=value` line, and dispatches to the ported
//!   field/meter/screen handlers. The `.dynamic` branch sets `screen->dynamic`
//!   but skips the unported `Platform_addDynamicScreen`; unknown keys are
//!   ignored, as in C.
//!
//! Still stubbed (the specific blocker is named on each stub below):
//!
//! * `Settings_newDynamicScreen` — the ported `DynamicScreen` models only
//!   `name`/`heading`, but the C reads `columnKeys`/`direction`.
//!
//! `HeaderLayout` and `HeaderLayout_getColumns` / `HeaderLayout_getName` /
//! `HeaderLayout_fromName` are ports of the pure `HeaderLayout.h` `static
//! inline` helpers, inlined here because the meter/config functions above
//! fundamentally need the per-layout column count and name and `HeaderLayout.c`
//! has no ported module yet.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

use std::ffi::CStr;
use std::os::unix::fs::DirBuilderExt;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ported::crt::ColorScheme;
use crate::ported::dynamiccolumn::{DynamicColumn_lookup, DynamicColumn_search};
use crate::ported::dynamicscreen::DynamicScreen;
use crate::ported::hashtable::Hashtable;
use crate::ported::linux::linuxprocess::{Process_fields, LAST_PROCESSFIELD};
use crate::ported::machine::{Machine, TableHandle};
use crate::ported::meter::{BAR_METERMODE, TEXT_METERMODE};
use crate::ported::process::{ProcessField, DEFAULT_HIGHLIGHT_SECS};
use crate::ported::xutils::{String_eq, String_split, String_startsWith, String_trim};

/// Port of `#define DEFAULT_DELAY 15` (`Settings.h:21`).
const DEFAULT_DELAY: i32 = 15;

/// Port of `#define CONFIG_READER_MIN_VERSION 3` (`Settings.h:23`).
const CONFIG_READER_MIN_VERSION: i32 = 3;

/// Port of the autoconf `CONFIGDIR` macro — `with_config` defaults to
/// `"/.config"` (`configure.ac:1477`), the per-`$HOME` config subdir.
const CONFIGDIR: &str = "/.config";

/// Port of the autoconf `SYSCONFDIR` macro — the system config dir
/// (`Makefile.am:27`; conventionally `/etc`).
const SYSCONFDIR: &str = "/etc";

/// Port of `PID` (`RowField.h:14`) — the hardcoded process-id field id `1`.
const PID: RowField = ProcessField::PID as RowField;

/// Port of `MeterMode.h:20` — `typedef unsigned int MeterModeId`.
pub type MeterModeId = u32;

/// Port of the `HeaderLayout` enum from `HeaderLayout.h:18`. Discriminants
/// match the C enum: `HF_INVALID = -1`, `HF_ONE_100 = 0`, then ascending.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HeaderLayout {
    HF_INVALID = -1,
    HF_ONE_100 = 0,
    /// htop's default header layout (`Settings.c:131`).
    #[default]
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
        HF_THREE_33_34_33 | HF_THREE_25_25_50 | HF_THREE_25_50_25 | HF_THREE_50_25_25
        | HF_THREE_40_30_30 | HF_THREE_30_40_30 | HF_THREE_30_30_40 | HF_THREE_40_20_40 => 3,
        HF_FOUR_25_25_25_25 => 4,
        HF_INVALID | LAST_HEADER_LAYOUT => {
            panic!("HeaderLayout_getColumns: uninitialized layout {hLayout:?}")
        }
    }
}

/// The [`HeaderLayout`] variants in `HeaderLayout_layouts[]` index order
/// (`HF_ONE_100 == 0` .. `HF_FOUR_25_25_25_25`), used to map a table index
/// back to its enum value in [`HeaderLayout_fromName`]. Not a C symbol —
/// C indexes the enum arithmetically (`(HeaderLayout) i`).
const HEADER_LAYOUTS_IN_ORDER: [HeaderLayout; HeaderLayout::LAST_HEADER_LAYOUT as usize] = {
    use HeaderLayout::*;
    [
        HF_ONE_100,
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
    ]
};

/// Port of `HeaderLayout_getName` (`HeaderLayout.h:66`, a pure `static
/// inline`). Returns the layout's config-file `name` from
/// `HeaderLayout_layouts[]`.
pub fn HeaderLayout_getName(hLayout: HeaderLayout) -> &'static str {
    HeaderLayout_layouts[hLayout as usize].name
}

/// Port of `HeaderLayout_fromName` (`HeaderLayout.h:75`, a pure `static
/// inline`). Scans `HeaderLayout_layouts[]` for the row whose `name` equals
/// `name` and returns that layout, else `LAST_HEADER_LAYOUT` (the C "not
/// found" sentinel).
pub fn HeaderLayout_fromName(name: &str) -> HeaderLayout {
    for i in 0..HeaderLayout::LAST_HEADER_LAYOUT as usize {
        if String_eq(HeaderLayout_layouts[i].name, name) {
            return HEADER_LAYOUTS_IN_ORDER[i];
        }
    }
    HeaderLayout::LAST_HEADER_LAYOUT
}

/// Port of the anonymous `HeaderLayout_layouts[]` row struct
/// (`HeaderLayout.h:36`): a layout's column count, per-column percentage
/// widths, config-file name, and setup-menu description.
pub struct HeaderLayoutDef {
    pub columns: u8,
    pub widths: [u8; 4],
    pub name: &'static str,
    pub description: &'static str,
}

/// Port of `static const ... HeaderLayout_layouts[LAST_HEADER_LAYOUT]`
/// (`HeaderLayout.h:41`) — the layout table, in [`HeaderLayout`] enum order.
/// `Header_draw` reads `.widths`, the config reader/writer `.name`, and the
/// Setup "Header Layout" panel `.description`.
#[allow(non_upper_case_globals)] // faithful C global name
pub static HeaderLayout_layouts: [HeaderLayoutDef; HeaderLayout::LAST_HEADER_LAYOUT as usize] = [
    HeaderLayoutDef {
        columns: 1,
        widths: [100, 0, 0, 0],
        name: "one_100",
        description: "1 column  - full width",
    },
    HeaderLayoutDef {
        columns: 2,
        widths: [50, 50, 0, 0],
        name: "two_50_50",
        description: "2 columns - 50/50 (default)",
    },
    HeaderLayoutDef {
        columns: 2,
        widths: [33, 67, 0, 0],
        name: "two_33_67",
        description: "2 columns - 33/67",
    },
    HeaderLayoutDef {
        columns: 2,
        widths: [67, 33, 0, 0],
        name: "two_67_33",
        description: "2 columns - 67/33",
    },
    HeaderLayoutDef {
        columns: 3,
        widths: [33, 34, 33, 0],
        name: "three_33_34_33",
        description: "3 columns - 33/34/33",
    },
    HeaderLayoutDef {
        columns: 3,
        widths: [25, 25, 50, 0],
        name: "three_25_25_50",
        description: "3 columns - 25/25/50",
    },
    HeaderLayoutDef {
        columns: 3,
        widths: [25, 50, 25, 0],
        name: "three_25_50_25",
        description: "3 columns - 25/50/25",
    },
    HeaderLayoutDef {
        columns: 3,
        widths: [50, 25, 25, 0],
        name: "three_50_25_25",
        description: "3 columns - 50/25/25",
    },
    HeaderLayoutDef {
        columns: 3,
        widths: [40, 30, 30, 0],
        name: "three_40_30_30",
        description: "3 columns - 40/30/30",
    },
    HeaderLayoutDef {
        columns: 3,
        widths: [30, 40, 30, 0],
        name: "three_30_40_30",
        description: "3 columns - 30/40/30",
    },
    HeaderLayoutDef {
        columns: 3,
        widths: [30, 30, 40, 0],
        name: "three_30_30_40",
        description: "3 columns - 30/30/40",
    },
    HeaderLayoutDef {
        columns: 3,
        widths: [40, 20, 40, 0],
        name: "three_40_20_40",
        description: "3 columns - 40/20/40",
    },
    HeaderLayoutDef {
        columns: 4,
        widths: [25, 25, 25, 25],
        name: "four_25_25_25_25",
        description: "4 columns - 25/25/25/25",
    },
];

/// Port of the `ScreenDefaults` descriptor (`Settings.h:29`). The four C
/// `const char*` members (any of which may be `NULL`) become
/// `Option<&str>`, matching both the `&'static` Platform default-screen
/// tables and the transient descriptor `Settings_read` builds inline from
/// a parsed `screen:` config line. Consumed by [`Settings_newScreen`].
pub struct ScreenDefaults<'a> {
    /// C `const char* name` — the screen's readable heading.
    pub name: Option<&'a str>,
    /// C `const char* columns` — the space-separated field list.
    pub columns: Option<&'a str>,
    /// C `const char* sortKey` — flat-view sort field name (`NULL` ⇒ `PID`).
    pub sortKey: Option<&'a str>,
    /// C `const char* treeSortKey` — tree-view sort field name (`NULL` ⇒ `PID`).
    pub treeSortKey: Option<&'a str>,
}

/// A subset of htop's `MeterColumnSetting` (`Settings.h:36`). The C
/// `char** names` is a NUL-terminated array; here it is an owned
/// `Vec<String>` wrapped in `Option` to distinguish "never set" (C
/// `NULL`) from "set to the empty list". `len` still counts *modes*, as
/// in C — it is written by `Settings_readMeterModes`, not by
/// `Settings_readMeters`.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct MeterColumnSetting {
    pub len: usize,
    pub names: Option<Vec<String>>,
    pub modes: Option<Vec<MeterModeId>>,
}

/// A subset of htop's `Settings` (`Settings.h:57`) holding the fields the
/// ported meter/layout/screen functions touch. The header layout and its
/// per-column meter settings, plus the `changed` dirty flag that
/// `Settings_setHeaderLayout` sets, plus the screen model.
///
/// `screens` fuses the C `ScreenSettings** screens` (a `NULL`-terminated
/// heap array) and its `unsigned int nScreens` count into one owned
/// `Vec<ScreenSettings>` — `screens.len()` is `nScreens`, and the C
/// `NULL` terminator is not modeled (Rust length-bounds the array).
/// `ssIndex` is the C `unsigned int ssIndex` (index of the active screen;
/// the C `ScreenSettings* ss` back-pointer is not modeled — the index
/// suffices and avoids a self-referential borrow). `lastUpdate` is the C
/// `uint64_t lastUpdate`. The `Settings.h` display toggles (`show*`/`hide*`/
/// `highlight*`, `colorScheme`, `delay`, …) are modeled below as their C
/// `bool`/`int` fields so the meter, machine, and process ports can read
/// them faithfully. The `char* filename`/`initialFilename` and the
/// `Hashtable* dynamicColumns/dynamicMeters/dynamicScreens` fields are still
/// omitted (they need the file-path and Hashtable substrate and no ported
/// reader touches them yet).
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// C `char* filename` — the resolved (realpath'd) config file path
    /// (`NULL` ⇒ `None`).
    pub filename: Option<String>,
    /// C `char* initialFilename` — the pre-realpath config path (`NULL` ⇒
    /// `None`).
    pub initialFilename: Option<String>,

    /// C `Hashtable* dynamicColumns` — runtime-discovered columns. A
    /// *borrowed* pointer owned by the `Machine`/`Platform` (passed into
    /// [`Settings_new`]); `None` = C `NULL`. Not freed by `Settings`.
    pub dynamicColumns: Option<*mut Hashtable>,
    /// C `Hashtable* dynamicMeters` — runtime-discovered meters (borrowed;
    /// `None` = `NULL`).
    pub dynamicMeters: Option<*mut Hashtable>,
    /// C `Hashtable* dynamicScreens` — runtime-discovered screens (borrowed;
    /// `None` = `NULL`).
    pub dynamicScreens: Option<*mut Hashtable>,

    pub hLayout: HeaderLayout,
    pub hColumns: Vec<MeterColumnSetting>,
    pub screens: Vec<ScreenSettings>,
    pub ssIndex: u32,

    // ---- Settings.h display toggles (bool unless noted) ----
    /// C `bool writeConfig` — write current settings on exit.
    pub writeConfig: bool,
    /// C `int config_version`.
    pub config_version: i32,
    /// C `int colorScheme`.
    pub colorScheme: i32,
    /// C `int delay` — update delay in tenths of a second.
    pub delay: i32,

    pub countCPUsFromOne: bool,
    pub detailedCPUTime: bool,
    pub showCPUUsage: bool,
    pub showCPUFrequency: bool,
    pub showCPUSMTLabels: bool,
    /// C `bool showCPUTemperature` (behind `BUILD_WITH_CPU_TEMP`).
    pub showCPUTemperature: bool,
    /// C `bool degreeFahrenheit` (behind `BUILD_WITH_CPU_TEMP`).
    pub degreeFahrenheit: bool,
    pub showProgramPath: bool,
    pub shadowOtherUsers: bool,
    pub showThreadNames: bool,
    pub hideKernelThreads: bool,
    pub hideRunningInContainer: bool,
    pub hideUserlandThreads: bool,
    pub highlightBaseName: bool,
    pub highlightDeletedExe: bool,
    pub shadowDistPathPrefix: bool,
    pub highlightMegabytes: bool,
    pub highlightThreads: bool,
    pub highlightChanges: bool,
    /// C `int highlightDelaySecs`.
    pub highlightDelaySecs: i32,
    pub findCommInCmdline: bool,
    pub stripExeFromCmdline: bool,
    pub showMergedCommand: bool,
    pub updateProcessNames: bool,
    pub accountGuestInCPUMeter: bool,
    pub headerMargin: bool,
    pub screenTabs: bool,
    pub showCachedMemory: bool,
    /// C `bool enableMouse` (behind `HAVE_GETMOUSE`).
    pub enableMouse: bool,
    /// C `int hideFunctionBar` — 0 off, 1 on-ESC-until-input, 2 permanent.
    pub hideFunctionBar: i32,
    /// C `bool topologyAffinity` (behind `HAVE_LIBHWLOC`).
    pub topologyAffinity: bool,

    pub changed: bool,
    pub lastUpdate: u64,
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

/// Port of `static void Settings_deleteColumns(Settings* this)` from
/// `Settings.c:35`. The C frees each header column's `names` string array
/// and `modes` array, then the `hColumns` array itself. Here each
/// [`MeterColumnSetting`] owns its `Vec`s, so clearing `hColumns` runs
/// their `Drop` — the faithful analog of the C `free` loop + `free`.
pub fn Settings_deleteColumns(this: &mut Settings) {
    this.hColumns.clear();
}

/// Port of `static void Settings_deleteScreens(Settings* this)` from
/// `Settings.c:43`. The C `ScreenSettings_delete`s each screen (guarded on
/// the `NULL`-terminated array) then frees the array. Here the owned
/// `Vec<ScreenSettings>` drops each element on `clear`, the faithful analog.
pub fn Settings_deleteScreens(this: &mut Settings) {
    this.screens.clear();
}

/// Port of `void Settings_delete(Settings* this)` from `Settings.c:51`.
/// The C frees `filename`/`initialFilename`, then the columns
/// ([`Settings_deleteColumns`]), the screens ([`Settings_deleteScreens`]),
/// and finally the struct. Modeled as by-value consume (the `FunctionBar_delete`
/// precedent): the `Option<String>` paths drop with `this`, the borrowed
/// `dynamicColumns`/`dynamicMeters`/`dynamicScreens` raw pointers are *not*
/// freed (owned by the `Machine`), and the struct drops at scope end.
pub fn Settings_delete(mut this: Settings) {
    // free(this->filename); free(this->initialFilename): owned Strings drop
    // with `this` below.
    Settings_deleteColumns(&mut this);
    Settings_deleteScreens(&mut this);
    // free(this): the struct drops here.
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

/// Port of `static const char* toFieldName(Hashtable* columns, int id, bool*
/// enabled)` from `Settings.c:181`. Maps a field id to its display name:
/// negative ids are disabled/`None`; dynamic ids (`>= ROW_DYNAMIC_FIELDS`,
/// i.e. `LAST_PROCESSFIELD`) resolve through `columns`; reserved ids index
/// the platform `Process_fields[]` table. `enabled` (C `bool*`) is an
/// optional out-flag.
pub fn toFieldName<'a>(
    columns: &'a Hashtable,
    id: i32,
    enabled: Option<&mut bool>,
) -> Option<&'a str> {
    if id < 0 {
        if let Some(e) = enabled {
            *e = false;
        }
        return None;
    }
    // ROW_DYNAMIC_FIELDS == LAST_RESERVED_FIELD == LAST_PROCESSFIELD.
    if id as usize >= LAST_PROCESSFIELD {
        let column = DynamicColumn_lookup(columns, id as u32);
        if let Some(e) = enabled {
            *e = column.is_some_and(|c| c.enabled);
        }
        return column.map(|c| c.name.as_str());
    }
    if let Some(e) = enabled {
        *e = true;
    }
    Some(Process_fields[id as usize].name)
}

/// Port of `static int toFieldIndex(Hashtable* columns, const char* str)` from
/// `Settings.c:198`. Resolves a config field token to a field id: a leading
/// digit is the old zero-based enum value (`atoi + 1`); a `Dynamic(<name>)`
/// token resolves through [`DynamicColumn_search`]; otherwise the field name
/// is matched against the table by-name. Returns `-1` when unresolved.
pub fn toFieldIndex(columns: &Hashtable, str_: &str) -> i32 {
    if str_.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        // "+1" for compatibility with the older enum format.
        let id = atoi(str_) + 1;
        if toFieldName(columns, id, None).is_some() {
            return id;
        }
    } else {
        // Dynamically-defined columns are stored by-name: `Dynamic(<name>)`.
        if let Some(after) = str_.strip_prefix("Dynamic(") {
            // C `sscanf(str, "Dynamic(%30s)", dynamic)`: up to 30 non-space
            // chars, then `strrchr(dynamic, ')')` truncates at the last ')'.
            let token: String = after
                .chars()
                .take_while(|c| !c.is_whitespace())
                .take(30)
                .collect();
            if let Some(pos) = token.rfind(')') {
                let name = &token[..pos];
                let mut key: u32 = 0;
                if DynamicColumn_search(Some(columns), name, Some(&mut key)).is_some() {
                    return key as i32;
                }
            }
        }
        // Fallback: iterative by-name scan of the reserved field table.
        for p in 1..LAST_PROCESSFIELD as i32 {
            if let Some(p_name) = toFieldName(columns, p, None) {
                if p_name == str_ {
                    return p;
                }
            }
        }
    }
    -1
}

/// Port of `static void ScreenSettings_readFields(ScreenSettings* ss,
/// Hashtable* columns, const char* line)` from `Settings.c:230`. Parses a
/// space-separated field list, replacing `ss.fields` with the resolved ids
/// and OR-ing each reserved field's scan flags into `ss.flags`. The C
/// `memset` + `xRealloc` array bookkeeping is subsumed by the `Vec`.
pub fn ScreenSettings_readFields(ss: &mut ScreenSettings, columns: &Hashtable, line: &str) {
    let trim = String_trim(line);
    let ids = String_split(&trim, ' ');

    // C resets the default fields (`memset(ss->fields, 0, …)`) then refills.
    ss.fields.clear();
    for id_str in &ids {
        let id = toFieldIndex(columns, id_str);
        if id >= 0 {
            ss.fields.push(id);
        }
        if id > 0 && (id as usize) < LAST_PROCESSFIELD {
            ss.flags |= Process_fields[id as usize].flags;
        }
    }
}

/// Port of `static ScreenSettings* Settings_initScreenSettings(ScreenSettings*
/// ss, Settings* this, const char* columns)` from `Settings.c:254`. Parses
/// the `columns` field list into `ss` via [`ScreenSettings_readFields`],
/// then appends `ss` to the screen array (the C `screens[nScreens] = ss;
/// nScreens++; xRealloc; screens[nScreens] = NULL` — the owned `Vec` push
/// subsumes the realloc + `NULL` terminator). Returns the new screen's index
/// (the Rust analog of the C `ScreenSettings*` return; the pointer-graph
/// model addresses screens by index).
///
/// The C dereferences `this->dynamicColumns` unconditionally; here a `None`
/// borrowed pointer (C `NULL`) is guarded — the field list is left empty
/// rather than dereferencing null.
pub fn Settings_initScreenSettings(
    this: &mut Settings,
    mut ss: ScreenSettings,
    columns: &str,
) -> usize {
    match this.dynamicColumns {
        Some(p) => ScreenSettings_readFields(&mut ss, unsafe { &*p }, columns),
        None => ss.fields.clear(),
    }
    this.screens.push(ss);
    this.screens.len() - 1
}

/// Port of `ScreenSettings* Settings_newScreen(Settings* this, const
/// ScreenDefaults* defaults)` from `Settings.c:263`. Resolves the flat/tree
/// sort-key field names to ids ([`toFieldIndex`], `PID` when the descriptor
/// leaves them `NULL`), derives the default direction from
/// `Process_fields[sortKey].defaultSortDesc`, builds the `ScreenSettings`,
/// and appends it via [`Settings_initScreenSettings`]. Returns the new
/// screen's index. The C `xCalloc(LAST_PROCESSFIELD, ...)` for `fields`
/// becomes an empty `Vec` ([`Settings_initScreenSettings`] fills it).
pub fn Settings_newScreen(this: &mut Settings, defaults: &ScreenDefaults) -> usize {
    // int sortKey = defaults->sortKey ? toFieldIndex(this->dynamicColumns, defaults->sortKey) : PID;
    let resolve = |name: &str| -> RowField {
        match this.dynamicColumns {
            Some(p) => toFieldIndex(unsafe { &*p }, name),
            None => -1,
        }
    };
    let sortKey = match defaults.sortKey {
        Some(sk) => resolve(sk),
        None => PID,
    };
    let treeSortKey = match defaults.treeSortKey {
        Some(tsk) => resolve(tsk),
        None => PID,
    };
    let sortDesc = if sortKey >= 0 && (sortKey as usize) < LAST_PROCESSFIELD {
        Process_fields[sortKey as usize].defaultSortDesc
    } else {
        true // C: `: 1`
    };

    let ss = ScreenSettings {
        heading: defaults.name.map(|s| s.to_string()),
        dynamic: None,
        table: None,
        fields: Vec::new(),
        flags: 0,
        direction: if sortDesc { -1 } else { 1 },
        treeDirection: 1,
        sortKey,
        treeSortKey,
        treeView: false,
        treeViewAlwaysByPID: false,
        allBranchesCollapsed: false,
        ..Default::default()
    };
    Settings_initScreenSettings(this, ss, defaults.columns.unwrap_or(""))
}

/// Port of `ScreenSettings* Settings_newDynamicScreen(Settings* this,
/// const char* tab, const DynamicScreen* screen, Table* table)` from
/// `Settings.c:286`. Builds a `ScreenSettings` for a runtime-discovered
/// dynamic screen: resolve the sort key from `screen->columnKeys` against the
/// borrowed `dynamicColumns` table, seed heading/dynamic-name/table/direction,
/// and hand off to [`Settings_initScreenSettings`] (which reads the field list
/// from `columnKeys`). Returns the new screen's index (the C returns the
/// `ScreenSettings*`; the port models the store as the `screens` `Vec`, so the
/// index is the stable handle — matching [`Settings_newScreen`]). The C
/// `Table*` maps to the borrowed [`TableHandle`] raw pointer.
pub fn Settings_newDynamicScreen(
    this: &mut Settings,
    tab: &str,
    screen: &DynamicScreen,
    table: Option<TableHandle>,
) -> usize {
    let column_keys = screen.columnKeys.get().map(String::as_str).unwrap_or("");
    // int sortKey = toFieldIndex(this->dynamicColumns, screen->columnKeys);
    let sortKey = match this.dynamicColumns {
        Some(p) => toFieldIndex(unsafe { &*p }, column_keys),
        None => -1,
    };
    // *ss = (ScreenSettings){ .heading = tab, .dynamic = screen->name,
    //   .table = table, .fields = xCalloc(LAST_PROCESSFIELD),
    //   .direction = screen->direction, .treeDirection = 1, .sortKey = sortKey };
    // The `fields` array is filled by Settings_initScreenSettings' readFields.
    let ss = ScreenSettings {
        heading: Some(tab.to_string()),
        dynamic: Some(screen.name.clone()),
        table,
        fields: Vec::new(),
        direction: screen.direction,
        treeDirection: 1,
        sortKey,
        ..Default::default()
    };
    // return Settings_initScreenSettings(ss, this, screen->columnKeys);
    Settings_initScreenSettings(this, ss, column_keys)
}

/// Port of `void ScreenSettings_delete(ScreenSettings* this)` from
/// `Settings.c:302`. The C frees `heading`/`dynamic`/`fields` and the
/// struct; modeled as by-value consume (the `FunctionBar_delete`
/// precedent) — the owned `Option<String>`/`Vec` fields drop with `this`.
pub fn ScreenSettings_delete(this: ScreenSettings) {
    // free(heading); free(dynamic); free(fields); free(this): owned fields
    // drop with `this`.
    let _ = this;
}

/// Port of `static ScreenSettings* Settings_defaultScreens(Settings* this)`
/// from `Settings.c:309`. If any screen already exists it returns the first
/// (`screens[0]`, index `0`); otherwise it builds the platform default
/// screens via [`Settings_newScreen`] and returns the first.
///
/// The C iterates `Platform_defaultScreens[0..Platform_numberOfDefaultScreens]`.
/// The darwin table (`darwin/Platform.c:67`) is a single entry
/// (`{ "Main", "PID USER PRIORITY NICE M_VIRT M_RESIDENT STATE PERCENT_CPU
/// PERCENT_MEM TIME Command", "PERCENT_CPU" }`); it is inlined here because
/// the darwin `Platform` screens array is not separately ported. The C
/// `Platform_defaultDynamicScreens(this)` call is a no-op on stock darwin
/// (no dynamic screens without PCP) and is skipped — see port note below.
/// Returns `0` (the index of `screens[0]`), the pointer-graph analog of the
/// C `ScreenSettings*` return.
pub fn Settings_defaultScreens(this: &mut Settings) -> usize {
    // if (this->nScreens) return this->screens[0];
    if !this.screens.is_empty() {
        return 0;
    }
    // for (i = 0; i < Platform_numberOfDefaultScreens; i++)
    //    Settings_newScreen(this, &Platform_defaultScreens[i]);
    // darwin/Platform.c:67 — one entry, inlined (darwin Platform screens
    // table not separately ported).
    Settings_newScreen(
        this,
        &ScreenDefaults {
            name: Some("Main"),
            columns: Some(
                "PID USER PRIORITY NICE M_VIRT M_RESIDENT STATE PERCENT_CPU PERCENT_MEM TIME Command",
            ),
            sortKey: Some("PERCENT_CPU"),
            treeSortKey: None,
        },
    );
    // Port note: Platform_defaultDynamicScreens(this) is empty on stock
    // darwin (no dynamic screens without PCP) — skipped.
    0
}

/// Port of `static bool Settings_read(Settings* this, const char* fileName,
/// const Machine* host, bool checkWritability)` from `Settings.c:320`.
///
/// Opens `fileName`, determines `writeConfig`, and parses each `key=value`
/// line, dispatching to the ported field/meter/screen handlers. The C
/// `open(O_RDWR|O_NOCTTY|O_NOFOLLOW)` writability probe and the read-only
/// fallback are reproduced via `OpenOptions` with the same `custom_flags`.
/// The C `String_readLine` loop is a byte-line iteration over the file text;
/// `String_split(line, '=')` is the ported [`String_split`], so `option[1]`
/// is the text between the first and second `=` exactly as in C.
///
/// The `.dynamic` config branch (`Settings.c:557`) sets `screen->dynamic`
/// (portable) but its `Platform_addDynamicScreen(screen)` call is skipped —
/// see the port note — because no platform module ports that symbol. Every
/// other branch is faithful; the `#ifdef HAVE_LIBHWLOC topology_affinity`
/// branch is omitted (that build flag is not defined). Unknown option keys
/// are ignored, matching the C `else`-less if-chain.
pub fn Settings_read(
    this: &mut Settings,
    fileName: &str,
    host: &Machine,
    checkWritability: bool,
) -> bool {
    use std::fs::OpenOptions;
    use std::io::{ErrorKind, Read};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

    // int fd = -1; const char* fopen_mode = "r+";
    let mut file: Option<std::fs::File> = None;

    if checkWritability {
        // do { fd = open(fileName, O_RDWR | O_NOCTTY | O_NOFOLLOW); }
        // while (fd < 0 && errno == EINTR);
        match OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_NOFOLLOW)
            .open(fileName)
        {
            Ok(f) => {
                // this->writeConfig = !err && S_ISREG(sb.st_mode)
                //   && (sb.st_mode & S_IWUSR) && sb.st_uid == geteuid();
                this.writeConfig = match f.metadata() {
                    Ok(sb) => {
                        // `S_IWUSR` is `mode_t` = u32 on Linux (cast is a no-op)
                        // but u16 on macOS (cast is required), so the cast can't
                        // be dropped without breaking darwin.
                        #[allow(clippy::unnecessary_cast)]
                        let writable = (sb.permissions().mode() & (libc::S_IWUSR as u32)) != 0;
                        sb.file_type().is_file()
                            && writable
                            && sb.uid() == unsafe { libc::geteuid() }
                    }
                    Err(_) => false,
                };
                file = Some(f);
            }
            Err(e) => {
                // this->writeConfig = (errno == ENOENT);
                this.writeConfig = e.kind() == ErrorKind::NotFound;
                // if (errno != EACCES && errno != EPERM && errno != EROFS)
                //    return false;
                match e.raw_os_error() {
                    Some(code)
                        if code == libc::EACCES || code == libc::EPERM || code == libc::EROFS => {}
                    _ => return false,
                }
            }
        }
    }

    // If opening for read & write is not needed or fails, open for read only.
    // if (fd < 0) { fopen_mode = "r"; fd = open(fileName, O_RDONLY | O_NOCTTY); }
    if file.is_none() {
        match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOCTTY)
            .open(fileName)
        {
            Ok(f) => file = Some(f),
            // if (fd < 0) return false;
            Err(_) => return false,
        }
    }

    let mut fp = match file {
        Some(f) => f,
        None => return false,
    };

    // Slurp the file; the per-line loop below plays the role of the C
    // String_readLine(fp) loop.
    let mut contents = String::new();
    if fp.read_to_string(&mut contents).is_err() {
        return false;
    }

    // ScreenSettings* screen = NULL; — modeled as an index into `this.screens`.
    let mut screen: Option<usize> = None;
    let mut didReadMeters = false;
    let mut didReadAny = false;

    for line in contents.lines() {
        didReadAny = true;

        // char** option = String_split(line, '=', &nOptions);
        let option = String_split(line, '=');
        // if (nOptions < 2) continue;
        if option.len() < 2 {
            continue;
        }
        let key = option[0].as_str();
        let val = option[1].as_str();

        if String_eq(key, "config_reader_min_version") {
            this.config_version = atoi(val);
            if this.config_version > CONFIG_READER_MIN_VERSION {
                // the version of the config file on disk is newer than we can read
                eprintln!("WARNING: {fileName} specifies configuration format");
                eprintln!(
                    "         version v{}, but this htop binary only supports up to version v{}.",
                    this.config_version, CONFIG_READER_MIN_VERSION
                );
                eprintln!(
                    "         The configuration file will be downgraded to v{CONFIG_READER_MIN_VERSION} when htop exits."
                );
                return false;
            }
        } else if String_eq(key, "fields") && this.config_version <= 2 {
            // old (no screen) naming, for backwards compatibility
            let idx = Settings_defaultScreens(this);
            screen = Some(idx);
            if let Some(p) = this.dynamicColumns {
                ScreenSettings_readFields(&mut this.screens[idx], unsafe { &*p }, val);
            }
        } else if String_eq(key, "sort_key") && this.config_version <= 2 {
            // "+1" is for compatibility with the older enum format.
            let idx = Settings_defaultScreens(this);
            screen = Some(idx);
            this.screens[idx].sortKey = atoi(val) + 1;
        } else if String_eq(key, "tree_sort_key") && this.config_version <= 2 {
            let idx = Settings_defaultScreens(this);
            screen = Some(idx);
            this.screens[idx].treeSortKey = atoi(val) + 1;
        } else if String_eq(key, "sort_direction") && this.config_version <= 2 {
            let idx = Settings_defaultScreens(this);
            screen = Some(idx);
            this.screens[idx].direction = atoi(val);
        } else if String_eq(key, "tree_sort_direction") && this.config_version <= 2 {
            let idx = Settings_defaultScreens(this);
            screen = Some(idx);
            this.screens[idx].treeDirection = atoi(val);
        } else if String_eq(key, "tree_view") && this.config_version <= 2 {
            let idx = Settings_defaultScreens(this);
            screen = Some(idx);
            this.screens[idx].treeView = atoi(val) != 0;
        } else if String_eq(key, "tree_view_always_by_pid") && this.config_version <= 2 {
            let idx = Settings_defaultScreens(this);
            screen = Some(idx);
            this.screens[idx].treeViewAlwaysByPID = atoi(val) != 0;
        } else if String_eq(key, "all_branches_collapsed") && this.config_version <= 2 {
            let idx = Settings_defaultScreens(this);
            screen = Some(idx);
            this.screens[idx].allBranchesCollapsed = atoi(val) != 0;
        } else if String_eq(key, "hide_kernel_threads") {
            this.hideKernelThreads = atoi(val) != 0;
        } else if String_eq(key, "hide_userland_threads") {
            this.hideUserlandThreads = atoi(val) != 0;
        } else if String_eq(key, "hide_running_in_container") {
            this.hideRunningInContainer = atoi(val) != 0;
        } else if String_eq(key, "shadow_other_users") {
            this.shadowOtherUsers = atoi(val) != 0;
        } else if String_eq(key, "show_thread_names") {
            this.showThreadNames = atoi(val) != 0;
        } else if String_eq(key, "show_program_path") {
            this.showProgramPath = atoi(val) != 0;
        } else if String_eq(key, "highlight_base_name") {
            this.highlightBaseName = atoi(val) != 0;
        } else if String_eq(key, "highlight_deleted_exe") {
            this.highlightDeletedExe = atoi(val) != 0;
        } else if String_eq(key, "shadow_distribution_path_prefix") {
            this.shadowDistPathPrefix = atoi(val) != 0;
        } else if String_eq(key, "highlight_megabytes") {
            this.highlightMegabytes = atoi(val) != 0;
        } else if String_eq(key, "highlight_threads") {
            this.highlightThreads = atoi(val) != 0;
        } else if String_eq(key, "highlight_changes") {
            this.highlightChanges = atoi(val) != 0;
        } else if String_eq(key, "highlight_changes_delay_secs") {
            this.highlightDelaySecs = atoi(val).clamp(1, 24 * 60 * 60);
        } else if String_eq(key, "find_comm_in_cmdline") {
            this.findCommInCmdline = atoi(val) != 0;
        } else if String_eq(key, "strip_exe_from_cmdline") {
            this.stripExeFromCmdline = atoi(val) != 0;
        } else if String_eq(key, "show_merged_command") {
            this.showMergedCommand = atoi(val) != 0;
        } else if String_eq(key, "header_margin") {
            this.headerMargin = atoi(val) != 0;
        } else if String_eq(key, "screen_tabs") {
            this.screenTabs = atoi(val) != 0;
        } else if String_eq(key, "expand_system_time") {
            // Compatibility option.
            this.detailedCPUTime = atoi(val) != 0;
        } else if String_eq(key, "detailed_cpu_time") {
            this.detailedCPUTime = atoi(val) != 0;
        } else if String_eq(key, "cpu_count_from_one") {
            this.countCPUsFromOne = atoi(val) != 0;
        } else if String_eq(key, "cpu_count_from_zero") {
            // old (inverted) naming, for backwards compatibility
            this.countCPUsFromOne = atoi(val) == 0;
        } else if String_eq(key, "show_cpu_smt_labels") {
            this.showCPUSMTLabels = atoi(val) != 0;
        } else if String_eq(key, "show_cpu_usage") {
            this.showCPUUsage = atoi(val) != 0;
        } else if String_eq(key, "show_cpu_frequency") {
            this.showCPUFrequency = atoi(val) != 0;
        } else if String_eq(key, "show_cached_memory") {
            this.showCachedMemory = atoi(val) != 0;
        } else if String_eq(key, "show_cpu_temperature") {
            // BUILD_WITH_CPU_TEMP
            this.showCPUTemperature = atoi(val) != 0;
        } else if String_eq(key, "degree_fahrenheit") {
            // BUILD_WITH_CPU_TEMP
            this.degreeFahrenheit = atoi(val) != 0;
        } else if String_eq(key, "update_process_names") {
            this.updateProcessNames = atoi(val) != 0;
        } else if String_eq(key, "account_guest_in_cpu_meter") {
            this.accountGuestInCPUMeter = atoi(val) != 0;
        } else if String_eq(key, "delay") {
            this.delay = atoi(val).clamp(1, 255);
        } else if String_eq(key, "color_scheme") {
            this.colorScheme = atoi(val);
            if this.colorScheme < 0 || this.colorScheme >= ColorScheme::LAST_COLORSCHEME as i32 {
                this.colorScheme = 0;
            }
        } else if String_eq(key, "enable_mouse") {
            // HAVE_GETMOUSE
            this.enableMouse = atoi(val) != 0;
        } else if String_eq(key, "header_layout") {
            // isdigit(option[1][0]) ? (HeaderLayout)atoi(option[1])
            //                       : HeaderLayout_fromName(option[1]);
            let layout = if val.as_bytes().first().is_some_and(u8::is_ascii_digit) {
                let n = atoi(val);
                if n >= 0 && (n as usize) < HeaderLayout::LAST_HEADER_LAYOUT as usize {
                    HEADER_LAYOUTS_IN_ORDER[n as usize]
                } else {
                    // Out of range: falls through to the HF_TWO_50_50 fixup.
                    HeaderLayout::HF_INVALID
                }
            } else {
                HeaderLayout_fromName(val)
            };
            // if (hLayout < 0 || hLayout >= LAST_HEADER_LAYOUT) hLayout = HF_TWO_50_50;
            this.hLayout = if (layout as i32) < 0
                || (layout as i32) >= HeaderLayout::LAST_HEADER_LAYOUT as i32
            {
                HeaderLayout::HF_TWO_50_50
            } else {
                layout
            };
            // free(hColumns); hColumns = xCalloc(HeaderLayout_getColumns(hLayout), ...);
            this.hColumns =
                vec![MeterColumnSetting::default(); HeaderLayout_getColumns(this.hLayout)];
        } else if String_eq(key, "left_meters") {
            Settings_readMeters(this, val, 0);
            didReadMeters = true;
        } else if String_eq(key, "right_meters") {
            Settings_readMeters(this, val, 1);
            didReadMeters = true;
        } else if String_eq(key, "left_meter_modes") {
            Settings_readMeterModes(this, val, 0);
            didReadMeters = true;
        } else if String_eq(key, "right_meter_modes") {
            Settings_readMeterModes(this, val, 1);
            didReadMeters = true;
        } else if String_startsWith(key, "column_meters_") {
            // C passes atoi() into the `unsigned int column` param (negatives
            // wrap 32-bit); Settings_readMeters then clamps to the last column.
            let col = atoi(&key["column_meters_".len()..]) as u32 as usize;
            Settings_readMeters(this, val, col);
            didReadMeters = true;
        } else if String_startsWith(key, "column_meter_modes_") {
            let col = atoi(&key["column_meter_modes_".len()..]) as u32 as usize;
            Settings_readMeterModes(this, val, col);
            didReadMeters = true;
        } else if String_eq(key, "hide_function_bar") {
            this.hideFunctionBar = atoi(val);
        } else if String_startsWith(key, "screen:") {
            // screen = Settings_newScreen(this,
            //    &(ScreenDefaults){ .name = option[0]+7, .columns = option[1] });
            let name = &key[7..];
            let idx = Settings_newScreen(
                this,
                &ScreenDefaults {
                    name: Some(name),
                    columns: Some(val),
                    sortKey: None,
                    treeSortKey: None,
                },
            );
            screen = Some(idx);
        } else if String_eq(key, ".sort_key") {
            if let Some(idx) = screen {
                let k = match this.dynamicColumns {
                    Some(p) => toFieldIndex(unsafe { &*p }, val),
                    None => -1,
                };
                this.screens[idx].sortKey = if k > 0 { k } else { PID };
            }
        } else if String_eq(key, ".tree_sort_key") {
            if let Some(idx) = screen {
                let k = match this.dynamicColumns {
                    Some(p) => toFieldIndex(unsafe { &*p }, val),
                    None => -1,
                };
                this.screens[idx].treeSortKey = if k > 0 { k } else { PID };
            }
        } else if String_eq(key, ".sort_direction") {
            if let Some(idx) = screen {
                this.screens[idx].direction = atoi(val);
            }
        } else if String_eq(key, ".tree_sort_direction") {
            if let Some(idx) = screen {
                this.screens[idx].treeDirection = atoi(val);
            }
        } else if String_eq(key, ".tree_view") {
            if let Some(idx) = screen {
                this.screens[idx].treeView = atoi(val) != 0;
            }
        } else if String_eq(key, ".tree_view_always_by_pid") {
            if let Some(idx) = screen {
                this.screens[idx].treeViewAlwaysByPID = atoi(val) != 0;
            }
        } else if String_eq(key, ".all_branches_collapsed") {
            if let Some(idx) = screen {
                this.screens[idx].allBranchesCollapsed = atoi(val) != 0;
            }
        } else if String_eq(key, ".dynamic") {
            if let Some(idx) = screen {
                // free_and_xStrdup(&screen->dynamic, option[1]);
                this.screens[idx].dynamic = Some(val.to_string());
                // Port note: dynamic screens need Platform_addDynamicScreen
                // (unported) — the Platform_addDynamicScreen(screen) call is
                // skipped.
            }
        }
        // (unknown option keys are ignored, as in the C else-less if-chain)
    }
    // fclose(fp): `fp` drops at scope end.

    if !didReadMeters || !Settings_validateMeters(this) {
        Settings_defaultMeters(this, host);
    }
    if this.screens.is_empty() {
        Settings_defaultScreens(this);
    }
    didReadAny
}

/// Port of `static void writeFields(OutputFunc of, FILE* fp, const
/// ProcessField* fields, Hashtable* columns, bool byName, char separator)`
/// from `Settings.c:575`. Writes the screen's field list: reserved fields
/// (`field < LAST_PROCESSFIELD`) are written by their [`toFieldName`] name
/// when `byName`, dynamic fields (`>= LAST_PROCESSFIELD`) as
/// `Dynamic(<name>)` when their column is enabled, and otherwise the
/// numeric `field - 1` (the older zero-based enum form). The C
/// `OutputFunc of` / `FILE* fp` sink is modeled as a `&mut String` buffer,
/// matching the sibling [`writeList`] / [`writeMeters`] ports.
///
/// The C `for (i = 0; fields[i]; i++)` stops at the array's trailing `0`
/// terminator; the owned `Vec<RowField>` holds exactly the resolved field
/// ids with no terminator (see [`ScreenSettings_readFields`], which never
/// pushes `0`), so iterating the whole slice is the faithful analog. The
/// `< / >=` comparisons are signed (`i32`), matching the C `ProcessField`
/// (`int`) comparison. A `None` name from `toFieldName` in the reserved
/// branch (unreachable for a valid reserved id) writes nothing, mirroring
/// C passing a would-be non-`NULL` `%s`.
pub fn writeFields(
    out: &mut String,
    fields: &[RowField],
    columns: &Hashtable,
    byName: bool,
    separator: char,
) {
    let mut sep = "";
    for &field in fields {
        if field < LAST_PROCESSFIELD as i32 && byName {
            let pName = toFieldName(columns, field, None);
            // C: of(fp, "%s%s", sep, pName)
            out.push_str(sep);
            out.push_str(pName.unwrap_or(""));
        } else if field >= LAST_PROCESSFIELD as i32 && byName {
            let mut enabled = false;
            let pName = toFieldName(columns, field, Some(&mut enabled));
            if enabled {
                // C: of(fp, "%sDynamic(%s)", sep, pName)
                out.push_str(sep);
                out.push_str("Dynamic(");
                out.push_str(pName.unwrap_or(""));
                out.push(')');
            }
        } else {
            // This "-1" is for compatibility with the older enum format.
            // C: of(fp, "%s%d", sep, (int) fields[i] - 1)
            out.push_str(sep);
            out.push_str(&(field - 1).to_string());
        }
        sep = " ";
    }
    out.push(separator);
}

/// Port of `Settings.c:603`. Appends `list[0..len]` to `out`,
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

/// Port of `Settings.c:613`. Writes column `column`'s meter names via
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

/// Port of `Settings.c:622`. Writes column `column`'s meter modes as
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

/// Port of `full_write` (`XUtils.c:344`) — the retry-on-`EINTR` write loop
/// draining `buf` to `fd`. Returns the byte count written, or the negative
/// `write(2)` return on a non-`EINTR` error. Private (as in C's `XUtils`);
/// used by [`signal_safe_fprintf`] and [`Settings_write`].
fn full_write(fd: libc::c_int, mut buf: &[u8]) -> libc::ssize_t {
    let mut written: libc::ssize_t = 0;
    while !buf.is_empty() {
        let r = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
        if r < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return r;
        }
        if r == 0 {
            break;
        }
        written += r;
        buf = &buf[r as usize..];
    }
    written
}

/// Port of `static int signal_safe_fprintf(FILE* stream, const char* fmt,
/// ...)` from `Settings.c:632`. The C `vsnprintf`s the varargs into a
/// 2048-byte stack buffer then `full_write_str(fileno(stream), buf)`s it —
/// the async-signal-safe writer used on the crash path (`stderr`). The
/// ported meter/field writers accumulate their output into a `&mut String`
/// (see [`writeFields`]/[`writeList`]), so here the caller ([`Settings_write`])
/// hands the already-formatted text and a raw fd; this signal-safe-writes it
/// via `full_write`. The per-call 2048-byte truncation is not modeled — the
/// batched `String` is written whole. The result is clamped to `INT_MAX`
/// (C `MINIMUM(INT_MAX, ret)`).
pub fn signal_safe_fprintf(fd: libc::c_int, s: &str) -> i32 {
    let ret = full_write(fd, s.as_bytes());
    ret.min(i32::MAX as libc::ssize_t) as i32
}

/// Port of the C `printSettingInteger` macro (`Settings.c:678`) —
/// `of(fp, "<setting>=%d%c", value, separator)`. Private helper (the C
/// form is a `#define`); the `OutputFunc`/`FILE*` sink is the accumulating
/// `&mut String` buffer.
fn printSettingInteger(out: &mut String, separator: char, setting: &str, value: i32) {
    out.push_str(setting);
    out.push('=');
    out.push_str(&value.to_string());
    out.push(separator);
}

/// Port of the C `printSettingString` macro (`Settings.c:680`) —
/// `of(fp, "<setting>=%s%c", value, separator)`. Private helper.
fn printSettingString(out: &mut String, separator: char, setting: &str, value: &str) {
    out.push_str(setting);
    out.push('=');
    out.push_str(value);
    out.push(separator);
}

/// Port of `int Settings_write(const Settings* this, bool onCrash)` from
/// `Settings.c:647`. Serializes the full settings to the config text and
/// commits it: on the crash path (`onCrash`) it writes to `stderr` with a
/// `;` separator via [`signal_safe_fprintf`]; otherwise (when `writeConfig`)
/// it `mkstemp`s `<filename>.tmp.XXXXXX` (mode `0600`), writes with a `\n`
/// separator, then atomically `rename`s over `filename`. The C `OutputFunc`
/// / `FILE*` sink is modeled by building the text into a single `String`
/// buffer and flushing it once (equivalent output; see the sibling
/// [`writeFields`]/[`writeMeters`] ports). Returns `0` on success or a
/// negative `errno` on failure, as in C.
///
/// The borrowed `dynamicColumns` pointer (owned by the `Machine`) must be
/// set — the field writers resolve dynamic-column names through it; a `None`
/// (C `NULL`) is a programming error (`Settings_new` always sets it) and
/// panics rather than dereferencing null.
pub fn Settings_write(this: &Settings, onCrash: bool) -> i32 {
    let separator: char;
    let mut tmpFilename: Option<String> = None;
    let mut fdtmp: libc::c_int = -1;

    if onCrash {
        separator = ';';
    } else if !this.writeConfig {
        return 0;
    } else {
        // create tempfile with mode 0600
        let cur_umask = unsafe { libc::umask(libc::S_IXUSR | libc::S_IRWXG | libc::S_IRWXO) };
        let template = format!("{}.tmp.XXXXXX", this.filename.as_deref().unwrap_or(""));
        // NUL-terminated, mutable: mkstemp overwrites the trailing "XXXXXX".
        let mut tmpl: Vec<u8> = template.into_bytes();
        tmpl.push(0);
        fdtmp = unsafe { libc::mkstemp(tmpl.as_mut_ptr() as *mut libc::c_char) };
        unsafe { libc::umask(cur_umask) };
        if fdtmp == -1 {
            return -std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        }
        // Recover the concrete temp path mkstemp filled in.
        let name = CStr::from_bytes_with_nul(&tmpl[..])
            .ok()
            .and_then(|c| c.to_str().ok())
            .map(str::to_string);
        tmpFilename = name;
        separator = '\n';
    }

    let columns: &Hashtable = unsafe {
        &*this
            .dynamicColumns
            .expect("Settings.dynamicColumns must be set (owned by Machine)")
    };

    let mut buf = String::new();

    if !onCrash {
        buf.push_str(
            "# Beware! This file is rewritten by htop when settings are changed in the interface.\n",
        );
        buf.push_str("# The parser is also very primitive, and not human-friendly.\n");
    }
    printSettingString(
        &mut buf,
        separator,
        "htop_version",
        env!("CARGO_PKG_VERSION"),
    );
    printSettingInteger(
        &mut buf,
        separator,
        "config_reader_min_version",
        CONFIG_READER_MIN_VERSION,
    );
    buf.push_str("fields=");
    writeFields(&mut buf, &this.screens[0].fields, columns, false, separator);
    printSettingInteger(
        &mut buf,
        separator,
        "hide_kernel_threads",
        this.hideKernelThreads as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "hide_userland_threads",
        this.hideUserlandThreads as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "hide_running_in_container",
        this.hideRunningInContainer as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "shadow_other_users",
        this.shadowOtherUsers as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "show_thread_names",
        this.showThreadNames as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "show_program_path",
        this.showProgramPath as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "highlight_base_name",
        this.highlightBaseName as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "highlight_deleted_exe",
        this.highlightDeletedExe as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "shadow_distribution_path_prefix",
        this.shadowDistPathPrefix as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "highlight_megabytes",
        this.highlightMegabytes as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "highlight_threads",
        this.highlightThreads as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "highlight_changes",
        this.highlightChanges as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "highlight_changes_delay_secs",
        this.highlightDelaySecs,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "find_comm_in_cmdline",
        this.findCommInCmdline as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "strip_exe_from_cmdline",
        this.stripExeFromCmdline as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "show_merged_command",
        this.showMergedCommand as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "header_margin",
        this.headerMargin as i32,
    );
    printSettingInteger(&mut buf, separator, "screen_tabs", this.screenTabs as i32);
    printSettingInteger(
        &mut buf,
        separator,
        "detailed_cpu_time",
        this.detailedCPUTime as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "cpu_count_from_one",
        this.countCPUsFromOne as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "show_cpu_smt_labels",
        this.showCPUSMTLabels as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "show_cpu_usage",
        this.showCPUUsage as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "show_cpu_frequency",
        this.showCPUFrequency as i32,
    );
    // BUILD_WITH_CPU_TEMP fields (show_cpu_temperature/degree_fahrenheit) are
    // gated out of this platform build, matching the C `#ifdef`.
    printSettingInteger(
        &mut buf,
        separator,
        "show_cached_memory",
        this.showCachedMemory as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "update_process_names",
        this.updateProcessNames as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "account_guest_in_cpu_meter",
        this.accountGuestInCPUMeter as i32,
    );
    printSettingInteger(&mut buf, separator, "color_scheme", this.colorScheme);
    printSettingInteger(&mut buf, separator, "enable_mouse", this.enableMouse as i32);
    printSettingInteger(&mut buf, separator, "delay", this.delay);
    printSettingInteger(
        &mut buf,
        separator,
        "hide_function_bar",
        this.hideFunctionBar,
    );
    // HAVE_LIBHWLOC field (topology_affinity) is gated out, matching the C `#ifdef`.

    printSettingString(
        &mut buf,
        separator,
        "header_layout",
        HeaderLayout_getName(this.hLayout),
    );
    for i in 0..HeaderLayout_getColumns(this.hLayout) {
        buf.push_str(&format!("column_meters_{i}="));
        writeMeters(this, &mut buf, separator, i);
        buf.push_str(&format!("column_meter_modes_{i}="));
        writeMeterModes(this, &mut buf, separator, i);
    }

    // Legacy compatibility with older versions of htop
    let s0 = &this.screens[0];
    printSettingInteger(&mut buf, separator, "tree_view", s0.treeView as i32);
    // This "-1" is for compatibility with the older enum format.
    printSettingInteger(&mut buf, separator, "sort_key", s0.sortKey - 1);
    printSettingInteger(&mut buf, separator, "tree_sort_key", s0.treeSortKey - 1);
    printSettingInteger(&mut buf, separator, "sort_direction", s0.direction);
    printSettingInteger(&mut buf, separator, "tree_sort_direction", s0.treeDirection);
    printSettingInteger(
        &mut buf,
        separator,
        "tree_view_always_by_pid",
        s0.treeViewAlwaysByPID as i32,
    );
    printSettingInteger(
        &mut buf,
        separator,
        "all_branches_collapsed",
        s0.allBranchesCollapsed as i32,
    );

    for i in 0..this.screens.len() {
        let ss = &this.screens[i];
        let sortKey = toFieldName(columns, ss.sortKey, None).unwrap_or("");
        let treeSortKey = toFieldName(columns, ss.treeSortKey, None).unwrap_or("");

        buf.push_str(&format!("screen:{}=", ss.heading.as_deref().unwrap_or("")));
        writeFields(&mut buf, &ss.fields, columns, true, separator);
        if let Some(dynamic) = ss.dynamic.as_deref() {
            printSettingString(&mut buf, separator, ".dynamic", dynamic);
            if ss.sortKey != 0 && ss.sortKey != PID {
                buf.push_str(&format!(".sort_key=Dynamic({sortKey}){separator}"));
            }
            if ss.treeSortKey != 0 && ss.treeSortKey != PID {
                buf.push_str(&format!(".tree_sort_key=Dynamic({treeSortKey}){separator}"));
            }
        } else {
            printSettingString(&mut buf, separator, ".sort_key", sortKey);
            printSettingString(&mut buf, separator, ".tree_sort_key", treeSortKey);
            printSettingInteger(
                &mut buf,
                separator,
                ".tree_view_always_by_pid",
                ss.treeViewAlwaysByPID as i32,
            );
        }
        printSettingInteger(&mut buf, separator, ".tree_view", ss.treeView as i32);
        printSettingInteger(&mut buf, separator, ".sort_direction", ss.direction);
        printSettingInteger(
            &mut buf,
            separator,
            ".tree_sort_direction",
            ss.treeDirection,
        );
        printSettingInteger(
            &mut buf,
            separator,
            ".all_branches_collapsed",
            ss.allBranchesCollapsed as i32,
        );
    }

    if onCrash {
        signal_safe_fprintf(libc::STDERR_FILENO, &buf);
        return 0;
    }

    // Flush the buffer to the temp file, then atomically rename into place.
    let mut r: i32 = 0;
    let written = full_write(fdtmp, buf.as_bytes());
    if written < 0 || (written as usize) != buf.len() {
        let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        r = if e != 0 { -e } else { -libc::EBADF };
    }
    if unsafe { libc::close(fdtmp) } != 0 && r == 0 {
        r = -std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    }
    let tmp = tmpFilename.as_deref().unwrap_or("");
    if r == 0 {
        r = match std::fs::rename(tmp, this.filename.as_deref().unwrap_or("")) {
            Ok(()) => 0,
            Err(e) => -e.raw_os_error().unwrap_or(libc::EIO),
        };
    }
    r
}

/// Port of `Settings* Settings_new(const Machine* host, Hashtable*
/// dynamicMeters, Hashtable* dynamicColumns, Hashtable* dynamicScreens)`
/// from `Settings.c:794`. The top-level constructor: installs the built-in
/// defaults, resolves the config path from `HTOPRC` / `HOME` /
/// `XDG_CONFIG_HOME` (falling back to `getpwuid` for a missing `HOME`),
/// `mkdir`s the config dir, `realpath`s the filename, then drives
/// [`Settings_read`] over the user / legacy-dotfile / `SYSCONFDIR` configs
/// and falls back to [`Settings_defaultMeters`] / [`Settings_defaultScreens`].
///
/// The C `Settings* this = xCalloc(...)` heap struct becomes an owned
/// `Settings` returned by value. The borrowed `dynamic*` `Hashtable`
/// pointers are stored as-is (not owned). The C `ss` back-pointer is not
/// modeled (the index `ssIndex` suffices).
///
/// This calls the (currently chain-stubbed) [`Settings_read`] /
/// [`Settings_defaultScreens`]; both are blocked on unported platform
/// substrate (see their docs), so invoking `Settings_new` reaches those
/// `todo!` sites at runtime. The env/path-resolution body itself is a
/// faithful, standalone port.
pub fn Settings_new(
    host: &Machine,
    dynamicMeters: Option<*mut Hashtable>,
    dynamicColumns: Option<*mut Hashtable>,
    dynamicScreens: Option<*mut Hashtable>,
) -> Settings {
    let mut this = Settings {
        writeConfig: true,
        dynamicScreens,
        dynamicColumns,
        dynamicMeters,
        hLayout: HeaderLayout::HF_TWO_50_50,
        ..Default::default()
    };
    this.hColumns = vec![MeterColumnSetting::default(); HeaderLayout_getColumns(this.hLayout)];

    this.shadowOtherUsers = false;
    this.showThreadNames = false;
    this.hideKernelThreads = true;
    this.hideUserlandThreads = false;
    this.hideRunningInContainer = false;
    this.highlightBaseName = false;
    this.highlightDeletedExe = true;
    this.shadowDistPathPrefix = false;
    this.highlightMegabytes = true;
    this.detailedCPUTime = false;
    this.countCPUsFromOne = false;
    this.showCPUSMTLabels = false;
    this.showCPUUsage = true;
    this.showCPUFrequency = false;
    // BUILD_WITH_CPU_TEMP (showCPUTemperature/degreeFahrenheit) gated out.
    this.showCachedMemory = true;
    this.updateProcessNames = false;
    this.showProgramPath = true;
    this.highlightThreads = true;
    this.highlightChanges = false;
    this.highlightDelaySecs = DEFAULT_HIGHLIGHT_SECS;
    this.findCommInCmdline = true;
    this.stripExeFromCmdline = true;
    this.showMergedCommand = false;
    this.hideFunctionBar = 0;
    this.headerMargin = true;
    // HAVE_LIBHWLOC (topologyAffinity) gated out.

    // this->screens = xCalloc(Platform_numberOfDefaultScreens, ...); nScreens = 0;
    // The owned `Vec` grows on demand, so the pre-sizing is unnecessary.
    this.screens = Vec::new();

    let mut legacyDotfile: Option<String> = None;
    if let Ok(rcfile) = std::env::var("HTOPRC") {
        this.initialFilename = Some(rcfile);
    } else {
        // HOME must be an absolute path; else fall back to getpwuid(getuid()).
        let mut home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() || !home.starts_with('/') {
            // const struct passwd* pw = getpwuid(getuid());
            // home = (pw && pw->pw_dir && pw->pw_dir[0] == '/') ? pw->pw_dir : "";
            home = unsafe {
                let pw = libc::getpwuid(libc::getuid());
                if !pw.is_null() && !(*pw).pw_dir.is_null() {
                    let dir = CStr::from_ptr((*pw).pw_dir).to_string_lossy().into_owned();
                    if dir.starts_with('/') {
                        dir
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            };
        }
        let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_default();
        let (initialFilename, configDir, htopDir);
        if !xdg.is_empty() && xdg.starts_with('/') {
            initialFilename = format!("{xdg}/htop/htoprc");
            configDir = xdg.clone();
            htopDir = format!("{xdg}/htop");
        } else {
            initialFilename = format!("{home}{CONFIGDIR}/htop/htoprc");
            configDir = format!("{home}{CONFIGDIR}");
            htopDir = format!("{home}{CONFIGDIR}/htop");
        }
        this.initialFilename = Some(initialFilename);
        // (void) mkdir(dir, 0700): errors ignored.
        let _ = std::fs::DirBuilder::new().mode(0o700).create(&configDir);
        let _ = std::fs::DirBuilder::new().mode(0o700).create(&htopDir);

        legacyDotfile = Some(format!("{home}/.htoprc"));
    }

    // realpath(initialFilename, buf); on failure keep initialFilename.
    let initial = this.initialFilename.clone().unwrap_or_default();
    this.filename = match std::fs::canonicalize(&initial) {
        Ok(p) => Some(p.to_string_lossy().into_owned()),
        Err(_) => Some(initial),
    };

    this.colorScheme = 0;
    this.enableMouse = true; // HAVE_GETMOUSE
    this.changed = false;
    this.delay = DEFAULT_DELAY;

    let filename = this.filename.clone().unwrap_or_default();
    let mut ok = Settings_read(&mut this, &filename, host, /*checkWritability*/ true);
    if !ok {
        if let Some(legacy) = legacyDotfile.clone() {
            let writeConfig = this.writeConfig;
            ok = Settings_read(&mut this, &legacy, host, writeConfig);
            if ok && this.writeConfig {
                // Transition to new location and delete old configuration file.
                if Settings_write(&this, false) == 0 {
                    let _ = std::fs::remove_file(&legacy);
                }
            }
        }
    }
    if !ok {
        this.screenTabs = true;
        this.changed = true;

        ok = Settings_read(
            &mut this,
            &format!("{SYSCONFDIR}/htoprc"),
            host,
            /*checkWritability*/ false,
        );
    }
    if !ok {
        Settings_defaultMeters(&mut this, host);
        Settings_defaultScreens(&mut this);
    }

    this.ssIndex = 0;
    // this->ss = this->screens[this->ssIndex]: the back-pointer is not modeled.

    this.lastUpdate = 1;

    this
}

/// Port of `RowField` (`RowField.h:60` — `typedef int32_t RowField`). The
/// screen sort keys and field list are `RowField`s: reserved process-field
/// ids (see [`crate::ported::process::ProcessField`]) plus runtime
/// dynamic-column ids past `ROW_DYNAMIC_FIELDS`, so the raw `int32_t` is
/// modeled directly rather than the narrower `ProcessField` enum.
pub type RowField = i32;

/// Port of htop's `ScreenSettings` (`Settings.h:42`). The C `char* heading`
/// / `char* dynamic` (either may be `NULL`) become `Option<String>`, and
/// `RowField* fields` (a heap array sized to `LAST_PROCESSFIELD`) becomes
/// an owned `Vec<RowField>`. The C `struct Table_* table` is modeled as an
/// `Option<TableHandle>` (a raw `*mut Table` in the crate's pointer-graph
/// ownership model; `None` = C `NULL`), keeping `ScreenSettings` the single
/// canonical config type that both [`crate::ported::machine`] and the panels
/// share.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct ScreenSettings {
    pub heading: Option<String>,
    pub dynamic: Option<String>,
    /// C `struct Table_* table` — the table this screen drives, a raw
    /// `*mut Table` ([`TableHandle`]; `None` = `NULL`, defaulted to the
    /// process table by
    /// [`crate::ported::machine::Machine_populateTablesFromSettings`]).
    pub table: Option<TableHandle>,
    pub fields: Vec<RowField>,
    pub flags: u32,
    pub direction: i32,
    pub treeDirection: i32,
    pub sortKey: RowField,
    pub treeSortKey: RowField,
    pub treeView: bool,
    pub treeViewAlwaysByPID: bool,
    pub allBranchesCollapsed: bool,
    pub stableTreeView: i32,
}

/// Port of `ScreenSettings_getActiveSortKey` (`Settings.h:122`, a pure
/// `static inline`). In tree view the active key is `treeSortKey`, unless
/// `treeViewAlwaysByPID` forces the hardcoded `PID` field (`1`, per
/// `RowField.h:14`); in flat view it is `sortKey`.
pub fn ScreenSettings_getActiveSortKey(this: &ScreenSettings) -> RowField {
    if this.treeView {
        if this.treeViewAlwaysByPID {
            1
        } else {
            this.treeSortKey
        }
    } else {
        this.sortKey
    }
}

/// Port of `ScreenSettings_getActiveDirection` (`Settings.h:128`, a pure
/// `static inline`). Returns `treeDirection` in tree view, else
/// `direction`.
pub fn ScreenSettings_getActiveDirection(this: &ScreenSettings) -> i32 {
    if this.treeView {
        this.treeDirection
    } else {
        this.direction
    }
}

/// Port of `Settings.c:922`. Flips the active sort direction between `1`
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

/// Port of `void ScreenSettings_setSortKey(ScreenSettings* this, ProcessField
/// sortKey)` from `Settings.c:918`. Sets the flat or tree sort key (whichever
/// the current view mode selects) and its default direction from
/// `Process_fields[sortKey].defaultSortDesc`; setting the flat key also
/// leaves tree view.
pub fn ScreenSettings_setSortKey(this: &mut ScreenSettings, sortKey: RowField) {
    if this.treeViewAlwaysByPID || !this.treeView {
        this.sortKey = sortKey;
        this.direction = if Process_fields[sortKey as usize].defaultSortDesc {
            -1
        } else {
            1
        };
        this.treeView = false;
    } else {
        this.treeSortKey = sortKey;
        this.treeDirection = if Process_fields[sortKey as usize].defaultSortDesc {
            -1
        } else {
            1
        };
    }
}

/// The file-static `bool readonly` from `Settings.c:938`. A process-wide
/// latch, so it is a `static` `AtomicBool` here rather than a passed
/// value.
static READONLY: AtomicBool = AtomicBool::new(false);

/// Port of `Settings.c:940`. Sets the process-wide `readonly` latch. The
/// C `readonly = true` becomes an atomic store.
pub fn Settings_enableReadonly() {
    READONLY.store(true, Ordering::Relaxed);
}

/// Port of `Settings.c:944`. Returns the current value of the
/// process-wide `readonly` latch.
pub fn Settings_isReadonly() -> bool {
    READONLY.load(Ordering::Relaxed)
}

/// Port of `Settings.c:948`. Resizes `hColumns` to the new layout's
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

    #[test]
    fn header_layouts_table_is_consistent() {
        use HeaderLayout::*;
        // One row per real layout (excludes HF_INVALID / LAST).
        assert_eq!(HeaderLayout_layouts.len(), LAST_HEADER_LAYOUT as usize);

        let all = [
            HF_ONE_100,
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
        ];
        for layout in all {
            let def = &HeaderLayout_layouts[layout as usize];
            // `columns` agrees with the independent `HeaderLayout_getColumns`.
            assert_eq!(
                def.columns as usize,
                HeaderLayout_getColumns(layout),
                "{layout:?}"
            );
            // The used columns' percentages sum to 100.
            let sum: u32 = def.widths[..def.columns as usize]
                .iter()
                .map(|&w| w as u32)
                .sum();
            assert_eq!(sum, 100, "{layout:?} widths sum");
            // Unused trailing widths are zero.
            assert!(def.widths[def.columns as usize..].iter().all(|&w| w == 0));
            assert!(!def.name.is_empty() && !def.description.is_empty());
        }
        // Spot-check a couple of specific rows.
        assert_eq!(
            HeaderLayout_layouts[HF_TWO_67_33 as usize].widths,
            [67, 33, 0, 0]
        );
        assert_eq!(HeaderLayout_layouts[HF_ONE_100 as usize].name, "one_100");
    }

    /// A default 2-column (`HF_TWO_50_50`) `Settings` with empty meter
    /// columns, matching what `Settings_new` sets up before reading a
    /// config file.
    fn two_column_settings() -> Settings {
        Settings {
            hLayout: HeaderLayout::HF_TWO_50_50,
            hColumns: vec![MeterColumnSetting::default(), MeterColumnSetting::default()],
            screens: Vec::new(),
            ssIndex: 0,
            changed: false,
            lastUpdate: 0,
            ..Default::default()
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
            [
                BAR_METERMODE,
                TEXT_METERMODE,
                TEXT_METERMODE,
                TEXT_METERMODE
            ]
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        };
        ScreenSettings_invertSortOrder(&mut ss);
        assert_eq!(ss.direction, 1);
    }

    #[test]
    fn active_sort_key_picks_field_by_view_mode() {
        // flat view -> sortKey, regardless of the tree fields
        let ss = ScreenSettings {
            treeView: false,
            sortKey: 47,     // PERCENT_CPU
            treeSortKey: 49, // USER
            treeViewAlwaysByPID: true,
            ..Default::default()
        };
        assert_eq!(ScreenSettings_getActiveSortKey(&ss), 47);

        // tree view without alwaysByPID -> treeSortKey
        let ss = ScreenSettings {
            treeView: true,
            sortKey: 47,
            treeSortKey: 49,
            treeViewAlwaysByPID: false,
            ..Default::default()
        };
        assert_eq!(ScreenSettings_getActiveSortKey(&ss), 49);

        // tree view WITH alwaysByPID -> hardcoded PID field (1)
        let ss = ScreenSettings {
            treeView: true,
            sortKey: 47,
            treeSortKey: 49,
            treeViewAlwaysByPID: true,
            ..Default::default()
        };
        assert_eq!(ScreenSettings_getActiveSortKey(&ss), 1);
    }

    #[test]
    fn active_direction_picks_by_view_mode() {
        let ss = ScreenSettings {
            treeView: false,
            direction: -1,
            treeDirection: 1,
            ..Default::default()
        };
        assert_eq!(ScreenSettings_getActiveDirection(&ss), -1);

        let ss = ScreenSettings {
            treeView: true,
            direction: -1,
            treeDirection: 1,
            ..Default::default()
        };
        assert_eq!(ScreenSettings_getActiveDirection(&ss), 1);
    }

    #[test]
    fn readonly_latch_starts_false_then_latches_true() {
        // single test owns the global latch to avoid cross-test races
        assert!(!Settings_isReadonly());
        Settings_enableReadonly();
        assert!(Settings_isReadonly());
    }

    /// [`ScreenSettings_setSortKey`]: flat/tree mode picks the right key pair
    /// and derives the direction from `Process_fields[key].defaultSortDesc`.
    #[test]
    fn set_sort_key_flat_and_tree_modes() {
        use crate::ported::process::ProcessField;

        // Flat view: sets sortKey + direction (PERCENT_CPU is defaultSortDesc
        // → -1), and leaves tree view.
        let mut ss = ScreenSettings {
            treeView: false,
            ..Default::default()
        };
        ScreenSettings_setSortKey(&mut ss, ProcessField::PERCENT_CPU as RowField);
        assert_eq!(ss.sortKey, ProcessField::PERCENT_CPU as RowField);
        assert_eq!(ss.direction, -1);
        assert!(!ss.treeView);
        // PID is not defaultSortDesc → +1.
        ScreenSettings_setSortKey(&mut ss, ProcessField::PID as RowField);
        assert_eq!(ss.direction, 1);

        // Tree view (not alwaysByPID): sets the tree key pair, keeps treeView.
        let mut ss = ScreenSettings {
            treeView: true,
            treeViewAlwaysByPID: false,
            ..Default::default()
        };
        ScreenSettings_setSortKey(&mut ss, ProcessField::PERCENT_CPU as RowField);
        assert_eq!(ss.treeSortKey, ProcessField::PERCENT_CPU as RowField);
        assert_eq!(ss.treeDirection, -1);
        assert!(ss.treeView);
    }

    /// [`toFieldName`] / [`toFieldIndex`] over the reserved field table (empty
    /// dynamic-column hashtable): id→name, the old-enum `atoi+1` digit form,
    /// by-name lookup, and the unresolved `-1`.
    #[test]
    fn to_field_name_and_index_reserved() {
        use crate::ported::hashtable::Hashtable_new;
        use crate::ported::process::ProcessField;

        let ht = Hashtable_new(0, false);
        assert_eq!(
            toFieldName(&ht, ProcessField::PID as i32, None),
            Some("PID")
        );
        assert_eq!(toFieldName(&ht, -1, None), None);
        // "0" → atoi 0 + 1 = 1 (PID) in the old zero-based enum form.
        assert_eq!(toFieldIndex(&ht, "0"), ProcessField::PID as i32);
        // By-name.
        assert_eq!(toFieldIndex(&ht, "Command"), ProcessField::COMM as i32);
        assert_eq!(toFieldIndex(&ht, "RCHAR"), ProcessField::RCHAR as i32);
        // Unresolved.
        assert_eq!(toFieldIndex(&ht, "Nonexistent"), -1);
    }

    /// [`ScreenSettings_readFields`]: parses a space-separated field list into
    /// `fields` and OR-s each reserved field's scan flags into `flags`.
    #[test]
    fn read_fields_parses_ids_and_flags() {
        use crate::ported::hashtable::Hashtable_new;
        use crate::ported::process::{ProcessField, PROCESS_FLAG_IO};

        let ht = Hashtable_new(0, false);
        let mut ss = ScreenSettings::default();
        // Old-enum digit form: "0 1" → PID(1), COMM(2); both flags 0.
        ScreenSettings_readFields(&mut ss, &ht, "0 1");
        assert_eq!(
            ss.fields,
            vec![
                ProcessField::PID as RowField,
                ProcessField::COMM as RowField
            ]
        );
        assert_eq!(ss.flags, 0);

        // By-name field carrying a scan flag: RCHAR → PROCESS_FLAG_IO.
        let mut ss = ScreenSettings::default();
        ScreenSettings_readFields(&mut ss, &ht, "RCHAR");
        assert_eq!(ss.fields, vec![ProcessField::RCHAR as RowField]);
        assert_eq!(ss.flags, PROCESS_FLAG_IO);
    }

    /// [`writeFields`]: reserved fields are written by their [`toFieldName`]
    /// names when `byName`, and as the older zero-based `field - 1` numeric
    /// form otherwise; the field list is space-joined and terminated by the
    /// separator. Round-trips with [`ScreenSettings_readFields`] on the
    /// by-name form.
    #[test]
    fn write_fields_by_name_and_numeric_form() {
        use crate::ported::hashtable::Hashtable_new;
        use crate::ported::process::ProcessField;

        let ht = Hashtable_new(0, false);
        let fields = vec![
            ProcessField::PID as RowField,
            ProcessField::COMM as RowField,
        ];

        // byName: reserved field names, space-joined, then the separator.
        let mut out = String::new();
        writeFields(&mut out, &fields, &ht, true, '\n');
        assert_eq!(out, "PID Command\n");
        // The written by-name form parses back to the same field ids.
        let mut ss = ScreenSettings::default();
        ScreenSettings_readFields(&mut ss, &ht, out.trim_end());
        assert_eq!(ss.fields, fields);

        // !byName: the older zero-based enum form (field - 1).
        let mut out = String::new();
        writeFields(&mut out, &fields, &ht, false, '\n');
        assert_eq!(
            out,
            format!(
                "{} {}\n",
                ProcessField::PID as i32 - 1,
                ProcessField::COMM as i32 - 1
            )
        );

        // Empty field list writes just the separator.
        let mut out = String::new();
        writeFields(&mut out, &[], &ht, true, ';');
        assert_eq!(out, ";");
    }

    #[test]
    fn read_missing_file_returns_false_and_sets_write_config() {
        // Settings.c:328 — open fails with ENOENT: writeConfig = true and the
        // function returns false (the common first-run path that Settings_new
        // falls through to defaults on).
        let mut s = Settings::default();
        // A path that cannot exist under a normal filesystem.
        let path = "/nonexistent/htoprs-test/does-not-exist/htoprc";
        let host = host_with_cpus(4);

        let ok = Settings_read(&mut s, path, &host, /*checkWritability*/ true);

        assert!(!ok, "missing file must return false");
        assert!(s.writeConfig, "ENOENT must set writeConfig = true");
    }

    #[test]
    fn default_screens_creates_main_screen() {
        // Settings.c:309 — empty settings: one "Main" screen is created and
        // index 0 is returned.
        let mut s = Settings::default();
        assert!(s.screens.is_empty());

        let idx = Settings_defaultScreens(&mut s);

        assert_eq!(idx, 0);
        assert_eq!(s.screens.len(), 1);
        assert_eq!(s.screens[0].heading.as_deref(), Some("Main"));

        // Idempotent: a second call returns the existing screen[0], no new push.
        let idx2 = Settings_defaultScreens(&mut s);
        assert_eq!(idx2, 0);
        assert_eq!(s.screens.len(), 1);
    }
}
