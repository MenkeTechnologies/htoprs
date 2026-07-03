//! Partial port of `linux/Platform.c` — htop's Linux platform hooks.
//!
//! Ported here (self-contained: only libc / Rust std / the already-ported
//! `Compat_*` file readers / `String_*` helpers / `ACPresence`):
//! - `Platform_getPressureStall` (`Platform.c:643`)
//! - `Platform_getProcessEnv` (`Platform.c:519`)
//! - `Platform_getUptime` (`Platform.c:283`)
//! - `Platform_getLoadAverage` (`Platform.c:302`)
//! - `Platform_getMaxPid` (`Platform.c:325`)
//! - `Platform_getFileDescriptors` (`Platform.c:661`)
//! - `Platform_Battery_getProcBatInfo` (`Platform.c:764`)
//! - `procAcpiCheck` (`Platform.c:827`)
//! - `Platform_Battery_getProcData` (`Platform.c:836`)
//! - `Platform_Battery_getSysData` (`Platform.c:845`, `HAVE_OPENAT` build)
//! - `Platform_getBattery` (`Platform.c:964`)
//! - `Platform_longOptionsUsage` (`Platform.c:994`, non-`HAVE_LIBCAP` build)
//! - `Platform_done` (`Platform.c:1171`, non-`HAVE_SENSORS` build)
//! - `Platform_init` (`Platform.c:1129`)
//!
//! Everything else is still `todo!()` and blocked on unported substrate —
//! chiefly the meter setters needing `Meter::host` (unmodeled field, owned by
//! `meter.rs`) plus the `LinuxMachine` memory/zfs/zswap/zram/gpu accessors,
//! and the panel/action/lock types (`DiskIOData`, `NetworkIOData`,
//! `FileLocks_*`, `CommandLineStatus`, `State`, `MainPanel`, …) owned by other
//! files. `HAVE_LIBCAP`-only functions (`dropCapabilities`, the
//! `Platform_getLongOption`/`longOptionsUsage` capability branches) are the
//! mutually-exclusive alternative build and are not ported (rule 3).
#![allow(non_snake_case)]
#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};
use std::ffi::{CStr, CString};
use std::sync::Mutex;

use crate::ported::batterymeter::ACPresence;
use crate::ported::crt::ColorElements;
use crate::ported::linux::compat::{Compat_openatArgClose, Compat_readfile, Compat_readfileat};
use crate::ported::meter::Meter;
use crate::ported::xutils::{
    saturatingSub, String_contains_i, String_eq, String_startsWith, sumPositiveValues,
};

/// Port of `typedef struct MemoryClass_` (`linux/Platform.h`) — one
/// memory-meter class: its label, whether it counts toward the "used" or
/// "cache" totals, and its `CRT_colors` element.
pub struct MemoryClass {
    pub label: &'static str,
    pub countsAsUsed: bool,
    pub countsAsCache: bool,
    pub color: ColorElements,
}

// `MEMORY_CLASS_*` indices (`linux/Platform.h`).
pub const MEMORY_CLASS_USED: usize = 0;
pub const MEMORY_CLASS_SHARED: usize = 1;
pub const MEMORY_CLASS_COMPRESSED: usize = 2;
pub const MEMORY_CLASS_BUFFERS: usize = 3;
pub const MEMORY_CLASS_CACHE: usize = 4;
pub const MEMORY_CLASS_AVAILABLE: usize = 5;

/// Port of `const MemoryClass Platform_memoryClasses[]`
/// (`linux/Platform.c:152`), in `MEMORY_CLASS_*` index order.
#[allow(non_upper_case_globals)] // faithful C global name
pub static Platform_memoryClasses: [MemoryClass; 6] = [
    MemoryClass { label: "used", countsAsUsed: true, countsAsCache: false, color: ColorElements::MEMORY_1 },
    MemoryClass { label: "shared", countsAsUsed: true, countsAsCache: false, color: ColorElements::MEMORY_2 },
    MemoryClass { label: "compressed", countsAsUsed: true, countsAsCache: false, color: ColorElements::MEMORY_3 },
    MemoryClass { label: "buffers", countsAsUsed: false, countsAsCache: true, color: ColorElements::MEMORY_4 },
    MemoryClass { label: "cache", countsAsUsed: false, countsAsCache: true, color: ColorElements::MEMORY_5 },
    MemoryClass { label: "available", countsAsUsed: false, countsAsCache: false, color: ColorElements::MEMORY_6 },
];

/// Port of `const unsigned int Platform_numberOfMemoryClasses`
/// (`linux/Platform.c:161`) — `ARRAYSIZE(Platform_memoryClasses)`.
#[allow(non_upper_case_globals)] // faithful C global name
pub const Platform_numberOfMemoryClasses: usize = Platform_memoryClasses.len();

/// `PROCDIR` — the procfs mount htop was compiled to read (a `config.h`
/// macro, default `"/proc"`). Defined locally so this module's `/proc`
/// reads reproduce the C string-literal concatenations verbatim.
const PROCDIR: &str = "/proc";

/// `PROC_BATTERY_DIR` — `PROCDIR "/acpi/battery"` (`Platform.c:755`).
const PROC_BATTERY_DIR: &str = "/proc/acpi/battery";
/// `PROC_POWERSUPPLY_ACSTATE_FILE` — `PROC_POWERSUPPLY_DIR "/AC/state"`,
/// i.e. `PROCDIR "/acpi/ac_adapter/AC/state"` (`Platform.c:757`).
const PROC_POWERSUPPLY_ACSTATE_FILE: &str = "/proc/acpi/ac_adapter/AC/state";
/// `SYS_POWERSUPPLY_DIR` — sysfs power-supply class dir (`Platform.c:758`).
const SYS_POWERSUPPLY_DIR: &str = "/sys/class/power_supply";

/// `O_PATH` — `Platform.c:74-76` declares it (`010000000`) for ancient
/// glibc / platforms whose libc omits the flag. Modeled as a local const so
/// the `openat` in [`Platform_Battery_getSysData`] compiles wherever the
/// ported tree is built.
const O_PATH: libc::c_int = 0o10000000;

/// Port of the global `bool Running_containerized` from `Platform.c:87`.
/// Set by [`Platform_init`] when htop detects it is running inside a
/// container. A mutable process-global C `bool`, modeled as an
/// [`AtomicBool`] per the global-mutable-static idiom (rule 4).
#[allow(non_upper_case_globals)] // faithful port of C global `Running_containerized`
pub static Running_containerized: AtomicBool = AtomicBool::new(false);

/// TODO: port of `static Htop_Reaction Platform_actionSetIOPriority(State* st` from `Platform.c:172`.
pub fn Platform_actionSetIOPriority() {
    todo!("port of Platform.c:172")
}

