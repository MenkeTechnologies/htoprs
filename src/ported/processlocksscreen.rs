//! Port of `ProcessLocksScreen.c` — htop's "file locks of a process"
//! `InfoScreen` subclass (F-key screen listing the fcntl/flock locks held
//! by the selected process).
//!
//! `ProcessLocksScreen` (`ProcessLocksScreen.h:19`) is a thin subclass:
//! `struct ProcessLocksScreen_ { InfoScreen super; pid_t pid; }`. The
//! constructor stores the process's (thread-group) pid, builds the shared
//! `InfoScreen` substrate via [`InfoScreen_init`], and installs the
//! fixed-column table header. The `scan`/`draw`/`delete` hooks are the
//! `InfoScreenClass` virtual methods.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module. A C fn
//! `Foo_bar(Foo* this)` ports to a free fn `Foo_bar(this: &mut Foo)` — free
//! fns, not methods, matching the `Vector.c` / `History.c` / `InfoScreen.c`
//! ports.
//!
//! # Struct mapping (`ProcessLocksScreen.h:19`)
//!
//! `InfoScreen super` becomes an owned [`InfoScreen`] field named `super_`
//! (`super` is a Rust keyword; the `super_` convention matches
//! `backtracescreen.rs` / `affinitypanel.rs`). `pid_t pid` becomes `i32`,
//! the type [`Process_getPid`] / [`Process_getThreadGroup`] return.
//!
//! # Ported
//!
//! - The [`ProcessLocksScreen`] struct (`ProcessLocksScreen.h:19`).
//! - [`ProcessLocksScreen_new`] (`ProcessLocksScreen.c:23`) — resolves the
//!   pid (thread-group id for a thread, else the pid), then chains through
//!   [`InfoScreen_init`] with `LINES - 2` height and the column header.
//! - [`ProcessLocksScreen_draw`] (`ProcessLocksScreen.c:38`) — the
//!   `InfoScreenClass` `draw` hook; a single forward to [`InfoScreen_drawTitled`]
//!   (now ported) with the `"Snapshot of file locks of process %d - %s"` title
//!   built from the stored pid and [`Process_getCommand`]. The latter is still a
//!   `todo!()`, so a real draw panics through it — faithful chain-of-stubs
//!   wiring (matching `CommandScreen_draw`).
//!
//! ## Divergences (documented)
//!
//! - **`xMalloc` + `Object_setClass`.** C allocates uninitialized storage
//!   and installs the `ProcessLocksScreen_class` vtable pointer, then
//!   `InfoScreen_init` overwrites every `super` field. The port builds a
//!   throwaway zeroed [`InfoScreen`] (the same field set `InfoScreen::empty`
//!   uses — private there, so replicated inline) which `InfoScreen_init`
//!   immediately overwrites; the vtable install has no analog (the ported
//!   `InfoScreen` carries no `Object super` vtable — see `infoscreen.rs`).
//! - **`LINES`.** C passes the ncurses `LINES` global; the ported analog is
//!   [`Ncurses::lines`], the terminal row count (the same source
//!   `Panel_draw` reads), matching how `InfoScreen_init` maps `COLS` to
//!   [`Ncurses::cols`].
//! - **`const Process*` return.** C returns a `ProcessLocksScreen*` (the
//!   `InfoScreen_init` identity chain-return, cast back). The port returns
//!   the owned struct by value.
//!
//! # Stubbed (cannot be ported faithfully yet), each naming its blocker
//!
//! - [`ProcessLocksScreen_delete`] (`ProcessLocksScreen.c:34`) —
//!   `free(InfoScreen_done((InfoScreen*)this))`, i.e. heap-free only.
//!   [`InfoScreen_done`](crate::ported::infoscreen::InfoScreen_done) is
//!   itself a `todo!()` (heap-free with no safe-Rust analog: an owned
//!   `InfoScreen`/`ProcessLocksScreen` releases its fields via `Drop`), so
//!   there is no algorithm to port (same class as `InfoScreen_done` /
//!   `History_delete`).
//! - [`FileLocks_Data_clear`] (`ProcessLocksScreen.c:42`) — `static inline`;
//!   frees the four `char*` fields (`locktype`/`exclusive`/`readwrite`/
//!   `filename`) of a `FileLocks_Data`. It is heap-free only: modeled with
//!   owned `String`s those fields free themselves via `Drop`, so there is no
//!   body to port. The `FileLocks_Data` / `FileLocks_LockData` /
//!   `FileLocks_ProcessData` structs (`ProcessLocksScreen.h:24`/`36`/`41`)
//!   are not modeled here because the only consumers are this free-only
//!   helper and [`ProcessLocksScreen_scan`] + `Platform_getProcessLocks`,
//!   all blocked below — defining them now would unblock nothing.
//! - [`ProcessLocksScreen_scan`] (`ProcessLocksScreen.c:49`) — the
//!   `InfoScreenClass` `scan` hook. Its per-line substrate is available
//!   (`Panel_getSelectedIndex` / `Panel_prune` / `Panel_setSelected`
//!   (`panel.rs`), `InfoScreen_addLine` (`infoscreen.rs`),
//!   `Vector_insertionSort` (`vector.rs`)), but its data source
//!   `Platform_getProcessLocks(pid)` (`Platform.c:555`) is an unported
//!   `todo!()` (`linux/platform.rs`): it parses `/proc/<pid>/*` lock state
//!   into the unmodeled `FileLocks_ProcessData` list. Without that lock
//!   enumeration there is nothing to iterate, format, and add — the whole
//!   function is gated on it.
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::functionbar::Ncurses;
use crate::ported::incset::IncSet_new;
use crate::ported::infoscreen::{InfoScreen, InfoScreen_drawTitled, InfoScreen_init};
use crate::ported::listitem::ListItem_new;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::Panel_new;
use crate::ported::process::{
    Process, Process_getCommand, Process_getPid, Process_getThreadGroup, Process_isThread,
};
use crate::ported::vector::Vector_new;

