//! Partial port of `BacktraceScreen.c` — htop's process backtrace panel.
//!
//! Only the substrate-free logic is ported: [`getBasename`] (pure
//! string), [`BacktracePanel_makePrintingHelper`] (pure column-width
//! computation over the panel rows), and [`BacktraceFrameData_new`]
//! (field-init constructor). Everything else in this file drives
//! ncurses `RichString`, `Panel`/`FunctionBar` widgets, the `Object`
//! vtable, `Vector`, libunwind ptrace, or manual `free()` — none of
//! which have a faithful safe-Rust analog yet — so those functions
//! remain exact `todo!()` stubs.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// `BacktracePanelRowType` discriminants from `BacktraceScreen.h:49`.
/// `row->type` is stored as a plain `int` in the C struct, so these are
/// modeled as `i32` constants matching the enum order.
pub const BACKTRACE_PANEL_ROW_DATA_FRAME: i32 = 0;
pub const BACKTRACE_PANEL_ROW_ERROR: i32 = 1;
pub const BACKTRACE_PANEL_ROW_PROCESS_INFORMATION: i32 = 2;

/// Model of `BacktraceFrameData` (`BacktraceScreen.h:20`). C `char*`
/// fields (nullable) become `Option<String>`; `size_t` become `usize`;
/// `unsigned int index` becomes `u32`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BacktraceFrameData {
    pub address: usize,
    pub offset: usize,
    pub functionName: Option<String>,
    pub demangleFunctionName: Option<String>,
    pub objectPath: Option<String>,
    pub index: u32,
    pub isSignalFrame: bool,
}

/// Model of `BacktracePanelPrintingHelper` (`BacktraceScreen.h:32`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BacktracePanelPrintingHelper {
    pub maxAddrLen: usize,
    pub maxFrameNumLen: usize,
    pub maxObjPathLen: usize,
    pub maxObjNameLen: usize,
    pub hasDemangledNames: bool,
}

/// Model of the subset of `BacktracePanelRow` (`BacktraceScreen.h:55`)
/// that [`BacktracePanel_makePrintingHelper`] reads: the `int type` tag
/// and the `data.frame` union arm (present only for
/// `BACKTRACE_PANEL_ROW_DATA_FRAME`). The `error`, `panel`, and
/// `process` fields are omitted — that pass never touches them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BacktracePanelRow {
    pub type_: i32,
    pub frame: Option<BacktraceFrameData>,
}

/// Port of `BacktraceFrameData_new(void)` from `BacktraceScreen.c:70`.
/// The C body allocates via `AllocThis` and zero/NULL-initializes every
/// field; the faithful Rust analog is an owned struct with those
/// defaults (`0`, `None`, `false`).
pub fn BacktraceFrameData_new() -> BacktraceFrameData {
    BacktraceFrameData {
        address: 0,
        offset: 0,
        functionName: None,
        demangleFunctionName: None,
        objectPath: None,
        index: 0,
        isSignalFrame: false,
    }
}

/// Port of `getBasename(const char* path)` from `BacktraceScreen.c:119`.
/// Returns everything after the last `/`, or the whole `path` when there
/// is no `/` — mirroring `strrchr(path, '/')` then `lastSlash + 1`. A
/// trailing `/` yields an empty basename (the slash is the last byte).
pub fn getBasename(path: &str) -> &str {
    match path.rfind('/') {
        Some(idx) => &path[idx + 1..],
        None => path,
    }
}

/// Port of `BacktracePanel_makePrintingHelper(const BacktracePanel*
/// this, BacktracePanelPrintingHelper* printingHelper)` from
/// `BacktraceScreen.c:124`. The C reads `this->super.items` (a `Vector`
/// of rows); that vector is modeled here as the `rows` slice. Computes
/// the column widths needed to render the panel, taking the `MAXIMUM`
/// against the helper's incoming values (which callers pre-seed with the
/// header label widths). `countDigits` is reused from the `XUtils` port.
pub fn BacktracePanel_makePrintingHelper(
    rows: &[BacktracePanelRow],
    printingHelper: &mut BacktracePanelPrintingHelper,
) {
    use crate::ported::xutils::countDigits;

    let mut maxFrameNum: u32 = 0;
    let mut longestAddress: usize = 0;

    printingHelper.hasDemangledNames = false;

    for row in rows {
        if row.type_ != BACKTRACE_PANEL_ROW_DATA_FRAME {
            continue;
        }
        // C unconditionally dereferences row->data.frame for DATA_FRAME rows.
        let frame = row
            .frame
            .as_ref()
            .expect("DATA_FRAME row must carry a frame");

        if frame.demangleFunctionName.is_some() {
            printingHelper.hasDemangledNames = true;
        }

        if let Some(objectPath) = frame.objectPath.as_deref() {
            let objectName = getBasename(objectPath);
            let objectNameLength = objectName.len();
            // C: (objectName - objectPath) + objectNameLength, where the
            // pointer delta is the basename's byte offset within the path.
            let objectPathLength = (objectPath.len() - objectNameLength) + objectNameLength;

            printingHelper.maxObjNameLen = objectNameLength.max(printingHelper.maxObjNameLen);
            printingHelper.maxObjPathLen = objectPathLength.max(printingHelper.maxObjPathLen);
        }

        maxFrameNum = frame.index.max(maxFrameNum);

        longestAddress = frame.address.max(longestAddress);
    }

    printingHelper.maxFrameNumLen =
        countDigits(maxFrameNum as usize, 10).max(printingHelper.maxFrameNumLen);
    printingHelper.maxAddrLen = countDigits(longestAddress, 16).max(printingHelper.maxAddrLen);
}

