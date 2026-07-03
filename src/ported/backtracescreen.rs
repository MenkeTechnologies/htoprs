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
//! - `BacktracePanel_eventHandler` (`:208`) — the key-dispatch switch;
//!   `HandlerResult` is now ported, so the `'p'`/`F3` full-path-toggle arm
//!   is live (relabels the bar, rebuilds the header). The `F5` refresh arm
//!   prunes then calls the still-blocked `populateFrames` (below), so it
//!   reaches that `todo!()` — the `ColumnsPanel_eventHandler` partial-port
//!   precedent. The `HAVE_DEMANGLING` `F2` arm is omitted (the crate's
//!   no-optional-dependency variant, as with `makeBacktrace`).
//! - `BacktracePanelRow_displayError` (`:416`) — appends the row's own
//!   error string in `CRT_colors[DEFAULT_COLOR]` via
//!   [`RichString_appendAscii`].
//! - `BacktracePanelRow_displayInformation` (`:308`) — renders the
//!   process-information header line, reading the row's `const Process*`
//!   back-pointer (modeled as a sound raw pointer to externally-owned memory
//!   — the same idiom `BacktracePanel.processes`/`settings` use). The C `%n`
//!   command-name offset becomes the length of the `"Thread %d: "` /
//!   `"Process %d: "` prefix.
//! - `BacktracePanelRow_highlightBasename` (`:283`) — repaints the object
//!   basename column when it matches the process executable's basename;
//!   reads the same `process` back-pointer. Its sole C caller
//!   (`displayFrame`) is still blocked, so it has no live caller yet.
//! - `BacktracePanelRow_display` (`:425`) — the dispatch switch; the
//!   `ERROR` arm (calls `displayError`) and the `PROCESS_INFORMATION` arm
//!   (calls the now-ported `displayInformation`) are live. The `DATA_FRAME`
//!   arm stays `todo!()` (its `displayFrame` needs the self-referential
//!   `panel` back-pointer), mirroring the `ListItem_display` partial-port.
//!
//! Stubbed (cannot be ported faithfully yet — blocker named on each):
//! - `BacktraceFrameData_delete` (`:82`), `BacktracePanel_delete` (`:277`),
//!   `BacktracePanelRow_delete` (`:450`) — pure `free()` / `Vector_delete`
//!   chains; owned Rust fields are released by `Drop`, so there is no body
//!   to port (same call as `History_delete`).
//! - `BacktracePanelRow_displayFrame` (`:356`) — reads the row's
//!   `const BacktracePanel* panel` back-pointer for `panel->printingHelper`
//!   / `panel->displayOptions` / `panel->settings->highlightBaseName`.
//!   Unlike the `process` back-pointer, `panel` is **self-referential** (the
//!   panel owns the row in `super.items` while the row points back at the
//!   panel), which the owned-value model cannot keep address-stable across a
//!   by-value move — porting it would dangle. Its formatting body and the
//!   `highlightBasename` callee are ported; only the `panel` deref is missing.
//! - `BacktracePanel_populateFrames` (`:168`) — adds rows to the panel as
//!   `Object`s (`Panel_add`) with `row->panel = this`, the self-referential
//!   panel back-pointer above; the rows would also need an `Object` vtable
//!   impl the row does not yet carry.
//! - `BacktracePanel_new` (`:248`) — calls `populateFrames` (blocked); it
//!   also stores the self-referential panel by value on return, which would
//!   dangle every row's `panel` back-pointer.
//! - `BacktracePanelRow_new` (`:444`) — its sole non-default action is
//!   `this->panel = panel`, the self-referential `const BacktracePanel*`
//!   back-pointer above.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::crt::{A_BOLD, ColorElements, ColorScheme, KEY_CTRL, KEY_F};
use crate::ported::functionbar::FunctionBar_setLabel;
use crate::ported::panel::{HandlerResult, Panel, Panel_delete, Panel_prune, Panel_setHeader};
use crate::ported::process::{
    CMDLINE_HIGHLIGHT_FLAG_BASENAME, Process, Process_getPid, Process_isThread,
};
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendnWide, RichString_setAttrn,
};
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
/// `type_`.
///
/// `process` is the C `const Process* process` back-pointer
/// (`BacktraceScreen.h`), modeled as a raw `*const Process` — a borrowed
/// handle to a process **owned outside** the panel (one of
/// `BacktracePanel.processes`), the same raw-back-pointer idiom the enclosing
/// [`BacktracePanel`] already uses for its own `processes`/`settings`. It is
/// read (via `unsafe` deref) by [`BacktracePanelRow_displayInformation`] and
/// [`BacktracePanelRow_highlightBasename`]. The C `const BacktracePanel*
/// panel` back-pointer is still omitted: unlike `process` it is
/// **self-referential** (the panel owns the row in `super.items` while the
/// row points back at the panel), which the owned-value model cannot make
/// address-stable — see the [`BacktracePanelRow_displayFrame`] /
/// [`BacktracePanel_populateFrames`] blockers in the module docs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BacktracePanelRow {
    pub type_: i32,
    pub frame: Option<BacktraceFrameData>,
    pub error: Option<String>,
    /// C `const Process* process` — the process this row describes; a raw
    /// back-pointer to externally-owned memory (`Default` = null). Never
    /// dereferenced unless the row was built pointing at a live `Process`.
    pub process: *const Process,
}

