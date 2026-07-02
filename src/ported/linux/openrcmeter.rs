//! Partial port of `linux/OpenRCMeter.c` — htop's OpenRC init-system state
//! meter (system and per-user variants).
//!
//! C names are preserved verbatim (`CamelCase_snake`), so `non_snake_case`
//! is allowed module-wide. The C keeps two file-scope caches, `static
//! OpenRCMeterContext_t ctx_system` / `ctx_user`, shared between the
//! `updateValues` (writer) and `display` (reader) vtable slots. Those
//! globals are reproduced as [`Mutex`]-wrapped [`OpenRCMeterContext`]
//! statics ([`CTX_SYSTEM`] / [`CTX_USER`]); the meters run single-threaded,
//! but the `Mutex` gives the interior mutability the C `static` had for
//! free. `#define INVALID_VALUE ((size_t)-1)` is [`INVALID_VALUE`] =
//! `usize::MAX`.
//!
//! `CRT_colors[X]` is `ColorElements::X.packed(ColorScheme::active())` and
//! `RichString_writeAscii`/`RichString_appendAscii` take `&[u8]`, matching
//! `filedescriptormeter.rs`. The `ATTR_UNUSED const Object* cast` display
//! params collapse away (the down-cast the C never performs), so the
//! display fns take the typed `RichString*`/context directly.
//!
//! The file-scope helper `OpenRCMeter_execRcStatus` (`OpenRCMeter.c:44`) is
//! not in the port-purity allowlist (only the combined `updateViaExec`
//! name is), so it is inlined as the `exec_rc_status` closure inside
//! [`updateViaExec`] — the same fork/pipe/dup2/`/dev/null`/exec pattern
//! `openfilesscreen.rs` uses, driven with `libc` directly. `execlp` becomes
//! `libc::execvp` (both search `PATH`); `xWaitpid(child, …, 0, false)` is a
//! plain `waitpid` that retries only on `EINTR` (the `xwaitpid` closure);
//! `free_and_xStrdup(&ctx->runlevel, buf)` is `ctx.runlevel = Some(buf)`.
//!
//! Ported (dependencies present):
//! - [`updateViaExec`] (`OpenRCMeter.c:96`, C `OpenRCMeter_updateViaExec`)
//! - [`OpenRCMeter_display`] (`OpenRCMeter.c:188`)
//! - [`OpenRCMeter_display_system`] (`OpenRCMeter.c:219`)
//! - [`OpenRCMeter_display_user`] (`OpenRCMeter.c:223`)
//!
//! Stubbed (blocked — see each fn's doc): [`OpenRCMeter_done`] and
//! [`OpenRCMeter_updateValues`] both need `Meter_name(this)` (`Meter.h:101`,
//! `As_Meter(this)->name`) to choose `ctx_user` vs `ctx_system`, but the
//! ported `Meter` instance carries no class-`name` field and `Meter_name`
//! is not ported anywhere in the crate, so there is no faithful way to
//! read the class name off a `&Meter`.
#![allow(non_snake_case)]
#![allow(dead_code)]

use core::ffi::{c_char, c_int};
use std::ffi::CString;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::unix::io::FromRawFd;
use std::sync::Mutex;

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::richstring::{RichString, RichString_appendAscii, RichString_writeAscii};
use crate::ported::settings::Settings_isReadonly;

/// Port of `#define INVALID_VALUE ((size_t)-1)` from `OpenRCMeter.c:26`.
const INVALID_VALUE: usize = usize::MAX;

/// Port of `typedef struct OpenRCMeterContext` from `OpenRCMeter.c:28` — the
/// file-scope cache shared between `updateValues` and `display`. C's
/// `char* runlevel` is `Option<String>`.
pub struct OpenRCMeterContext {
    runlevel: Option<String>,
    services_stopped: usize,
    services_started: usize,
}

/// Port of `static OpenRCMeterContext_t ctx_system` from `OpenRCMeter.c:34`.
/// A C `static` is zero-initialized, so `runlevel == NULL` and the counts
/// are `0` (not `INVALID_VALUE`) until the first `updateValues`.
static CTX_SYSTEM: Mutex<OpenRCMeterContext> = Mutex::new(OpenRCMeterContext {
    runlevel: None,
    services_stopped: 0,
    services_started: 0,
});

