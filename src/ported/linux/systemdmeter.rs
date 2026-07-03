//! Partial port of `linux/SystemdMeter.c` — htop's systemd system/user
//! state meters.
//!
//! C names are preserved verbatim (`CamelCase_snake`), so `non_snake_case`
//! is allowed module-wide — matching the spec name-for-name is the point.
//!
//! The two file-scope caches `ctx_system` / `ctx_user`
//! (`SystemdMeter.c:68`-`69`) are the shared blocks the display renderers
//! read; they are modeled as `SystemdMeterContext_t` behind a `Mutex`
//! (Rust module-private mutable statics need interior mutability — the C
//! statics are single-threaded and unlocked), the same shape
//! `diskiometer.rs` uses for its file-scope static block. The C struct's
//! `sd_bus* bus` field (`SystemdMeter.c:59`, guarded on
//! `!BUILD_STATIC || HAVE_LIBSYSTEMD`) is *not* modeled: it is touched only
//! by `updateViaLib` and `SystemdMeter_done`, both of which stay stubbed
//! (the dlopen'd libsystemd sd-bus FFI has no Rust counterpart), so there is
//! nothing that reads it.
//!
//! `CRT_colors[X]` is reproduced as `ColorElements::X.packed(scheme)`
//! (`CRT_colorSchemes[CRT_colorScheme][X]`), the mapping the other ported
//! meter renderers use. `xSnprintf(buffer, ..., "%u", v)` becomes
//! `format!("{v}")`; the returned `len` is the string's byte length.
//!
//! Ported (self-contained — `RichString` + `CRT_colors` + `String_eq` are
//! ported and the ctx cache is modeled here):
//! - [`updateViaExec`] (`SystemdMeter.c:214`) — the `systemctl show` spawn +
//!   parse (inlined `libc` fork/exec/pipe/waitpid, the `openfilesscreen.rs`
//!   precedent; takes `user: bool`, so needs no `Meter_name`)
//! - [`zeroDigitColor`] (`SystemdMeter.c:318`)
//! - [`valueDigitColor`] (`SystemdMeter.c:329`)
//! - `SystemdMeter_display` (`SystemdMeter.c:341`)
//! - [`SystemdMeter_display_system`] (`SystemdMeter.c:399`)
//! - [`SystemdMeter_display_user`] (`SystemdMeter.c:403`)
//!
//! Stubbed (blocked on unported substrate — each keeps its `todo!()`; see
//! the per-fn docs):
//! - `SystemdMeter_done` (`SystemdMeter.c:71`)
//! - `updateViaLib` (`SystemdMeter.c:98`)
//! - `SystemdMeter_updateValues` (`SystemdMeter.c:300`)
#![allow(non_snake_case)]
// `SystemdMeterContext_t` mirrors the C typedef name verbatim (SystemdMeter.c:57).
#![allow(non_camel_case_types)]
// `ctx_system` / `ctx_user` mirror the C file-scope static names (SystemdMeter.c:68-69).
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::ffi::{c_char, c_int};
use std::ffi::CString;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::unix::io::FromRawFd;
use std::sync::Mutex;

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendnAscii, RichString_writeAscii,
};
use crate::ported::settings::Settings_isReadonly;
use crate::ported::xutils::{String_eq, String_startsWith};

/// Port of `#define INVALID_VALUE ((unsigned int)-1)` from
/// `SystemdMeter.c:55` — the sentinel a count field holds when the
/// property could not be read.
const INVALID_VALUE: u32 = u32::MAX;

