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
//! Also ported: `Platform_setCPUValues` / `_setMemoryValues` /
//! `_setSwapValues` (read the `FreeBSDMachine` via the `#[repr(C)]`
//! `*Machine`→`*FreeBSDMachine` downcast) and `Platform_getProcessLocks`
//! (FreeBSD's body returns `NULL` unconditionally → `None`).
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `Platform_setZfsArcValues` / `_setZfsCompressedArcValues` — need
//!   `ZfsArcMeter_readStats` / `ZfsCompressedArcMeter_readStats`
//!   (`zfs/Zfs*Meter.c`, unported; the darwin port is likewise deferred).
//! - `Platform_getFileDescriptors` — needs `Generic_getFileDescriptors_sysctl`
//!   (`generic/fdstat_sysctl.c`, unported).
//! - `Platform_getDiskIO` — needs `libdevstat`: the `struct statinfo` /
//!   `struct devinfo` / `struct devstat` types (absent from `libc`) plus the
//!   variadic `devstat_compute_statistics(..., DSM_* selectors, ...)` call,
//!   which cannot be transcribed faithfully without those headers.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::mem::size_of;
use std::os::raw::{c_int, c_void};
use std::ptr;

use crate::ported::batterymeter::ACPresence;
use crate::ported::freebsd::freebsdmachine::FreeBSDMachine;
use crate::ported::meter::Meter;
use crate::ported::networkiometer::NetworkIOData;
use crate::ported::processlocksscreen::FileLocks_ProcessData;

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

// FreeBSD's `CPU_METER_*` indices (`CPUMeter.h`) into `Meter::values`.
const CPU_METER_NICE: usize = 0;
const CPU_METER_NORMAL: usize = 1;
const CPU_METER_KERNEL: usize = 2;
const CPU_METER_IRQ: usize = 3;
const CPU_METER_FREQUENCY: usize = 8;
const CPU_METER_TEMPERATURE: usize = 9;

/// Port of `double Platform_setCPUValues(Meter* this, unsigned int cpu)` from
/// `Platform.c:215`. Reads the per-core [`CPUData`](crate::ported::freebsd::freebsdmachine::CPUData)
/// off the `FreeBSDMachine` (`(FreeBSDMachine*)this->host`) — slot 0 on a
/// single-CPU box, else `cpu` — and fills the meter's nice/normal/kernel
/// (+irq when `detailedCPUTime`) percentages, frequency and temperature,
/// returning the clamped total usage.
pub fn Platform_setCPUValues(mtr: &mut Meter, cpu: u32) -> f64 {
    let host = mtr.host;
    let fhost = host as *const FreeBSDMachine;
    let cpus = unsafe { (*host).activeCPUs };

    // single CPU box has everything in fhost->cpus[0]
    let cpuData = unsafe {
        // Borrow the Vec explicitly before indexing so it does not implicitly
        // autoref the raw-pointer deref (`&(*fhost)`), which the compiler denies.
        let cpus_arr = &(*fhost).cpus;
        if cpus == 1 {
            cpus_arr[0]
        } else {
            cpus_arr[cpu as usize]
        }
    };

    let detailed = unsafe {
        (*host)
            .settings
            .as_ref()
            .is_some_and(|s| s.detailedCPUTime)
    };

    mtr.values[CPU_METER_NICE] = cpuData.nicePercent;
    mtr.values[CPU_METER_NORMAL] = cpuData.userPercent;

    let percent = if detailed {
        mtr.values[CPU_METER_KERNEL] = cpuData.systemPercent;
        mtr.values[CPU_METER_IRQ] = cpuData.irqPercent;
        mtr.curItems = 4;
        mtr.values[CPU_METER_NICE]
            + mtr.values[CPU_METER_NORMAL]
            + mtr.values[CPU_METER_KERNEL]
            + mtr.values[CPU_METER_IRQ]
    } else {
        mtr.values[CPU_METER_KERNEL] = cpuData.systemAllPercent;
        mtr.curItems = 3;
        mtr.values[CPU_METER_NICE] + mtr.values[CPU_METER_NORMAL] + mtr.values[CPU_METER_KERNEL]
    };

    let percent = percent.clamp(0.0, 100.0);

    mtr.values[CPU_METER_FREQUENCY] = cpuData.frequency;
    mtr.values[CPU_METER_TEMPERATURE] = cpuData.temperature;

    percent
}

// FreeBSD's `MEMORY_CLASS_*` enum (`freebsd/Platform.c:101`) — indices into
// `Meter::values`, in this exact order.
const MEMORY_CLASS_WIRED: usize = 0;
const MEMORY_CLASS_BUFFERS: usize = 1;
const MEMORY_CLASS_ACTIVE: usize = 2;
const MEMORY_CLASS_LAUNDRY: usize = 3;
const MEMORY_CLASS_INACTIVE: usize = 4;
const MEMORY_CLASS_ARC: usize = 5;

