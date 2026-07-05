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
//! - `Platform_getOSRelease` (`Platform.c:760`) — CoreFoundation
//!   `SystemVersion.plist` reader (ProductName + ProductVersion)
//! - `Platform_getRelease` (`Platform.c:827`) — `Generic_unameRelease`
//!   ([`crate::ported::generic::uname`]) with the CF OS-release fetch
//! - `Platform_getDiskIO` (`Platform.c:537`) — IOKit `IOBlockStorageDriver`
//!   statistics summed across all drives
//! - `Platform_getBattery` (`Platform.c:684`) — IOKit `IOPowerSources`
//!   internal-source capacity + AC state
//!
//! Still `todo!()` and blocked on unported substrate:
//! - the `Platform_set*Values` meter setters — `Meter::host` (`meter.rs`)
//!   is typed as the concrete `LinuxMachine`, so a `DarwinMachine`-backed
//!   meter cannot be modeled until `meter.rs` generalizes that field.
//! - `Platform_getFileDescriptors` — needs `Generic_getFileDescriptors_sysctl`
//!   (`generic/fdstat_sysctl.c`, unported).
//! - `Platform_getProcessLocks` — `FileLocks_ProcessData` is unmodeled
//!   (C returns `NULL` unconditionally on Darwin).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
// The IOKit FFI aliases (`io_object_t`, `kern_return_t`, …) keep their C type
// names, as the fn/const names do.
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::mem::{size_of, zeroed};
use std::os::raw::{c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::ported::crt::{CRT_fatalError, ColorElements};
use crate::ported::linux::platform::MemoryClass;
use crate::ported::signalspanel::SignalItem;
// Used only by the `#[cfg(target_arch = "x86_64")]` Rosetta workaround below.
use crate::ported::darwin::darwinmachine::DarwinMachine;
#[cfg(target_arch = "x86_64")]
use crate::ported::darwin::platformhelpers::{
    KernelVersion, Platform_CompareKernelVersion, Platform_isRunningTranslated,
};
// `Machine` is named only by the meter-setter tests now (the setters read the
// typed `mtr.host` and cast to `*DarwinMachine`).
use crate::ported::batterymeter::BatteryMeter_class;
use crate::ported::cpumeter::{
    AllCPUs2Meter_class, AllCPUs4Meter_class, AllCPUs8Meter_class, AllCPUsMeter_class,
    CPUMeter_class, LeftCPUs2Meter_class, LeftCPUs4Meter_class, LeftCPUs8Meter_class,
    LeftCPUsMeter_class, RightCPUs2Meter_class, RightCPUs4Meter_class, RightCPUs8Meter_class,
    RightCPUsMeter_class,
};
use crate::ported::datetimemeter::{ClockMeter_class, DateMeter_class, DateTimeMeter_class};
use crate::ported::hostnamemeter::HostnameMeter_class;
use crate::ported::loadaveragemeter::{LoadAverageMeter_class, LoadMeter_class};
#[cfg(test)]
use crate::ported::machine::Machine;
use crate::ported::memorymeter::MemoryMeter_class;
use crate::ported::meter::{BlankMeter_class, Meter, MeterClass};
use crate::ported::networkiometer::NetworkIOData;
use crate::ported::swapmeter::SwapMeter_class;
use crate::ported::sysarchmeter::SysArchMeter_class;
use crate::ported::tasksmeter::TasksMeter_class;
use crate::ported::uptimemeter::{SecondsUptimeMeter_class, UptimeMeter_class};
use crate::ported::xutils::saturatingSub;

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
/// Darwin's C body is `(void) keys;` — no platform-specific bindings — so the
/// `keys` table is accepted and left untouched.
pub fn Platform_setBindings(_keys: &mut [Option<crate::ported::action::Htop_Action>]) {
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

/// Port of `const SignalItem Platform_signals[]` (`darwin/Platform.c:77`) —
/// the signal picker table for `actionKill`. Transcribed verbatim from the C
/// designated initializer (Darwin has no real-time signals, so no `SIGRTMIN`
/// rows). `Platform_numberOfSignals` is the slice length.
pub static Platform_signals: &[SignalItem] = &[
    SignalItem {
        name: " 0 Cancel",
        number: 0,
    },
    SignalItem {
        name: " 1 SIGHUP",
        number: 1,
    },
    SignalItem {
        name: " 2 SIGINT",
        number: 2,
    },
    SignalItem {
        name: " 3 SIGQUIT",
        number: 3,
    },
    SignalItem {
        name: " 4 SIGILL",
        number: 4,
    },
    SignalItem {
        name: " 5 SIGTRAP",
        number: 5,
    },
    SignalItem {
        name: " 6 SIGABRT",
        number: 6,
    },
    SignalItem {
        name: " 6 SIGIOT",
        number: 6,
    },
    SignalItem {
        name: " 7 SIGEMT",
        number: 7,
    },
    SignalItem {
        name: " 8 SIGFPE",
        number: 8,
    },
    SignalItem {
        name: " 9 SIGKILL",
        number: 9,
    },
    SignalItem {
        name: "10 SIGBUS",
        number: 10,
    },
    SignalItem {
        name: "11 SIGSEGV",
        number: 11,
    },
    SignalItem {
        name: "12 SIGSYS",
        number: 12,
    },
    SignalItem {
        name: "13 SIGPIPE",
        number: 13,
    },
    SignalItem {
        name: "14 SIGALRM",
        number: 14,
    },
    SignalItem {
        name: "15 SIGTERM",
        number: 15,
    },
    SignalItem {
        name: "16 SIGURG",
        number: 16,
    },
    SignalItem {
        name: "17 SIGSTOP",
        number: 17,
    },
    SignalItem {
        name: "18 SIGTSTP",
        number: 18,
    },
    SignalItem {
        name: "19 SIGCONT",
        number: 19,
    },
    SignalItem {
        name: "20 SIGCHLD",
        number: 20,
    },
    SignalItem {
        name: "21 SIGTTIN",
        number: 21,
    },
    SignalItem {
        name: "22 SIGTTOU",
        number: 22,
    },
    SignalItem {
        name: "23 SIGIO",
        number: 23,
    },
    SignalItem {
        name: "24 SIGXCPU",
        number: 24,
    },
    SignalItem {
        name: "25 SIGXFSZ",
        number: 25,
    },
    SignalItem {
        name: "26 SIGVTALRM",
        number: 26,
    },
    SignalItem {
        name: "27 SIGPROF",
        number: 27,
    },
    SignalItem {
        name: "28 SIGWINCH",
        number: 28,
    },
    SignalItem {
        name: "29 SIGINFO",
        number: 29,
    },
    SignalItem {
        name: "30 SIGUSR1",
        number: 30,
    },
    SignalItem {
        name: "31 SIGUSR2",
        number: 31,
    },
];

/// Port of `const unsigned int Platform_numberOfSignals`
/// (`darwin/Platform.c:113`) — `ARRAYSIZE(Platform_signals)`.
pub const Platform_numberOfSignals: usize = Platform_signals.len();

/// Port of `pid_t Platform_getMaxPid(void)` (`Platform.c:299`).
pub fn Platform_getMaxPid() -> libc::pid_t {
    /* http://opensource.apple.com/source/xnu/xnu-2782.1.97/bsd/sys/proc_internal.hh */
    99999
}

// Darwin's `CPU_METER_*` indices (`CPUMeter.h`) into `Meter::values`.
const CPU_METER_NICE: usize = 0;
const CPU_METER_NORMAL: usize = 1;
const CPU_METER_KERNEL: usize = 2;
const CPU_METER_FREQUENCY: usize = 8;
const CPU_METER_TEMPERATURE: usize = 9;

/// Port of `static double Platform_setCPUAverageValues(Meter* mtr)` from
/// `Platform.c:304`. Averages the per-CPU nice/normal/kernel values (and the
/// summed percentage) across `existingCPUs`, dividing by `activeCPUs`.
///
/// Bridge: `host` is passed explicitly until `Meter::host` is flipped to
/// `*const Machine` (the C reads `mtr->host`).
fn Platform_setCPUAverageValues(mtr: &mut Meter) -> f64 {
    let host = mtr.host;
    let (active_cpus, existing_cpus) = unsafe { ((*host).activeCPUs, (*host).existingCPUs) };

    let mut sum_nice = 0.0;
    let mut sum_normal = 0.0;
    let mut sum_kernel = 0.0;
    let mut sum_percent = 0.0;
    for i in 1..=existing_cpus {
        sum_percent += Platform_setCPUValues(mtr, i);
        sum_nice += mtr.values[CPU_METER_NICE];
        sum_normal += mtr.values[CPU_METER_NORMAL];
        sum_kernel += mtr.values[CPU_METER_KERNEL];
    }

    mtr.values[CPU_METER_NICE] = sum_nice / active_cpus as f64;
    mtr.values[CPU_METER_NORMAL] = sum_normal / active_cpus as f64;
    mtr.values[CPU_METER_KERNEL] = sum_kernel / active_cpus as f64;
    sum_percent / active_cpus as f64
}

/// Port of `double Platform_setCPUValues(Meter* mtr, unsigned int cpu)` from
/// `Platform.c:323`. For `cpu == 0` delegates to
/// `Platform_setCPUAverageValues`; otherwise computes the nice/normal/
/// kernel percentages for CPU `cpu` from the `curr_load - prev_load`
/// cpu-tick deltas, sets frequency/temperature to `NAN`, and returns the
/// clamped total usage. `host` is the bridge param (see the average fn).
pub fn Platform_setCPUValues(mtr: &mut Meter, cpu: u32) -> f64 {
    if cpu == 0 {
        return Platform_setCPUAverageValues(mtr);
    }

    let dhost = mtr.host as *const DarwinMachine;
    let (prev, curr) = unsafe {
        (
            &*(*dhost).prev_load.add((cpu - 1) as usize),
            &*(*dhost).curr_load.add((cpu - 1) as usize),
        )
    };

    // Sum of all cpu-state tick deltas.
    let mut total = 0.0;
    for i in 0..libc::CPU_STATE_MAX as usize {
        total += curr.cpu_ticks[i] as f64 - prev.cpu_ticks[i] as f64;
    }

    let delta = |state: c_int| {
        curr.cpu_ticks[state as usize] as f64 - prev.cpu_ticks[state as usize] as f64
    };
    if total > 1e-6 {
        mtr.values[CPU_METER_NICE] = delta(libc::CPU_STATE_NICE) * 100.0 / total;
        mtr.values[CPU_METER_NORMAL] = delta(libc::CPU_STATE_USER) * 100.0 / total;
        mtr.values[CPU_METER_KERNEL] = delta(libc::CPU_STATE_SYSTEM) * 100.0 / total;
    } else {
        mtr.values[CPU_METER_NICE] = 0.0;
        mtr.values[CPU_METER_NORMAL] = 0.0;
        mtr.values[CPU_METER_KERNEL] = 0.0;
    }

    mtr.curItems = 3;

    let total_pct =
        mtr.values[CPU_METER_NICE] + mtr.values[CPU_METER_NORMAL] + mtr.values[CPU_METER_KERNEL];

    mtr.values[CPU_METER_FREQUENCY] = f64::NAN;
    mtr.values[CPU_METER_TEMPERATURE] = f64::NAN;

    total_pct.clamp(0.0, 100.0)
}

/// TODO: port of `void Platform_setGPUValues(Meter* mtr, double* totalUsage,
/// unsigned long long* totalGPUTimeDiff)` from `Platform.c:363`. Blocked:
/// `Meter::host` typed as `LinuxMachine` + IOKit FFI.
pub fn Platform_setGPUValues() {
    todo!("port of Platform.c:363")
}

/// Port of `const MemoryClass Platform_memoryClasses[]`
/// (`darwin/Platform.c:125`), in `MEMORY_CLASS_*` index order — the darwin
/// 6-class breakdown the memory meter's display iterates.
#[allow(non_upper_case_globals)] // faithful C global name
/// Port of `const MeterClass* const Platform_meterTypes[]` from
/// `darwin/Platform.c`. The C array is `NULL`-terminated and iterated as
/// `for (const MeterClass* const* type = Platform_meterTypes; *type; type++)`;
/// here it is a slice, so its length replaces the sentinel and the loop is a
/// plain `.iter()`.
///
/// Only the meter classes whose `MeterClass` static is ported appear — the
/// table grows as those statics land. Currently ported: `BlankMeter`.
/// Pending, in the C order: `CPU`, `Clock`, `Date`, `DateTime`,
/// `LoadAverage`, `Load`, `Memory`, `Swap`, `MemorySwap`, `Tasks`,
/// `Battery`, `Hostname`, `SysArch`, `Uptime`, `SecondsUptime`,
/// `AllCPUs{,2,4,8}`, `{Left,Right}CPUs{,2,4,8}`, `ZfsArc`,
/// `ZfsCompressedArc`, `DiskIO{Rate,Time,}`, `NetworkIO`, `FileDescriptor`,
/// `GPU`. Each is blocked only on defining its `MeterClass` static (the
/// `updateValues`/`display` fns are ported for several already).
///
/// Ported entries are listed in their C-array positions relative to each
/// other; `BlankMeter` is last in the C array too.
pub static Platform_meterTypes: &[&MeterClass] = &[
    &CPUMeter_class,
    &ClockMeter_class,
    &DateMeter_class,
    &DateTimeMeter_class,
    &LoadAverageMeter_class,
    &LoadMeter_class,
    &MemoryMeter_class,
    &SwapMeter_class,
    &TasksMeter_class,
    &BatteryMeter_class,
    &HostnameMeter_class,
    &SysArchMeter_class,
    &UptimeMeter_class,
    &SecondsUptimeMeter_class,
    &AllCPUsMeter_class,
    &AllCPUs2Meter_class,
    &AllCPUs4Meter_class,
    &AllCPUs8Meter_class,
    &LeftCPUsMeter_class,
    &RightCPUsMeter_class,
    &LeftCPUs2Meter_class,
    &RightCPUs2Meter_class,
    &LeftCPUs4Meter_class,
    &RightCPUs4Meter_class,
    &LeftCPUs8Meter_class,
    &RightCPUs8Meter_class,
    &BlankMeter_class,
];

pub static Platform_memoryClasses: [MemoryClass; 6] = [
    MemoryClass {
        label: "wired",
        countsAsUsed: true,
        countsAsCache: false,
        color: ColorElements::MEMORY_1,
    },
    MemoryClass {
        label: "speculative",
        countsAsUsed: true,
        countsAsCache: true,
        color: ColorElements::MEMORY_2,
    },
    MemoryClass {
        label: "active",
        countsAsUsed: true,
        countsAsCache: false,
        color: ColorElements::MEMORY_3,
    },
    MemoryClass {
        label: "purgeable",
        countsAsUsed: false,
        countsAsCache: true,
        color: ColorElements::MEMORY_4,
    },
    MemoryClass {
        label: "compressed",
        countsAsUsed: true,
        countsAsCache: false,
        color: ColorElements::MEMORY_5,
    },
    MemoryClass {
        label: "inactive",
        countsAsUsed: true,
        countsAsCache: true,
        color: ColorElements::MEMORY_6,
    },
];

/// Port of `const unsigned int Platform_numberOfMemoryClasses`
/// (`darwin/Platform.c:132`) — `ARRAYSIZE(Platform_memoryClasses)`.
#[allow(non_upper_case_globals)] // faithful C global name
pub const Platform_numberOfMemoryClasses: usize = Platform_memoryClasses.len();

// Darwin's `MEMORY_CLASS_*` enum (`darwin/Platform.c:116`) — indices into
// `Meter::values`, in this exact order.
const MEMORY_CLASS_WIRED: usize = 0;
const MEMORY_CLASS_SPECULATIVE: usize = 1;
const MEMORY_CLASS_ACTIVE: usize = 2;
const MEMORY_CLASS_PURGEABLE: usize = 3;
const MEMORY_CLASS_COMPRESSED: usize = 4;
const MEMORY_CLASS_INACTIVE: usize = 5;

/// Port of `void Platform_setMemoryValues(Meter* mtr)` from `Platform.c:409`
/// (`HAVE_STRUCT_VM_STATISTICS64` branch). Fills the memory meter's class
/// values in kB from the host's `vm_statistics64`: wired/active/inactive/
/// speculative/purgeable/compressed page counts scaled by the page size,
/// with `showCachedMemory` selecting the active/speculative split.
/// `saturatingSub` guards the macOS underflow the C comments describe.
///
/// Bridge: the C reads the `DarwinMachine` from `mtr->host`; until
/// `Meter::host` is flipped to `*const Machine`, the host is passed
/// explicitly. The body (downcast + compute) is the final form.
pub fn Platform_setMemoryValues(mtr: &mut Meter) {
    let host = mtr.host;
    let dhost = host as *const DarwinMachine;
    let page_k = unsafe { libc::vm_page_size } as f64 / 1024.0;

    let vm = unsafe { &(*dhost).vm_stats };
    let external_page_count = vm.external_page_count;
    let compressor_page_count = vm.compressor_page_count;

    let show_cached = unsafe {
        (*host)
            .settings
            .as_ref()
            .is_some_and(|s| s.showCachedMemory)
    };

    mtr.total = (unsafe { (*dhost).host_info.max_mem } / 1024) as f64;
    mtr.values[MEMORY_CLASS_WIRED] = page_k * vm.wire_count as f64;

    if show_cached {
        mtr.values[MEMORY_CLASS_SPECULATIVE] = page_k * vm.speculative_count as f64;
        mtr.values[MEMORY_CLASS_ACTIVE] = page_k
            * saturatingSub(
                vm.active_count as u64,
                vm.purgeable_count as u64 + external_page_count as u64,
            ) as f64;
        mtr.values[MEMORY_CLASS_PURGEABLE] = page_k * vm.purgeable_count as f64;
    } else {
        mtr.values[MEMORY_CLASS_SPECULATIVE] = 0.0;
        mtr.values[MEMORY_CLASS_ACTIVE] = page_k
            * saturatingSub(
                vm.speculative_count as u64 + vm.active_count as u64,
                external_page_count as u64,
            ) as f64;
        mtr.values[MEMORY_CLASS_PURGEABLE] = 0.0;
    }
    mtr.values[MEMORY_CLASS_COMPRESSED] = page_k * compressor_page_count as f64;
    // macOS counts inactive pages in the "used" memory.
    mtr.values[MEMORY_CLASS_INACTIVE] = page_k * vm.inactive_count as f64;
}

/// Port of `void Platform_setSwapValues(Meter* mtr)` from `Platform.c:455`.
/// Reads swap totals via `sysctl(CTL_VM, VM_SWAPUSAGE)` — no host access —
/// and fills the swap meter's total and used values (kB).
pub fn Platform_setSwapValues(mtr: &mut Meter) {
    /// `SWAP_METER_USED = 0` (`SwapMeter.h`).
    const SWAP_METER_USED: usize = 0;

    let mut mib: [c_int; 2] = [libc::CTL_VM, libc::VM_SWAPUSAGE];
    let mut swapused: libc::xsw_usage = unsafe { zeroed() };
    let mut swlen = size_of::<libc::xsw_usage>();
    unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            &mut swapused as *mut libc::xsw_usage as *mut c_void,
            &mut swlen,
            ptr::null_mut(),
            0,
        );
    }

    mtr.total = (swapused.xsu_total / 1024) as f64;
    mtr.values[SWAP_METER_USED] = (swapused.xsu_used / 1024) as f64;
}