/// Port of `static OpenRCMeterContext_t ctx_user` from `OpenRCMeter.c:35`.
static CTX_USER: Mutex<OpenRCMeterContext> = Mutex::new(OpenRCMeterContext {
    runlevel: None,
    services_stopped: 0,
    services_started: 0,
});

/// TODO: port of `static void OpenRCMeter_done(ATTR_UNUSED Meter* this)`
/// from `OpenRCMeter.c:38`. Blocked: the body selects `ctx_user` vs
/// `ctx_system` with `String_eq(Meter_name(this), "OpenRCUser")`, but
/// `Meter_name` (`Meter.h:101`, `As_Meter(this)->name`) is not ported and
/// the ported `Meter` instance carries no class-`name` field, so a `&Meter`
/// cannot resolve which context to clear. (It is also not a plain
/// `Drop`-covered teardown: it frees a module-global cache, not the meter.)
pub fn OpenRCMeter_done() {
    todo!("port of OpenRCMeter.c:38: needs Meter_name (Meter.h:101) — Meter instance has no class name field")
}

/// Port of `static void OpenRCMeter_updateViaExec(bool user)` from
/// `OpenRCMeter.c:96`. Marks both service counts `INVALID_VALUE`, then
/// (unless the settings are read-only) forks `rc-status` twice: first
/// `rc-status -C [--user] -r` to read the current runlevel (one line), then
/// `rc-status -C [--user] -f ini -a` to tally `started`/`stopped` services.
/// A non-zero exit or a `waitpid` failure leaves the counts `INVALID_VALUE`.
/// The inlined `exec_rc_status` closure is the C static
/// `OpenRCMeter_execRcStatus` (`OpenRCMeter.c:44`): it bails for the user
/// variant when `XDG_RUNTIME_DIR` is unset/empty, builds a `pipe`, forks,
/// and in the child redirects stdout to the pipe and stderr to `/dev/null`
/// before `execvp`-ing `rc-status`.
pub fn updateViaExec(user: bool) {
    let ctx_mutex = if user { &CTX_USER } else { &CTX_SYSTEM };
    let mut ctx = ctx_mutex.lock().unwrap();

    ctx.services_started = INVALID_VALUE;
    ctx.services_stopped = INVALID_VALUE;

    // C: if (Settings_isReadonly()) return;
    if Settings_isReadonly() {
        return;
    }

    // C static OpenRCMeter_execRcStatus(user, full, &childPid) -> fd; returns
    // (-1, -1) on failure. argv is built before the fork (async-signal-safety
    // — the child does no allocation, only libc calls, then execs).
    let exec_rc_status = |user: bool, full: bool| -> (c_int, libc::pid_t) {
        // C: if (user) { const char* xdg = getenv("XDG_RUNTIME_DIR");
        //                if (!xdg || !*xdg) return -1; }
        if user {
            match std::env::var_os("XDG_RUNTIME_DIR") {
                Some(v) if !v.is_empty() => {}
                _ => return (-1, -1),
            }
        }

        // C: int fdpair[2] = {-1, -1}; if (pipe(fdpair) < 0) return -1;
        let mut fdpair: [c_int; 2] = [-1, -1];
        if unsafe { libc::pipe(fdpair.as_mut_ptr()) } < 0 {
            return (-1, -1);
        }

        // C: execlp("rc-status", "rc-status", "-C", [...], (char*)NULL);
        let args: &[&str] = match (user, full) {
            (true, true) => &["rc-status", "-C", "--user", "-f", "ini", "-a"],
            (true, false) => &["rc-status", "-C", "--user", "-r"],
            (false, true) => &["rc-status", "-C", "-f", "ini", "-a"],
            (false, false) => &["rc-status", "-C", "-r"],
        };
        let cargs: Vec<CString> = args
            .iter()
            .map(|s| CString::new(*s).expect("no interior NUL"))
            .collect();
        let mut argv: Vec<*const c_char> = cargs.iter().map(|c| c.as_ptr()).collect();
        argv.push(core::ptr::null());

        // C: pid_t child = fork();
        let child = unsafe { libc::fork() };
        if child < 0 {
            // C: close(fdpair[1]); close(fdpair[0]); return -1;
            unsafe {
                libc::close(fdpair[1]);
                libc::close(fdpair[0]);
            }
            return (-1, -1);
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
                libc::execvp(cargs[0].as_ptr(), argv.as_ptr());
                // C: _exit(127);  (only reached if exec failed — rc-status not found)
                libc::_exit(127);
            }
        }

        // Parent. C: close(fdpair[1]); return fdpair[0];
        unsafe {
            libc::close(fdpair[1]);
        }
        (fdpair[0], child)
    };

    // C: xWaitpid(child, &wstatus, 0, false) — a plain waitpid that retries
    // only on EINTR (XUtils.c). Returns None on error, else the wstatus.
    let xwaitpid = |child: libc::pid_t| -> Option<c_int> {
        let mut wstatus: c_int = 0;
        let ret = loop {
            let r = unsafe { libc::waitpid(child, &mut wstatus, 0) };
            if r == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break r;
        };
        if ret < 0 {
            None
        } else {
            Some(wstatus)
        }
    };

    // ── First exec: the runlevel line ────────────────────────────────────
    let (fd, child) = exec_rc_status(user, false);
    if fd < 0 {
        return;
    }
    {
        // C: FILE* commandOutput = fdopen(fd, "r"); if (fgets(lineBuffer, ...)) {...}
        let file = unsafe { File::from_raw_fd(fd) };
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) > 0 {
            // C: char* newline = strchr(lineBuffer, '\n'); if (newline) *newline = '\0';
            if line.ends_with('\n') {
                line.pop();
            }
            // C: free_and_xStrdup(&ctx->runlevel, lineBuffer);
            ctx.runlevel = Some(line);
        }
        // Dropping `reader` closes the fd — the C fclose(commandOutput).
    }

    // C: if (xWaitpid(...) < 0 || !WIFEXITED || WEXITSTATUS != 0) return;
    match xwaitpid(child) {
        Some(ws) if libc::WIFEXITED(ws) && libc::WEXITSTATUS(ws) == 0 => {}
        _ => return,
    }

    // ── Second exec: tally started/stopped services ──────────────────────
    let (fd, child) = exec_rc_status(user, true);
    if fd < 0 {
        return;
    }

    // C: ctx->services_started = 0; ctx->services_stopped = 0;
    ctx.services_started = 0;
    ctx.services_stopped = 0;

    {
        let file = unsafe { File::from_raw_fd(fd) };
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            // C: char* equals = strchr(lineBuffer, '='); if (!equals) continue;
            let eq = match line.find('=') {
                Some(i) => i,
                None => continue,
            };
            // C: char* status = equals + 1; while (*status==' '||*status=='\t') status++;
            let mut status = &line[eq + 1..];
            status = status.trim_start_matches([' ', '\t']);
            // C: char* newline = strchr(status, '\n'); if (newline) *newline = '\0';
            let status = match status.find('\n') {
                Some(i) => &status[..i],
                None => status,
            };
            // C: if (strstr(status, "started")) started++; else if (strstr(status, "stopped")) stopped++;
            if status.contains("started") {
                ctx.services_started += 1;
            } else if status.contains("stopped") {
                ctx.services_stopped += 1;
            }
        }
        // Dropping `reader` closes the fd — the C fclose(commandOutput).
    }

    // C: if (xWaitpid(...) < 0 || !WIFEXITED || WEXITSTATUS != 0) {
    //        ctx->services_started = INVALID_VALUE; ctx->services_stopped = INVALID_VALUE; }
    match xwaitpid(child) {
        Some(ws) if libc::WIFEXITED(ws) && libc::WEXITSTATUS(ws) == 0 => {}
        _ => {
            ctx.services_started = INVALID_VALUE;
            ctx.services_stopped = INVALID_VALUE;
        }
    }
}

