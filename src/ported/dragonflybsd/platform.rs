//! Port of `dragonflybsd/Platform.c` — the DragonFly BSD platform hooks.
//!
//! Ported: the pure [`Platform_signals`] table (the kill-menu signal list) and
//! [`Platform_numberOfSignals`]. The platform functions (`Platform_getUptime`,
//! `Platform_getLoadAverage`, `Platform_setCPUValues`, battery/disk/net, …)
//! read `sysctl`/`kvm`, which exist only on DragonFly BSD; they are faithful
//! `todo!()` stubs (named after the C functions so the port gate accepts the
//! module) to be ported behind `#[cfg(target_os = "dragonfly")]`.
//!
//! `Platform_meterTypes[]` (the `const MeterClass*[]` of available meters) is
//! deferred: it references meter-class statics (`MemoryMeter_class`,
//! `SwapMeter_class`, the `*CPUsMeter_class` family, …) that are not yet ported,
//! and the array cannot be built until they exist.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::signalspanel::SignalItem;

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

/// TODO: port of `bool Platform_init(void)` (`Platform.c:149`). Trivial
/// (`return true`) but paired with the DragonFly-only platform layer.
pub fn Platform_init() {
    todo!("port of dragonflybsd/Platform.c:149 (DragonFly-only platform layer)")
}

/// TODO: port of `void Platform_done(void)` (`Platform.c:154`).
pub fn Platform_done() {
    todo!("port of dragonflybsd/Platform.c:154 (DragonFly-only platform layer)")
}

/// TODO: port of `void Platform_setBindings(Htop_Action* keys)`
/// (`Platform.c:158`). Needs the `Htop_Action` dispatch table (unported).
pub fn Platform_setBindings() {
    todo!("port of dragonflybsd/Platform.c:158 — needs Htop_Action table")
}

/// TODO: port of `int Platform_getUptime(void)` (`Platform.c:163`). Reads
/// `kern.boottime` via sysctl. DragonFly-only.
pub fn Platform_getUptime() {
    todo!("port of dragonflybsd/Platform.c:163 — kern.boottime sysctl (DragonFly-only)")
}

/// TODO: port of `void Platform_getLoadAverage(double* one, double* five,
/// double* fifteen)` (`Platform.c:177`). `getloadavg`/`vm.loadavg` sysctl.
pub fn Platform_getLoadAverage() {
    todo!("port of dragonflybsd/Platform.c:177 — vm.loadavg sysctl (DragonFly-only)")
}

/// TODO: port of `pid_t Platform_getMaxPid(void)` (`Platform.c:194`). Reads
/// `kern.pid_max` via sysctl. DragonFly-only.
pub fn Platform_getMaxPid() {
    todo!("port of dragonflybsd/Platform.c:194 — kern.pid_max sysctl (DragonFly-only)")
}

/// TODO: port of `double Platform_setCPUValues(Meter* this, unsigned int cpu)`
/// (`Platform.c:204`). Fills the CPU meter from the DragonFly `CPUData`.
pub fn Platform_setCPUValues() {
    todo!("port of dragonflybsd/Platform.c:204 — DragonFlyBSDMachine CPUData (DragonFly-only)")
}

/// TODO: port of `void Platform_setMemoryValues(Meter* this)`
/// (`Platform.c:240`). Fills the memory meter from `DragonFlyBSDMachine`.
pub fn Platform_setMemoryValues() {
    todo!("port of dragonflybsd/Platform.c:240 — DragonFlyBSDMachine memory (DragonFly-only)")
}

/// TODO: port of `void Platform_setSwapValues(Meter* this)`
/// (`Platform.c:258`). Fills the swap meter from `Machine`.
pub fn Platform_setSwapValues() {
    todo!("port of dragonflybsd/Platform.c:258 — DragonFlyBSDMachine swap (DragonFly-only)")
}

/// TODO: port of `char* Platform_getProcessEnv(pid_t pid)` (`Platform.c:264`).
/// `kvm_getenvv`. DragonFly-only.
pub fn Platform_getProcessEnv() {
    todo!("port of dragonflybsd/Platform.c:264 — kvm_getenvv (DragonFly-only)")
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// (`Platform.c:270`). Unsupported on DragonFly (returns `NULL`), but needs
/// the `FileLocks_ProcessData` type (unported).
pub fn Platform_getProcessLocks() {
    todo!("port of dragonflybsd/Platform.c:270 — needs FileLocks_ProcessData type")
}

/// TODO: port of `void Platform_getFileDescriptors(double* used, double* max)`
/// (`Platform.c:275`). `kern.openfiles`/`kern.maxfiles` sysctl. DragonFly-only.
pub fn Platform_getFileDescriptors() {
    todo!("port of dragonflybsd/Platform.c:275 — kern.openfiles sysctl (DragonFly-only)")
}

/// TODO: port of `bool Platform_getDiskIO(DiskIOData* data)`
/// (`Platform.c:279`). `kern.disks` + `devstat` sysctl. DragonFly-only.
pub fn Platform_getDiskIO() {
    todo!("port of dragonflybsd/Platform.c:279 — devstat sysctl (DragonFly-only)")
}

/// TODO: port of `bool Platform_getNetworkIO(NetworkIOData* data)`
/// (`Platform.c:340`). `net.route`/`ifmib` sysctl. DragonFly-only.
pub fn Platform_getNetworkIO() {
    todo!("port of dragonflybsd/Platform.c:340 — ifmib sysctl (DragonFly-only)")
}

/// TODO: port of `void Platform_getBattery(double* percent, ACPresence*
/// isOnAC)` (`Platform.c:366`). `hw.acpi.battery`/`hw.acpi.acline` sysctl.
/// DragonFly-only.
pub fn Platform_getBattery() {
    todo!("port of dragonflybsd/Platform.c:366 — hw.acpi sysctl (DragonFly-only)")
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