/// Port of `typedef struct SystemdMeterContext` from `SystemdMeter.c:57`.
/// The C `sd_bus* bus` field (`:59`) is omitted: it is used only by the
/// stubbed `updateViaLib` / `SystemdMeter_done` (the dlopen'd libsystemd
/// sd-bus FFI is not ported), so nothing reads it. `char* systemState`
/// (`:61`) becomes `Option<String>` (NULL ⇒ `None`); the `unsigned int`
/// counters map to `u32`.
struct SystemdMeterContext_t {
    /// C `char* systemState` (`SystemdMeter.c:61`).
    systemState: Option<String>,
    /// C `unsigned int nFailedUnits` (`SystemdMeter.c:62`).
    nFailedUnits: u32,
    /// C `unsigned int nInstalledJobs` (`SystemdMeter.c:63`).
    nInstalledJobs: u32,
    /// C `unsigned int nNames` (`SystemdMeter.c:64`).
    nNames: u32,
    /// C `unsigned int nJobs` (`SystemdMeter.c:65`).
    nJobs: u32,
}

impl SystemdMeterContext_t {
    /// Zero-initialized cache, matching the C file-scope statics
    /// (`systemState == NULL`, all counters `0`). `const` so it can seed a
    /// `Mutex` static initializer.
    const fn new() -> Self {
        SystemdMeterContext_t {
            systemState: None,
            nFailedUnits: 0,
            nInstalledJobs: 0,
            nNames: 0,
            nJobs: 0,
        }
    }
}

/// Port of `static SystemdMeterContext_t ctx_system` from
/// `SystemdMeter.c:68`. Behind a `Mutex` for interior mutability (see the
/// module docs).
static ctx_system: Mutex<SystemdMeterContext_t> = Mutex::new(SystemdMeterContext_t::new());

/// Port of `static SystemdMeterContext_t ctx_user` from
/// `SystemdMeter.c:69`.
static ctx_user: Mutex<SystemdMeterContext_t> = Mutex::new(SystemdMeterContext_t::new());

/// TODO: port of `static void SystemdMeter_done(ATTR_UNUSED Meter* this)`
/// from `SystemdMeter.c:71`. Blocked on two fronts: (1) it selects
/// `&ctx_user` vs `&ctx_system` via `String_eq(Meter_name(this), ...)`, but
/// the partial `Meter` in `meter.rs` carries no `name` field — `Meter_name`
/// (`Meter.h:101`, `As_Meter(this)->name`) has no instance target to read;
/// (2) the body is a `.done` free-teardown that unrefs `ctx->bus` /
/// `dlclose`s the libsystemd handle, and the dlopen'd sd-bus path is not
/// ported (the `bus` field is not even modeled). Freeing `ctx->systemState`
/// is handled by `Drop` on the ctx cache. Kept stubbed per the teardown
/// rule and the `Meter_name` blocker.
pub fn SystemdMeter_done() {
    todo!("port of SystemdMeter.c:71: needs Meter.name (Meter_name) + sd_bus/dlopen teardown")
}

/// TODO: port of `static int updateViaLib(bool user)` from
/// `SystemdMeter.c:98`. Blocked: the entire body is the dlopen'd libsystemd
/// sd-bus client — `dlopen("libsystemd.so.0")` + `dlsym` symbol resolution
/// of `sd_bus_open_system` / `sd_bus_open_user` /
/// `sd_bus_get_property_string` / `sd_bus_get_property_trivial` /
/// `sd_bus_unref`, then D-Bus property reads off `org.freedesktop.systemd1`.
/// No libsystemd FFI binding is ported anywhere in the crate, so there is no
/// faithful call target; reproducing it would be an adhoc reimplementation.
pub fn updateViaLib() {
    todo!("port of SystemdMeter.c:98: needs libsystemd sd-bus FFI (dlopen/dlsym)")
}