/// TODO: port of `static bool Platform_changeAutogroupPriority(MainPanel* panel, int delta` from `Platform.c:194`.
pub fn Platform_changeAutogroupPriority() {
    todo!("port of Platform.c:194")
}

/// TODO: port of `static Htop_Reaction Platform_actionHigherAutogroupPriority(State* st` from `Platform.c:206`.
pub fn Platform_actionHigherAutogroupPriority() {
    todo!("port of Platform.c:206")
}

/// TODO: port of `static Htop_Reaction Platform_actionLowerAutogroupPriority(State* st` from `Platform.c:214`.
pub fn Platform_actionLowerAutogroupPriority() {
    todo!("port of Platform.c:214")
}

/// TODO: port of `void Platform_setBindings(Htop_Action* keys` from `Platform.c:222`.
pub fn Platform_setBindings() {
    todo!("port of Platform.c:222")
}

/// Port of `int Platform_getUptime(void)` from `Platform.c:283`. Reads
/// `PROCDIR/uptime` via [`Compat_readfile`] and returns `floor(uptime)`
/// (the first of the two whitespace-separated doubles), or `0` on any read
/// or parse failure — mirroring the C `sscanf("%lf %lf")` needing 2 fields.
pub fn Platform_getUptime() -> i32 {
    let mut uptimedata = [0u8; 64];
    let path = CString::new(format!("{}/uptime", PROCDIR)).unwrap();
    let uptimeread = Compat_readfile(&path, &mut uptimedata);
    if uptimeread < 1 {
        return 0;
    }

    let text = String::from_utf8_lossy(&uptimedata[..uptimeread as usize]);
    let mut tokens = text.split_whitespace();
    let uptime: Option<f64> = tokens.next().and_then(|t| t.parse().ok());
    let idle: Option<f64> = tokens.next().and_then(|t| t.parse().ok());
    // C: sscanf must fill both (`n != 2` → return 0).
    match (uptime, idle) {
        (Some(uptime), Some(_idle)) => uptime.floor() as i32,
        _ => 0,
    }
}

/// Port of `void Platform_getLoadAverage(double* one, double* five, double* fifteen)`
/// from `Platform.c:302`. Reads `PROCDIR/loadavg`, sets the three out-params
/// to its first three doubles, or leaves them `NAN` on any read/parse
/// failure (the C `sscanf("%lf %lf %lf")` needing 3 fields).
pub fn Platform_getLoadAverage(one: &mut f64, five: &mut f64, fifteen: &mut f64) {
    *one = f64::NAN;
    *five = f64::NAN;
    *fifteen = f64::NAN;

    let mut loaddata = [0u8; 128];
    let path = CString::new(format!("{}/loadavg", PROCDIR)).unwrap();
    let loadread = Compat_readfile(&path, &mut loaddata);
    if loadread < 1 {
        return;
    }

    let text = String::from_utf8_lossy(&loaddata[..loadread as usize]);
    let mut tokens = text.split_whitespace();
    let scan_one: Option<f64> = tokens.next().and_then(|t| t.parse().ok());
    let scan_five: Option<f64> = tokens.next().and_then(|t| t.parse().ok());
    let scan_fifteen: Option<f64> = tokens.next().and_then(|t| t.parse().ok());
    if let (Some(a), Some(b), Some(c)) = (scan_one, scan_five, scan_fifteen) {
        *one = a;
        *five = b;
        *fifteen = c;
    }
}

/// Port of `pid_t Platform_getMaxPid(void)` from `Platform.c:325`. Reads
/// `PROCDIR/sys/kernel/pid_max`; on any read/parse failure returns the C
/// fallback `0x3FFFFF` (4194303).
pub fn Platform_getMaxPid() -> libc::pid_t {
    let mut piddata = [0u8; 32];
    let path = CString::new(format!("{}/sys/kernel/pid_max", PROCDIR)).unwrap();
    let pidread = Compat_readfile(&path, &mut piddata);
    if pidread < 1 {
        return 0x3FFFFF; // 4194303
    }

    let text = String::from_utf8_lossy(&piddata[..pidread as usize]);
    // C: sscanf("%32d") — first integer token.
    match text
        .split_whitespace()
        .next()
        .and_then(|t| t.parse::<i32>().ok())
    {
        Some(pidmax) => pidmax as libc::pid_t,
        None => 0x3FFFFF, // 4194303
    }
}

/// Port of `void Platform_setGPUValues(Meter* this, double* totalUsage,
/// unsigned long long* totalGPUTimeDiff)` from `linux/Platform.c`. On a new
/// monotonic sample, walks the host's per-engine GPU time list into the
/// shared [`GPUMeter_engineData`](crate::ported::gpumeter::GPUMeter_engineData)
/// rows (busy-time delta / percentage), computes the residue percentage, and
/// writes the aggregate usage/time-diff out-params. The three C
/// function-`static` caches (`prevMonotonicMs`/`residuePercentage`/
/// `prevResidueTime`) are held in a module `Mutex`. `saturatingSub` is the
/// ported `Macros.h` helper. On an unchanged sample the out-params are left
/// as-is (matching the C statics retaining their prior values), then the
/// value slots are filled from the cached rows.
pub fn Platform_setGPUValues(this: &mut Meter, total_usage: &mut f64, total_gpu_time_diff: &mut u64) {
    use crate::ported::gpumeter::GPUMeter_engineData;

    // C function-static residue caches: (prevMonotonicMs, residuePercentage,
    // prevResidueTime).
    static RESIDUE: Mutex<(u64, f64, u64)> = Mutex::new((0, 0.0, 0));
    const RESIDUE_INDEX: usize = 4; // ARRAYSIZE(GPUMeter_engineData)

    let host = this
        .host
        .as_ref()
        .expect("Platform_setGPUValues: this->host (C dereferences it)")
        .clone();
    let h = host.borrow();

    let mut r = RESIDUE.lock().unwrap();
    if h.super_.monotonicMs > r.0 {
        let monotonic_delta = (h.super_.monotonicMs - r.0) as f64;
        let mut cur_residue_time = h.curGpuTime;

        {
            let mut ed = GPUMeter_engineData.lock().unwrap();
            let mut node = h.gpuEngineData.as_deref();
            let mut i = 0;
            while let Some(g) = node {
                if i >= RESIDUE_INDEX {
                    break;
                }
                ed[i].key = g.key.clone();
                ed[i].timeDiff = saturatingSub(g.curTime, g.prevTime);
                ed[i].percentage =
                    100.0 * ed[i].timeDiff as f64 / (1000.0 * 1000.0) / monotonic_delta;
                cur_residue_time = saturatingSub(cur_residue_time, g.curTime);
                node = g.next.as_deref();
                i += 1;
            }
        }

        r.1 = 100.0 * saturatingSub(cur_residue_time, r.2) as f64 / (1000.0 * 1000.0)
            / monotonic_delta;

        *total_gpu_time_diff = saturatingSub(h.curGpuTime, h.prevGpuTime);
        *total_usage = 100.0 * *total_gpu_time_diff as f64 / (1000.0 * 1000.0) / monotonic_delta;

        r.2 = cur_residue_time;
        r.0 = h.super_.monotonicMs;
    }

    this.curItems = (RESIDUE_INDEX + 1) as u8;
    let ed = GPUMeter_engineData.lock().unwrap();
    for i in 0..RESIDUE_INDEX {
        this.values[i] = ed[i].percentage;
    }
    this.values[RESIDUE_INDEX] = r.1;
}

