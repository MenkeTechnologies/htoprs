//! Port of `pcp/PCPMachine.c` + `.h` — the Performance Co-Pilot implementation
//! of htop's per-host `Machine`: the PMAPI sample cadence, the aggregate/per-CPU
//! time derivation, Linux/Darwin memory partition, and ZFS ARC / zswap scans.
//!
//! 1:1 faithful port; the C is the spec. The struct "extends" [`Machine`] via the
//! embedded `super_` first member (htop's `Machine super;`); the C
//! `(PCPMachine*)super` up/down-cast is a `&PCPMachine` / `&mut PCPMachine` here.
//!
//! # Array modelling
//!
//! The C `xCalloc`'d value buffers become owned `Vec`s:
//! - `cpu` — `Vec<pmAtomValue>` sized `CPU_METRIC_COUNT` (aggregate values,
//!   indexed by [`CPUMetric`]); allocated once in [`Machine_new`].
//! - `percpu` — `Vec<Vec<pmAtomValue>>`, one inner `Vec<pmAtomValue>` per CPU
//!   (each sized `CPU_METRIC_COUNT`); (re)allocated by
//!   `PCPMachine_updateCPUcount`.
//! - `values` — `Vec<pmAtomValue>` sized by the CPU count, a scratch buffer for
//!   one per-CPU metric fetch.
//!
//! [`pmAtomValue`] is a `#[repr(C)]` union, so every `.ull` / `.d` / `.cp` field
//! access is `unsafe` (as in the `Metric` layer). The CPU-time math treats the C
//! unsigned wraparound as `wrapping_add` / `wrapping_sub`.
//!
//! # `MemoryMetric` overlapping discriminants
//!
//! The C `enum MemoryMetric_` deliberately overlaps the Linux and Darwin value
//! classes onto the same `memValue[]` slots (`MEMORY_CLASS_USED == 0` and
//! `MEMORY_CLASS_WIRED == 0`, etc.). A Rust `enum` cannot carry duplicate
//! discriminants, so the classes are modelled as `usize` module consts with the
//! exact C values (a shared 6-slot index space).
//!
//! # Substrate gaps (forward references)
//!
//! `Platform_getMaxCPU` and `Platform_updateTables` are `pcp/Platform.c`
//! functions (not yet ported) that touch the deferred `pcp->ncpu` /
//! dynamic-table fields of [`Platform`](super::platform::Platform); they are
//! scaffolded as `todo!()` in [`platform`](super::platform) and imported here so
//! this port's call sites stay 1:1 until `Platform.c` lands. `pmtimevalToReal`
//! is part of the hand-declared PMAPI surface in [`pmapi`](super::pmapi).
//!
//! Confined to the `pcp` cargo feature; it will not link libpcp on macOS —
//! verified by `cargo check --features pcp` + primary-source reading + the
//! port-purity gate (the tier-3 model shared by the whole `pcp/` sub-tree).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::ffi::{c_void, CStr};
use std::os::raw::c_int;
use std::ptr;

use crate::ported::linux::linuxmachine::{memory_t, ZfsArcStats, ZswapStats};
use crate::ported::linux::linuxprocess::{
    PROCESS_FLAG_LINUX_AUTOGROUP, PROCESS_FLAG_LINUX_CGROUP, PROCESS_FLAG_LINUX_CTXT,
    PROCESS_FLAG_LINUX_OOM, PROCESS_FLAG_LINUX_SECATTR,
};
use crate::ported::machine::{Machine, Machine_done, Machine_init};
use crate::ported::pcp::metric::{
    Metric, Metric_enable, Metric_fetch, Metric_fromId, Metric_instance, Metric_instanceCount,
    Metric_values,
};
use crate::ported::pcp::platform::{Platform_getMaxCPU, Platform_updateTables};
use crate::ported::pcp::pmapi::{
    pmAtomValue, pmtimevalToReal, PM_TYPE_DOUBLE, PM_TYPE_STRING, PM_TYPE_U32, PM_TYPE_U64,
};
use crate::ported::xutils::String_eq;

use Metric::*;

/// `#define ONE_K 1024` (`Macros.h`).
const ONE_K: memory_t = 1024;

// ---------------------------------------------------------------------------
// `pcp/PCPMachine.h` enums
// ---------------------------------------------------------------------------

/// Port of `typedef enum CPUMetric_` (`PCPMachine.h:22`) — an index into the
/// aggregate `cpu` / per-CPU `percpu` buffers. Modelled as `usize` consts (not a
/// Rust `enum`) because the time/period pairing arithmetic
/// (`values[metric + CPU_TOTAL_PERIOD]`, `values[previous - CPU_TOTAL_PERIOD]`)
/// indexes the buffer directly. `CPU_METRIC_COUNT` is the buffer length.
pub type CPUMetric = usize;

