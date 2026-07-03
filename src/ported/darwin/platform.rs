//! Partial port of `darwin/Platform.c` — htop's Darwin platform hooks.
//!
//! Ported here (self-contained: only `libc` / Rust std / mach FFI /
//! the already-ported `Platform_*KernelVersion*` helpers, `CRT_fatalError`
//! and `NetworkIOData`):
//! - `Platform_calculateNanosecondsPerMachTick` (`Platform.c:182`)
//! - `Platform_machTicksToNanoseconds` (`Platform.c:226`)
//! - `Platform_init` (`Platform.c:239`)
//! - `Platform_schedulerTicksToNanoseconds` (`Platform.c:258`)
//! - `Platform_done` (`Platform.c:262`)
//! - `Platform_setBindings` (`Platform.c:266`)
//! - `Platform_getUptime` (`Platform.c:271`)
//! - `Platform_getLoadAverage` (`Platform.c:285`)
//! - `Platform_getMaxPid` (`Platform.c:299`)
//! - `Platform_getProcessEnv` (`Platform.c:477`)
//! - `Platform_getNetworkIO` (`Platform.c:626`)
//! - `Platform_gettime_monotonic` (`Platform.c:739`, mach clock branch)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - the `Platform_set*Values` meter setters — `Meter::host` (`meter.rs`)
//!   is typed as the concrete `LinuxMachine`, so a `DarwinMachine`-backed
//!   meter cannot be modeled until `meter.rs` generalizes that field.
//! - `Platform_getFileDescriptors` — needs `Generic_getFileDescriptors_sysctl`
//!   (`generic/fdstat_sysctl.c`, unported).
//! - `Platform_getProcessLocks` — `FileLocks_ProcessData` is unmodeled
//!   (C returns `NULL` unconditionally on Darwin).
//! - `Platform_getDiskIO` / `Platform_getBattery` — need CoreFoundation /
//!   IOKit FFI bindings not yet established in this tree.
//! - `Platform_getOSRelease` / `Platform_getRelease` — need
//!   `Generic_unameRelease` (`generic/uname.c`, unported) and a
//!   CoreFoundation property-list reader.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::mem::{size_of, zeroed};
use std::os::raw::{c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::ported::crt::CRT_fatalError;
// Used only by the `#[cfg(target_arch = "x86_64")]` Rosetta workaround below.
#[cfg(target_arch = "x86_64")]
use crate::ported::darwin::platformhelpers::{
    KernelVersion, Platform_CompareKernelVersion, Platform_isRunningTranslated,
};
use crate::ported::networkiometer::NetworkIOData;

// `KERN_SUCCESS` (`mach/kern_return.h`).
const KERN_SUCCESS: c_int = 0;

// `IFT_LOOP` (`net/if_types.h`) — the loopback interface type, absent
// from `libc`. Used to exclude loopback traffic in `Platform_getNetworkIO`.
const IFT_LOOP: u8 = 0x18;

// `SYSTEM_CLOCK` (`mach/clock_types.h`) — the monotonic system clock id.
const SYSTEM_CLOCK: c_int = 0;

/// Port of `struct mach_timespec` / `mach_timespec_t`
/// (`mach/clock_types.h`), used by `clock_get_time`.
#[repr(C)]
struct mach_timespec_t {
    tv_sec: libc::c_uint,
    tv_nsec: c_int,
}

extern "C" {
    fn host_get_clock_service(
        host: libc::mach_port_t,
        clock_id: c_int,
        clock_serv: *mut libc::mach_port_t,
    ) -> c_int;
    fn clock_get_time(clock_serv: libc::mach_port_t, cur_time: *mut mach_timespec_t) -> c_int;
    fn mach_port_deallocate(task: libc::mach_port_t, name: libc::mach_port_t) -> c_int;
}

// File-level statics from `darwin/Platform.c:177-180`.
static Platform_nanosecondsPerMachTickNumer: AtomicU64 = AtomicU64::new(1);
static Platform_nanosecondsPerMachTickDenom: AtomicU64 = AtomicU64::new(1);
static Platform_nanosecondsPerSchedulerTick: Mutex<f64> = Mutex::new(-1.0);

/// Port of `static void Platform_calculateNanosecondsPerMachTick(uint64_t*
/// numer, uint64_t* denom)` (`Platform.c:182`).
// `libc::mach_timebase_info` and its `numer`/`denom` fields are marked
// deprecated in `libc` (it steers callers to the `mach2` crate); the C
// original uses exactly this call, and adding a fashionable dependency
// violates the "vendorable and durable" constraint, so keep the libc path.
#[allow(deprecated)]
pub fn Platform_calculateNanosecondsPerMachTick(numer: &mut u64, denom: &mut u64) {
    // Check if we can determine the timebase used on this system.

    #[cfg(target_arch = "x86_64")]
    {
        /* WORKAROUND for `mach_timebase_info` giving incorrect values on M1 under Rosetta 2.
         *    rdar://FB9546856 http://www.openradar.appspot.com/FB9546856
         *
         *    Rosetta 2 only supports x86-64, so skip this workaround when building for other architectures.
         */
        let isRunningUnderRosetta2 = Platform_isRunningTranslated();

        // Kernel versions >= 20.0.0 (macOS 11.0 AKA Big Sur) affected
        let isBuggedVersion = 0
            <= Platform_CompareKernelVersion(KernelVersion {
                major: 20,
                minor: 0,
                patch: 0,
            });

        if isRunningUnderRosetta2 && isBuggedVersion {
            // In this case `mach_timebase_info` provides the wrong value, so we hard-code the correct factor.
            *numer = 125;
            *denom = 3;
            return;
        }
    }

    let mut info: libc::mach_timebase_info_data_t = unsafe { zeroed() };
    if unsafe { libc::mach_timebase_info(&mut info) } == KERN_SUCCESS {
        *numer = info.numer as u64;
        *denom = info.denom as u64;
        return;
    }

    // No info on actual timebase found; assume timebase in nanoseconds.
    *numer = 1;
    *denom = 1;
}

/// Port of `uint64_t Platform_machTicksToNanoseconds(uint64_t mach_ticks)`
/// (`Platform.c:226`).
// Converts ticks in the Mach "timebase" to nanoseconds.
pub fn Platform_machTicksToNanoseconds(mach_ticks: u64) -> u64 {
    let numer = Platform_nanosecondsPerMachTickNumer.load(Ordering::Relaxed);
    let denom = Platform_nanosecondsPerMachTickDenom.load(Ordering::Relaxed);

    let ticks_quot = mach_ticks / denom;
    let ticks_rem = mach_ticks % denom;

    let part1 = ticks_quot * numer;

    // When denom * numer is less than 2^64, ticks_rem * numer will be less
    // than 2^64 as well, i.e. never overflows.
    let part2 = (ticks_rem * numer) / denom;

    part1 + part2
}

/// Port of `bool Platform_init(void)` (`Platform.c:239`).
pub fn Platform_init() -> bool {
    let mut numer: u64 = 1;
    let mut denom: u64 = 1;
    Platform_calculateNanosecondsPerMachTick(&mut numer, &mut denom);
    Platform_nanosecondsPerMachTickNumer.store(numer, Ordering::Relaxed);
    Platform_nanosecondsPerMachTickDenom.store(denom, Ordering::Relaxed);

    // Determine the number of scheduler clock ticks per second
    let scheduler_ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };

    if scheduler_ticks_per_sec < 1 {
        CRT_fatalError("Unable to retrieve clock tick rate");
    }

    let nanos_per_sec = 1e9;
    *Platform_nanosecondsPerSchedulerTick.lock().unwrap() =
        nanos_per_sec / scheduler_ticks_per_sec as f64;

    true
}