/// TODO: port of `void BacktraceFrameData_delete(Object* object` from `BacktraceScreen.c:82`.
pub fn BacktraceFrameData_delete() {
    todo!("port of BacktraceScreen.c:82")
}

/// TODO: port of `static void BacktracePanel_displayHeader(BacktracePanel* this` from `BacktraceScreen.c:90`.
pub fn BacktracePanel_displayHeader() {
    todo!("port of BacktraceScreen.c:90")
}

/// TODO: port of `static void BacktracePanel_makeBacktrace(Vector* frames, pid_t pid, char** error` from `BacktraceScreen.c:158`.
pub fn BacktracePanel_makeBacktrace() {
    todo!("port of BacktraceScreen.c:158")
}

/// TODO: port of `static void BacktracePanel_populateFrames(BacktracePanel* this` from `BacktraceScreen.c:168`.
pub fn BacktracePanel_populateFrames() {
    todo!("port of BacktraceScreen.c:168")
}

/// TODO: port of `static HandlerResult BacktracePanel_eventHandler(Panel* super, int ch` from `BacktraceScreen.c:208`.
pub fn BacktracePanel_eventHandler() {
    todo!("port of BacktraceScreen.c:208")
}

/// TODO: port of `BacktracePanel* BacktracePanel_new(Vector* processes, const Settings* settings` from `BacktraceScreen.c:248`.
pub fn BacktracePanel_new() {
    todo!("port of BacktraceScreen.c:248")
}

/// TODO: port of `void BacktracePanel_delete(Object* object` from `BacktraceScreen.c:277`.
pub fn BacktracePanel_delete() {
    todo!("port of BacktraceScreen.c:277")
}

/// TODO: port of `static void BacktracePanelRow_highlightBasename(const BacktracePanelRow* row, RichString* out, char* line, int objectPathStart` from `BacktraceScreen.c:283`.
pub fn BacktracePanelRow_highlightBasename() {
    todo!("port of BacktraceScreen.c:283")
}

/// TODO: port of `static void BacktracePanelRow_displayInformation(const Object* super, RichString* out` from `BacktraceScreen.c:308`.
pub fn BacktracePanelRow_displayInformation() {
    todo!("port of BacktraceScreen.c:308")
}

/// TODO: port of `static void BacktracePanelRow_displayFrame(const Object* super, RichString* out` from `BacktraceScreen.c:356`.
pub fn BacktracePanelRow_displayFrame() {
    todo!("port of BacktraceScreen.c:356")
}

/// TODO: port of `static void BacktracePanelRow_displayError(const Object* super, RichString* out` from `BacktraceScreen.c:416`.
pub fn BacktracePanelRow_displayError() {
    todo!("port of BacktraceScreen.c:416")
}

/// TODO: port of `static void BacktracePanelRow_display(const Object* super, RichString* out` from `BacktraceScreen.c:425`.
pub fn BacktracePanelRow_display() {
    todo!("port of BacktraceScreen.c:425")
}

/// TODO: port of `BacktracePanelRow* BacktracePanelRow_new(const BacktracePanel* panel` from `BacktraceScreen.c:444`.
pub fn BacktracePanelRow_new() {
    todo!("port of BacktraceScreen.c:444")
}

