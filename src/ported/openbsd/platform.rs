//! Partial port of `openbsd/Platform.c` — htop's OpenBSD platform hooks.
//!
//! Ported here (self-contained: only `libc` / Rust std and the already-ported
//! `NetworkIOData` / `DiskIOData`):
//! - `Platform_init` (`Platform.c:152`)
//! - `Platform_done` (`Platform.c:157`)
//! - `Platform_setBindings` (`Platform.c:161`)
//! - `Platform_getUptime` (`Platform.c:166`)
//! - `Platform_getLoadAverage` (`Platform.c:180`)
//! - `Platform_getMaxPid` (`Platform.c:197`)
//! - `Platform_getFileDescriptors` (`Platform.c:325`)
//! - `Platform_getDiskIO` (`Platform.c:349`)
//! - `Platform_getNetworkIO` (`Platform.c:355`)
//!
//! # Verification note
//!
//! OpenBSD is a tier-3 Rust target with no prebuilt `std`, so this module
//! cannot be cross-compiled on the darwin dev host. Every `libc` symbol used
//! here (`CTL_KERN`, `KERN_BOOTTIME`, `CTL_VM`, `KERN_MAXFILES`,
//! `KERN_NFILES`, `sysctl`, `timeval`, `gettimeofday`) was verified against
//! `libc`'s `unix/bsd/netbsdlike/openbsd` source; the port-purity gate checks
//! the fn names. It is not compile-verified.
//!
//! Still `todo!()` and blocked on unported substrate:
//! - the `Platform_set*Values` meter setters — `Meter::host` (`meter.rs`) is
//!   typed as the concrete `LinuxMachine`.
//! - `Platform_getProcessEnv` — needs `libkvm` (`kvm_openfiles`/`kvm_getprocs`/
//!   `kvm_getenvv`) FFI and the `kinfo_proc` struct.
//! - `Platform_getProcessLocks` — `FileLocks_ProcessData` is unmodeled
//!   (OpenBSD's body returns `NULL` unconditionally).
//! - `findDevice` / `Platform_getBattery` — need the `hw.sensors`
//!   `struct sensor` / `struct sensordev` types (`sys/sensors.h`).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::mem::size_of;
use std::os::raw::{c_int, c_void};
use std::ptr;

use crate::ported::diskiometer::DiskIOData;
use crate::ported::networkiometer::NetworkIOData;

// `VM_LOADAVG` (`sys/sysctl.h`) — the `CTL_VM` sysctl for load average.
// Absent from `libc`.
const VM_LOADAVG: c_int = 2;

// `THREAD_PID_OFFSET` (`sys/proc.h`) = 100000; absent from `libc`.
const THREAD_PID_OFFSET: libc::pid_t = 100000;

/// Port of `struct loadavg` (`sys/resource.h`) — the kernel load-average
/// triple. `fixpt_t` is `uint32_t`. Absent from `libc`.
#[repr(C)]
struct loadavg {
    ldavg: [u32; 3],
    fscale: libc::c_long,
}

/// Port of `bool Platform_init(void)` (`Platform.c:152`).
pub fn Platform_init() -> bool {
    /* no platform-specific setup needed */
    true
}

/// Port of `void Platform_done(void)` (`Platform.c:157`).
pub fn Platform_done() {
    /* no platform-specific cleanup needed */
}

/// Port of `void Platform_setBindings(Htop_Action* keys)` (`Platform.c:161`).
pub fn Platform_setBindings() {
    /* no platform-specific key bindings */
}

