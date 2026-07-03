//! Port of `solaris/Platform.c` — htop's Solaris/illumos platform hooks.
//!
//! Ported here:
//! - `Platform_init` (`Platform.c:148`)
//! - `Platform_done` (`Platform.c:153`)
//! - `Platform_setBindings` (`Platform.c:157`)
//! - `Platform_getUptime` (`Platform.c:162`)
//! - `Platform_getLoadAverage` (`Platform.c:178`)
//! - `Platform_getMaxPid` (`Platform.c:191`) — via the `unix:0:var` kstat.
//! - `Platform_setCPUValues` (`Platform.c:211`)
//! - `Platform_setMemoryValues` (`Platform.c:252`)
//! - `Platform_setSwapValues` (`Platform.c:260`)
//! - `Platform_buildenv` (`Platform.c:278`) + `Platform_getProcessEnv`
//!   (`Platform.c:300`) — via `libproc` (`Pgrab`/`Penv_iter`/`Prelease`).
//! - `Platform_getFileDescriptors` (`Platform.c:328`)
//! - `Platform_getDiskIO` (`Platform.c:333`)
//! - `Platform_getNetworkIO` (`Platform.c:339`)
//! - `Platform_getBattery` (`Platform.c:345`)
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `Platform_setZfsArcValues` / `Platform_setZfsCompressedArcValues` —
//!   need `ZfsArcMeter_readStats` / `ZfsCompressedArcMeter_readStats`
//!   (`zfs/ZfsArcMeter.c` / `zfs/ZfsCompressedArcMeter.c`), which are not yet
//!   ported and live outside `solaris/`.
//! - `Platform_getProcessLocks` — `FileLocks_ProcessData` is unmodeled
//!   (Solaris's body returns `NULL` unconditionally; same as the linux port).
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::ported::batterymeter::ACPresence;
use crate::ported::diskiometer::DiskIOData;
use crate::ported::meter::Meter;
use crate::ported::networkiometer::NetworkIOData;
use crate::ported::solaris::solarismachine::{
    kstat_close, kstat_lookup_wrapper, kstat_open, kstat_read, SolarisMachine,
};
use crate::ported::xutils::sumPositiveValues;

// `LOADAVG_1MIN` / `LOADAVG_5MIN` / `LOADAVG_15MIN` (`sys/loadavg.h`).
const LOADAVG_1MIN: usize = 0;
const LOADAVG_5MIN: usize = 1;
const LOADAVG_15MIN: usize = 2;

/// Port of `bool Platform_init(void)` (`Platform.c:148`).
pub fn Platform_init() -> bool {
    /* no platform-specific setup needed */
    true
}

/// Port of `void Platform_done(void)` (`Platform.c:153`).
pub fn Platform_done() {
    /* no platform-specific cleanup needed */
}

/// Port of `void Platform_setBindings(Htop_Action* keys)` (`Platform.c:157`).
pub fn Platform_setBindings(_keys: &mut [Option<crate::ported::action::Htop_Action>]) {
    /* no platform-specific key bindings */
}

/// Port of `int Platform_getUptime(void)` (`Platform.c:162`). Scans the
/// `utmpx` database for the "system boot" record and returns the seconds
/// since it.
pub fn Platform_getUptime() -> c_int {
    let mut boot_time: c_int = 0;
    let curr_time = unsafe { libc::time(ptr::null_mut()) } as c_int;

    loop {
        let ent = unsafe { libc::getutxent() };
        if ent.is_null() {
            break;
        }
        let entry = unsafe { &*ent };
        let line = unsafe { CStr::from_ptr(entry.ut_line.as_ptr()) };
        if line.to_bytes() == b"system boot" {
            boot_time = entry.ut_tv.tv_sec as c_int;
        }
    }

    unsafe { libc::endutxent() };

    curr_time - boot_time
}

/// Port of `void Platform_getLoadAverage(double* one, double* five, double*
/// fifteen)` (`Platform.c:178`).
pub fn Platform_getLoadAverage(one: &mut f64, five: &mut f64, fifteen: &mut f64) {
    let mut plat_loadavg = [0.0f64; 3];
    if unsafe { libc::getloadavg(plat_loadavg.as_mut_ptr(), 3) } < 0 {
        *one = f64::NAN;
        *five = f64::NAN;
        *fifteen = f64::NAN;
        return;
    }
    *one = plat_loadavg[LOADAVG_1MIN];
    *five = plat_loadavg[LOADAVG_5MIN];
    *fifteen = plat_loadavg[LOADAVG_15MIN];
}

