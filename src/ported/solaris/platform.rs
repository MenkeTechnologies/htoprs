//! Partial port of `solaris/Platform.c` — htop's Solaris/illumos platform
//! hooks.
//!
//! Ported here (self-contained: only `libc` / Rust std and the already-ported
//! `NetworkIOData` / `DiskIOData` / `ACPresence`):
//! - `Platform_init` (`Platform.c:148`)
//! - `Platform_done` (`Platform.c:153`)
//! - `Platform_setBindings` (`Platform.c:157`)
//! - `Platform_getUptime` (`Platform.c:162`)
//! - `Platform_getLoadAverage` (`Platform.c:178`)
//! - `Platform_getFileDescriptors` (`Platform.c:328`)
//! - `Platform_getDiskIO` (`Platform.c:333`)
//! - `Platform_getNetworkIO` (`Platform.c:339`)
//! - `Platform_getBattery` (`Platform.c:345`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `Platform_getMaxPid` — needs `libkstat` (`kstat_open`/`kstat_lookup`/
//!   `kstat_read`) FFI and the `kvar_t` struct.
//! - the `Platform_set*Values` meter setters — `Meter::host` (`meter.rs`) is
//!   typed as the concrete `LinuxMachine`, so a `SolarisMachine`-backed meter
//!   is unmodeled.
//! - `Platform_buildenv` / `Platform_getProcessEnv` — need `libproc`
//!   (`Pgrab`/`Penv_iter`/`Prelease`) FFI.
//! - `Platform_getProcessLocks` — `FileLocks_ProcessData` is unmodeled
//!   (Solaris's body returns `NULL` unconditionally).
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::ffi::CStr;
use std::os::raw::c_int;
use std::ptr;

use crate::ported::batterymeter::ACPresence;
use crate::ported::diskiometer::DiskIOData;
use crate::ported::networkiometer::NetworkIOData;

// `LOADAVG_1MIN` / `LOADAVG_5MIN` / `LOADAVG_15MIN` (`sys/loadavg.h`).
const LOADAVG_1MIN: usize = 0;
const LOADAVG_5MIN: usize = 1;
const LOADAVG_15MIN: usize = 2;

/// Port of `bool Platform_init(void)` (`Platform.c:148`).
pub fn Platform_init() -> bool {
    /* no platform-specific setup needed */
    true
}

/// Port of `void Platform_done(void)` (`Platform.c:153`).
pub fn Platform_done() {
    /* no platform-specific cleanup needed */
}

/// Port of `void Platform_setBindings(Htop_Action* keys)` (`Platform.c:157`).
pub fn Platform_setBindings() {
    /* no platform-specific key bindings */
}

/// Port of `int Platform_getUptime(void)` (`Platform.c:162`). Scans the
/// `utmpx` database for the "system boot" record and returns the seconds
/// since it.
pub fn Platform_getUptime() -> c_int {
    let mut boot_time: c_int = 0;
    let curr_time = unsafe { libc::time(ptr::null_mut()) } as c_int;

    loop {
        let ent = unsafe { libc::getutxent() };
        if ent.is_null() {
            break;
        }
        let entry = unsafe { &*ent };
        let line = unsafe { CStr::from_ptr(entry.ut_line.as_ptr()) };
        if line.to_bytes() == b"system boot" {
            boot_time = entry.ut_tv.tv_sec as c_int;
        }
    }

    unsafe { libc::endutxent() };

    curr_time - boot_time
}

/// Port of `void Platform_getLoadAverage(double* one, double* five, double*
/// fifteen)` (`Platform.c:178`).
pub fn Platform_getLoadAverage(one: &mut f64, five: &mut f64, fifteen: &mut f64) {
    let mut plat_loadavg = [0.0f64; 3];
    if unsafe { libc::getloadavg(plat_loadavg.as_mut_ptr(), 3) } < 0 {
        *one = f64::NAN;
        *five = f64::NAN;
        *fifteen = f64::NAN;
        return;
    }
    *one = plat_loadavg[LOADAVG_1MIN];
    *five = plat_loadavg[LOADAVG_5MIN];
    *fifteen = plat_loadavg[LOADAVG_15MIN];
}