/// Port of `double Platform_schedulerTicksToNanoseconds(const double
/// scheduler_ticks)` (`Platform.c:258`).
pub fn Platform_schedulerTicksToNanoseconds(scheduler_ticks: f64) -> f64 {
    scheduler_ticks * *Platform_nanosecondsPerSchedulerTick.lock().unwrap()
}

/// Port of `void Platform_done(void)` (`Platform.c:262`).
pub fn Platform_done() {
    /* no platform-specific cleanup needed */
}

/// Port of `void Platform_setBindings(Htop_Action* keys)` (`Platform.c:266`).
pub fn Platform_setBindings() {
    /* no platform-specific key bindings */
}

/// Port of `int Platform_getUptime(void)` (`Platform.c:271`).
pub fn Platform_getUptime() -> c_int {
    let mut bootTime: libc::timeval = unsafe { zeroed() };
    let mut mib: [c_int; 2] = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    let mut size = size_of::<libc::timeval>();

    let err = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
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
    let mut currTime: libc::timeval = unsafe { zeroed() };
    unsafe { libc::gettimeofday(&mut currTime, ptr::null_mut()) };

    (currTime.tv_sec - bootTime.tv_sec) as c_int
}

/// Port of `void Platform_getLoadAverage(double* one, double* five, double*
/// fifteen)` (`Platform.c:285`).
pub fn Platform_getLoadAverage(one: &mut f64, five: &mut f64, fifteen: &mut f64) {
    let mut results = [0.0f64; 3];

    if 3 == unsafe { libc::getloadavg(results.as_mut_ptr(), 3) } {
        *one = results[0];
        *five = results[1];
        *fifteen = results[2];
    } else {
        *one = 0.0;
        *five = 0.0;
        *fifteen = 0.0;
    }
}

