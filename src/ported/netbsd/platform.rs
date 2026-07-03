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
//! Also ported (with the `NetBSDMachine` downcast / local FFI structs):
//! - the `Platform_set*Values` meter setters (`Platform.c:257`/`290`/`300`) —
//!   `Meter::host` is a base `*const Machine`, downcast to `NetBSDMachine`.
//! - `Platform_getProcessEnv` (`Platform.c:306`) — `libkvm`
//!   (`kvm_openfiles`/`kvm_getproc2`/`kvm_getenvv2`) + `kinfo_proc2`.
//! - `Platform_getDiskIO` (`Platform.c:369`) — the local `io_sysctl` struct
//!   over `sysctl(HW_IOSTATS)`.
//!
//! Still `todo!()` and blocked on unported substrate:
//! - `Platform_getFileDescriptors` — the faithful body is a single call to
//!   `Generic_getFileDescriptors_sysctl` (`generic/fdstat_sysctl.c`), which is
//!   unported and out of scope for this module.
//! - `Platform_getProcessLocks` — returns `FileLocks_ProcessData*`, an
//!   unmodeled type; the C body returns `NULL` unconditionally but the return
//!   type cannot be named here.
//! - `Platform_getBattery` — needs NetBSD `proplib` (the opaque
//!   `prop_dictionary_*`/`prop_object_iterator_*` object model, ~15 FFI
//!   symbols) plus the `ENVSYS` ioctl; the runtime-correctness of that opaque
//!   FFI cannot be validated from this cross-compile-only target.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::ported::crt::CRT_fatalError;
use crate::ported::diskiometer::DiskIOData;
use crate::ported::meter::Meter;
use crate::ported::netbsd::netbsdmachine::NetBSDMachine;
use crate::ported::netbsd::netbsdprocesstable::kinfo_proc2;
use crate::ported::networkiometer::NetworkIOData;

// `VM_LOADAVG` (`uvm/uvm_param.h`) — the `CTL_VM` sysctl for load average.
// Absent from `libc`.
const VM_LOADAVG: c_int = 2;

// ── CPU meter value indices (`CPUMeter.h`), re-declared here (they are
// module-private `const`s in `cpumeter.rs`); data, not functions.
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

/// `SWAP_METER_USED = 0` (`SwapMeter.h`).
const SWAP_METER_USED: usize = 0;

// Memory class value indices (`Platform.c:153` enum); the chart order.
const MEMORY_CLASS_WIRED: usize = 0;
const MEMORY_CLASS_ACTIVE: usize = 1;
const MEMORY_CLASS_PAGED: usize = 2;
const MEMORY_CLASS_INACTIVE: usize = 3;

/// `HW_IOSTATS` (`sys/sysctl.h`) — the `CTL_HW` disk-IO statistics sysctl.
/// Absent from `libc`.
const HW_IOSTATS: c_int = 9;
/// `IOSTAT_DISK = 0` (`sys/iostat.h`) — the `io_sysctl.type` for a disk.
const IOSTAT_DISK: i32 = 0;
/// `#define IOSTATNAMELEN 16` (`sys/iostat.h`).
const IOSTATNAMELEN: usize = 16;
/// `KVM_NO_FILES` (`kvm.h`) — `((int)0x80000000)`.
const KVM_NO_FILES: c_int = 0x8000_0000u32 as c_int;
/// `_POSIX2_LINE_MAX` — the `kvm_openfiles` error-buffer size.
const POSIX2_LINE_MAX: usize = 2048;

extern "C" {
    /// `kvm_t* kvm_openfiles(const char*, const char*, const char*, int,
    /// char*)` (`kvm.h`). Not exposed by `libc`.
    fn kvm_openfiles(
        execfile: *const c_char,
        corefile: *const c_char,
        swapfile: *const c_char,
        flags: c_int,
        errbuf: *mut c_char,
    ) -> *mut c_void;
    /// `int kvm_close(kvm_t*)` (`kvm.h`).
    fn kvm_close(kd: *mut c_void) -> c_int;
    /// `struct kinfo_proc2* kvm_getproc2(kvm_t*, int, int, size_t, int*)`
    /// (`kvm.h`).
    fn kvm_getproc2(
        kd: *mut c_void,
        op: c_int,
        arg: c_int,
        elemsize: usize,
        cnt: *mut c_int,
    ) -> *const kinfo_proc2;
    /// `char** kvm_getenvv2(kvm_t*, const struct kinfo_proc2*, int)` (`kvm.h`).
    fn kvm_getenvv2(
        kd: *mut c_void,
        p: *const kinfo_proc2,
        limit: c_int,
    ) -> *const *const c_char;
}

