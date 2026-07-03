//! Port of `dragonflybsd/Platform.c` — the DragonFly BSD platform hooks.
//!
//! Ported here (self-contained: only `libc` / Rust std and the already-ported
//! [`Platform_signals`] table, `NetworkIOData` / `ACPresence`):
//! - `Platform_init` (`Platform.c:149`)
//! - `Platform_done` (`Platform.c:154`)
//! - `Platform_setBindings` (`Platform.c:158`)
//! - `Platform_getUptime` (`Platform.c:163`)
//! - `Platform_getLoadAverage` (`Platform.c:177`)
//! - `Platform_getMaxPid` (`Platform.c:194`)
//! - `Platform_getProcessEnv` (`Platform.c:264`)
//! - `Platform_getNetworkIO` (`Platform.c:340`)
//! - `Platform_getBattery` (`Platform.c:366`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - the `Platform_set*Values` meter setters — `Meter::host` (`meter.rs`) is
//!   typed as the concrete `LinuxMachine`, so a `DragonFlyBSDMachine`-backed
//!   meter is unmodeled.
//! - `Platform_getProcessLocks` — `FileLocks_ProcessData` is unmodeled
//!   (DragonFly's body returns `NULL` unconditionally).
//! - `Platform_getFileDescriptors` — needs `Generic_getFileDescriptors_sysctl`
//!   (`generic/fdstat_sysctl.c`, unported).
//! - `Platform_getDiskIO` — needs `libdevstat` (`getdevs` / `selectdevs`) FFI
//!   bindings.
//!
//! `Platform_meterTypes[]` (the `const MeterClass*[]` of available meters) is
//! deferred: it references meter-class statics (`MemoryMeter_class`,
//! `SwapMeter_class`, the `*CPUsMeter_class` family, …) that are not yet ported,
//! and the array cannot be built until they exist.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::mem::size_of;
use std::os::raw::{c_int, c_uint, c_void};
use std::ptr;

use crate::ported::batterymeter::ACPresence;
use crate::ported::networkiometer::NetworkIOData;
use crate::ported::signalspanel::SignalItem;

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

/// Port of `const SignalItem Platform_signals[]` (`Platform.c:55`) — the
/// DragonFly BSD signal list shown in the kill menu. Names and numbers are
/// verbatim from the C table (leading-space padding is significant for the
/// menu's fixed-width display).
pub static Platform_signals: [SignalItem; 34] = [
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
    SignalItem {
        name: "32 SIGTHR",
        number: 32,
    },
    SignalItem {
        name: "33 SIGLIBRT",
        number: 33,
    },
];

/// Port of `const unsigned int Platform_numberOfSignals =
/// ARRAYSIZE(Platform_signals)` (`Platform.c:92`).
pub const Platform_numberOfSignals: u32 = Platform_signals.len() as u32;

/// Port of `bool Platform_init(void)` (`Platform.c:149`).
pub fn Platform_init() -> bool {
    /* no platform-specific setup needed */
    true
}

/// Port of `void Platform_done(void)` (`Platform.c:154`).
pub fn Platform_done() {
    /* no platform-specific cleanup needed */
}

/// Port of `void Platform_setBindings(Htop_Action* keys)` (`Platform.c:158`).
pub fn Platform_setBindings() {
    /* no platform-specific key bindings */
}

/// Port of `int Platform_getUptime(void)` (`Platform.c:163`). Reads
/// `kern.boottime` via sysctl and diffs against the current time.
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
/// fifteen)` (`Platform.c:177`). Reads the `vm.loadavg` sysctl.
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

/// Port of `pid_t Platform_getMaxPid(void)` (`Platform.c:194`). Reads
/// `kern.pid_max` via sysctl.
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
        return 999999;
    }
    maxPid
}

/// TODO: port of `double Platform_setCPUValues(Meter* this, unsigned int cpu)`
/// (`Platform.c:204`). Blocked: `Meter::host` typed as `LinuxMachine`; a
/// `DragonFlyBSDMachine`-backed meter (its `CPUData`) is unmodeled.
pub fn Platform_setCPUValues() {
    todo!("port of dragonflybsd/Platform.c:204 — Meter::host typed as LinuxMachine")
}

/// TODO: port of `void Platform_setMemoryValues(Meter* this)`
/// (`Platform.c:240`). Blocked: `Meter::host` typed as `LinuxMachine`; the
/// `DragonFlyBSDMachine` memory partitions are unmodeled.
pub fn Platform_setMemoryValues() {
    todo!("port of dragonflybsd/Platform.c:240 — Meter::host typed as LinuxMachine")
}

/// TODO: port of `void Platform_setSwapValues(Meter* this)`
/// (`Platform.c:258`). Blocked: `Meter::host` typed as `LinuxMachine`.
pub fn Platform_setSwapValues() {
    todo!("port of dragonflybsd/Platform.c:258 — Meter::host typed as LinuxMachine")
}