/// Port of `pid_t Platform_getMaxPid(void)` (`Platform.c:299`).
pub fn Platform_getMaxPid() -> libc::pid_t {
    /* http://opensource.apple.com/source/xnu/xnu-2782.1.97/bsd/sys/proc_internal.hh */
    99999
}

/// TODO: port of `static double Platform_setCPUAverageValues(Meter* mtr)` from
/// `Platform.c:304`. Blocked: `Meter::host` (`meter.rs`) is typed as the
/// concrete `LinuxMachine`; a `DarwinMachine`-backed meter is unmodeled.
pub fn Platform_setCPUAverageValues() {
    todo!("port of Platform.c:304")
}

/// TODO: port of `double Platform_setCPUValues(Meter* mtr, unsigned int cpu)`
/// from `Platform.c:323`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setCPUValues() {
    todo!("port of Platform.c:323")
}

/// TODO: port of `void Platform_setGPUValues(Meter* mtr, double* totalUsage,
/// unsigned long long* totalGPUTimeDiff)` from `Platform.c:363`. Blocked:
/// `Meter::host` typed as `LinuxMachine` + IOKit FFI.
pub fn Platform_setGPUValues() {
    todo!("port of Platform.c:363")
}

/// TODO: port of `void Platform_setMemoryValues(Meter* mtr)` from
/// `Platform.c:409`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setMemoryValues() {
    todo!("port of Platform.c:409")
}

/// TODO: port of `void Platform_setSwapValues(Meter* mtr)` from
/// `Platform.c:455`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setSwapValues() {
    todo!("port of Platform.c:455")
}

/// TODO: port of `void Platform_setZfsArcValues(Meter* this)` from
/// `Platform.c:465`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setZfsArcValues() {
    todo!("port of Platform.c:465")
}

/// TODO: port of `void Platform_setZfsCompressedArcValues(Meter* this)` from
/// `Platform.c:471`. Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setZfsCompressedArcValues() {
    todo!("port of Platform.c:471")
}

/// Port of `char* Platform_getProcessEnv(pid_t pid)` (`Platform.c:477`).
/// Returns the raw environment block (NUL-separated, double-NUL terminated)
/// as a `String`, or `None` when the process args cannot be read.
pub fn Platform_getProcessEnv(pid: libc::pid_t) -> Option<String> {
    let mut env: Option<String> = None;

    let mut argmax: c_int = 0;
    let mut bufsz = size_of::<c_int>();

    let mut mib: [c_int; 3] = [libc::CTL_KERN, libc::KERN_ARGMAX, 0];
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            &mut argmax as *mut c_int as *mut c_void,
            &mut bufsz,
            ptr::null_mut(),
            0,
        )
    } == 0
    {
        let mut buf = vec![0u8; argmax as usize];
        mib[0] = libc::CTL_KERN;
        mib[1] = libc::KERN_PROCARGS2;
        mib[2] = pid;
        bufsz = argmax as usize;
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr() as *mut c_void,
                &mut bufsz,
                ptr::null_mut(),
                0,
            )
        } == 0
        {
            if bufsz > size_of::<c_int>() {
                let endp = bufsz;
                let mut p = 0usize;
                let mut argc = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
                p += size_of::<c_int>();

                // skip exe: strchr(p, 0) + 1
                while p < endp && buf[p] != 0 {
                    p += 1;
                }
                if p < endp {
                    p += 1;
                }

                // skip padding
                while p < endp && buf[p] == 0 {
                    p += 1;
                }

                // skip argv
                while argc > 0 && p < endp {
                    argc -= 1;
                    while p < endp && buf[p] != 0 {
                        p += 1;
                    }
                    if p < endp {
                        p += 1;
                    }
                }

                // skip padding
                while p < endp && buf[p] == 0 {
                    p += 1;
                }

                let mut bytes = buf[p..endp].to_vec();
                bytes.push(0);
                bytes.push(0);
                env = Some(String::from_utf8_lossy(&bytes).into_owned());
            }
        }
    }

    env
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// from `Platform.c:528`. Blocked: `FileLocks_ProcessData` is unmodeled
/// (Darwin's body returns `NULL` unconditionally).
pub fn Platform_getProcessLocks() {
    todo!("port of Platform.c:528")
}

/// TODO: port of `void Platform_getFileDescriptors(double* used, double* max)`
/// from `Platform.c:533`. Blocked: needs `Generic_getFileDescriptors_sysctl`
/// (`generic/fdstat_sysctl.c`, unported).
pub fn Platform_getFileDescriptors() {
    todo!("port of Platform.c:533")
}

/// TODO: port of `bool Platform_getDiskIO(DiskIOData* data)` from
/// `Platform.c:537`. Blocked: needs CoreFoundation / IOKit FFI bindings
/// not yet established in this tree.
pub fn Platform_getDiskIO() {
    todo!("port of Platform.c:537")
}