/// Port of `void Platform_setMemoryValues(Meter* this)` from `Platform.c:247`.
/// Fills the memory meter's class values (kB) from the `FreeBSDMachine`:
/// wired/buffers (merged when `showCachedMemory` is off), active, laundry,
/// inactive, and the shrinkable ZFS ARC (`size - min`, when enabled).
pub fn Platform_setMemoryValues(mtr: &mut Meter) {
    let host = mtr.host;
    let fhost = host as *const FreeBSDMachine;

    mtr.total = unsafe { (*host).totalMem } as f64;

    let show_cached = unsafe {
        (*host)
            .settings
            .as_ref()
            .is_some_and(|s| s.showCachedMemory)
    };

    unsafe {
        if show_cached {
            mtr.values[MEMORY_CLASS_WIRED] = (*fhost).wiredMem as f64;
            mtr.values[MEMORY_CLASS_BUFFERS] = (*fhost).buffersMem as f64;
        } else {
            // merge buffers into the wired pages
            mtr.values[MEMORY_CLASS_WIRED] = ((*fhost).wiredMem + (*fhost).buffersMem) as f64;
            mtr.values[MEMORY_CLASS_BUFFERS] = 0.0;
        }
        mtr.values[MEMORY_CLASS_ACTIVE] = (*fhost).activeMem as f64;
        mtr.values[MEMORY_CLASS_LAUNDRY] = (*fhost).laundryMem as f64;
        mtr.values[MEMORY_CLASS_INACTIVE] = (*fhost).inactiveMem as f64;

        if (*fhost).zfs.enabled != 0 {
            // ZFS does not shrink below the value of zfs_arc_min.
            let mut shrinkableSize: u64 = 0;
            if (*fhost).zfs.size > (*fhost).zfs.min {
                shrinkableSize = (*fhost).zfs.size - (*fhost).zfs.min;
            }
            mtr.values[MEMORY_CLASS_ARC] = shrinkableSize as f64;
        } else {
            mtr.values[MEMORY_CLASS_ARC] = 0.0;
        }
    }
}

/// Port of `void Platform_setSwapValues(Meter* this)` from `Platform.c:274`.
/// Copies the host's swap totals (kB) into the swap meter.
pub fn Platform_setSwapValues(mtr: &mut Meter) {
    /// `SWAP_METER_USED = 0` (`SwapMeter.h`).
    const SWAP_METER_USED: usize = 0;

    let host = mtr.host;
    mtr.total = unsafe { (*host).totalSwap } as f64;
    mtr.values[SWAP_METER_USED] = unsafe { (*host).usedSwap } as f64;
}

/// TODO: port of `void Platform_setZfsArcValues(Meter* this)` from
/// `Platform.c:281`. Blocked: `ZfsArcMeter_readStats` (`zfs/ZfsArcMeter.c`)
/// is unported, so the ARC stats on `(FreeBSDMachine*)this->host->zfs`
/// cannot be rendered into the meter (the darwin port is likewise deferred).
pub fn Platform_setZfsArcValues() {
    todo!("port of Platform.c:281")
}

/// TODO: port of `void Platform_setZfsCompressedArcValues(Meter* this)` from
/// `Platform.c:287`. Blocked: `ZfsCompressedArcMeter_readStats`
/// (`zfs/ZfsCompressedArcMeter.c`) is unported (the darwin port is likewise
/// deferred).
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

/// Port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)` from
/// `Platform.c:314`. FreeBSD does not expose per-process file locks, so the C
/// body is `(void)pid; return NULL;` — the faithful analog returns `None`,
/// and `ProcessLocksScreen_scan` renders "not supported".
pub fn Platform_getProcessLocks(pid: libc::pid_t) -> Option<FileLocks_ProcessData> {
    let _ = pid;
    None
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

/// Port of `freebsd/Platform.h:104`. Non-PCP build: no dynamic meters, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicMeters() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `freebsd/Platform.h:108`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-meter table.
pub fn Platform_dynamicMetersDone(_table: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `freebsd/Platform.h:110`. `ATTR_UNUSED` no-op meter init.
pub fn Platform_dynamicMeterInit(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `freebsd/Platform.h:112`. `ATTR_UNUSED` no-op value update.
pub fn Platform_dynamicMeterUpdateValues(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `freebsd/Platform.h:114`. `ATTR_UNUSED` no-op display.
pub fn Platform_dynamicMeterDisplay(
    _meter: &crate::ported::meter::Meter,
    _out: &mut crate::ported::richstring::RichString,
) {
}

/// Port of `freebsd/Platform.h:130`. Non-PCP build: no dynamic screens, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicScreens() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `freebsd/Platform.h:140`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-screen table.
pub fn Platform_dynamicScreensDone(_screens: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `freebsd/Platform.h:138`. `ATTR_UNUSED` no-op — non-PCP builds add
/// no dynamic-screen columns.
pub fn Platform_addDynamicScreenAvailableColumns(
    _availableColumns: &mut crate::ported::panel::Panel,
    _screen: &str,
) {
}