/// Port of `char* Platform_getProcessEnv(pid_t pid)` (`Platform.c:264`).
/// DragonFly's body is an unimplemented `// TODO` that returns `NULL`; the
/// faithful port returns `None`.
pub fn Platform_getProcessEnv(pid: libc::pid_t) -> Option<String> {
    // TODO (as in C)
    let _ = pid; // prevent unused warning
    None
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// (`Platform.c:270`). Blocked: `FileLocks_ProcessData` is unmodeled
/// (DragonFly's body returns `NULL` unconditionally).
pub fn Platform_getProcessLocks() {
    todo!("port of dragonflybsd/Platform.c:270 — needs FileLocks_ProcessData type")
}

/// TODO: port of `void Platform_getFileDescriptors(double* used, double* max)`
/// (`Platform.c:275`). Blocked: needs `Generic_getFileDescriptors_sysctl`
/// (`generic/fdstat_sysctl.c`, unported).
pub fn Platform_getFileDescriptors() {
    todo!("port of dragonflybsd/Platform.c:275 — needs Generic_getFileDescriptors_sysctl")
}

/// TODO: port of `bool Platform_getDiskIO(DiskIOData* data)`
/// (`Platform.c:279`). Blocked: needs `libdevstat` (`getdevs` / `selectdevs` /
/// `getdevs`) FFI bindings.
pub fn Platform_getDiskIO() {
    todo!("port of dragonflybsd/Platform.c:279 — needs libdevstat (getdevs/selectdevs) FFI")
}

/// Port of `bool Platform_getNetworkIO(NetworkIOData* data)`
/// (`Platform.c:340`). Walks `getifaddrs`, accumulating non-loopback link-layer
/// interface counters onto the caller-zeroed `data`, matching the C `+=`
/// aggregation.
pub fn Platform_getNetworkIO(data: &mut NetworkIOData) -> bool {
    let mut ifaddrs: *mut libc::ifaddrs = ptr::null_mut();

    if unsafe { libc::getifaddrs(&mut ifaddrs) } != 0 {
        return false;
    }

    let mut ifa = ifaddrs;
    while !ifa.is_null() {
        let cur = unsafe { &*ifa };
        ifa = cur.ifa_next;

        if cur.ifa_addr.is_null() {
            continue;
        }
        if unsafe { (*cur.ifa_addr).sa_family } as c_int != libc::AF_LINK {
            continue;
        }
        if cur.ifa_flags & libc::IFF_LOOPBACK as c_uint != 0 {
            continue;
        }

        let ifd = cur.ifa_data as *const libc::if_data;
        if ifd.is_null() {
            continue;
        }
        let ifd = unsafe { &*ifd };

        data.bytesReceived += ifd.ifi_ibytes as u64;
        data.packetsReceived += ifd.ifi_ipackets as u64;
        data.bytesTransmitted += ifd.ifi_obytes as u64;
        data.packetsTransmitted += ifd.ifi_opackets as u64;
    }

    unsafe { libc::freeifaddrs(ifaddrs) };
    true
}

/// Port of `void Platform_getBattery(double* percent, ACPresence* isOnAC)`
/// (`Platform.c:366`). Reads `hw.acpi.battery.life` / `hw.acpi.acline` via
/// sysctl.
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

/// Port of `dragonflybsd/Platform.h:104`. Non-PCP build: no dynamic meters, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicMeters() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `dragonflybsd/Platform.h:108`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-meter table.
pub fn Platform_dynamicMetersDone(_table: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `dragonflybsd/Platform.h:110`. `ATTR_UNUSED` no-op meter init.
pub fn Platform_dynamicMeterInit(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `dragonflybsd/Platform.h:112`. `ATTR_UNUSED` no-op value update.
pub fn Platform_dynamicMeterUpdateValues(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `dragonflybsd/Platform.h:114`. `ATTR_UNUSED` no-op display.
pub fn Platform_dynamicMeterDisplay(
    _meter: &crate::ported::meter::Meter,
    _out: &mut crate::ported::richstring::RichString,
) {
}

/// Port of `dragonflybsd/Platform.h:130`. Non-PCP build: no dynamic screens, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicScreens() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `dragonflybsd/Platform.h:140`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-screen table.
pub fn Platform_dynamicScreensDone(_screens: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `dragonflybsd/Platform.h:138`. `ATTR_UNUSED` no-op — non-PCP builds add
/// no dynamic-screen columns.
pub fn Platform_addDynamicScreenAvailableColumns(
    _availableColumns: &mut crate::ported::panel::Panel,
    _screen: &str,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The DragonFly signal table transcribes `Platform.c` exactly: 34
    /// entries, `number == index`, and the padded names.
    #[test]
    fn signal_table_matches_c() {
        assert_eq!(Platform_signals.len(), 34);
        assert_eq!(Platform_numberOfSignals, 34);
        for (i, s) in Platform_signals.iter().enumerate() {
            assert_eq!(s.number, i as i32);
        }
        assert_eq!(Platform_signals[0].name, " 0 Cancel");
        assert_eq!(Platform_signals[9].name, " 9 SIGKILL");
        assert_eq!(Platform_signals[15].name, "15 SIGTERM");
        assert_eq!(Platform_signals[33].name, "33 SIGLIBRT");
    }
}