/// TODO: port of `pid_t Platform_getMaxPid(void)` from `Platform.c:191`.
/// Blocked: needs `libkstat` (`kstat_open`/`kstat_lookup`/`kstat_read`) FFI
/// and the `kvar_t` struct (`v_proc`).
pub fn Platform_getMaxPid() {
    todo!("port of Platform.c:191")
}

/// TODO: port of `double Platform_setCPUValues(Meter* this, unsigned int cpu)`
/// from `Platform.c:211`. Blocked: `Meter::host` typed as `LinuxMachine`; a
/// `SolarisMachine`-backed meter is unmodeled.
pub fn Platform_setCPUValues() {
    todo!("port of Platform.c:211")
}

/// TODO: port of `void Platform_setMemoryValues(Meter* this)` from
/// `Platform.c:252`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setMemoryValues() {
    todo!("port of Platform.c:252")
}

/// TODO: port of `void Platform_setSwapValues(Meter* this)` from
/// `Platform.c:260`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setSwapValues() {
    todo!("port of Platform.c:260")
}

/// TODO: port of `void Platform_setZfsArcValues(Meter* this)` from
/// `Platform.c:266`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setZfsArcValues() {
    todo!("port of Platform.c:266")
}

/// TODO: port of `void Platform_setZfsCompressedArcValues(Meter* this)` from
/// `Platform.c:272`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setZfsCompressedArcValues() {
    todo!("port of Platform.c:272")
}

/// TODO: port of `static int Platform_buildenv(void* accum, const prmap_t*
/// map, const char* name)` from `Platform.c:278`. Blocked: `libproc`
/// `Penv_iter` callback with the `prmap_t` / `ps_prochandle` types.
pub fn Platform_buildenv() {
    todo!("port of Platform.c:278")
}

/// TODO: port of `char* Platform_getProcessEnv(pid_t pid)` from
/// `Platform.c:300`. Blocked: needs `libproc` (`Pgrab`/`Penv_iter`/`Prelease`)
/// FFI.
pub fn Platform_getProcessEnv() {
    todo!("port of Platform.c:300")
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// from `Platform.c:323`. Blocked: `FileLocks_ProcessData` is unmodeled
/// (Solaris's body returns `NULL` unconditionally).
pub fn Platform_getProcessLocks() {
    todo!("port of Platform.c:323")
}

/// Port of `void Platform_getFileDescriptors(double* used, double* max)`
/// (`Platform.c:328`). Solaris does not yet expose these, so both are `NaN`.
pub fn Platform_getFileDescriptors(used: &mut f64, max: &mut f64) {
    *used = f64::NAN;
    *max = f64::NAN;
}

/// Port of `bool Platform_getDiskIO(DiskIOData* data)` (`Platform.c:333`).
/// Not yet implemented on Solaris; returns `false` without touching `data`.
pub fn Platform_getDiskIO(_data: &mut DiskIOData) -> bool {
    // TODO
    false
}

/// Port of `bool Platform_getNetworkIO(NetworkIOData* data)`
/// (`Platform.c:339`). Not yet implemented on Solaris; returns `false`.
pub fn Platform_getNetworkIO(_data: &mut NetworkIOData) -> bool {
    // TODO
    false
}

/// Port of `void Platform_getBattery(double* percent, ACPresence* isOnAC)`
/// (`Platform.c:345`). Solaris has no battery probe, so `percent` is `NaN` and
/// the AC state is `AC_ERROR`.
pub fn Platform_getBattery(percent: &mut f64, isOnAC: &mut ACPresence) {
    *percent = f64::NAN;
    *isOnAC = ACPresence::AC_ERROR;
}
