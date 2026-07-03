//! Partial port of `TraceScreen.c` — htop's live strace/truss viewer.
//!
//! `TraceScreen` forks an external tracer (`strace -T -tt -s 512 -p PID`
//! on Linux, `truss -s 512 -p PID` on the BSDs/Solaris), reads its output
//! through a non-blocking pipe, and streams the lines into an
//! [`InfoScreen`] panel with follow/pause toggles. The C struct is
//! `struct { InfoScreen super; FILE* strace; pid_t child; bool tracing;
//! bool contLine; bool follow; bool strace_alive; }` (`TraceScreen.h:19`);
//! every function is dispatched on `TraceScreen*` or on the downcast
//! `InfoScreen* super` and dereferences `this->super.process`,
//! `super->display`, or a sibling `InfoScreen_*` method.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module.
//!
//! # Struct mapping (`TraceScreen.h:19`)
//!
//! `InfoScreen super` becomes the embedded `super_: InfoScreen` field (the
//! same `super_` idiom `openfilesscreen.rs` / `commandscreen.rs` use for
//! their `InfoScreen` base). The C `FILE* strace` — a buffered handle over
//! the pipe read end obtained with `fdopen` — becomes an owned
//! `Option<File>`: `fileno(this->strace)` maps to `File::as_raw_fd`,
//! `fread` maps to `File::read`, and `fclose` maps to dropping the `File`
//! (`None`). `pid_t child` is `libc::pid_t`; the three tracer flags map
//! straight across.
//!
//! # Ported
//!
//! - The [`TraceScreen`] struct (`TraceScreen.h:19`).
//! - [`TraceScreen_new`] (`TraceScreen.c:37`) — builds the object with the
//!   `tracing = true` / `strace_alive = false` defaults, the
//!   `TraceScreen*` function bar, disables the CRT input delay, and hands
//!   the embedded `super` to [`InfoScreen_init`] (now ported).
//! - [`TraceScreen_forkTracer`] (`TraceScreen.c:67`) — the raw
//!   `pipe`/`fcntl(O_NONBLOCK)`/`fork`/`dup2`/`execvp` tracer launch, via
//!   `libc` (the same direct-`libc` idiom `affinity.rs` / `scheduling.rs`
//!   use; `libc`/`nix` are both crate deps). `execlp` maps to `execvp`
//!   (both search `$PATH`); the per-OS tracer selection uses `cfg` in
//!   place of the C `#if defined(HTOP_*)`. On darwin (the dev host) and any
//!   other unsupported target the child writes the "Tracing unavailable"
//!   message and `_exit(127)`, exactly like the C `#else` arm — so the file
//!   compiles and the fork path is exercised on darwin.
//! - [`TraceScreen_delete`] (`TraceScreen.c:48`) — the real teardown:
//!   `kill(child, SIGTERM)` + the `xWaitpid(child, NULL, 0, false)` reap
//!   loop + `fclose(strace)` (drop the `File`) + `CRT_enableDelay`. The
//!   trailing `free(InfoScreen_done(...))` is heap-free only, so the owned
//!   `super_` fields are released by `Drop` (the same reasoning
//!   `InfoScreen_done` / `OpenFilesScreen_delete` document).
//! - [`TraceScreen_draw`] (`TraceScreen.c:63`) — the vtable `draw` hook: a
//!   one-line [`InfoScreen_drawTitled`] delegation (now ported) with the
//!   `"Trace of process %d - %s"` title pre-formatted from [`Process_getPid`]
//!   / [`Process_getCommand`]. `Process_getCommand` is a `todo!()`, so a live
//!   draw panics through it — the faithful chain-of-stubs wiring.
//! - [`TraceScreen_updateTrace`] (`TraceScreen.c:134`) — the vtable `onErr`
//!   hook: the `select`/`fread` pipe drain, the `'\n'`-split
//!   [`InfoScreen_addLine`] path, the `follow` `Panel_setSelected`, and the
//!   inlined `xWaitpid(WNOHANG)` liveness check. The `contLine` branch calls
//!   the now-ported [`InfoScreen_appendLine`]`(&this->super, line)` to merge a
//!   continuation onto the previous partial line.
//! - [`TraceScreen_onKey`] (`TraceScreen.c:185`) — the vtable `onKey` hook:
//!   the `f`/`F8` follow toggle and the `t`/`F9` tracing toggle
//!   (`FunctionBar_setLabel` relabel + repaint). The C `InfoScreen_draw(this)`
//!   vtable dispatch (`InfoScreen.h:45`) resolves statically to
//!   [`TraceScreen_draw`] on a `TraceScreen`, so it is a direct call.
//!
//! ## Divergences (documented, per "port what you can")
//!
//! - **argv built pre-fork.** C fills a `char buffer[32]` with the pid via
//!   a stack `xSnprintf` *inside* the child, then `execlp`s string
//!   literals — an allocation-free child. The port builds the argv
//!   `CString`s (including the pid) in the parent, before `fork`, so the
//!   child performs only async-signal-safe `libc` calls (`close`/`dup2`/
//!   `execvp`/`write`/`_exit`) with no post-fork allocation. Same observable
//!   exec.
//! - **`fdopen` is infallible here.** C `goto err`s if `fdopen(fdpair[0],
//!   "r")` returns `NULL`; `File::from_raw_fd` cannot fail, so that arm is
//!   unreachable and omitted (the fd is already open and valid).
//! - **`xWaitpid` inlined.** `xWaitpid(this->child, NULL, 0, false)`
//!   (`XUtils.c:321`) with `wait_for_exit == false` reduces to the
//!   `EINTR`-retry `waitpid` loop; it is inlined because `xWaitpid` lives
//!   in the still-unported `XUtils.c` and cannot be called.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::ffi::c_int;
use std::fs::File;
use std::io::Read;
use std::os::unix::io::{AsRawFd, FromRawFd};