/// Port of `#define VECTOR_DEFAULT_SIZE (10)` from `Vector.h:15` — the
/// initial `lines` capacity for the throwaway `InfoScreen` (overwritten by
/// [`InfoScreen_init`]); mirrors the value `InfoScreen::empty` uses.
const VECTOR_DEFAULT_SIZE: i32 = 10;

/// Port of `struct ProcessLocksScreen_` (`ProcessLocksScreen.h:19`): an
/// `InfoScreen super` (owned; named `super_` since `super` is reserved) plus
/// the resolved `pid_t pid` of the process whose locks are shown.
pub struct ProcessLocksScreen {
    /// C `InfoScreen super` — the shared scrollable-info-panel substrate.
    pub super_: InfoScreen,
    /// C `pid_t pid` — the pid (thread-group id for a thread) whose file
    /// locks this screen enumerates.
    pub pid: i32,
}

/// Port of `ProcessLocksScreen* ProcessLocksScreen_new(const Process*
/// process)` from `ProcessLocksScreen.c:23`.
///
/// Resolves `pid` to the thread-group id when `process` is a thread
/// (C `Process_isThread`), else its own pid, then chains through
/// [`InfoScreen_init`] with height `LINES - 2` ([`Ncurses::lines`]) and the
/// fixed column header. The `xMalloc` + `Object_setClass` allocation maps to
/// a throwaway zeroed `super_` that `InfoScreen_init` overwrites (see the
/// module docs). Returns the owned struct (C returns the `InfoScreen_init`
/// identity, cast back to `ProcessLocksScreen*`).
pub fn ProcessLocksScreen_new(process: &Process) -> ProcessLocksScreen {
    // C: if (Process_isThread(process)) this->pid = Process_getThreadGroup(process);
    //    else this->pid = Process_getPid(process);
    let pid = if Process_isThread(process) {
        Process_getThreadGroup(process)
    } else {
        Process_getPid(process)
    };

    // C: ProcessLocksScreen* this = xMalloc(...); Object_setClass(...);
    // The `super_` here is the uninitialized-storage analog — InfoScreen_init
    // overwrites process/display/inc/lines below. Same field set as the
    // (private) `InfoScreen::empty` bootstrap.
    let list_item_class: &'static ObjectClass = ListItem_new("", 0).klass();
    let mut this = ProcessLocksScreen {
        super_: InfoScreen {
            process: core::ptr::null(),
            display: Panel_new(0, 0, 0, 0, None),
            inc: IncSet_new(None),
            lines: Vector_new(list_item_class, true, VECTOR_DEFAULT_SIZE),
        },
        pid,
    };

    // C: return (ProcessLocksScreen*) InfoScreen_init(&this->super, process,
    //       NULL, LINES - 2, "   FD TYPE       EXCLUSION ...  FILENAME");
    InfoScreen_init(
        &mut this.super_,
        process as *const Process,
        None,
        Ncurses::lines() - 2,
        "   FD TYPE       EXCLUSION  READ/WRITE DEVICE       NODE               START                 END  FILENAME",
    );

    this
}