/// Port of `typedef struct var kvar_t` (`Platform.h:45` / `<sys/var.h>`) —
/// the kernel tunables read by [`Platform_getMaxPid`] (`v_proc`, the
/// system-wide max process count). Modeled `#[repr(C)]` so `v_proc`'s offset
/// matches the `ks_data` layout kstat fills.
#[repr(C)]
struct kvar_t {
    v_buf: c_int,
    v_call: c_int,
    v_proc: c_int,
    v_maxupttl: c_int,
    v_nglobpris: c_int,
    v_maxsyspri: c_int,
    v_clist: c_int,
    v_maxup: c_int,
    v_hbuf: c_int,
    v_hmask: c_int,
    v_pbuf: c_int,
    v_sptmap: c_int,
    v_maxpmem: c_int,
    v_autoup: c_int,
    v_bufhwm: c_int,
}

/// Port of `pid_t Platform_getMaxPid(void)` (`Platform.c:191`). Opens a
/// throw-away kstat handle and reads `v_proc` from the `unix:0:var` kstat,
/// falling back to the Solaris default `32778` when unavailable.
pub fn Platform_getMaxPid() -> libc::pid_t {
    let mut vproc: c_int = 32778; // Reasonable Solaris default

    let kc = unsafe { kstat_open() };
    if !kc.is_null() {
        let kshandle = unsafe { kstat_lookup_wrapper(kc, "unix", 0, Some("var")) };
        if !kshandle.is_null() {
            unsafe { kstat_read(kc, kshandle, ptr::null_mut()) };

            let ksvar = unsafe { (*kshandle).ks_data } as *const kvar_t;
            if !ksvar.is_null() && unsafe { (*ksvar).v_proc } > 0 {
                vproc = unsafe { (*ksvar).v_proc };
            }
        }
        unsafe { kstat_close(kc) };
    }

    vproc as libc::pid_t
}

// Solaris's `CPU_METER_*` indices (`CPUMeter.h`) into `Meter::values`.
const CPU_METER_NICE: usize = 0;
const CPU_METER_NORMAL: usize = 1;
const CPU_METER_KERNEL: usize = 2;
const CPU_METER_IRQ: usize = 3;
const CPU_METER_FREQUENCY: usize = 8;
const CPU_METER_TEMPERATURE: usize = 9;

/// Port of `double Platform_setCPUValues(Meter* this, unsigned int cpu)` from
/// `Platform.c:211`. Downcasts `this->host` to the [`SolarisMachine`], reads
/// CPU `cpu`'s pre-computed percentages (index `0` for a single-CPU host,
/// else `cpu`), fills the nice/normal/kernel(+irq) values by
/// `detailedCPUTime`, and returns the clamped total usage. An offline CPU
/// yields `NaN` with `curItems = 0`.
///
/// Bridge: `this->host` is read through the `Meter::host` `*const Machine`
/// and cast to `*const SolarisMachine` (the C `(const SolarisMachine*)host`).
pub fn Platform_setCPUValues(this: &mut Meter, cpu: u32) -> f64 {
    let host = this.host;
    let shost = host as *const SolarisMachine;
    let cpus = unsafe { (*host).existingCPUs };

    let idx = if cpus == 1 { 0 } else { cpu as usize };
    // Borrow the Vec explicitly before indexing so it does not implicitly
    // autoref the raw-pointer deref (`&(*shost)`), which the compiler denies.
    let cpuData = unsafe {
        let cpus_arr = &(*shost).cpus;
        &cpus_arr[idx]
    };

    if !cpuData.online {
        this.curItems = 0;
        return f64::NAN;
    }

    let detailedCPUTime = unsafe { (*host).settings.as_ref().is_some_and(|s| s.detailedCPUTime) };

    this.values[CPU_METER_NICE] = cpuData.nicePercent;
    this.values[CPU_METER_NORMAL] = cpuData.userPercent;
    if detailedCPUTime {
        this.values[CPU_METER_KERNEL] = cpuData.systemPercent;
        this.values[CPU_METER_IRQ] = cpuData.irqPercent;
        this.curItems = 4;
    } else {
        this.values[CPU_METER_KERNEL] = cpuData.systemAllPercent;
        this.curItems = 3;
    }

    let mut percent = sumPositiveValues(&this.values[0..this.curItems as usize]);
    percent = percent.min(100.0);

    this.values[CPU_METER_FREQUENCY] = cpuData.frequency;
    this.values[CPU_METER_TEMPERATURE] = f64::NAN;

    percent
}

// Solaris's `MEMORY_CLASS_*` enum (`Platform.c:102`) into `Meter::values`.
const MEMORY_CLASS_USED: usize = 0;
const MEMORY_CLASS_LOCKED: usize = 1;