/// Port of `int Platform_getUptime(void)` (`Platform.c:166`).
pub fn Platform_getUptime() -> c_int {
    let mut bootTime: libc::timeval = unsafe { std::mem::zeroed() };
    let mib: [c_int; 2] = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    let mut size = size_of::<libc::timeval>();

    let err = unsafe {
        libc::sysctl(
            mib.as_ptr(),
            2,
            &mut bootTime as *mut libc::timeval as *mut c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if err != 0 {
        return -1;
    }
    let mut currTime: libc::timeval = unsafe { std::mem::zeroed() };
    unsafe { libc::gettimeofday(&mut currTime, ptr::null_mut()) };

    (currTime.tv_sec - bootTime.tv_sec) as c_int
}

/// Port of `void Platform_getLoadAverage(double* one, double* five, double*
/// fifteen)` (`Platform.c:180`).
pub fn Platform_getLoadAverage(one: &mut f64, five: &mut f64, fifteen: &mut f64) {
    let mut loadAverage: loadavg = unsafe { std::mem::zeroed() };
    let mib: [c_int; 2] = [libc::CTL_VM, VM_LOADAVG];
    let mut size = size_of::<loadavg>();

    let err = unsafe {
        libc::sysctl(
            mib.as_ptr(),
            2,
            &mut loadAverage as *mut loadavg as *mut c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if err != 0 {
        *one = 0.0;
        *five = 0.0;
        *fifteen = 0.0;
        return;
    }
    *one = loadAverage.ldavg[0] as f64 / loadAverage.fscale as f64;
    *five = loadAverage.ldavg[1] as f64 / loadAverage.fscale as f64;
    *fifteen = loadAverage.ldavg[2] as f64 / loadAverage.fscale as f64;
}

/// Port of `pid_t Platform_getMaxPid(void)` (`Platform.c:197`).
pub fn Platform_getMaxPid() -> libc::pid_t {
    2 * THREAD_PID_OFFSET
}

/// TODO: port of `double Platform_setCPUValues(Meter* this, unsigned int cpu)`
/// from `Platform.c:201`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setCPUValues() {
    todo!("port of Platform.c:201")
}

/// TODO: port of `void Platform_setMemoryValues(Meter* this)` from
/// `Platform.c:243`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setMemoryValues() {
    todo!("port of Platform.c:243")
}

/// TODO: port of `void Platform_setSwapValues(Meter* this)` from
/// `Platform.c:259`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setSwapValues() {
    todo!("port of Platform.c:259")
}

/// TODO: port of `char* Platform_getProcessEnv(pid_t pid)` from
/// `Platform.c:265`. Blocked: needs `libkvm` (`kvm_openfiles`/`kvm_getprocs`/
/// `kvm_getenvv`) FFI and the `kinfo_proc` struct.
pub fn Platform_getProcessEnv() {
    todo!("port of Platform.c:265")
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// from `Platform.c:320`. Blocked: `FileLocks_ProcessData` is unmodeled
/// (OpenBSD's body returns `NULL` unconditionally).
pub fn Platform_getProcessLocks() {
    todo!("port of Platform.c:320")
}

/// Port of `void Platform_getFileDescriptors(double* used, double* max)`
/// (`Platform.c:325`). Reads `kern.maxfiles` and `kern.nfiles` via `sysctl`.
pub fn Platform_getFileDescriptors(used: &mut f64, max: &mut f64) {
    let mib_kern_maxfile: [c_int; 2] = [libc::CTL_KERN, libc::KERN_MAXFILES];
    let mut sysctl_maxfile: c_int = 0;
    let mut size_maxfile = size_of::<c_int>();
    if unsafe {
        libc::sysctl(
            mib_kern_maxfile.as_ptr(),
            2,
            &mut sysctl_maxfile as *mut c_int as *mut c_void,
            &mut size_maxfile,
            ptr::null_mut(),
            0,
        )
    } < 0
    {
        *max = f64::NAN;
    } else if size_maxfile != size_of::<c_int>() || sysctl_maxfile < 1 {
        *max = f64::NAN;
    } else {
        *max = sysctl_maxfile as f64;
    }

    let mib_kern_nfiles: [c_int; 2] = [libc::CTL_KERN, libc::KERN_NFILES];
    let mut sysctl_nfiles: c_int = 0;
    let mut size_nfiles = size_of::<c_int>();
    if unsafe {
        libc::sysctl(
            mib_kern_nfiles.as_ptr(),
            2,
            &mut sysctl_nfiles as *mut c_int as *mut c_void,
            &mut size_nfiles,
            ptr::null_mut(),
            0,
        )
    } < 0
    {
        *used = f64::NAN;
    } else if size_nfiles != size_of::<c_int>() || sysctl_nfiles < 0 {
        *used = f64::NAN;
    } else {
        *used = sysctl_nfiles as f64;
    }
}

/// Port of `bool Platform_getDiskIO(DiskIOData* data)` (`Platform.c:349`).
/// Not yet implemented on OpenBSD; returns `false` without touching `data`.
pub fn Platform_getDiskIO(_data: &mut DiskIOData) -> bool {
    // TODO
    false
}

/// Port of `bool Platform_getNetworkIO(NetworkIOData* data)`
/// (`Platform.c:355`). Not yet implemented on OpenBSD; returns `false`.
pub fn Platform_getNetworkIO(_data: &mut NetworkIOData) -> bool {
    // TODO
    false
}

/// TODO: port of `static bool findDevice(const char* name, int* mib, struct
/// sensordev* snsrdev, size_t* sdlen)` from `Platform.c:361`. Blocked: needs
/// the `hw.sensors` `struct sensordev` type (`sys/sensors.h`).
pub fn findDevice() {
    todo!("port of Platform.c:361")
}

/// TODO: port of `void Platform_getBattery(double* percent, ACPresence*
/// isOnAC)` from `Platform.c:376`. Blocked: needs `findDevice` plus the
/// `hw.sensors` `struct sensor` / `struct sensordev` types (`sys/sensors.h`).
pub fn Platform_getBattery() {
    todo!("port of Platform.c:376")
}