/// Port of `void Generic_hostname(char* buffer, size_t size)` from
/// `generic/hostname.c:15`. C fills `buffer` via `gethostname(buffer,
/// size-1)` then NUL-terminates. The port returns the hostname as an owned
/// `String` (the C `char*` out-param → return value, idiom rule 4); a
/// 256-byte scratch buffer covers `HOST_NAME_MAX`. Non-UTF-8 bytes are
/// replaced.
pub fn Generic_hostname() -> String {
    let mut buf = [0u8; 256];
    // C: gethostname(buffer, size - 1); buffer[size - 1] = '\0';
    unsafe {
        libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len() - 1);
    }
    let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..nul]).into_owned()
}

/// Port of `void Platform_getHostname(char* buffer, size_t size)` from
/// `linux/Platform.c` — a thin wrapper delegating to [`Generic_hostname`].
pub fn Platform_getHostname() -> String {
    Generic_hostname()
}

/// Port of `static void parseOSRelease(char* buffer, size_t bufferLen)` from
/// `generic/uname.c:24`. Reads `/etc/os-release` (falling back to
/// `/usr/lib/os-release`) and returns `PRETTY_NAME`, or `NAME`+`VERSION`, or
/// the empty string. The C `char*`+len out-param becomes an owned `String`.
pub fn parseOSRelease() -> String {
    for path in ["/etc/os-release", "/usr/lib/os-release"] {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let (mut name, mut version) = (String::new(), String::new());
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("PRETTY_NAME=\"") {
                // C: strrchr for the LAST quote; return immediately.
                if let Some(end) = rest.rfind('"') {
                    if end > 0 {
                        return rest[..end].to_string();
                    }
                }
            } else if let Some(rest) = line.strip_prefix("NAME=\"") {
                if let Some(end) = rest.rfind('"') {
                    if end > 0 {
                        name = rest[..end].to_string();
                    }
                }
            } else if let Some(rest) = line.strip_prefix("VERSION=\"") {
                if let Some(end) = rest.rfind('"') {
                    if end > 0 {
                        version = rest[..end].to_string();
                    }
                }
            }
        }
        // C: snprintf("%s%s%s", name, name&&version ? " " : "", version)
        let sep = if !name.is_empty() && !version.is_empty() { " " } else { "" };
        return format!("{name}{sep}{version}");
    }
    String::new()
}

/// Port of `const char* Generic_uname(void)` / `Generic_unameRelease`
/// (`generic/uname.c:88/114`). Builds `"<sysname> <release> [<machine>]"`
/// from `uname(2)`, appending ` @ <distro>` when the OS-release name is
/// present and not already contained. Cached on first call (C's
/// `static ... savedString` + `loaded_data`); the port uses a `OnceLock`.
pub fn Generic_uname() -> &'static str {
    static SAVED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SAVED.get_or_init(|| {
        // Safety: `uname` fills the whole struct; a zeroed struct is a valid
        // initial state for the out-param.
        let mut info: libc::utsname = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::uname(&mut info) };
        let distro = parseOSRelease();

        let field = |arr: &[libc::c_char]| -> String {
            unsafe { CStr::from_ptr(arr.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        };

        if result == 0 {
            let mut s = format!(
                "{} {} [{}]",
                field(&info.sysname),
                field(&info.release),
                field(&info.machine)
            );
            if !distro.is_empty() && !String_contains_i(&s, &distro, false) {
                s.push_str(&format!(" @ {distro}"));
            }
            s
        } else if !distro.is_empty() {
            distro
        } else {
            "No information".to_string()
        }
    })
}

/// Port of `const char* Platform_getRelease(void)` from `linux/Platform.c`
/// — delegates to [`Generic_uname`].
pub fn Platform_getRelease() -> &'static str {
    Generic_uname()
}

// CPUMeter.h `CPU_METER_*` indices into `Meter::values`.
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

/// Port of `double Platform_setCPUValues(Meter* this, unsigned int cpu)` from
/// `linux/Platform.c`. Fills the per-CPU-time-class percentages from
/// `lhost->cpuData[cpu]` relative to `totalPeriod`, honoring
/// `detailedCPUTime` (8-class breakdown vs 4-class summary) and
/// `accountGuestInCPUMeter`, and returns the summed active percentage
/// (capped at 100). Offline CPUs set `curItems = 0` and return `NAN`.
/// Temperature is `NAN` (no `BUILD_WITH_CPU_TEMP` in this build).
pub fn Platform_setCPUValues(this: &mut Meter, cpu: u32) -> f64 {
    let host = this
        .host
        .as_ref()
        .expect("Platform_setCPUValues: this->host (C dereferences it)")
        .clone();
    let h = host.borrow();
    let cpu_data = &h.cpuData[cpu as usize];
    let total = if cpu_data.totalPeriod == 0 {
        1.0
    } else {
        cpu_data.totalPeriod as f64
    };

    if !cpu_data.online {
        this.curItems = 0;
        return f64::NAN;
    }

    let settings = h
        .super_
        .settings
        .as_ref()
        .expect("Platform_setCPUValues: host->settings");
    let detailed = settings.detailedCPUTime;
    let account_guest = settings.accountGuestInCPUMeter;

    this.values[CPU_METER_NICE] = cpu_data.nicePeriod as f64 / total * 100.0;
    this.values[CPU_METER_NORMAL] = cpu_data.userPeriod as f64 / total * 100.0;
    if detailed {
        this.values[CPU_METER_KERNEL] = cpu_data.systemPeriod as f64 / total * 100.0;
        this.values[CPU_METER_IRQ] = cpu_data.irqPeriod as f64 / total * 100.0;
        this.values[CPU_METER_SOFTIRQ] = cpu_data.softIrqPeriod as f64 / total * 100.0;
        this.curItems = 5;

        this.values[CPU_METER_STEAL] = cpu_data.stealPeriod as f64 / total * 100.0;
        this.values[CPU_METER_GUEST] = cpu_data.guestPeriod as f64 / total * 100.0;
        if account_guest {
            this.curItems = 7;
        }

        this.values[CPU_METER_IOWAIT] = cpu_data.ioWaitPeriod as f64 / total * 100.0;
    } else {
        this.values[CPU_METER_KERNEL] = cpu_data.systemAllPeriod as f64 / total * 100.0;
        this.values[CPU_METER_IRQ] =
            (cpu_data.stealPeriod + cpu_data.guestPeriod) as f64 / total * 100.0;
        this.curItems = 4;
    }

    let percent = sumPositiveValues(&this.values[..this.curItems as usize]).min(100.0);

    if detailed {
        this.curItems = 8;
    }

    this.values[CPU_METER_FREQUENCY] = cpu_data.frequency;
    this.values[CPU_METER_TEMPERATURE] = f64::NAN;

    percent
}

