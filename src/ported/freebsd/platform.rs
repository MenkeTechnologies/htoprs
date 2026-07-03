//! Partial port of `freebsd/Platform.c` — htop's FreeBSD platform hooks.
//!
//! Ported here (self-contained: only `libc` / Rust std and the already-ported
//! `NetworkIOData` / `ACPresence`):
//! - `Platform_init` (`Platform.c:160`)
//! - `Platform_done` (`Platform.c:165`)
//! - `Platform_setBindings` (`Platform.c:169`)
//! - `Platform_getUptime` (`Platform.c:174`)
//! - `Platform_getLoadAverage` (`Platform.c:188`)
//! - `Platform_getMaxPid` (`Platform.c:205`)
//! - `Platform_getProcessEnv` (`Platform.c:293`)
//! - `Platform_getNetworkIO` (`Platform.c:367`)
//! - `Platform_getBattery` (`Platform.c:399`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - the `Platform_set*Values` meter setters — `Meter::host` (`meter.rs`) is
//!   typed as the concrete `LinuxMachine`, so a `FreeBSDMachine`-backed meter
//!   is unmodeled.
//! - `Platform_getFileDescriptors` — needs `Generic_getFileDescriptors_sysctl`
//!   (`generic/fdstat_sysctl.c`, unported).
//! - `Platform_getProcessLocks` — `FileLocks_ProcessData` is unmodeled
//!   (FreeBSD's body returns `NULL` unconditionally).
//! - `Platform_getDiskIO` — needs `libdevstat` (`devstat_*`) FFI bindings.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::mem::size_of;
use std::os::raw::{c_int, c_void};
use std::ptr;

use crate::ported::batterymeter::ACPresence;
use crate::ported::networkiometer::NetworkIOData;

// `VM_LOADAVG` (`sys/vm/vm_param.h`) — the `CTL_VM` sysctl for load average.
// Absent from `libc`.
const VM_LOADAVG: c_int = 2;

/// Port of `struct loadavg` (`sys/resource.h`) — the kernel load-average
/// triple. `fixpt_t` is `__uint32_t`. Absent from `libc`.
#[repr(C)]
struct loadavg {
    ldavg: [u32; 3],
    fscale: libc::c_long,
}

/// Port of `bool Platform_init(void)` (`Platform.c:160`).
pub fn Platform_init() -> bool {
    /* no platform-specific setup needed */
    true
}

/// Port of `void Platform_done(void)` (`Platform.c:165`).
pub fn Platform_done() {
    /* no platform-specific cleanup needed */
}

/// Port of `void Platform_setBindings(Htop_Action* keys)` (`Platform.c:169`).
pub fn Platform_setBindings() {
    /* no platform-specific key bindings */
}

