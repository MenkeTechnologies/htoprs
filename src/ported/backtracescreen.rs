//! Partial port of `BacktraceScreen.c` — htop's process backtrace panel.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
//!
//! Ported (self-contained, or on already-ported substrate):
//! - `BacktraceFrameData_new` (`:70`) — field-init constructor.
//! - `getBasename` (`:119`) — pure string basename (`strrchr`).
//! - `BacktracePanel_makePrintingHelper` (`:124`) — pure column-width pass
//!   over the panel rows (reuses `xutils::countDigits`).
//! - `BacktracePanel_displayHeader` (`:90`) — builds the `printf`-formatted
//!   column header from `printingHelper`/`displayOptions` and installs it
//!   via the ported [`Panel_setHeader`]. The C `%*s` / `%-*s` width
//!   specifiers map to Rust `{:>w$}` / `{:<w$}`.
//! - `BacktracePanel_makeBacktrace` (`:158`) — the `#else`
//!   (`!HAVE_LIBUNWIND_PTRACE`) branch, which is the variant this crate
//!   actually compiles (no libunwind dependency): sets `*error` to the
//!   fixed "not implemented" message. The `HAVE_LIBUNWIND_PTRACE` branch
//!   delegates to `UnwindPtrace_makeBacktrace` (unported
//!   `generic/UnwindPtrace.c`), so it is not reproduced.
//! - `BacktracePanelRow_displayError` (`:416`) — appends the row's own
//!   error string in `CRT_colors[DEFAULT_COLOR]` via
//!   [`RichString_appendAscii`].
//! - `BacktracePanelRow_display` (`:425`) — the dispatch switch; the
//!   `BACKTRACE_PANEL_ROW_ERROR` arm is live (calls `displayError`), the
//!   other two arms are blocked (below) and stay `todo!()`, mirroring the
//!   `ListItem_display` partial-port precedent.
//!
//! Stubbed (cannot be ported faithfully yet — blocker named on each):
//! - `BacktraceFrameData_delete` (`:82`), `BacktracePanel_delete` (`:277`),
//!   `BacktracePanelRow_delete` (`:450`) — pure `free()` / `Vector_delete`
//!   chains; owned Rust fields are released by `Drop`, so there is no body
//!   to port (same call as `History_delete`).
//! - `BacktracePanelRow_displayInformation` (`:308`) and
//!   `BacktracePanelRow_highlightBasename` (`:283`) — read the row's
//!   `const Process*` back-pointer. A row that borrows a `&Process` cannot
//!   be `'static`, so it cannot be stored in `Panel.items`
//!   (`Vec<Box<dyn Object>>`); the back-pointer is not modeled (the same
//!   friction `MainPanel.state` documents).
//! - `BacktracePanelRow_displayFrame` (`:356`) — reads the row's
//!   `const BacktracePanel*` back-pointer (above) AND
//!   `settings->highlightBaseName`, a `Settings` field the partial
//!   `settings.rs` port does not model.
//! - `BacktracePanel_populateFrames` (`:168`) — adds rows to the panel as
//!   `Object`s (`Panel_add`), but rows carry the `Process` /
//!   `BacktracePanel` back-pointers above and so cannot be
//!   `Box<dyn Object>`.
//! - `BacktracePanel_eventHandler` (`:208`) — returns the unported
//!   `HandlerResult` enum and, on refresh, calls `populateFrames` (blocked).
//! - `BacktracePanel_new` (`:248`) — calls `populateFrames` (blocked) and
//!   reads `settings->showProgramPath` (unmodeled `Settings` field).
//! - `BacktracePanelRow_new` (`:444`) — its sole non-default action is
//!   `this->panel = panel`, the unmodeled `const BacktracePanel*`
//!   back-pointer.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::panel::{Panel, Panel_setHeader};
use crate::ported::process::Process;
use crate::ported::richstring::{RichString, RichString_appendAscii};
use crate::ported::settings::Settings;

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
/// used by the ported functions. The C `union { BacktraceFrameData* frame;
/// char* error; }` is modeled as two owned `Option`s — `frame` (the
/// `BACKTRACE_PANEL_ROW_DATA_FRAME` arm, read by
/// [`BacktracePanel_makePrintingHelper`]) and `error` (the
/// `BACKTRACE_PANEL_ROW_ERROR` arm, read by
/// [`BacktracePanelRow_displayError`]) — only one of which is set per
/// `type_`. The C `panel` / `process` back-pointers are omitted: a row
/// that borrows them could not be `'static` (see the module docs), and
/// none of the ported functions touch them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BacktracePanelRow {
    pub type_: i32,
    pub frame: Option<BacktraceFrameData>,
    pub error: Option<String>,
}

