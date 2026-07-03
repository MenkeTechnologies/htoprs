//! Partial port of `netbsd/Platform.c` — htop's NetBSD platform hooks.
//!
//! Ported here (self-contained: only `libc` / Rust std and the already-ported
//! `NetworkIOData`):
//! - `Platform_init` (`Platform.c:206`)
//! - `Platform_done` (`Platform.c:211`)
//! - `Platform_setBindings` (`Platform.c:215`)
//! - `Platform_getUptime` (`Platform.c:220`)
//! - `Platform_getLoadAverage` (`Platform.c:234`)
//! - `Platform_getMaxPid` (`Platform.c:251`)
//! - `Platform_getNetworkIO` (`Platform.c:423`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - the `Platform_set*Values` meter setters — `Meter::host` (`meter.rs`) is
//!   typed as the concrete `LinuxMachine`, so a `NetBSDMachine`-backed meter
//!   is unmodeled.
//! - `Platform_getProcessEnv` — needs `libkvm` (`kvm_openfiles`/`kvm_getproc2`/
//!   `kvm_getenvv2`) and the `kinfo_proc2` struct.
//! - `Platform_getFileDescriptors` — needs `Generic_getFileDescriptors_sysctl`
//!   (`generic/fdstat_sysctl.c`, unported).
//! - `Platform_getProcessLocks` — `FileLocks_ProcessData` is unmodeled
//!   (NetBSD's body returns `NULL` unconditionally).
//! - `Platform_getDiskIO` — needs the `io_sysctl` struct (`sys/iostat.h`).
//! - `Platform_getBattery` — needs NetBSD `proplib` (`prop_dictionary_*`) FFI
//!   plus the `ENVSYS` ioctl.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::mem::size_of;
use std::os::raw::{c_int, c_void};
use std::ptr;

use crate::ported::networkiometer::NetworkIOData;

// `VM_LOADAVG` (`uvm/uvm_param.h`) — the `CTL_VM` sysctl for load average.
// Absent from `libc`.
const VM_LOADAVG: c_int = 2;

/// Port of `struct loadavg` (`sys/resource.h`) — the kernel load-average
/// triple. `fixpt_t` is `uint32_t`. Absent from `libc`.
#[repr(C)]
struct loadavg {
    ldavg: [u32; 3],
    fscale: libc::c_long,
}

/// Port of `bool Platform_init(void)` (`Platform.c:206`).
pub fn Platform_init() -> bool {
    /* no platform-specific setup needed */
    true
}

/// Port of `void Platform_done(void)` (`Platform.c:211`).
pub fn Platform_done() {
    /* no platform-specific cleanup needed */
}

/// Port of `void Platform_setBindings(Htop_Action* keys)` (`Platform.c:215`).
pub fn Platform_setBindings() {
    /* no platform-specific key bindings */
}

/// Port of `int Platform_getUptime(void)` (`Platform.c:220`).
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
            ptr::null(),
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
/// fifteen)` (`Platform.c:234`).
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
            ptr::null(),
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

/// Port of `pid_t Platform_getMaxPid(void)` (`Platform.c:251`).
pub fn Platform_getMaxPid() -> libc::pid_t {
    // https://nxr.netbsd.org/xref/src/sys/sys/ansi.h#__pid_t
    // pid is assigned as a 32bit Integer.
    i32::MAX
}

/// TODO: port of `double Platform_setCPUValues(Meter* this, int cpu)` from
/// `Platform.c:257`. Blocked: `Meter::host` typed as `LinuxMachine`; a
/// `NetBSDMachine`-backed meter is unmodeled.
pub fn Platform_setCPUValues() {
    todo!("port of Platform.c:257")
}

/// TODO: port of `void Platform_setMemoryValues(Meter* this)` from
/// `Platform.c:290`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setMemoryValues() {
    todo!("port of Platform.c:290")
}

/// TODO: port of `void Platform_setSwapValues(Meter* this)` from
/// `Platform.c:300`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setSwapValues() {
    todo!("port of Platform.c:300")
}

/// TODO: port of `char* Platform_getProcessEnv(pid_t pid)` from
/// `Platform.c:306`. Blocked: needs `libkvm` (`kvm_openfiles`/`kvm_getproc2`/
/// `kvm_getenvv2`) FFI and the `kinfo_proc2` struct.
pub fn Platform_getProcessEnv() {
    todo!("port of Platform.c:306")
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// from `Platform.c:360`. Blocked: `FileLocks_ProcessData` is unmodeled
/// (NetBSD's body returns `NULL` unconditionally).
pub fn Platform_getProcessLocks() {
    todo!("port of Platform.c:360")
}

/// TODO: port of `void Platform_getFileDescriptors(double* used, double* max)`
/// from `Platform.c:365`. Blocked: needs `Generic_getFileDescriptors_sysctl`
/// (`generic/fdstat_sysctl.c`, unported).
pub fn Platform_getFileDescriptors() {
    todo!("port of Platform.c:365")
}

/// TODO: port of `bool Platform_getDiskIO(DiskIOData* data)` from
/// `Platform.c:369`. Blocked: needs the `io_sysctl` struct (`sys/iostat.h`,
/// absent from `libc`) for the `CTL_HW`/`HW_IOSTATS` sysctl.
pub fn Platform_getDiskIO() {
    todo!("port of Platform.c:369")
}

/// Port of `bool Platform_getNetworkIO(NetworkIOData* data)`
/// (`Platform.c:423`). Walks `getifaddrs`, summing non-loopback `AF_LINK`
/// interface counters onto the caller-zeroed `data`.
pub fn Platform_getNetworkIO(data: &mut NetworkIOData) -> bool {
    let mut ifap: *mut libc::ifaddrs = ptr::null_mut();

    if unsafe { libc::getifaddrs(&mut ifap) } != 0 {
        return false;
    }

    let mut ifa = ifap;
    while !ifa.is_null() {
        let cur = unsafe { &*ifa };
        ifa = cur.ifa_next;

        if cur.ifa_addr.is_null() {
            continue;
        }
        if unsafe { (*cur.ifa_addr).sa_family } as c_int != libc::AF_LINK {
            continue;
        }
        if cur.ifa_flags & libc::IFF_LOOPBACK as libc::c_uint != 0 {
            continue;
        }

        let ifd = cur.ifa_data as *const libc::if_data;
        if ifd.is_null() {
            continue;
        }
        let d = unsafe { &*ifd };

        data.bytesReceived += d.ifi_ibytes;
        data.packetsReceived += d.ifi_ipackets;
        data.bytesTransmitted += d.ifi_obytes;
        data.packetsTransmitted += d.ifi_opackets;
    }

    unsafe { libc::freeifaddrs(ifap) };
    true
}

/// TODO: port of `void Platform_getBattery(double* percent, ACPresence*
/// isOnAC)` from `Platform.c:449`. Blocked: needs NetBSD `proplib`
/// (`prop_dictionary_recv_ioctl` / `prop_*`) FFI plus the `ENVSYS`
/// (`_PATH_SYSMON`) ioctl.
pub fn Platform_getBattery() {
    todo!("port of Platform.c:449")
}