pub const CPU_TOTAL_TIME: CPUMetric = 0;
pub const CPU_USER_TIME: CPUMetric = 1;
pub const CPU_SYSTEM_TIME: CPUMetric = 2;
pub const CPU_SYSTEM_ALL_TIME: CPUMetric = 3;
pub const CPU_IDLE_ALL_TIME: CPUMetric = 4;
pub const CPU_IDLE_TIME: CPUMetric = 5;
pub const CPU_NICE_TIME: CPUMetric = 6;
pub const CPU_IOWAIT_TIME: CPUMetric = 7;
pub const CPU_IRQ_TIME: CPUMetric = 8;
pub const CPU_SOFTIRQ_TIME: CPUMetric = 9;
pub const CPU_STEAL_TIME: CPUMetric = 10;
pub const CPU_GUEST_TIME: CPUMetric = 11;
pub const CPU_GUESTNICE_TIME: CPUMetric = 12;

pub const CPU_TOTAL_PERIOD: CPUMetric = 13;
pub const CPU_USER_PERIOD: CPUMetric = 14;
pub const CPU_SYSTEM_PERIOD: CPUMetric = 15;
pub const CPU_SYSTEM_ALL_PERIOD: CPUMetric = 16;
pub const CPU_IDLE_ALL_PERIOD: CPUMetric = 17;
pub const CPU_IDLE_PERIOD: CPUMetric = 18;
pub const CPU_NICE_PERIOD: CPUMetric = 19;
pub const CPU_IOWAIT_PERIOD: CPUMetric = 20;
pub const CPU_IRQ_PERIOD: CPUMetric = 21;
pub const CPU_SOFTIRQ_PERIOD: CPUMetric = 22;
pub const CPU_STEAL_PERIOD: CPUMetric = 23;
pub const CPU_GUEST_PERIOD: CPUMetric = 24;
pub const CPU_GUESTNICE_PERIOD: CPUMetric = 25;

pub const CPU_FREQUENCY: CPUMetric = 26;

pub const CPU_METRIC_COUNT: usize = 27;

/// Port of `typedef enum MemoryMetric_` (`PCPMachine.h:56`) — the shared 6-slot
/// `memValue[]` index space. The Linux and Darwin classes intentionally overlap
/// (e.g. `USED == WIRED == 0`), which a Rust `enum` cannot express, so they are
/// `usize` consts carrying the exact C discriminants.
pub const MEMORY_CLASS_USED: usize = 0; // Linux
pub const MEMORY_CLASS_SHARED: usize = 1;
pub const MEMORY_CLASS_BUFFERS: usize = 2;
pub const MEMORY_CLASS_CACHE: usize = 3;
pub const MEMORY_CLASS_COMPRESSED: usize = 4;
pub const MEMORY_CLASS_AVAILABLE: usize = 5;
pub const MEMORY_CLASS_WIRED: usize = 0; // Darwin
pub const MEMORY_CLASS_SPECULATIVE: usize = 1;
pub const MEMORY_CLASS_ACTIVE: usize = 2;
pub const MEMORY_CLASS_PURGEABLE: usize = 3;
pub const MEMORY_CLASS_INACTIVE: usize = 5;
pub const MEMORY_CLASS_LIMIT: usize = 6; // Maximum

/// Port of `typedef enum SystemName_` (`PCPMachine.h:74`) — which OS the PMDA is
/// sampling, selecting the memory-info variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemName {
    SYSTEM_NAME_LINUX,
    SYSTEM_NAME_DARWIN,
    SYSTEM_NAME_UNKNOWN,
}
use SystemName::*;

// ---------------------------------------------------------------------------
// `pcp/PCPMachine.h` struct
// ---------------------------------------------------------------------------

/// Port of `typedef struct PCPMachine_` (`PCPMachine.h:80`). Embeds the base
/// [`Machine`] as `super_` (C's `Machine super;` first member). `#[repr(C)]`
/// keeps `super_` at offset 0 so the C `(PCPMachine*)super` downcast is sound.
///
/// No `#[derive(Default)]`: [`pmAtomValue`] is a union (no `Default`), so the
/// `Vec` buffers are constructed explicitly in [`Machine_new`].
#[repr(C)]
pub struct PCPMachine {
    /// C `Machine super`.
    pub super_: Machine,
    /// C `SystemName sys`.
    pub sys: SystemName,
    /// C `int smaps_flag` — toggled to sample smaps on alternate passes.
    pub smaps_flag: c_int,
    /// C `double period` — elapsed hundredths-of-a-second since last sample.
    pub period: f64,
    /// C `double timestamp` — previous sample timestamp (seconds).
    pub timestamp: f64,
    /// C `memory_t memValue[MEMORY_CLASS_LIMIT]`.
    pub memValue: [memory_t; MEMORY_CLASS_LIMIT],
    /// C `pmAtomValue* cpu` — aggregate values for each [`CPUMetric`].
    pub cpu: Vec<pmAtomValue>,
    /// C `pmAtomValue** percpu` — per-processor values for each [`CPUMetric`].
    pub percpu: Vec<Vec<pmAtomValue>>,
    /// C `pmAtomValue* values` — per-processor buffer for just one metric.
    pub values: Vec<pmAtomValue>,
    /// C `ZfsArcStats zfs`.
    pub zfs: ZfsArcStats,
    /// C `ZswapStats zswap`.
    pub zswap: ZswapStats,
}

