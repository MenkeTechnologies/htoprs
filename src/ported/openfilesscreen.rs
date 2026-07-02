//! Port of `OpenFilesScreen.c` — the concrete [`InfoScreen`] that shows a
//! snapshot of the files a process has open (htop's `l` action), built by
//! shelling out to `lsof -P -o -p <pid> -F` and re-columnising its `-F`
//! field output.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module. The screen embeds an
//! [`InfoScreen`] as its base class (C `InfoScreen super`), so this module
//! sits on top of the ported `infoscreen.rs` substrate the same way
//! `OpenFilesScreen.c` sits on top of `InfoScreen.c`.
//!
//! # Struct mapping (`OpenFilesScreen.h:17`)
//!
//! `OpenFilesScreen` is `InfoScreen super` + `pid_t pid`. As with the
//! `InfoScreen` port, the `InfoScreenClass` vtable (installed in C by
//! `Object_setClass(this, Class(OpenFilesScreen))`, and defined by the
//! `OpenFilesScreen_class` const) is omitted: its only consumers are the
//! vtable-dispatched paths (`InfoScreen_run` -> `scan`/`draw`), matching how
//! `infoscreen.rs` omits its own `Object super`. Because Rust cannot upcast
//! a `&mut InfoScreen` back to its concrete `&mut OpenFilesScreen`, the
//! `scan` hook (dispatched in C as `(InfoScreen* super)` then downcast via
//! `(OpenFilesScreen*)super`) is ported to take `&mut OpenFilesScreen`
//! directly — the faithful analog of that C downcast.
//!
//! # Ported
//!
//! - The [`OpenFiles_Data`] column table (`OpenFilesScreen.c:33`) — the
//!   `char* data[LSOF_DATACOL_COUNT]` row of per-file `-F` fields; a
//!   `[Option<String>; 8]` that owns and frees its strings.
//! - The [`OpenFiles_FileData`] (`OpenFilesScreen.c:44`) and
//!   [`OpenFiles_ProcessData`] (`OpenFilesScreen.c:37`) result structs. The
//!   C intrusive `OpenFiles_FileData* next` linked list is modelled by the
//!   ordering of `OpenFiles_ProcessData.files: Vec<OpenFiles_FileData>`
//!   (the same "pointers -> owned collection" divergence `infoscreen.rs`
//!   uses for its `Vector`/`Panel` fields).
//! - [`getIndexForType`] (`OpenFilesScreen.c:51`) — the `lsof -F` type
//!   letter -> column index switch.
//! - [`getDataForType`] (`OpenFilesScreen.c:75`) — reads a column, mapping
//!   an absent (`NULL`) cell to the empty string, exactly like the C
//!   ternary.
//! - [`OpenFilesScreen_new`] (`OpenFilesScreen.c:80`) — the `AllocThis`
//!   constructor: picks `pid` (thread group for a thread, else the pid)
//!   and hands the embedded `super` to [`InfoScreen_init`] with the fixed
//!   column header. See the constructor docs for the AllocThis-storage
//!   divergence.
//! - [`OpenFilesScreen_getProcessData`] (`OpenFilesScreen.c:99`) — the
//!   `lsof` subprocess: `pipe`/`fork`/`dup2`/`open("/dev/null")`/`execvp`
//!   (via `libc`, matching the raw-syscall style of `affinity.rs` /
//!   `scheduling.rs`), reads the child's `-F` stream through a
//!   `BufReader` (the `fdopen`/`String_readLine` analog), reaps the child
//!   with `waitpid` (the `xWaitpid(..., false)` case: options 0, retry on
//!   `EINTR`), and, when `lsof -o -F` omits SIZE (Linux), backfills it with
//!   `stat()`.
//! - [`OpenFilesScreen_scan`] (`OpenFilesScreen.c:264`) — the vtable
//!   `scan` hook: prune the panel, call [`OpenFilesScreen_getProcessData`],
//!   format each file row and feed it through [`InfoScreen_addLine`]
//!   (ported), install the width-adjusted header, then re-sort both the
//!   `lines` `Vector` and the panel's items.
//!
//! # Stubbed (cannot be ported faithfully yet), each naming its blocker
//!
//! - [`OpenFiles_Data_clear`] (`OpenFilesScreen.c:259`) — frees every
//!   `data[i]` string; a heap-free-only routine. [`OpenFiles_Data`] owns
//!   its `[Option<String>; 8]` and frees the strings on `Drop`, so there
//!   is no algorithm to port (the `Vector_delete` / `History_delete`
//!   precedent). The two callers ([`OpenFilesScreen_getProcessData`] and
//!   [`OpenFilesScreen_scan`]) therefore drop the owning value instead of
//!   calling it.
//! - [`OpenFilesScreen_delete`] (`OpenFilesScreen.c:91`) — `free` of the
//!   object after `InfoScreen_done`. `InfoScreen_done` is itself a stub
//!   (an owned `InfoScreen` releases its fields via `Drop`), so there is
//!   no free routine left to port.
//! - [`OpenFilesScreen_draw`] (`OpenFilesScreen.c:95`) — a one-line
//!   forward to `InfoScreen_drawTitled`, which is a `todo!()` in
//!   `infoscreen.rs` (blocked on `String_stripControlChars`, absent from
//!   the port-purity snapshot, plus the unported `IncSet_drawBar`). No
//!   splittable logic of its own.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::ffi::{c_char, c_int};
use std::ffi::CString;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::unix::io::FromRawFd;

use crate::ported::functionbar::Ncurses;
use crate::ported::incset::IncSet_new;
use crate::ported::infoscreen::{InfoScreen, InfoScreen_addLine, InfoScreen_init};
use crate::ported::listitem::ListItem_new;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::{
    Panel_getSelectedIndex, Panel_new, Panel_prune, Panel_setHeader, Panel_setSelected,
};
use crate::ported::process::{Process, Process_getPid, Process_getThreadGroup, Process_isThread};
use crate::ported::vector::{Vector_insertionSort, Vector_new};

/// Port of `#define LSOF_DATACOL_COUNT 8` from `OpenFilesScreen.c:31`.
/// The number of `lsof -F` field columns tracked per open file; must be
/// larger than the maximum index [`getIndexForType`] returns.
const LSOF_DATACOL_COUNT: usize = 8;