/// Port of `void Platform_setMemoryValues(Meter* this)` from
/// `linux/Platform.c:441`. Fills the six memory classes from the host's
/// memory counters, then applies the ZFS-ARC shrinkable adjustment (unless
/// containerized) and the zswap compression adjustment. `this->host` is the
/// concrete [`LinuxMachine`]; `totalMem` lives on `super_`, the rest on the
/// `LinuxMachine`.
pub fn Platform_setMemoryValues(this: &mut Meter) {
    let host = this
        .host
        .as_ref()
        .expect("Platform_setMemoryValues: this->host (C dereferences it)")
        .clone();
    let h = host.borrow();

    this.total = h.super_.totalMem as f64;
    this.values[MEMORY_CLASS_USED] = h.usedMem as f64;
    this.values[MEMORY_CLASS_SHARED] = h.sharedMem as f64;
    this.values[MEMORY_CLASS_COMPRESSED] = 0.0; /* compressed */
    this.values[MEMORY_CLASS_BUFFERS] = h.buffersMem as f64;
    this.values[MEMORY_CLASS_CACHE] = h.cachedMem as f64;
    this.values[MEMORY_CLASS_AVAILABLE] = h.availableMem as f64;

    if h.zfs.enabled != 0 && !Running_containerized.load(Ordering::Relaxed) {
        // ZFS does not shrink below the value of zfs_arc_min.
        let mut shrinkable_size: u64 = 0;
        if h.zfs.size > h.zfs.min {
            shrinkable_size = h.zfs.size - h.zfs.min;
        }
        this.values[MEMORY_CLASS_USED] -= shrinkable_size as f64;
        this.values[MEMORY_CLASS_CACHE] += shrinkable_size as f64;
        this.values[MEMORY_CLASS_AVAILABLE] += shrinkable_size as f64;
    }

    if h.zswap.usedZswapOrig > 0 || h.zswap.usedZswapComp > 0 {
        this.values[MEMORY_CLASS_USED] -= h.zswap.usedZswapComp as f64;
        this.values[MEMORY_CLASS_COMPRESSED] += h.zswap.usedZswapComp as f64;
    }
}

/// Port of `void Platform_setSwapValues(Meter* this)` from
/// `linux/Platform.c:469`. Fills the swap meter's `total`/`values` from the
/// host's swap counters, then applies the zswap adjustment: zswapped pages
/// are subtracted from `USED` (overflow spilling into `CACHE`) and added to
/// `FRONTSWAP`. `this->host` is the concrete [`LinuxMachine`]; its generic
/// swap totals live on `super_`, the zswap counters on the `LinuxMachine`.
/// `SwapMeter.h` indices: `USED=0`, `CACHE=1`, `FRONTSWAP=2`.
pub fn Platform_setSwapValues(this: &mut Meter) {
    let host = this
        .host
        .as_ref()
        .expect("Platform_setSwapValues: this->host (C dereferences it)")
        .clone();
    let h = host.borrow();

    this.total = h.super_.totalSwap as f64;
    this.values[0] = h.super_.usedSwap as f64;
    this.values[1] = h.super_.cachedSwap as f64;
    this.values[2] = 0.0; // frontswap

    if h.zswap.usedZswapOrig > 0 || h.zswap.usedZswapComp > 0 {
        this.values[0] -= h.zswap.usedZswapOrig as f64;
        if this.values[0] < 0.0 {
            // subtract the overflow from SwapCached
            this.values[1] += this.values[0];
            this.values[0] = 0.0;
        }
        this.values[2] += h.zswap.usedZswapOrig as f64;
    }
}

/// Port of `void Platform_setZramValues(Meter* this)` from
/// `linux/Platform.c:499`. `total` is the zram device size; `COMPRESSED=0`
/// is the compressed pool size and `UNCOMPRESSED=1` is the extra original
/// bytes (`usedZramOrig - usedZramComp`). The scan clamps
/// `usedZramComp <= usedZramOrig`, so the subtraction never underflows;
/// `wrapping_sub` mirrors C's unsigned arithmetic for the impossible case.
pub fn Platform_setZramValues(this: &mut Meter) {
    let host = this
        .host
        .as_ref()
        .expect("Platform_setZramValues: this->host (C dereferences it)")
        .clone();
    let h = host.borrow();

    this.total = h.zram.totalZram as f64;
    this.values[0] = h.zram.usedZramComp as f64;
    this.values[1] = h.zram.usedZramOrig.wrapping_sub(h.zram.usedZramComp) as f64;
}

/// TODO: port of `void Platform_setZfsArcValues(Meter* this` from `Platform.c:507`.
pub fn Platform_setZfsArcValues() {
    todo!("port of Platform.c:507")
}

/// TODO: port of `void Platform_setZfsCompressedArcValues(Meter* this` from `Platform.c:513`.
pub fn Platform_setZfsCompressedArcValues() {
    todo!("port of Platform.c:513")
}