/// TODO: port of `void ProcessLocksScreen_delete(Object* this)` from
/// `ProcessLocksScreen.c:34`. `free(InfoScreen_done((InfoScreen*)this))` —
/// heap-free only; `InfoScreen_done` is itself a `todo!()` (owned fields free
/// via `Drop`, no algorithm to port). Same class as `InfoScreen_done` /
/// `History_delete`.
pub fn ProcessLocksScreen_delete() {
    todo!(
        "port of ProcessLocksScreen.c:34 — free(InfoScreen_done(...)); Drop releases owned fields"
    )
}

/// Port of `static void ProcessLocksScreen_draw(InfoScreen* this)` from
/// `ProcessLocksScreen.c:38`. A single forward to [`InfoScreen_drawTitled`]:
/// C `InfoScreen_drawTitled(this, "Snapshot of file locks of process %d - %s",
/// ((ProcessLocksScreen*)this)->pid, Process_getCommand(this->process))`.
///
/// `%d` is the stored [`ProcessLocksScreen::pid`] (C's `(ProcessLocksScreen*)this`
/// downcast — so the port takes `&mut ProcessLocksScreen`, not `&mut InfoScreen`,
/// to reach the field) and `%s` is [`Process_getCommand`] on the
/// `super_.process` back-pointer (a `const char*`, rendered lossily from its
/// bytes; `None` -> empty). The variadic `fmt, ...` becomes a pre-built `&str`
/// (the `xSnprintf`/`vsnprintf` idiom [`InfoScreen_drawTitled`] expects).
/// `Process_getCommand` is still a `todo!()` stub, so a real draw panics
/// through it — the faithful chain-of-stubs wiring (same as `CommandScreen_draw`).
pub fn ProcessLocksScreen_draw(this: &mut ProcessLocksScreen) {
    // C: InfoScreen_drawTitled(this, "Snapshot of file locks of process %d - %s",
    //        ((ProcessLocksScreen*)this)->pid, Process_getCommand(this->process));
    let pid = this.pid;
    let cmd = Process_getCommand(unsafe { &*this.super_.process });
    let cmd = match cmd {
        Some(b) => String::from_utf8_lossy(b).into_owned(),
        None => String::new(),
    };
    let title = format!("Snapshot of file locks of process {} - {}", pid, cmd);
    InfoScreen_drawTitled(&mut this.super_, &title);
}

/// TODO: port of `static inline void FileLocks_Data_clear(FileLocks_Data*
/// data)` from `ProcessLocksScreen.c:42`. Frees the four `char*` fields —
/// heap-free only; modeled with owned `String`s they free via `Drop`, so
/// there is no body to port. The `FileLocks_*` structs are not modeled here
/// (their only consumers — this helper and the blocked scan below — do not
/// need them yet).
pub fn FileLocks_Data_clear() {
    todo!("port of ProcessLocksScreen.c:42 — heap-free of 4 char* fields; owned String frees via Drop")
}