/// Port of `struct io_sysctl` (`sys/iostat.h`) — the per-device IO statistics
/// entry filled by `sysctl(HW_IOSTATS)`. `libc` does not model it;
/// transcribed field-for-field so the `type`/`rbytes`/`wbytes`/`busysum_usec`
/// offsets are exact.
#[repr(C)]
#[derive(Clone, Copy)]
struct io_sysctl {
    name: [c_char; IOSTATNAMELEN],
    busy: i32,
    r#type: i32,
    xfer: u64,
    seek: u64,
    bytes: u64,
    attachtime_sec: u32,
    attachtime_usec: u32,
    timestamp_sec: u32,
    timestamp_usec: u32,
    time_sec: u32,
    time_usec: u32,
    rxfer: u64,
    rbytes: u64,
    wxfer: u64,
    wbytes: u64,
    wait_sec: u32,
    wait_usec: u32,
    waitsum_sec: u32,
    waitsum_usec: u32,
    busysum_sec: u32,
    busysum_usec: u32,
}

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

/// Port of `double Platform_setCPUValues(Meter* this, int cpu)` from
/// `Platform.c:257`. Reads `nhost->cpuData[cpu]` (via the `NetBSDMachine`
/// downcast of `this->host`) and fills the CPU meter's `values`, returning the
/// clamped total percentage.
pub fn Platform_setCPUValues(this: &mut Meter, cpu: c_int) -> f64 {
    let host = this.host;
    let nhost = host as *const NetBSDMachine;
    // SAFETY: `this.host` is the base of a live `NetBSDMachine` (the same
    // downcast htop performs); `cpuData[cpu]` is in range for the meter.
    // `CPUData` is `Copy`, so this reads the entry out by value.
    let cpuData = unsafe {
        let nh = &*nhost;
        nh.cpuData[cpu as usize]
    };
    let total = if cpuData.totalPeriod == 0 {
        1.0
    } else {
        cpuData.totalPeriod as f64
    };

    let detailed = unsafe {
        (*host)
            .settings
            .as_ref()
            .map(|s| s.detailedCPUTime)
            .unwrap_or(false)
    };

    let v = &mut this.values;
    v[CPU_METER_NICE] = cpuData.nicePeriod as f64 / total * 100.0;
    v[CPU_METER_NORMAL] = cpuData.userPeriod as f64 / total * 100.0;
    if detailed {
        v[CPU_METER_KERNEL] = cpuData.sysPeriod as f64 / total * 100.0;
        v[CPU_METER_IRQ] = cpuData.intrPeriod as f64 / total * 100.0;
        v[CPU_METER_SOFTIRQ] = 0.0;
        v[CPU_METER_STEAL] = 0.0;
        v[CPU_METER_GUEST] = 0.0;
        v[CPU_METER_IOWAIT] = 0.0;
        v[CPU_METER_FREQUENCY] = f64::NAN;
        this.curItems = 8;
    } else {
        v[CPU_METER_KERNEL] = cpuData.sysAllPeriod as f64 / total * 100.0;
        v[CPU_METER_IRQ] = 0.0; // No steal nor guest on NetBSD
        this.curItems = 4;
    }
    let mut totalPercent = this.values[CPU_METER_NICE]
        + this.values[CPU_METER_NORMAL]
        + this.values[CPU_METER_KERNEL]
        + this.values[CPU_METER_IRQ];
    totalPercent = totalPercent.clamp(0.0, 100.0);

    this.values[CPU_METER_FREQUENCY] = cpuData.frequency;
    this.values[CPU_METER_TEMPERATURE] = f64::NAN;

    totalPercent
}

/// Port of `void Platform_setMemoryValues(Meter* this)` from `Platform.c:290`.
/// Fills the four NetBSD memory-class values from the `NetBSDMachine`.
pub fn Platform_setMemoryValues(this: &mut Meter) {
    let host = this.host;
    let nhost = host as *const NetBSDMachine;
    // SAFETY: `this.host` is the base of a live `NetBSDMachine`.
    unsafe {
        this.total = (*host).totalMem as f64;
        this.values[MEMORY_CLASS_WIRED] = (*nhost).wiredMem as f64;
        this.values[MEMORY_CLASS_ACTIVE] = (*nhost).activeMem as f64;
        this.values[MEMORY_CLASS_PAGED] = (*nhost).pagedMem as f64;
        this.values[MEMORY_CLASS_INACTIVE] = (*nhost).inactiveMem as f64;
    }
}

/// Port of `void Platform_setSwapValues(Meter* this)` from `Platform.c:300`.
pub fn Platform_setSwapValues(this: &mut Meter) {
    let host = this.host;
    // SAFETY: `this.host` is a live `Machine`.
    unsafe {
        this.total = (*host).totalSwap as f64;
        this.values[SWAP_METER_USED] = (*host).usedSwap as f64;
    }
}