/// Port of `char* Platform_getProcessEnv(pid_t pid)` from `Platform.c:519`.
/// Reads `PROCDIR/<pid>/environ` (the process's NUL-separated environment
/// block) whole and returns it with two trailing NUL terminators appended,
/// exactly as the C does (`env[size] = env[size+1] = '\0'`).
///
/// Signature mapping: C `pid_t pid` → [`libc::pid_t`]; the C `char*` result
/// / `NULL` → `Option<String>` (idiom rule 4). The C grows a heap buffer in
/// 4096-byte `fread` chunks; the faithful analog reads the file whole
/// (`std::fs::read`). Any open **or** read error yields `None`, matching the
/// C returning `NULL` on `!fp` and on `ferror`/`bytes < 0`. Non-UTF-8 bytes
/// are replaced (`from_utf8_lossy`); the interior and trailing NULs are
/// valid UTF-8 and preserved for the consumer's NUL-splitting.
pub fn Platform_getProcessEnv(pid: libc::pid_t) -> Option<String> {
    let procname = format!("{}/{}/environ", PROCDIR, pid);
    let mut env = std::fs::read(&procname).ok()?;
    env.push(b'\0');
    env.push(b'\0');
    Some(String::from_utf8_lossy(&env).into_owned())
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid` from `Platform.c:555`.
pub fn Platform_getProcessLocks() {
    todo!("port of Platform.c:555")
}

/// Port of `void Platform_getPressureStall(const char* file, bool some, double* ten, double* sixty, double* threehundred)` from `Platform.c:643`.
/// Reads `PROCDIR/pressure/<file>` and returns the 10/60/300-second pressure
/// averages via the three out-params. When the file cannot be opened all
/// three become `NAN`; otherwise they hold the `some` line's `avg10/60/300`,
/// and when `some == false` the `full` line's values overwrite them —
/// reproducing the C's two sequential `fscanf` calls.
///
/// Signature mapping: C `double*` out-params → `&mut f64`; `const char*
/// file` → `&str`. The C `sscanf`/`fscanf` field extraction is done by
/// scanning whitespace tokens for the `avgN=` prefixes. The C's
/// `assert(total == 3)` becomes a `debug_assert!` on having parsed all three
/// averages of the selected line.
pub fn Platform_getPressureStall(
    file: &str,
    some: bool,
    ten: &mut f64,
    sixty: &mut f64,
    threehundred: &mut f64,
) {
    *ten = 0.0;
    *sixty = 0.0;
    *threehundred = 0.0;

    let procname = format!("{}/pressure/{}", PROCDIR, file);
    let content = match std::fs::read_to_string(&procname) {
        Ok(c) => c,
        Err(_) => {
            *ten = f64::NAN;
            *sixty = f64::NAN;
            *threehundred = f64::NAN;
            return;
        }
    };

    // Extract avg10/avg60/avg300 from a "some ..."/"full ..." line; returns
    // the three values only if all parsed (the C `fscanf` returning 3).
    let parse_line = |line: &str| -> Option<(f64, f64, f64)> {
        let mut a10: Option<f64> = None;
        let mut a60: Option<f64> = None;
        let mut a300: Option<f64> = None;
        for tok in line.split_whitespace() {
            if let Some(v) = tok.strip_prefix("avg10=") {
                a10 = v.parse().ok();
            } else if let Some(v) = tok.strip_prefix("avg60=") {
                a60 = v.parse().ok();
            } else if let Some(v) = tok.strip_prefix("avg300=") {
                a300 = v.parse().ok();
            }
        }
        match (a10, a60, a300) {
            (Some(x), Some(y), Some(z)) => Some((x, y, z)),
            _ => None,
        }
    };

    // First fscanf: the "some" line.
    let mut total = 0;
    if let Some((x, y, z)) = content
        .lines()
        .find(|l| l.starts_with("some"))
        .and_then(parse_line)
    {
        *ten = x;
        *sixty = y;
        *threehundred = z;
        total = 3;
    }

    // Second fscanf: only when caller wants the "full" line, overwriting.
    if !some {
        total = 0;
        if let Some((x, y, z)) = content
            .lines()
            .find(|l| l.starts_with("full"))
            .and_then(parse_line)
        {
            *ten = x;
            *sixty = y;
            *threehundred = z;
            total = 3;
        }
    }

    debug_assert!(total == 3);
}

/// Port of `void Platform_getFileDescriptors(double* used, double* max)` from
/// `Platform.c:661`. Reads `PROCDIR/sys/fs/file-nr` (three integers: allocated,
/// free, max). Defaults are `used = NAN`, `max = 65536`; when all three parse
/// (`sscanf` returning 3) `used` becomes the first value and `max` the third.
pub fn Platform_getFileDescriptors(used: &mut f64, max: &mut f64) {
    *used = f64::NAN;
    *max = 65536.0;

    let mut buffer = [0u8; 128];
    let path = CString::new(format!("{}/sys/fs/file-nr", PROCDIR)).unwrap();
    let fdread = Compat_readfile(&path, &mut buffer);
    if fdread < 1 {
        return;
    }

    let text = String::from_utf8_lossy(&buffer[..fdread as usize]);
    let mut tokens = text.split_whitespace();
    let v1: Option<u64> = tokens.next().and_then(|t| t.parse().ok());
    let v2: Option<u64> = tokens.next().and_then(|t| t.parse().ok());
    let v3: Option<u64> = tokens.next().and_then(|t| t.parse().ok());
    if let (Some(v1), Some(_v2), Some(v3)) = (v1, v2, v3) {
        *used = v1 as f64;
        *max = v3 as f64;
    }
}

/// TODO: port of `bool Platform_getDiskIO(DiskIOData* data` from `Platform.c:679`.
pub fn Platform_getDiskIO() {
    todo!("port of Platform.c:679")
}

/// TODO: port of `bool Platform_getNetworkIO(NetworkIOData* data` from `Platform.c:722`.
pub fn Platform_getNetworkIO() {
    todo!("port of Platform.c:722")
}

/// Port of `static double Platform_Battery_getProcBatInfo(void)` from
/// `Platform.c:764`. Sums "last full capacity" (from each `BAT*/info`) and
/// "remaining capacity" (from each `BAT*/state`) under `PROC_BATTERY_DIR`,
/// returning the percentage or `NAN` when no batteries / total full is 0.
pub fn Platform_Battery_getProcBatInfo() -> f64 {
    let batdir = CString::new(PROC_BATTERY_DIR).unwrap();
    let battery_dir = unsafe { libc::opendir(batdir.as_ptr()) };
    if battery_dir.is_null() {
        return f64::NAN;
    }

    let mut total_full: u64 = 0;
    let mut total_remain: u64 = 0;

    // `%d` conversion: skip leading whitespace, optional sign, require ≥1 digit.
    let scan_c_int = |s: &str| -> Option<i32> {
        let s = s.trim_start();
        let bytes = s.as_bytes();
        let mut end = 0;
        if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
            end += 1;
        }
        let digits_start = end;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == digits_start {
            return None;
        }
        s[..end].parse::<i32>().ok()
    };
    // sscanf(line, "%99[^:]:%d", field, &val) — both fields required (C `== 2`).
    let scan_field_colon_int = |line: &str| -> Option<(String, i32)> {
        let colon = line.find(':')?;
        if colon == 0 {
            return None; // %99[^:] matched nothing
        }
        let field = &line[..colon.min(99)];
        let val = scan_c_int(&line[colon + 1..])?;
        Some((field.to_string(), val))
    };

    loop {
        let dir_entry = unsafe { libc::readdir(battery_dir) };
        if dir_entry.is_null() {
            break;
        }
        let entry_name = unsafe { CStr::from_ptr((*dir_entry).d_name.as_ptr()) }.to_string_lossy();
        if !String_startsWith(&entry_name, "BAT") {
            continue;
        }

        let mut buf_info = [0u8; 1024];
        let info_path = CString::new(format!("{}/{}/info", PROC_BATTERY_DIR, entry_name)).unwrap();
        let r = Compat_readfile(&info_path, &mut buf_info);
        if r < 0 {
            continue;
        }
        let info = String::from_utf8_lossy(&buf_info[..r as usize]).into_owned();

        let mut buf_state = [0u8; 1024];
        let state_path =
            CString::new(format!("{}/{}/state", PROC_BATTERY_DIR, entry_name)).unwrap();
        let r = Compat_readfile(&state_path, &mut buf_state);
        if r < 0 {
            continue;
        }
        let state = String::from_utf8_lossy(&buf_state[..r as usize]).into_owned();

        // Getting total charge for all batteries
        for line in info.split('\n') {
            if let Some((field, val)) = scan_field_colon_int(line) {
                if String_eq(&field, "last full capacity") {
                    total_full += val as u64;
                    break;
                }
            }
        }

        // Getting remaining charge for all batteries
        for line in state.split('\n') {
            if let Some((field, val)) = scan_field_colon_int(line) {
                if String_eq(&field, "remaining capacity") {
                    total_remain += val as u64;
                    break;
                }
            }
        }
    }

    unsafe {
        libc::closedir(battery_dir);
    }

    if total_full > 0 {
        (total_remain as f64 * 100.0) / total_full as f64
    } else {
        f64::NAN
    }
}