/// Port of `static void updateViaExec(bool user)` from `SystemdMeter.c:214`.
/// Shells out to `systemctl show --{system,user} --property=…` and parses
/// its `Key=value` output into the selected `ctx_*` cache. `user` picks
/// `ctx_user` vs `ctx_system` (the C ternary), so no `Meter_name` is needed.
///
/// The `pipe`/`fork`/`dup2`/`execlp` + reap pipeline is reproduced with raw
/// `libc` syscalls, exactly as `OpenFilesScreen_getProcessData`
/// (`openfilesscreen.rs`) ports the sibling `lsof` pipeline: the child path
/// stays async-signal-safe (the `argv` `CString`s are built before the
/// `fork`), `execlp` becomes `execvp` (both do a `$PATH` lookup) over an
/// explicit `argv`, and the `xWaitpid(child, &wstatus, 0, false)` (options
/// `0`, retry only on `EINTR` — `XUtils.c:321`) is an inlined `waitpid`
/// loop. `free_and_xStrdup(&ctx->systemState, …)` maps to assigning
/// `Option<String>` (the openfilesscreen precedent).
///
/// Faithful quirk: like the C, `fgets` (here `read_line`) keeps the trailing
/// `'\n'`. The `SystemState=` branch strips it (the C `strchr('\n')`), but
/// the numeric branches copy the C verbatim — `strtoul` is emulated to yield
/// the value and the byte at `endptr`, and the field is stored only when
/// `value <= UINT_MAX && *endptr == '\0'`. Because the retained newline
/// leaves `*endptr == '\n'`, the numeric fields are stored only from a
/// final line lacking a newline, mirroring the C's exact behavior (not
/// "fixed"). The fixed `char lineBuffer[128]` is a C-string-buffer artifact,
/// so the owned `String` read is not length-capped (the `history.rs` /
/// `openfilesscreen.rs` treatment of C's fixed `fgets` buffers).
pub fn updateViaExec(user: bool) {
    // C: SystemdMeterContext_t* ctx = user ? &ctx_user : &ctx_system;
    let ctx_mutex = if user { &ctx_user } else { &ctx_system };

    // C: if (Settings_isReadonly()) return;
    if Settings_isReadonly() {
        return;
    }

    // C: int fdpair[2] = {-1, -1}; if (pipe(fdpair) < 0) return;
    let mut fdpair: [c_int; 2] = [-1, -1];
    if unsafe { libc::pipe(fdpair.as_mut_ptr()) } < 0 {
        return;
    }

    // Build the execvp argv before the fork so the child does no allocation
    // (async-signal-safety). C: execlp("systemctl", "systemctl", "show",
    // user ? "--user" : "--system", "--property=SystemState", …, (char*)NULL).
    let c_systemctl = CString::new("systemctl").expect("no interior NUL");
    let c_show = CString::new("show").expect("no interior NUL");
    let c_scope = CString::new(if user { "--user" } else { "--system" }).expect("no interior NUL");
    let c_p_state = CString::new("--property=SystemState").expect("no interior NUL");
    let c_p_failed = CString::new("--property=NFailedUnits").expect("no interior NUL");
    let c_p_names = CString::new("--property=NNames").expect("no interior NUL");
    let c_p_jobs = CString::new("--property=NJobs").expect("no interior NUL");
    let c_p_installed = CString::new("--property=NInstalledJobs").expect("no interior NUL");
    let argv: [*const c_char; 9] = [
        c_systemctl.as_ptr(),
        c_show.as_ptr(),
        c_scope.as_ptr(),
        c_p_state.as_ptr(),
        c_p_failed.as_ptr(),
        c_p_names.as_ptr(),
        c_p_jobs.as_ptr(),
        c_p_installed.as_ptr(),
        core::ptr::null(),
    ];

    // C: pid_t child = fork();
    let child = unsafe { libc::fork() };
    if child < 0 {
        // C: close(fdpair[1]); close(fdpair[0]); return;
        unsafe {
            libc::close(fdpair[1]);
            libc::close(fdpair[0]);
        }
        return;
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
            libc::execvp(c_systemctl.as_ptr(), argv.as_ptr());
            // C: _exit(127);  (only reached if execvp failed — systemctl not found)
            libc::_exit(127);
        }
    }

    // Parent. C: close(fdpair[1]);
    unsafe {
        libc::close(fdpair[1]);
    }

    // C: int wstatus; if (xWaitpid(child, &wstatus, 0, false) < 0
    //        || !WIFEXITED(wstatus) || WEXITSTATUS(wstatus) != 0) { close; return; }
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
    if ret < 0 || !libc::WIFEXITED(wstatus) || libc::WEXITSTATUS(wstatus) != 0 {
        unsafe {
            libc::close(fdpair[0]);
        }
        return;
    }

    // C: FILE* commandOutput = fdopen(fdpair[0], "r"); if (!commandOutput) {...}
    // The BufReader owns fdpair[0]; dropping it at the end is the C fclose.
    // (File::from_raw_fd is infallible, so the C NULL check is unreachable.)
    let file = unsafe { File::from_raw_fd(fdpair[0]) };
    let mut reader = BufReader::new(file);

    // strtoul emulation (base 10): reproduces C's parse so the `*endptr`
    // check is faithful. Returns the parsed value (saturating, matching
    // strtoul's ULONG_MAX clamp for our small inputs) and the byte the C
    // `endptr` would point at — a NUL-terminated char* read models
    // index >= len as 0 (rule 4).
    let strtoul = |s: &str| -> (u64, u8) {
        let bytes = s.as_bytes();
        let mut i = 0usize;
        // C strtoul skips leading isspace().
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
            i += 1;
        }
        // C strtoul accepts an optional sign.
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let mut val: u64 = 0;
        let mut any = false;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            any = true;
            val = val
                .saturating_mul(10)
                .saturating_add((bytes[i] - b'0') as u64);
            i += 1;
        }
        // On a successful conversion endptr is the first unparsed byte; with
        // no digits C leaves endptr == nptr (the original start).
        let endbyte = if any {
            bytes.get(i).copied().unwrap_or(0)
        } else {
            bytes.first().copied().unwrap_or(0)
        };
        (val, endbyte)
    };

    let mut ctx = ctx_mutex.lock().unwrap();

    // C: char lineBuffer[128]; while (fgets(lineBuffer, sizeof, commandOutput)) { … }
    // read_line keeps the trailing '\n', matching fgets.
    let mut lineBuffer = String::new();
    loop {
        lineBuffer.clear();
        match reader.read_line(&mut lineBuffer) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }

        if String_startsWith(&lineBuffer, "SystemState=") {
            // C: char* newline = strchr(…, '\n'); if (newline) *newline = '\0';
            //    free_and_xStrdup(&ctx->systemState, lineBuffer + strlen("SystemState="));
            let rest = &lineBuffer["SystemState=".len()..];
            let rest = match rest.find('\n') {
                Some(p) => &rest[..p],
                None => rest,
            };
            ctx.systemState = Some(rest.to_string());
        } else if String_startsWith(&lineBuffer, "NFailedUnits=") {
            let (value, endbyte) = strtoul(&lineBuffer["NFailedUnits=".len()..]);
            if value <= u32::MAX as u64 && endbyte == 0 {
                ctx.nFailedUnits = value as u32;
            }
        } else if String_startsWith(&lineBuffer, "NNames=") {
            let (value, endbyte) = strtoul(&lineBuffer["NNames=".len()..]);
            if value <= u32::MAX as u64 && endbyte == 0 {
                ctx.nNames = value as u32;
            }
        } else if String_startsWith(&lineBuffer, "NJobs=") {
            let (value, endbyte) = strtoul(&lineBuffer["NJobs=".len()..]);
            if value <= u32::MAX as u64 && endbyte == 0 {
                ctx.nJobs = value as u32;
            }
        } else if String_startsWith(&lineBuffer, "NInstalledJobs=") {
            let (value, endbyte) = strtoul(&lineBuffer["NInstalledJobs=".len()..]);
            if value <= u32::MAX as u64 && endbyte == 0 {
                ctx.nInstalledJobs = value as u32;
            }
        }
    }
    // C: fclose(commandOutput);  (BufReader/File Drop here.)
}