use crate::ported::crt::{CRT_disableDelay, CRT_enableDelay, KEY_F};
use crate::ported::functionbar::{FunctionBar_new, FunctionBar_setLabel, Ncurses};
use crate::ported::incset::IncSet_new;
use crate::ported::infoscreen::{
    InfoScreen, InfoScreen_addLine, InfoScreen_appendLine, InfoScreen_drawTitled, InfoScreen_init,
};
use crate::ported::listitem::ListItem_new;
use crate::ported::object::{Object, ObjectClass};
use crate::ported::panel::{Panel_new, Panel_setSelected, Panel_size};
use crate::ported::process::{Process, Process_getCommand, Process_getPid};
use crate::ported::vector::Vector_new;

/// Port of `#define VECTOR_DEFAULT_SIZE (10)` from `Vector.h:15` — the
/// initial `lines` vector capacity the seeded `super` hands to `Vector_new`
/// (overwritten by [`InfoScreen_init`]).
const VECTOR_DEFAULT_SIZE: c_int = 10;

/// Port of `static const char* const TraceScreenFunctions[]`
/// (`TraceScreen.c:31`), minus the trailing `NULL` (Rust length terminates).
const TraceScreenFunctions: [&str; 5] = [
    "Search ",
    "Filter ",
    "AutoScroll ",
    "Stop Tracing   ",
    "Done   ",
];

/// Port of `static const char* const TraceScreenKeys[]` (`TraceScreen.c:33`).
const TraceScreenKeys: [&str; 5] = ["F3", "F4", "F8", "F9", "Esc"];

/// Port of `static const int TraceScreenEvents[]` (`TraceScreen.c:35`):
/// `{KEY_F(3), KEY_F(4), KEY_F(8), KEY_F(9), 27}` (`crt::KEY_F` reproduces
/// the ncurses codes; `27` is `Esc`).
const TraceScreenEvents: [c_int; 5] = [KEY_F(3), KEY_F(4), KEY_F(8), KEY_F(9), 27];

/// Port of `struct TraceScreen_` (`TraceScreen.h:19`). See the module docs
/// for the field mapping: `InfoScreen super` -> embedded `super_`,
/// `FILE* strace` -> `Option<File>` over the pipe read end, `pid_t child`
/// -> `libc::pid_t`, and the three tracer flags map straight across.
pub struct TraceScreen {
    /// C `InfoScreen super` — the scrollable info panel base class.
    pub super_: InfoScreen,
    /// C `FILE* strace` — buffered handle over the tracer pipe read end
    /// (`fdopen(fdpair[0], "r")`); `None` == closed.
    pub strace: Option<File>,
    /// C `pid_t child` — the forked tracer's pid (`0` == none).
    pub child: libc::pid_t,
    /// C `bool tracing` — whether new trace lines are appended (F9 toggle).
    pub tracing: bool,
    /// C `bool contLine` — the previous read ended mid-line; the next
    /// complete line continues it (`InfoScreen_appendLine`).
    pub contLine: bool,
    /// C `bool follow` — auto-scroll to the newest line (F8 toggle).
    pub follow: bool,
    /// C `bool strace_alive` — the tracer child is still running.
    pub strace_alive: bool,
}