// A zero-initialized `pmAtomValue` (the C `xCalloc` / `memset(...,0,...)` state)
// is written inline as `unsafe { core::mem::zeroed() }` at every call site — a
// Rust-original constructor helper is disallowed by the port-purity gate. An
// all-zero bit pattern is a valid `pmAtomValue` (every member is an integer /
// float / pointer).

// ---------------------------------------------------------------------------
// `pcp/PCPMachine.c` functions
// ---------------------------------------------------------------------------

/// Port of `static void PCPMachine_updateCPUcount(PCPMachine* this)`
/// (`PCPMachine.c:32`). Refreshes `activeCPUs` from the per-CPU instance count
/// and, when the existing-CPU count changed, (re)allocates the `percpu` /
/// `values` buffers.
fn PCPMachine_updateCPUcount(this: &mut PCPMachine) {
    this.super_.activeCPUs = Metric_instanceCount(PCP_PERCPU_SYSTEM) as u32;
    let mut cpus = Platform_getMaxCPU();
    if cpus == this.super_.existingCPUs {
        return;
    }
    if cpus == 0 {
        cpus = this.super_.activeCPUs;
    }
    if cpus <= 1 {
        this.super_.activeCPUs = 1;
        cpus = 1;
    }
    this.super_.existingCPUs = cpus;

    // free(this->percpu); free(this->values); + xCalloc — modelled as Vec realloc.
    let cpus = cpus as usize;
    this.percpu = (0..cpus)
        .map(|_| vec![unsafe { core::mem::zeroed() }; CPU_METRIC_COUNT])
        .collect();
    this.values = vec![unsafe { core::mem::zeroed() }; cpus];
}

/// Port of `static void PCPMachine_updateSystemName(PCPMachine* this)`
/// (`PCPMachine.c:53`). Reads `kernel.uname.sysname` and selects the OS variant.
/// The C `char*` (`sysname.cp`, from `pmExtractValue`) is freed with `free`.
fn PCPMachine_updateSystemName(this: &mut PCPMachine) {
    let mut sysname = unsafe { core::mem::zeroed() };
    unsafe {
        if Metric_values(PCP_UNAME_SYSNAME, &mut sysname, 1, PM_TYPE_STRING).is_null() {
            sysname.cp = ptr::null_mut();
        } else {
            let s = CStr::from_ptr(sysname.cp).to_string_lossy();
            if String_eq(&s, "Linux") {
                this.sys = SYSTEM_NAME_LINUX;
            } else if String_eq(&s, "Darwin") {
                this.sys = SYSTEM_NAME_DARWIN;
            }
        }
        libc::free(sysname.cp as *mut c_void); // free(NULL) is a no-op
    }
}

/// Port of `static void PCPMachine_updateLinuxMemoryInfo(PCPMachine* this)`
/// (`PCPMachine.c:64`). Computes the procps-style Linux memory partition from
/// the `mem.util.*` / `swap.*` metrics. Unsigned arithmetic wraps (C semantics).
// The swapfree/swaptotal `else if` chains fetch a *fallback* metric
// (`PCP_SWAP_FREE` / `PCP_SWAP_LENGTH`) into the same target — the identical
// bodies are the faithful C, not a copy-paste bug.
#[allow(clippy::if_same_then_else)]
fn PCPMachine_updateLinuxMemoryInfo(this: &mut PCPMachine) {
    let mut freeMem: memory_t = 0;
    let mut swapFreeMem: memory_t = 0;
    let mut sreclaimableMem: memory_t = 0;

    let mut value = unsafe { core::mem::zeroed() };
    unsafe {
        if !Metric_values(PCP_MEM_FREE, &mut value, 1, PM_TYPE_U64).is_null() {
            freeMem = value.ull;
        }
        if !Metric_values(PCP_MEM_BUFFERS, &mut value, 1, PM_TYPE_U64).is_null() {
            this.memValue[MEMORY_CLASS_BUFFERS] = value.ull;
        }
        if !Metric_values(PCP_MEM_SRECLAIM, &mut value, 1, PM_TYPE_U64).is_null() {
            sreclaimableMem = value.ull;
        }
        if !Metric_values(PCP_MEM_SHARED, &mut value, 1, PM_TYPE_U64).is_null() {
            this.memValue[MEMORY_CLASS_SHARED] = value.ull;
        }
        if !Metric_values(PCP_MEM_CACHED, &mut value, 1, PM_TYPE_U64).is_null() {
            this.memValue[MEMORY_CLASS_CACHE] = value
                .ull
                .wrapping_add(sreclaimableMem)
                .wrapping_sub(this.memValue[MEMORY_CLASS_SHARED]);
        }
        let usedDiff = freeMem
            .wrapping_add(this.memValue[MEMORY_CLASS_CACHE])
            .wrapping_add(sreclaimableMem)
            .wrapping_add(this.memValue[MEMORY_CLASS_BUFFERS]);
        this.memValue[MEMORY_CLASS_USED] = if this.super_.totalMem >= usedDiff {
            this.super_.totalMem.wrapping_sub(usedDiff)
        } else {
            this.super_.totalMem.wrapping_sub(freeMem)
        };
        if !Metric_values(PCP_MEM_AVAILABLE, &mut value, 1, PM_TYPE_U64).is_null() {
            this.memValue[MEMORY_CLASS_AVAILABLE] = value.ull.min(this.super_.totalMem);
        } else {
            this.memValue[MEMORY_CLASS_AVAILABLE] = freeMem;
        }
        if !Metric_values(PCP_MEM_SWAPFREE, &mut value, 1, PM_TYPE_U64).is_null() {
            swapFreeMem = value.ull;
        } else if !Metric_values(PCP_SWAP_FREE, &mut value, 1, PM_TYPE_U64).is_null() {
            swapFreeMem = value.ull;
        }
        if !Metric_values(PCP_MEM_SWAPTOTAL, &mut value, 1, PM_TYPE_U64).is_null() {
            this.super_.totalSwap = value.ull;
        } else if !Metric_values(PCP_SWAP_LENGTH, &mut value, 1, PM_TYPE_U64).is_null() {
            this.super_.totalSwap = value.ull;
        }
        if !Metric_values(PCP_MEM_SWAPCACHED, &mut value, 1, PM_TYPE_U64).is_null() {
            this.super_.cachedSwap = value.ull;
        }
        this.super_.usedSwap = this
            .super_
            .totalSwap
            .wrapping_sub(swapFreeMem)
            .wrapping_sub(this.super_.cachedSwap);
    }
}

