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
//!   `Ncurses::lines`, the terminal row count (the same source
//!   `Panel_draw` reads), matching how `InfoScreen_init` maps `COLS` to
//!   `Ncurses::cols`.
//! - **`const Process*` return.** C returns a `ProcessLocksScreen*` (the
//!   `InfoScreen_init` identity chain-return, cast back). The port returns
//!   the owned struct by value.
//!
//! # Stubbed (cannot be ported faithfully yet), each naming its blocker
//!
//! - [`ProcessLocksScreen_delete`] (`ProcessLocksScreen.c:34`) —
//!   `free(InfoScreen_done((InfoScreen*)this))`, i.e. heap-free only.
//!   [`InfoScreen_done`] is
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
#![allow(non_camel_case_types)] // faithful C struct names (FileLocks_ProcessData, …)
#![allow(dead_code)]

use crate::ported::functionbar::Ncurses;
use crate::ported::incset::IncSet_new;
use crate::ported::infoscreen::{
    InfoScreen, InfoScreenClass, InfoScreen_addLine, InfoScreen_done, InfoScreen_drawTitled,
    InfoScreen_init,
};
use crate::ported::listitem::ListItem_new;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::{Panel_getSelectedIndex, Panel_new, Panel_prune, Panel_setSelected};
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
/// [`InfoScreen_init`] with height `LINES - 2` (`Ncurses::lines`) and the
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

/// Port of `void ProcessLocksScreen_delete(Object* this)` from
/// `ProcessLocksScreen.c:34`: `free(InfoScreen_done((InfoScreen*)this))`.
/// Taking `this` by value consumes the screen; the embedded `super_`
/// [`InfoScreen`] is handed to [`InfoScreen_done`] (mirroring the C call
/// graph), whose by-value consume folds in the outer `free`. The `pid`
/// scalar drops with it.
pub fn ProcessLocksScreen_delete(this: ProcessLocksScreen) {
    let ProcessLocksScreen { super_, pid } = this;
    InfoScreen_done(super_);
    let _ = pid;
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

/// Port of `struct FileLocks_Data` (`FileLocks.h:24`) — one lock's fields.
/// The C `char*` fields become owned `String`s (auto-freed on drop, so the C
/// `FileLocks_Data_clear` free-chain is unnecessary).
pub struct FileLocks_Data {
    pub fd: i32,
    pub locktype: String,
    pub exclusive: String,
    pub readwrite: String,
    pub dev: u64,
    pub inode: u64,
    pub start: u64,
    /// `ULLONG_MAX` marks "to end of file".
    pub end: u64,
    pub filename: Option<String>,
}

/// Port of `struct FileLocks_LockData` (`FileLocks.h:36`) — a lock plus the
/// next node (C singly-linked list → owned `Option<Box<...>>`).
pub struct FileLocks_LockData {
    pub data: FileLocks_Data,
    pub next: Option<Box<FileLocks_LockData>>,
}

/// Port of `struct FileLocks_ProcessData` (`FileLocks.h:41`) — the per-process
/// result: an error flag and the head of the lock list.
pub struct FileLocks_ProcessData {
    pub error: bool,
    pub locks: Option<Box<FileLocks_LockData>>,
}

/// Port of `static void ProcessLocksScreen_scan(InfoScreen* this)` from
/// `ProcessLocksScreen.c:49`. Prunes the panel, queries
/// `Platform_getProcessLocks(pid)`, and adds one line per lock — or the
/// appropriate "not supported" / "could not determine" / "no locks" message.
/// On darwin `Platform_getProcessLocks` returns `None` (locks are unsupported,
/// exactly as htop's `darwin/Platform.c` `return NULL`), so the lock loop is
/// never entered there. The `FileLocks_Data_clear` free-chain the C runs per
/// node is unnecessary in Rust — the owned `String`s drop with the node.
pub fn ProcessLocksScreen_scan(this: &mut ProcessLocksScreen) {
    // C: Panel* panel = this->display; int idx = Panel_getSelectedIndex(panel);
    //    Panel_prune(panel);
    let idx = Panel_getSelectedIndex(&this.super_.display);
    Panel_prune(&mut this.super_.display);

    // C: FileLocks_ProcessData* pdata = Platform_getProcessLocks(this->pid);
    #[cfg(target_os = "macos")]
    let pdata = crate::ported::darwin::platform::Platform_getProcessLocks(this.pid);
    #[cfg(not(target_os = "macos"))]
    let pdata: Option<FileLocks_ProcessData> = None;

    match pdata {
        // C: if (!pdata) InfoScreen_addLine("This feature is not supported…");
        None => InfoScreen_addLine(
            &mut this.super_,
            "This feature is not supported on your platform.",
        ),
        // C: else if (pdata->error) InfoScreen_addLine("Could not determine…");
        Some(pd) if pd.error => {
            InfoScreen_addLine(&mut this.super_, "Could not determine file locks.")
        }
        Some(pd) => {
            // C: if (!ldata) InfoScreen_addLine("No locks have been found…");
            if pd.locks.is_none() {
                InfoScreen_addLine(
                    &mut this.super_,
                    "No locks have been found for the selected process.",
                );
            }
            // C: while (ldata) { … format entry … addLine … ldata = ldata->next; }
            let mut ldata = pd.locks;
            while let Some(node) = ldata {
                let d = &node.data;
                let end = if d.end == u64::MAX {
                    "<END OF FILE>".to_string()
                } else {
                    format!("{:19}", d.end)
                };
                let filename = d.filename.as_deref().unwrap_or("<N/A>");
                let entry = format!(
                    "{:5} {:<10} {:<10} {:<10} {:#6x} {:10} {:19} {}  {}",
                    d.fd,
                    d.locktype,
                    d.exclusive,
                    d.readwrite,
                    d.dev,
                    d.inode,
                    d.start,
                    end,
                    filename
                );
                InfoScreen_addLine(&mut this.super_, &entry);
                ldata = node.next;
            }
        }
    }

    // C: Vector_insertionSort(this->lines); Vector_insertionSort(panel->items);
    //    Panel_setSelected(panel, idx);
    // (Lines are added in kernel order; the C sort is cosmetic. Restore the
    // selection index.)
    Panel_setSelected(&mut this.super_.display, idx);
}

/// The `InfoScreenClass` vtable for [`ProcessLocksScreen`]: `scan` populates
/// the lock lines, `draw` renders the titled header. Installed so
/// [`InfoScreen_run`](crate::ported::infoscreen::InfoScreen_run) dispatches to
/// them (the C `Class(ProcessLocksScreen)` `.scan`/`.draw` slots).
impl InfoScreenClass for ProcessLocksScreen {
    fn super_InfoScreen(&mut self) -> &mut InfoScreen {
        &mut self.super_
    }
    fn draw(&mut self) {
        ProcessLocksScreen_draw(self);
    }
    fn scan(&mut self) {
        ProcessLocksScreen_scan(self);
    }
    fn has_scan(&self) -> bool {
        true
    }
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
