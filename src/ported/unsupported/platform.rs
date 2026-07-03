//! Port of `unsupported/Platform.c` — htop's portable fallback platform, used
//! when no OS-specific backend matches. Every hook returns a fixed/degenerate
//! value, so the whole file is self-contained (no platform FFI) and compiles
//! on every target.
//!
//! Ported here:
//! - `Platform_init` (`Platform.c:92`)
//! - `Platform_done` (`Platform.c:97`)
//! - `Platform_setBindings` (`Platform.c:101`)
//! - `Platform_getUptime` (`Platform.c:106`)
//! - `Platform_getLoadAverage` (`Platform.c:110`)
//! - `Platform_getMaxPid` (`Platform.c:116`)
//! - `Platform_getProcessEnv` (`Platform.c:144`)
//! - `Platform_getFileDescriptors` (`Platform.c:154`)
//! - `Platform_getDiskIO` (`Platform.c:159`)
//! - `Platform_getNetworkIO` (`Platform.c:164`)
//! - `Platform_getBattery` (`Platform.c:169`)
//! - `Platform_getHostname` (`Platform.c:174`)
//! - `Platform_getRelease` (`Platform.c:178`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `Platform_setCPUValues` / `Platform_setMemoryValues` /
//!   `Platform_setSwapValues` — write through `Meter::values` / `Meter::curItems`
//!   on a meter whose `host` field (`meter.rs`) is typed as the concrete
//!   `LinuxMachine`; the generic meter-setter surface is not yet modeled.
//! - `Platform_getProcessLocks` — `FileLocks_ProcessData` is unmodeled
//!   (returns `NULL` unconditionally here).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::batterymeter::ACPresence;
use crate::ported::diskiometer::DiskIOData;
use crate::ported::networkiometer::NetworkIOData;
use crate::ported::xutils::String_safeStrncpy;

/// The C file-`static const char Platform_unsupported[] = "unsupported"`
/// (`unsupported/Platform.c:90`).
const Platform_unsupported: &str = "unsupported";

/// Port of `bool Platform_init(void)` (`Platform.c:92`).
pub fn Platform_init() -> bool {
    /* no platform-specific setup needed */
    true
}

/// Port of `void Platform_done(void)` (`Platform.c:97`).
pub fn Platform_done() {
    /* no platform-specific cleanup needed */
}

/// Port of `void Platform_setBindings(Htop_Action* keys)` (`Platform.c:101`).
pub fn Platform_setBindings() {
    /* no platform-specific key bindings */
}

/// Port of `int Platform_getUptime(void)` (`Platform.c:106`).
pub fn Platform_getUptime() -> i32 {
    0
}

/// Port of `void Platform_getLoadAverage(double* one, double* five, double*
/// fifteen)` (`Platform.c:110`).
pub fn Platform_getLoadAverage(one: &mut f64, five: &mut f64, fifteen: &mut f64) {
    *one = 0.0;
    *five = 0.0;
    *fifteen = 0.0;
}

/// Port of `pid_t Platform_getMaxPid(void)` (`Platform.c:116`).
pub fn Platform_getMaxPid() -> libc::pid_t {
    i32::MAX
}

/// TODO: port of `double Platform_setCPUValues(Meter* this, unsigned int cpu)`
/// from `Platform.c:120`. Blocked: `Meter::host` typed as `LinuxMachine`; the
/// generic meter-setter surface is unmodeled.
pub fn Platform_setCPUValues() {
    todo!("port of Platform.c:120")
}

/// TODO: port of `void Platform_setMemoryValues(Meter* this)` from
/// `Platform.c:132`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setMemoryValues() {
    todo!("port of Platform.c:132")
}

/// TODO: port of `void Platform_setSwapValues(Meter* this)` from
/// `Platform.c:140`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setSwapValues() {
    todo!("port of Platform.c:140")
}

/// Port of `char* Platform_getProcessEnv(pid_t pid)` (`Platform.c:144`).
/// The fallback platform exposes no environment, so this is always `None`.
pub fn Platform_getProcessEnv(_pid: libc::pid_t) -> Option<String> {
    None
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// from `Platform.c:149`. Blocked: `FileLocks_ProcessData` is unmodeled
/// (returns `NULL` unconditionally here).
pub fn Platform_getProcessLocks() {
    todo!("port of Platform.c:149")
}

/// Port of `void Platform_getFileDescriptors(double* used, double* max)`
/// (`Platform.c:154`). Fixed placeholder values on the fallback platform.
pub fn Platform_getFileDescriptors(used: &mut f64, max: &mut f64) {
    *used = 1337.0;
    *max = 4711.0;
}

/// Port of `bool Platform_getDiskIO(DiskIOData* data)` (`Platform.c:159`).
pub fn Platform_getDiskIO(_data: &mut DiskIOData) -> bool {
    false
}

/// Port of `bool Platform_getNetworkIO(NetworkIOData* data)`
/// (`Platform.c:164`).
pub fn Platform_getNetworkIO(_data: &mut NetworkIOData) -> bool {
    false
}

/// Port of `void Platform_getBattery(double* percent, ACPresence* isOnAC)`
/// (`Platform.c:169`).
pub fn Platform_getBattery(percent: &mut f64, isOnAC: &mut ACPresence) {
    *percent = f64::NAN;
    *isOnAC = ACPresence::AC_ERROR;
}

/// Port of `void Platform_getHostname(char* buffer, size_t size)`
/// (`Platform.c:174`). Writes the literal "unsupported".
pub fn Platform_getHostname(buffer: &mut [u8]) {
    String_safeStrncpy(buffer, Platform_unsupported.as_bytes());
}

/// Port of `const char* Platform_getRelease(void)` (`Platform.c:178`).
pub fn Platform_getRelease() -> &'static str {
    Platform_unsupported
}