/// Port of `static void PCPMachine_updateDarwinMemoryInfo(PCPMachine* this,
/// Settings* settings)` (`PCPMachine.c:101`). The C reads `settings->
/// showCachedMemory`; the port takes that single bool (the base `Machine`'s
/// `settings` is already borrowed by `&mut this`), a faithful narrowing.
fn PCPMachine_updateDarwinMemoryInfo(this: &mut PCPMachine, showCachedMemory: bool) {
    let mut freeSwap: memory_t = 0;
    let mut activeMem: memory_t = 0;
    let mut externalMem: memory_t = 0;
    let mut purgeableMem: memory_t = 0;
    let mut speculativeMem: memory_t = 0;

    let mut value = unsafe { core::mem::zeroed() };
    unsafe {
        if !Metric_values(PCP_MEM_WIRED, &mut value, 1, PM_TYPE_U64).is_null() {
            this.memValue[MEMORY_CLASS_WIRED] = value.ull;
        }
        if !Metric_values(PCP_MEM_ACTIVE, &mut value, 1, PM_TYPE_U64).is_null() {
            activeMem = value.ull;
        }
        if !Metric_values(PCP_MEM_EXTERNAL, &mut value, 1, PM_TYPE_U64).is_null() {
            externalMem = value.ull;
        }
        if !Metric_values(PCP_MEM_PURGEABLE, &mut value, 1, PM_TYPE_U64).is_null() {
            purgeableMem = value.ull;
        }
        if !Metric_values(PCP_MEM_SPECULATIVE, &mut value, 1, PM_TYPE_U64).is_null() {
            speculativeMem = value.ull;
        }

        if showCachedMemory {
            this.memValue[MEMORY_CLASS_SPECULATIVE] = speculativeMem;
            this.memValue[MEMORY_CLASS_ACTIVE] = activeMem
                .wrapping_sub(purgeableMem)
                .wrapping_sub(externalMem);
            this.memValue[MEMORY_CLASS_PURGEABLE] = purgeableMem;
        } else {
            this.memValue[MEMORY_CLASS_SPECULATIVE] = 0;
            this.memValue[MEMORY_CLASS_ACTIVE] = speculativeMem
                .wrapping_add(activeMem)
                .wrapping_sub(externalMem);
            this.memValue[MEMORY_CLASS_PURGEABLE] = 0;
        }

        if !Metric_values(PCP_MEM_COMPRESSED, &mut value, 1, PM_TYPE_U64).is_null() {
            this.memValue[MEMORY_CLASS_COMPRESSED] = value.ull;
        }
        if !Metric_values(PCP_MEM_INACTIVE, &mut value, 1, PM_TYPE_U64).is_null() {
            this.memValue[MEMORY_CLASS_INACTIVE] = value.ull;
        }

        if !Metric_values(PCP_SWAP_FREE, &mut value, 1, PM_TYPE_U64).is_null() {
            freeSwap = value.ull;
        }
        if !Metric_values(PCP_SWAP_LENGTH, &mut value, 1, PM_TYPE_U64).is_null() {
            this.super_.totalSwap = value.ull;
        }
        this.super_.usedSwap = this.super_.totalSwap.wrapping_sub(freeSwap);
    }
}

