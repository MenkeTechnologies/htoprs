//! Port of `openbsd/Platform.c` — htop's OpenBSD platform hooks.
//!
//! Ported here:
//! - `Platform_init` (`Platform.c:152`)
//! - `Platform_done` (`Platform.c:157`)
//! - `Platform_setBindings` (`Platform.c:161`)
//! - `Platform_getUptime` (`Platform.c:166`)
//! - `Platform_getLoadAverage` (`Platform.c:180`)
//! - `Platform_getMaxPid` (`Platform.c:197`)
//! - `Platform_setCPUValues` (`Platform.c:201`)
//! - `Platform_setMemoryValues` (`Platform.c:243`)
//! - `Platform_setSwapValues` (`Platform.c:259`)
//! - `Platform_getProcessEnv` (`Platform.c:265`)
//! - `Platform_getFileDescriptors` (`Platform.c:325`)
//! - `Platform_getDiskIO` (`Platform.c:349`)
//! - `Platform_getNetworkIO` (`Platform.c:355`)
//! - `findDevice` (`Platform.c:361`) / `Platform_getBattery` (`Platform.c:376`)
//!
//! # Verification note
//!
//! OpenBSD is a tier-3 Rust target with no prebuilt `std`, so this module
//! cannot be cross-compiled on the darwin dev host. Every `libc` symbol used
//! here was verified against `libc`'s `unix/bsd/netbsdlike/openbsd` source;
//! the `kvm`/`swapctl`/`hw.sensors` FFI and every non-`libc` constant / struct
//! is transcribed from the OpenBSD kernel headers cited inline. The meter
//! setters mirror the compiled darwin port. It is source-reviewed, not
//! compile-verified.
//!
//! Still `todo!()`:
//! - `Platform_getProcessLocks` (`Platform.c:320`) — OpenBSD returns `NULL`
//!   unconditionally, but the `FileLocks_ProcessData` return type is unmodeled
//!   in the Rust port (the darwin/netbsd/dragonflybsd precedent stubs this
//!   identically on every platform).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::ported::batterymeter::ACPresence;
use crate::ported::diskiometer::DiskIOData;
use crate::ported::machine::Machine;
use crate::ported::meter::Meter;
use crate::ported::networkiometer::NetworkIOData;
use crate::ported::openbsd::openbsdmachine::{
    kvm_close, kvm_getenvv, kvm_getprocs, kvm_openfiles, OpenBSDMachine, _POSIX2_LINE_MAX,
    KVM_NO_FILES,
};

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

// `CPUMeter.h` `CPU_METER_*` indices into `Meter::values`.
const CPU_METER_NICE: usize = 0;
const CPU_METER_NORMAL: usize = 1;
const CPU_METER_KERNEL: usize = 2;
const CPU_METER_IRQ: usize = 3;
const CPU_METER_SOFTIRQ: usize = 4;
const CPU_METER_STEAL: usize = 5;
const CPU_METER_GUEST: usize = 6;
const CPU_METER_IOWAIT: usize = 7;
const CPU_METER_FREQUENCY: usize = 8;
const CPU_METER_TEMPERATURE: usize = 9;

// `openbsd/Platform.c:101` `MEMORY_CLASS_*` indices into `Meter::values`.
const MEMORY_CLASS_WIRED: usize = 0;
const MEMORY_CLASS_CACHE: usize = 1;
const MEMORY_CLASS_ACTIVE: usize = 2;
const MEMORY_CLASS_PAGING: usize = 3;
const MEMORY_CLASS_INACTIVE: usize = 4;

/// `SWAP_METER_USED = 0` (`SwapMeter.h`).
const SWAP_METER_USED: usize = 0;

// ── `hw.sensors` (`sys/sensors.h`), absent from `libc`. ──────────────────────

/// `#define HW_SENSORS 11` (`sys/sysctl.h`).
const HW_SENSORS: c_int = 11;
/// `SENSOR_WATTHOUR` — position 7 in `enum sensor_type` (`sys/sensors.h`).
const SENSOR_WATTHOUR: c_int = 7;
/// `SENSOR_INDICATOR` — position 9 in `enum sensor_type` (`sys/sensors.h`).
const SENSOR_INDICATOR: c_int = 9;
/// `SENSOR_MAX_TYPES` — the `enum sensor_type` count (`sys/sensors.h`), the
/// `sensordev.maxnumt` array length.
const SENSOR_MAX_TYPES: usize = 23;