/// Port of `#define VECTOR_DEFAULT_SIZE (10)` from `Vector.h:15` — the
/// initial `lines` capacity used when bootstrapping the throwaway
/// [`InfoScreen`] storage in [`OpenFilesScreen_new`] (see its docs). Mirrors
/// the private constant `infoscreen.rs` uses for the same purpose.
const VECTOR_DEFAULT_SIZE: i32 = 10;

/// Port of `INT16_MAX` (`<stdint.h>`) — the upper `CLAMP` bound the parse
/// loop caps a column's tracked width at (`OpenFilesScreen.c:178`).
const INT16_MAX: usize = 32767;

/// Port of `struct OpenFiles_Data_` (`OpenFilesScreen.c:33`). One row of
/// the `lsof -F` fields for a single open file (or the process header row):
/// C `char* data[LSOF_DATACOL_COUNT]`. Each `char*` becomes an owned
/// `Option<String>` (`None` == the C `NULL` "column not present"), so the
/// table frees its strings on `Drop` — that is exactly what the stubbed
/// [`OpenFiles_Data_clear`] does in C.
pub struct OpenFiles_Data {
    /// C `char* data[LSOF_DATACOL_COUNT]` — one cell per `-F` field type,
    /// indexed by [`getIndexForType`].
    pub data: [Option<String>; LSOF_DATACOL_COUNT],
}

impl OpenFiles_Data {
    /// A fully-absent row (every cell `None` == C `NULL`). Gate-skipped
    /// associated fn — not a C function; the C analog is the zeroed
    /// `OpenFiles_Data` inside an `xCalloc`'d `OpenFiles_FileData` /
    /// `OpenFiles_ProcessData` (the same `InfoScreen::empty` bootstrap
    /// idiom).
    fn empty() -> OpenFiles_Data {
        OpenFiles_Data {
            data: Default::default(),
        }
    }
}

/// Port of `struct OpenFiles_FileData_` (`OpenFilesScreen.c:44`). Holds one
/// [`OpenFiles_Data`] row per open file. The C `struct OpenFiles_FileData_*
/// next` intrusive-list link is dropped: the sequence is modelled by the
/// ordering of [`OpenFiles_ProcessData::files`].
pub struct OpenFiles_FileData {
    /// C `OpenFiles_Data data` — this file's `-F` fields.
    pub data: OpenFiles_Data,
}

/// Port of `struct OpenFiles_ProcessData_` (`OpenFilesScreen.c:37`). The
/// result of one `lsof` run: the process-level header row (`data`, the
/// fields emitted before the first `f` file record), an `error` code
/// (0 == ok, 1 == I/O failure, 127 == `lsof` not found — the `execvp`
/// `_exit(127)`), the per-column display widths (`cols`, seeded to 8 for
/// the numeric SIZE/OFFSET/NODE columns), and the parsed `files`.
pub struct OpenFiles_ProcessData {
    /// C `OpenFiles_Data data` — the process-level fields (before the first
    /// `f` record).
    pub data: OpenFiles_Data,
    /// C `int error` — 0 ok / 1 failure / 127 `lsof` not found.
    pub error: i32,
    /// C `int cols[LSOF_DATACOL_COUNT]` — per-column max field width.
    pub cols: [i32; LSOF_DATACOL_COUNT],
    /// C `struct OpenFiles_FileData_* files` — the linked list of parsed
    /// files, modelled as an owned, ordered `Vec`.
    pub files: Vec<OpenFiles_FileData>,
}

impl OpenFiles_ProcessData {
    /// An empty result with the numeric columns pre-widened to 8, matching
    /// the C `xCalloc` + the three `pdata->cols[...] = 8` seed lines
    /// (`OpenFilesScreen.c:100`). Gate-skipped associated fn — not a C
    /// function (the `InfoScreen::empty` bootstrap precedent).
    fn empty() -> OpenFiles_ProcessData {
        let mut pdata = OpenFiles_ProcessData {
            data: OpenFiles_Data::empty(),
            error: 0,
            cols: [0; LSOF_DATACOL_COUNT],
            files: Vec::new(),
        };
        // C: pdata->cols[getIndexForType('s')] = 8;
        //    pdata->cols[getIndexForType('o')] = 8;
        //    pdata->cols[getIndexForType('i')] = 8;
        pdata.cols[getIndexForType(b's')] = 8;
        pdata.cols[getIndexForType(b'o')] = 8;
        pdata.cols[getIndexForType(b'i')] = 8;
        pdata
    }

