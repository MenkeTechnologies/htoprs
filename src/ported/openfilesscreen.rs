//! Partial port of `OpenFilesScreen.c` — the concrete [`InfoScreen`] that
//! shows a snapshot of the files a process has open (htop's `l` action),
//! built by shelling out to `lsof -P -o -p <pid> -F` and re-columnising
//! its `-F` field output.
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
//! stubbed vtable-dispatched paths (`InfoScreen_run` -> `scan`/`draw`),
//! matching how `infoscreen.rs` omits its own `Object super`.
//!
//! # Ported
//!
//! - The [`OpenFiles_Data`] column table (`OpenFilesScreen.c:33`) — the
//!   `char* data[LSOF_DATACOL_COUNT]` row of per-file `-F` fields; a
//!   `[Option<String>; 8]` that owns and frees its strings.
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
//!
//! # Stubbed (cannot be ported faithfully yet), each naming its blocker
//!
//! - [`OpenFiles_Data_clear`] (`OpenFilesScreen.c:259`) — frees every
//!   `data[i]` string; a heap-free-only routine. [`OpenFiles_Data`] owns
//!   its `[Option<String>; 8]` and frees the strings on `Drop`, so there
//!   is no algorithm to port (the `Vector_delete` / `History_delete`
//!   precedent). It is also only ever reached from the two stubbed
//!   `lsof`-consuming functions below.
//! - [`OpenFilesScreen_delete`] (`OpenFilesScreen.c:91`) — `free` of the
//!   object after `InfoScreen_done`. `InfoScreen_done` is itself a stub
//!   (an owned `InfoScreen` releases its fields via `Drop`), so there is
//!   no free routine left to port.
//! - [`OpenFilesScreen_draw`] (`OpenFilesScreen.c:95`) — a one-line
//!   forward to `InfoScreen_drawTitled`, which is a `todo!()` in
//!   `infoscreen.rs` (blocked on `String_stripControlChars`, absent from
//!   the port-purity snapshot, plus the unported `IncSet_drawBar`). No
//!   splittable logic of its own.
//! - [`OpenFilesScreen_getProcessData`] (`OpenFilesScreen.c:99`) — the
//!   `lsof` subprocess: `pipe`/`fork`/`dup2`/`execlp`, `String_readLine`
//!   over the child's `-F` stream, `xWaitpid`, and a `stat()` size
//!   fallback. All of it is direct POSIX syscall work (`unistd.h` /
//!   `sys/wait.h` / `sys/stat.h`) with no libc-dependency modelled in the
//!   port yet, so it cannot be ported faithfully.
//! - [`OpenFilesScreen_scan`] (`OpenFilesScreen.c:264`) — the vtable
//!   `scan` hook: it prunes the panel, calls
//!   [`OpenFilesScreen_getProcessData`] (stubbed above), formats each
//!   file row with `xAsprintf`, feeds it through `InfoScreen_addLine`
//!   (ported), then re-sorts. It is fundamentally gated on the `lsof`
//!   data the stubbed `getProcessData` would produce, so it stays a stub
//!   until that syscall substrate exists.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::ported::functionbar::Ncurses;
use crate::ported::incset::IncSet_new;
use crate::ported::infoscreen::{InfoScreen, InfoScreen_init};
use crate::ported::listitem::ListItem_new;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::Panel_new;
use crate::ported::process::{Process, Process_getPid, Process_getThreadGroup, Process_isThread};
use crate::ported::vector::Vector_new;

/// Port of `#define LSOF_DATACOL_COUNT 8` from `OpenFilesScreen.c:31`.
/// The number of `lsof -F` field columns tracked per open file; must be
/// larger than the maximum index [`getIndexForType`] returns.
const LSOF_DATACOL_COUNT: usize = 8;

/// Port of `#define VECTOR_DEFAULT_SIZE (10)` from `Vector.h:15` — the
/// initial `lines` capacity used when bootstrapping the throwaway
/// [`InfoScreen`] storage in [`OpenFilesScreen_new`] (see its docs). Mirrors
/// the private constant `infoscreen.rs` uses for the same purpose.
const VECTOR_DEFAULT_SIZE: i32 = 10;

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
/// omitted (see the module docs) — only the stubbed `scan`/`draw` dispatch
/// paths read it.
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

/// TODO: port of `static OpenFiles_ProcessData* OpenFilesScreen_getProcessData(pid_t pid)`
/// from `OpenFilesScreen.c:99`. Runs `lsof -P -o -p <pid> -F` as a child
/// process (`pipe`/`fork`/`dup2`/`execlp`), parses the `-F` field stream
/// line by line (`String_readLine`), reaps the child (`xWaitpid`), and, on
/// Linux where `lsof -o -F` omits SIZE, backfills it with `stat()`. This is
/// all direct POSIX syscall work (`unistd.h`/`sys/wait.h`/`sys/stat.h`)
/// with no libc-dependency substrate in the port yet, so it cannot be
/// ported faithfully. (Its `OpenFiles_ProcessData` / `OpenFiles_FileData`
/// result structs would be defined here alongside it once this lands.)
pub fn OpenFilesScreen_getProcessData() {
    todo!("port of OpenFilesScreen.c:99 — pipe/fork/execlp lsof + waitpid + stat; no POSIX-syscall substrate ported")
}

/// TODO: port of `static void OpenFiles_Data_clear(OpenFiles_Data* data)`
/// from `OpenFilesScreen.c:259`. Frees every `data->data[i]` string — a
/// heap-free-only routine. [`OpenFiles_Data`] owns its `[Option<String>; 8]`
/// and frees the strings via `Drop`, so there is no algorithm to port (the
/// `Vector_delete` / `History_delete` precedent). It is also only reached
/// from the stubbed [`OpenFilesScreen_getProcessData`] / [`OpenFilesScreen_scan`].
pub fn OpenFiles_Data_clear() {
    todo!("port of OpenFilesScreen.c:259 — Drop frees the owned column strings")
}

/// TODO: port of `static void OpenFilesScreen_scan(InfoScreen* super)` from
/// `OpenFilesScreen.c:264`. The vtable `scan` hook: `Panel_prune`, then
/// [`OpenFilesScreen_getProcessData`] (stubbed — `lsof` subprocess), then
/// per-file `xAsprintf` row formatting fed through `InfoScreen_addLine`
/// (ported), `Panel_setHeader`, and a final `Vector_insertionSort` /
/// `Panel_setSelected`. It is fundamentally gated on the `lsof` data the
/// stubbed `getProcessData` produces, so it stays a stub until that
/// syscall substrate exists.
pub fn OpenFilesScreen_scan() {
    todo!("port of OpenFilesScreen.c:264 — depends on OpenFilesScreen_getProcessData (lsof subprocess, unported)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::incset::IncSet_filter;
    use crate::ported::panel::{Panel_headerHeight, Panel_size};
    use crate::ported::process::{Process, Process_setPid, Process_setThreadGroup};
    use crate::ported::vector::Vector_size;

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
        let mut d = OpenFiles_Data {
            data: Default::default(),
        };
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
        let d = OpenFiles_Data {
            data: Default::default(),
        };
        for &c in b"faDinsto" {
            assert_eq!(getDataForType(&d, c), "");
        }
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
        let bar = s.super_.display.defaultBar.as_ref().expect("default bar built");
        // The InfoScreen bar labels/keys (Search/Filter/Refresh/Done).
        assert_eq!(bar.functions, vec!["Search ", "Filter ", "Refresh", "Done   "]);
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
}