/// Port of `char* Platform_getProcessEnv(pid_t pid)` from `Platform.c:306`.
/// Opens a `kvm` handle, resolves the process, and joins its environment
/// vector (`kvm_getenvv2`) into a NUL-separated, double-NUL-terminated block,
/// returned as a `String`. `None` mirrors the C `NULL` failure paths.
pub fn Platform_getProcessEnv(pid: libc::pid_t) -> Option<String> {
    let mut errbuf = [0 as c_char; POSIX2_LINE_MAX];

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
        kvm_getproc2(
            kt,
            libc::KERN_PROC_PID,
            pid,
            size_of::<kinfo_proc2>(),
            &mut count,
        )
    };
    if kproc.is_null() {
        unsafe { kvm_close(kt) };
        return None;
    }

    let envv = unsafe { kvm_getenvv2(kt, kproc, 0) };
    if envv.is_null() {
        unsafe { kvm_close(kt) };
        return None;
    }

    // Accumulate each "VAR=VAL\0" run, matching the C's env+size fill.
    let mut env: Vec<u8> = Vec::with_capacity(4096);
    let mut i = 0isize;
    loop {
        let p = unsafe { *envv.offset(i) };
        if p.is_null() {
            break;
        }
        let bytes = unsafe { std::ffi::CStr::from_ptr(p) }.to_bytes();
        env.extend_from_slice(bytes);
        env.push(0);
        i += 1;
    }

    // Ensure the double-NUL terminator the caller expects.
    if env.len() < 2 || env[env.len() - 1] != 0 || env[env.len() - 2] != 0 {
        env.push(0);
        env.push(0);
    }

    unsafe { kvm_close(kt) };
    Some(String::from_utf8_lossy(&env).into_owned())
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

/// Port of `bool Platform_getDiskIO(DiskIOData* data)` from `Platform.c:369`.
/// Reads the per-device IO statistics via `sysctl(HW_IOSTATS)` (retrying on
/// `ENOMEM`) and sums the disk-type entries. A sizing/read failure is fatal,
/// as in the C.
pub fn Platform_getDiskIO(data: &mut DiskIOData) -> bool {
    let mib: [c_int; 3] = [libc::CTL_HW, HW_IOSTATS, size_of::<io_sysctl>() as c_int];
    let mut buf: Vec<u8> = Vec::new();
    let mut size: usize = 0;

    let mut last_errno = 0;
    for _retry in (1..=3).rev() {
        /* get the size of the IO statistic array */
        if unsafe {
            libc::sysctl(
                mib.as_ptr(),
                mib.len() as u32,
                ptr::null_mut(),
                &mut size,
                ptr::null(),
                0,
            )
        } < 0
        {
            CRT_fatalError("Unable to get size of io_sysctl");
        }

        if size == 0 {
            return false;
        }

        buf.resize(size, 0);

        let rc = unsafe {
            libc::sysctl(
                mib.as_ptr(),
                mib.len() as u32,
                buf.as_mut_ptr() as *mut c_void,
                &mut size,
                ptr::null(),
                0,
            )
        };
        if rc == 0 {
            last_errno = 0;
            break;
        }

        last_errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if last_errno != libc::ENOMEM {
            CRT_fatalError("Unable to get disk IO statistics");
        }
    }

    if last_errno == libc::ENOMEM {
        CRT_fatalError("Unable to get disk IO statistics");
    }

    let mut bytesReadSum: u64 = 0;
    let mut bytesWriteSum: u64 = 0;
    let mut busyTimeSum: u64 = 0;
    let mut numDisks: u64 = 0;

    let count = size / size_of::<io_sysctl>();
    let entries =
        unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const io_sysctl, count) };
    for io in entries {
        /* ignore NFS activity */
        if io.r#type != IOSTAT_DISK {
            continue;
        }
        bytesReadSum += io.rbytes;
        bytesWriteSum += io.wbytes;
        busyTimeSum += io.busysum_usec as u64;
        numDisks += 1;
    }

    data.totalBytesRead = bytesReadSum;
    data.totalBytesWritten = bytesWriteSum;
    data.totalMsTimeSpend = busyTimeSum / 1000;
    data.numDisks = numDisks;

    true
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

/// Port of `netbsd/Platform.h:108`. Non-PCP build: no dynamic meters, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicMeters() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `netbsd/Platform.h:112`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-meter table.
pub fn Platform_dynamicMetersDone(_table: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `netbsd/Platform.h:114`. `ATTR_UNUSED` no-op meter init.
pub fn Platform_dynamicMeterInit(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `netbsd/Platform.h:116`. `ATTR_UNUSED` no-op value update.
pub fn Platform_dynamicMeterUpdateValues(_meter: &mut crate::ported::meter::Meter) {}

/// Port of `netbsd/Platform.h:118`. `ATTR_UNUSED` no-op display.
pub fn Platform_dynamicMeterDisplay(
    _meter: &crate::ported::meter::Meter,
    _out: &mut crate::ported::richstring::RichString,
) {
}

/// Port of `netbsd/Platform.h:134`. Non-PCP build: no dynamic screens, so the
/// `static inline` returns `NULL`.
pub fn Platform_dynamicScreens() -> *mut crate::ported::hashtable::Hashtable {
    std::ptr::null_mut()
}

/// Port of `netbsd/Platform.h:144`. `ATTR_UNUSED` no-op teardown for the
/// non-PCP build's (nonexistent) dynamic-screen table.
pub fn Platform_dynamicScreensDone(_screens: *mut crate::ported::hashtable::Hashtable) {}

/// Port of `netbsd/Platform.h:142`. `ATTR_UNUSED` no-op — non-PCP builds add
/// no dynamic-screen columns.
pub fn Platform_addDynamicScreenAvailableColumns(
    _availableColumns: &mut crate::ported::panel::Panel,
    _screen: &str,
) {
}