/// TODO: port of `static void ProcessLocksScreen_scan(InfoScreen* this)` from
/// `ProcessLocksScreen.c:49`. Blocked on `Platform_getProcessLocks(pid)`
/// (`Platform.c:555`), an unported `todo!()` in `linux/platform.rs` that
/// parses `/proc/<pid>` lock state into the unmodeled `FileLocks_ProcessData`
/// list. The per-line substrate (`Panel_prune` / `Panel_getSelectedIndex` /
/// `Panel_setSelected`, `InfoScreen_addLine`, `Vector_insertionSort`) is
/// available, but there is no lock data to iterate and format without it.
pub fn ProcessLocksScreen_scan() {
    todo!("port of ProcessLocksScreen.c:49 — needs Platform_getProcessLocks (Platform.c:555, unported) + FileLocks_ProcessData structs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::infoscreen::InfoScreen_addLine;
    use crate::ported::panel::{Panel_headerHeight, Panel_size};
    use crate::ported::process::{Process_setPid, Process_setThreadGroup};
    use crate::ported::richstring::RichString;
    use crate::ported::vector::Vector_size;

    /// Render a `RichString` back to a plain `String` (the header is stored
    /// as a `RichString`; matches the `backtracescreen.rs` test helper).
    fn rendered(rs: &RichString) -> String {
        (0..rs.chlen as usize).map(|i| rs.chptr[i].chars).collect()
    }

    /// A non-thread `Process` with the given pid.
    fn proc_with_pid(pid: i32) -> Process {
        let mut p = Process::default();
        Process_setPid(&mut p, pid);
        p
    }

    #[test]
    fn new_uses_pid_for_non_thread() {
        let p = proc_with_pid(4321);
        let s = ProcessLocksScreen_new(&p);
        // Not a thread -> pid is the process's own pid.
        assert!(!Process_isThread(&p));
        assert_eq!(s.pid, 4321);
    }

    #[test]
    fn new_uses_thread_group_for_thread() {
        let mut p = Process::default();
        Process_setPid(&mut p, 4321); // the thread's own tid
        Process_setThreadGroup(&mut p, 999); // the owning process
        p.isUserlandThread = true; // makes Process_isThread true
        assert!(Process_isThread(&p));

        let s = ProcessLocksScreen_new(&p);
        // Thread -> pid resolves to the thread group, not the tid.
        assert_eq!(s.pid, 999);
    }

    #[test]
    fn new_stores_process_backpointer() {
        let p = proc_with_pid(7);
        let s = ProcessLocksScreen_new(&p);
        // InfoScreen_init stored the &Process as a raw back-pointer.
        assert_eq!(s.super_.process, &p as *const Process);
    }

    #[test]
    fn new_installs_infoscreen_geometry_and_header() {
        let p = proc_with_pid(10);
        let s = ProcessLocksScreen_new(&p);
        // InfoScreen_init: Panel_new(0, 1, COLS, LINES - 2, ...).
        assert_eq!(s.super_.display.x, 0);
        assert_eq!(s.super_.display.y, 1);
        assert_eq!(s.super_.display.w, Ncurses::cols());
        assert_eq!(s.super_.display.h, Ncurses::lines() - 2);
        // Header installed -> headerHeight 1; lines/panel start empty.
        assert_eq!(Panel_headerHeight(&s.super_.display), 1);
        assert_eq!(Vector_size(&s.super_.lines), 0);
        assert_eq!(Panel_size(&s.super_.display), 0);
    }

    #[test]
    fn new_header_matches_c_column_layout() {
        let p = proc_with_pid(1);
        let s = ProcessLocksScreen_new(&p);
        // The exact fixed-column header string from ProcessLocksScreen.c:31.
        assert_eq!(
            rendered(&s.super_.display.header),
            "   FD TYPE       EXCLUSION  READ/WRITE DEVICE       NODE               START                 END  FILENAME"
        );
    }

    #[test]
    fn addline_flows_through_the_ported_infoscreen() {
        // The scan hook is stubbed (Platform_getProcessLocks unported), but
        // the InfoScreen substrate the constructor wired up is live: a line
        // added lands in both `lines` and the (unfiltered) panel.
        let p = proc_with_pid(2);
        let mut s = ProcessLocksScreen_new(&p);
        InfoScreen_addLine(&mut s.super_, "  12 POSIX ...  /tmp/foo");
        assert_eq!(Vector_size(&s.super_.lines), 1);
        assert_eq!(Panel_size(&s.super_.display), 1);
    }
}