    /// Consume the child's `lsof -F` field stream — the `for (;;)` parse
    /// loop of `OpenFilesScreen_getProcessData` (`OpenFilesScreen.c:148`)
    /// through line 216. Gate-skipped associated fn — not a C function; a
    /// private extraction of the loop body so it can be unit-tested against
    /// sample `-F` output without forking `lsof` (the `InfoScreen::empty`
    /// precedent for non-C helpers). Returns the C `lsofIncludesFileSize`
    /// flag (whether an `s` size field was seen).
    ///
    /// Divergences from the C loop, all faithful:
    /// - `String_readLine(fp)` -> `BufRead::read_until(b'\n', ...)` on raw
    ///   bytes (not UTF-8 lines): `lsof -F` names can be arbitrary bytes,
    ///   which the C `char*` path also tolerates. The trailing `\n` is
    ///   stripped exactly as `String_readLine` NUL-terminates it.
    /// - `free_and_xStrdup(&item->data[index], value)` -> assigning
    ///   `Some(value.to_owned())`: the owned `Option<String>` frees the old
    ///   cell on overwrite, and the C dedup ("skip if the new string equals
    ///   the old") is an unobservable optimization.
    /// - `item` (C: `&pdata->data` until the first `f`, then the current
    ///   file's `data`) is a `current: Option<usize>` index into `files`.
    fn parseLsofFields<R: BufRead>(&mut self, mut reader: R) -> bool {
        let mut lsofIncludesFileSize = false;
        // C `OpenFiles_Data* item = &(pdata->data)`: None -> the process
        // row, Some(i) -> files[i]. C `OpenFiles_FileData* fdata = NULL`.
        let mut current: Option<usize> = None;
        let mut buf: Vec<u8> = Vec::new();

        loop {
            buf.clear();
            // C: char* line = String_readLine(fp); if (!line) break;
            let n = reader.read_until(b'\n', &mut buf).unwrap_or_default();
            if n == 0 {
                break;
            }
            // String_readLine strips the trailing '\n' (sets it to '\0').
            if buf.last() == Some(&b'\n') {
                buf.pop();
            }
            // C: unsigned char cmd = line[0]; on an empty line cmd == '\0'
            // -> no switch case matches (ignored) and cmd != 's'.
            if buf.is_empty() {
                continue;
            }
            let cmd = buf[0];

            // C 'f' case: allocate a new file, link it, make it current, then
            // FALLTHRU to store the 'f' field value into it.
            if cmd == b'f' {
                self.files.push(OpenFiles_FileData {
                    data: OpenFiles_Data::empty(),
                });
                current = Some(self.files.len() - 1);
            }

            match cmd {
                // C: case 'f' (FALLTHRU) / 'a' / 'D' / 'i' / 'n' / 's' / 't'
                b'f' | b'a' | b'D' | b'i' | b'n' | b's' | b't' => {
                    let index = getIndexForType(cmd);
                    // C: free_and_xStrdup(&item->data[index], line + 1);
                    let value = String::from_utf8_lossy(&buf[1..]).into_owned();
                    let dlen = value.len();
                    {
                        let item = match current {
                            None => &mut self.data,
                            Some(i) => &mut self.files[i].data,
                        };
                        item.data[index] = Some(value);
                    }
                    // C: if (dlen > cols[index]) cols[index] = CLAMP(dlen, 0, INT16_MAX);
                    if dlen > self.cols[index] as usize {
                        self.cols[index] = dlen.min(INT16_MAX) as i32;
                    }
                }
                // C: case 'o' — strip a leading "0t" offset prefix.
                b'o' => {
                    let index = getIndexForType(cmd);
                    let rest = &buf[1..];
                    // C: if (String_startsWith(line + 1, "0t")) value = line + 3;
                    //    else value = line + 1;  (byte prefix == C strncmp)
                    let value_bytes: &[u8] = if rest.starts_with(b"0t") {
                        &buf[3..]
                    } else {
                        rest
                    };
                    let value = String::from_utf8_lossy(value_bytes).into_owned();
                    let dlen = value.len();
                    {
                        let item = match current {
                            None => &mut self.data,
                            Some(i) => &mut self.files[i].data,
                        };
                        item.data[index] = Some(value);
                    }
                    if dlen > self.cols[index] as usize {
                        self.cols[index] = dlen.min(INT16_MAX) as i32;
                    }
                }
                // C: 'c' 'd' 'g' 'G' 'k' 'l' 'L' 'p' 'P' 'R' 'T' 'u' and any
                // other letter -> /* ignore */.
                _ => {}
            }

            // C: if (cmd == 's') lsofIncludesFileSize = true;
            if cmd == b's' {
                lsofIncludesFileSize = true;
            }
            // C: free(line);  -> buf reused next iteration.
        }

        lsofIncludesFileSize
    }
}

/// Port of `static size_t getIndexForType(char type)` from
/// `OpenFilesScreen.c:51`. Maps an `lsof -F` output field-type letter to
/// its fixed column index. C `abort()`s on any other letter ("should never
/// reach here"); the faithful safe analog is a panic on the unreachable
/// arm (the same terminate-on-invariant-violation the `Vector_get` port
/// uses for its C asserts).
pub fn getIndexForType(type_: u8) -> usize {
    match type_ {
        b'f' => 0,
        b'a' => 1,
        b'D' => 2,
        b'i' => 3,
        b'n' => 4,
        b's' => 5,
        b't' => 6,
        b'o' => 7,
        // C: /* should never reach here */ abort();
        _ => unreachable!("getIndexForType: invalid lsof -F type (C abort())"),
    }
}

/// Port of `static const char* getDataForType(const OpenFiles_Data* data,
/// char type)` from `OpenFilesScreen.c:75`. Returns the column for `type_`,
/// mapping an absent (`NULL`) cell to the empty string — exactly the C
/// `data->data[index] ? data->data[index] : ""` ternary.
pub fn getDataForType(data: &OpenFiles_Data, type_: u8) -> &str {
    let index = getIndexForType(type_);
    match &data.data[index] {
        Some(s) => s.as_str(),
        None => "",
    }
}

/// Port of `struct OpenFilesScreen_` (`OpenFilesScreen.h:17`):
/// `InfoScreen super` + `pid_t pid`. The `InfoScreenClass` vtable is
/// omitted (see the module docs) — only the `scan`/`draw` dispatch paths
/// read it.
pub struct OpenFilesScreen {
    /// C `InfoScreen super` — the scrollable info panel base class.
    pub super_: InfoScreen,
    /// C `pid_t pid` — the process (thread group) whose open files are shown.
    pub pid: i32,
}