/// TODO: port of `static void SystemdMeter_updateValues(Meter* this)` from
/// `SystemdMeter.c:300`. Blocked on `Meter_name`: it selects `&ctx_user` vs
/// `&ctx_system` via `String_eq(Meter_name(this), "SystemdUser")` — the
/// concrete class name (`As_Meter(this)->name`, `Meter.h:101`) — but the
/// ported `Meter` (`meter.rs`) carries no per-instance klass pointer (its
/// `klass()` always returns the base `Meter_class`), so `user` cannot be
/// derived from a `&Meter`. It also drives the refresh through `updateViaLib`
/// (still stubbed — libsystemd sd-bus FFI) with an `updateViaExec` fallback
/// (now ported), and writes `this->txtBuffer` (modeled) from
/// `ctx->systemState`; but the `Meter_name` blocker prevents a faithful port.
pub fn SystemdMeter_updateValues() {
    todo!("port of SystemdMeter.c:300: needs Meter.name (Meter_name) + updateViaLib")
}

/// Port of `static int zeroDigitColor(unsigned int value)` from
/// `SystemdMeter.c:318`. Colors a count whose *good* value is zero:
/// `0` ⇒ `METER_VALUE`, `INVALID_VALUE` ⇒ `METER_VALUE_ERROR`, otherwise
/// `METER_VALUE_NOTICE`. `CRT_colors[X]` is `ColorElements::X.packed`.
pub fn zeroDigitColor(value: u32) -> i32 {
    let scheme = ColorScheme::active();
    match value {
        0 => ColorElements::METER_VALUE.packed(scheme),
        INVALID_VALUE => ColorElements::METER_VALUE_ERROR.packed(scheme),
        _ => ColorElements::METER_VALUE_NOTICE.packed(scheme),
    }
}