/// Port of `enum BacktraceScreenDisplayOptions_` (`BacktraceScreen.c:65`) —
/// the bitmask stored in `BacktracePanel.displayOptions`.
const DEMANGLE_NAME_FUNCTION: i32 = 1 << 0;
const SHOW_FULL_PATH_OBJECT: i32 = 1 << 1;

/// Model of `BacktracePanel` (`BacktraceScreen.h:40`). Embeds `Panel super`
/// as `super_` (the Rust-keyword workaround the ported panels use). The C
/// `Vector* processes` and `const Settings*` are borrowed handles; they are
/// modeled as raw pointers so the struct stays `'static` (the
/// `MainPanel.state` back-pointer precedent). Only `super_` /
/// `printingHelper` / `displayOptions` are read by the ported functions;
/// the raw-pointer fields exist to keep the struct a faithful shape and are
/// never dereferenced here.
pub struct BacktracePanel {
    pub super_: Panel,
    pub processes: Vec<*const Process>,
    pub printingHelper: BacktracePanelPrintingHelper,
    pub settings: *const Settings,
    pub displayOptions: i32,
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

/// TODO: port of `void BacktraceFrameData_delete(Object* object)` from
/// `BacktraceScreen.c:82`. Pure `free()` chain (the three `char*` fields +
/// the struct); `BacktraceFrameData` owns its `Option<String>` fields and
/// frees them via `Drop`, so there is no body to port (same as
/// `History_delete`).
pub fn BacktraceFrameData_delete() {
    todo!("port of BacktraceScreen.c:82")
}

/// Port of `static void BacktracePanel_displayHeader(BacktracePanel* this)`
/// from `BacktraceScreen.c:90`. Formats the fixed column header — a
/// right-justified `#`, then left-justified `ADDRESS` / `FILE` columns
/// sized to `printingHelper`, then the `NAME` / `NAME (demangled)` label
/// chosen by `displayOptions` — and installs it via the ported
/// [`Panel_setHeader`]. The C `%*s` (right) and `%-*s` (left) width
/// specifiers map to Rust `{:>w$}` / `{:<w$}`; the C `INT_MAX` overflow
/// asserts on the `(int)` width casts become `debug_assert!`s (Rust format
/// widths are already `usize`, so no cast can overflow).
pub fn BacktracePanel_displayHeader(this: &mut BacktracePanel) {
    let displayOptions = this.displayOptions;

    let showDemangledNames =
        (displayOptions & DEMANGLE_NAME_FUNCTION) != 0 && this.printingHelper.hasDemangledNames;

    let showFullPathObject = (displayOptions & SHOW_FULL_PATH_OBJECT) != 0;
    let maxObjLen = if showFullPathObject {
        this.printingHelper.maxObjPathLen
    } else {
        this.printingHelper.maxObjNameLen
    };

    let maxFrameNumLen = this.printingHelper.maxFrameNumLen;
    let maxAddrLen = this.printingHelper.maxAddrLen;

    // The parameters for printf are of type int; guard against overflow of
    // the (int) width casts, exactly as the C asserts do.
    debug_assert!(maxFrameNumLen <= i32::MAX as usize);
    debug_assert!(maxAddrLen <= i32::MAX as usize - "0x".len());
    debug_assert!(maxObjLen <= i32::MAX as usize);

    let name = if showDemangledNames {
        "NAME (demangled)"
    } else {
        "NAME"
    };

    let line = format!(
        "{:>fnw$} {:<addrw$} {:<objw$} {}",
        "#",
        "ADDRESS",
        "FILE",
        name,
        fnw = maxFrameNumLen,
        addrw = maxAddrLen + "0x".len(),
        objw = maxObjLen,
    );

    Panel_setHeader(&mut this.super_, &line);
}

/// Port of `static void BacktracePanel_makeBacktrace(Vector* frames, pid_t
/// pid, char** error)` from `BacktraceScreen.c:158`, the `#else`
/// (`!HAVE_LIBUNWIND_PTRACE`) branch — the variant this crate compiles, as
/// it has no libunwind dependency. It ignores `frames` / `pid` and sets
/// `*error` to the fixed "not implemented" message (C
/// `xAsprintf(error, "The backtrace screen is not implemented")`). The
/// `HAVE_LIBUNWIND_PTRACE` branch delegates to `UnwindPtrace_makeBacktrace`
/// (`generic/UnwindPtrace.c`, unported), which has no analog without a
/// libunwind/ptrace substrate. `pid` is the C `pid_t` (an `int`).
pub fn BacktracePanel_makeBacktrace(
    frames: &mut Vec<BacktraceFrameData>,
    pid: i32,
    error: &mut Option<String>,
) {
    let _ = frames;
    let _ = pid;
    *error = Some("The backtrace screen is not implemented".to_string());
}

/// TODO: port of `static void BacktracePanel_populateFrames(BacktracePanel*
/// this)` from `BacktraceScreen.c:168`. Blocked: it appends
/// `BacktracePanelRow`s to the panel as `Object`s (`Panel_add`), but a row
/// carries the `const Process*` / `const BacktracePanel*` back-pointers,
/// which prevent it from being `'static` and therefore from being stored as
/// `Box<dyn Object>` in `Panel.items`.
pub fn BacktracePanel_populateFrames() {
    todo!("port of BacktraceScreen.c:168")
}

/// TODO: port of `static HandlerResult BacktracePanel_eventHandler(Panel*
/// super, int ch)` from `BacktraceScreen.c:208`. Blocked: returns the
/// unported `HandlerResult` enum, and its refresh arm calls
/// `BacktracePanel_populateFrames` (itself blocked).
pub fn BacktracePanel_eventHandler() {
    todo!("port of BacktraceScreen.c:208")
}

/// TODO: port of `BacktracePanel* BacktracePanel_new(Vector* processes,
/// const Settings* settings)` from `BacktraceScreen.c:248`. Blocked: reads
/// `settings->showProgramPath` (a `Settings` field the partial `settings.rs`
/// port does not model) and calls `BacktracePanel_populateFrames` (blocked).
pub fn BacktracePanel_new() {
    todo!("port of BacktraceScreen.c:248")
}

/// TODO: port of `void BacktracePanel_delete(Object* object)` from
/// `BacktraceScreen.c:277`. Pure `Vector_delete(processes)` + `Panel_delete`
/// free chain; the owned Rust fields are released by `Drop`, so there is no
/// body to port.
pub fn BacktracePanel_delete() {
    todo!("port of BacktraceScreen.c:277")
}

/// TODO: port of `static void BacktracePanelRow_highlightBasename(const
/// BacktracePanelRow* row, RichString* out, char* line, int
/// objectPathStart)` from `BacktraceScreen.c:283`. Blocked: reads the row's
/// `const Process*` back-pointer (`process->procExe` /
/// `procExeBasenameOffset`), which is not modeled — a row borrowing a
/// `&Process` cannot be `'static` (see the module docs).
pub fn BacktracePanelRow_highlightBasename() {
    todo!("port of BacktraceScreen.c:283")
}

/// TODO: port of `static void BacktracePanelRow_displayInformation(const
/// Object* super, RichString* out)` from `BacktraceScreen.c:308`. Blocked:
/// reads the row's `const Process*` back-pointer (`mergedCommand` /
/// `cmdline` / `Process_isThread` / `Process_getPid`), not modeled (above).
pub fn BacktracePanelRow_displayInformation() {
    todo!("port of BacktraceScreen.c:308")
}

/// TODO: port of `static void BacktracePanelRow_displayFrame(const Object*
/// super, RichString* out)` from `BacktraceScreen.c:356`. Blocked: reads the
/// row's `const BacktracePanel*` back-pointer (`printingHelper` /
/// `displayOptions`, above) AND `panel->settings->highlightBaseName`, a
/// `Settings` field the partial `settings.rs` port does not model.
pub fn BacktracePanelRow_displayFrame() {
    todo!("port of BacktraceScreen.c:356")
}

/// Port of `static void BacktracePanelRow_displayError(const Object* super,
/// RichString* out)` from `BacktraceScreen.c:416`. Appends the row's own
/// error string (the `data.error` union arm) in `CRT_colors[DEFAULT_COLOR]`
/// via the ported [`RichString_appendAscii`]. The C `assert`s on the row
/// type and a non-NULL error become `debug_assert!` / an `expect`.
pub fn BacktracePanelRow_displayError(row: &BacktracePanelRow, out: &mut RichString) {
    debug_assert_eq!(row.type_, BACKTRACE_PANEL_ROW_ERROR);
    let error = row
        .error
        .as_deref()
        .expect("ERROR row must carry an error message");
    let color = ColorElements::DEFAULT_COLOR.packed(ColorScheme::active());
    RichString_appendAscii(out, color, error.as_bytes());
}

/// Port of `static void BacktracePanelRow_display(const Object* super,
/// RichString* out)` from `BacktraceScreen.c:425`. Dispatches on the row
/// type. The `BACKTRACE_PANEL_ROW_ERROR` arm is ported (calls
/// [`BacktracePanelRow_displayError`]); the `DATA_FRAME` and
/// `PROCESS_INFORMATION` arms call `displayFrame` / `displayInformation`,
/// which read the row's unmodeled `BacktracePanel` / `Process`
/// back-pointers, so those arms stay `todo!()` — the same partial-port
/// shape as `ListItem_display`.
pub fn BacktracePanelRow_display(row: &BacktracePanelRow, out: &mut RichString) {
    match row.type_ {
        BACKTRACE_PANEL_ROW_DATA_FRAME => {
            todo!("BacktraceScreen.c:431 — BacktracePanelRow_displayFrame reads row->panel (unmodeled back-pointer) + settings->highlightBaseName (unported)")
        }
        BACKTRACE_PANEL_ROW_PROCESS_INFORMATION => {
            todo!("BacktraceScreen.c:435 — BacktracePanelRow_displayInformation reads row->process (unmodeled back-pointer)")
        }
        BACKTRACE_PANEL_ROW_ERROR => BacktracePanelRow_displayError(row, out),
        _ => {}
    }
}

/// TODO: port of `BacktracePanelRow* BacktracePanelRow_new(const
/// BacktracePanel* panel)` from `BacktraceScreen.c:444`. Blocked: after
/// `AllocThis` zero-inits the row, its sole action is `this->panel = panel`
/// — storing the unmodeled `const BacktracePanel*` back-pointer. Porting it
/// without that field would drop the one meaningful assignment.
pub fn BacktracePanelRow_new() {
    todo!("port of BacktraceScreen.c:444")
}

/// TODO: port of `void BacktracePanelRow_delete(Object* object)` from
/// `BacktraceScreen.c:450`. Pure free chain (`BacktraceFrameData_delete` for
/// a frame row, `free(error)` for an error row, then `free(this)`); the
/// owned Rust fields are released by `Drop`, so there is no body to port.
pub fn BacktracePanelRow_delete() {
    todo!("port of BacktraceScreen.c:450")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::panel::Panel_new;

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
            error: None,
        }
    }

    /// Visible characters of the valid `[0, chlen)` range of a RichString.
    fn rendered(rs: &RichString) -> String {
        (0..rs.chlen as usize).map(|i| rs.chptr[i].chars).collect()
    }

    /// Visible characters of a BacktracePanel's installed header.
    fn header_text(p: &BacktracePanel) -> String {
        rendered(&p.super_.header)
    }

    /// A BacktracePanel with a seeded printing helper and no frames — the
    /// state `BacktracePanel_new` leaves before `populateFrames` runs.
    /// `settings` is a null raw pointer (never dereferenced by the ported
    /// functions); the embedded panel is built by the ported `Panel_new`.
    fn empty_backtrace_panel() -> BacktracePanel {
        BacktracePanel {
            super_: Panel_new(1, 1, 0, 1, None),
            processes: Vec::new(),
            printingHelper: seeded_helper(),
            settings: std::ptr::null(),
            displayOptions: 0,
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
                error: None,
            },
            BacktracePanelRow {
                type_: BACKTRACE_PANEL_ROW_ERROR,
                frame: None,
                error: None,
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
                error: None,
            },
            BacktracePanelRow {
                type_: BACKTRACE_PANEL_ROW_ERROR,
                frame: None,
                error: None,
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

    // ── displayHeader ─────────────────────────────────────────────────

    #[test]
    fn display_header_formats_seeded_columns() {
        let mut p = empty_backtrace_panel();
        // seeded_helper: maxFrameNumLen=1, maxAddrLen=5, maxObjName/Path=4.
        // displayOptions=0 -> NAME (not demangled), basename column width.
        BacktracePanel_displayHeader(&mut p);
        // "#" right in 1, "ADDRESS" left in 5+2=7 (exact), "FILE" left in 4,
        // then "NAME".
        assert_eq!(header_text(&p), "# ADDRESS FILE NAME");
    }

    #[test]
    fn display_header_demangled_and_full_path_widen() {
        let mut p = empty_backtrace_panel();
        p.printingHelper = BacktracePanelPrintingHelper {
            maxAddrLen: 12,
            maxFrameNumLen: 3,
            maxObjPathLen: 18,
            maxObjNameLen: 9,
            hasDemangledNames: true,
        };
        p.displayOptions = DEMANGLE_NAME_FUNCTION | SHOW_FULL_PATH_OBJECT;
        BacktracePanel_displayHeader(&mut p);
        let expected = format!(
            "{:>3} {:<14} {:<18} {}",
            "#", "ADDRESS", "FILE", "NAME (demangled)"
        );
        assert_eq!(header_text(&p), expected);
    }

    #[test]
    fn display_header_demangle_option_without_demangled_names_shows_plain() {
        let mut p = empty_backtrace_panel();
        // Demangle requested, but no row carried a demangled name.
        p.printingHelper.hasDemangledNames = false;
        p.displayOptions = DEMANGLE_NAME_FUNCTION;
        BacktracePanel_displayHeader(&mut p);
        assert!(header_text(&p).ends_with("NAME"));
        assert!(!header_text(&p).contains("demangled"));
    }

    // ── makeBacktrace (non-libunwind branch) ──────────────────────────

    #[test]
    fn make_backtrace_sets_not_implemented_error() {
        let mut frames: Vec<BacktraceFrameData> = Vec::new();
        let mut error: Option<String> = None;
        BacktracePanel_makeBacktrace(&mut frames, 1234, &mut error);
        assert_eq!(
            error.as_deref(),
            Some("The backtrace screen is not implemented")
        );
        // The #else branch never populates frames.
        assert!(frames.is_empty());
    }

    // ── displayError ──────────────────────────────────────────────────

    #[test]
    fn display_error_appends_error_in_default_color() {
        let row = BacktracePanelRow {
            type_: BACKTRACE_PANEL_ROW_ERROR,
            frame: None,
            error: Some("ptrace failed".to_string()),
        };
        let mut out = RichString::new();
        BacktracePanelRow_displayError(&row, &mut out);
        assert_eq!(rendered(&out), "ptrace failed");
        // CRT_colors[DEFAULT_COLOR], masked as the ASCII write path masks.
        let expect = ColorElements::DEFAULT_COLOR.packed(ColorScheme::active()) & 0xffffff;
        for i in 0..out.chlen as usize {
            assert_eq!(out.chptr[i].attr, expect, "attr at {i}");
        }
    }

    // ── display dispatch ──────────────────────────────────────────────

    #[test]
    fn display_dispatches_error_arm_to_display_error() {
        let row = BacktracePanelRow {
            type_: BACKTRACE_PANEL_ROW_ERROR,
            frame: None,
            error: Some("boom".to_string()),
        };
        let mut out = RichString::new();
        BacktracePanelRow_display(&row, &mut out);
        assert_eq!(rendered(&out), "boom");
    }

    #[test]
    #[should_panic(expected = "displayFrame")]
    fn display_frame_arm_is_blocked() {
        let row = BacktracePanelRow {
            type_: BACKTRACE_PANEL_ROW_DATA_FRAME,
            frame: Some(BacktraceFrameData::default()),
            error: None,
        };
        let mut out = RichString::new();
        BacktracePanelRow_display(&row, &mut out);
    }

    #[test]
    #[should_panic(expected = "displayInformation")]
    fn display_information_arm_is_blocked() {
        let row = BacktracePanelRow {
            type_: BACKTRACE_PANEL_ROW_PROCESS_INFORMATION,
            frame: None,
            error: None,
        };
        let mut out = RichString::new();
        BacktracePanelRow_display(&row, &mut out);
    }
}