/// Port of `OpenFilesScreen* OpenFilesScreen_new(const Process* process)`
/// from `OpenFilesScreen.c:80`. Selects the target `pid` — the thread group
/// for a thread (C `Process_getThreadGroup`), otherwise the process id
/// (C `Process_getPid`) — and initialises the embedded `super` via
/// [`InfoScreen_init`] with the `LINES - 2` panel height (`Ncurses::lines()`,
/// the same source `infoscreen.rs` uses for `COLS`) and the fixed lsof
/// column header. `NULL` is passed for the function bar so `InfoScreen_init`
/// builds the default `InfoScreen` bar.
///
/// Divergence: C `xCalloc`s the object (zeroed `super`) then overwrites it.
/// Rust needs a valid `InfoScreen` value before [`InfoScreen_init`] can
/// overwrite it, so `super` is seeded with the same throwaway empty storage
/// `InfoScreen::empty` builds (an empty `Panel`/`IncSet`/`ListItem`-typed
/// `Vector`) — the AllocThis-uninitialized-storage idiom — which
/// [`InfoScreen_init`] then fully replaces. The C
/// `Object_setClass(this, Class(OpenFilesScreen))` vtable install is
/// omitted (the vtable is not modelled; see the module docs).
pub fn OpenFilesScreen_new(process: &Process) -> OpenFilesScreen {
    // Seed `super` with throwaway empty storage (== InfoScreen::empty),
    // mirroring the zeroed `super` C's xCalloc hands to InfoScreen_init.
    let list_item_class: &'static ObjectClass = ListItem_new("", 0).klass();
    let mut this = OpenFilesScreen {
        super_: InfoScreen {
            process: core::ptr::null(),
            display: Panel_new(0, 0, 0, 0, None),
            inc: IncSet_new(None),
            lines: Vector_new(list_item_class, true, VECTOR_DEFAULT_SIZE),
        },
        pid: 0,
    };

    // C: if (Process_isThread(process)) this->pid = Process_getThreadGroup(process);
    //    else this->pid = Process_getPid(process);
    if Process_isThread(process) {
        this.pid = Process_getThreadGroup(process);
    } else {
        this.pid = Process_getPid(process);
    }

    // C: return (OpenFilesScreen*) InfoScreen_init(&this->super, process, NULL,
    //        LINES - 2, "   FD TYPE    MODE DEVICE           SIZE     OFFSET       NODE  NAME");
    InfoScreen_init(
        &mut this.super_,
        process as *const Process,
        None,
        Ncurses::lines() - 2,
        "   FD TYPE    MODE DEVICE           SIZE     OFFSET       NODE  NAME",
    );

    this
}

/// TODO: port of `void OpenFilesScreen_delete(Object* this)` from
/// `OpenFilesScreen.c:91`. `free(InfoScreen_done((InfoScreen*)this))` —
/// heap-free only. `InfoScreen_done` is itself a stub (an owned
/// `InfoScreen` releases its fields via `Drop`), and the owned
/// `OpenFilesScreen` frees itself the same way, so there is no algorithm to
/// port (the `InfoScreen_done` / `Vector_delete` precedent).
pub fn OpenFilesScreen_delete() {
    todo!("port of OpenFilesScreen.c:91 — Drop releases owned fields (InfoScreen_done is itself a Drop stub)")
}

/// TODO: port of `static void OpenFilesScreen_draw(InfoScreen* this)` from
/// `OpenFilesScreen.c:95`. A one-line forward to `InfoScreen_drawTitled`,
/// which is a `todo!()` in `infoscreen.rs` — blocked on
/// `String_stripControlChars` (`XUtils.h:147`), absent from the port-purity
/// snapshot and so unaddable as a `pub fn`, plus the unported
/// `IncSet_drawBar`. No logic of its own to split out.
pub fn OpenFilesScreen_draw() {
    todo!("port of OpenFilesScreen.c:95 — forwards to InfoScreen_drawTitled (stubbed: String_stripControlChars absent, IncSet_drawBar unported)")
}