/// Port of `static ACPresence procAcpiCheck(void)` from `Platform.c:827`.
/// Reads `PROC_POWERSUPPLY_ACSTATE_FILE`; returns [`ACPresence::AC_ERROR`] on
/// read failure, else [`ACPresence::AC_PRESENT`] iff the content equals
/// `"on-line"` (otherwise [`ACPresence::AC_ABSENT`]).
pub fn procAcpiCheck() -> ACPresence {
    let mut buffer = [0u8; 1024];
    let path = CString::new(PROC_POWERSUPPLY_ACSTATE_FILE).unwrap();
    let r = Compat_readfile(&path, &mut buffer);
    if r < 1 {
        return ACPresence::AC_ERROR;
    }

    let content = String::from_utf8_lossy(&buffer[..r as usize]);
    if String_eq(&content, "on-line") {
        ACPresence::AC_PRESENT
    } else {
        ACPresence::AC_ABSENT
    }
}

/// Port of `static void Platform_Battery_getProcData(double* percent, ACPresence* isOnAC)`
/// from `Platform.c:836`. Sets `isOnAC` from [`procAcpiCheck`], then `percent`
/// from [`Platform_Battery_getProcBatInfo`] unless AC state errored.
pub fn Platform_Battery_getProcData(percent: &mut f64, isOnAC: &mut ACPresence) {
    *isOnAC = procAcpiCheck();
    *percent = if *isOnAC != ACPresence::AC_ERROR {
        Platform_Battery_getProcBatInfo()
    } else {
        f64::NAN
    };
}

/// Port of `static void Platform_Battery_getSysData(double* percent, ACPresence* isOnAC)`
/// from `Platform.c:845` (the `HAVE_OPENAT` variant, matching this build's
/// [`Compat_readfileat`]/[`Compat_openatArgClose`]). Walks
/// `SYS_POWERSUPPLY_DIR`, summing battery `ENERGY/CHARGE_FULL` and
/// `ENERGY/CHARGE_NOW` (falling back to `CAPACITY` × full when no `_NOW`),
/// and reading the first mains adapter's `online` flag into `isOnAC`.
pub fn Platform_Battery_getSysData(percent: &mut f64, isOnAC: &mut ACPresence) {
    *percent = f64::NAN;
    *isOnAC = ACPresence::AC_ERROR;

    let sysdir = CString::new(SYS_POWERSUPPLY_DIR).unwrap();
    let dir = unsafe { libc::opendir(sysdir.as_ptr()) };
    if dir.is_null() {
        return;
    }

    let mut total_full: u64 = 0;
    let mut total_remain: u64 = 0;

    // `%d` conversion: skip leading whitespace, optional sign, require ≥1 digit.
    let scan_c_int = |s: &str| -> Option<i32> {
        let s = s.trim_start();
        let bytes = s.as_bytes();
        let mut end = 0;
        if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
            end += 1;
        }
        let digits_start = end;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == digits_start {
            return None;
        }
        s[..end].parse::<i32>().ok()
    };

    // AC / BAT mirror the sysfs entry-name prefixes ("AC*", "BAT*"); keep them.
    #[allow(clippy::upper_case_acronyms)]
    #[derive(PartialEq)]
    enum EntryType {
        AC,
        BAT,
    }

    loop {
        let dir_entry = unsafe { libc::readdir(dir) };
        if dir_entry.is_null() {
            break;
        }
        let entry_name_c = unsafe { CStr::from_ptr((*dir_entry).d_name.as_ptr()) };

        // C (HAVE_OPENAT): openat(xDirfd(dir), entryName, O_DIRECTORY | O_PATH)
        let entry_fd = unsafe {
            libc::openat(
                libc::dirfd(dir),
                entry_name_c.as_ptr(),
                libc::O_DIRECTORY | O_PATH,
            )
        };
        if entry_fd < 0 {
            continue;
        }

        // C's `goto next` teardown (Compat_openatArgClose) is the `'next` block's exit.
        'next: {
            let entry_name = entry_name_c.to_string_lossy();

            let etype = if String_startsWith(&entry_name, "BAT") {
                EntryType::BAT
            } else if String_startsWith(&entry_name, "AC") {
                EntryType::AC
            } else {
                let mut buffer = [0u8; 32];
                let ret = Compat_readfileat(entry_fd, c"type", &mut buffer);
                if ret <= 0 {
                    break 'next;
                }
                // drop optional trailing newlines
                let typestr = String::from_utf8_lossy(&buffer[..ret as usize]);
                let typestr = typestr.trim_end_matches('\n');

                if String_eq(typestr, "Battery") {
                    EntryType::BAT
                } else if String_eq(typestr, "Mains") {
                    EntryType::AC
                } else {
                    break 'next;
                }
            };

            if etype == EntryType::BAT {
                let mut buffer = [0u8; 1024];
                let r = Compat_readfileat(entry_fd, c"uevent", &mut buffer);
                if r < 0 {
                    break 'next;
                }

                let mut full = false;
                let mut now = false;

                let mut full_charge: f64 = 0.0;
                let mut capacity_level: f64 = f64::NAN;

                let content = String::from_utf8_lossy(&buffer[..r as usize]).into_owned();
                for line in content.split('\n') {
                    // sscanf(line, "POWER_SUPPLY_%99[^=]=%d", field, &val)
                    let rest = match line.strip_prefix("POWER_SUPPLY_") {
                        Some(r) => r,
                        None => continue,
                    };
                    let eq = match rest.find('=') {
                        Some(e) if e > 0 => e,
                        _ => continue, // %99[^=] needs ≥1 char before '='
                    };
                    let field = &rest[..eq.min(99)];
                    let val = match scan_c_int(&rest[eq + 1..]) {
                        Some(v) => v,
                        None => continue,
                    };

                    if String_eq(field, "CAPACITY") {
                        capacity_level = val as f64 / 100.0;
                        continue;
                    }

                    if String_eq(field, "ENERGY_FULL") || String_eq(field, "CHARGE_FULL") {
                        full_charge = val as f64;
                        total_full += full_charge as u64;
                        full = true;
                        if now {
                            break;
                        }
                        continue;
                    }

                    if String_eq(field, "ENERGY_NOW") || String_eq(field, "CHARGE_NOW") {
                        total_remain += val as u64;
                        now = true;
                        if full {
                            break;
                        }
                        continue;
                    }
                }

                // isNonnegative(capacityLevel): false for NAN.
                if !now && full && capacity_level >= 0.0 {
                    total_remain += (capacity_level * full_charge) as u64;
                }
            } else {
                // EntryType::AC
                if *isOnAC != ACPresence::AC_ERROR {
                    break 'next;
                }

                let mut buffer = [0u8; 2];
                let r = Compat_readfileat(entry_fd, c"online", &mut buffer);
                if r < 1 {
                    *isOnAC = ACPresence::AC_ERROR;
                    break 'next;
                }

                if buffer[0] == b'0' {
                    *isOnAC = ACPresence::AC_ABSENT;
                } else if buffer[0] == b'1' {
                    *isOnAC = ACPresence::AC_PRESENT;
                }
            }
        }

        Compat_openatArgClose(entry_fd);
    }

    unsafe {
        libc::closedir(dir);
    }

    *percent = if total_full > 0 {
        (total_remain as f64 * 100.0) / total_full as f64
    } else {
        f64::NAN
    };
}