/// Port of `struct sensor` (`sys/sensors.h`) — one hardware-monitor reading.
/// `enum sensor_type` / `enum sensor_status` are C `int`s. Only `value` is
/// read, but the whole layout is transcribed so the sysctl length matches.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct sensor {
    pub desc: [c_char; 32],
    pub tv: libc::timeval,
    pub value: i64,
    pub type_: c_int,
    pub status: c_int,
    pub numt: c_int,
    pub flags: c_int,
}

/// Port of `struct sensordev` (`sys/sensors.h`) — a sensor device. Only
/// `xname` is read (matched against the device name).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct sensordev {
    pub num: c_int,
    pub xname: [c_char; 16],
    pub maxnumt: [c_int; SENSOR_MAX_TYPES],
    pub sensors_count: c_int,
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

/// Port of `double Platform_setCPUValues(Meter* this, unsigned int cpu)` from
/// `Platform.c:201`. Fills the per-CPU meter from
/// `OpenBSDMachine.cpuData[cpu]` (index 0 = average). Offline CPUs report
/// `NAN`; the detailed/simple split follows `detailedCPUTime`.
///
/// # Safety
/// `mtr.host` must be the `super_` base of a live [`OpenBSDMachine`].
pub fn Platform_setCPUValues(mtr: &mut Meter, cpu: u32) -> f64 {
    let host = mtr.host;
    let ohost = host as *const OpenBSDMachine;
    let cpuData = unsafe { &(*ohost).cpuData[cpu as usize] };

    if !cpuData.online {
        mtr.curItems = 0;
        return f64::NAN;
    }

    let total = if cpuData.totalPeriod == 0 {
        1.0
    } else {
        cpuData.totalPeriod as f64
    };

    mtr.values[CPU_METER_NICE] = cpuData.nicePeriod as f64 / total * 100.0;
    mtr.values[CPU_METER_NORMAL] = cpuData.userPeriod as f64 / total * 100.0;
    let detailed = unsafe { (*host).settings.as_ref().is_some_and(|s| s.detailedCPUTime) };
    if detailed {
        mtr.values[CPU_METER_KERNEL] = cpuData.sysPeriod as f64 / total * 100.0;
        mtr.values[CPU_METER_IRQ] = cpuData.intrPeriod as f64 / total * 100.0;
        mtr.values[CPU_METER_SOFTIRQ] = 0.0;
        mtr.values[CPU_METER_STEAL] = 0.0;
        mtr.values[CPU_METER_GUEST] = 0.0;
        mtr.values[CPU_METER_IOWAIT] = 0.0;
        mtr.values[CPU_METER_FREQUENCY] = f64::NAN;
        mtr.curItems = 8;
    } else {
        mtr.values[CPU_METER_KERNEL] = cpuData.sysAllPeriod as f64 / total * 100.0;
        mtr.values[CPU_METER_IRQ] = 0.0; // No steal nor guest on OpenBSD
        mtr.curItems = 4;
    }

    let mut totalPercent = mtr.values[CPU_METER_NICE]
        + mtr.values[CPU_METER_NORMAL]
        + mtr.values[CPU_METER_KERNEL]
        + mtr.values[CPU_METER_IRQ];
    totalPercent = totalPercent.clamp(0.0, 100.0);

    mtr.values[CPU_METER_TEMPERATURE] = f64::NAN;

    let cpuSpeed = unsafe { (*ohost).cpuSpeed };
    mtr.values[CPU_METER_FREQUENCY] = if cpuSpeed != -1 {
        cpuSpeed as f64
    } else {
        f64::NAN
    };

    totalPercent
}

/// Port of `void Platform_setMemoryValues(Meter* this)` from `Platform.c:243`.
/// Fills the memory meter's class values (kB) from the `OpenBSDMachine`
/// breakdown; `showCachedMemory` folds cache into wired when disabled.
///
/// # Safety
/// `mtr.host` must be the `super_` base of a live [`OpenBSDMachine`].
pub fn Platform_setMemoryValues(mtr: &mut Meter) {
    let host = mtr.host;
    let ohost = host as *const OpenBSDMachine;

    mtr.total = unsafe { (*host).totalMem } as f64;
    let show_cached = unsafe {
        (*host)
            .settings
            .as_ref()
            .is_some_and(|s| s.showCachedMemory)
    };
    let o = unsafe { &*ohost };
    if show_cached {
        mtr.values[MEMORY_CLASS_WIRED] = o.wiredMem as f64;
        mtr.values[MEMORY_CLASS_CACHE] = o.cacheMem as f64;
    } else {
        // merge cache into the wired pages
        mtr.values[MEMORY_CLASS_WIRED] = (o.wiredMem + o.cacheMem) as f64;
        mtr.values[MEMORY_CLASS_CACHE] = 0.0;
    }
    mtr.values[MEMORY_CLASS_ACTIVE] = o.activeMem as f64;
    mtr.values[MEMORY_CLASS_PAGING] = o.pagingMem as f64;
    mtr.values[MEMORY_CLASS_INACTIVE] = o.inactiveMem as f64;
}