/// Port of `static void PCPMachine_updateMemoryInfo(Machine* super)`
/// (`PCPMachine.c:143`). Reads `mem.physmem`, zeroes `memValue[]`, then
/// dispatches to the Linux/Darwin variant.
fn PCPMachine_updateMemoryInfo(this: &mut PCPMachine) {
    let mut value = unsafe { core::mem::zeroed() };
    unsafe {
        if !Metric_values(PCP_MEM_TOTAL, &mut value, 1, PM_TYPE_U64).is_null() {
            this.super_.totalMem = value.ull;
        } else {
            this.super_.totalMem = 0;
        }
    }

    this.memValue = [0; MEMORY_CLASS_LIMIT]; // memset(this->memValue, 0, ...)
    if this.sys == SYSTEM_NAME_DARWIN {
        // C: PCPMachine_updateDarwinMemoryInfo(this, super->settings);
        let showCachedMemory = this
            .super_
            .settings
            .as_ref()
            .expect("PCPMachine_updateMemoryInfo: super->settings (C dereferences it)")
            .showCachedMemory;
        PCPMachine_updateDarwinMemoryInfo(this, showCachedMemory);
    } else if this.sys == SYSTEM_NAME_LINUX {
        PCPMachine_updateLinuxMemoryInfo(this);
    }
}

/// Port of `static inline void PCPMachine_backupCPUTime(pmAtomValue* values)`
/// (`PCPMachine.c:160`). Copies the TIME fields into their mirrored PERIOD
/// slots so the next derivation can diff against the previous sample.
fn PCPMachine_backupCPUTime(values: &mut [pmAtomValue]) {
    for metric in CPU_TOTAL_TIME..CPU_TOTAL_PERIOD {
        values[metric + CPU_TOTAL_PERIOD] = values[metric];
    }
}

/// Port of `static inline void PCPMachine_saveCPUTimePeriod(pmAtomValue* values,
/// CPUMetric previous, pmAtomValue* latest)` (`PCPMachine.c:167`). Computes the
/// new PERIOD delta (`latest - old`, clamped at 0) and writes `latest` back into
/// the TIME slot. `latest` is an index into `values` (`previous -
/// CPU_TOTAL_PERIOD` at every call site).
fn PCPMachine_saveCPUTimePeriod(
    values: &mut [pmAtomValue],
    previous: CPUMetric,
    latest: CPUMetric,
) {
    unsafe {
        let latest_ull = values[latest].ull;

        // new value for period
        let prev_ull = values[previous].ull;
        values[previous].ull = if latest_ull > prev_ull {
            latest_ull - prev_ull
        } else {
            0
        };

        // new value for time
        values[previous - CPU_TOTAL_PERIOD].ull = latest_ull;
    }
}

/// Port of `static void PCPMachine_deriveCPUTime(pmAtomValue* values)`
/// (`PCPMachine.c:183`). Reconstructs the htop CPU-time buckets (guest split out
/// of user/nice, the idle-all / system-all / virt-all aggregates, the grand
/// total) from the raw sampled counters, then records each PERIOD delta. Every
/// step mirrors the C pointer aliasing (`virtalltime` aliases `CPU_GUEST_TIME`);
/// unsigned arithmetic wraps.
fn PCPMachine_deriveCPUTime(values: &mut [pmAtomValue]) {
    unsafe {
        // usertime -= guesttime
        values[CPU_USER_TIME].ull = values[CPU_USER_TIME]
            .ull
            .wrapping_sub(values[CPU_GUEST_TIME].ull);
        // nicetime -= guestnicetime
        values[CPU_NICE_TIME].ull = values[CPU_NICE_TIME]
            .ull
            .wrapping_sub(values[CPU_GUESTNICE_TIME].ull);
        // idlealltime = idletime + iowaittime
        values[CPU_IDLE_ALL_TIME].ull = values[CPU_IDLE_TIME]
            .ull
            .wrapping_add(values[CPU_IOWAIT_TIME].ull);
        // systalltime = systemtime + irqtime + softirqtime
        values[CPU_SYSTEM_ALL_TIME].ull = values[CPU_SYSTEM_TIME]
            .ull
            .wrapping_add(values[CPU_IRQ_TIME].ull)
            .wrapping_add(values[CPU_SOFTIRQ_TIME].ull);
        // virtalltime = guesttime + guestnicetime  (writes CPU_GUEST_TIME slot)
        values[CPU_GUEST_TIME].ull = values[CPU_GUEST_TIME]
            .ull
            .wrapping_add(values[CPU_GUESTNICE_TIME].ull);
        // totaltime = usertime + nicetime + systalltime + idlealltime + steal + virtalltime
        values[CPU_TOTAL_TIME].ull = values[CPU_USER_TIME]
            .ull
            .wrapping_add(values[CPU_NICE_TIME].ull)
            .wrapping_add(values[CPU_SYSTEM_ALL_TIME].ull)
            .wrapping_add(values[CPU_IDLE_ALL_TIME].ull)
            .wrapping_add(values[CPU_STEAL_TIME].ull)
            .wrapping_add(values[CPU_GUEST_TIME].ull);
    }

    PCPMachine_saveCPUTimePeriod(values, CPU_USER_PERIOD, CPU_USER_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_NICE_PERIOD, CPU_NICE_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_SYSTEM_PERIOD, CPU_SYSTEM_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_SYSTEM_ALL_PERIOD, CPU_SYSTEM_ALL_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_IDLE_ALL_PERIOD, CPU_IDLE_ALL_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_IDLE_PERIOD, CPU_IDLE_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_IOWAIT_PERIOD, CPU_IOWAIT_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_IRQ_PERIOD, CPU_IRQ_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_SOFTIRQ_PERIOD, CPU_SOFTIRQ_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_STEAL_PERIOD, CPU_STEAL_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_GUEST_PERIOD, CPU_GUEST_TIME);
    PCPMachine_saveCPUTimePeriod(values, CPU_TOTAL_PERIOD, CPU_TOTAL_TIME);
}