/// TODO: port of `static void OpenRCMeter_updateValues(Meter* this)` from
/// `OpenRCMeter.c:115`. Blocked: the first line is `bool user =
/// String_eq(Meter_name(this), "OpenRCUser")` to pick the context and the
/// meter's variant, but `Meter_name` (`Meter.h:101`, `As_Meter(this)->name`)
/// is not ported and the ported `Meter` instance stores no class-`name`
/// field, so `user` cannot be resolved faithfully. Once `Meter_name` lands,
/// the rest is mechanical: clear the chosen context, call
/// [`updateViaExec`]`(user)`, and copy `ctx.runlevel` (or `"???"`) into
/// `this.txtBuffer`.
pub fn OpenRCMeter_updateValues() {
    todo!("port of OpenRCMeter.c:115: needs Meter_name (Meter.h:101) — Meter instance has no class name field")
}

/// Port of `static void OpenRCMeter_display(ATTR_UNUSED const Object* cast,
/// RichString* out, OpenRCMeterContext_t* ctx)` from `OpenRCMeter.c:188`.
/// Writes `Runlevel: ` (`METER_TEXT`) then the runlevel or `N/A`
/// (`METER_VALUE`). If both service counts are `INVALID_VALUE`, stops there;
/// otherwise appends ` (`, the started count (or `?`, `METER_VALUE_OK`),
/// ` started, `, the stopped count (or `?`, `METER_VALUE_ERROR`), and
/// ` stopped)`. The `ATTR_UNUSED cast` param collapses away.
pub fn OpenRCMeter_display(out: &mut RichString, ctx: &OpenRCMeterContext) {
    let scheme = ColorScheme::active();

    RichString_writeAscii(out, ColorElements::METER_TEXT.packed(scheme), b"Runlevel: ");
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE.packed(scheme),
        ctx.runlevel.as_deref().unwrap_or("N/A").as_bytes(),
    );

    // C: if (services_started == INVALID && services_stopped == INVALID) return;
    if ctx.services_started == INVALID_VALUE && ctx.services_stopped == INVALID_VALUE {
        return;
    }

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" (");

    let buffer = if ctx.services_started == INVALID_VALUE {
        "?".to_string()
    } else {
        ctx.services_started.to_string()
    };
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE_OK.packed(scheme),
        buffer.as_bytes(),
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" started, ");

    let buffer = if ctx.services_stopped == INVALID_VALUE {
        "?".to_string()
    } else {
        ctx.services_stopped.to_string()
    };
    RichString_appendAscii(
        out,
        ColorElements::METER_VALUE_ERROR.packed(scheme),
        buffer.as_bytes(),
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" stopped)");
}