/// Port of `void Platform_setMemoryValues(Meter* this)` from `Platform.c:252`.
/// Sets the memory meter's total (host `totalMem`) plus the used and locked
/// values from the [`SolarisMachine`].
pub fn Platform_setMemoryValues(this: &mut Meter) {
    let host = this.host;
    let shost = host as *const SolarisMachine;
    this.total = unsafe { (*host).totalMem } as f64;
    this.values[MEMORY_CLASS_USED] = unsafe { (*shost).usedMem } as f64;
    this.values[MEMORY_CLASS_LOCKED] = unsafe { (*shost).lockedMem } as f64;
}

/// `SWAP_METER_USED = 0` (`SwapMeter.h`).
const SWAP_METER_USED: usize = 0;

/// Port of `void Platform_setSwapValues(Meter* this)` from `Platform.c:260`.
pub fn Platform_setSwapValues(this: &mut Meter) {
    let host = this.host;
    this.total = unsafe { (*host).totalSwap } as f64;
    this.values[SWAP_METER_USED] = unsafe { (*host).usedSwap } as f64;
}

/// TODO: port of `void Platform_setZfsArcValues(Meter* this)` from
/// `Platform.c:266`. Blocked: needs `ZfsArcMeter_readStats`
/// (`zfs/ZfsArcMeter.c`), which is not yet ported and lives outside
/// `solaris/`; the `SolarisMachine.zfs` source data is modeled.
pub fn Platform_setZfsArcValues() {
    todo!("port of Platform.c:266")
}

/// TODO: port of `void Platform_setZfsCompressedArcValues(Meter* this)` from
/// `Platform.c:272`. Blocked: needs `ZfsCompressedArcMeter_readStats`
/// (`zfs/ZfsCompressedArcMeter.c`), which is not yet ported and lives outside
/// `solaris/`; the `SolarisMachine.zfs` source data is modeled.
pub fn Platform_setZfsCompressedArcValues() {
    todo!("port of Platform.c:272")
}

/// Port of htop's `typedef struct envAccum_` (`Platform.h:47`) — the
/// growing environment-string buffer [`Platform_buildenv`] appends into.
#[repr(C)]
struct envAccum {
    capacity: usize,
    size: usize,
    bytes: usize,
    env: *mut c_char,
}

/// `#define PGRAB_RDONLY 0x04` (`<libproc.h>`).
const PGRAB_RDONLY: c_int = 0x04;

/// `typedef int proc_env_f(void*, struct ps_prochandle*, uintptr_t, const
/// char*)` (`<libproc.h>`) — the `Penv_iter` per-variable callback type.
type proc_env_f = extern "C" fn(*mut c_void, *mut c_void, usize, *const c_char) -> c_int;

#[link(name = "proc")]
extern "C" {
    // `struct ps_prochandle* Pgrab(pid_t pid, int gflag, int* perr)`.
    fn Pgrab(pid: libc::pid_t, gflag: c_int, perr: *mut c_int) -> *mut c_void;
    // `void Prelease(struct ps_prochandle* P, int flags)`.
    fn Prelease(P: *mut c_void, flags: c_int);
    // `int Penv_iter(struct ps_prochandle* P, proc_env_f* func, void* data)`.
    fn Penv_iter(P: *mut c_void, func: proc_env_f, data: *mut c_void) -> c_int;
}

/// Port of `static int Platform_buildenv(void* accum, struct ps_prochandle*
/// Phandle, uintptr_t addr, const char* str)` from `Platform.c:278`. The
/// `Penv_iter` callback: appends `str` (NUL-terminated, then a `'\n'`) into
/// the [`envAccum`] buffer, doubling its capacity when full, and returns `1`
/// (stop) only if the capacity would overflow.
pub extern "C" fn Platform_buildenv(
    accum: *mut c_void,
    _Phandle: *mut c_void,
    _addr: usize,
    str_: *const c_char,
) -> c_int {
    let accump = accum as *mut envAccum;
    let thissz = unsafe { libc::strlen(str_) };

    unsafe {
        while (thissz + 2) > ((*accump).capacity - (*accump).size) {
            if (*accump).capacity > (usize::MAX / 2) {
                return 1;
            }

            (*accump).capacity *= 2;
            (*accump).env =
                libc::realloc((*accump).env as *mut c_void, (*accump).capacity) as *mut c_char;
        }

        // strlcpy(env + size, str, capacity - size)
        let dst = (*accump).env.add((*accump).size);
        ptr::copy_nonoverlapping(str_, dst, thissz);
        *dst.add(thissz) = 0;

        // strncpy(env + size + thissz + 1, "\n", 2)
        let nl = (*accump).env.add((*accump).size + thissz + 1);
        *nl = b'\n' as c_char;
        *nl.add(1) = 0;

        (*accump).size += thissz + 1;
    }

    0
}

