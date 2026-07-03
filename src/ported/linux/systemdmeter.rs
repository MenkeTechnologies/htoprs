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
//! `!BUILD_STATIC || HAVE_LIBSYSTEMD`) is modeled linux-only as a cached
//! `zbus::blocking::Connection` — the pure-Rust D-Bus handle `updateViaLib`
//! opens once and reuses.
//!
//! GOVERNING RULE: no FFI, no dlopen. The C `updateViaLib` dlopen's
//! `libsystemd.so.0` and `dlsym`s the sd-bus symbols; that entire mechanism
//! is replaced by the `zbus` crate (pure-Rust D-Bus over the bus socket, no
//! shared object loaded). On non-linux the C's no-libsystemd variant is
//! mirrored: `updateViaLib` reports failure and the `systemctl show` exec
//! fallback ([`updateViaExec`]) runs.
//!
//! `CRT_colors[X]` is reproduced as `ColorElements::X.packed(scheme)`
//! (`CRT_colorSchemes[CRT_colorScheme][X]`), the mapping the other ported
//! meter renderers use. `xSnprintf(buffer, ..., "%u", v)` becomes
//! `format!("{v}")`; the returned `len` is the string's byte length.
//! `Meter_name(this)` reads the mirrored instance field `this.name`
//! (`meter.rs`, `Meter.h:101`).
//!
//! Ported:
//! - [`SystemdMeter_done`] (`SystemdMeter.c:71`) — the `.done` teardown
//! - [`updateViaLib`] (`SystemdMeter.c:98`) — linux: zbus D-Bus property
//!   reads; non-linux: no-op failure so the exec fallback runs
//! - [`updateViaExec`] (`SystemdMeter.c:214`) — the `systemctl show` spawn +
//!   parse (inlined `libc` fork/exec/pipe/waitpid, the `openfilesscreen.rs`
//!   precedent; takes `user: bool`, so needs no `Meter_name`)
//! - [`SystemdMeter_updateValues`] (`SystemdMeter.c:300`) — the `.updateValues`
//!   refresh slot
//! - [`zeroDigitColor`] (`SystemdMeter.c:318`)
//! - [`valueDigitColor`] (`SystemdMeter.c:329`)
//! - `SystemdMeter_display` (`SystemdMeter.c:341`)
//! - [`SystemdMeter_display_system`] (`SystemdMeter.c:399`)
//! - [`SystemdMeter_display_user`] (`SystemdMeter.c:403`)
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
use crate::ported::meter::Meter;
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
/// `char* systemState` (`:61`) becomes `Option<String>` (NULL ⇒ `None`); the
/// `unsigned int` counters map to `u32`. The C `sd_bus* bus` field (`:59`,
/// guarded on `!BUILD_STATIC || HAVE_LIBSYSTEMD`) becomes a cached
/// `zbus::blocking::Connection` — the pure-Rust D-Bus analogue of the sd-bus
/// handle `updateViaLib` reuses across refreshes (`if (!ctx->bus)`,
/// `SystemdMeter.c:127`). It is linux-only, matching the C guard: the darwin
/// build compiles the no-libsystemd variant where `bus` does not exist.
struct SystemdMeterContext_t {
    /// C `sd_bus* bus` (`SystemdMeter.c:59`) — the cached system/session bus
    /// connection, opened once and reused (`SystemdMeter.c:127`). Dropping it
    /// (setting `None`) is the `sd_bus_unref` (`:80`/`:199`).
    #[cfg(target_os = "linux")]
    bus: Option<zbus::blocking::Connection>,
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
    /// (`bus == NULL`, `systemState == NULL`, all counters `0`). `const` so it
    /// can seed a `Mutex` static initializer.
    const fn new() -> Self {
        SystemdMeterContext_t {
            #[cfg(target_os = "linux")]
            bus: None,
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

/// Port of `static void SystemdMeter_done(ATTR_UNUSED Meter* this)` from
/// `SystemdMeter.c:71`. The `.done` teardown slot: selects `&ctx_user` vs
/// `&ctx_system` via `String_eq(Meter_name(this), "SystemdUser")`
/// (`this.name`, `Meter.h:101`), frees `ctx->systemState` (`:74`, here setting
/// `None`), and drops the cached bus (`sym_sd_bus_unref(ctx->bus);
/// ctx->bus = NULL`, `:79`-`:82`/`:85`-`:88`) — on linux setting
/// `ctx.bus = None` runs the `zbus::blocking::Connection` `Drop`, the
/// `sd_bus_unref` analogue. The C `dlclose`-when-both-contexts-are-torn-down
/// branch (`:90`-`:93`) is a dlopen-handle-lifecycle detail with no zbus
/// counterpart (zbus links no shared object), so it is not modeled.
pub fn SystemdMeter_done(this: &mut Meter) {
    // C: SystemdMeterContext_t* ctx =
    //        String_eq(Meter_name(this), "SystemdUser") ? &ctx_user : &ctx_system;
    let ctx_mutex = if String_eq(this.name, "SystemdUser") {
        &ctx_user
    } else {
        &ctx_system
    };
    let mut ctx = ctx_mutex.lock().unwrap();

    // C: free(ctx->systemState); ctx->systemState = NULL;
    ctx.systemState = None;

    // C: if (ctx->bus) sym_sd_bus_unref(ctx->bus); ctx->bus = NULL;
    //    (dropping the cached Connection is the unref).
    #[cfg(target_os = "linux")]
    {
        ctx.bus = None;
    }
}

/// Port of `static int updateViaLib(bool user)` from `SystemdMeter.c:98`
/// (linux arm). The C body is the dlopen'd libsystemd sd-bus client; here the
/// GOVERNING RULE forbids FFI/dlopen, so the equivalent work is done with the
/// pure-Rust `zbus` crate's blocking D-Bus client — no shared object is
/// loaded, zbus speaks the D-Bus wire protocol directly over the bus socket.
///
/// The `dlopen`/`dlsym` symbol-resolution preamble (`:100`-`:123`) has no
/// analogue and drops out. The bus connect (`:127`-`:135`,
/// `sd_bus_open_user`/`sd_bus_open_system` cached in `ctx->bus`) becomes
/// `zbus::blocking::Connection::session()`/`::system()`, cached in `ctx.bus`
/// and reused across refreshes (the `if (!ctx->bus)` guard). Each
/// property read off service `org.freedesktop.systemd1`, object
/// `/org/freedesktop/systemd1`, interface `org.freedesktop.systemd1.Manager`
/// (`:137`-`:193`) maps to a `zbus::blocking::Proxy::get_property`: the
/// `sd_bus_get_property_string("SystemState")` (`:141`) is
/// `get_property::<String>`; each `sd_bus_get_property_trivial(…, 'u', …)`
/// (`:151`-`:191`, the `u` D-Bus type is `u32`) is `get_property::<u32>`.
///
/// Any failure jumps to the C `busfailure` label (`:198`-`:201`):
/// `sd_bus_unref(ctx->bus); ctx->bus = NULL; return -2` — here dropping the
/// cached `Connection` (`ctx.bus = None`) and returning `-2`, so the caller
/// falls back to [`updateViaExec`]. The `dlfailure` path (`return -1`) folds
/// into the same negative-return contract (a failed `Connection::*` open has
/// no bus to unref, returning `-2`). The return value is only tested `< 0` by
/// the caller, so `-1`/`-2` are interchangeable there.
#[cfg(target_os = "linux")]
pub fn updateViaLib(user: bool) -> i32 {
    use zbus::blocking::{Connection, Proxy};

    // C: SystemdMeterContext_t* ctx = user ? &ctx_user : &ctx_system;
    let ctx_mutex = if user { &ctx_user } else { &ctx_system };
    let mut ctx = ctx_mutex.lock().unwrap();

    // C: if (!ctx->bus) { r = user ? sd_bus_open_user(&ctx->bus)
    //                              : sd_bus_open_system(&ctx->bus);
    //                     if (r < 0) goto busfailure; }
    if ctx.bus.is_none() {
        let conn = if user {
            Connection::session()
        } else {
            Connection::system()
        };
        match conn {
            Ok(c) => ctx.bus = Some(c),
            // Open failed: no bus to unref, fall back to exec (busfailure).
            Err(_) => return -2,
        }
    }

    // C: static const char* busServiceName/busObjectPath/busInterfaceName.
    const BUS_SERVICE_NAME: &str = "org.freedesktop.systemd1";
    const BUS_OBJECT_PATH: &str = "/org/freedesktop/systemd1";
    const BUS_INTERFACE_NAME: &str = "org.freedesktop.systemd1.Manager";

    // `Connection` is a cheap ref-counted handle; clone it out of `ctx` so the
    // proxy borrows the clone, leaving `ctx` free for the property writes.
    let conn = ctx.bus.as_ref().unwrap().clone();
    let proxy = match Proxy::new(&conn, BUS_SERVICE_NAME, BUS_OBJECT_PATH, BUS_INTERFACE_NAME) {
        Ok(p) => p,
        // C busfailure: unref bus, return -2.
        Err(_) => {
            ctx.bus = None;
            return -2;
        }
    };

    // C: r = sd_bus_get_property_string(…, "SystemState", …, &ctx->systemState);
    //    if (r < 0) goto busfailure;
    match proxy.get_property::<String>("SystemState") {
        Ok(v) => ctx.systemState = Some(v),
        Err(_) => {
            ctx.bus = None;
            return -2;
        }
    }

    // C: r = sd_bus_get_property_trivial(…, "<name>", …, 'u', &ctx-><field>);
    //    if (r < 0) goto busfailure;   (repeated for each u32 counter)
    macro_rules! read_u32_property {
        ($property:literal, $field:ident) => {
            match proxy.get_property::<u32>($property) {
                Ok(v) => ctx.$field = v,
                Err(_) => {
                    ctx.bus = None;
                    return -2;
                }
            }
        };
    }
    read_u32_property!("NFailedUnits", nFailedUnits);
    read_u32_property!("NInstalledJobs", nInstalledJobs);
    read_u32_property!("NNames", nNames);
    read_u32_property!("NJobs", nJobs);

    // C: /* success */ return 0;
    0
}

/// Port of `static int updateViaLib(bool user)` from `SystemdMeter.c:98`
/// (non-linux arm). Mirrors htop's `BUILD_STATIC && !HAVE_LIBSYSTEMD` variant
/// where no libsystemd path exists and `SystemdMeter_updateValues` reaches
/// only [`updateViaExec`]. There is no systemd D-Bus off Linux, so this
/// always reports failure (`return -1`, the C `dlfailure` value), which makes
/// the caller fall back to the `systemctl show` exec path — behaviorally
/// identical to the C `#else` branch that calls `updateViaExec` directly.
#[cfg(not(target_os = "linux"))]
pub fn updateViaLib(_user: bool) -> i32 {
    -1
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

/// Port of `static void SystemdMeter_updateValues(Meter* this)` from
/// `SystemdMeter.c:300`. Selects `&ctx_user` vs `&ctx_system` via
/// `String_eq(Meter_name(this), "SystemdUser")` (`this.name`, `Meter.h:101`),
/// resets the cache (`systemState = NULL`; counters ⇒ `INVALID_VALUE`,
/// `:304`-`:306`), then refreshes: `if (updateViaLib(user) < 0)
/// updateViaExec(user)` (`:308`-`:313`). On non-linux [`updateViaLib`] always
/// returns `-1`, so the fallback exec path runs — matching the C `#else`
/// (no-libsystemd) branch that calls `updateViaExec` directly. Finally writes
/// `this->txtBuffer` from `ctx->systemState` (`"???"` when unset, `:315`).
///
/// The `updateViaLib`/`updateViaExec` calls each lock the ctx cache
/// internally, so the reset and the final `txtBuffer` read take the lock in
/// their own scopes — the `std::sync::Mutex` is not reentrant.
pub fn SystemdMeter_updateValues(this: &mut Meter) {
    // C: bool user = String_eq(Meter_name(this), "SystemdUser");
    let user = String_eq(this.name, "SystemdUser");
    // C: SystemdMeterContext_t* ctx = user ? &ctx_user : &ctx_system;
    let ctx_mutex = if user { &ctx_user } else { &ctx_system };

    // C: free(ctx->systemState); ctx->systemState = NULL;
    //    ctx->nFailedUnits = ctx->nInstalledJobs = ctx->nNames = ctx->nJobs
    //        = INVALID_VALUE;
    {
        let mut ctx = ctx_mutex.lock().unwrap();
        ctx.systemState = None;
        ctx.nFailedUnits = INVALID_VALUE;
        ctx.nInstalledJobs = INVALID_VALUE;
        ctx.nNames = INVALID_VALUE;
        ctx.nJobs = INVALID_VALUE;
    }

    // C: if (updateViaLib(user) < 0) updateViaExec(user);
    if updateViaLib(user) < 0 {
        updateViaExec(user);
    }

    // C: xSnprintf(this->txtBuffer, …, "%s",
    //        ctx->systemState ? ctx->systemState : "???");
    let ctx = ctx_mutex.lock().unwrap();
    this.txtBuffer = ctx.systemState.clone().unwrap_or_else(|| "???".to_string());
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
            #[cfg(target_os = "linux")]
            bus: None,
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
            #[cfg(target_os = "linux")]
            bus: None,
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