/// Port of `bool Platform_getNetworkIO(NetworkIOData* data)`
/// (`Platform.c:626`).
/* Caution: Given that interfaces are dynamic, and it is not possible to get statistics on interfaces that no longer exist,
if some interface disappears between the time of two samples, the values of the second sample may be lower than those of
the first one. */
pub fn Platform_getNetworkIO(data: &mut NetworkIOData) -> bool {
    let mut mib: [c_int; 6] = [
        libc::CTL_NET,
        libc::PF_ROUTE,       /* routing messages */
        0,                    /* protocol number, currently always 0 */
        0,                    /* select all address families */
        libc::NET_RT_IFLIST2, /* interface list with addresses */
        0,
    ];

    for retry in 0..4usize {
        let mut len: usize = 0;

        /* Determine len */
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                6,
                ptr::null_mut(),
                &mut len,
                ptr::null_mut(),
                0,
            )
        } < 0
            || len == 0
        {
            return false;
        }

        len += 16 * retry * retry * size_of::<libc::if_msghdr2>();
        let mut buf = vec![0u8; len];

        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                6,
                buf.as_mut_ptr() as *mut c_void,
                &mut len,
                ptr::null_mut(),
                0,
            )
        } < 0
        {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ENOMEM) && retry < 3 {
                continue;
            } else {
                return false;
            }
        }

        let mut bytesReceived_sum: u64 = 0;
        let mut packetsReceived_sum: u64 = 0;
        let mut bytesTransmitted_sum: u64 = 0;
        let mut packetsTransmitted_sum: u64 = 0;

        let mut next = 0usize;
        while next < len {
            let ifm =
                unsafe { ptr::read_unaligned(buf.as_ptr().add(next) as *const libc::if_msghdr) };

            next += ifm.ifm_msglen as usize;

            if ifm.ifm_type as c_int != libc::RTM_IFINFO2 {
                continue;
            }

            let ifm2 = unsafe {
                ptr::read_unaligned(
                    buf.as_ptr().add(next - ifm.ifm_msglen as usize) as *const libc::if_msghdr2
                )
            };

            if ifm2.ifm_data.ifi_type != IFT_LOOP {
                /* do not count loopback traffic */
                bytesReceived_sum += ifm2.ifm_data.ifi_ibytes;
                packetsReceived_sum += ifm2.ifm_data.ifi_ipackets;
                bytesTransmitted_sum += ifm2.ifm_data.ifi_obytes;
                packetsTransmitted_sum += ifm2.ifm_data.ifi_opackets;
            }
        }

        data.bytesReceived = bytesReceived_sum;
        data.packetsReceived = packetsReceived_sum;
        data.bytesTransmitted = bytesTransmitted_sum;
        data.packetsTransmitted = packetsTransmitted_sum;
    }

    true
}

/// TODO: port of `void Platform_getBattery(double* percent, ACPresence*
/// isOnAC)` from `Platform.c:684`. Blocked: needs CoreFoundation / IOKit
/// (`IOPowerSources`) FFI bindings not yet established in this tree.
pub fn Platform_getBattery() {
    todo!("port of Platform.c:684")
}

/// Port of `void Platform_gettime_monotonic(uint64_t* msec)`
/// (`Platform.c:739`, `HAVE_HOST_GET_CLOCK_SERVICE` mach-clock branch).
// `libc::mach_host_self`/`mach_task_self` are deprecated in `libc` in favor
// of `mach2`; the C original uses these directly, so keep the libc path.
#[allow(deprecated)]
pub fn Platform_gettime_monotonic(msec: &mut u64) {
    let mut cclock: libc::mach_port_t = 0;
    let mut mts = mach_timespec_t {
        tv_sec: 0,
        tv_nsec: 0,
    };

    unsafe {
        host_get_clock_service(libc::mach_host_self(), SYSTEM_CLOCK, &mut cclock);
        clock_get_time(cclock, &mut mts);
        mach_port_deallocate(libc::mach_task_self(), cclock);
    }

    *msec = (mts.tv_sec as u64 * 1000) + (mts.tv_nsec as u64 / 1000000);
}

/// TODO: port of `static void Platform_getOSRelease(char* buffer, size_t
/// bufferLen)` from `Platform.c:760`. Blocked: needs a CoreFoundation
/// property-list reader for `SystemVersion.plist`.
pub fn Platform_getOSRelease() {
    todo!("port of Platform.c:760")
}

/// TODO: port of `const char* Platform_getRelease(void)` from
/// `Platform.c:827`. Blocked: needs `Generic_unameRelease`
/// (`generic/uname.c`, unported) + `Platform_getOSRelease`.
pub fn Platform_getRelease() {
    todo!("port of Platform.c:827")
}