/// Port of `static void OpenRCMeter_display_system(ATTR_UNUSED const Object*
/// cast, RichString* out)` from `OpenRCMeter.c:219` — renders the system
/// context (`&ctx_system`).
pub fn OpenRCMeter_display_system(out: &mut RichString) {
    let ctx = CTX_SYSTEM.lock().unwrap();
    OpenRCMeter_display(out, &ctx);
}

/// Port of `static void OpenRCMeter_display_user(ATTR_UNUSED const Object*
/// cast, RichString* out)` from `OpenRCMeter.c:223` — renders the user
/// context (`&ctx_user`).
pub fn OpenRCMeter_display_user(out: &mut RichString) {
    let ctx = CTX_USER.lock().unwrap();
    OpenRCMeter_display(out, &ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Visible characters of the valid `[0, chlen)` range of `out`.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    fn ctx(runlevel: Option<&str>, started: usize, stopped: usize) -> OpenRCMeterContext {
        OpenRCMeterContext {
            runlevel: runlevel.map(|s| s.to_string()),
            services_started: started,
            services_stopped: stopped,
        }
    }

    #[test]
    fn display_runlevel_only_when_counts_invalid() {
        let c = ctx(Some("default"), INVALID_VALUE, INVALID_VALUE);
        let mut out = RichString::new();
        OpenRCMeter_display(&mut out, &c);
        assert_eq!(text(&out), "Runlevel: default");
    }

    #[test]
    fn display_na_when_no_runlevel() {
        let c = ctx(None, INVALID_VALUE, INVALID_VALUE);
        let mut out = RichString::new();
        OpenRCMeter_display(&mut out, &c);
        assert_eq!(text(&out), "Runlevel: N/A");
    }

    #[test]
    fn display_with_counts() {
        let c = ctx(Some("default"), 3, 1);
        let mut out = RichString::new();
        OpenRCMeter_display(&mut out, &c);
        assert_eq!(text(&out), "Runlevel: default (3 started, 1 stopped)");
    }

    #[test]
    fn display_question_mark_for_single_invalid_count() {
        // Only one count invalid — the block is still entered.
        let c = ctx(Some("default"), INVALID_VALUE, 2);
        let mut out = RichString::new();
        OpenRCMeter_display(&mut out, &c);
        assert_eq!(text(&out), "Runlevel: default (? started, 2 stopped)");
    }
}