/// Port of `TraceScreen* TraceScreen_new(const Process* process)` from
/// `TraceScreen.c:37`.
///
/// Initialises every flag to the C defaults — `tracing = true`,
/// `strace_alive = false`, and (the C `xCalloc` zeroing) `contLine`/
/// `follow`/`child`/`strace` all clear — builds the `TraceScreen` function
/// bar, disables the CRT input delay (`CRT_disableDelay`, so the trace loop
/// polls without blocking), and hands the embedded `super` to
/// [`InfoScreen_init`] with the `LINES - 2` panel height (`Ncurses::lines()`,
/// the same terminal-metric source `infoscreen.rs` uses) and the `" "`
/// panel header.
///
/// Divergence: C `xCalloc`s the object (zeroed `super`) then overwrites it;
/// Rust needs a valid `InfoScreen` value first, so `super_` is seeded with
/// the same throwaway empty storage `InfoScreen::empty` builds — the
/// AllocThis-uninitialized-storage idiom — which [`InfoScreen_init`] then
/// fully replaces. The C `Object_setClass(this, Class(TraceScreen))` vtable
/// install is omitted (the vtable is not modelled). C returns
/// `(TraceScreen*) InfoScreen_init(&this->super, ...)`; since `super` is at
/// offset 0 the cast is identity, so the port returns `this`.
pub fn TraceScreen_new(process: &Process) -> TraceScreen {
    // Seed `super` with throwaway empty storage (== InfoScreen::empty),
    // mirroring the zeroed `super` C's xCalloc hands to InfoScreen_init.
    let list_item_class: &'static ObjectClass = ListItem_new("", 0).klass();
    let mut this = TraceScreen {
        super_: InfoScreen {
            process: core::ptr::null(),
            display: Panel_new(0, 0, 0, 0, None),
            inc: IncSet_new(None),
            lines: Vector_new(list_item_class, true, VECTOR_DEFAULT_SIZE),
        },
        strace: None,
        child: 0,
        // C: this->tracing = true; this->strace_alive = false;
        // (all other fields zeroed by xCalloc).
        tracing: true,
        contLine: false,
        follow: false,
        strace_alive: false,
    };

    // C: FunctionBar* fuBar = FunctionBar_new(TraceScreenFunctions, TraceScreenKeys, TraceScreenEvents);
    let fuBar = FunctionBar_new(
        Some(&TraceScreenFunctions[..]),
        Some(&TraceScreenKeys[..]),
        Some(&TraceScreenEvents[..]),
    );

    // C: CRT_disableDelay();
    CRT_disableDelay();

    // C: return (TraceScreen*) InfoScreen_init(&this->super, process, fuBar, LINES - 2, " ");
    InfoScreen_init(
        &mut this.super_,
        process as *const Process,
        Some(fuBar),
        Ncurses::lines() - 2,
        " ",
    );

    this
}

/// Port of `void TraceScreen_delete(Object* cast)` from `TraceScreen.c:48`.
///
/// If a tracer child is running, sends it `SIGTERM` and reaps it — C
/// `kill(this->child, SIGTERM); xWaitpid(this->child, NULL, 0, false);`.
/// `xWaitpid` with `wait_for_exit == false` (`XUtils.c:321`) is the
/// `EINTR`-retry `waitpid` loop, inlined here because `XUtils.c` is
/// unported. Closes the tracer pipe (`fclose(this->strace)` -> drop the
/// `File`), restores the CRT input delay (`CRT_enableDelay`), and lets the
/// owned `super_` fields free themselves — the C tail
/// `free(InfoScreen_done((InfoScreen*)this))` is heap-free only (the same
/// reasoning `InfoScreen_done` / `OpenFilesScreen_delete` document).
pub fn TraceScreen_delete(this: &mut TraceScreen) {
    // C: if (this->child > 0) { kill(this->child, SIGTERM); xWaitpid(...); }
    if this.child > 0 {
        unsafe {
            libc::kill(this.child, libc::SIGTERM);
            // xWaitpid(this->child, NULL, 0, false): retry waitpid on EINTR.
            let mut status: c_int = 0;
            loop {
                let ret = libc::waitpid(this.child, &mut status, 0);
                if ret != -1 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR)
                {
                    break;
                }
            }
        }
    }

    // C: if (this->strace) fclose(this->strace);  — drop closes the fd.
    this.strace = None;

    // C: CRT_enableDelay();
    CRT_enableDelay();

    // C: free(InfoScreen_done((InfoScreen*)this));  — owned super_ frees via Drop.
}