/// TODO: port of `void BacktracePanelRow_delete(Object* object` from `BacktraceScreen.c:450`.
pub fn BacktracePanelRow_delete() {
    todo!("port of BacktraceScreen.c:450")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backtrace_frame_data_new_is_all_default() {
        let f = BacktraceFrameData_new();
        assert_eq!(f.address, 0);
        assert_eq!(f.offset, 0);
        assert_eq!(f.functionName, None);
        assert_eq!(f.demangleFunctionName, None);
        assert_eq!(f.objectPath, None);
        assert_eq!(f.index, 0);
        assert!(!f.isSignalFrame);
    }

    #[test]
    fn get_basename_matches_strrchr_semantics() {
        // last path component after final '/'
        assert_eq!(getBasename("/usr/lib/libc.so.6"), "libc.so.6");
        // no slash -> whole string
        assert_eq!(getBasename("libc.so"), "libc.so");
        // trailing slash -> empty basename (slash is the last byte)
        assert_eq!(getBasename("/foo/"), "");
        // bare root
        assert_eq!(getBasename("/"), "");
        // empty input
        assert_eq!(getBasename(""), "");
        // relative multi-segment
        assert_eq!(getBasename("a/b/c"), "c");
    }

    fn frame(
        index: u32,
        address: usize,
        obj: Option<&str>,
        demangle: Option<&str>,
    ) -> BacktracePanelRow {
        BacktracePanelRow {
            type_: BACKTRACE_PANEL_ROW_DATA_FRAME,
            frame: Some(BacktraceFrameData {
                address,
                offset: 0,
                functionName: None,
                demangleFunctionName: demangle.map(str::to_string),
                objectPath: obj.map(str::to_string),
                index,
                isSignalFrame: false,
            }),
        }
    }

    // Helper seeded with the header-label floors that BacktracePanel_new sets
    // (BacktraceScreen.c:252-256): maxAddrLen = strlen("ADDRESS")-strlen("0x")=5,
    // maxFrameNumLen = strlen("#")=1, maxObjNameLen = maxObjPathLen = strlen("FILE")=4.
    fn seeded_helper() -> BacktracePanelPrintingHelper {
        BacktracePanelPrintingHelper {
            maxAddrLen: "ADDRESS".len() - "0x".len(),
            maxFrameNumLen: "#".len(),
            maxObjPathLen: "FILE".len(),
            maxObjNameLen: "FILE".len(),
            hasDemangledNames: false,
        }
    }

    #[test]
    fn make_printing_helper_computes_widths_and_skips_non_frames() {
        let rows = vec![
            frame(3, 0xff, Some("/usr/lib/libc.so.6"), Some("demangled")),
            frame(150, 0x10000, Some("ld.so"), None),
            BacktracePanelRow {
                type_: BACKTRACE_PANEL_ROW_PROCESS_INFORMATION,
                frame: None,
            },
            BacktracePanelRow {
                type_: BACKTRACE_PANEL_ROW_ERROR,
                frame: None,
            },
        ];
        let mut h = seeded_helper();
        BacktracePanel_makePrintingHelper(&rows, &mut h);

        // basename "libc.so.6" = 9 chars, "ld.so" = 5 -> max 9
        assert_eq!(h.maxObjNameLen, 9);
        // full path "/usr/lib/libc.so.6" = 18 chars -> max 18
        assert_eq!(h.maxObjPathLen, 18);
        // max index 150 -> 3 decimal digits
        assert_eq!(h.maxFrameNumLen, 3);
        // max address 0x10000 = 65536 -> 5 hex digits
        assert_eq!(h.maxAddrLen, 5);
        // first frame carried a demangled name
        assert!(h.hasDemangledNames);
    }

    #[test]
    fn make_printing_helper_respects_incoming_floors() {
        // no DATA_FRAME rows: widths stay at their seeded floors,
        // demangled flag resets to false, and the digit counts of the
        // zero maxima (countDigits(0,10)=1, countDigits(0,16)=1) do not
        // lower the seeded 1 / 5 floors.
        let rows = vec![
            BacktracePanelRow {
                type_: BACKTRACE_PANEL_ROW_PROCESS_INFORMATION,
                frame: None,
            },
            BacktracePanelRow {
                type_: BACKTRACE_PANEL_ROW_ERROR,
                frame: None,
            },
        ];
        let mut h = seeded_helper();
        h.hasDemangledNames = true; // must be cleared by the function
        BacktracePanel_makePrintingHelper(&rows, &mut h);

        assert_eq!(h.maxObjNameLen, 4);
        assert_eq!(h.maxObjPathLen, 4);
        assert_eq!(h.maxFrameNumLen, 1);
        assert_eq!(h.maxAddrLen, 5);
        assert!(!h.hasDemangledNames);
    }

    #[test]
    fn make_printing_helper_short_names_do_not_shrink_floor() {
        // a frame with a 2-char basename must not drop maxObjNameLen below
        // the seeded "FILE"=4 floor.
        let rows = vec![frame(1, 0x1, Some("ab"), None)];
        let mut h = seeded_helper();
        BacktracePanel_makePrintingHelper(&rows, &mut h);
        assert_eq!(h.maxObjNameLen, 4);
        assert_eq!(h.maxObjPathLen, 4);
    }
}