/// Port of `void Platform_setSwapValues(Meter* this)` from `Platform.c:259`.
/// Reads only the base [`Machine`] swap totals (no OpenBSD-specific state).
///
/// # Safety
/// `mtr.host` must be a live [`Machine`].
pub fn Platform_setSwapValues(mtr: &mut Meter) {
    let host: *const Machine = mtr.host;
    mtr.total = unsafe { (*host).totalSwap } as f64;
    mtr.values[SWAP_METER_USED] = unsafe { (*host).usedSwap } as f64;
}

/// Port of `char* Platform_getProcessEnv(pid_t pid)` from `Platform.c:265`.
/// Opens a fileless `kvm` handle, looks up the process, and returns its raw
/// environment block (`kvm_getenvv`, NUL-separated, double-NUL terminated) as
/// a `String`, or `None` on any failure.
pub fn Platform_getProcessEnv(pid: libc::pid_t) -> Option<String> {
    let mut errbuf = [0 as c_char; _POSIX2_LINE_MAX];
    let kt = unsafe {
        kvm_openfiles(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            KVM_NO_FILES,
            errbuf.as_mut_ptr(),
        )
    };
    if kt.is_null() {
        return None;
    }

    let mut count: c_int = 0;
    let kproc = unsafe {
        kvm_getprocs(
            kt,
            libc::KERN_PROC_PID,
            pid,
            size_of::<libc::kinfo_proc>(),
            &mut count,
        )
    };
    if kproc.is_null() {
        unsafe { kvm_close(kt) };
        return None;
    }

    let ptrs = unsafe { kvm_getenvv(kt, kproc, 0) };
    if ptrs.is_null() {
        unsafe { kvm_close(kt) };
        return None;
    }

    let mut env: Vec<u8> = Vec::new();
    unsafe {
        let mut i: isize = 0;
        loop {
            let p = *ptrs.offset(i);
            if p.is_null() {
                break;
            }
            env.extend_from_slice(std::ffi::CStr::from_ptr(p).to_bytes());
            env.push(0);
            i += 1;
        }
        kvm_close(kt);
    }

    // Ensure the double-NUL terminator the C guarantees (even for an empty
    // environment, C:308-313 writes two trailing NULs).
    while env.len() < 2 || env[env.len() - 1] != 0 || env[env.len() - 2] != 0 {
        env.push(0);
    }
    Some(String::from_utf8_lossy(&env).into_owned())
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// from `Platform.c:320`. Kept stubbed: OpenBSD's body returns `NULL`
/// unconditionally, but the `FileLocks_ProcessData` return type is unmodeled
/// in the Rust port (the darwin/netbsd/dragonflybsd precedent — every platform
/// stubs this identically until that struct family lands).
pub fn Platform_getProcessLocks() {
    todo!("port of Platform.c:320 — FileLocks_ProcessData unmodeled (returns NULL)")
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

/// Port of `static bool findDevice(const char* name, int* mib, struct
/// sensordev* snsrdev, size_t* sdlen)` from `Platform.c:361`. Scans
/// `hw.sensors.<devn>` for a device whose `xname` matches `name`, skipping
/// gaps (`ENXIO`) and stopping at the end of the list (`ENOENT`). Writes the
/// device number into `mib[2]` (as the C does) so the caller's follow-up
/// `sysctl(mib, 5, …)` reads that device's sensors.
pub fn findDevice(
    name: &str,
    mib: &mut [c_int; 5],
    snsrdev: &mut sensordev,
    sdlen: &mut usize,
) -> bool {
    let mut devn: c_int = 0;
    loop {
        mib[2] = devn;
        if unsafe {
            libc::sysctl(
                mib.as_ptr(),
                3,
                snsrdev as *mut sensordev as *mut c_void,
                sdlen,
                ptr::null_mut(),
                0,
            )
        } == -1
        {
            match std::io::Error::last_os_error().raw_os_error() {
                Some(e) if e == libc::ENXIO => {
                    devn += 1;
                    continue;
                }
                Some(e) if e == libc::ENOENT => return false,
                _ => {}
            }
        }
        let xname_bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(snsrdev.xname.as_ptr() as *const u8, snsrdev.xname.len())
        };
        let n = xname_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(xname_bytes.len());
        if name.as_bytes() == &xname_bytes[..n] {
            return true;
        }
        devn += 1;
    }
}

/// Port of `void Platform_getBattery(double* percent, ACPresence* isOnAC)`
/// from `Platform.c:376`. Reads the ACPI battery (`acpibat0`) watt-hour
/// sensors to derive charge percent, and the AC adapter (`acpiac0`) indicator
/// sensor for AC presence, via `hw.sensors`.
pub fn Platform_getBattery(percent: &mut f64, isOnAC: &mut ACPresence) {
    let mut mib: [c_int; 5] = [libc::CTL_HW, HW_SENSORS, 0, 0, 0];
    let mut s: sensor = unsafe { std::mem::zeroed() };
    let mut slen = size_of::<sensor>();
    let mut snsrdev: sensordev = unsafe { std::mem::zeroed() };
    let mut sdlen = size_of::<sensordev>();

    let found = findDevice("acpibat0", &mut mib, &mut snsrdev, &mut sdlen);

    *percent = f64::NAN;
    if found {
        // See "sys/dev/acpi/acpibat.c" of OpenBSD for the field indices.
        mib[3] = SENSOR_WATTHOUR;
        mib[4] = 0; /* "last full capacity" */
        let mut last_full_capacity = 0.0f64;
        if unsafe {
            libc::sysctl(
                mib.as_ptr(),
                5,
                &mut s as *mut sensor as *mut c_void,
                &mut slen,
                ptr::null_mut(),
                0,
            )
        } != -1
        {
            last_full_capacity = s.value as f64;
        }
        if last_full_capacity > 0.0 {
            mib[3] = SENSOR_WATTHOUR;
            mib[4] = 3; /* "remaining capacity" */
            if unsafe {
                libc::sysctl(
                    mib.as_ptr(),
                    5,
                    &mut s as *mut sensor as *mut c_void,
                    &mut slen,
                    ptr::null_mut(),
                    0,
                )
            } != -1
            {
                let charge = s.value as f64;
                *percent = 100.0 * (charge / last_full_capacity);
                if charge >= last_full_capacity {
                    *percent = 100.0;
                }
            }
        }
    }

    let found = findDevice("acpiac0", &mut mib, &mut snsrdev, &mut sdlen);

    *isOnAC = ACPresence::AC_ERROR;
    if found {
        // See "sys/dev/acpi/acpiac.c" — one sensor for this device.
        mib[3] = SENSOR_INDICATOR;
        mib[4] = 0; /* "power supply" (status indicator) */
        if unsafe {
            libc::sysctl(
                mib.as_ptr(),
                5,
                &mut s as *mut sensor as *mut c_void,
                &mut slen,
                ptr::null_mut(),
                0,
            )
        } != -1
        {
            *isOnAC = if s.value != 0 {
                ACPresence::AC_PRESENT
            } else {
                ACPresence::AC_ABSENT
            };
        }
    }
}

/// Port of `openbsd/Platform.h:102`. Non-PCP build: no dynamic meters, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicMeters() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `openbsd/Platform.h:106`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-meter table.
pub fn Platform_dynamicMetersDone(_table: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `openbsd/Platform.h:108`. `ATTR_UNUSED` no-op meter init.
pub fn Platform_dynamicMeterInit(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `openbsd/Platform.h:110`. `ATTR_UNUSED` no-op value update.
pub fn Platform_dynamicMeterUpdateValues(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `openbsd/Platform.h:112`. `ATTR_UNUSED` no-op display.
pub fn Platform_dynamicMeterDisplay(
    _meter: &crate::ported::meter::Meter,
    _out: &mut crate::ported::richstring::RichString,
) {
}

/// Port of `openbsd/Platform.h:128`. Non-PCP build: no dynamic screens, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicScreens() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `openbsd/Platform.h:138`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-screen table.
pub fn Platform_dynamicScreensDone(_screens: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `openbsd/Platform.h:136`. `ATTR_UNUSED` no-op — non-PCP builds add
/// no dynamic-screen columns.
pub fn Platform_addDynamicScreenAvailableColumns(
    _availableColumns: &mut crate::ported::panel::Panel,
    _screen: &str,
) {
}