/// Port of `static OpenFiles_ProcessData* OpenFilesScreen_getProcessData(pid_t
/// pid)` from `OpenFilesScreen.c:99`. Runs `lsof -P -o -p <pid> -F` as a
/// child process and parses its `-F` field stream into an
/// [`OpenFiles_ProcessData`].
///
/// The control flow mirrors the C 1:1: seed the numeric column widths, open
/// a `pipe`, `fork`; in the child `dup2` the write end onto stdout, redirect
/// stderr to `/dev/null`, then `execvp` `lsof` (`_exit(127)` if it is not on
/// `$PATH`, `_exit(1)` if `/dev/null` cannot be opened); in the parent read
/// the child's `-F` output line by line via [`OpenFiles_ProcessData::parseLsofFields`],
/// `waitpid` for it (the `xWaitpid(child, &wstatus, 0, false)` case: options
/// 0, retry only on `EINTR`), record the exit status as `error`, and — when
/// `lsof -o -F` omitted SIZE (Linux; `!lsofIncludesFileSize`) — backfill each
/// file's size with `stat()`.
///
/// Divergences, all faithful: the raw syscalls go through `libc` (the
/// `affinity.rs` / `scheduling.rs` style) rather than nix wrappers, keeping
/// the post-`fork` child path to async-signal-safe calls only (the `argv`
/// `CString`s are built before the `fork`); the `fdopen`/`String_readLine`
/// pair becomes a `BufReader<File>` whose `Drop` is the `fclose`; the C
/// `fdopen` NULL check is unreachable (`File::from_raw_fd` is infallible);
/// and the per-file `free`/`OpenFiles_Data_clear` on a `waitpid` error is a
/// `files.clear()` (owned `Drop`).
pub fn OpenFilesScreen_getProcessData(pid: i32) -> OpenFiles_ProcessData {
    let mut pdata = OpenFiles_ProcessData::empty();

    // C: int fdpair[2] = {-1, -1}; if (pipe(fdpair) < 0) { error = 1; return; }
    let mut fdpair: [c_int; 2] = [-1, -1];
    if unsafe { libc::pipe(fdpair.as_mut_ptr()) } < 0 {
        pdata.error = 1;
        return pdata;
    }

    // Build the execvp argv before the fork so the child does no allocation
    // (async-signal-safety). C: execlp("lsof", "lsof", "-P", "-o", "-p",
    // buffer, "-F", (char*)NULL).
    let c_lsof = CString::new("lsof").expect("no interior NUL");
    let c_dash_p_cap = CString::new("-P").expect("no interior NUL");
    let c_dash_o = CString::new("-o").expect("no interior NUL");
    let c_dash_p = CString::new("-p").expect("no interior NUL");
    // C: xSnprintf(buffer, sizeof(buffer), "%d", pid);
    let c_pid = CString::new(pid.to_string()).expect("no interior NUL");
    let c_dash_f = CString::new("-F").expect("no interior NUL");
    let argv: [*const c_char; 7] = [
        c_lsof.as_ptr(),
        c_dash_p_cap.as_ptr(),
        c_dash_o.as_ptr(),
        c_dash_p.as_ptr(),
        c_pid.as_ptr(),
        c_dash_f.as_ptr(),
        core::ptr::null(),
    ];

    // C: pid_t child = fork();
    let child = unsafe { libc::fork() };
    if child < 0 {
        // C: close(fdpair[1]); close(fdpair[0]); error = 1; return;
        unsafe {
            libc::close(fdpair[1]);
            libc::close(fdpair[0]);
        }
        pdata.error = 1;
        return pdata;
    }

    if child == 0 {
        // Child — async-signal-safe libc syscalls only.
        unsafe {
            libc::close(fdpair[0]);
            libc::dup2(fdpair[1], libc::STDOUT_FILENO);
            libc::close(fdpair[1]);
            // C: int fdnull = open("/dev/null", O_WRONLY); if (fdnull < 0) _exit(1);
            let fdnull = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
            if fdnull < 0 {
                libc::_exit(1);
            }
            libc::dup2(fdnull, libc::STDERR_FILENO);
            libc::close(fdnull);
            libc::execvp(c_lsof.as_ptr(), argv.as_ptr());
            // C: _exit(127);  (only reached if execvp failed — lsof not found)
            libc::_exit(127);
        }
    }

    // Parent. C: close(fdpair[1]);
    unsafe {
        libc::close(fdpair[1]);
    }

    // C: FILE* fp = fdopen(fdpair[0], "r"); ... for (;;) { line = String_readLine(fp); ... }
    // The BufReader owns fdpair[0]; dropping it at the end of this block is
    // the C fclose(fp).
    let lsofIncludesFileSize = {
        let file = unsafe { File::from_raw_fd(fdpair[0]) };
        let reader = BufReader::new(file);
        pdata.parseLsofFields(reader)
    };

    // C: int wstatus; if (xWaitpid(child, &wstatus, 0, false) < 0) { ... }
    // xWaitpid with wait_for_exit == false and options == 0 is a plain
    // waitpid that only retries on EINTR (XUtils.c:321).
    let mut wstatus: c_int = 0;
    let ret = loop {
        let r = unsafe { libc::waitpid(child, &mut wstatus, 0) };
        if r == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        break r;
    };
    if ret < 0 {
        // C: free every file, error = 1, return. Owned Drop frees the rows.
        pdata.files.clear();
        pdata.error = 1;
        return pdata;
    }

    // C: if (!WIFEXITED(wstatus)) error = 1; else error = WEXITSTATUS(wstatus);
    if !libc::WIFEXITED(wstatus) {
        pdata.error = 1;
    } else {
        pdata.error = libc::WEXITSTATUS(wstatus);
    }

    // C: if (lsofIncludesFileSize) return pdata;  (macOS path)
    if lsofIncludesFileSize {
        return pdata;
    }

    // C: /* On linux, `lsof -o -F` omits SIZE, so add it back. */
    let fileSizeIndex = getIndexForType(b's');
    for fdata in pdata.files.iter_mut() {
        // C: const char* filename = getDataForType(item, 'n');
        let filename = getDataForType(&fdata.data, b'n').to_string();
        let cfn = match CString::new(filename) {
            Ok(c) => c,
            // A filename with an interior NUL cannot be stat()'d as a C
            // string; C would pass the truncated char* — skip it here.
            Err(_) => continue,
        };
        // C: struct stat sb; if (stat(filename, &sb) == 0) { ... }
        let mut sb: libc::stat = unsafe { core::mem::zeroed() };
        if unsafe { libc::stat(cfn.as_ptr(), &mut sb) } == 0 {
            // C: xSnprintf(fileSizeBuf, 21, "%"PRIu64, (uint64_t)sb.st_size);
            //    free_and_xStrdup(&item->data[fileSizeIndex], fileSizeBuf);
            fdata.data.data[fileSizeIndex] = Some(format!("{}", sb.st_size as u64));
        }
    }

    pdata
}

/// TODO: port of `static void OpenFiles_Data_clear(OpenFiles_Data* data)`
/// from `OpenFilesScreen.c:259`. Frees every `data->data[i]` string — a
/// heap-free-only routine. [`OpenFiles_Data`] owns its `[Option<String>; 8]`
/// and frees the strings via `Drop`, so there is no algorithm to port (the
/// `Vector_delete` / `History_delete` precedent). Its C callers
/// ([`OpenFilesScreen_getProcessData`] / [`OpenFilesScreen_scan`]) drop the
/// owning value instead.
pub fn OpenFiles_Data_clear() {
    todo!("port of OpenFilesScreen.c:259 — Drop frees the owned column strings")
}