/// Port of `int Platform_getUptime(void)` (`Platform.c:174`).
pub fn Platform_getUptime() -> c_int {
    let mut bootTime: libc::timeval = unsafe { std::mem::zeroed() };
    let mib: [c_int; 2] = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    let mut size = size_of::<libc::timeval>();

    let err = unsafe {
        libc::sysctl(
            mib.as_ptr() as *mut c_int,
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
/// fifteen)` (`Platform.c:188`).
pub fn Platform_getLoadAverage(one: &mut f64, five: &mut f64, fifteen: &mut f64) {
    let mut loadAverage: loadavg = unsafe { std::mem::zeroed() };
    let mib: [c_int; 2] = [libc::CTL_VM, VM_LOADAVG];
    let mut size = size_of::<loadavg>();

    let err = unsafe {
        libc::sysctl(
            mib.as_ptr() as *mut c_int,
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

/// Port of `pid_t Platform_getMaxPid(void)` (`Platform.c:205`).
pub fn Platform_getMaxPid() -> libc::pid_t {
    let mut maxPid: c_int = 0;
    let mut size = size_of::<c_int>();
    let err = unsafe {
        libc::sysctlbyname(
            b"kern.pid_max\0".as_ptr() as *const libc::c_char,
            &mut maxPid as *mut c_int as *mut c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if err != 0 {
        return 99999;
    }
    maxPid
}

/// TODO: port of `double Platform_setCPUValues(Meter* this, unsigned int cpu)`
/// from `Platform.c:215`. Blocked: `Meter::host` typed as `LinuxMachine`; a
/// `FreeBSDMachine`-backed meter is unmodeled.
pub fn Platform_setCPUValues() {
    todo!("port of Platform.c:215")
}

/// TODO: port of `void Platform_setMemoryValues(Meter* this)` from
/// `Platform.c:247`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setMemoryValues() {
    todo!("port of Platform.c:247")
}

/// TODO: port of `void Platform_setSwapValues(Meter* this)` from
/// `Platform.c:274`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setSwapValues() {
    todo!("port of Platform.c:274")
}

/// TODO: port of `void Platform_setZfsArcValues(Meter* this)` from
/// `Platform.c:281`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setZfsArcValues() {
    todo!("port of Platform.c:281")
}

/// TODO: port of `void Platform_setZfsCompressedArcValues(Meter* this)` from
/// `Platform.c:287`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setZfsCompressedArcValues() {
    todo!("port of Platform.c:287")
}

/// Port of `char* Platform_getProcessEnv(pid_t pid)` (`Platform.c:293`).
/// Returns the raw environment block (NUL-separated, double-NUL terminated)
/// as a `String`, or `None` on failure.
pub fn Platform_getProcessEnv(pid: libc::pid_t) -> Option<String> {
    let mib: [c_int; 4] = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_ENV, pid];

    let mut capacity = libc::ARG_MAX as usize;
    let mut env = vec![0u8; capacity];

    let err = unsafe {
        libc::sysctl(
            mib.as_ptr() as *mut c_int,
            4,
            env.as_mut_ptr() as *mut c_void,
            &mut capacity,
            ptr::null_mut(),
            0,
        )
    };
    if err != 0 || capacity == 0 {
        return None;
    }

    env.truncate(capacity);
    // Ensure the double-NUL terminator the caller expects.
    if env[capacity - 1] != 0 || (capacity >= 2 && env[capacity - 2] != 0) {
        env.push(0);
        env.push(0);
    }

    Some(String::from_utf8_lossy(&env).into_owned())
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// from `Platform.c:314`. Blocked: `FileLocks_ProcessData` is unmodeled
/// (FreeBSD's body returns `NULL` unconditionally).
pub fn Platform_getProcessLocks() {
    todo!("port of Platform.c:314")
}

/// TODO: port of `void Platform_getFileDescriptors(double* used, double* max)`
/// from `Platform.c:319`. Blocked: needs `Generic_getFileDescriptors_sysctl`
/// (`generic/fdstat_sysctl.c`, unported).
pub fn Platform_getFileDescriptors() {
    todo!("port of Platform.c:319")
}

/// TODO: port of `bool Platform_getDiskIO(DiskIOData* data)` from
/// `Platform.c:323`. Blocked: needs `libdevstat` (`devstat_checkversion` /
/// `devstat_getdevs` / `devstat_compute_statistics`) FFI bindings.
pub fn Platform_getDiskIO() {
    todo!("port of Platform.c:323")
}

/// Port of `bool Platform_getNetworkIO(NetworkIOData* data)`
/// (`Platform.c:367`). Accumulates non-loopback interface counters onto the
/// caller-zeroed `data`, matching the C `+=` aggregation.
pub fn Platform_getNetworkIO(data: &mut NetworkIOData) -> bool {
    // get number of interfaces
    let mut count: c_int = 0;
    let mut countLen = size_of::<c_int>();
    let countMib: [c_int; 5] = [
        libc::CTL_NET,
        libc::PF_LINK,
        libc::NETLINK_GENERIC,
        libc::IFMIB_SYSTEM,
        libc::IFMIB_IFCOUNT,
    ];

    let mut r = unsafe {
        libc::sysctl(
            countMib.as_ptr() as *mut c_int,
            countMib.len() as u32,
            &mut count as *mut c_int as *mut c_void,
            &mut countLen,
            ptr::null_mut(),
            0,
        )
    };
    if r < 0 {
        return false;
    }

    for i in 1..=count {
        let mut ifmd: libc::ifmibdata = unsafe { std::mem::zeroed() };
        let mut ifmdLen = size_of::<libc::ifmibdata>();

        let dataMib: [c_int; 6] = [
            libc::CTL_NET,
            libc::PF_LINK,
            libc::NETLINK_GENERIC,
            libc::IFMIB_IFDATA,
            i,
            libc::IFDATA_GENERAL,
        ];

        r = unsafe {
            libc::sysctl(
                dataMib.as_ptr() as *mut c_int,
                dataMib.len() as u32,
                &mut ifmd as *mut libc::ifmibdata as *mut c_void,
                &mut ifmdLen,
                ptr::null_mut(),
                0,
            )
        };
        if r < 0 {
            continue;
        }

        if ifmd.ifmd_flags & libc::IFF_LOOPBACK != 0 {
            continue;
        }

        data.bytesReceived += ifmd.ifmd_data.ifi_ibytes;
        data.packetsReceived += ifmd.ifmd_data.ifi_ipackets;
        data.bytesTransmitted += ifmd.ifmd_data.ifi_obytes;
        data.packetsTransmitted += ifmd.ifmd_data.ifi_opackets;
    }

    true
}

/// Port of `void Platform_getBattery(double* percent, ACPresence* isOnAC)`
/// (`Platform.c:399`).
pub fn Platform_getBattery(percent: &mut f64, isOnAC: &mut ACPresence) {
    let mut life: c_int = 0;
    let mut life_len = size_of::<c_int>();
    if unsafe {
        libc::sysctlbyname(
            b"hw.acpi.battery.life\0".as_ptr() as *const libc::c_char,
            &mut life as *mut c_int as *mut c_void,
            &mut life_len,
            ptr::null_mut(),
            0,
        )
    } == -1
    {
        *percent = f64::NAN;
    } else {
        *percent = life as f64;
    }

    let mut acline: c_int = 0;
    let mut acline_len = size_of::<c_int>();
    if unsafe {
        libc::sysctlbyname(
            b"hw.acpi.acline\0".as_ptr() as *const libc::c_char,
            &mut acline as *mut c_int as *mut c_void,
            &mut acline_len,
            ptr::null_mut(),
            0,
        )
    } == -1
    {
        *isOnAC = ACPresence::AC_ERROR;
    } else {
        *isOnAC = if acline == 0 {
            ACPresence::AC_ABSENT
        } else {
            ACPresence::AC_PRESENT
        };
    }
}