/// Port of `static int valueDigitColor(unsigned int value)` from
/// `SystemdMeter.c:329`. Colors a count whose *good* value is nonzero:
/// `0` ⇒ `METER_VALUE_NOTICE`, `INVALID_VALUE` ⇒ `METER_VALUE_ERROR`,
/// otherwise `METER_VALUE`.
pub fn valueDigitColor(value: u32) -> i32 {
    let scheme = ColorScheme::active();
    match value {
        0 => ColorElements::METER_VALUE_NOTICE.packed(scheme),
        INVALID_VALUE => ColorElements::METER_VALUE_ERROR.packed(scheme),
        _ => ColorElements::METER_VALUE.packed(scheme),
    }
}

/// Port of `static void SystemdMeter_display(ATTR_UNUSED const Object* cast,
/// RichString* out, SystemdMeterContext_t* ctx)` from `SystemdMeter.c:341`.
/// The unused `cast` (`Object*`) is dropped. Writes the system state word
/// (colored `METER_VALUE_OK` for `"running"`, `METER_VALUE_ERROR` for
/// `"degraded"` or a missing state, else `METER_VALUE_WARN`), then the
/// `(nFailedUnits/nNames failed) (nJobs/nInstalledJobs jobs)` breakdown —
/// each `INVALID_VALUE` count rendered as `"?"`, each figure colored by
/// [`zeroDigitColor`] / [`valueDigitColor`].
fn SystemdMeter_display(out: &mut RichString, ctx: &SystemdMeterContext_t) {
    let scheme = ColorScheme::active();

    let color = if let Some(state) = ctx.systemState.as_deref() {
        if String_eq(state, "running") {
            ColorElements::METER_VALUE_OK
        } else if String_eq(state, "degraded") {
            ColorElements::METER_VALUE_ERROR
        } else {
            ColorElements::METER_VALUE_WARN
        }
    } else {
        ColorElements::METER_VALUE_ERROR
    };
    RichString_writeAscii(
        out,
        color.packed(scheme),
        ctx.systemState.as_deref().unwrap_or("N/A").as_bytes(),
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" (");

    let buffer = if ctx.nFailedUnits == INVALID_VALUE {
        "?".to_string()
    } else {
        format!("{}", ctx.nFailedUnits)
    };
    RichString_appendnAscii(
        out,
        zeroDigitColor(ctx.nFailedUnits),
        buffer.as_bytes(),
        buffer.len(),
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b"/");

    let buffer = if ctx.nNames == INVALID_VALUE {
        "?".to_string()
    } else {
        format!("{}", ctx.nNames)
    };
    RichString_appendnAscii(
        out,
        valueDigitColor(ctx.nNames),
        buffer.as_bytes(),
        buffer.len(),
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" failed) (");

    let buffer = if ctx.nJobs == INVALID_VALUE {
        "?".to_string()
    } else {
        format!("{}", ctx.nJobs)
    };
    RichString_appendnAscii(
        out,
        zeroDigitColor(ctx.nJobs),
        buffer.as_bytes(),
        buffer.len(),
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b"/");

    let buffer = if ctx.nInstalledJobs == INVALID_VALUE {
        "?".to_string()
    } else {
        format!("{}", ctx.nInstalledJobs)
    };
    RichString_appendnAscii(
        out,
        valueDigitColor(ctx.nInstalledJobs),
        buffer.as_bytes(),
        buffer.len(),
    );

    RichString_appendAscii(out, ColorElements::METER_TEXT.packed(scheme), b" jobs)");
}