/// Global battery cache backing [`Platform_getBattery`] — the four file-static
/// C variables (`Platform_Battery_method`, `_cacheTime`, `_cachePercent`,
/// `_cacheIsOnAC`) modeled as one [`Mutex`]-guarded record per the
/// global-mutable-static idiom (rule 4). `method` mirrors the C anonymous
/// `enum { BAT_PROC, BAT_SYS, BAT_ERR }`.
// C names preserved verbatim per port rules (C's anonymous enum `{ BAT_PROC,
// BAT_SYS, BAT_ERR }` and the file-static battery cache record).
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
enum Platform_Battery_method_t {
    BAT_PROC,
    BAT_SYS,
    BAT_ERR,
}

#[allow(non_camel_case_types)]
struct Platform_Battery_cache_t {
    method: Platform_Battery_method_t,
    cacheTime: libc::time_t,
    cachePercent: f64,
    cacheIsOnAC: ACPresence,
}

static PLATFORM_BATTERY: Mutex<Platform_Battery_cache_t> = Mutex::new(Platform_Battery_cache_t {
    method: Platform_Battery_method_t::BAT_PROC, // C: static ... = BAT_PROC
    cacheTime: 0,
    cachePercent: f64::NAN,             // C: = NAN
    cacheIsOnAC: ACPresence::AC_ABSENT, // C: zero-initialized static (AC_ABSENT == 0)
});

/// Port of `void Platform_getBattery(double* percent, ACPresence* isOnAC)`
/// from `Platform.c:964`. Serves the cached reading for 10 seconds, else
/// refreshes it: try the `/proc` method, falling back to `/sys`, then giving
/// up (`AC_ERROR`/`NAN`). A successful reading is clamped to `0..=100`.
pub fn Platform_getBattery(percent: &mut f64, isOnAC: &mut ACPresence) {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut cache = PLATFORM_BATTERY.lock().unwrap();

    // update battery reading is slow. Update it each 10 seconds only.
    if now < cache.cacheTime + 10 {
        *percent = cache.cachePercent;
        *isOnAC = cache.cacheIsOnAC;
        return;
    }

    if matches!(cache.method, Platform_Battery_method_t::BAT_PROC) {
        Platform_Battery_getProcData(percent, isOnAC);
        if !(*percent >= 0.0) {
            cache.method = Platform_Battery_method_t::BAT_SYS;
        }
    }
    if matches!(cache.method, Platform_Battery_method_t::BAT_SYS) {
        Platform_Battery_getSysData(percent, isOnAC);
        if !(*percent >= 0.0) {
            cache.method = Platform_Battery_method_t::BAT_ERR;
        }
    }
    if matches!(cache.method, Platform_Battery_method_t::BAT_ERR) {
        *percent = f64::NAN;
        *isOnAC = ACPresence::AC_ERROR;
    } else {
        // C CLAMP(*percent, 0.0, 100.0)
        *percent = percent.clamp(0.0, 100.0);
    }

    cache.cachePercent = *percent;
    cache.cacheIsOnAC = *isOnAC;
    cache.cacheTime = now;
}

/// Port of `void Platform_longOptionsUsage(const char* name)` from
/// `Platform.c:994`. On this build `HAVE_LIBCAP` is undefined, so the C body
/// is just `(void) name;` — a no-op. The `HAVE_LIBCAP` branch (which prints
/// the `--drop-capabilities` help text) is the mutually-exclusive
/// alternative build and is not ported (rule 3).
pub fn Platform_longOptionsUsage(_name: &str) {}

/// TODO: port of `CommandLineStatus Platform_getLongOption(int opt, int argc, char** argv` from `Platform.c:1008`.
pub fn Platform_getLongOption() {
    todo!("port of Platform.c:1008")
}

/// TODO: port of `static int dropCapabilities(enum CapMode mode` from `Platform.c:1044`.
pub fn dropCapabilities() {
    todo!("port of Platform.c:1044")
}