/// Port of `char* Platform_getProcessEnv(pid_t pid)` from `Platform.c:300`.
/// Grabs the target process read-only via `Pgrab` (the htop `pid / 1024`
/// LWP-id convention), iterates its environment with `Penv_iter` into an
/// [`envAccum`], releases the handle, and returns the assembled NUL/newline
/// block as an owned `String` (C returns the raw buffer). `None` when the
/// process cannot be grabbed.
pub fn Platform_getProcessEnv(pid: libc::pid_t) -> Option<String> {
    let realpid = pid / 1024;
    let mut graberr: c_int = 0;

    let Phandle = unsafe { Pgrab(realpid, PGRAB_RDONLY, &mut graberr) };
    if Phandle.is_null() {
        return None;
    }

    let mut envBuilder = envAccum {
        capacity: 4096,
        size: 0,
        bytes: 0,
        env: unsafe { libc::malloc(4096) } as *mut c_char,
    };

    unsafe {
        Penv_iter(
            Phandle,
            Platform_buildenv,
            &mut envBuilder as *mut envAccum as *mut c_void,
        );
    }

    unsafe { Prelease(Phandle, 0) };

    // strncpy(env + size, "\0", 1)
    unsafe { *envBuilder.env.add(envBuilder.size) = 0 };

    // C returns xRealloc(env, size + 1); materialize as an owned String and
    // release the C buffer.
    let bytes = unsafe { std::slice::from_raw_parts(envBuilder.env as *const u8, envBuilder.size) };
    let out = String::from_utf8_lossy(bytes).into_owned();
    unsafe { libc::free(envBuilder.env as *mut c_void) };

    Some(out)
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// from `Platform.c:323`. Blocked: `FileLocks_ProcessData` is unmodeled
/// (Solaris's body returns `NULL` unconditionally; same as the linux port).
pub fn Platform_getProcessLocks() {
    todo!("port of Platform.c:323")
}

/// Port of `void Platform_getFileDescriptors(double* used, double* max)`
/// (`Platform.c:328`). Solaris does not yet expose these, so both are `NaN`.
pub fn Platform_getFileDescriptors(used: &mut f64, max: &mut f64) {
    *used = f64::NAN;
    *max = f64::NAN;
}

/// Port of `bool Platform_getDiskIO(DiskIOData* data)` (`Platform.c:333`).
/// Not yet implemented on Solaris; returns `false` without touching `data`.
pub fn Platform_getDiskIO(_data: &mut DiskIOData) -> bool {
    // TODO
    false
}

/// Port of `bool Platform_getNetworkIO(NetworkIOData* data)`
/// (`Platform.c:339`). Not yet implemented on Solaris; returns `false`.
pub fn Platform_getNetworkIO(_data: &mut NetworkIOData) -> bool {
    // TODO
    false
}

/// Port of `void Platform_getBattery(double* percent, ACPresence* isOnAC)`
/// (`Platform.c:345`). Solaris has no battery probe, so `percent` is `NaN` and
/// the AC state is `AC_ERROR`.
pub fn Platform_getBattery(percent: &mut f64, isOnAC: &mut ACPresence) {
    *percent = f64::NAN;
    *isOnAC = ACPresence::AC_ERROR;
}

/// Port of `solaris/Platform.h:142`. Non-PCP build: no dynamic meters, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicMeters() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `solaris/Platform.h:146`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-meter table.
pub fn Platform_dynamicMetersDone(_table: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `solaris/Platform.h:148`. `ATTR_UNUSED` no-op meter init.
pub fn Platform_dynamicMeterInit(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `solaris/Platform.h:150`. `ATTR_UNUSED` no-op value update.
pub fn Platform_dynamicMeterUpdateValues(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `solaris/Platform.h:152`. `ATTR_UNUSED` no-op display.
pub fn Platform_dynamicMeterDisplay(
    _meter: &crate::ported::meter::Meter,
    _out: &mut crate::ported::richstring::RichString,
) {
}

/// Port of `solaris/Platform.h:168`. Non-PCP build: no dynamic screens, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicScreens() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `solaris/Platform.h:178`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-screen table.
pub fn Platform_dynamicScreensDone(_screens: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `solaris/Platform.h:176`. `ATTR_UNUSED` no-op — non-PCP builds add
/// no dynamic-screen columns.
pub fn Platform_addDynamicScreenAvailableColumns(
    _availableColumns: &mut crate::ported::panel::Panel,
    _screen: &str,
) {
}
