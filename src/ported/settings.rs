//! Partial port of `Settings.c` — htop's config-file settings layer.
//!
//! Most of `Settings.c` is I/O and substrate: `Settings_read`/`_write`
//! do `open`/`fstat`/`mkstemp`/`rename` syscalls, `Settings_new` reads
//! the environment and calls `Platform_*`, the meter/screen helpers
//! call `HeaderLayout_getColumns`, `Process_fields`, `DynamicColumn_*`,
//! `Hashtable`, and the `x*` heap-alloc wrappers — none of which have a
//! faithful safe-Rust analog here. Those functions are left as their
//! exact `todo!()` stubs.
//!
//! The functions ported below are the ones whose *full* behavior is
//! reproducible in safe Rust without any of that substrate:
//! `Settings_splitLineToIDs` (pure string work delegating to the ported
//! `XUtils` helpers), `ScreenSettings_invertSortOrder` (pure logic over
//! three `ScreenSettings` fields), and the `readonly` latch pair
//! (`Settings_enableReadonly` / `Settings_isReadonly`).
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};

use crate::ported::xutils::{String_split, String_trim};

/// Port of `Settings.c:59`. Trims leading/trailing ` `/`\t`/`\n` from
/// `line`, then splits the remainder on single spaces. Interior empty
/// fields (consecutive spaces) are kept and a trailing empty field is
/// dropped, exactly as htop's `String_split(trim, ' ', NULL)` does. The
/// C `free(trim)` is handled by Rust ownership.
pub fn Settings_splitLineToIDs(line: &str) -> Vec<String> {
    let trim = String_trim(line);
    String_split(&trim, ' ')
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

/// The file-static `bool readonly` from `Settings.c:929`. A process-wide
/// latch, so it is a `static` `AtomicBool` here rather than a passed
/// value.
static READONLY: AtomicBool = AtomicBool::new(false);

/// TODO: port of `static void Settings_deleteColumns(Settings* this` from `Settings.c:35`.
pub fn Settings_deleteColumns() {
    todo!("port of Settings.c:35")
}

/// TODO: port of `static void Settings_deleteScreens(Settings* this` from `Settings.c:43`.
pub fn Settings_deleteScreens() {
    todo!("port of Settings.c:43")
}

/// TODO: port of `void Settings_delete(Settings* this` from `Settings.c:51`.
pub fn Settings_delete() {
    todo!("port of Settings.c:51")
}

/// TODO: port of `static void Settings_readMeters(Settings* this, const char* line, size_t column` from `Settings.c:66`.
pub fn Settings_readMeters() {
    todo!("port of Settings.c:66")
}

/// TODO: port of `static void Settings_readMeterModes(Settings* this, const char* line, size_t column` from `Settings.c:71`.
pub fn Settings_readMeterModes() {
    todo!("port of Settings.c:71")
}

/// TODO: port of `static bool Settings_validateMeters(Settings* this` from `Settings.c:90`.
pub fn Settings_validateMeters() {
    todo!("port of Settings.c:90")
}

/// TODO: port of `static void Settings_defaultMeters(Settings* this, const Machine* host` from `Settings.c:120`.
pub fn Settings_defaultMeters() {
    todo!("port of Settings.c:120")
}

/// TODO: port of `static const char* toFieldName(Hashtable* columns, int id, bool* enabled` from `Settings.c:181`.
pub fn toFieldName() {
    todo!("port of Settings.c:181")
}

/// TODO: port of `static int toFieldIndex(Hashtable* columns, const char* str` from `Settings.c:198`.
pub fn toFieldIndex() {
    todo!("port of Settings.c:198")
}

/// TODO: port of `static void ScreenSettings_readFields(ScreenSettings* ss, Hashtable* columns, const char* line` from `Settings.c:230`.
pub fn ScreenSettings_readFields() {
    todo!("port of Settings.c:230")
}

/// TODO: port of `static ScreenSettings* Settings_initScreenSettings(ScreenSettings* ss, Settings* this, const char* columns` from `Settings.c:254`.
pub fn Settings_initScreenSettings() {
    todo!("port of Settings.c:254")
}

/// TODO: port of `ScreenSettings* Settings_newScreen(Settings* this, const ScreenDefaults* defaults` from `Settings.c:263`.
pub fn Settings_newScreen() {
    todo!("port of Settings.c:263")
}

/// TODO: port of `ScreenSettings* Settings_newDynamicScreen(Settings* this, const char* tab, const DynamicScreen* screen, Table* table` from `Settings.c:286`.
pub fn Settings_newDynamicScreen() {
    todo!("port of Settings.c:286")
}

/// TODO: port of `void ScreenSettings_delete(ScreenSettings* this` from `Settings.c:302`.
pub fn ScreenSettings_delete() {
    todo!("port of Settings.c:302")
}

/// TODO: port of `static ScreenSettings* Settings_defaultScreens(Settings* this` from `Settings.c:309`.
pub fn Settings_defaultScreens() {
    todo!("port of Settings.c:309")
}

/// TODO: port of `static bool Settings_read(Settings* this, const char* fileName, const Machine* host, bool checkWritability` from `Settings.c:320`.
pub fn Settings_read() {
    todo!("port of Settings.c:320")
}

/// TODO: port of `static void writeFields(OutputFunc of, FILE* fp,` from `Settings.c:575`.
pub fn writeFields() {
    todo!("port of Settings.c:575")
}

/// TODO: port of `static void writeList(OutputFunc of, FILE* fp,` from `Settings.c:597`.
pub fn writeList() {
    todo!("port of Settings.c:597")
}

/// TODO: port of `static void writeMeters(const Settings* this, OutputFunc of,` from `Settings.c:607`.
pub fn writeMeters() {
    todo!("port of Settings.c:607")
}

/// TODO: port of `static void writeMeterModes(const Settings* this, OutputFunc of,` from `Settings.c:616`.
pub fn writeMeterModes() {
    todo!("port of Settings.c:616")
}

/// TODO: port of `static int signal_safe_fprintf(FILE* stream, const char* fmt, ...` from `Settings.c:632`.
pub fn signal_safe_fprintf() {
    todo!("port of Settings.c:632")
}

/// TODO: port of `int Settings_write(const Settings* this, bool onCrash` from `Settings.c:647`.
pub fn Settings_write() {
    todo!("port of Settings.c:647")
}

/// TODO: port of `Settings* Settings_new(const Machine* host, Hashtable* dynamicMeters, Hashtable* dynamicColumns, Hashtable* dynamicScreens` from `Settings.c:794`.
pub fn Settings_new() {
    todo!("port of Settings.c:794")
}

/// TODO: port of `void ScreenSettings_setSortKey(ScreenSettings* this, ProcessField sortKey` from `Settings.c:918`.
pub fn ScreenSettings_setSortKey() {
    todo!("port of Settings.c:918")
}

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

/// TODO: port of `void Settings_setHeaderLayout(Settings* this, HeaderLayout hLayout` from `Settings.c:939`.
pub fn Settings_setHeaderLayout() {
    todo!("port of Settings.c:939")
}

#[cfg(test)]
mod tests {
    use super::*;

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