/// Port of `bool Platform_init(void)` from `Platform.c:1129`. Verifies
/// procfs is readable, then detects whether htop is running containerized:
/// first by comparing the `PROCDIR/self/ns/pid` namespace link against the
/// host init inode's magic string, then (if inconclusive) by scanning
/// `PROCDIR/1/mounts` for `lxcfs`/`overlay` markers. Sets
/// [`Running_containerized`] and returns whether init succeeded.
///
/// The `HAVE_LIBCAP` prelude (`dropCapabilities`) and the
/// `HAVE_SENSORS_SENSORS_H` `LibSensors_init()` call are `#if`-omitted on
/// this build, so — like the C preprocessor here — they are simply absent.
/// `access`/`readlink` are called via `libc` (the affinity-module
/// precedent for leaf syscalls); the mounts file is read with `std::fs`
/// (the C `fopen` returning `NULL` maps to the `Err` arm: skip the scan).
pub fn Platform_init() -> bool {
    let procdir = std::ffi::CString::new(PROCDIR).unwrap();
    if unsafe { libc::access(procdir.as_ptr(), libc::R_OK) } != 0 {
        eprintln!(
            "Error: could not read procfs (compiled to look in {}).",
            PROCDIR
        );
        return false;
    }

    let nspath = std::ffi::CString::new(format!("{}/self/ns/pid", PROCDIR)).unwrap();
    let mut target = [0u8; 4096];
    let ret = unsafe {
        libc::readlink(
            nspath.as_ptr(),
            target.as_mut_ptr() as *mut libc::c_char,
            target.len() - 1,
        )
    };
    if ret > 0 {
        // C: target[ret] = '\0'; — slice to the read length instead.
        let link = String::from_utf8_lossy(&target[..ret as usize]);
        // magic constant PROC_PID_INIT_INO from include/linux/proc_ns.h#L46
        if !String_eq("pid:[4026531836]", &link) {
            Running_containerized.store(true, Ordering::Relaxed);
            return true; // early return
        }
    }

    if let Ok(mounts) = std::fs::read_to_string(format!("{}/1/mounts", PROCDIR)) {
        for lineBuffer in mounts.lines() {
            // detect lxc or overlayfs and guess that this means we are running containerized
            if String_startsWith(lineBuffer, "lxcfs /proc")
                || String_startsWith(lineBuffer, "overlay / overlay")
            {
                Running_containerized.store(true, Ordering::Relaxed);
                break;
            }
        }
    }

    true
}

/// Port of `void Platform_done(void)` from `Platform.c:1171`. On this build
/// `HAVE_SENSORS_SENSORS_H` is undefined, so the sole statement
/// (`LibSensors_cleanup()`) is `#if`-omitted and the body is empty. This is
/// not a `free()`/`Drop` teardown — there is nothing to release — so the
/// faithful port of the non-sensors build is a genuine no-op.
pub fn Platform_done() {}

/// Port of `linux/Platform.h:140`. The Linux build has no dynamic columns,
/// so the `static inline` returns `NULL` — [`DynamicColumns_new`] then falls
/// back to `Hashtable_new(0, true)`.
///
/// [`DynamicColumns_new`]: crate::ported::dynamiccolumn::DynamicColumns_new
pub fn Platform_dynamicColumns() -> Option<crate::ported::hashtable::Hashtable> {
    None
}

/// Port of `linux/Platform.h:144`. `ATTR_UNUSED` no-op teardown for the
/// Linux build's (nonexistent) dynamic-column table.
pub fn Platform_dynamicColumnsDone(_table: &crate::ported::hashtable::Hashtable) {}

/// Port of `linux/Platform.h:146`. No dynamic columns on Linux, so the
/// `static inline` returns `NULL` for every key.
pub fn Platform_dynamicColumnName(_key: u32) -> Option<&'static str> {
    None
}

/// Port of `linux/Platform.h:150`. No dynamic columns on Linux, so the
/// `static inline` writes nothing and returns `false`.
pub fn Platform_dynamicColumnWriteField(
    _proc: &crate::ported::process::Process,
    _str: &mut crate::ported::richstring::RichString,
    _key: u32,
) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Platform_getProcessEnv` returns `None` (C `NULL`) when the target
    /// `PROCDIR/<pid>/environ` cannot be opened — here an impossible pid, so
    /// the result is deterministic on any host.
    #[test]
    fn getprocessenv_missing_pid_is_none() {
        assert!(Platform_getProcessEnv(2147483646).is_none());
    }

    /// On Linux the current process always has a readable `environ`, so the
    /// result is `Some` and ends with the two NUL terminators the C appends.
    #[cfg(target_os = "linux")]
    #[test]
    fn getprocessenv_self_has_double_nul_terminator() {
        let env = Platform_getProcessEnv(std::process::id() as libc::pid_t)
            .expect("self environ must be readable on Linux");
        assert!(env.ends_with("\0\0"));
    }

    /// `Platform_getPressureStall` sets all three averages to `NAN` when the
    /// pressure file is absent — a nonexistent name reproduces the C
    /// `!fp` branch on any host.
    #[test]
    fn getpressurestall_missing_file_is_nan() {
        let (mut ten, mut sixty, mut threehundred) = (0.0, 0.0, 0.0);
        Platform_getPressureStall(
            "zzz_nonexistent_pressure_file_zzz",
            true,
            &mut ten,
            &mut sixty,
            &mut threehundred,
        );
        assert!(ten.is_nan() && sixty.is_nan() && threehundred.is_nan());
    }

    /// The no-op ports must not panic when invoked.
    #[test]
    fn noop_ports_do_not_panic() {
        Platform_longOptionsUsage("htop");
        Platform_done();
    }

    /// `Platform_getMaxPid` is always positive: the fallback `0x3FFFFF` on a
    /// host without `PROCDIR/sys/kernel/pid_max`, or the parsed value on Linux.
    #[test]
    fn getmaxpid_is_positive() {
        assert!(Platform_getMaxPid() > 0);
    }

    /// `Platform_getUptime` never returns a negative number, and returns `0`
    /// on a host lacking `PROCDIR/uptime`.
    #[test]
    fn getuptime_nonnegative() {
        assert!(Platform_getUptime() >= 0);
    }

    /// `Platform_getFileDescriptors` defaults `max` to at least `65536` (the C
    /// default on read failure) and always writes a value.
    #[test]
    fn getfiledescriptors_sets_max() {
        let (mut used, mut max) = (0.0, 0.0);
        Platform_getFileDescriptors(&mut used, &mut max);
        assert!(max > 0.0);
        let _ = used;
    }

    /// `Platform_getBattery` completes without panicking and yields a valid
    /// `ACPresence`; on a battery-less host it degrades to `AC_ERROR`/`NAN`.
    #[test]
    fn getbattery_does_not_panic() {
        let mut percent = 0.0;
        let mut is_on_ac = ACPresence::AC_ABSENT;
        Platform_getBattery(&mut percent, &mut is_on_ac);
        // Either a clamped percentage or NAN; simply ensure the call returned.
        assert!(percent.is_nan() || (0.0..=100.0).contains(&percent));
    }
}