/// Port of `static void TraceScreen_draw(InfoScreen* this)` from
/// `TraceScreen.c:63` — the vtable `draw` hook. A single
/// [`InfoScreen_drawTitled`] call with the C `printf`-style `"Trace of
/// process %d - %s"` pre-formatted (the ported `InfoScreen_drawTitled` takes
/// an already-built `&str`, the standard `xSnprintf`/`vsnprintf` idiom, the
/// same way [`crate::ported::commandscreen`]`::CommandScreen_draw` builds its
/// title). `%d` is [`Process_getPid`] and `%s` is [`Process_getCommand`] (a
/// `const char*`, rendered lossily from its bytes; `None` -> empty).
/// `Process_getCommand` is still a `todo!()` stub, so a live draw panics
/// through it — the faithful chain-of-stubs wiring.
pub fn TraceScreen_draw(this: &mut InfoScreen) {
    // C: InfoScreen_drawTitled(this, "Trace of process %d - %s",
    //        Process_getPid(this->process), Process_getCommand(this->process));
    let pid = Process_getPid(unsafe { &*this.process });
    let cmd = match Process_getCommand(unsafe { &*this.process }) {
        Some(b) => String::from_utf8_lossy(b).into_owned(),
        None => String::new(),
    };
    let title = format!("Trace of process {} - {}", pid, cmd);
    InfoScreen_drawTitled(this, &title);
}