/// Port of `static void PCPMachine_updateAllCPUTime(PCPMachine* this, Metric
/// metric, CPUMetric cpumetric)` (`PCPMachine.c:226`). Fetches one aggregate
/// metric into `cpu[cpumetric]`, zeroing it on a fetch miss.
fn PCPMachine_updateAllCPUTime(this: &mut PCPMachine, metric: Metric, cpumetric: CPUMetric) {
    let value = &mut this.cpu[cpumetric];
    if Metric_values(metric, value as *mut pmAtomValue, 1, PM_TYPE_U64).is_null() {
        *value = unsafe { core::mem::zeroed() };
    }
}

/// Port of `static void PCPMachine_updatePerCPUTime(PCPMachine* this, Metric
/// metric, CPUMetric cpumetric)` (`PCPMachine.c:233`). Fetches one per-CPU
/// integer metric into the scratch `values` buffer (zeroing on a miss), then
/// scatters it into each `percpu[i][cpumetric].ull`.
fn PCPMachine_updatePerCPUTime(this: &mut PCPMachine, metric: Metric, cpumetric: CPUMetric) {
    let cpus = this.super_.existingCPUs as usize;
    if Metric_values(metric, this.values.as_mut_ptr(), cpus as c_int, PM_TYPE_U64).is_null() {
        for v in this.values.iter_mut().take(cpus) {
            *v = unsafe { core::mem::zeroed() };
        }
    }
    for i in 0..cpus {
        unsafe {
            this.percpu[i][cpumetric].ull = this.values[i].ull;
        }
    }
}

/// Port of `static void PCPMachine_updatePerCPUReal(PCPMachine* this, Metric
/// metric, CPUMetric cpumetric)` (`PCPMachine.c:242`). As
/// [`PCPMachine_updatePerCPUTime`] but for a `PM_TYPE_DOUBLE` metric (CPU
/// frequency), scattering into the `.d` union field.
fn PCPMachine_updatePerCPUReal(this: &mut PCPMachine, metric: Metric, cpumetric: CPUMetric) {
    let cpus = this.super_.existingCPUs as usize;
    if Metric_values(
        metric,
        this.values.as_mut_ptr(),
        cpus as c_int,
        PM_TYPE_DOUBLE,
    )
    .is_null()
    {
        for v in this.values.iter_mut().take(cpus) {
            *v = unsafe { core::mem::zeroed() };
        }
    }
    for i in 0..cpus {
        unsafe {
            this.percpu[i][cpumetric].d = this.values[i].d;
        }
    }
}

/// Port of `static inline void PCPMachine_scanZswapInfo(PCPMachine* this)`
/// (`PCPMachine.c:251`). Resets and refills the zswap pool sizes.
fn PCPMachine_scanZswapInfo(this: &mut PCPMachine) {
    let mut value = unsafe { core::mem::zeroed() };
    this.zswap = ZswapStats::default(); // memset(&this->zswap, 0, ...)
    unsafe {
        if !Metric_values(PCP_MEM_ZSWAP, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zswap.usedZswapComp = value.ull;
        }
        if !Metric_values(PCP_MEM_ZSWAPPED, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zswap.usedZswapOrig = value.ull;
        }
    }
}

/// Port of `static inline void PCPMachine_scanZfsArcstats(PCPMachine* this)`
/// (`PCPMachine.c:261`). Resets and refills the ZFS ARC stats (all sizes scaled
/// to kB by `ONE_K`), deriving `other` / `enabled` / `isCompressed`.
fn PCPMachine_scanZfsArcstats(this: &mut PCPMachine) {
    let mut dbufSize: memory_t = 0;
    let mut dnodeSize: memory_t = 0;
    let mut bonusSize: memory_t = 0;
    let mut value = unsafe { core::mem::zeroed() };

    this.zfs = ZfsArcStats::default(); // memset(&this->zfs, 0, ...)
    unsafe {
        if !Metric_values(PCP_ZFS_ARC_ANON_SIZE, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zfs.anon = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_C_MIN, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zfs.min = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_C_MAX, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zfs.max = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_BONUS_SIZE, &mut value, 1, PM_TYPE_U64).is_null() {
            bonusSize = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_DBUF_SIZE, &mut value, 1, PM_TYPE_U64).is_null() {
            dbufSize = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_DNODE_SIZE, &mut value, 1, PM_TYPE_U64).is_null() {
            dnodeSize = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_COMPRESSED_SIZE, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zfs.compressed = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_UNCOMPRESSED_SIZE, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zfs.uncompressed = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_HDR_SIZE, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zfs.header = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_MFU_SIZE, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zfs.MFU = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_MRU_SIZE, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zfs.MRU = value.ull / ONE_K;
        }
        if !Metric_values(PCP_ZFS_ARC_SIZE, &mut value, 1, PM_TYPE_U64).is_null() {
            this.zfs.size = value.ull / ONE_K;
        }
    }

    this.zfs.other = (dbufSize + dnodeSize + bonusSize) / ONE_K;
    this.zfs.enabled = (this.zfs.size > 0) as i32;
    this.zfs.isCompressed = (this.zfs.compressed > 0) as i32;
}