/// Port of `void Platform_setZfsArcValues(Meter* this)` from `Platform.c:465`.
/// Casts the host to the concrete [`DarwinMachine`] and hands its `zfs` snapshot
/// to [`ZfsArcMeter_readStats`](crate::ported::zfsarcmeter::ZfsArcMeter_readStats).
pub fn Platform_setZfsArcValues(this: &mut Meter) {
    let dhost = unsafe { &*(this.host as *const DarwinMachine) };

    crate::ported::zfsarcmeter::ZfsArcMeter_readStats(this, &dhost.zfs);
}

/// Port of `void Platform_setZfsCompressedArcValues(Meter* this)` from
/// `Platform.c:471`. Casts the host to the concrete [`DarwinMachine`] and hands
/// its `zfs` snapshot to [`ZfsCompressedArcMeter_readStats`](crate::ported::zfscompressedarcmeter::ZfsCompressedArcMeter_readStats).
pub fn Platform_setZfsCompressedArcValues(this: &mut Meter) {
    let dhost = unsafe { &*(this.host as *const DarwinMachine) };

    crate::ported::zfscompressedarcmeter::ZfsCompressedArcMeter_readStats(this, &dhost.zfs);
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
            && bufsz > size_of::<c_int>()
        {
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

    env
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// from `Platform.c:528`. Blocked: `FileLocks_ProcessData` is unmodeled
/// (Darwin's body returns `NULL` unconditionally).
/// Port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)` from
/// `darwin/Platform.c:528`. Darwin does not expose per-process file locks, so
/// the C body is `(void)pid; return NULL;` — the faithful analog returns
/// `None`, and `ProcessLocksScreen_scan` renders "not supported".
pub fn Platform_getProcessLocks(
    pid: libc::pid_t,
) -> Option<crate::ported::processlocksscreen::FileLocks_ProcessData> {
    let _ = pid;
    None
}

/// Port of `void Platform_getFileDescriptors(double* used, double* max)`
/// (`Platform.c:533`) — delegates to the shared `Generic_getFileDescriptors_sysctl`.
pub fn Platform_getFileDescriptors(used: &mut f64, max: &mut f64) {
    crate::ported::generic::fdstat_sysctl::Generic_getFileDescriptors_sysctl(used, max);
}

/// Port of `bool Platform_getDiskIO(DiskIOData* data)` from `Platform.c:537`.
///
/// Sums the read/write byte and time counters across every `IOBlockStorageDriver`
/// in the IOKit registry. `iokit_port` is a file-scope `static mach_port_t` the
/// C never initializes (zero-filled BSS), so it is `MACH_PORT_NULL` (`0`) — the
/// default main port that `IOServiceGetMatchingServices` accepts. Returns
/// `false` on the two hard-failure paths (matching-service lookup fails, or a
/// per-drive `IORegistryEntryCreateCFProperties` fails); a drive with no
/// properties / no statistics dictionary is skipped, as in the C.
pub fn Platform_getDiskIO(data: &mut crate::ported::diskiometer::DiskIOData) -> bool {
    // static mach_port_t iokit_port; — zero-initialized BSS == MACH_PORT_NULL.
    const IOKIT_PORT: libc::mach_port_t = 0;

    unsafe {
        let mut drive_list: io_iterator_t = 0;
        // if (IOServiceGetMatchingServices(iokit_port, IOServiceMatching(
        //        "IOBlockStorageDriver"), &drive_list)) return false;
        if IOServiceGetMatchingServices(
            IOKIT_PORT,
            IOServiceMatching(c"IOBlockStorageDriver".as_ptr()),
            &mut drive_list,
        ) != 0
        {
            return false;
        }

        let mut read_sum: u64 = 0;
        let mut write_sum: u64 = 0;
        let mut time_spend_sum: u64 = 0;
        let mut num_disks: u64 = 0;

        // Reads a SInt64 statistic keyed by `key` from `statistics`, adding it
        // to `acc` when present — the repeated C `CFDictionaryGetValue` +
        // `CFNumberGetValue(..., kCFNumberSInt64Type, ...)` block. A closure
        // (the C inlines it four times), like `Generic_unameRelease`'s `field`.
        let add_stat = |statistics: CFDictionaryRef, key: &std::ffi::CStr, acc: &mut u64| {
            let cfkey = CFStringCreateWithCString(ptr::null(), key.as_ptr(), kCFStringEncodingUTF8);
            let number = CFDictionaryGetValue(statistics, cfkey);
            if !cfkey.is_null() {
                CFRelease(cfkey);
            }
            if !number.is_null() {
                let mut value: u64 = 0;
                CFNumberGetValue(
                    number,
                    kCFNumberSInt64Type,
                    &mut value as *mut u64 as *mut c_void,
                );
                *acc += value;
            }
        };

        loop {
            // while ((drive = IOIteratorNext(drive_list)) != 0)
            let drive = IOIteratorNext(drive_list);
            if drive == 0 {
                break;
            }

            let mut properties: CFMutableDictionaryRef = ptr::null();
            // if (IORegistryEntryCreateCFProperties(drive, &properties, ...))
            //    { IOObjectRelease(drive); IOObjectRelease(drive_list); return false; }
            if IORegistryEntryCreateCFProperties(drive, &mut properties, ptr::null(), 0) != 0 {
                IOObjectRelease(drive);
                IOObjectRelease(drive_list);
                return false;
            }
            if properties.is_null() {
                IOObjectRelease(drive);
                continue;
            }

            // statistics = CFDictionaryGetValue(properties, "Statistics");
            let stat_key = CFStringCreateWithCString(
                ptr::null(),
                c"Statistics".as_ptr(),
                kCFStringEncodingUTF8,
            );
            let statistics = CFDictionaryGetValue(properties, stat_key);
            if !stat_key.is_null() {
                CFRelease(stat_key);
            }
            if statistics.is_null() {
                CFRelease(properties);
                IOObjectRelease(drive);
                continue;
            }

            num_disks += 1;

            add_stat(statistics, c"Bytes (Read)", &mut read_sum);
            add_stat(statistics, c"Bytes (Write)", &mut write_sum);
            add_stat(statistics, c"Total Time (Read)", &mut time_spend_sum);
            add_stat(statistics, c"Total Time (Write)", &mut time_spend_sum);

            CFRelease(properties);
            IOObjectRelease(drive);
        }

        data.totalBytesRead = read_sum;
        data.totalBytesWritten = write_sum;
        // C: totalMsTimeSpend = timeSpend_sum / 1e6 (ns -> ms), truncated to u64.
        data.totalMsTimeSpend = (time_spend_sum as f64 / 1e6) as u64;
        data.numDisks = num_disks;

        if drive_list != 0 {
            IOObjectRelease(drive_list);
        }

        true
    }
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

/// Port of `void Platform_getBattery(double* percent, ACPresence* isOnAC)`
/// from `Platform.c:684`.
///
/// Averages the internal power sources' `Current Capacity` / `Max Capacity`
/// into a battery percentage and reports AC presence. Starts at `NAN` /
/// `AC_ERROR`; only internal (`kIOPSInternalType`) sources contribute. The C
/// passes possibly-`NULL` `CFStringRef`s straight to `CFStringCompare` /
/// `CFNumberGetValue`; the port guards each `NULL` (a missing key skips that
/// source's field rather than dereferencing `NULL`), which cannot change the
/// result on real power-source dictionaries where the keys are present.
pub fn Platform_getBattery(
    percent: &mut f64,
    isOnAC: &mut crate::ported::batterymeter::ACPresence,
) {
    use crate::ported::batterymeter::ACPresence;

    *percent = f64::NAN;
    *isOnAC = ACPresence::AC_ERROR;

    unsafe {
        let power_sources = IOPSCopyPowerSourcesInfo();
        if power_sources.is_null() {
            return;
        }

        let list = IOPSCopyPowerSourcesList(power_sources);
        if list.is_null() {
            CFRelease(power_sources);
            return;
        }

        // Builds a CFString from a C string literal, `CFStringCompare`s it
        // against `s`, and equals-checks — the C `kCFCompareEqualTo ==
        // CFStringCompare(s, CFSTR(lit), 0)` idiom. Returns false when `s` is
        // NULL (missing key). A closure (inlined twice in the C).
        let str_eq = |s: CFStringRef, lit: &std::ffi::CStr| -> bool {
            if s.is_null() {
                return false;
            }
            let cflit = CFStringCreateWithCString(ptr::null(), lit.as_ptr(), kCFStringEncodingUTF8);
            let eq = CFStringCompare(s, cflit, 0) == kCFCompareEqualTo;
            if !cflit.is_null() {
                CFRelease(cflit);
            }
            eq
        };

        // Reads a Double keyed by `key` from `power_source`, returning 0.0 when
        // the key is absent — the C `CFNumberGetValue(CFDictionaryGetValue(...,
        // key), kCFNumberDoubleType, &tmp)` block.
        let get_double = |power_source: CFDictionaryRef, key: &std::ffi::CStr| -> f64 {
            let cfkey = CFStringCreateWithCString(ptr::null(), key.as_ptr(), kCFStringEncodingUTF8);
            let number = CFDictionaryGetValue(power_source, cfkey);
            if !cfkey.is_null() {
                CFRelease(cfkey);
            }
            let mut tmp: f64 = 0.0;
            if !number.is_null() {
                CFNumberGetValue(
                    number,
                    kCFNumberDoubleType,
                    &mut tmp as *mut f64 as *mut c_void,
                );
            }
            tmp
        };

        let mut cap_current = 0.0;
        let mut cap_max = 0.0;

        let len = CFArrayGetCount(list);
        for i in 0..len {
            let power_source =
                IOPSGetPowerSourceDescription(power_sources, CFArrayGetValueAtIndex(list, i));
            if power_source.is_null() {
                continue;
            }

            // power_type must be the internal battery type, else skip.
            let key_transport = CFStringCreateWithCString(
                ptr::null(),
                c"Transport Type".as_ptr(),
                kCFStringEncodingUTF8,
            );
            let power_type = CFDictionaryGetValue(power_source, key_transport);
            if !key_transport.is_null() {
                CFRelease(key_transport);
            }
            if !str_eq(power_type, c"Internal") {
                continue;
            }

            // Determine the AC state (once AC_PRESENT, keep it).
            if *isOnAC != ACPresence::AC_PRESENT {
                let key_state = CFStringCreateWithCString(
                    ptr::null(),
                    c"Power Source State".as_ptr(),
                    kCFStringEncodingUTF8,
                );
                let power_state = CFDictionaryGetValue(power_source, key_state);
                if !key_state.is_null() {
                    CFRelease(key_state);
                }
                *isOnAC = if str_eq(power_state, c"AC Power") {
                    ACPresence::AC_PRESENT
                } else {
                    ACPresence::AC_ABSENT
                };
            }

            cap_current += get_double(power_source, c"Current Capacity");
            cap_max += get_double(power_source, c"Max Capacity");
        }

        if cap_max > 0.0 {
            *percent = 100.0 * cap_current / cap_max;
        }

        CFRelease(list);
        CFRelease(power_sources);
    }
}

/// Port of `static inline void Platform_gettime_realtime(struct timeval*
/// tv, uint64_t* msec)` (darwin `Platform.h:106`), which forwards to
/// `Generic_gettime_realtime` (`generic/gettime.c`). macOS provides
/// `clock_gettime(CLOCK_REALTIME, ...)` (10.12+), so the `HAVE_CLOCK_GETTIME`
/// branch is faithful: on success fill `tv` (µs-truncated) and `msec`;
/// on failure zero both.
pub fn Platform_gettime_realtime(tv: &mut libc::timeval, msec: &mut u64) {
    let mut ts: libc::timespec = unsafe { zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts) } == 0 {
        tv.tv_sec = ts.tv_sec;
        tv.tv_usec = (ts.tv_nsec / 1000) as libc::suseconds_t;
        *msec = (ts.tv_sec as u64 * 1000) + (ts.tv_nsec as u64 / 1_000_000);
    } else {
        *tv = unsafe { zeroed() };
        *msec = 0;
    }
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

// CoreFoundation FFI — the minimal surface `Platform_getOSRelease` needs to
// read `SystemVersion.plist`. Types are the LP64 aliases from `CFBase.h`
// (`CFTypeID`/`CFOptionFlags` = `unsigned long`, `CFIndex` = `signed long`,
// `CFStringEncoding` = `UInt32`, `Boolean` = `unsigned char`); every opaque
// `CF*Ref` is a `const void*`. Values verified against the macOS SDK headers:
// `kCFStringEncodingUTF8 = 0x08000100` (`CFString.h`), `kCFURLPOSIXPathStyle =
// 0` (`CFURL.h`), `kCFPropertyListImmutable = 0` (`CFPropertyList.h`).
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFURLRef = *const c_void;
type CFReadStreamRef = *const c_void;
type CFPropertyListRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFTypeID = libc::c_ulong;
type CFOptionFlags = libc::c_ulong;
type CFIndex = libc::c_long;
type CFStringEncoding = u32;
type CFBoolean = libc::c_uchar;
// Additional CF aliases used by the IOKit disk/battery readers.
type CFNumberRef = *const c_void;
type CFArrayRef = *const c_void;
type CFMutableDictionaryRef = *const c_void;
type CFNumberType = CFIndex;
type CFComparisonResult = CFIndex;

const kCFStringEncodingUTF8: CFStringEncoding = 0x0800_0100;
const kCFURLPOSIXPathStyle: CFIndex = 0;
const kCFPropertyListImmutable: CFOptionFlags = 0;
// `CFNumber.h`: kCFNumberSInt64Type = 4, kCFNumberDoubleType = 13.
const kCFNumberSInt64Type: CFNumberType = 4;
const kCFNumberDoubleType: CFNumberType = 13;
// `CFBase.h`: kCFCompareEqualTo = 0.
const kCFCompareEqualTo: CFComparisonResult = 0;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        c_str: *const libc::c_char,
        encoding: CFStringEncoding,
    ) -> CFStringRef;
    fn CFStringGetCString(
        the_string: CFStringRef,
        buffer: *mut libc::c_char,
        buffer_size: CFIndex,
        encoding: CFStringEncoding,
    ) -> CFBoolean;
    fn CFURLCreateWithFileSystemPath(
        allocator: CFAllocatorRef,
        file_path: CFStringRef,
        path_style: CFIndex,
        is_directory: CFBoolean,
    ) -> CFURLRef;
    fn CFReadStreamCreateWithFile(alloc: CFAllocatorRef, file_url: CFURLRef) -> CFReadStreamRef;
    fn CFReadStreamOpen(stream: CFReadStreamRef) -> CFBoolean;
    fn CFReadStreamClose(stream: CFReadStreamRef);
    fn CFPropertyListCreateWithStream(
        allocator: CFAllocatorRef,
        stream: CFReadStreamRef,
        stream_length: CFIndex,
        options: CFOptionFlags,
        format: *mut c_void,
        error: *mut c_void,
    ) -> CFPropertyListRef;
    fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    fn CFDictionaryGetTypeID() -> CFTypeID;
    fn CFDictionaryGetValue(the_dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
    fn CFNumberGetValue(
        number: CFNumberRef,
        the_type: CFNumberType,
        value_ptr: *mut c_void,
    ) -> CFBoolean;
    fn CFArrayGetCount(the_array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(the_array: CFArrayRef, idx: CFIndex) -> *const c_void;
    fn CFStringCompare(
        s1: CFStringRef,
        s2: CFStringRef,
        compare_options: CFOptionFlags,
    ) -> CFComparisonResult;
    fn CFRelease(cf: CFTypeRef);
}

// IOKit FFI — the storage-driver statistics (`Platform_getDiskIO`) and power
// sources (`Platform_getBattery`) surface. `io_object_t`/`io_iterator_t`/
// `io_registry_entry_t` are all `mach_port_t` (`IOTypes.h`); `IOOptionBits` is
// `UInt32`; `kern_return_t` is `int`. The disk key strings are verified against
// `IOKit/storage/IOBlockStorageDriver.h`, the power-source keys against
// `IOKit/ps/IOPSKeys.h`.
type io_object_t = libc::mach_port_t;
type io_iterator_t = io_object_t;
type io_registry_entry_t = io_object_t;
type IOOptionBits = u32;
type kern_return_t = c_int;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const libc::c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        main_port: libc::mach_port_t,
        matching: CFDictionaryRef,
        existing: *mut io_iterator_t,
    ) -> kern_return_t;
    fn IOIteratorNext(iterator: io_iterator_t) -> io_object_t;
    fn IORegistryEntryCreateCFProperties(
        entry: io_registry_entry_t,
        properties: *mut CFMutableDictionaryRef,
        allocator: CFAllocatorRef,
        options: IOOptionBits,
    ) -> kern_return_t;
    fn IOObjectRelease(object: io_object_t) -> kern_return_t;
    fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
    fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
}

/// Port of `static void Platform_getOSRelease(char* buffer, size_t bufferLen)`
/// from `Platform.c:760`. Reads `/System/Library/CoreServices/SystemVersion.plist`
/// through CoreFoundation and returns `"<ProductName> <ProductVersion>"` (e.g.
/// `"macOS 15.5"`), or the empty string on any failure — the C `char*`+len
/// out-param becomes an owned `String`.
///
/// The C builds the result with `CFStringCreateWithFormat("%@%@%@", productName,
/// separator, productVersion)`; the port extracts each value with the
/// `cfstring_to_string` closure and joins them with the same separator rule
/// (a space only when *both* keys are present), avoiding a varargs FFI. The
/// `OSRELEASEFILE` test-override path is `#ifdef`-only (undefined by default),
/// so only the canonical plist path is probed.
pub fn Platform_getOSRelease() -> String {
    // static const CFStringRef osfiles[] = { "/System/.../SystemVersion.plist" };
    let osfiles = [c"/System/Library/CoreServices/SystemVersion.plist"];

    unsafe {
        // Models the C `CFStringGetCString(str, buffer, bufferLen,
        // kCFStringEncodingUTF8)`: copy a CFString into an owned Rust `String`,
        // `None` on the C `ok == false` (does-not-fit) path. A local closure
        // (the C's own inline conversion has no separate function), like
        // `Generic_unameRelease`'s `field` helper. 512 bytes comfortably holds
        // "macOS <version>".
        let cfstring_to_string = |s: CFStringRef| -> Option<String> {
            let mut buf = [0i8; 512];
            let ok = CFStringGetCString(
                s,
                buf.as_mut_ptr(),
                buf.len() as CFIndex,
                kCFStringEncodingUTF8,
            );
            if ok == 0 {
                return None;
            }
            Some(
                std::ffi::CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy()
                    .into_owned(),
            )
        };

        let mut plist: CFPropertyListRef = ptr::null();
        for path in osfiles {
            let cfpath =
                CFStringCreateWithCString(ptr::null(), path.as_ptr(), kCFStringEncodingUTF8);
            if cfpath.is_null() {
                continue;
            }
            let url = CFURLCreateWithFileSystemPath(
                ptr::null(),
                cfpath,
                kCFURLPOSIXPathStyle,
                /* isDirectory */ 0,
            );
            CFRelease(cfpath);
            if url.is_null() {
                continue;
            }

            let stream = CFReadStreamCreateWithFile(ptr::null(), url);
            CFRelease(url);
            if stream.is_null() {
                continue;
            }

            let can_read = CFReadStreamOpen(stream) != 0;
            if can_read {
                plist = CFPropertyListCreateWithStream(
                    ptr::null(),
                    stream,
                    0,
                    kCFPropertyListImmutable,
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                CFReadStreamClose(stream);
            }
            CFRelease(stream);

            if can_read {
                break;
            }
        }

        if plist.is_null() {
            return String::new();
        }

        let mut result = String::new();
        if CFGetTypeID(plist) == CFDictionaryGetTypeID() {
            let dict: CFDictionaryRef = plist;

            let key_name = CFStringCreateWithCString(
                ptr::null(),
                c"ProductName".as_ptr(),
                kCFStringEncodingUTF8,
            );
            let key_version = CFStringCreateWithCString(
                ptr::null(),
                c"ProductVersion".as_ptr(),
                kCFStringEncodingUTF8,
            );

            // CFDictionaryGetValue returns a borrowed reference (owned by the
            // dict) — do not release the returned CFStringRefs.
            let product_name = CFDictionaryGetValue(dict, key_name);
            let product_version = CFDictionaryGetValue(dict, key_version);

            if !key_name.is_null() {
                CFRelease(key_name);
            }
            if !key_version.is_null() {
                CFRelease(key_version);
            }

            // C: separator is " " only when BOTH pointers are non-NULL;
            // each missing value defaults to the empty string.
            let name = if product_name.is_null() {
                String::new()
            } else {
                cfstring_to_string(product_name).unwrap_or_default()
            };
            let version = if product_version.is_null() {
                String::new()
            } else {
                cfstring_to_string(product_version).unwrap_or_default()
            };
            let sep = if !product_name.is_null() && !product_version.is_null() {
                " "
            } else {
                ""
            };
            result = format!("{name}{sep}{version}");
        }
        CFRelease(plist);

        result
    }
}

/// Port of `const char* Platform_getRelease(void)` from `Platform.c:827`:
/// `return Generic_unameRelease(Platform_getOSRelease);`. Darwin supplies its
/// CoreFoundation-backed [`Platform_getOSRelease`] as the OS-release fetch,
/// where Linux uses `parseOSRelease` (see [`crate::ported::generic::uname`]).
pub fn Platform_getRelease() -> &'static str {
    crate::ported::generic::uname::Generic_unameRelease(Platform_getOSRelease)
}

/// Port of `darwin/Platform.h:112`. Non-PCP build: no dynamic meters, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicMeters() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `darwin/Platform.h:116`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-meter table.
pub fn Platform_dynamicMetersDone(_table: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `darwin/Platform.h:118`. `ATTR_UNUSED` no-op meter init.
pub fn Platform_dynamicMeterInit(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `darwin/Platform.h:120`. `ATTR_UNUSED` no-op value update.
pub fn Platform_dynamicMeterUpdateValues(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `darwin/Platform.h:122`. `ATTR_UNUSED` no-op display.
pub fn Platform_dynamicMeterDisplay(
    _meter: &crate::ported::meter::Meter,
    _out: &mut crate::ported::richstring::RichString,
) {
}

/// Port of `darwin/Platform.h:138`. Non-PCP build: no dynamic screens, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicScreens() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `darwin/Platform.h:148`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-screen table.
pub fn Platform_dynamicScreensDone(_screens: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `darwin/Platform.h:146`. `ATTR_UNUSED` no-op — non-PCP builds add
/// no dynamic-screen columns.
pub fn Platform_addDynamicScreenAvailableColumns(
    _availableColumns: &mut crate::ported::panel::Panel,
    _screen: &str,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn getOSRelease_reads_systemversion_plist() {
        // The CoreFoundation plist reader must return "<ProductName>
        // <ProductVersion>" from /System/Library/CoreServices/SystemVersion.plist.
        // Cross-check against `sw_vers`, which reads the same keys — this pins
        // the CF FFI ABI (a mismatched CFIndex/Boolean width would corrupt the
        // read and this would fail).
        let out = std::process::Command::new("sw_vers")
            .output()
            .expect("sw_vers");
        let text = String::from_utf8_lossy(&out.stdout);
        let field = |k: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(k))
                .map(|v| v.trim())
                .unwrap_or("")
                .to_string()
        };
        let expected = format!("{} {}", field("ProductName:"), field("ProductVersion:"));

        let got = Platform_getOSRelease();
        assert_eq!(got, expected);
        assert!(got.starts_with("macOS "));
    }

    #[test]
    fn getRelease_embeds_os_release_via_generic_uname() {
        // Platform_getRelease = Generic_unameRelease(Platform_getOSRelease):
        // "<sysname> <release> [<machine>] @ <os-release>". On macOS uname's
        // sysname is "Darwin", so the CF distro string is appended (not
        // already contained).
        let release = Platform_getRelease();
        assert!(release.starts_with("Darwin "));
        assert!(release.contains('['));
        assert!(release.contains(" @ macOS "));
    }

    #[test]
    fn getDiskIO_reads_ioblockstoragedriver_stats() {
        // Every running Mac has at least one IOBlockStorageDriver and has read
        // bytes from it since boot. A wrong statistics key string (or a broken
        // CFNumber/IOKit ABI) would leave the sums at zero, failing this.
        let mut data = crate::ported::diskiometer::DiskIOData::default();
        let ok = Platform_getDiskIO(&mut data);
        assert!(ok, "Platform_getDiskIO returned false");
        assert!(data.numDisks >= 1, "no disks enumerated");
        assert!(data.totalBytesRead > 0, "no bytes read tallied");
    }

    #[test]
    fn getBattery_matches_pmset_ground_truth() {
        use crate::ported::batterymeter::ACPresence;

        let mut percent = 0.0;
        let mut on_ac = ACPresence::AC_ABSENT;
        Platform_getBattery(&mut percent, &mut on_ac);

        // `pmset -g batt` is the OS's own view of the same IOPS power sources.
        let out = std::process::Command::new("pmset")
            .args(["-g", "batt"])
            .output()
            .expect("pmset");
        let text = String::from_utf8_lossy(&out.stdout);
        let has_internal_battery = text.contains("InternalBattery");

        if has_internal_battery {
            // A present internal source => a real percentage and a resolved AC
            // state (never AC_ERROR, which is the no-source sentinel).
            assert_ne!(on_ac, ACPresence::AC_ERROR, "battery present but AC_ERROR");
            assert!(percent.is_finite(), "percent not finite: {percent}");
            assert!(
                (0.0..=100.0).contains(&percent),
                "percent out of range: {percent}"
            );
        } else {
            // No power sources => the NAN / AC_ERROR start state is unchanged.
            assert!(percent.is_nan(), "no battery but percent = {percent}");
            assert_eq!(on_ac, ACPresence::AC_ERROR);
        }
    }

    #[test]
    fn setSwapValues_reads_system_swap() {
        let mut m = Meter::empty();
        m.values = vec![0.0; 3];

        Platform_setSwapValues(&mut m);

        // Swap totals are non-negative and used never exceeds total.
        assert!(m.total >= 0.0);
        assert!(m.values[0] >= 0.0);
        assert!(m.values[0] <= m.total);
    }

    #[test]
    fn setMemoryValues_reads_vm_stats_from_host() {
        use crate::ported::darwin::darwinmachine::{DarwinMachine_freeCPULoadInfo, Machine_new};
        use crate::ported::machine::{ScreenSettings, Settings};

        // A real host (fills vm_stats + host_info via mach), with settings.
        let mut dm = Machine_new(None, 0);
        dm.super_.settings = Some(Settings {
            showCachedMemory: true,
            screens: vec![ScreenSettings::default()],
            ..Default::default()
        });

        let mut m = Meter::empty();
        m.values = vec![0.0; 6];
        m.host = &dm.super_ as *const Machine;

        Platform_setMemoryValues(&mut m);

        // Total is physical memory in kB; wired pages always exist.
        assert!(m.total > 0.0);
        assert!(m.values[MEMORY_CLASS_WIRED] > 0.0);
        assert!(m.values.iter().all(|&v| v >= 0.0));

        DarwinMachine_freeCPULoadInfo(&mut dm.prev_load);
        DarwinMachine_freeCPULoadInfo(&mut dm.curr_load);
    }

    #[test]
    fn setCPUValues_computes_percentages_from_load_deltas() {
        use crate::ported::darwin::darwinmachine::{DarwinMachine_freeCPULoadInfo, Machine_new};

        let mut dm = Machine_new(None, 0);

        let mut m = Meter::empty();
        m.values = vec![0.0; 10]; // through CPU_METER_TEMPERATURE (9)
        m.host = &dm.super_ as *const Machine;

        // cpu == 0 → the average across all CPUs.
        let avg = Platform_setCPUValues(&mut m, 0);
        assert!((0.0..=100.0).contains(&avg));
        assert!(m.values[CPU_METER_NICE].is_finite());
        assert!(m.values[CPU_METER_NORMAL].is_finite());
        assert!(m.values[CPU_METER_KERNEL].is_finite());
        assert!(m.values[CPU_METER_FREQUENCY].is_nan());

        // A specific CPU also yields a valid clamped percentage.
        let one = Platform_setCPUValues(&mut m, 1);
        assert!((0.0..=100.0).contains(&one));
        assert_eq!(m.curItems, 3);

        DarwinMachine_freeCPULoadInfo(&mut dm.prev_load);
        DarwinMachine_freeCPULoadInfo(&mut dm.curr_load);
    }

    #[test]
    fn gettime_realtime_fills_tv_and_msec_consistently() {
        let mut tv: libc::timeval = unsafe { zeroed() };
        let mut msec: u64 = 0;

        Platform_gettime_realtime(&mut tv, &mut msec);

        // The realtime clock is well past the epoch, so both are populated.
        assert!(msec > 0);
        assert!(tv.tv_sec > 0);
        // µs field is a truncated sub-second remainder.
        assert!(tv.tv_usec >= 0 && (tv.tv_usec as i64) < 1_000_000);
        // msec and tv agree to whole-second granularity (C derives both from
        // the same timespec): floor(msec/1000) == tv_sec.
        assert_eq!((msec / 1000) as i64, tv.tv_sec as i64);
    }
}