/// Port of `bool TraceScreen_forkTracer(TraceScreen* this)` from
/// `TraceScreen.c:67`.
///
/// Creates a pipe, sets both ends non-blocking (`fcntl(F_SETFL,
/// O_NONBLOCK)`), forks, and in the child redirects stdout/stderr onto the
/// write end (`dup2`) before `execvp`-ing the tracer (`strace` on Linux,
/// `truss` on the BSDs/Solaris; `execlp` -> `execvp`, both search `$PATH`).
/// The parent stores the child pid, wraps the read end in a buffered `File`
/// (`fdopen`), closes the write end, and marks the tracer alive. Every
/// error path closes the pipe and returns `false`, matching the C
/// `goto err`. See the module docs for the pre-fork argv, the infallible
/// `fdopen`, and the per-OS `cfg` divergences.
pub fn TraceScreen_forkTracer(this: &mut TraceScreen) -> bool {
    // C: int fdpair[2] = {-1, -1};
    let mut fdpair: [c_int; 2] = [-1, -1];

    // Build the tracer argv (including the pid) before fork so the child is
    // allocation-free (see module docs). Only the supported tracers need it.
    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        target_os = "solaris"
    ))]
    let argv_store: Vec<std::ffi::CString> = {
        // C: xSnprintf(buffer, sizeof(buffer), "%d", Process_getPid(this->super.process));
        let pid = unsafe { crate::ported::process::Process_getPid(&*this.super_.process) };
        let pid_c = std::ffi::CString::new(pid.to_string()).expect("pid string has no NUL");
        #[cfg(target_os = "linux")]
        {
            // C: execlp("strace", "strace", "-T", "-tt", "-s", "512", "-p", buffer, NULL);
            vec![
                std::ffi::CString::new("strace").unwrap(),
                std::ffi::CString::new("-T").unwrap(),
                std::ffi::CString::new("-tt").unwrap(),
                std::ffi::CString::new("-s").unwrap(),
                std::ffi::CString::new("512").unwrap(),
                std::ffi::CString::new("-p").unwrap(),
                pid_c,
            ]
        }
        #[cfg(not(target_os = "linux"))]
        {
            // C: execlp("truss", "truss", "-s", "512", "-p", buffer, NULL);
            vec![
                std::ffi::CString::new("truss").unwrap(),
                std::ffi::CString::new("-s").unwrap(),
                std::ffi::CString::new("512").unwrap(),
                std::ffi::CString::new("-p").unwrap(),
                pid_c,
            ]
        }
    };
    // Null-terminated argv pointer array for execvp.
    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        target_os = "solaris"
    ))]
    let argv: Vec<*const core::ffi::c_char> = {
        let mut v: Vec<*const core::ffi::c_char> = argv_store.iter().map(|c| c.as_ptr()).collect();
        v.push(core::ptr::null());
        v
    };

    unsafe {
        // C: if (pipe(fdpair) < 0) return false;
        if libc::pipe(fdpair.as_mut_ptr()) < 0 {
            return false;
        }

        // C: if (fcntl(fdpair[0], F_SETFL, O_NONBLOCK) < 0) goto err;
        if libc::fcntl(fdpair[0], libc::F_SETFL, libc::O_NONBLOCK) < 0 {
            libc::close(fdpair[1]);
            libc::close(fdpair[0]);
            return false;
        }
        // C: if (fcntl(fdpair[1], F_SETFL, O_NONBLOCK) < 0) goto err;
        if libc::fcntl(fdpair[1], libc::F_SETFL, libc::O_NONBLOCK) < 0 {
            libc::close(fdpair[1]);
            libc::close(fdpair[0]);
            return false;
        }

        // C: pid_t child = fork();
        let child = libc::fork();
        // C: if (child < 0) goto err;
        if child < 0 {
            libc::close(fdpair[1]);
            libc::close(fdpair[0]);
            return false;
        }

        if child == 0 {
            // C: close(fdpair[0]);
            libc::close(fdpair[0]);

            // C: dup2(fdpair[1], STDOUT_FILENO); dup2(fdpair[1], STDERR_FILENO);
            libc::dup2(fdpair[1], libc::STDOUT_FILENO);
            libc::dup2(fdpair[1], libc::STDERR_FILENO);
            // C: close(fdpair[1]);
            libc::close(fdpair[1]);

            #[cfg(any(
                target_os = "freebsd",
                target_os = "openbsd",
                target_os = "netbsd",
                target_os = "dragonfly",
                target_os = "solaris"
            ))]
            {
                libc::execvp(argv[0], argv.as_ptr());
                // Should never reach here, unless execvp fails ...
                let message: &[u8] =
                    b"Could not execute 'truss'. Please make sure it is available in your $PATH.";
                let _ = libc::write(libc::STDERR_FILENO, message.as_ptr().cast(), message.len());
            }
            #[cfg(target_os = "linux")]
            {
                libc::execvp(argv[0], argv.as_ptr());
                // Should never reach here, unless execvp fails ...
                let message: &[u8] =
                    b"Could not execute 'strace'. Please make sure it is available in your $PATH.";
                let _ = libc::write(libc::STDERR_FILENO, message.as_ptr().cast(), message.len());
            }
            #[cfg(not(any(
                target_os = "linux",
                target_os = "freebsd",
                target_os = "openbsd",
                target_os = "netbsd",
                target_os = "dragonfly",
                target_os = "solaris"
            )))]
            {
                // HTOP_DARWIN, HTOP_PCP == HTOP_UNSUPPORTED
                let message: &[u8] = b"Tracing unavailable on not supported system.";
                let _ = libc::write(libc::STDERR_FILENO, message.as_ptr().cast(), message.len());
            }

            // C: _exit(127);
            libc::_exit(127);
        }

        // C: this->child = child;
        this.child = child;

        // C: FILE* fp = fdopen(fdpair[0], "r");  — File::from_raw_fd is infallible,
        // so the `if (!fp) goto err;` arm is unreachable and omitted.
        let fp = File::from_raw_fd(fdpair[0]);

        // C: close(fdpair[1]);
        libc::close(fdpair[1]);

        // C: this->strace = fp; this->strace_alive = true; return true;
        this.strace = Some(fp);
        this.strace_alive = true;
        true
    }
}