/// Port of `enum BacktraceScreenDisplayOptions_` (`BacktraceScreen.c:65`) —
/// the bitmask stored in `BacktracePanel.displayOptions`.
const DEMANGLE_NAME_FUNCTION: i32 = 1 << 0;
const SHOW_FULL_PATH_OBJECT: i32 = 1 << 1;

/// Key-code constants for the [`BacktracePanel_eventHandler`] dispatch.
/// The C `switch` uses `KEY_F(3)` / `KEY_F(5)` / `KEY_CTRL('L')` / `'p'`
/// directly, but Rust `match` patterns cannot contain `const fn` calls or
/// casts, so they are bound to `const`s here (the same idiom `ColumnsPanel`
/// uses for its `KEY_F7` / `KEY_F8` labels).
const KEY_F3: i32 = KEY_F(3);
const KEY_F5: i32 = KEY_F(5);
const KEY_CTRL_L: i32 = KEY_CTRL(b'L' as i32);
const KEY_LOWER_P: i32 = b'p' as i32;

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

/// Port of `void BacktraceFrameData_delete(Object* object)` from
/// `BacktraceScreen.c:82`: `free(functionName); free(demangleFunctionName);
/// free(objectPath); free(this);`. Taking `this` by value consumes the
/// frame; the three owned `Option<String>` fields and the struct drop
/// together — the whole C free chain.
pub fn BacktraceFrameData_delete(this: BacktraceFrameData) {
    let _ = this;
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

/// Port of `static HandlerResult BacktracePanel_eventHandler(Panel* super,
/// int ch)` from `BacktraceScreen.c:208`. Dispatches keyboard input; the C
/// always returns the initial `IGNORED` (no arm reassigns `result`), so this
/// returns `HandlerResult::IGNORED`. The `HAVE_DEMANGLING`-gated `KEY_F(2)`
/// arm is omitted — this crate compiles the no-optional-dependency variant
/// (the same choice the ported [`BacktracePanel_makeBacktrace`] makes for
/// `!HAVE_LIBUNWIND_PTRACE`), so `HAVE_DEMANGLING` is treated as undefined.
/// The `'p'` / `KEY_F(3)` (full-path toggle) arm is fully ported: it flips
/// [`SHOW_FULL_PATH_OBJECT`], relabels the function bar via the ported
/// [`FunctionBar_setLabel`], marks the panel dirty, and rebuilds the header
/// via [`BacktracePanel_displayHeader`]. The `KEY_CTRL('L')` / `KEY_F(5)`
/// refresh arm prunes the panel (ported [`Panel_prune`]) and then calls
/// [`BacktracePanel_populateFrames`], which is still a documented `todo!()`
/// (below); that call therefore panics before returning — the honest
/// transitive block, the same partial-port shape as `ColumnsPanel_eventHandler`.
pub fn BacktracePanel_eventHandler(this: &mut BacktracePanel, ch: i32) -> HandlerResult {
    let result = HandlerResult::IGNORED;

    match ch {
        // C: case 'p': case KEY_F(3):
        KEY_LOWER_P | KEY_F3 => {
            this.displayOptions ^= SHOW_FULL_PATH_OBJECT;

            let showFullPathObject = (this.displayOptions & SHOW_FULL_PATH_OBJECT) != 0;
            if let Some(bar) = this.super_.defaultBar.as_mut() {
                FunctionBar_setLabel(
                    bar,
                    KEY_F(3),
                    if showFullPathObject {
                        "Basename "
                    } else {
                        "Full Path"
                    },
                );
            }

            this.super_.needsRedraw = true;
            BacktracePanel_displayHeader(this);
        }

        // C: case KEY_CTRL('L'): case KEY_F(5):
        KEY_CTRL_L | KEY_F5 => {
            Panel_prune(&mut this.super_);
            BacktracePanel_populateFrames();
        }

        _ => {}
    }

    result
}

/// TODO: port of `BacktracePanel* BacktracePanel_new(Vector* processes,
/// const Settings* settings)` from `BacktraceScreen.c:248`. Blocked: reads
/// `settings->showProgramPath` (a `Settings` field the partial `settings.rs`
/// port does not model) and calls `BacktracePanel_populateFrames` (blocked).
pub fn BacktracePanel_new() {
    todo!("port of BacktraceScreen.c:248")
}

/// Port of `void BacktracePanel_delete(Object* object)` from
/// `BacktraceScreen.c:277`: `Vector_delete(this->processes);
/// Panel_delete(object);`. Taking `this` by value consumes the panel. The
/// `processes` list is a `Vec<*const Process>` of non-owning aliases (C's
/// non-owner `Vector`), so dropping it frees only the array, not the
/// pointees — matching C's `Vector_delete`; the embedded `super_` [`Panel`]
/// is handed to [`Panel_delete`] (mirroring the C call graph), and the
/// remaining scalar/back-pointer fields drop with it.
pub fn BacktracePanel_delete(this: BacktracePanel) {
    let BacktracePanel {
        super_, processes, ..
    } = this;
    // C: Vector_delete(this->processes) — non-owning aliases; the Vec drop
    // reclaims the array only.
    let _ = processes;
    Panel_delete(super_);
}

/// Port of `static void BacktracePanelRow_highlightBasename(const
/// BacktracePanelRow* row, RichString* out, char* line, int
/// objectPathStart)` from `BacktraceScreen.c:283`. Reads the row's
/// [`process`](BacktracePanelRow::process) back-pointer (a sound raw
/// pointer to externally-owned memory) for `procExe` /
/// `procExeBasenameOffset`, scans the object column of the pre-formatted
/// `line` for its basename, and — when that basename matches the process's
/// own executable basename — repaints it in `CRT_colors[PROCESS_BASENAME]`
/// via [`RichString_setAttrn`]. The C `char* line` becomes `&str`; the C
/// `strncmp(line + lastSlash, procExe, sizeBasename) == 0` (which stops at
/// `procExe`'s NUL) is reproduced as `procExe.len() >= sizeBasename &&
/// procExe[..sizeBasename] == line-slice`. The C `assert`s on the row type
/// and `objectPathStart >= 0` become a `debug_assert!` (the `usize`
/// `objectPathStart` is `>= 0` by construction).
///
/// Its sole C caller is [`BacktracePanelRow_displayFrame`], which is still a
/// documented `todo!()` (the self-referential `panel` back-pointer), so this
/// has no live caller in the port yet; the body is a faithful standalone port.
pub fn BacktracePanelRow_highlightBasename(
    row: &BacktracePanelRow,
    out: &mut RichString,
    line: &str,
    objectPathStart: usize,
) {
    debug_assert_eq!(row.type_, BACKTRACE_PANEL_ROW_DATA_FRAME);

    // C: const Process* process = row->process;
    let process: &Process = unsafe { &*row.process };

    // C: char* procExe = process->procExe ? process->procExe + process->procExeBasenameOffset : NULL;
    //    if (!procExe) return;
    let procExe: &[u8] = match process.procExe.as_deref() {
        Some(s) => &s.as_bytes()[process.procExeBasenameOffset..],
        None => return,
    };

    let line_b = line.as_bytes();

    // C: size_t endBasenameIndex = objectPathStart; size_t lastSlashBasenameIndex = objectPathStart;
    //    for (; line[end] != 0 && line[end] != ' '; end++)
    //       if (line[end] == '/') lastSlash = end + 1;
    // (a `&str` has no interior NUL terminator; `line.len()` bounds the scan.)
    let mut endBasenameIndex = objectPathStart;
    let mut lastSlashBasenameIndex = objectPathStart;
    while endBasenameIndex < line_b.len() && line_b[endBasenameIndex] != b' ' {
        if line_b[endBasenameIndex] == b'/' {
            lastSlashBasenameIndex = endBasenameIndex + 1;
        }
        endBasenameIndex += 1;
    }

    // C: size_t sizeBasename = endBasenameIndex - lastSlashBasenameIndex;
    let sizeBasename = endBasenameIndex - lastSlashBasenameIndex;

    // C: if (strncmp(line + lastSlash, procExe, sizeBasename) == 0)
    //        RichString_setAttrn(out, CRT_colors[PROCESS_BASENAME], lastSlash, sizeBasename);
    // strncmp compares `sizeBasename` bytes, stopping at procExe's NUL: it can
    // only be equal when procExe holds at least that many bytes.
    let lineSlice = &line_b[lastSlashBasenameIndex..lastSlashBasenameIndex + sizeBasename];
    if procExe.len() >= sizeBasename && &procExe[..sizeBasename] == lineSlice {
        RichString_setAttrn(
            out,
            ColorElements::PROCESS_BASENAME.packed(ColorScheme::active()),
            lastSlashBasenameIndex,
            sizeBasename,
        );
    }
}

/// Port of `static void BacktracePanelRow_displayInformation(const
/// Object* super, RichString* out)` from `BacktraceScreen.c:308`. Reads the
/// row's [`process`](BacktracePanelRow::process) back-pointer (a sound raw
/// pointer to externally-owned memory) and renders the process-information
/// header line (`"Thread %d: %s"` / `"Process %d: %s"`) with the command
/// name highlighted in `PROCESS_THREAD_BASENAME` / `PROCESS_BASENAME`.
///
/// The command name comes from `process->mergedCommand.str` (with the first
/// `CMDLINE_HIGHLIGHT_FLAG_BASENAME` highlight's offset/length) or, failing
/// that, `process->cmdline`. The C `%n` conversion (which captures the byte
/// offset of the `%s` command name within the formatted string) is
/// reproduced as the length of the `"Thread %d: "` / `"Process %d: "`
/// prefix. The C `xAsprintf` -> Rust `format!`; `RichString_appendnWide`
/// and `RichString_setAttrn` are the ported RichString ops.
///
/// The C loop reads `process->mergedCommand.highlights` (the array decays to
/// `&highlights[0]`) on **every** iteration rather than `&highlights[i]`;
/// this reproduces that behavior faithfully (it inspects `highlights[0]`).
pub fn BacktracePanelRow_displayInformation(row: &BacktracePanelRow, out: &mut RichString) {
    debug_assert_eq!(row.type_, BACKTRACE_PANEL_ROW_PROCESS_INFORMATION);

    // C: const Process* process = row->process;
    let process: &Process = unsafe { &*row.process };

    // C: int colorBasename = DEFAULT_COLOR; size_t highlightLen = 0; size_t highlightOffset = 0;
    // (the C `DEFAULT_COLOR` seed is always overwritten in the thread/process
    // branch below, so it is left to that definite assignment here.)
    let colorBasename;
    let mut highlightLen: usize = 0;
    let mut highlightOffset: usize = 0;

    // C: const char* processName = "";
    //    if (process->mergedCommand.str) { processName = ...; for (...) BASENAME highlight }
    //    else if (process->cmdline) processName = process->cmdline;
    let processName: &str = if let Some(s) = process.mergedCommand.str.as_deref() {
        for _i in 0..process.mergedCommand.highlightCount {
            // C: const ProcessCmdlineHighlight* highlight = process->mergedCommand.highlights;
            // (the array decays to &highlights[0] — the C inspects [0] each pass).
            let highlight = &process.mergedCommand.highlights[0];
            if highlight.flags & CMDLINE_HIGHLIGHT_FLAG_BASENAME != 0 {
                highlightLen = highlight.length;
                highlightOffset = highlight.offset;
                break;
            }
        }
        s
    } else if let Some(c) = process.cmdline.as_deref() {
        c
    } else {
        ""
    };

    // C: if (highlightLen == 0) highlightLen = strlen(processName);
    if highlightLen == 0 {
        highlightLen = processName.len();
    }

    // C: xAsprintf(&information, "Thread %d: %n%s", Process_getPid(process), &indexProcessComm, processName)
    //    (or "Process %d: %n%s"); %n captures the byte offset before %s.
    let pid = Process_getPid(process);
    let verb = if Process_isThread(process) {
        colorBasename = ColorElements::PROCESS_THREAD_BASENAME;
        "Thread"
    } else {
        colorBasename = ColorElements::PROCESS_BASENAME;
        "Process"
    };
    let prefix = format!("{} {}: ", verb, pid);
    let indexProcessComm = prefix.len(); // the C `%n` capture (always set)
    let information = format!("{}{}", prefix, processName);
    let len = information.len();

    let scheme = ColorScheme::active();

    // C: RichString_appendnWide(out, CRT_colors[DEFAULT_COLOR] | A_BOLD, information, len);
    RichString_appendnWide(
        out,
        ColorElements::DEFAULT_COLOR.packed(scheme) | A_BOLD,
        information.as_bytes(),
        len,
    );

    // C: if (indexProcessComm != -1) RichString_setAttrn(out, CRT_colors[colorBasename] | A_BOLD,
    //        indexProcessComm + highlightOffset, highlightLen);
    // indexProcessComm is always set here (the prefix is always written).
    RichString_setAttrn(
        out,
        colorBasename.packed(scheme) | A_BOLD,
        indexProcessComm + highlightOffset,
        highlightLen,
    );
}

/// TODO: port of `static void BacktracePanelRow_displayFrame(const Object*
/// super, RichString* out)` from `BacktraceScreen.c:356`. Blocked on the
/// row's `const BacktracePanel* panel` back-pointer, which it dereferences
/// for `panel->printingHelper` / `panel->displayOptions` /
/// `panel->settings->highlightBaseName`. Unlike the `process` back-pointer
/// (externally owned, ported as a sound raw pointer), `panel` is
/// **self-referential** — the panel owns this row in `super.items` while the
/// row points back at the panel — and the owned-value model cannot keep that
/// pointer address-stable across a by-value move of the panel. Porting it
/// would introduce a dangling self-reference (the same blocker that gates
/// [`BacktracePanel_populateFrames`] / [`BacktracePanel_new`] /
/// [`BacktracePanelRow_new`]). The frame-formatting logic itself and its
/// [`BacktracePanelRow_highlightBasename`] callee are ported; only the
/// `panel` deref is missing substrate.
pub fn BacktracePanelRow_displayFrame() {
    todo!("port of BacktraceScreen.c:356 — reads row->panel (self-referential back-pointer, no address-stable model)")
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
/// type. The `BACKTRACE_PANEL_ROW_ERROR` arm calls
/// [`BacktracePanelRow_displayError`] and the `BACKTRACE_PANEL_ROW_PROCESS_INFORMATION`
/// arm calls [`BacktracePanelRow_displayInformation`] (both now ported — they
/// read only the sound `row->process` back-pointer). The `DATA_FRAME` arm calls
/// [`BacktracePanelRow_displayFrame`], which reads the row's **self-referential**
/// `const BacktracePanel* panel` back-pointer (not modeled — see the module
/// docs), so that arm stays `todo!()` — the same partial-port shape as
/// `ListItem_display`.
pub fn BacktracePanelRow_display(row: &BacktracePanelRow, out: &mut RichString) {
    match row.type_ {
        BACKTRACE_PANEL_ROW_DATA_FRAME => {
            todo!("BacktraceScreen.c:431 — BacktracePanelRow_displayFrame reads row->panel (self-referential back-pointer, no address-stable model) + settings->highlightBaseName")
        }
        BACKTRACE_PANEL_ROW_PROCESS_INFORMATION => {
            BacktracePanelRow_displayInformation(row, out)
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

/// Port of `void BacktracePanelRow_delete(Object* object)` from
/// `BacktraceScreen.c:450`: `switch (this->type) { case
/// BACKTRACE_PANEL_ROW_DATA_FRAME: BacktraceFrameData_delete(data.frame);
/// case BACKTRACE_PANEL_ROW_ERROR: free(data.error); } free(this);`.
///
/// Taking `this` by value consumes the row. The C `data` union is modeled
/// as separate `frame`/`error` `Option` fields; the switch on `type_`
/// mirrors the C: a frame row hands its owned [`BacktraceFrameData`] to
/// [`BacktraceFrameData_delete`], an error row drops its owned `error`
/// `String`. The inactive-arm field is `None` (union invariant), so it drops
/// as a no-op; the `process` back-pointer is a non-owning raw pointer.
pub fn BacktracePanelRow_delete(this: BacktracePanelRow) {
    let BacktracePanelRow {
        type_,
        frame,
        error,
        process,
    } = this;
    match type_ {
        BACKTRACE_PANEL_ROW_DATA_FRAME => {
            if let Some(frame) = frame {
                BacktraceFrameData_delete(frame);
            }
            let _ = error;
        }
        BACKTRACE_PANEL_ROW_ERROR => {
            let _ = error;
            let _ = frame;
        }
        _ => {
            let _ = frame;
            let _ = error;
        }
    }
    let _ = process;
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
            process: std::ptr::null(),
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
                process: std::ptr::null(),
            },
            BacktracePanelRow {
                type_: BACKTRACE_PANEL_ROW_ERROR,
                frame: None,
                error: None,
                process: std::ptr::null(),
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
                process: std::ptr::null(),
            },
            BacktracePanelRow {
                type_: BACKTRACE_PANEL_ROW_ERROR,
                frame: None,
                error: None,
                process: std::ptr::null(),
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
            process: std::ptr::null(),
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
            process: std::ptr::null(),
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
            process: std::ptr::null(),
        };
        let mut out = RichString::new();
        BacktracePanelRow_display(&row, &mut out);
    }

    // ── displayInformation (reads the sound `process` back-pointer) ───────

    use crate::ported::process::{
        Process_setPid, Process_setThreadGroup, ProcessCmdlineHighlight,
    };

    /// A PROCESS_INFORMATION row pointing at `p`.
    fn info_row(p: &Process) -> BacktracePanelRow {
        BacktracePanelRow {
            type_: BACKTRACE_PANEL_ROW_PROCESS_INFORMATION,
            frame: None,
            error: None,
            process: p as *const Process,
        }
    }

    #[test]
    fn display_information_process_uses_cmdline_when_no_merged() {
        let mut p = Process::default();
        Process_setPid(&mut p, 4321);
        p.mergedCommand.str = None;
        p.cmdline = Some("/usr/bin/sleep 100".to_string());
        let row = info_row(&p);

        let mut out = RichString::new();
        BacktracePanelRow_displayInformation(&row, &mut out);
        // "Process %d: %s"
        assert_eq!(rendered(&out), "Process 4321: /usr/bin/sleep 100");

        // The command name (no BASENAME highlight -> whole name) is repainted
        // in PROCESS_BASENAME|A_BOLD starting right after the "Process 4321: "
        // prefix; the prefix keeps DEFAULT_COLOR|A_BOLD.
        let scheme = ColorScheme::active();
        let prefixLen = "Process 4321: ".len();
        let defaultAttr = (ColorElements::DEFAULT_COLOR.packed(scheme) | A_BOLD) & 0xffffff;
        let baseAttr = (ColorElements::PROCESS_BASENAME.packed(scheme) | A_BOLD) & 0xffffff;
        for i in 0..prefixLen {
            assert_eq!(out.chptr[i].attr, defaultAttr, "prefix attr at {i}");
        }
        for i in prefixLen..out.chlen as usize {
            assert_eq!(out.chptr[i].attr, baseAttr, "name attr at {i}");
        }
    }

    #[test]
    fn display_information_thread_uses_thread_verb_and_color() {
        let mut p = Process::default();
        Process_setPid(&mut p, 77);
        Process_setThreadGroup(&mut p, 5);
        p.isUserlandThread = true; // Process_isThread -> true
        assert!(Process_isThread(&p));
        p.mergedCommand.str = None;
        p.cmdline = Some("worker".to_string());
        let row = info_row(&p);

        let mut out = RichString::new();
        BacktracePanelRow_displayInformation(&row, &mut out);
        assert_eq!(rendered(&out), "Thread 77: worker");

        let scheme = ColorScheme::active();
        let baseAttr = (ColorElements::PROCESS_THREAD_BASENAME.packed(scheme) | A_BOLD) & 0xffffff;
        let prefixLen = "Thread 77: ".len();
        assert_eq!(out.chptr[prefixLen].attr, baseAttr);
    }

    #[test]
    fn display_information_merged_basename_highlight_offsets() {
        let mut p = Process::default();
        Process_setPid(&mut p, 9);
        // mergedCommand.str present with a BASENAME highlight covering "sleep".
        p.mergedCommand.str = Some("/usr/bin/sleep".to_string());
        p.mergedCommand.highlightCount = 1;
        p.mergedCommand.highlights[0] = ProcessCmdlineHighlight {
            offset: "/usr/bin/".len(), // 9
            length: "sleep".len(),     // 5
            attr: 0,
            flags: CMDLINE_HIGHLIGHT_FLAG_BASENAME,
        };
        let row = info_row(&p);

        let mut out = RichString::new();
        BacktracePanelRow_displayInformation(&row, &mut out);
        assert_eq!(rendered(&out), "Process 9: /usr/bin/sleep");

        // Only the "sleep" span (prefix + highlight offset, length 5) is
        // repainted PROCESS_BASENAME; the "/usr/bin/" part keeps DEFAULT.
        let scheme = ColorScheme::active();
        let prefixLen = "Process 9: ".len();
        let defaultAttr = (ColorElements::DEFAULT_COLOR.packed(scheme) | A_BOLD) & 0xffffff;
        let baseAttr = (ColorElements::PROCESS_BASENAME.packed(scheme) | A_BOLD) & 0xffffff;
        let hlStart = prefixLen + "/usr/bin/".len();
        for i in prefixLen..hlStart {
            assert_eq!(out.chptr[i].attr, defaultAttr, "pre-basename attr at {i}");
        }
        for i in hlStart..hlStart + "sleep".len() {
            assert_eq!(out.chptr[i].attr, baseAttr, "basename attr at {i}");
        }
    }

    #[test]
    fn display_dispatches_information_arm() {
        let mut p = Process::default();
        Process_setPid(&mut p, 3);
        p.mergedCommand.str = None;
        p.cmdline = Some("cmd".to_string());
        let row = info_row(&p);

        let mut out = RichString::new();
        BacktracePanelRow_display(&row, &mut out);
        assert_eq!(rendered(&out), "Process 3: cmd");
    }

    // ── highlightBasename (reads the sound `process` back-pointer) ────────

    #[test]
    fn highlight_basename_marks_matching_executable_basename() {
        // process->procExe = "/usr/bin/sleep", basename offset at "sleep".
        let mut p = Process::default();
        Process_setPid(&mut p, 1);
        p.procExe = Some("/usr/bin/sleep".to_string());
        p.procExeBasenameOffset = "/usr/bin/".len(); // procExe suffix = "sleep"
        let row = frame(0, 0x0, Some("/usr/bin/sleep"), None);
        // Give that DATA_FRAME row the process back-pointer.
        let row = BacktracePanelRow {
            process: &p as *const Process,
            ..row
        };

        // A rendered frame line whose object column (starting at index 5) is
        // "/usr/bin/sleep" followed by a space then the function name.
        let line = "  0  /usr/bin/sleep func+0x0";
        let objectPathStart = 5usize;
        // Seed `out` with the same visible text so setAttrn has cells to paint.
        let mut out = RichString::new();
        RichString_appendAscii(&mut out, 0, line.as_bytes());

        BacktracePanelRow_highlightBasename(&row, &mut out, line, objectPathStart);

        // The basename "sleep" spans [lastSlash, lastSlash+5). lastSlash is the
        // index just past the final '/' before the space at end of the column.
        let lastSlash = line.find("sleep").unwrap();
        let baseAttr = ColorElements::PROCESS_BASENAME.packed(ColorScheme::active()) & 0xffffff;
        for i in lastSlash..lastSlash + "sleep".len() {
            assert_eq!(out.chptr[i].attr, baseAttr, "basename attr at {i}");
        }
        // A byte just before the basename is untouched (attr 0 from the seed).
        assert_eq!(out.chptr[lastSlash - 1].attr, 0);
    }

    #[test]
    fn highlight_basename_no_proc_exe_is_noop() {
        let mut p = Process::default();
        p.procExe = None;
        let row = BacktracePanelRow {
            process: &p as *const Process,
            ..frame(0, 0x0, Some("/lib/x"), None)
        };
        let line = "  0  /lib/x f+0x0";
        let mut out = RichString::new();
        RichString_appendAscii(&mut out, 0, line.as_bytes());
        BacktracePanelRow_highlightBasename(&row, &mut out, line, 5);
        // Nothing repainted (all attrs stay 0).
        for i in 0..out.chlen as usize {
            assert_eq!(out.chptr[i].attr, 0, "attr at {i}");
        }
    }
}