/// Port of `static void PCPMachine_scan(PCPMachine* this)` (`PCPMachine.c:298`).
/// The per-tick pipeline: memory info, CPU-count refresh, aggregate CPU-time
/// derivation, per-CPU time derivation, optional CPU frequency, then the ZFS /
/// zswap scans.
fn PCPMachine_scan(this: &mut PCPMachine) {
    PCPMachine_updateMemoryInfo(this);
    PCPMachine_updateCPUcount(this);

    PCPMachine_backupCPUTime(&mut this.cpu);
    PCPMachine_updateAllCPUTime(this, PCP_CPU_USER, CPU_USER_TIME);
    PCPMachine_updateAllCPUTime(this, PCP_CPU_NICE, CPU_NICE_TIME);
    PCPMachine_updateAllCPUTime(this, PCP_CPU_SYSTEM, CPU_SYSTEM_TIME);
    PCPMachine_updateAllCPUTime(this, PCP_CPU_IDLE, CPU_IDLE_TIME);
    PCPMachine_updateAllCPUTime(this, PCP_CPU_IOWAIT, CPU_IOWAIT_TIME);
    PCPMachine_updateAllCPUTime(this, PCP_CPU_IRQ, CPU_IRQ_TIME);
    PCPMachine_updateAllCPUTime(this, PCP_CPU_SOFTIRQ, CPU_SOFTIRQ_TIME);
    PCPMachine_updateAllCPUTime(this, PCP_CPU_STEAL, CPU_STEAL_TIME);
    PCPMachine_updateAllCPUTime(this, PCP_CPU_GUEST, CPU_GUEST_TIME);
    PCPMachine_deriveCPUTime(&mut this.cpu);

    for i in 0..this.super_.existingCPUs as usize {
        PCPMachine_backupCPUTime(&mut this.percpu[i]);
    }
    PCPMachine_updatePerCPUTime(this, PCP_PERCPU_USER, CPU_USER_TIME);
    PCPMachine_updatePerCPUTime(this, PCP_PERCPU_NICE, CPU_NICE_TIME);
    PCPMachine_updatePerCPUTime(this, PCP_PERCPU_SYSTEM, CPU_SYSTEM_TIME);
    PCPMachine_updatePerCPUTime(this, PCP_PERCPU_IDLE, CPU_IDLE_TIME);
    PCPMachine_updatePerCPUTime(this, PCP_PERCPU_IOWAIT, CPU_IOWAIT_TIME);
    PCPMachine_updatePerCPUTime(this, PCP_PERCPU_IRQ, CPU_IRQ_TIME);
    PCPMachine_updatePerCPUTime(this, PCP_PERCPU_SOFTIRQ, CPU_SOFTIRQ_TIME);
    PCPMachine_updatePerCPUTime(this, PCP_PERCPU_STEAL, CPU_STEAL_TIME);
    PCPMachine_updatePerCPUTime(this, PCP_PERCPU_GUEST, CPU_GUEST_TIME);
    for i in 0..this.super_.existingCPUs as usize {
        PCPMachine_deriveCPUTime(&mut this.percpu[i]);
    }

    let showCPUFrequency = this
        .super_
        .settings
        .as_ref()
        .expect("PCPMachine_scan: super->settings (C dereferences it)")
        .showCPUFrequency;
    if showCPUFrequency {
        PCPMachine_updatePerCPUReal(this, PCP_HINV_CPUCLOCK, CPU_FREQUENCY);
    }

    PCPMachine_scanZfsArcstats(this);
    PCPMachine_scanZswapInfo(this);
}