/// Port of `static void TraceScreen_updateTrace(InfoScreen* super)` from
/// `TraceScreen.c:134` — the vtable `onErr` hook, driven each idle tick of
/// [`crate::ported::infoscreen::InfoScreen_run`].
///
/// `select`s stdin and the tracer pipe with a 500 µs timeout; when the pipe
/// is readable, `fread`s up to 1024 bytes and (while `tracing`) splits the
/// buffer on `'\n'`, routing each complete line to [`InfoScreen_addLine`] (or
/// [`InfoScreen_appendLine`] when the previous read ended mid-line), stashing
/// any trailing partial line for the next call (`contLine = true`), and — when
/// `follow` is set — scrolling to the newest row. On an empty read it polls
/// the child with `xWaitpid(WNOHANG)` and clears `strace_alive` once it exits.
///
/// Adaptations:
/// - **`fileno`/`fread` -> `File`.** `fileno(this->strace)` is
///   [`File::as_raw_fd`] (`-1` when the handle is closed, matching C's NULL
///   deref guard via `strace_alive`); `fread(buffer, 1, 1024, this->strace)`
///   is [`Read::read`] into a `[u8; 1025]`, a short/`WouldBlock` read mapping
///   to `nread == 0` (the fd is `O_NONBLOCK`).
/// - **`xWaitpid(this->child, NULL, WNOHANG, false)` inlined.** With
///   `wait_for_exit == false` (`XUtils.c:321`) it is just the `EINTR`-retry
///   `waitpid`; inlined because `XUtils.c` is unported.
/// - **`InfoScreen_appendLine` (contLine branch).** The now-ported
///   [`InfoScreen_appendLine`]`(&mut this.super_, &s)` merges the continuation
///   line `s` onto the previous partial line, mirroring the C
///   `InfoScreen_appendLine(&this->super, line)`.
///
/// [`infoscreen.rs`]: crate::ported::infoscreen
pub fn TraceScreen_updateTrace(this: &mut TraceScreen) {
    // C: int fd_strace = fileno(this->strace);
    let fd_strace = this.strace.as_ref().map_or(-1, |f| f.as_raw_fd());

    // C: fd_set fds; FD_ZERO(&fds); FD_SET(STDIN_FILENO, &fds);
    let mut fds: libc::fd_set = unsafe { core::mem::zeroed() };
    unsafe {
        libc::FD_ZERO(&mut fds);
        libc::FD_SET(libc::STDIN_FILENO, &mut fds);
    }
    // C: if (this->strace_alive) { assert(fd_strace != -1); FD_SET(fd_strace, &fds); }
    if this.strace_alive {
        debug_assert!(fd_strace != -1);
        unsafe {
            libc::FD_SET(fd_strace, &mut fds);
        }
    }

    // C: struct timeval tv = { .tv_sec = 0, .tv_usec = 500 };
    let mut tv = libc::timeval {
        tv_sec: 0,
        tv_usec: 500,
    };
    // C: int ready = select(MAXIMUM(STDIN_FILENO, fd_strace) + 1, &fds, NULL, NULL, &tv);
    let nfds = core::cmp::max(libc::STDIN_FILENO, fd_strace) + 1;
    let ready = unsafe {
        libc::select(
            nfds,
            &mut fds,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &mut tv,
        )
    };

    // C: char buffer[1025]; size_t nread = 0;
    let mut buffer = [0u8; 1025];
    let mut nread: usize = 0;
    // C: if (ready > 0 && FD_ISSET(fd_strace, &fds)) nread = fread(buffer, 1, sizeof(buffer) - 1, this->strace);
    if ready > 0 && unsafe { libc::FD_ISSET(fd_strace, &fds) } {
        if let Some(f) = this.strace.as_mut() {
            nread = f.read(&mut buffer[..1024]).unwrap_or(0);
        }
    }

    if nread != 0 && this.tracing {
        // C: const char* line = buffer; buffer[nread] = '\0';
        // (slices carry their own length; each line is buffer[line_start..i].)
        let mut line_start: usize = 0;
        // C: for (size_t i = 0; i < nread; i++) if (buffer[i] == '\n') { ... }
        let mut i = 0usize;
        while i < nread {
            if buffer[i] == b'\n' {
                // C: buffer[i] = '\0'; — line is buffer[line_start..i].
                let s = String::from_utf8_lossy(&buffer[line_start..i]).into_owned();
                if this.contLine {
                    // C: InfoScreen_appendLine(&this->super, line);
                    InfoScreen_appendLine(&mut this.super_, &s);
                    this.contLine = false;
                } else {
                    InfoScreen_addLine(&mut this.super_, &s);
                }
                // C: line = buffer + i + 1;
                line_start = i + 1;
            }
            i += 1;
        }
        // C: if (line < buffer + nread) { InfoScreen_addLine(&this->super, line); this->contLine = true; }
        if line_start < nread {
            let s = String::from_utf8_lossy(&buffer[line_start..nread]).into_owned();
            InfoScreen_addLine(&mut this.super_, &s);
            this.contLine = true;
        }
        // C: if (this->follow) Panel_setSelected(this->super.display, Panel_size(this->super.display) - 1);
        if this.follow {
            let sz = Panel_size(&this.super_.display);
            Panel_setSelected(&mut this.super_.display, sz - 1);
        }
    } else {
        // C: if (this->strace_alive && xWaitpid(this->child, NULL, WNOHANG, false) != 0)
        //        this->strace_alive = false;
        if this.strace_alive {
            // xWaitpid(..., WNOHANG, false): EINTR-retry waitpid, return its ret.
            let mut status: c_int = 0;
            let ret = loop {
                let r = unsafe { libc::waitpid(this.child, &mut status, libc::WNOHANG) };
                if r != -1 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    break r;
                }
            };
            if ret != 0 {
                this.strace_alive = false;
            }
        }
    }
}