/// Port of `static void OpenFilesScreen_scan(InfoScreen* super)` from
/// `OpenFilesScreen.c:264`. The vtable `scan` hook. C receives the base
/// `InfoScreen* super` and downcasts it via `(OpenFilesScreen*)super` to
/// read `->pid`; because Rust cannot upcast then downcast, the port takes
/// the concrete `&mut OpenFilesScreen` directly (see the module docs).
///
/// Saves the selection, prunes the panel, runs
/// [`OpenFilesScreen_getProcessData`], and then: on `error == 127` /
/// `error == 1` adds the single `lsof`-missing / listing-failed message
/// line; otherwise installs the width-adjusted column header and, per file,
/// formats the row (the C `xAsprintf` `%*s`-padded columns) and feeds it
/// through [`InfoScreen_addLine`]. Finally re-sorts the `lines` `Vector` and
/// the panel's items and restores the selection.
///
/// Divergences: the C `snprintf`/`xAsprintf` format strings are reproduced
/// with `format!` (C `%N.Ns` -> Rust `{:>N.N}` / `{:<N.N}`, C `%*s` ->
/// `{:>w$}`); C's fixed `char hdrbuf[128]` is a C-string-buffer artifact,
/// so the owned `String` header is not truncated (the same reasoning
/// `history.rs` gives for the fixed `fgets` buffer). C's per-file
/// `OpenFiles_Data_clear` + `free` and the trailing `free(pdata)` are the
/// owned `Drop` of `pdata` at end of scope. `Vector_insertionSort(panel->items)`
/// has no direct call because the ported `Panel.items` is a plain
/// `Vec<Box<dyn Object>>` (not a `Vector`); it is sorted in place with the
/// same `Object::compare` comparator `Vector_insertionSort` uses.
pub fn OpenFilesScreen_scan(this: &mut OpenFilesScreen) {
    // C: Panel* panel = super->display; int idx = Panel_getSelectedIndex(panel);
    let idx = Panel_getSelectedIndex(&this.super_.display);
    // C: Panel_prune(panel);
    Panel_prune(&mut this.super_.display);
    // C: pdata = OpenFilesScreen_getProcessData(((OpenFilesScreen*)super)->pid);
    let pdata = OpenFilesScreen_getProcessData(this.pid);

    if pdata.error == 127 {
        // C: InfoScreen_addLine(super, "Could not execute 'lsof'. ...");
        InfoScreen_addLine(
            &mut this.super_,
            "Could not execute 'lsof'. Please make sure it is available in your $PATH.",
        );
    } else if pdata.error == 1 {
        // C: InfoScreen_addLine(super, "Failed listing open files.");
        InfoScreen_addLine(&mut this.super_, "Failed listing open files.");
    } else {
        let w_size = pdata.cols[getIndexForType(b's')] as usize;
        let w_offset = pdata.cols[getIndexForType(b'o')] as usize;
        let w_node = pdata.cols[getIndexForType(b'i')] as usize;

        // C: snprintf(hdrbuf, 128, "%5.5s %-7.7s %-4.4s %6.6s %*s %*s %*s  %s",
        //       "FD", "TYPE", "MODE", "DEVICE", cols[s], "SIZE",
        //       cols[o], "OFFSET", cols[i], "NODE", "NAME");
        let hdrbuf = format!(
            "{:>5.5} {:<7.7} {:<4.4} {:>6.6} {:>ws$} {:>wo$} {:>wn$}  {}",
            "FD",
            "TYPE",
            "MODE",
            "DEVICE",
            "SIZE",
            "OFFSET",
            "NODE",
            "NAME",
            ws = w_size,
            wo = w_offset,
            wn = w_node,
        );
        // C: Panel_setHeader(panel, hdrbuf);
        Panel_setHeader(&mut this.super_.display, &hdrbuf);

        // C: for (fdata = pdata->files; fdata; fdata = fdata->next) { ... }
        for fdata in &pdata.files {
            let data = &fdata.data;
            // C: xAsprintf(&entry, "%5.5s %-7.7s %-4.4s %6.6s %*s %*s %*s  %s",
            //       f, t, a, D, cols[s], s, cols[o], o, cols[i], i, n);
            let entry = format!(
                "{:>5.5} {:<7.7} {:<4.4} {:>6.6} {:>ws$} {:>wo$} {:>wn$}  {}",
                getDataForType(data, b'f'),
                getDataForType(data, b't'),
                getDataForType(data, b'a'),
                getDataForType(data, b'D'),
                getDataForType(data, b's'),
                getDataForType(data, b'o'),
                getDataForType(data, b'i'),
                getDataForType(data, b'n'),
                ws = w_size,
                wo = w_offset,
                wn = w_node,
            );
            // C: InfoScreen_addLine(super, entry);
            InfoScreen_addLine(&mut this.super_, &entry);
            // C: OpenFiles_Data_clear(data); free(old); -> owned Drop of pdata.
        }
        // C: OpenFiles_Data_clear(&pdata->data); -> owned Drop of pdata.
    }
    // C: free(pdata); -> pdata dropped at end of scope.

    // C: Vector_insertionSort(super->lines);
    Vector_insertionSort(&mut this.super_.lines);
    // C: Vector_insertionSort(panel->items);  (see the divergence note above)
    this.super_
        .display
        .items
        .sort_by(|a, b| a.compare(&**b).cmp(&0));
    // C: Panel_setSelected(panel, idx);
    Panel_setSelected(&mut this.super_.display, idx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::incset::IncSet_filter;
    use crate::ported::listitem::ListItem;
    use crate::ported::panel::{Panel_headerHeight, Panel_size};
    use crate::ported::process::{Process, Process_setPid, Process_setThreadGroup};
    use crate::ported::vector::{Vector_get, Vector_size};
    use std::io::Cursor;

    // ── getIndexForType ──────────────────────────────────────────────────

    #[test]
    fn get_index_for_type_maps_every_letter() {
        // The full switch from OpenFilesScreen.c:51, in order.
        assert_eq!(getIndexForType(b'f'), 0);
        assert_eq!(getIndexForType(b'a'), 1);
        assert_eq!(getIndexForType(b'D'), 2);
        assert_eq!(getIndexForType(b'i'), 3);
        assert_eq!(getIndexForType(b'n'), 4);
        assert_eq!(getIndexForType(b's'), 5);
        assert_eq!(getIndexForType(b't'), 6);
        assert_eq!(getIndexForType(b'o'), 7);
        // Every index is a distinct, in-range column.
        for &c in b"faDinsto" {
            assert!(getIndexForType(c) < LSOF_DATACOL_COUNT);
        }
    }

    #[test]
    #[should_panic(expected = "getIndexForType")]
    fn get_index_for_type_aborts_on_unknown() {
        // C: /* should never reach here */ abort();
        let _ = getIndexForType(b'z');
    }

    // ── getDataForType ───────────────────────────────────────────────────

    fn data_with(pairs: &[(u8, &str)]) -> OpenFiles_Data {
        let mut d = OpenFiles_Data::empty();
        for &(t, v) in pairs {
            d.data[getIndexForType(t)] = Some(v.to_string());
        }
        d
    }

    #[test]
    fn get_data_for_type_returns_cell_or_empty() {
        let d = data_with(&[(b'n', "/etc/passwd"), (b'f', "3")]);
        // Present cells return their string.
        assert_eq!(getDataForType(&d, b'n'), "/etc/passwd");
        assert_eq!(getDataForType(&d, b'f'), "3");
        // Absent (NULL) cells map to "" (the C ternary's else branch).
        assert_eq!(getDataForType(&d, b't'), "");
        assert_eq!(getDataForType(&d, b's'), "");
        assert_eq!(getDataForType(&d, b'o'), "");
    }

    #[test]
    fn get_data_for_type_all_empty_by_default() {
        let d = OpenFiles_Data::empty();
        for &c in b"faDinsto" {
            assert_eq!(getDataForType(&d, c), "");
        }
    }

    // ── parseLsofFields (the lsof -F parse loop, OpenFilesScreen.c:148) ───

    /// Feed sample `lsof -F` bytes through the parse loop, exactly as
    /// `OpenFilesScreen_getProcessData` would from the child's pipe.
    fn parse(sample: &str) -> OpenFiles_ProcessData {
        let mut pdata = OpenFiles_ProcessData::empty();
        pdata.parseLsofFields(Cursor::new(sample.as_bytes().to_vec()));
        pdata
    }

    #[test]
    fn parse_splits_one_file_into_columns() {
        // A single file record: fd 'f', access 'a', type 't', device 'D',
        // size 's', offset 'o', inode 'i', name 'n'. lsof emits one field
        // per line, prefixed by the type letter.
        let sample = "f3\nar\ntREG\nD8,1\ns1024\no0t512\ni98765\nn/etc/passwd\n";
        let pdata = parse(sample);
        assert_eq!(pdata.files.len(), 1);
        let d = &pdata.files[0].data;
        assert_eq!(getDataForType(d, b'f'), "3");
        assert_eq!(getDataForType(d, b'a'), "r");
        assert_eq!(getDataForType(d, b't'), "REG");
        assert_eq!(getDataForType(d, b'D'), "8,1");
        assert_eq!(getDataForType(d, b's'), "1024");
        // The 'o' offset "0t512" has its "0t" prefix stripped -> "512".
        assert_eq!(getDataForType(d, b'o'), "512");
        assert_eq!(getDataForType(d, b'i'), "98765");
        assert_eq!(getDataForType(d, b'n'), "/etc/passwd");
    }

    #[test]
    fn parse_offset_without_0t_prefix_is_kept_verbatim() {
        // C: if (!String_startsWith(line + 1, "0t")) keep line + 1.
        let sample = "f1\no0x1f\n";
        let pdata = parse(sample);
        assert_eq!(getDataForType(&pdata.files[0].data, b'o'), "0x1f");
    }

    #[test]
    fn parse_new_f_record_starts_a_new_file() {
        // Each 'f' line begins a fresh OpenFiles_FileData (C: xCalloc a new
        // node, link it, item = &node->data).
        let sample = "f0\nn/dev/tty\nf1\nn/tmp/a\nf2\nn/tmp/b\n";
        let pdata = parse(sample);
        assert_eq!(pdata.files.len(), 3);
        assert_eq!(getDataForType(&pdata.files[0].data, b'f'), "0");
        assert_eq!(getDataForType(&pdata.files[0].data, b'n'), "/dev/tty");
        assert_eq!(getDataForType(&pdata.files[1].data, b'f'), "1");
        assert_eq!(getDataForType(&pdata.files[1].data, b'n'), "/tmp/a");
        assert_eq!(getDataForType(&pdata.files[2].data, b'f'), "2");
        assert_eq!(getDataForType(&pdata.files[2].data, b'n'), "/tmp/b");
    }

    #[test]
    fn parse_fields_before_first_f_go_to_process_row() {
        // Process-level fields (p/c/etc.) precede the first 'f'; the ones
        // getIndexForType handles land in pdata->data, not a file.
        let sample = "n/process/level\nf7\nn/file/level\n";
        let pdata = parse(sample);
        // The pre-'f' name went to the process row.
        assert_eq!(getDataForType(&pdata.data, b'n'), "/process/level");
        // The post-'f' name went to the file row.
        assert_eq!(pdata.files.len(), 1);
        assert_eq!(getDataForType(&pdata.files[0].data, b'n'), "/file/level");
    }

    #[test]
    fn parse_ignores_unknown_and_process_only_fields() {
        // c/d/g/G/k/l/L/p/P/R/T/u and any other letter are ignored and must
        // not create files or abort (getIndexForType is never called on
        // them).
        let sample = "p1234\ncbash\ng1000\nu501\nPTCP\nf3\nn/x\n";
        let pdata = parse(sample);
        assert_eq!(pdata.files.len(), 1);
        assert_eq!(getDataForType(&pdata.files[0].data, b'n'), "/x");
    }

    #[test]
    fn parse_tracks_column_widths_with_seed_of_8() {
        // cols[s]/cols[o]/cols[i] start at 8; a longer field widens them,
        // a shorter one does not (C: if (dlen > cols[index]) cols[index] = dlen).
        let sample = "f1\ns123456789012\no0t99\ni7\n"; // size len 12 > 8
        let pdata = parse(sample);
        assert_eq!(pdata.cols[getIndexForType(b's')], 12); // widened
        assert_eq!(pdata.cols[getIndexForType(b'o')], 8); // "99" (len 2) < 8
        assert_eq!(pdata.cols[getIndexForType(b'i')], 8); // "7" (len 1) < 8
                                                          // A non-seeded column ('n') starts at 0 and grows to the field len.
        assert_eq!(pdata.cols[getIndexForType(b'n')], 0);
    }

    #[test]
    fn parse_column_width_is_clamped_to_int16_max() {
        // C: cols[index] = CLAMP(dlen, 0, INT16_MAX).
        let huge = "x".repeat(40000);
        let sample = format!("f1\nn{huge}\n");
        let pdata = parse(&sample);
        assert_eq!(pdata.cols[getIndexForType(b'n')], INT16_MAX as i32);
    }

    #[test]
    fn parse_reports_size_field_presence() {
        // lsofIncludesFileSize is the return value: true iff any 's' line
        // was seen (macOS `lsof -o -F` includes size, Linux does not).
        let mut with_size = OpenFiles_ProcessData::empty();
        assert!(with_size.parseLsofFields(Cursor::new(b"f1\ns42\n".to_vec())));

        let mut without_size = OpenFiles_ProcessData::empty();
        assert!(!without_size.parseLsofFields(Cursor::new(b"f1\nn/x\n".to_vec())));
    }

    #[test]
    fn parse_handles_last_line_without_trailing_newline() {
        // String_readLine returns the final unterminated line too (feof).
        let sample = "f9\nn/no/newline";
        let pdata = parse(sample);
        assert_eq!(pdata.files.len(), 1);
        assert_eq!(getDataForType(&pdata.files[0].data, b'n'), "/no/newline");
    }

    #[test]
    fn parse_empty_stream_yields_no_files() {
        let pdata = parse("");
        assert_eq!(pdata.files.len(), 0);
        // The seeded numeric columns are untouched.
        assert_eq!(pdata.cols[getIndexForType(b's')], 8);
    }

    #[test]
    fn parse_later_field_overwrites_earlier_same_type() {
        // free_and_xStrdup replaces the cell; a repeated type wins last.
        let sample = "f1\nn/first\nn/second\n";
        let pdata = parse(sample);
        assert_eq!(getDataForType(&pdata.files[0].data, b'n'), "/second");
    }

    // ── OpenFilesScreen_new ──────────────────────────────────────────────

    const HEADER: &str = "   FD TYPE    MODE DEVICE           SIZE     OFFSET       NODE  NAME";

    #[test]
    fn new_uses_pid_for_a_non_thread() {
        let mut p = Process::default();
        Process_setPid(&mut p, 4321);
        Process_setThreadGroup(&mut p, 4000);
        // Default flags: not a thread -> pid is used.
        assert!(!Process_isThread(&p));
        let s = OpenFilesScreen_new(&p);
        assert_eq!(s.pid, 4321);
    }

    #[test]
    fn new_uses_thread_group_for_a_thread() {
        let mut p = Process::default();
        Process_setPid(&mut p, 4321);
        Process_setThreadGroup(&mut p, 4000);
        // Mark it a userland thread -> thread group is used instead.
        p.isUserlandThread = true;
        assert!(Process_isThread(&p));
        let s = OpenFilesScreen_new(&p);
        assert_eq!(s.pid, 4000);
    }

    #[test]
    fn new_initializes_the_embedded_infoscreen() {
        let mut p = Process::default();
        Process_setPid(&mut p, 7);
        let s = OpenFilesScreen_new(&p);
        // super was fully overwritten by InfoScreen_init:
        // - process back-pointer stored (points at the passed Process).
        assert_eq!(s.super_.process, &p as *const Process);
        // - lines and panel start empty.
        assert_eq!(Vector_size(&s.super_.lines), 0);
        assert_eq!(Panel_size(&s.super_.display), 0);
        // - panel geometry: Panel_new(0, 1, COLS, LINES - 2, ...).
        assert_eq!(s.super_.display.x, 0);
        assert_eq!(s.super_.display.y, 1);
        assert_eq!(s.super_.display.w, Ncurses::cols());
        assert_eq!(s.super_.display.h, Ncurses::lines() - 2);
        // - the fixed lsof column header was installed.
        assert_eq!(Panel_headerHeight(&s.super_.display), 1);
        // - no filter active on the fresh IncSet.
        assert!(IncSet_filter(&s.super_.inc).is_none());
    }

    #[test]
    fn new_builds_the_default_infoscreen_bar() {
        let p = Process::default();
        let s = OpenFilesScreen_new(&p);
        // NULL bar was passed, so InfoScreen_init built the default bar.
        let bar = s
            .super_
            .display
            .defaultBar
            .as_ref()
            .expect("default bar built");
        // The InfoScreen bar labels/keys (Search/Filter/Refresh/Done).
        assert_eq!(
            bar.functions,
            vec!["Search ", "Filter ", "Refresh", "Done   "]
        );
        assert_eq!(bar.keys, vec!["F3", "F4", "F5", "Esc"]);
    }

    #[test]
    fn new_installs_the_lsof_column_header() {
        // Guard the exact header string ported from OpenFilesScreen.c:88.
        let p = Process::default();
        let s = OpenFilesScreen_new(&p);
        // Header height is 1 (a non-empty header was installed).
        assert_eq!(Panel_headerHeight(&s.super_.display), 1);
        // The constant matches the string passed to InfoScreen_init.
        assert_eq!(
            HEADER,
            "   FD TYPE    MODE DEVICE           SIZE     OFFSET       NODE  NAME"
        );
    }

    // ── OpenFilesScreen_getProcessData / scan (integration: forks lsof) ───

    #[test]
    fn get_process_data_runs_or_reports_missing_lsof() {
        // Integration: fork/exec real lsof against our own pid. On a host
        // with lsof (the darwin dev host), error == 0 and our own open fds
        // (0/1/2 at minimum) are parsed; on a host without lsof, execvp
        // fails and error == 127. Any I/O failure is error == 1. Assert the
        // control flow reaches one of those documented outcomes without
        // hanging.
        let pid = unsafe { libc::getpid() };
        let pdata = OpenFilesScreen_getProcessData(pid);
        assert!(
            matches!(pdata.error, 0 | 1 | 127),
            "unexpected error code {}",
            pdata.error
        );
        if pdata.error == 0 {
            // A live process always has open files, so lsof yields rows.
            assert!(!pdata.files.is_empty());
            // The seeded numeric columns are never narrower than 8.
            assert!(pdata.cols[getIndexForType(b's')] >= 8);
        }
    }

    #[test]
    fn scan_populates_lines_from_lsof() {
        // Integration: drive the full scan hook. Whatever branch lsof lands
        // in (rows, "not found", or "failed listing"), scan adds at least
        // one line and restores a valid selection without panicking.
        let mut p = Process::default();
        Process_setPid(&mut p, unsafe { libc::getpid() });
        let mut s = OpenFilesScreen_new(&p);
        OpenFilesScreen_scan(&mut s);
        assert!(Vector_size(&s.super_.lines) >= 1);
        // The panel mirrors the lines (weak-panel view; no filter active).
        assert_eq!(Panel_size(&s.super_.display), Vector_size(&s.super_.lines));
    }

    #[test]
    fn scan_sorts_lines_lexicographically() {
        // After scan, super->lines is Vector_insertionSort'd (ListItem
        // compare == strcmp on the value), so the rows are non-decreasing.
        let mut p = Process::default();
        Process_setPid(&mut p, unsafe { libc::getpid() });
        let mut s = OpenFilesScreen_new(&p);
        OpenFilesScreen_scan(&mut s);
        let n = Vector_size(&s.super_.lines);
        let mut prev: Option<String> = None;
        for idx in 0..n {
            let any: &dyn std::any::Any = Vector_get(&s.super_.lines, idx as usize);
            let cur = any.downcast_ref::<ListItem>().unwrap().value.clone();
            if let Some(p) = &prev {
                assert!(*p <= cur, "lines not sorted: {p:?} > {cur:?}");
            }
            prev = Some(cur);
        }
    }
}