/// Port of `void Machine_scan(Machine* super)` (`PCPMachine.c:337`). Enables the
/// per-tick metric set (adjusting the optional per-flag metrics), toggles the
/// alternate-pass smaps metrics, fetches a new PMAPI sample (bailing on
/// failure), updates the timestamp/period, then runs `PCPMachine_scan`.
pub fn Machine_scan(this: &mut PCPMachine) {
    let (flags, showCPUFrequency) = {
        let settings = this
            .super_
            .settings
            .as_ref()
            .expect("Machine_scan: super->settings (C dereferences it)");
        // settings->ss == settings->screens[ssIndex]; flags = ss->flags
        let flags = settings.screens[settings.ssIndex as usize].flags;
        (flags, settings.showCPUFrequency)
    };

    for metric in (PCP_PROC_PID as usize)..(PCP_METRIC_COUNT as usize) {
        Metric_enable(Metric_fromId(metric), true);
    }

    Metric_enable(PCP_HINV_CPUCLOCK, showCPUFrequency);
    Metric_enable(PCP_PROC_CGROUPS, (flags & PROCESS_FLAG_LINUX_CGROUP) != 0);
    Metric_enable(PCP_PROC_OOMSCORE, (flags & PROCESS_FLAG_LINUX_OOM) != 0);
    let ctxt = (flags & PROCESS_FLAG_LINUX_CTXT) != 0;
    Metric_enable(PCP_PROC_VCTXSW, ctxt);
    Metric_enable(PCP_PROC_NVCTXSW, ctxt);
    Metric_enable(PCP_PROC_LABELS, (flags & PROCESS_FLAG_LINUX_SECATTR) != 0);
    let autogroup = (flags & PROCESS_FLAG_LINUX_AUTOGROUP) != 0;
    Metric_enable(PCP_PROC_AUTOGROUP_ID, autogroup);
    Metric_enable(PCP_PROC_AUTOGROUP_NICE, autogroup);

    /* Sample smaps metrics on every second pass to improve performance */
    this.smaps_flag = (this.smaps_flag != 0) as c_int;
    let smaps = this.smaps_flag != 0;
    Metric_enable(PCP_PROC_SMAPS_PSS, smaps);
    Metric_enable(PCP_PROC_SMAPS_SWAP, smaps);
    Metric_enable(PCP_PROC_SMAPS_SWAPPSS, smaps);

    let mut timestamp: libc::timeval = unsafe { core::mem::zeroed() };
    if !Metric_fetch(Some(&mut timestamp)) {
        return;
    }

    let sample = this.timestamp;
    this.timestamp = unsafe { pmtimevalToReal(&timestamp) };
    this.period = (this.timestamp - sample) * 100.0;

    PCPMachine_scan(this);
}

/// Port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// (`PCPMachine.c:378`). Allocates the `PCPMachine` (C `xCalloc`, mirrored by
/// explicit zero-init), runs the base [`Machine_init`], seeds the previous
/// timestamp from `gettimeofday`, resolves the system name, allocates the
/// aggregate `cpu` buffer, sizes the per-CPU buffers, and updates the dynamic
/// tables. Returns the owning `Box<PCPMachine>` (C returns `&this->super`).
pub fn Machine_new(usersTable: Option<usize>, userId: u32) -> Box<PCPMachine> {
    let mut this = Box::new(PCPMachine {
        super_: Machine::default(),
        sys: SYSTEM_NAME_UNKNOWN,
        smaps_flag: 0,
        period: 0.0,
        timestamp: 0.0,
        memValue: [0; MEMORY_CLASS_LIMIT],
        cpu: Vec::new(),
        percpu: Vec::new(),
        values: Vec::new(),
        zfs: ZfsArcStats::default(),
        zswap: ZswapStats::default(),
    });

    Machine_init(&mut this.super_, usersTable, userId);

    let mut timestamp: libc::timeval = unsafe { core::mem::zeroed() };
    unsafe {
        libc::gettimeofday(&mut timestamp, ptr::null_mut());
    }
    this.timestamp = unsafe { pmtimevalToReal(&timestamp) };

    this.sys = SYSTEM_NAME_UNKNOWN;
    PCPMachine_updateSystemName(&mut this);

    this.cpu = vec![unsafe { core::mem::zeroed() }; CPU_METRIC_COUNT];
    PCPMachine_updateCPUcount(&mut this);

    Platform_updateTables(&mut this.super_);

    this
}

/// Port of `void Machine_delete(Machine* super)` (`PCPMachine.c:399`). Runs the
/// base [`Machine_done`], then drops the machine — the C `free`s of the `values`
/// / per-CPU `percpu` / `cpu` buffers and `free(this)` are the `Vec`/`Box`
/// destructors running when `this` falls out of scope.
pub fn Machine_delete(mut this: Box<PCPMachine>) {
    Machine_done(&mut this.super_);
}

/// Port of `bool Machine_isCPUonline(const Machine* host, unsigned int id)`
/// (`PCPMachine.c:410`). Online iff the per-CPU system metric has an instance
/// for `id`.
pub fn Machine_isCPUonline(host: &Machine, id: u32) -> bool {
    debug_assert!(id < host.existingCPUs);
    let mut value = unsafe { core::mem::zeroed() };
    !Metric_instance(
        PCP_PERCPU_SYSTEM,
        id as c_int,
        id as c_int,
        &mut value,
        PM_TYPE_U32,
    )
    .is_null()
}

/// Port of `int Machine_getCPUPhysicalCoreID(const Machine* host, unsigned int
/// id)` (`PCPMachine.c:420`). PCP exposes no topology, so the physical core id
/// is the CPU id itself.
pub fn Machine_getCPUPhysicalCoreID(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    id as i32
}

/// Port of `int Machine_getCPUThreadIndex(const Machine* host, unsigned int id)`
/// (`PCPMachine.c:426`). No SMT topology via PCP, so every CPU is thread 0.
pub fn Machine_getCPUThreadIndex(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    let _ = id;
    0
}