/// Port of `static bool TraceScreen_onKey(InfoScreen* super, int ch)` from
/// `TraceScreen.c:185` — the vtable `onKey` hook.
///
/// `f`/`F8` toggle `follow` (jumping to the last row when enabling it);
/// `t`/`F9` toggle `tracing`, relabel the F9 function-bar slot, and repaint.
/// Any other key clears `follow` and reports the key unhandled (`false`).
///
/// The C `switch (ch)` takes `InfoScreen* super` then downcasts to
/// `TraceScreen*`; because `InfoScreen super` is the offset-0 base, the port
/// takes `&mut TraceScreen` and reaches the base through `super_`. The C
/// `InfoScreen_draw(this)` is the `As_InfoScreen(this)->draw(this)` vtable
/// dispatch (`InfoScreen.h:45`); on a `TraceScreen` that slot is
/// [`TraceScreen_draw`], so the dispatch resolves statically to a direct
/// `TraceScreen_draw(&mut this.super_)` call.
pub fn TraceScreen_onKey(this: &mut TraceScreen, ch: c_int) -> bool {
    // C: case 'f': case KEY_F(8):
    if ch == 'f' as c_int || ch == KEY_F(8) {
        // C: this->follow = !(this->follow);
        this.follow = !this.follow;
        // C: if (this->follow) Panel_setSelected(super->display, Panel_size(super->display) - 1);
        if this.follow {
            let sz = Panel_size(&this.super_.display);
            Panel_setSelected(&mut this.super_.display, sz - 1);
        }
        return true;
    }
    // C: case 't': case KEY_F(9):
    if ch == 't' as c_int || ch == KEY_F(9) {
        // C: this->tracing = !this->tracing;
        this.tracing = !this.tracing;
        // C: FunctionBar_setLabel(super->display->defaultBar, KEY_F(9),
        //        this->tracing ? "Stop Tracing   " : "Resume Tracing ");
        let label = if this.tracing {
            "Stop Tracing   "
        } else {
            "Resume Tracing "
        };
        if let Some(bar) = this.super_.display.defaultBar.as_mut() {
            FunctionBar_setLabel(bar, KEY_F(9), label);
        }
        // C: InfoScreen_draw(this); — vtable draw slot == TraceScreen_draw.
        TraceScreen_draw(&mut this.super_);
        return true;
    }

    // C: this->follow = false; return false;
    this.follow = false;
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::functionbar::Ncurses;
    use crate::ported::incset::IncSet_filter;
    use crate::ported::panel::{Panel_headerHeight, Panel_size};
    use crate::ported::process::{Process, Process_setPid};
    use crate::ported::vector::Vector_size;

    #[test]
    fn new_sets_tracer_defaults() {
        let mut p = Process::default();
        Process_setPid(&mut p, 1234);
        let ts = TraceScreen_new(&p);
        // C: this->tracing = true; this->strace_alive = false; rest zeroed.
        assert!(ts.tracing);
        assert!(!ts.strace_alive);
        assert!(!ts.contLine);
        assert!(!ts.follow);
        assert_eq!(ts.child, 0);
        assert!(ts.strace.is_none());
    }

    #[test]
    fn new_initializes_the_embedded_infoscreen() {
        let mut p = Process::default();
        Process_setPid(&mut p, 7);
        let ts = TraceScreen_new(&p);
        // super was fully overwritten by InfoScreen_init:
        // - process back-pointer stored (points at the passed Process).
        assert_eq!(ts.super_.process, &p as *const Process);
        // - lines and panel start empty.
        assert_eq!(Vector_size(&ts.super_.lines), 0);
        assert_eq!(Panel_size(&ts.super_.display), 0);
        // - panel geometry: Panel_new(0, 1, COLS, LINES - 2, ...).
        assert_eq!(ts.super_.display.x, 0);
        assert_eq!(ts.super_.display.y, 1);
        assert_eq!(ts.super_.display.w, Ncurses::cols());
        assert_eq!(ts.super_.display.h, Ncurses::lines() - 2);
        // - the " " header installed -> headerHeight 1.
        assert_eq!(Panel_headerHeight(&ts.super_.display), 1);
        // - no filter active on a fresh IncSet.
        assert!(IncSet_filter(&ts.super_.inc).is_none());
    }

    #[test]
    fn new_builds_the_tracescreen_function_bar() {
        let mut p = Process::default();
        Process_setPid(&mut p, 9);
        let ts = TraceScreen_new(&p);
        // C hands a TraceScreen-specific fuBar to InfoScreen_init.
        let bar = ts
            .super_
            .display
            .defaultBar
            .as_ref()
            .expect("default bar built");
        assert_eq!(bar.functions, TraceScreenFunctions.to_vec());
        assert_eq!(bar.keys, TraceScreenKeys.to_vec());
        assert_eq!(bar.events, TraceScreenEvents.to_vec());
    }

    #[test]
    fn tracescreen_events_match_keys() {
        // Port of {KEY_F(3), KEY_F(4), KEY_F(8), KEY_F(9), 27}.
        assert_eq!(
            TraceScreenEvents,
            [KEY_F(3), KEY_F(4), KEY_F(8), KEY_F(9), 27]
        );
    }

    #[test]
    fn delete_without_child_does_not_kill_or_panic() {
        let mut p = Process::default();
        Process_setPid(&mut p, 55);
        let mut ts = TraceScreen_new(&p);
        // No tracer was forked: child == 0, strace == None.
        TraceScreen_delete(&mut ts);
        // Teardown left the fields consistent (no kill attempted).
        assert_eq!(ts.child, 0);
        assert!(ts.strace.is_none());
    }

    #[test]
    fn onkey_f8_toggles_follow_and_consumes_key() {
        let mut p = Process::default();
        Process_setPid(&mut p, 101);
        let mut ts = TraceScreen_new(&p);
        assert!(!ts.follow);
        // C: case 'f': case KEY_F(8): this->follow = !this->follow; return true;
        assert!(TraceScreen_onKey(&mut ts, KEY_F(8)));
        assert!(ts.follow);
        // 'f' toggles it back off.
        assert!(TraceScreen_onKey(&mut ts, 'f' as c_int));
        assert!(!ts.follow);
    }

    #[test]
    fn onkey_unhandled_clears_follow_and_returns_false() {
        let mut p = Process::default();
        Process_setPid(&mut p, 202);
        let mut ts = TraceScreen_new(&p);
        // Turn follow on via F8, then press an unrelated key.
        assert!(TraceScreen_onKey(&mut ts, KEY_F(8)));
        assert!(ts.follow);
        // C: default -> this->follow = false; return false;
        assert!(!TraceScreen_onKey(&mut ts, 'z' as c_int));
        assert!(!ts.follow);
    }

    #[test]
    fn delete_closes_the_strace_handle() {
        let mut p = Process::default();
        Process_setPid(&mut p, 77);
        let mut ts = TraceScreen_new(&p);
        // Simulate an open tracer pipe handle (no fork): fclose == drop.
        ts.strace = Some(File::open("/dev/null").expect("/dev/null opens"));
        ts.child = 0; // no child to reap
        TraceScreen_delete(&mut ts);
        assert!(ts.strace.is_none());
    }
}