/// Port of `static void SystemdMeter_display_system(ATTR_UNUSED const
/// Object* cast, RichString* out)` from `SystemdMeter.c:399`. Renders the
/// `ctx_system` cache. The unused `cast` is dropped.
pub fn SystemdMeter_display_system(out: &mut RichString) {
    let ctx = ctx_system.lock().unwrap();
    SystemdMeter_display(out, &ctx);
}

/// Port of `static void SystemdMeter_display_user(ATTR_UNUSED const Object*
/// cast, RichString* out)` from `SystemdMeter.c:403`. Renders the
/// `ctx_user` cache. The unused `cast` is dropped.
pub fn SystemdMeter_display_user(out: &mut RichString) {
    let ctx = ctx_user.lock().unwrap();
    SystemdMeter_display(out, &ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Visible characters of the valid `[0, chlen)` range of `out`.
    fn text(r: &RichString) -> String {
        (0..r.chlen as usize).map(|i| r.chptr[i].chars).collect()
    }

    #[test]
    fn display_running_all_known() {
        let ctx = SystemdMeterContext_t {
            systemState: Some("running".to_string()),
            nFailedUnits: 0,
            nInstalledJobs: 5,
            nNames: 42,
            nJobs: 0,
        };
        let mut out = RichString::new();
        SystemdMeter_display(&mut out, &ctx);
        assert_eq!(text(&out), "running (0/42 failed) (0/5 jobs)");
    }

    #[test]
    fn display_no_state_all_invalid() {
        let ctx = SystemdMeterContext_t {
            systemState: None,
            nFailedUnits: INVALID_VALUE,
            nInstalledJobs: INVALID_VALUE,
            nNames: INVALID_VALUE,
            nJobs: INVALID_VALUE,
        };
        let mut out = RichString::new();
        SystemdMeter_display(&mut out, &ctx);
        assert_eq!(text(&out), "N/A (?/? failed) (?/? jobs)");
    }

    #[test]
    fn digit_colors_map_each_branch() {
        // Each branch selects the C's `CRT_colors[...]` element; compare
        // against the packed element under the active scheme so the test is
        // scheme-independent (some schemes, e.g. monochrome, give several
        // elements the same attribute, so distinctness cannot be asserted).
        let scheme = ColorScheme::active();

        assert_eq!(zeroDigitColor(0), ColorElements::METER_VALUE.packed(scheme));
        assert_eq!(
            zeroDigitColor(INVALID_VALUE),
            ColorElements::METER_VALUE_ERROR.packed(scheme)
        );
        assert_eq!(
            zeroDigitColor(3),
            ColorElements::METER_VALUE_NOTICE.packed(scheme)
        );

        assert_eq!(
            valueDigitColor(0),
            ColorElements::METER_VALUE_NOTICE.packed(scheme)
        );
        assert_eq!(
            valueDigitColor(INVALID_VALUE),
            ColorElements::METER_VALUE_ERROR.packed(scheme)
        );
        assert_eq!(
            valueDigitColor(3),
            ColorElements::METER_VALUE.packed(scheme)
        );
    }
}
